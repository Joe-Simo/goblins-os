//! The persistent Goblins AI runtime.
//!
//! The resident is an always-available OS process. It does two things at once:
//!
//! * a heartbeat that publishes live runtime state to the OS state directory so
//!   the core daemon and native surfaces can report whether the assistant is
//!   online; and
//! * a local IPC server on a Unix domain socket that answers `ping`, `status`,
//!   and `chat` requests with a newline-delimited JSON protocol.
//!
//! Conversation requests are forwarded through OS-owned model access so
//! that OpenAI credentials never live in, or pass through, the runtime as
//! client state — they stay server-side or in OS services only.

use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};

use goblins_os_ai::{action_registry, REGISTRY_VERSION};
use goblins_os_core_client::{
    initialize, ClientKind, CoreClient, Error as CoreClientError, Response,
};

const DEFAULT_STATE_DIR: &str = "/var/lib/goblins-os/resident";
const DEFAULT_MODEL_DIR: &str = "/var/lib/goblins-os/models";
const DEFAULT_SOCKET_PATH: &str = "/run/goblins-os/resident.sock";
const LOCAL_MODEL_RELAY_ENV: &str = "GOBLINS_OS_LOCAL_MODEL_RELAY";
const LOCAL_MODEL_RELAY_LEGACY_ENV: &str = "OPENAI_OS_LOCAL_MODEL_RELAY";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const MAX_REQUEST_BYTES: u64 = 64 * 1024;
const MAX_MESSAGE_CHARS: usize = 1000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let core = initialize(ClientKind::Resident)?;
    let state_dir = state_dir();
    fs::create_dir_all(&state_dir)?;

    println!(
        "Goblins AI runtime online; state={} socket={}",
        state_dir.display(),
        socket_path().display()
    );

    let heartbeat_dir = state_dir.clone();
    thread::Builder::new()
        .name("resident-heartbeat".to_string())
        .spawn(move || heartbeat_loop(&heartbeat_dir))?;

    serve_ipc(core)
}

fn heartbeat_loop(state_dir: &Path) -> ! {
    loop {
        let state = build_resident_state();
        if let Err(error) = write_resident_state(state_dir, &state) {
            eprintln!("resident_heartbeat_error={error}");
        }
        thread::sleep(HEARTBEAT_INTERVAL);
    }
}

#[derive(Serialize)]
struct ResidentState {
    generated_at: String,
    source: &'static str,
    pid: u32,
    mode: &'static str,
    engine: ResidentEngine,
    storage: ResidentStorage,
    ipc: ResidentIpc,
    capabilities: Vec<ResidentCapability>,
    ai: ResidentAiRegistry,
}

#[derive(Serialize)]
struct ResidentEngine {
    selected: &'static str,
    cloud_relay_configured: bool,
    local_relay_configured: bool,
    relay_contract: &'static str,
}

#[derive(Serialize)]
struct ResidentStorage {
    state_dir: String,
    model_dir: String,
    build_sandbox: Option<String>,
}

#[derive(Serialize)]
struct ResidentIpc {
    transport: &'static str,
    socket: String,
    core_transport: &'static str,
}

#[derive(Serialize)]
struct ResidentCapability {
    id: &'static str,
    label: &'static str,
    state: CapabilityState,
    detail: String,
}

#[derive(Serialize)]
struct ResidentAiRegistry {
    registry_version: &'static str,
    action_count: usize,
    permission_controls: Vec<&'static str>,
    contract: &'static str,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum CapabilityState {
    Ready,
    Waiting,
    PermissionGated,
}

