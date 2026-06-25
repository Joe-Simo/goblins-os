use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use crate::ai::{audit_ai_action, AiActionOutcome};

const DEFAULT_RESIDENT_STATE_DIR: &str = "/var/lib/goblins-os/resident";
const RESIDENT_HEARTBEAT_STALE_SECS: u64 = 90;
const LOCAL_MODEL_RELAY_ENV: &str = "GOBLINS_OS_LOCAL_MODEL_RELAY";
const LOCAL_MODEL_RELAY_LEGACY_ENV: &str = "OPENAI_OS_LOCAL_MODEL_RELAY";
const RESIDENT_RELAY_ENV: &str = "GOBLINS_OS_RESIDENT_RELAY_URL";
const RESIDENT_RELAY_LEGACY_ENV: &str = "OPENAI_OS_RESIDENT_RELAY_URL";

#[derive(Deserialize)]
pub struct ResidentRequest {
    message: String,
}

#[derive(Serialize)]
pub struct ResidentResponse {
    text: String,
}

#[derive(Serialize)]
pub struct ResidentStatus {
    generated_at: String,
    source: &'static str,
    state_path: String,
    process: ResidentProcess,
    engine: ResidentEngine,
    capabilities: Vec<ResidentCapability>,
}

#[derive(Serialize)]
pub struct ResidentProcess {
    state: ResidentProcessState,
    pid: Option<u32>,
    mode: String,
    heartbeat_age_secs: Option<u64>,
    detail: String,
}

#[derive(Serialize)]
pub struct ResidentEngine {
    selected: String,
    cloud_relay_configured: bool,
    local_relay_configured: bool,
    relay_contract: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ResidentCapability {
    id: String,
    label: String,
    state: CapabilityState,
    detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResidentProcessState {
    Online,
    Stale,
    Waiting,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityState {
    Ready,
    Waiting,
    PermissionGated,
}

#[derive(Deserialize)]
struct StoredResidentState {
    pid: u32,
    mode: String,
    engine: StoredResidentEngine,
    capabilities: Vec<ResidentCapability>,
}

#[derive(Deserialize)]
struct StoredResidentEngine {
    selected: String,
    cloud_relay_configured: bool,
    local_relay_configured: bool,
    relay_contract: String,
}

#[derive(Deserialize)]
struct RelayResponse {
    text: String,
}

#[derive(Deserialize)]
struct OllamaReply {
    response: String,
}

#[derive(Deserialize)]
struct OpenAiResponsesReply {
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    output: Vec<OpenAiResponseOutput>,
}

#[derive(Deserialize)]
struct OpenAiResponseOutput {
    #[serde(default)]
    content: Vec<OpenAiResponseContent>,
}

#[derive(Deserialize)]
struct OpenAiResponseContent {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    refusal: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ResidentRelay {
    /// An OS-owned relay speaking the `{message} -> {text}` resident contract
    /// (a server-side OpenAI relay or a local contract adapter).
    Contract {
        url: String,
        authorization: Option<String>,
    },
    /// A locally-detected open-weight inference runtime spoken to natively, so
    /// local-only users get a working resident with just a supported runtime
    /// installed — no hand-written adapter required. This is the GPT-OSS heart.
    Ollama { url: String, model: String },
    /// The user's own OpenAI API key (OS-owned), used only when they explicitly
    /// pick the hosted-OpenAI engine over the default local GPT-OSS heart.
    OpenAiApi {
        key: String,
        model: String,
        base: String,
    },
    /// The user's OpenAI *account* via OpenAI's Codex CLI (Sign in with ChatGPT).
    /// The OS drives `codex exec` non-interactively; credentials stay with Codex.
    Codex,
}

pub async fn ai_runtime_status() -> Json<ResidentStatus> {
    Json(build_resident_status())
}

pub async fn ai_runtime(
    Json(payload): Json<ResidentRequest>,
) -> (StatusCode, Json<ResidentResponse>) {
    let message = payload.message.trim();

    if message.is_empty() || message.chars().count() > 1000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ResidentResponse {
                text: "Goblins AI needs a message between 1 and 1000 characters.".to_string(),
            }),
        );
    }

    let status = build_resident_status();
    let Some(relay) = resident_relay() else {
        audit_ai_action("ask-goblins", Some("launcher"), AiActionOutcome::Blocked);
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ResidentResponse {
                text: format!(
                    "Goblins AI is {}, but model access is not ready. Open Models to configure local or cloud model access.",
                    resident_process_label(&status.process.state)
                ),
            }),
        );
    };

    match forward_resident_message(&relay, message) {
        Ok(text) => {
            audit_ai_action("ask-goblins", Some("launcher"), AiActionOutcome::Succeeded);
            (StatusCode::OK, Json(ResidentResponse { text }))
        }
        Err(detail) => {
            audit_ai_action("ask-goblins", Some("launcher"), AiActionOutcome::Failed);
            (
                StatusCode::BAD_GATEWAY,
                Json(ResidentResponse {
                    text: format!("Goblins AI could not complete the request: {detail}"),
                }),
            )
        }
    }
}

