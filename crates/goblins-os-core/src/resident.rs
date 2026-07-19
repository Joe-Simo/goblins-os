use axum::{
    http::{StatusCode, Uri},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use crate::{
    ai::{audit_ai_action, AiActionOutcome},
    credentials::{openai_credential, openai_credential_with_compat},
    openai_key::EngineSelection,
    policy::{policy_state_for_control, PolicyControlState},
};

const DEFAULT_RESIDENT_STATE_DIR: &str = "/var/lib/goblins-os/resident";
const RESIDENT_HEARTBEAT_STALE_SECS: u64 = 90;
const LOCAL_MODEL_RELAY_ENV: &str = "GOBLINS_OS_LOCAL_MODEL_RELAY";
const LOCAL_MODEL_RELAY_LEGACY_ENV: &str = "OPENAI_OS_LOCAL_MODEL_RELAY";
const LOCAL_MODEL_KEEP_ALIVE_ENV: &str = "GOBLINS_OS_LOCAL_MODEL_KEEP_ALIVE";
const RESIDENT_RELAY_ENV: &str = "GOBLINS_OS_RESIDENT_RELAY_URL";
const RESIDENT_RELAY_LEGACY_ENV: &str = "OPENAI_OS_RESIDENT_RELAY_URL";
const DEFAULT_OPENAI_API_BASE: &str = "https://api.openai.com";

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
    ready: bool,
    provider: String,
    locality: String,
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
    #[serde(rename = "engine")]
    _engine: serde_json::Value,
    capabilities: Vec<ResidentCapability>,
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
    /// An on-device loopback adapter speaking the `{message} -> {text}` contract.
    LocalContract { url: String },
    /// A locally-detected open-weight inference runtime spoken to natively, so
    /// local-only users get a working resident with just a supported runtime
    /// installed — no hand-written adapter required. This is the GPT-OSS heart.
    LocalRuntime { url: String, model: String },
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
    /// An operator-managed HTTPS relay, used only when the explicit
    /// `cloud-openai` engine selection is active.
    ManagedCloud { url: String, authorization: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EngineLocality {
    OnDevice,
    Cloud,
}

impl EngineLocality {
    const fn as_id(self) -> &'static str {
        match self {
            Self::OnDevice => "on-device",
            Self::Cloud => "cloud",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResidentRouteKind {
    LocalContract,
    LocalRuntime,
    Codex,
    OpenAiApi,
    ManagedCloud,
}

impl ResidentRouteKind {
    const fn engine_label(self) -> &'static str {
        match self {
            Self::LocalContract | Self::LocalRuntime => "local-gpt-oss",
            Self::Codex => "codex",
            Self::OpenAiApi => "openai-api",
            Self::ManagedCloud => "cloud-openai",
        }
    }

    const fn provider(self) -> &'static str {
        match self {
            Self::LocalContract => "gpt-oss-local-contract",
            Self::LocalRuntime => "gpt-oss-local-runtime",
            Self::Codex => "codex",
            Self::OpenAiApi => "openai-api",
            Self::ManagedCloud => "managed-openai-relay",
        }
    }

    const fn locality(self) -> EngineLocality {
        match self {
            Self::LocalContract | Self::LocalRuntime => EngineLocality::OnDevice,
            Self::Codex | Self::OpenAiApi | Self::ManagedCloud => EngineLocality::Cloud,
        }
    }

    const fn contract_detail(self) -> &'static str {
        match self {
            Self::LocalContract => "On-device GPT-OSS through an OS-owned loopback adapter.",
            Self::LocalRuntime => "On-device GPT-OSS through the local model runtime.",
            Self::Codex => "OpenAI account access through the sandboxed Codex CLI.",
            Self::OpenAiApi => "Hosted OpenAI through the Responses API using your OS-owned key.",
            Self::ManagedCloud => {
                "Hosted OpenAI through the explicitly selected managed HTTPS service."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RouteInputs {
    selection: EngineSelection,
    private_mode: bool,
    cloud_allowed: bool,
    local_contract_ready: bool,
    local_runtime_ready: bool,
    codex_ready: bool,
    openai_key_ready: bool,
    openai_base_https: bool,
    managed_cloud_ready: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RouteUnavailable {
    PrivateMode,
    CloudPolicyBlocked,
    LocalNotReady,
    CodexNotReady,
    OpenAiKeyMissing,
    OpenAiBaseInvalid,
    ManagedCloudNotReady,
}

#[derive(Debug, Eq, PartialEq)]
struct ResolvedResidentRoute {
    kind: ResidentRouteKind,
    relay: ResidentRelay,
}

pub async fn ai_runtime_status() -> Json<ResidentStatus> {
    Json(build_resident_status())
}

/// One conversation turn blocks for up to the resident read timeout (120s by
/// default) or a `codex exec` run under its 600s bound, so the body runs on
/// the blocking pool instead of pinning an async runtime worker.
pub async fn ai_runtime(
    Json(payload): Json<ResidentRequest>,
) -> (StatusCode, Json<ResidentResponse>) {
    crate::bounded::run_blocking(move || ai_runtime_blocking(payload))
        .await
        .unwrap_or_else(|_| {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(ResidentResponse {
                    text: crate::bounded::LONG_OPERATION_BUSY_MESSAGE.to_string(),
                }),
            )
        })
}

fn ai_runtime_blocking(payload: ResidentRequest) -> (StatusCode, Json<ResidentResponse>) {
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
    let route = match resolve_resident_route() {
        Ok(route) => route,
        Err(reason) => {
            audit_ai_action("ask-goblins", Some("launcher"), AiActionOutcome::Blocked);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ResidentResponse {
                    text: format!(
                        "Goblins AI is {}. {} Open Models to review the selected engine.",
                        resident_process_label(&status.process.state),
                        route_unavailable_detail(reason)
                    ),
                }),
            );
        }
    };

    match forward_resident_message(&route.relay, message) {
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

    // Status and execution consume the same authoritative route resolution. A
    // selected local engine can therefore never be labelled ready through a cloud
    // fallback, and a hosted route cannot look local in the client contract.
    let selection = crate::openai_key::selected_engine();
    let route = resolve_resident_route().ok();
    let ready = route.is_some();
    let locality = route.as_ref().map(|route| route.kind.locality());
    let cloud_configured = locality == Some(EngineLocality::Cloud);
    let local_configured = locality == Some(EngineLocality::OnDevice);
    let provider = route
        .as_ref()
        .map(|route| route.kind.provider())
        .unwrap_or("not-ready");
    let relay_contract = route
        .as_ref()
        .map(|route| route.kind.contract_detail())
        .unwrap_or("The selected engine is not ready.");

    let (pid, mode, mut engine, mut capabilities) = match stored {
        Some(stored) => (
            Some(stored.pid),
            stored.mode,
            ResidentEngine {
                selected: selection.as_id().to_string(),
                ready,
                provider: provider.to_string(),
                locality: locality
                    .map(EngineLocality::as_id)
                    .unwrap_or("unavailable")
                    .to_string(),
                cloud_relay_configured: cloud_configured,
                local_relay_configured: local_configured,
                relay_contract: relay_contract.to_string(),
            },
            stored.capabilities,
        ),
        None => (
            None,
            "persistent".to_string(),
            ResidentEngine {
                selected: selection.as_id().to_string(),
                ready,
                provider: provider.to_string(),
                locality: locality
                    .map(EngineLocality::as_id)
                    .unwrap_or("unavailable")
                    .to_string(),
                cloud_relay_configured: cloud_configured,
                local_relay_configured: local_configured,
                relay_contract: relay_contract.to_string(),
            },
            default_capabilities(ready),
        ),
    };

    // Overwrite all route fields from core's resolver; resident heartbeat state is
    // useful only for process health and can never choose or relabel model access.
    engine.selected = selection.as_id().to_string();
    engine.ready = ready;
    engine.provider = provider.to_string();
    engine.locality = locality
        .map(EngineLocality::as_id)
        .unwrap_or("unavailable")
        .to_string();
    engine.cloud_relay_configured = cloud_configured;
    engine.local_relay_configured = local_configured;
    engine.relay_contract = relay_contract.to_string();

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
    if relay.locality() == EngineLocality::Cloud && !hosted_execution_allowed() {
        return Err("hosted model access is blocked by Private mode or OS policy");
    }

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(resident_read_timeout_secs()))
        .timeout_write(Duration::from_secs(10))
        .build();

    match relay {
        ResidentRelay::LocalContract { url } => {
            let response = agent
                .post(url)
                .send_json(serde_json::json!({ "message": message }))
                .map_err(|_| "model access request was rejected")?;
            let relay_response: RelayResponse = response
                .into_json()
                .map_err(|_| "model access response was not understood")?;
            finalize_reply(&relay_response.text)
        }
        ResidentRelay::LocalRuntime { url, model } => {
            let endpoint = format!("{}/api/generate", url.trim_end_matches('/'));
            let response = agent
                .post(&endpoint)
                .send_json(ollama_generate_payload(
                    model,
                    message,
                    local_model_keep_alive().as_deref(),
                ))
                .map_err(local_model_runtime_rejection)?;
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
                .send_json(openai_responses_payload(model, message))
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
        ResidentRelay::ManagedCloud { url, authorization } => {
            let response = agent
                .post(url)
                .set("Authorization", authorization)
                .send_json(serde_json::json!({ "message": message }))
                .map_err(|_| "managed model access request was rejected")?;
            let relay_response: RelayResponse = response
                .into_json()
                .map_err(|_| "managed model access response was not understood")?;
            finalize_reply(&relay_response.text)
        }
    }
}

fn openai_responses_payload(model: &str, message: &str) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "model": model,
        "input": message,
        "store": false,
    });
    // GPT-5.6 defaults to medium, but naming the balanced reasoning contract
    // explicitly keeps a future API default change from silently changing the
    // latency/quality profile of the Goblins resident. Older/custom models do
    // not receive a parameter they may not support.
    if model == "gpt-5.6" || model.starts_with("gpt-5.6-") {
        payload["reasoning"] = serde_json::json!({ "effort": "medium" });
    }
    payload
}

impl ResidentRelay {
    const fn locality(&self) -> EngineLocality {
        match self {
            Self::LocalContract { .. } | Self::LocalRuntime { .. } => EngineLocality::OnDevice,
            Self::OpenAiApi { .. } | Self::Codex | Self::ManagedCloud { .. } => {
                EngineLocality::Cloud
            }
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

fn local_model_keep_alive() -> Option<String> {
    env::var(LOCAL_MODEL_KEEP_ALIVE_ENV).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn ollama_generate_payload(
    model: &str,
    message: &str,
    keep_alive: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({ "model": model, "prompt": message, "stream": false });
    if let Some(keep_alive) = keep_alive {
        payload["keep_alive"] = serde_json::json!(keep_alive);
    }
    payload
}

fn local_model_runtime_rejection(error: ureq::Error) -> &'static str {
    match error {
        ureq::Error::Status(status, _) => {
            // Runtime bodies can echo prompts or other user context. Log only the
            // HTTP status; response content is never diagnostics-safe.
            eprintln!("GOBLINS_OS_LOCAL_MODEL_RUNTIME_REJECTED status={status}");
        }
        other => {
            eprintln!(
                "GOBLINS_OS_LOCAL_MODEL_RUNTIME_REJECTED transport={}",
                match other {
                    ureq::Error::Transport(_) => "transport",
                    ureq::Error::Status(_, _) => "status",
                }
            );
        }
    }
    "local model runtime request was rejected"
}

/// Pure timeout policy (testable without touching the process environment): keep the
/// 120s default when unset/unparsable, otherwise clamp the override to 5s..=3600s.
/// Delegates to the shared helper in `goblins-os-ai` so core and the resident binary
/// agree on one clamp source.
fn clamp_resident_timeout(parsed: Option<u64>) -> u64 {
    goblins_os_ai::resident_timeout::clamp_secs(parsed)
}

/// The exact engine that would answer now. Selection is authoritative: an
/// unavailable local route returns `none` instead of falling through to cloud.
pub(crate) fn active_engine_label() -> &'static str {
    resolve_resident_route()
        .map(|route| route.kind.engine_label())
        .unwrap_or("none")
}

pub(crate) fn active_engine_locality() -> Option<EngineLocality> {
    resolve_resident_route()
        .ok()
        .map(|route| route.kind.locality())
}

pub(crate) fn resident_engine_ready() -> bool {
    resolve_resident_route().is_ok()
}

pub(crate) fn resident_generate(prompt: &str) -> Result<String, &'static str> {
    resident_generate_with_engine(prompt).map(|(text, _)| text)
}

pub(crate) fn resident_generate_with_engine(
    prompt: &str,
) -> Result<(String, &'static str), &'static str> {
    let route = resolve_resident_route().map_err(route_unavailable_detail)?;
    let engine = route.kind.engine_label();
    forward_resident_message(&route.relay, prompt).map(|text| (text, engine))
}

/// True only when a hosted request is permitted at the moment it executes.
/// Codex calls this too because Studio invokes Codex directly for workspace turns.
pub(crate) fn hosted_execution_allowed() -> bool {
    !crate::privacy::offline_enabled()
        && policy_state_for_control("cloud-openai") == PolicyControlState::Allowed
}

fn resolve_route_kind(inputs: RouteInputs) -> Result<ResidentRouteKind, RouteUnavailable> {
    match inputs.selection {
        EngineSelection::LocalGptOss => {
            if inputs.local_contract_ready {
                Ok(ResidentRouteKind::LocalContract)
            } else if inputs.local_runtime_ready {
                Ok(ResidentRouteKind::LocalRuntime)
            } else {
                Err(RouteUnavailable::LocalNotReady)
            }
        }
        selection => {
            if inputs.private_mode {
                return Err(RouteUnavailable::PrivateMode);
            }
            if !inputs.cloud_allowed {
                return Err(RouteUnavailable::CloudPolicyBlocked);
            }
            match selection {
                EngineSelection::Codex if inputs.codex_ready => Ok(ResidentRouteKind::Codex),
                EngineSelection::Codex => Err(RouteUnavailable::CodexNotReady),
                EngineSelection::OpenAiApi if !inputs.openai_key_ready => {
                    Err(RouteUnavailable::OpenAiKeyMissing)
                }
                EngineSelection::OpenAiApi if !inputs.openai_base_https => {
                    Err(RouteUnavailable::OpenAiBaseInvalid)
                }
                EngineSelection::OpenAiApi => Ok(ResidentRouteKind::OpenAiApi),
                EngineSelection::ManagedCloud if inputs.managed_cloud_ready => {
                    Ok(ResidentRouteKind::ManagedCloud)
                }
                EngineSelection::ManagedCloud => Err(RouteUnavailable::ManagedCloudNotReady),
                EngineSelection::LocalGptOss => unreachable!("local selection handled above"),
            }
        }
    }
}

fn resolve_resident_route() -> Result<ResolvedResidentRoute, RouteUnavailable> {
    let selection = crate::openai_key::selected_engine();
    let local_contract = local_contract_url();
    let local_runtime = local_runtime_config();
    let openai_key = crate::openai_key::stored_api_key();
    let openai_base = validated_openai_api_base();
    let managed_cloud = managed_cloud_config();
    let inputs = RouteInputs {
        selection,
        private_mode: crate::privacy::offline_enabled(),
        cloud_allowed: policy_state_for_control("cloud-openai") == PolicyControlState::Allowed,
        local_contract_ready: local_contract.is_some(),
        local_runtime_ready: local_runtime.is_some(),
        codex_ready: crate::codex::codex_available(),
        openai_key_ready: openai_key.is_some(),
        openai_base_https: openai_base.is_some(),
        managed_cloud_ready: managed_cloud.is_some(),
    };
    let kind = resolve_route_kind(inputs)?;
    let relay = match kind {
        ResidentRouteKind::LocalContract => ResidentRelay::LocalContract {
            url: local_contract.expect("route kind requires local contract"),
        },
        ResidentRouteKind::LocalRuntime => {
            let (url, model) = local_runtime.expect("route kind requires local runtime");
            ResidentRelay::LocalRuntime { url, model }
        }
        ResidentRouteKind::Codex => ResidentRelay::Codex,
        ResidentRouteKind::OpenAiApi => ResidentRelay::OpenAiApi {
            key: openai_key.expect("route kind requires OpenAI key"),
            model: crate::openai_key::configured_model(),
            base: openai_base.expect("route kind requires HTTPS OpenAI base"),
        },
        ResidentRouteKind::ManagedCloud => {
            let (url, authorization) =
                managed_cloud.expect("route kind requires managed cloud configuration");
            ResidentRelay::ManagedCloud { url, authorization }
        }
    };
    Ok(ResolvedResidentRoute { kind, relay })
}

fn route_unavailable_detail(reason: RouteUnavailable) -> &'static str {
    match reason {
        RouteUnavailable::PrivateMode => "Private mode blocks the selected cloud engine.",
        RouteUnavailable::CloudPolicyBlocked => {
            "The active policy blocks the selected cloud engine."
        }
        RouteUnavailable::LocalNotReady => "On-device GPT-OSS is not ready.",
        RouteUnavailable::CodexNotReady => "Codex is not installed and signed in.",
        RouteUnavailable::OpenAiKeyMissing => "Your OpenAI API key is not configured.",
        RouteUnavailable::OpenAiBaseInvalid => {
            "The configured OpenAI API address is not a valid HTTPS address."
        }
        RouteUnavailable::ManagedCloudNotReady => {
            "The selected managed OpenAI service is not configured."
        }
    }
}

fn local_contract_url() -> Option<String> {
    openai_credential_with_compat(LOCAL_MODEL_RELAY_ENV, LOCAL_MODEL_RELAY_LEGACY_ENV)
        .filter(|url| local_http_url(url))
}

fn local_runtime_config() -> Option<(String, String)> {
    let url = env::var("GOBLINS_OS_LOCAL_RUNTIME_URL").ok()?;
    let model = env::var("GOBLINS_OS_LOCAL_MODEL").ok()?;
    let model = model.trim();
    if local_http_url(&url) && !model.is_empty() {
        Some((url, model.to_string()))
    } else {
        None
    }
}

fn managed_cloud_config() -> Option<(String, String)> {
    let url = openai_credential_with_compat(RESIDENT_RELAY_ENV, RESIDENT_RELAY_LEGACY_ENV)?;
    if !server_https_url(&url) {
        return None;
    }
    let key = openai_credential("AI_GATEWAY_API_KEY")?;
    (!key.trim().is_empty()).then(|| (url, format!("Bearer {key}")))
}

pub(crate) fn local_model_route_configured() -> bool {
    local_contract_url().is_some() || local_runtime_config().is_some()
}

pub(crate) fn managed_cloud_route_configured() -> bool {
    managed_cloud_config().is_some()
}

fn validated_openai_api_base_from(value: Option<&str>) -> Option<String> {
    let base = value.unwrap_or(DEFAULT_OPENAI_API_BASE).trim();
    let uri = base.parse::<Uri>().ok()?;
    (server_https_url(base) && uri.query().is_none())
        .then(|| base.trim_end_matches('/').to_string())
}

fn validated_openai_api_base() -> Option<String> {
    let configured = env::var("GOBLINS_OS_OPENAI_API_BASE").ok();
    validated_openai_api_base_from(configured.as_deref())
}

pub(crate) fn openai_api_base_is_valid() -> bool {
    validated_openai_api_base().is_some()
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

fn default_capabilities(ready: bool) -> Vec<ResidentCapability> {
    vec![
        ResidentCapability {
            id: "conversation".to_string(),
            label: "Conversation".to_string(),
            state: if ready {
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

fn server_https_url(value: &str) -> bool {
    let Ok(uri) = value.parse::<Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    uri.scheme_str() == Some("https")
        && !authority.host().is_empty()
        && !authority.as_str().contains('@')
}

fn local_http_url(value: &str) -> bool {
    let Some(host) = http_host(value) else {
        return false;
    };

    matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1")
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
        build_resident_status, clamp_resident_timeout, extract_openai_response_text,
        forward_resident_message, local_http_url, ollama_generate_payload,
        openai_responses_payload, resident_process_detail, resolve_route_kind, server_https_url,
        validated_openai_api_base_from, CapabilityState, EngineLocality, OpenAiResponseContent,
        OpenAiResponseOutput, OpenAiResponsesReply, ResidentProcessState, ResidentRelay,
        ResidentRouteKind, RouteInputs, RouteUnavailable,
    };
    use crate::openai_key::EngineSelection;

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
        let relay = ResidentRelay::LocalContract { url };
        let reply = forward_resident_message(&relay, "ping from the Goblins OS resident")
            .expect("resident conversation should round-trip through the local relay");
        assert_eq!(reply, "Hello from the local model relay.");
    }

    #[test]
    fn relay_rejects_empty_reply() {
        let url = serve_relay_once(r#"{"text":""}"#.to_string());
        let relay = ResidentRelay::LocalContract { url };
        assert!(
            forward_resident_message(&relay, "ping").is_err(),
            "an empty relay reply must be rejected, never surfaced as a conversation turn"
        );
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
    fn gpt_5_6_responses_payload_is_private_and_explicitly_balanced() {
        for model in ["gpt-5.6", "gpt-5.6-sol"] {
            let payload = openai_responses_payload(model, "Build a private note.");
            assert_eq!(payload["model"], model);
            assert_eq!(payload["input"], "Build a private note.");
            assert_eq!(payload["store"], false);
            assert_eq!(payload["reasoning"]["effort"], "medium");
        }
    }

    #[test]
    fn responses_payload_does_not_send_unsupported_reasoning_to_other_models() {
        let payload = openai_responses_payload("operator-approved-model", "ping");
        assert_eq!(payload["store"], false);
        assert!(payload.get("reasoning").is_none());
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
        let relay = ResidentRelay::LocalRuntime {
            url,
            model: "gpt-oss-20b".to_string(),
        };
        let reply = forward_resident_message(&relay, "ping")
            .expect("native local runtime conversation should round-trip");
        assert_eq!(reply, "Local gpt-oss reply from the runtime.");
    }

    #[test]
    fn native_local_runtime_payload_can_keep_model_warm() {
        let payload = ollama_generate_payload("gpt-oss-20b", "ping", Some("30m"));
        assert_eq!(payload["model"], "gpt-oss-20b");
        assert_eq!(payload["prompt"], "ping");
        assert_eq!(payload["stream"], false);
        assert_eq!(payload["keep_alive"], "30m");

        let without_keepalive = ollama_generate_payload("gpt-oss-20b", "ping", None);
        assert!(without_keepalive.get("keep_alive").is_none());
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
    fn relay_urls_are_strictly_scoped() {
        assert!(local_http_url("http://127.0.0.1:11434/v1/resident"));
        assert!(local_http_url("http://[::1]:11434/v1/resident"));
        assert!(!local_http_url("http://example.com/v1/resident"));
        assert!(server_https_url("https://relay.example.com/goblins-os"));
        assert!(!server_https_url("http://relay.example.com/goblins-os"));
        assert!(!server_https_url("https://user@example.com/goblins-os"));
        assert!(!server_https_url("https://exa mple.com/goblins-os"));
    }

    #[test]
    fn custom_openai_api_base_must_be_https() {
        assert_eq!(
            validated_openai_api_base_from(None).as_deref(),
            Some("https://api.openai.com")
        );
        assert_eq!(
            validated_openai_api_base_from(Some("https://gateway.example.com/openai/")).as_deref(),
            Some("https://gateway.example.com/openai")
        );
        assert!(validated_openai_api_base_from(Some("http://gateway.example.com")).is_none());
        assert!(validated_openai_api_base_from(Some("https://user@gateway.example.com")).is_none());
        assert!(validated_openai_api_base_from(Some("https://exa mple.com")).is_none());
        assert!(
            validated_openai_api_base_from(Some("https://gateway.example.com?token=x")).is_none()
        );
        assert!(validated_openai_api_base_from(Some("gateway.example.com")).is_none());
    }

    fn expected_route(inputs: RouteInputs) -> Result<ResidentRouteKind, RouteUnavailable> {
        match inputs.selection {
            EngineSelection::LocalGptOss if inputs.local_contract_ready => {
                Ok(ResidentRouteKind::LocalContract)
            }
            EngineSelection::LocalGptOss if inputs.local_runtime_ready => {
                Ok(ResidentRouteKind::LocalRuntime)
            }
            EngineSelection::LocalGptOss => Err(RouteUnavailable::LocalNotReady),
            _ if inputs.private_mode => Err(RouteUnavailable::PrivateMode),
            _ if !inputs.cloud_allowed => Err(RouteUnavailable::CloudPolicyBlocked),
            EngineSelection::Codex if inputs.codex_ready => Ok(ResidentRouteKind::Codex),
            EngineSelection::Codex => Err(RouteUnavailable::CodexNotReady),
            EngineSelection::OpenAiApi if !inputs.openai_key_ready => {
                Err(RouteUnavailable::OpenAiKeyMissing)
            }
            EngineSelection::OpenAiApi if !inputs.openai_base_https => {
                Err(RouteUnavailable::OpenAiBaseInvalid)
            }
            EngineSelection::OpenAiApi => Ok(ResidentRouteKind::OpenAiApi),
            EngineSelection::ManagedCloud if inputs.managed_cloud_ready => {
                Ok(ResidentRouteKind::ManagedCloud)
            }
            EngineSelection::ManagedCloud => Err(RouteUnavailable::ManagedCloudNotReady),
        }
    }

    #[test]
    fn route_matrix_is_exhaustive_and_never_crosses_provider_or_locality() {
        let selections = [
            EngineSelection::LocalGptOss,
            EngineSelection::Codex,
            EngineSelection::OpenAiApi,
            EngineSelection::ManagedCloud,
        ];

        // Exhaust every combination of privacy, policy, and the six concrete
        // route-readiness inputs for every selection: 4 * 2^8 = 1024 cases.
        for selection in selections {
            for bits in 0_u16..=u8::MAX as u16 {
                let enabled = |bit: u8| bits & (1_u16 << bit) != 0;
                let inputs = RouteInputs {
                    selection,
                    private_mode: enabled(0),
                    cloud_allowed: enabled(1),
                    local_contract_ready: enabled(2),
                    local_runtime_ready: enabled(3),
                    codex_ready: enabled(4),
                    openai_key_ready: enabled(5),
                    openai_base_https: enabled(6),
                    managed_cloud_ready: enabled(7),
                };
                assert_eq!(
                    resolve_route_kind(inputs),
                    expected_route(inputs),
                    "route mismatch for {inputs:?}"
                );
                if let Ok(route) = resolve_route_kind(inputs) {
                    assert_eq!(route.engine_label(), selection.as_id());
                    assert_eq!(
                        route.locality() == EngineLocality::Cloud,
                        selection.is_hosted(),
                        "route crossed locality for {inputs:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn route_contracts_report_their_real_locality_and_provider() {
        assert_eq!(
            ResidentRouteKind::LocalContract.locality(),
            EngineLocality::OnDevice
        );
        assert_eq!(
            ResidentRouteKind::LocalRuntime.locality(),
            EngineLocality::OnDevice
        );
        assert_eq!(
            ResidentRouteKind::ManagedCloud.locality(),
            EngineLocality::Cloud
        );
        assert_ne!(
            ResidentRouteKind::LocalContract.provider(),
            ResidentRouteKind::ManagedCloud.provider()
        );
        assert!(ResidentRouteKind::LocalContract
            .contract_detail()
            .contains("loopback"));
        assert!(ResidentRouteKind::ManagedCloud
            .contract_detail()
            .contains("HTTPS"));
        assert_eq!(
            ResidentRelay::LocalContract {
                url: "http://127.0.0.1:11434/v1/resident".to_string(),
            }
            .locality(),
            EngineLocality::OnDevice
        );
        assert_eq!(
            ResidentRelay::ManagedCloud {
                url: "https://relay.example.com/resident".to_string(),
                authorization: "redacted-test-value".to_string(),
            }
            .locality(),
            EngineLocality::Cloud
        );
    }

    #[test]
    fn capability_state_serializes_for_native_ui() {
        assert_eq!(
            serde_json::to_string(&CapabilityState::PermissionGated).unwrap(),
            "\"permission-gated\""
        );
    }
}