fn build_resident_state() -> ResidentState {
    // Hosted readiness is authoritative only inside the protected core. The
    // resident never receives or probes gateway credentials.
    let cloud_relay = false;
    let local_relay = local_relay_configured();
    let build_sandbox = env::var("GOBLINS_OS_BUILD_SANDBOX").ok();
    let model_dir = env::var("GOBLINS_OS_MODEL_DIR").unwrap_or_else(|_| DEFAULT_MODEL_DIR.into());

    ResidentState {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-resident",
        pid: process::id(),
        mode: "persistent",
        engine: ResidentEngine {
            selected: selected_engine(cloud_relay, local_relay),
            cloud_relay_configured: cloud_relay,
            local_relay_configured: local_relay,
            relay_contract: "POST JSON {message:string} -> {text:string}; secrets remain server-side or OS-owned",
        },
        storage: ResidentStorage {
            state_dir: state_dir().display().to_string(),
            model_dir: model_dir.clone(),
            build_sandbox: build_sandbox.clone(),
        },
        ipc: ResidentIpc {
            transport: "unix-socket",
            socket: socket_path().display().to_string(),
            core_transport: "capability-scoped-unix-socket",
        },
        capabilities: vec![
            ResidentCapability {
                id: "conversation",
                label: "Conversation",
                state: if cloud_relay || local_relay {
                    CapabilityState::Ready
                } else {
                    CapabilityState::Waiting
                },
                detail: if cloud_relay {
                    "Cloud model access is configured for Goblins AI conversations."
                        .to_string()
                } else if local_relay {
                    "Local model access is configured for Goblins AI conversations.".to_string()
                } else {
                    "Waiting for cloud or local model access before Goblins AI can answer.".to_string()
                },
            },
            ResidentCapability {
                id: "app-builder",
                label: "Build Studio",
                state: if build_sandbox.is_some() && (cloud_relay || local_relay) {
                    CapabilityState::Ready
                } else {
                    CapabilityState::Waiting
                },
                detail: match (build_sandbox.as_deref(), cloud_relay || local_relay) {
                    (Some(_), true) => "Local app building is ready for Goblins AI."
                        .to_string(),
                    (Some(_), false) => {
                        "Local app building is ready; waiting for model access.".to_string()
                    }
                    (None, _) => {
                        "Waiting for local app building before app creation is ready.".to_string()
                    }
                },
            },
            ResidentCapability {
                id: "local-models",
                label: "Local gpt-oss models",
                state: if Path::new(&model_dir).exists() {
                    CapabilityState::Ready
                } else {
                    CapabilityState::Waiting
                },
                detail: format!(
                    "Local model directory is {model_dir}; model weights remain outside the immutable OS image."
                ),
            },
            ResidentCapability {
                id: "computer-use",
                label: "Computer Use",
                state: CapabilityState::PermissionGated,
                detail:
                    "OS automation requires explicit local permission before Goblins AI acts."
                        .to_string(),
            },
        ],
        ai: ResidentAiRegistry {
            registry_version: REGISTRY_VERSION,
            action_count: action_registry().len(),
            permission_controls: action_registry()
                .iter()
                .map(|action| action.permission.control_id())
                .collect(),
            contract:
                "Typed Goblins AI actions declare context, permission, confirmation, and entrypoint before execution.",
        },
    }
}

fn write_resident_state(state_dir: &Path, state: &ResidentState) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(state)?;
    write_state_file_atomic(&state_dir.join("resident.json"), &json)?;

    let summary = format!(
        "status={}\nmode=persistent\nengine={}\nsocket={}\n",
        if state.engine.cloud_relay_configured || state.engine.local_relay_configured {
            "ready"
        } else {
            "waiting-for-engine"
        },
        state.engine.selected,
        state.ipc.socket,
    );
    write_state_file_atomic(&state_dir.join("resident.state"), summary.as_bytes())
}

fn write_state_file_atomic(path: &Path, body: &[u8]) -> std::io::Result<()> {
    static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);

    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("resident state path has no parent"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let nonce = NEXT_WRITE.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{file_name}.{}-{nonce:016x}.tmp", process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o640)
            .open(&tmp)?;
        file.write_all(body)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        fs::File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

// ── IPC protocol ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct IpcRequest {
    op: String,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ResidentOp {
    Ping,
    Status,
    Chat(String),
}

#[derive(Debug, PartialEq, Eq)]
enum RequestError {
    Malformed,
    UnknownOp,
    MissingMessage,
    InvalidMessage,
}

impl RequestError {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Malformed => "request was not newline-delimited JSON",
            Self::UnknownOp => "unknown op; expected ping, status, or chat",
            Self::MissingMessage => "chat requires a message field",
            Self::InvalidMessage => "chat message must be between 1 and 1000 characters",
        }
    }
}

fn parse_request(line: &str) -> Result<ResidentOp, RequestError> {
    let request: IpcRequest =
        serde_json::from_str(line.trim()).map_err(|_| RequestError::Malformed)?;

    match request.op.as_str() {
        "ping" => Ok(ResidentOp::Ping),
        "status" => Ok(ResidentOp::Status),
        "chat" => {
            let message = request.message.ok_or(RequestError::MissingMessage)?;
            let trimmed = message.trim();
            if trimmed.is_empty() || trimmed.chars().count() > MAX_MESSAGE_CHARS {
                return Err(RequestError::InvalidMessage);
            }
            Ok(ResidentOp::Chat(trimmed.to_string()))
        }
        _ => Err(RequestError::UnknownOp),
    }
}

fn dispatch_with_relay(line: &str, relay: impl FnOnce(&str) -> Result<String, String>) -> String {
    match parse_request(line) {
        Ok(ResidentOp::Ping) => {
            serde_json::json!({ "ok": true, "status": "online", "pid": process::id() }).to_string()
        }
        Ok(ResidentOp::Status) => serde_json::to_string(&build_resident_state())
            .unwrap_or_else(|_| error_json("resident status could not be encoded")),
        Ok(ResidentOp::Chat(message)) => match relay(&message) {
            Ok(text) => serde_json::json!({ "text": text }).to_string(),
            Err(detail) => error_json(&detail),
        },
        Err(error) => error_json(error.as_str()),
    }
}