pub(crate) fn build_resident_status() -> ResidentStatus {
    let path = resident_state_path();
    let stored = read_resident_state(&path);
    let heartbeat_age = heartbeat_age_secs(&path);
    let process_state = match (&stored, heartbeat_age) {
        (Some(_), Some(age)) if age <= RESIDENT_HEARTBEAT_STALE_SECS => {
            ResidentProcessState::Online
        }
        (Some(_), _) => ResidentProcessState::Stale,
        (None, _) => ResidentProcessState::Waiting,
    };

    // Core owns the answer path, so core's status must report what would ACTUALLY
    // run — never the resident's looser env-presence guess. Derive both engine
    // booleans (and the selected engine) from the same authoritative gate
    // `resident_relay()` uses, so the status can never advertise cloud-ready while
    // offline/private mode or a non-HTTPS relay URL means a turn would be refused.
    let (cloud_configured, local_configured) = relay_eligibility();

    let (pid, mode, mut engine, mut capabilities) = match stored {
        Some(stored) => (
            Some(stored.pid),
            stored.mode,
            ResidentEngine {
                selected: stored.engine.selected,
                cloud_relay_configured: stored.engine.cloud_relay_configured,
                local_relay_configured: stored.engine.local_relay_configured,
                relay_contract: stored.engine.relay_contract,
            },
            stored.capabilities,
        ),
        None => (
            None,
            "persistent".to_string(),
            ResidentEngine {
                selected: selected_engine(cloud_configured, local_configured).to_string(),
                cloud_relay_configured: cloud_configured,
                local_relay_configured: local_configured,
                relay_contract: "POST JSON {message:string} -> {text:string}; secrets remain server-side or OS-owned".to_string(),
            },
            default_capabilities(),
        ),
    };

    // Overwrite the engine eligibility (including a stored heartbeat's possibly stale
    // or env-derived booleans) with core's authoritative view.
    engine.cloud_relay_configured = cloud_configured;
    engine.local_relay_configured = local_configured;
    engine.selected = selected_engine(cloud_configured, local_configured).to_string();

    // The conversation capability must match: Ready only if a relay would truly answer.
    let conversation_ready = if cloud_configured || local_configured {
        CapabilityState::Ready
    } else {
        CapabilityState::Waiting
    };
    for capability in &mut capabilities {
        if capability.id == "conversation" {
            capability.state = conversation_ready.clone();
        }
    }

    ResidentStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        state_path: path.display().to_string(),
        process: ResidentProcess {
            state: process_state,
            pid,
            mode,
            heartbeat_age_secs: heartbeat_age,
            detail: resident_process_detail(heartbeat_age),
        },
        engine,
        capabilities,
    }
}

