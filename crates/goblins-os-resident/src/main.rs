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
    net::TcpStream,
    path::{Path, PathBuf},
    process, thread,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};

use goblins_os_ai::{action_registry, REGISTRY_VERSION};

const DEFAULT_STATE_DIR: &str = "/var/lib/goblins-os/resident";
const DEFAULT_MODEL_DIR: &str = "/var/lib/goblins-os/models";
const DEFAULT_SOCKET_PATH: &str = "/run/goblins-os/resident.sock";
const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";
const LOCAL_MODEL_RELAY_ENV: &str = "GOBLINS_OS_LOCAL_MODEL_RELAY";
const LOCAL_MODEL_RELAY_LEGACY_ENV: &str = "OPENAI_OS_LOCAL_MODEL_RELAY";
const RESIDENT_RELAY_ENV: &str = "GOBLINS_OS_RESIDENT_RELAY_URL";
const RESIDENT_RELAY_LEGACY_ENV: &str = "OPENAI_OS_RESIDENT_RELAY_URL";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const MAX_REQUEST_BYTES: u64 = 64 * 1024;
const MAX_CORE_BODY_BYTES: u64 = 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 1000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    serve_ipc()
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
    core_url: String,
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
    let cloud_relay = cloud_relay_configured();
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
            core_url: core_url(),
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
    let tmp = state_dir.join("resident.json.tmp");
    let target = state_dir.join("resident.json");

    fs::write(&tmp, json)?;
    fs::rename(tmp, target)?;

    fs::write(
        state_dir.join("resident.state"),
        format!(
            "status={}\nmode=persistent\nengine={}\nsocket={}\n",
            if state.engine.cloud_relay_configured || state.engine.local_relay_configured {
                "ready"
            } else {
                "waiting-for-relay"
            },
            state.engine.selected,
            state.ipc.socket,
        ),
    )
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

fn dispatch(line: &str) -> String {
    match parse_request(line) {
        Ok(ResidentOp::Ping) => {
            serde_json::json!({ "ok": true, "status": "online", "pid": process::id() }).to_string()
        }
        Ok(ResidentOp::Status) => serde_json::to_string(&build_resident_state())
            .unwrap_or_else(|_| error_json("resident status could not be encoded")),
        Ok(ResidentOp::Chat(message)) => match forward_chat_to_core(&core_url(), &message) {
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
fn serve_ipc() -> Result<(), Box<dyn std::error::Error>> {
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
                thread::spawn(move || handle_client(stream));
            }
            Err(error) => eprintln!("resident_ipc_accept_error={error}"),
        }
    }

    Ok(())
}

#[cfg(not(unix))]
fn serve_ipc() -> Result<(), Box<dyn std::error::Error>> {
    loop {
        thread::sleep(HEARTBEAT_INTERVAL);
    }
}

#[cfg(unix)]
fn handle_client(stream: std::os::unix::net::UnixStream) {
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

    let response = dispatch(&line);
    let _ = writeln!(writer, "{response}");
    let _ = writer.flush();
}

// ── Core relay client (std-only HTTP/1.1, localhost) ──────────────────────

struct HttpEndpoint {
    host: String,
    port: u16,
}