fn error_json(detail: &str) -> String {
    serde_json::json!({ "error": detail }).to_string()
}

#[cfg(unix)]
fn serve_ipc(core: CoreClient) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    let socket_path = socket_path();
    if let Some(parent) = socket_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::remove_file(&socket_path);

    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "resident_ipc_bind_error={error} socket={}; continuing heartbeat-only",
                socket_path.display()
            );
            loop {
                thread::sleep(HEARTBEAT_INTERVAL);
            }
        }
    };

    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    println!("resident_ipc_listening={}", socket_path.display());

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let core = core.clone();
                thread::spawn(move || handle_client(stream, core));
            }
            Err(error) => eprintln!("resident_ipc_accept_error={error}"),
        }
    }

    Ok(())
}

#[cfg(not(unix))]
fn serve_ipc(_core: CoreClient) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        thread::sleep(HEARTBEAT_INTERVAL);
    }
}

#[cfg(unix)]
fn handle_client(stream: std::os::unix::net::UnixStream, core: CoreClient) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let read_half = match stream.try_clone() {
        Ok(clone) => clone,
        Err(_) => return,
    };
    let mut reader = BufReader::new(read_half).take(MAX_REQUEST_BYTES);
    let mut writer = stream;

    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
        return;
    }

    let response = dispatch_with_relay(&line, |message| forward_chat_to_core(&core, message));
    let _ = writeln!(writer, "{response}");
    let _ = writer.flush();
}

// ── Capability-scoped core relay client ───────────────────────────────────