fn forward_resident_message(relay: &ResidentRelay, message: &str) -> Result<String, &'static str> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(resident_read_timeout_secs()))
        .timeout_write(Duration::from_secs(10))
        .build();

    match relay {
        ResidentRelay::Contract { url, authorization } => {
            let request = agent.post(url);
            let request = match authorization {
                Some(authorization) => request.set("Authorization", authorization),
                None => request,
            };
            let response = request
                .send_json(serde_json::json!({ "message": message }))
                .map_err(|_| "model access request was rejected")?;
            let relay_response: RelayResponse = response
                .into_json()
                .map_err(|_| "model access response was not understood")?;
            finalize_reply(&relay_response.text)
        }
        ResidentRelay::Ollama { url, model } => {
            let endpoint = format!("{}/api/generate", url.trim_end_matches('/'));
            let response = agent
                .post(&endpoint)
                .send_json(
                    serde_json::json!({ "model": model, "prompt": message, "stream": false }),
                )
                .map_err(|_| "local model runtime request was rejected")?;
            let reply: OllamaReply = response
                .into_json()
                .map_err(|_| "local model runtime response was not understood")?;
            finalize_reply(&reply.response)
        }
        ResidentRelay::OpenAiApi { key, model, base } => {
            let endpoint = format!("{}/v1/responses", base.trim_end_matches('/'));
            let response = agent
                .post(&endpoint)
                .set("Authorization", &format!("Bearer {key}"))
                .send_json(serde_json::json!({
                    "model": model,
                    "input": message,
                    "store": false,
                }))
                .map_err(|_| "OpenAI API request was rejected")?;
            let reply: OpenAiResponsesReply = response
                .into_json()
                .map_err(|_| "OpenAI API response was not understood")?;
            finalize_reply(&extract_openai_response_text(reply)?)
        }
        ResidentRelay::Codex => {
            // The Codex CLI runs the user's OpenAI account locally; no HTTP agent
            // is involved here — the OS shells out and reads the final message.
            let reply = crate::codex::run_codex(message)?;
            finalize_reply(&reply)
        }
    }
}

/// Trim a single resident turn, reject an empty reply, and bound an over-long one
/// rather than failing it — a verbose local model should still answer.
fn finalize_reply(text: &str) -> Result<String, &'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("model access returned an empty reply");
    }
    Ok(trimmed.chars().take(4000).collect())
}

fn extract_openai_response_text(reply: OpenAiResponsesReply) -> Result<String, &'static str> {
    if let Some(text) = reply.output_text {
        if !text.trim().is_empty() {
            return Ok(text);
        }
    }

    let mut parts = Vec::new();
    for item in reply.output {
        for content in item.content {
            if let Some(text) = content.text {
                if !text.trim().is_empty() {
                    parts.push(text);
                }
            } else if let Some(refusal) = content.refusal {
                if !refusal.trim().is_empty() {
                    parts.push(refusal);
                }
            }
        }
    }

    if parts.is_empty() {
        Err("OpenAI API returned no text output")
    } else {
        Ok(parts.join("\n"))
    }
}

/// How long to wait for a resident relay to answer one turn. Defaults to 120s — the
/// right ceiling for hosted OpenAI and a GPU/Metal-accelerated on-device model. A
/// large open-weight model on modest CPU-only hardware can legitimately take longer
/// per turn, so `GOBLINS_OS_RESIDENT_TIMEOUT_SECS` raises it; the value is clamped to
/// a sane 5s..=3600s range so a bad env var can never disable the timeout entirely.
fn resident_read_timeout_secs() -> u64 {
    clamp_resident_timeout(
        env::var("GOBLINS_OS_RESIDENT_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok()),
    )
}

/// Pure timeout policy (testable without touching the process environment): keep the
/// 120s default when unset/unparsable, otherwise clamp the override to 5s..=3600s.
/// Delegates to the shared helper in `goblins-os-ai` so core and the resident binary
/// agree on one clamp source.
fn clamp_resident_timeout(parsed: Option<u64>) -> u64 {
    goblins_os_ai::resident_timeout::clamp_secs(parsed)
}

/// Generate text through whatever resident engine is active (GPT-OSS by default,
/// or the user's OpenAI key if they selected it). Used by the app builder so the
/// OS can turn intent into apps with the same brain that powers conversation.
/// The engine that would actually answer right now, after the relay's priority and
/// the offline gate — what the Studio shows as the active brain for a turn.
pub(crate) fn active_engine_label() -> &'static str {
    match resident_relay() {
        Some(ResidentRelay::OpenAiApi { .. }) => "openai-api",
        Some(ResidentRelay::Codex) => "codex",
        Some(ResidentRelay::Ollama { .. } | ResidentRelay::Contract { .. }) => "local-gpt-oss",
        None => "none",
    }
}

pub(crate) fn resident_engine_ready() -> bool {
    resident_relay().is_some()
}

pub(crate) fn resident_generate(prompt: &str) -> Result<String, &'static str> {
    let relay = resident_relay()
        .ok_or("no model engine is configured (set up GPT-OSS locally or add an OpenAI API key)")?;
    forward_resident_message(&relay, prompt)
}