fn forward_chat_to_core(core_url: &str, message: &str) -> Result<String, String> {
    let endpoint = parse_http_endpoint(core_url)
        .ok_or_else(|| "core url is not a local http endpoint".to_string())?;
    let body = serde_json::json!({ "message": message }).to_string();
    let head = format!(
        "POST /v1/codex/resident HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        endpoint.host,
        body.len()
    );

    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .map_err(|_| "resident could not reach local OS services".to_string())?;
    // Core's worst-case wall time behind /v1/codex/resident is its model read timeout
    // (GOBLINS_OS_RESIDENT_TIMEOUT_SECS, default 120s) plus its own connect+write budget.
    // Read the same clamped value core uses and add ~30s of headroom so the resident's
    // socket read never fires before core can legitimately answer on slow CPU-only hardware.
    let core_timeout = goblins_os_ai::resident_timeout::clamp_secs(
        env::var("GOBLINS_OS_RESIDENT_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok()),
    );
    let _ = stream.set_read_timeout(Some(Duration::from_secs(core_timeout + 30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body.as_bytes()))
        .map_err(|_| "resident core request failed".to_string())?;

    let mut response = Vec::new();
    stream
        .take(MAX_CORE_BODY_BYTES)
        .read_to_end(&mut response)
        .map_err(|_| "resident core response failed".to_string())?;

    let (status, body) =
        split_http_response(&response).ok_or_else(|| "core response was malformed".to_string())?;

    #[derive(Deserialize)]
    struct CoreText {
        text: String,
    }

    if !(200..=299).contains(&status) {
        // Core returns a `{ text: "..." }` body with actionable, credential-free guidance
        // on its error paths; surface it. Fall back to the generic line only when the body
        // is missing or unparsable so a body-less error still hides core internals.
        let detail = serde_json::from_slice::<CoreText>(&body)
            .map(|core| core.text)
            .unwrap_or_else(|_| {
                format!("core relay returned HTTP {status} without exposing credentials")
            });
        return Err(detail);
    }

    let parsed: CoreText = serde_json::from_slice(&body)
        .map_err(|_| "core response did not match the resident contract".to_string())?;
    Ok(parsed.text)
}

fn split_http_response(response: &[u8]) -> Option<(u16, Vec<u8>)> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let headers = std::str::from_utf8(&response[..header_end]).ok()?;
    let status = headers
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;

    Some((status, response[(header_end + 4)..].to_vec()))
}

fn parse_http_endpoint(url: &str) -> Option<HttpEndpoint> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?.trim();
    if authority.is_empty() {
        return None;
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (authority, 80),
    };

    if host.is_empty() {
        None
    } else {
        Some(HttpEndpoint {
            host: host.to_string(),
            port,
        })
    }
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

fn core_url() -> String {
    env::var("GOBLINS_OS_CORE_URL")
        .or_else(|_| env::var("OPENAI_OS_CORE_URL"))
        .unwrap_or_else(|_| DEFAULT_CORE_URL.to_string())
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

fn cloud_relay_configured() -> bool {
    // Defense-in-depth only: require the relay URL to be present, HTTPS, and paired with a
    // gateway key. Core (resident_relay) remains the authority — it additionally applies the
    // offline/private gate, which the resident must NOT self-determine from env alone.
    env_var_with_compat(RESIDENT_RELAY_ENV, RESIDENT_RELAY_LEGACY_ENV)
        .map(|url| url.trim().starts_with("https://"))
        .unwrap_or(false)
        && env::var_os("AI_GATEWAY_API_KEY").is_some()
}

fn local_relay_configured() -> bool {
    env_var_os_with_compat(LOCAL_MODEL_RELAY_ENV, LOCAL_MODEL_RELAY_LEGACY_ENV)
}

fn env_var_os_with_compat(primary: &str, legacy: &str) -> bool {
    env::var_os(primary).is_some() || env::var_os(legacy).is_some()
}

fn env_var_with_compat(primary: &str, legacy: &str) -> Option<String> {
    env::var(primary).or_else(|_| env::var(legacy)).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        dispatch, parse_http_endpoint, parse_request, selected_engine, split_http_response,
        CapabilityState, RequestError, ResidentOp,
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
        let ping = dispatch(r#"{"op":"ping"}"#);
        assert!(ping.contains("\"ok\":true"));
        assert!(ping.contains("\"status\":\"online\""));

        let status = dispatch(r#"{"op":"status"}"#);
        assert!(status.contains("\"capabilities\""));
        assert!(status.contains("\"engine\""));
        assert!(status.contains("\"ai\""));
        assert!(status.contains("ai-native-os-actions"));

        let bad = dispatch("garbage");
        assert!(bad.contains("\"error\""));
    }

    #[test]
    fn parses_local_core_endpoint_for_relay() {
        let endpoint = parse_http_endpoint("http://127.0.0.1:8787").unwrap();
        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 8787);
        assert!(parse_http_endpoint("https://127.0.0.1:8787").is_none());
    }

    #[test]
    fn splits_core_http_response() {
        let (status, body) = split_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"text\":\"hi\"}",
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, br#"{"text":"hi"}"#);
    }
}