fn forward_chat_to_core(core: &CoreClient, message: &str) -> Result<String, String> {
    let body = serde_json::json!({ "message": message }).to_string();
    // Core's worst-case wall time behind /v1/codex/resident is its model read timeout
    // (GOBLINS_OS_RESIDENT_TIMEOUT_SECS, default 120s) plus its own connect+write budget.
    // Read the same clamped value core uses and add ~30s of headroom so the resident's
    // socket read never fires before core can legitimately answer on slow CPU-only hardware.
    let read_timeout = resident_core_read_timeout(
        env::var("GOBLINS_OS_RESIDENT_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok()),
    );
    let response = match core.post_json("/v1/codex/resident", body.as_bytes(), read_timeout) {
        Ok(response) => response,
        Err(CoreClientError::ConnectionBroken(_) | CoreClientError::RelaunchRequired) => {
            // The capability transport deliberately never reconnects after a
            // broken exchange. Exit so the service manager can start a fresh
            // resident with a newly authenticated one-time connection.
            eprintln!("resident_core_connection=relaunch-required");
            process::exit(75);
        }
        Err(_) => return Err("resident could not reach local OS services".to_string()),
    };
    decode_core_response(response)
}

fn resident_core_read_timeout(parsed: Option<u64>) -> Duration {
    Duration::from_secs(goblins_os_ai::resident_timeout::clamp_secs(parsed).saturating_add(30))
}

fn decode_core_response(response: Response) -> Result<String, String> {
    #[derive(Deserialize)]
    struct CoreText {
        text: String,
    }

    if !response.is_success() {
        // Core returns a `{ text: "..." }` body with actionable, credential-free guidance
        // on its error paths; surface it. Fall back to the generic line only when the body
        // is missing or unparsable so a body-less error still hides core internals.
        let detail = serde_json::from_slice::<CoreText>(&response.body)
            .map(|core| core.text)
            .unwrap_or_else(|_| {
                format!(
                    "Goblins OS returned HTTP {} without exposing credentials",
                    response.status
                )
            });
        return Err(detail);
    }

    let parsed: CoreText = serde_json::from_slice(&response.body)
        .map_err(|_| "Goblins OS returned an unreadable response".to_string())?;
    Ok(parsed.text)
}

fn state_dir() -> PathBuf {
    env::var_os("GOBLINS_OS_RESIDENT_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(DEFAULT_STATE_DIR).to_path_buf())
}

fn socket_path() -> PathBuf {
    env::var_os("GOBLINS_OS_RESIDENT_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(DEFAULT_SOCKET_PATH).to_path_buf())
}

fn selected_engine(cloud_relay: bool, local_relay: bool) -> &'static str {
    if local_relay {
        "local-gpt-oss"
    } else if cloud_relay {
        "cloud-openai"
    } else {
        "not-configured"
    }
}

fn local_relay_configured() -> bool {
    env_var_os_with_compat(LOCAL_MODEL_RELAY_ENV, LOCAL_MODEL_RELAY_LEGACY_ENV)
}

fn env_var_os_with_compat(primary: &str, legacy: &str) -> bool {
    env::var_os(primary).is_some() || env::var_os(legacy).is_some()
}

#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt, time::Duration};

    use goblins_os_core_client::{Response, MAX_READ_TIMEOUT};

    use super::{
        decode_core_response, dispatch_with_relay, parse_request, resident_core_read_timeout,
        selected_engine, write_state_file_atomic, CapabilityState, RequestError, ResidentOp,
    };

    #[test]
    fn local_engine_has_priority_when_available() {
        assert_eq!(selected_engine(true, true), "local-gpt-oss");
        assert_eq!(selected_engine(true, false), "cloud-openai");
        assert_eq!(selected_engine(false, false), "not-configured");
    }

    #[test]
    fn capability_states_are_stable_contract_values() {
        assert_eq!(
            serde_json::to_string(&CapabilityState::PermissionGated).unwrap(),
            "\"permission-gated\""
        );
    }

    #[test]
    fn resident_source_never_reads_hosted_service_secrets() {
        let source = include_str!("main.rs");
        let gateway_secret = ["AI_GATEWAY", "API_KEY"].join("_");
        let openai_secret = ["OPENAI", "API_KEY"].join("_");
        assert!(!source.contains(&gateway_secret));
        assert!(!source.contains(&openai_secret));
    }

    #[test]
    fn parses_supported_ipc_ops() {
        assert!(matches!(
            parse_request(r#"{"op":"ping"}"#),
            Ok(ResidentOp::Ping)
        ));
        assert!(matches!(
            parse_request(r#"{"op":"status"}"#),
            Ok(ResidentOp::Status)
        ));
        match parse_request(r#"{"op":"chat","message":"  hello  "}"#) {
            Ok(ResidentOp::Chat(message)) => assert_eq!(message, "hello"),
            other => panic!("expected trimmed chat op, got {:?}", other.is_ok()),
        }
    }

    #[test]
    fn rejects_invalid_ipc_requests() {
        assert_eq!(
            parse_request("not json").unwrap_err(),
            RequestError::Malformed
        );
        assert_eq!(
            parse_request(r#"{"op":"reboot"}"#).unwrap_err(),
            RequestError::UnknownOp
        );
        assert_eq!(
            parse_request(r#"{"op":"chat"}"#).unwrap_err(),
            RequestError::MissingMessage
        );
        assert_eq!(
            parse_request(r#"{"op":"chat","message":"   "}"#).unwrap_err(),
            RequestError::InvalidMessage
        );
        let too_long = format!(r#"{{"op":"chat","message":"{}"}}"#, "x".repeat(1001));
        assert_eq!(
            parse_request(&too_long).unwrap_err(),
            RequestError::InvalidMessage
        );
    }

    #[test]
    fn dispatch_serves_ping_and_status_without_network() {
        fn no_relay(_: &str) -> Result<String, String> {
            Err("relay must not be used".to_string())
        }
        let ping = dispatch_with_relay(r#"{"op":"ping"}"#, no_relay);
        assert!(ping.contains("\"ok\":true"));
        assert!(ping.contains("\"status\":\"online\""));

        let status = dispatch_with_relay(r#"{"op":"status"}"#, no_relay);
        assert!(status.contains("\"capabilities\""));
        assert!(status.contains("\"engine\""));
        assert!(status.contains("\"ai\""));
        assert!(status.contains("ai-native-os-actions"));

        let bad = dispatch_with_relay("garbage", no_relay);
        assert!(bad.contains("\"error\""));
    }

    #[test]
    fn decodes_core_response_contract() {
        let text = decode_core_response(Response {
            status: 200,
            headers: Vec::new(),
            body: br#"{"text":"hi"}"#.to_vec(),
        })
        .unwrap();
        assert_eq!(text, "hi");
    }

    #[test]
    fn maximum_model_timeout_fits_the_capability_transport_ceiling() {
        let timeout = resident_core_read_timeout(Some(u64::MAX));
        assert_eq!(timeout, Duration::from_secs(3_630));
        assert!(timeout <= MAX_READ_TIMEOUT);
    }

    #[test]
    fn resident_state_files_are_private_atomic_and_replaceable() {
        let directory = std::env::temp_dir().join(format!(
            "goblins-resident-state-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let path = directory.join("resident.state");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        write_state_file_atomic(&path, b"status=waiting\n").unwrap();
        write_state_file_atomic(&path, b"status=ready\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"status=ready\n");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert!(std::fs::read_dir(&directory).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));

        std::fs::remove_dir_all(directory).unwrap();
    }
}