fn resident_relay() -> Option<ResidentRelay> {
    // Offline / private mode is the authoritative egress gate: when it is on, only
    // on-device, loopback relays are eligible — hosted OpenAI and any server relay
    // are skipped entirely so the AI never reaches the network.
    let offline = crate::privacy::offline_enabled();

    // 0. The user explicitly chose hosted OpenAI over the local GPT-OSS heart,
    //    and supplied their own OS-owned API key. (GPT-OSS remains the default.)
    //    Hosted models reach the internet, so they are never used while offline.
    if !offline && crate::openai_key::openai_engine_selected() {
        if let Some(key) = crate::openai_key::stored_api_key() {
            return Some(ResidentRelay::OpenAiApi {
                key,
                model: crate::openai_key::configured_model(),
                base: openai_api_base(),
            });
        }
    }

    // 0b. The user chose their OpenAI account via Codex CLI (Sign in with ChatGPT)
    //     and is signed in. Codex reaches the internet, so it is offline-gated too.
    if !offline && crate::openai_key::codex_engine_selected() && crate::codex::codex_available() {
        return Some(ResidentRelay::Codex);
    }

    // 1. An explicit OS-owned local contract relay (loopback only) — offline-safe.
    if let Some(url) = env_var_with_compat(LOCAL_MODEL_RELAY_ENV, LOCAL_MODEL_RELAY_LEGACY_ENV) {
        if local_http_url(&url) {
            return Some(ResidentRelay::Contract {
                url,
                authorization: None,
            });
        }
    }

    // 2. A locally-detected open-weight runtime spoken to natively (loopback only)
    //    — the GPT-OSS heart, and the only engine that runs fully offline.
    if let (Ok(url), Ok(model)) = (
        env::var("GOBLINS_OS_LOCAL_RUNTIME_URL"),
        env::var("GOBLINS_OS_LOCAL_MODEL"),
    ) {
        if local_http_url(&url) && !model.trim().is_empty() {
            return Some(ResidentRelay::Ollama {
                url,
                model: model.trim().to_string(),
            });
        }
    }

    // 3. A server-side OpenAI relay (HTTPS) — network egress, so never offline.
    if offline {
        return None;
    }
    let url = env_var_with_compat(RESIDENT_RELAY_ENV, RESIDENT_RELAY_LEGACY_ENV)?;
    if !server_https_url(&url) {
        return None;
    }
    let key = env::var("AI_GATEWAY_API_KEY").ok()?;
    Some(ResidentRelay::Contract {
        url,
        authorization: Some(format!("Bearer {key}")),
    })
}

/// The authoritative relay eligibility, derived from the SAME gate `resident_relay()`
/// uses to actually answer a turn (offline/private mode + the HTTPS / loopback URL
/// checks). The status the daemon serves must report exactly what would run, so it can
/// never claim cloud-ready while `resident_relay()` would return `None` (e.g. offline
/// mode is on or the server relay URL is not HTTPS). `cloud` is true only when a real
/// hosted engine (HTTPS server relay, the user's OpenAI key/account) would be selected;
/// `local` only when a real loopback engine (local contract relay or local runtime)
/// would be selected.
fn relay_eligibility() -> (bool, bool) {
    classify_relay(resident_relay().as_ref())
}

/// Map an eligible relay (the one `resident_relay()` would actually use, already past
/// the offline gate and the HTTPS / loopback URL checks) to `(cloud, local)`. `None`
/// — including when offline mode or a non-HTTPS server relay made every cloud branch
/// ineligible — yields `(false, false)`.
fn classify_relay(relay: Option<&ResidentRelay>) -> (bool, bool) {
    match relay {
        Some(ResidentRelay::OpenAiApi { .. } | ResidentRelay::Codex) => (true, false),
        // A contract relay is either an offline-safe loopback relay (local) or an
        // HTTPS server relay (cloud); distinguish by the URL the gate accepted.
        Some(ResidentRelay::Contract { url, .. }) if local_http_url(url) => (false, true),
        Some(ResidentRelay::Contract { .. }) => (true, false),
        Some(ResidentRelay::Ollama { .. }) => (false, true),
        None => (false, false),
    }
}

fn read_resident_state(path: &Path) -> Option<StoredResidentState> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn heartbeat_age_secs(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified.elapsed().ok().map(|elapsed| elapsed.as_secs())
}

fn resident_state_path() -> PathBuf {
    env::var("GOBLINS_OS_RESIDENT_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_RESIDENT_STATE_DIR).to_path_buf())
        .join("resident.json")
}

fn resident_process_detail(age: Option<u64>) -> String {
    match age {
        Some(age) if age <= RESIDENT_HEARTBEAT_STALE_SECS => {
            format!("Goblins AI runtime checked in {age}s ago.")
        }
        Some(age) => format!(
            "Goblins AI runtime has not checked in for {age}s; the OS will try to restore it."
        ),
        None => "Waiting for Goblins AI runtime status.".to_string(),
    }
}

fn resident_process_label(state: &ResidentProcessState) -> &'static str {
    match state {
        ResidentProcessState::Online => "online",
        ResidentProcessState::Stale => "stale",
        ResidentProcessState::Waiting => "waiting",
    }
}

fn default_capabilities() -> Vec<ResidentCapability> {
    let cloud = cloud_relay_configured_from_env();
    let local = local_relay_configured_from_env();
    vec![
        ResidentCapability {
            id: "conversation".to_string(),
            label: "Conversation".to_string(),
            state: if cloud || local {
                CapabilityState::Ready
            } else {
                CapabilityState::Waiting
            },
            detail: "Waiting for Goblins AI runtime status.".to_string(),
        },
        ResidentCapability {
            id: "computer-use".to_string(),
            label: "Computer Use".to_string(),
            state: CapabilityState::PermissionGated,
            detail: "OS automation requires explicit local permission.".to_string(),
        },
    ]
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

fn cloud_relay_configured_from_env() -> bool {
    env_var_os_with_compat(RESIDENT_RELAY_ENV, RESIDENT_RELAY_LEGACY_ENV)
        && env::var_os("AI_GATEWAY_API_KEY").is_some()
}

fn local_relay_configured_from_env() -> bool {
    env_var_os_with_compat(LOCAL_MODEL_RELAY_ENV, LOCAL_MODEL_RELAY_LEGACY_ENV)
        || (env::var_os("GOBLINS_OS_LOCAL_RUNTIME_URL").is_some()
            && env::var_os("GOBLINS_OS_LOCAL_MODEL").is_some())
}

fn env_var_with_compat(primary: &str, legacy: &str) -> Option<String> {
    env::var(primary).or_else(|_| env::var(legacy)).ok()
}

fn env_var_os_with_compat(primary: &str, legacy: &str) -> bool {
    env::var_os(primary).is_some() || env::var_os(legacy).is_some()
}

fn openai_api_base() -> String {
    env::var("GOBLINS_OS_OPENAI_API_BASE").unwrap_or_else(|_| "https://api.openai.com".to_string())
}

fn server_https_url(value: &str) -> bool {
    value.starts_with("https://") && https_host(value).is_some()
}

fn local_http_url(value: &str) -> bool {
    let Some(host) = http_host(value) else {
        return false;
    };

    matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1")
}

fn https_host(value: &str) -> Option<String> {
    host_after_scheme(value, "https://")
}

fn http_host(value: &str) -> Option<String> {
    host_after_scheme(value, "http://")
}

fn host_after_scheme(value: &str, scheme: &str) -> Option<String> {
    let rest = value.strip_prefix(scheme)?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return None;
    }

    let host = if let Some(rest) = authority.strip_prefix('[') {
        let (host, _) = rest.split_once(']')?;
        host
    } else {
        authority.split(':').next()?
    };

    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_resident_status, clamp_resident_timeout, classify_relay,
        extract_openai_response_text, forward_resident_message, local_http_url,
        resident_process_detail, selected_engine, server_https_url, CapabilityState,
        OpenAiResponseContent, OpenAiResponseOutput, OpenAiResponsesReply, ResidentProcessState,
        ResidentRelay,
    };

    #[test]
    fn resident_timeout_defaults_and_clamps() {
        // Unset / unparsable -> the shipped 120s default.
        assert_eq!(clamp_resident_timeout(None), 120);
        // A normal override is honored (e.g. a slow on-device model).
        assert_eq!(clamp_resident_timeout(Some(600)), 600);
        // Out-of-range values clamp, so a bad env var can never disable the timeout.
        assert_eq!(clamp_resident_timeout(Some(0)), 5);
        assert_eq!(clamp_resident_timeout(Some(10_000)), 3600);
    }

    /// Serve a single JSON reply over a real localhost socket, consuming the
    /// request body first, so the resident relay contract can be exercised
    /// against a genuine HTTP round-trip (the documented local-model relay path).
    fn serve_relay_once(reply_json: String) -> String {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind relay test server");
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/v1/resident");
        std::thread::spawn(move || {
            if let Some(Ok(mut stream)) = listener.incoming().next() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let mut content_length = 0usize;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 {
                        break;
                    }
                    if header == "\r\n" || header == "\n" {
                        break;
                    }
                    if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:")
                    {
                        content_length = value.trim().parse().unwrap_or(0);
                    }
                }
                if content_length > 0 {
                    let mut body = vec![0u8; content_length];
                    let _ = reader.read_exact(&mut body);
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply_json.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(reply_json.as_bytes());
                let _ = stream.flush();
            }
        });
        url
    }

    #[test]
    fn relay_round_trip_returns_real_reply() {
        let url = serve_relay_once(r#"{"text":"Hello from the local model relay."}"#.to_string());
        let relay = ResidentRelay::Contract {
            url,
            authorization: None,
        };
        let reply = forward_resident_message(&relay, "ping from the Goblins OS resident")
            .expect("resident conversation should round-trip through the local relay");
        assert_eq!(reply, "Hello from the local model relay.");
    }

    #[test]
    fn relay_rejects_empty_reply() {
        let url = serve_relay_once(r#"{"text":""}"#.to_string());
        let relay = ResidentRelay::Contract {
            url,
            authorization: None,
        };
        assert!(
            forward_resident_message(&relay, "ping").is_err(),
            "an empty relay reply must be rejected, never surfaced as a conversation turn"
        );
    }

    #[test]
    fn byo_openai_key_round_trips_in_responses_protocol() {
        // The bring-your-own-key engine speaks OpenAI's current Responses API
        // shape; verify it parses output_text from a real socket.
        let base = serve_relay_once(
            r#"{"id":"resp_test","object":"response","output_text":"Hosted OpenAI reply."}"#
                .to_string(),
        );
        let base = base.trim_end_matches("/v1/resident").to_string();
        let relay = ResidentRelay::OpenAiApi {
            key: "sk-proj-test".to_string(),
            model: "gpt-5.5".to_string(),
            base,
        };
        let reply = forward_resident_message(&relay, "ping")
            .expect("bring-your-own-key conversation should round-trip");
        assert_eq!(reply, "Hosted OpenAI reply.");
    }

    #[test]
    fn byo_openai_key_extracts_nested_responses_output_text() {
        let reply = OpenAiResponsesReply {
            output_text: None,
            output: vec![OpenAiResponseOutput {
                content: vec![OpenAiResponseContent {
                    text: Some("Nested hosted OpenAI reply.".to_string()),
                    refusal: None,
                }],
            }],
        };

        assert_eq!(
            extract_openai_response_text(reply).unwrap(),
            "Nested hosted OpenAI reply."
        );
    }

    #[test]
    fn source_does_not_use_legacy_chat_completions_for_hosted_openai() {
        let source = include_str!("resident.rs");
        assert!(source.contains("/v1/responses"));
        let legacy_endpoint = ["/v1/chat", "completions"].join("/");
        let legacy_parser = [
            format!("choices{}", "[0]"),
            "message".into(),
            "content".into(),
        ]
        .join(".");
        assert!(!source.contains(&legacy_endpoint));
        assert!(!source.contains(&legacy_parser));
    }

    #[test]
    fn native_local_runtime_round_trips_in_ollama_protocol() {
        // The OS speaks a local open-weight runtime's native protocol directly,
        // so local-only users need only a supported runtime, not an adapter.
        let base =
            serve_relay_once(r#"{"response":"Local gpt-oss reply from the runtime."}"#.to_string());
        // serve_relay_once returns "http://host:port/v1/resident"; the Ollama relay
        // appends /api/generate to the base, so trim the test path back to the root.
        let url = base.trim_end_matches("/v1/resident").to_string();
        let relay = ResidentRelay::Ollama {
            url,
            model: "gpt-oss-20b".to_string(),
        };
        let reply = forward_resident_message(&relay, "ping")
            .expect("native local runtime conversation should round-trip");
        assert_eq!(reply, "Local gpt-oss reply from the runtime.");
    }

    #[test]
    fn status_uses_os_owned_default_path() {
        let status = build_resident_status();

        assert_eq!(status.source, "goblins-os-core");
        assert!(status.state_path.contains("/var/lib/goblins-os/resident"));
        assert!(matches!(
            status.process.state,
            ResidentProcessState::Online
                | ResidentProcessState::Stale
                | ResidentProcessState::Waiting
        ));
    }

    #[test]
    fn resident_status_copy_is_goblins_native() {
        assert_eq!(
            resident_process_detail(Some(4)),
            "Goblins AI runtime checked in 4s ago."
        );
        assert_eq!(
            resident_process_detail(Some(120)),
            "Goblins AI runtime has not checked in for 120s; the OS will try to restore it."
        );
        assert_eq!(
            resident_process_detail(None),
            "Waiting for Goblins AI runtime status."
        );
    }

    #[test]
    fn resident_user_copy_hides_backend_plumbing() {
        let source = include_str!("resident.rs");
        assert!(source.contains("Goblins AI needs a message between 1 and 1000 characters."));
        assert!(source.contains("Open Models to configure local or cloud model access."));
        assert!(source.contains("model access request was rejected"));

        let old_runtime_name = ["Codex", "resident"].join(" ");
        let old_unready_copy = ["OS-owned model", "relay is configured"].join(" ");
        let old_heartbeat_copy = ["resident", "heartbeat is fresh"].join(" ");
        assert!(!source.contains(&old_runtime_name));
        assert!(!source.contains(&old_unready_copy));
        assert!(!source.contains(&old_heartbeat_copy));
    }

    #[test]
    fn local_engine_has_priority() {
        assert_eq!(selected_engine(true, true), "local-gpt-oss");
        assert_eq!(selected_engine(true, false), "cloud-openai");
        assert_eq!(selected_engine(false, false), "not-configured");
    }

    #[test]
    fn relay_urls_are_strictly_scoped() {
        assert!(local_http_url("http://127.0.0.1:11434/v1/resident"));
        assert!(local_http_url("http://[::1]:11434/v1/resident"));
        assert!(!local_http_url("http://example.com/v1/resident"));
        assert!(server_https_url("https://relay.example.com/goblins-os"));
        assert!(!server_https_url("http://relay.example.com/goblins-os"));
        assert!(!server_https_url("https://user@example.com/goblins-os"));
    }

    #[test]
    fn status_reports_only_actual_relay_eligibility() {
        // A hosted engine that would actually run reports cloud, never local.
        assert_eq!(
            classify_relay(Some(&ResidentRelay::OpenAiApi {
                key: "sk-test".to_string(),
                model: "gpt-5.5".to_string(),
                base: "https://api.openai.com".to_string(),
            })),
            (true, false)
        );
        assert_eq!(classify_relay(Some(&ResidentRelay::Codex)), (true, false));
        // An HTTPS server relay is cloud; a loopback contract relay is local.
        assert_eq!(
            classify_relay(Some(&ResidentRelay::Contract {
                url: "https://relay.example.com/goblins-os".to_string(),
                authorization: Some("Bearer test".to_string()),
            })),
            (true, false)
        );
        assert_eq!(
            classify_relay(Some(&ResidentRelay::Contract {
                url: "http://127.0.0.1:11434/v1/resident".to_string(),
                authorization: None,
            })),
            (false, true)
        );
        assert_eq!(
            classify_relay(Some(&ResidentRelay::Ollama {
                url: "http://127.0.0.1:11434".to_string(),
                model: "gpt-oss-20b".to_string(),
            })),
            (false, true)
        );
        // When no relay is eligible — e.g. offline/private mode disqualifies every
        // cloud branch, or the server relay URL is not HTTPS — cloud must read
        // not-configured, never Ready, so the status can't claim cloud while
        // `resident_relay()` returns None.
        assert_eq!(classify_relay(None), (false, false));
        assert_eq!(selected_engine(false, false), "not-configured");
    }

    #[test]
    fn capability_state_serializes_for_native_ui() {
        assert_eq!(
            serde_json::to_string(&CapabilityState::PermissionGated).unwrap(),
            "\"permission-gated\""
        );
    }
}
