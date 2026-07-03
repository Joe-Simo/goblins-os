use axum::{
    extract::Query,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use crate::http_error::error_response;

const PENDING_AUTH_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Serialize)]
pub struct OpenAIAuthStatus {
    configured: bool,
    authenticated: bool,
    provider: &'static str,
    session_storage: String,
    message: String,
}

pub async fn openai_auth_status() -> Json<OpenAIAuthStatus> {
    let configured = openai_auth_provider_configured();
    let authenticated = openai_account_authenticated();
    let session_path = auth_session_path();

    Json(OpenAIAuthStatus {
        configured,
        authenticated,
        provider: if configured {
            "openai-oidc"
        } else {
            "unconfigured"
        },
        session_storage: session_path.display().to_string(),
        message: if configured {
            if authenticated {
                "OpenAI account session is stored privately on this device.".to_string()
            } else {
                "OpenAI account sign-in is configured for this device.".to_string()
            }
        } else {
            "No supported OpenAI account identity provider is configured.".to_string()
        },
    })
}

pub fn openai_auth_provider_configured() -> bool {
    auth_config().is_some()
}

/// Re-authenticate this many seconds before the token's nominal expiry so the
/// session re-locks slightly early rather than mid-request.
const SESSION_CLOCK_SKEW_SECS: u64 = 60;

pub fn openai_account_authenticated() -> bool {
    let path = auth_session_path();
    let Ok(bytes) = fs::read(&path) else {
        return false;
    };
    // A present session whose stored shape we cannot parse stays authoritative by
    // presence (e.g. a legacy file); only a parseable, definitively-expired token
    // re-locks the desktop.
    match serde_json::from_slice::<StoredAuthSessionRead>(&bytes) {
        Ok(stored) => session_is_active(
            stored.created_at_unix,
            stored.token.expires_in,
            now_unix(),
            SESSION_CLOCK_SKEW_SECS,
        ),
        Err(_) => true,
    }
}

/// A session is active when its token has no advertised lifetime, when its
/// creation time is unknown (presence is then authoritative), or when the
/// lifetime (minus a small skew) has not yet elapsed.
fn session_is_active(
    created_at_unix: u64,
    expires_in: Option<u64>,
    now_unix: u64,
    skew_secs: u64,
) -> bool {
    if created_at_unix == 0 {
        return true;
    }
    match expires_in {
        Some(ttl) => now_unix.saturating_add(skew_secs) < created_at_unix.saturating_add(ttl),
        None => true,
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[derive(Deserialize)]
struct StoredAuthSessionRead {
    #[serde(default)]
    created_at_unix: u64,
    token: TokenResponse,
}

pub async fn openai_auth_start() -> Response {
    let Some(config) = auth_config() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "OpenAI account login is not configured. Goblins OS will not fake an identity provider.",
        );
    };

    let state = random_url_token(32);
    let verifier = random_url_token(64);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    if let Err(text) = validate_auth_config(&config) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, text);
    }

    let destination = with_query_params(
        &config.auth_url,
        &[
            ("client_id", &config.client_id),
            ("redirect_uri", &config.redirect_uri),
            ("response_type", "code"),
            ("scope", &config.scope),
            ("state", &state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ],
    );
    remember_pending_auth(state.clone(), verifier);

    // The auth URL is operator-supplied; a stray control character or non-ASCII byte
    // would make HeaderValue::from_str fail, so return a clean 500 instead of panicking
    // on a live request path.
    let Ok(location) = HeaderValue::from_str(&destination) else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "OpenAI account auth URL is malformed.",
        );
    };

    let mut response = StatusCode::FOUND.into_response();
    let headers = response.headers_mut();

    headers.insert(header::LOCATION, location);
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie("goblins_os_auth_state", &state))
            .expect("valid auth state cookie"),
    );

    response
}

pub async fn openai_auth_callback(
    headers: HeaderMap,
    Query(query): Query<AuthCallbackQuery>,
) -> Response {
    let Some(config) = auth_config() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "OpenAI account login is not configured. Goblins OS will not fake an identity provider.",
        );
    };
    if let Err(text) = validate_auth_config(&config) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, text);
    }
    if query.error.is_some() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "OpenAI account provider returned an authorization error.",
        );
    }

    let Some(code) = query.code else {
        return error_response(StatusCode::BAD_REQUEST, "Missing authorization code.");
    };
    let Some(state) = query.state else {
        return error_response(StatusCode::BAD_REQUEST, "Missing authorization state.");
    };
    // Defense-in-depth double-submit check: the single-use server-side state store
    // below is the authoritative CSRF defense, but if the browser did send the
    // OS-set state cookie it must match the callback state — a mismatch is a clear
    // cross-session anomaly and is refused.
    if auth_state_cookie_mismatches(&headers, &state) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Authorization state cookie did not match the callback; refusing a cross-session login.",
        );
    }
    let Some(pending) = take_pending_auth(&state) else {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "Authorization state was not recognized by Goblins OS.",
        );
    };

    let token_response = match exchange_code_for_tokens(&config, &code, &pending.verifier).await {
        Ok(token_response) => token_response,
        Err(status) => {
            return error_response(
                status,
                "OpenAI account token exchange failed inside Goblins OS.",
            );
        }
    };

    if persist_auth_session(&token_response).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "OpenAI account session could not be written to OS-owned secret storage.",
        );
    }

    Json(AuthCallbackSuccess {
        ok: true,
        provider: "openai-oidc",
        message: "OpenAI account session stored privately on this device.",
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct AuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct AuthCallbackSuccess {
    ok: bool,
    provider: &'static str,
    message: &'static str,
}

#[derive(Clone)]
struct PendingAuth {
    verifier: String,
    created_at: Instant,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
struct TokenResponse {
    access_token: String,
    token_type: Option<String>,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    scope: Option<String>,
}

#[derive(Clone)]
struct AuthConfig {
    auth_url: String,
    token_url: String,
    client_id: String,
    client_secret: Option<String>,
    redirect_uri: String,
    scope: String,
    device_auth_url: Option<String>,
}

fn auth_config() -> Option<AuthConfig> {
    Some(AuthConfig {
        auth_url: std::env::var("OPENAI_ACCOUNT_AUTH_URL").ok()?,
        token_url: std::env::var("OPENAI_ACCOUNT_TOKEN_URL").ok()?,
        client_id: std::env::var("OPENAI_ACCOUNT_CLIENT_ID").ok()?,
        client_secret: std::env::var("OPENAI_ACCOUNT_CLIENT_SECRET").ok(),
        redirect_uri: std::env::var("OPENAI_ACCOUNT_REDIRECT_URI").ok()?,
        scope: std::env::var("OPENAI_ACCOUNT_SCOPE")
            .unwrap_or_else(|_| "openid profile email".to_string()),
        device_auth_url: std::env::var("OPENAI_ACCOUNT_DEVICE_AUTH_URL").ok(),
    })
}

fn validate_auth_config(config: &AuthConfig) -> Result<(), &'static str> {
    if !config.auth_url.starts_with("https://") {
        return Err("Configured OpenAI account auth URL must use HTTPS.");
    }
    if !config.token_url.starts_with("https://") {
        return Err("Configured OpenAI account token URL must use HTTPS.");
    }
    if let Some(device_auth_url) = &config.device_auth_url {
        if !device_auth_url.starts_with("https://") {
            return Err("Configured OpenAI account device auth URL must use HTTPS.");
        }
    }

    Ok(())
}

async fn exchange_code_for_tokens(
    config: &AuthConfig,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse, StatusCode> {
    let config = config.clone();
    let code = code.to_string();
    let verifier = verifier.to_string();

    tokio::task::spawn_blocking(move || {
        exchange_code_for_tokens_blocking(&config, &code, &verifier)
    })
    .await
    .map_err(|_| StatusCode::BAD_GATEWAY)?
}

// Bounded HTTP agent for every OAuth exchange: an unresponsive token endpoint
// must not wedge a `spawn_blocking` worker forever, mirroring the bounded
// download agent in `model_manager`. Timeouts surface as transport errors and
// take the same branch as any other failed request.
fn auth_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
}

fn exchange_code_for_tokens_blocking(
    config: &AuthConfig,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse, StatusCode> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("client_id", config.client_id.as_str()),
        ("redirect_uri", config.redirect_uri.as_str()),
        ("code", code),
        ("code_verifier", verifier),
    ];
    if let Some(client_secret) = &config.client_secret {
        form.push(("client_secret", client_secret.as_str()));
    }

    let response = auth_agent()
        .post(&config.token_url)
        .send_form(&form)
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !(200..=299).contains(&response.status()) {
        return Err(StatusCode::BAD_GATEWAY);
    }

    response
        .into_json::<TokenResponse>()
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

fn persist_auth_session(token_response: &TokenResponse) -> std::io::Result<()> {
    let path = auth_session_path();
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("auth session path has no parent"));
    };

    create_secret_dir(parent)?;
    let body = serde_json::to_vec(&StoredAuthSession {
        provider: "openai-oidc",
        created_at: format!("{:?}", SystemTime::now()),
        created_at_unix: now_unix(),
        token: token_response,
    })?;
    write_secret_file(&path, &body)
}

// ── Refresh-token rotation ───────────────────────────────────────────────────
// Completes the session lifecycle: an OS-owned refresh keeps the desktop
// authenticated without re-login, and a failed refresh leaves the (possibly
// expired) session to re-lock via `session_is_active`. The refresh exchange runs
// entirely inside the core; the refresh token never leaves OS-owned storage.
pub async fn openai_auth_refresh() -> Response {
    let Some(config) = auth_config() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "OpenAI account login is not configured.",
        );
    };
    if let Err(text) = validate_auth_config(&config) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, text);
    }
    let Some(refresh_token) = stored_refresh_token() else {
        return error_response(
            StatusCode::CONFLICT,
            "No refreshable OpenAI session is stored.",
        );
    };

    let token = match tokio::task::spawn_blocking(move || {
        refresh_session_blocking(&config, &refresh_token)
    })
    .await
    {
        Ok(Ok(token)) => token,
        _ => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "OpenAI account session refresh failed inside Goblins OS.",
            )
        }
    };

    if persist_auth_session(&token).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Refreshed OpenAI session could not be written to OS-owned secret storage.",
        );
    }

    Json(AuthCallbackSuccess {
        ok: true,
        provider: "openai-oidc",
        message: "OpenAI account session refreshed.",
    })
    .into_response()
}

fn stored_refresh_token() -> Option<String> {
    let bytes = fs::read(auth_session_path()).ok()?;
    let stored: StoredAuthSessionRead = serde_json::from_slice(&bytes).ok()?;
    stored.token.refresh_token
}

fn refresh_session_blocking(
    config: &AuthConfig,
    refresh_token: &str,
) -> Result<TokenResponse, StatusCode> {
    let mut form = refresh_form(&config.client_id, refresh_token);
    if let Some(client_secret) = &config.client_secret {
        form.push(("client_secret", client_secret.as_str()));
    }

    let response = auth_agent()
        .post(&config.token_url)
        .send_form(&form)
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !(200..=299).contains(&response.status()) {
        return Err(StatusCode::BAD_GATEWAY);
    }
    response
        .into_json::<TokenResponse>()
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

fn refresh_form<'a>(client_id: &'a str, refresh_token: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
    ]
}

// ── OAuth 2.0 Device Authorization Grant (RFC 8628) ──────────────────────────
// The OS-appropriate real-account login: the device shows a short user code and
// a verification URL; the person approves on any browser; the OS polls for the
// token. No embedded browser, no client secret on the device, and the device
// code stays OS-owned (only the user code is ever shown).
const DEVICE_AUTH_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Deserialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
}

#[derive(Serialize)]
struct DeviceStartResponse {
    handle: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    interval: u64,
    expires_in: u64,
}

#[derive(Deserialize)]
pub struct DevicePollRequest {
    handle: String,
}

#[derive(Serialize)]
struct DeviceStatusResponse {
    status: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
enum DevicePollOutcome {
    Authorized(Box<TokenResponse>),
    Pending,
    SlowDown,
    AccessDenied,
    Expired,
    Failed,
}

struct DevicePending {
    device_code: String,
    created_at: Instant,
}

pub async fn openai_auth_device_start() -> Response {
    let Some(config) = auth_config() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "OpenAI account login is not configured. Goblins OS will not fake an identity provider.",
        );
    };
    if let Err(text) = validate_auth_config(&config) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, text);
    }
    if config.device_auth_url.is_none() {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "OpenAI device login is not configured (no device authorization endpoint).",
        );
    }

    let device =
        match tokio::task::spawn_blocking(move || request_device_authorization_blocking(&config))
            .await
        {
            Ok(Ok(device)) => device,
            _ => {
                return error_response(
                    StatusCode::BAD_GATEWAY,
                    "OpenAI device authorization request failed inside Goblins OS.",
                )
            }
        };

    let handle = random_url_token(24);
    remember_device_auth(handle.clone(), device.device_code);

    Json(DeviceStartResponse {
        handle,
        user_code: device.user_code,
        verification_uri: device.verification_uri,
        verification_uri_complete: device.verification_uri_complete,
        interval: device.interval.unwrap_or(5),
        expires_in: device.expires_in.unwrap_or(900),
    })
    .into_response()
}

pub async fn openai_auth_device_poll(Json(request): Json<DevicePollRequest>) -> Response {
    let Some(config) = auth_config() else {
        return error_response(
            StatusCode::NOT_IMPLEMENTED,
            "OpenAI account login is not configured.",
        );
    };
    let Some(device_code) = device_code_for(&request.handle) else {
        return device_status(StatusCode::GONE, "expired");
    };

    let outcome = match tokio::task::spawn_blocking(move || {
        poll_device_token_blocking(&config, &device_code)
    })
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "Device token poll failed."),
    };

    match outcome {
        DevicePollOutcome::Authorized(token) => {
            forget_device_auth(&request.handle);
            if persist_auth_session(&token).is_err() {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "OpenAI account session could not be written to OS-owned secret storage.",
                );
            }
            device_status(StatusCode::OK, "authorized")
        }
        DevicePollOutcome::Pending => device_status(StatusCode::ACCEPTED, "authorization-pending"),
        DevicePollOutcome::SlowDown => device_status(StatusCode::ACCEPTED, "slow-down"),
        DevicePollOutcome::AccessDenied => {
            forget_device_auth(&request.handle);
            device_status(StatusCode::FORBIDDEN, "access-denied")
        }
        DevicePollOutcome::Expired => {
            forget_device_auth(&request.handle);
            device_status(StatusCode::GONE, "expired")
        }
        DevicePollOutcome::Failed => device_status(StatusCode::BAD_GATEWAY, "failed"),
    }
}

fn device_status(status: StatusCode, label: &'static str) -> Response {
    (status, Json(DeviceStatusResponse { status: label })).into_response()
}

fn request_device_authorization_blocking(
    config: &AuthConfig,
) -> Result<DeviceAuthResponse, StatusCode> {
    let url = config
        .device_auth_url
        .as_deref()
        .ok_or(StatusCode::NOT_IMPLEMENTED)?;
    if !url.starts_with("https://") {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let response = auth_agent()
        .post(url)
        .send_form(&[
            ("client_id", config.client_id.as_str()),
            ("scope", config.scope.as_str()),
        ])
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !(200..=299).contains(&response.status()) {
        return Err(StatusCode::BAD_GATEWAY);
    }
    response
        .into_json::<DeviceAuthResponse>()
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

fn poll_device_token_blocking(config: &AuthConfig, device_code: &str) -> DevicePollOutcome {
    let form = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("device_code", device_code),
        ("client_id", config.client_id.as_str()),
    ];

    // RFC 8628 reports a still-pending authorization as an OAuth *error* response
    // (HTTP 400 with `error: authorization_pending`), which ureq surfaces as
    // `Error::Status`; both the success and error bodies must be classified.
    match auth_agent().post(&config.token_url).send_form(&form) {
        Ok(response) => classify_device_poll(response.status(), &into_json_value(response)),
        Err(ureq::Error::Status(status, response)) => {
            classify_device_poll(status, &into_json_value(response))
        }
        Err(_) => DevicePollOutcome::Failed,
    }
}

fn into_json_value(response: ureq::Response) -> serde_json::Value {
    response
        .into_json::<serde_json::Value>()
        .unwrap_or(serde_json::Value::Null)
}

fn classify_device_poll(status: u16, body: &serde_json::Value) -> DevicePollOutcome {
    if (200..=299).contains(&status) {
        return match serde_json::from_value::<TokenResponse>(body.clone()) {
            Ok(token) if !token.access_token.trim().is_empty() => {
                DevicePollOutcome::Authorized(Box::new(token))
            }
            _ => DevicePollOutcome::Failed,
        };
    }

    match body.get("error").and_then(|error| error.as_str()) {
        Some("authorization_pending") => DevicePollOutcome::Pending,
        Some("slow_down") => DevicePollOutcome::SlowDown,
        Some("access_denied") => DevicePollOutcome::AccessDenied,
        Some("expired_token") => DevicePollOutcome::Expired,
        _ => DevicePollOutcome::Failed,
    }
}

fn device_auths() -> &'static Mutex<HashMap<String, DevicePending>> {
    static DEVICE_AUTHS: OnceLock<Mutex<HashMap<String, DevicePending>>> = OnceLock::new();
    DEVICE_AUTHS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_device_auth(handle: String, device_code: String) {
    let mut store = device_auths()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune_device_auths(&mut store);
    store.insert(
        handle,
        DevicePending {
            device_code,
            created_at: Instant::now(),
        },
    );
}

fn device_code_for(handle: &str) -> Option<String> {
    let mut store = device_auths()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune_device_auths(&mut store);
    store.get(handle).map(|entry| entry.device_code.clone())
}

fn forget_device_auth(handle: &str) {
    let mut store = device_auths()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store.remove(handle);
}

fn prune_device_auths(store: &mut HashMap<String, DevicePending>) {
    let now = Instant::now();
    store.retain(|_, entry| now.duration_since(entry.created_at) <= DEVICE_AUTH_TTL);
}

#[derive(Serialize)]
struct StoredAuthSession<'a> {
    provider: &'static str,
    created_at: String,
    created_at_unix: u64,
    token: &'a TokenResponse,
}

fn auth_session_path() -> PathBuf {
    std::env::var("OPENAI_ACCOUNT_SESSION_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/var/lib/goblins-os/secrets/openai/session.json").into())
}

fn create_secret_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}

fn write_secret_file(path: &Path, body: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;

        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?
    };

    #[cfg(not(unix))]
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;

    file.write_all(body)?;
    file.sync_all()
}

fn pending_auths() -> &'static Mutex<HashMap<String, PendingAuth>> {
    static PENDING_AUTHS: OnceLock<Mutex<HashMap<String, PendingAuth>>> = OnceLock::new();
    PENDING_AUTHS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_pending_auth(state: String, verifier: String) {
    let mut pending = pending_auths()
        .lock()
        .expect("pending auth store should not be poisoned");
    prune_pending_auths(&mut pending);
    pending.insert(
        state,
        PendingAuth {
            verifier,
            created_at: Instant::now(),
        },
    );
}

fn take_pending_auth(state: &str) -> Option<PendingAuth> {
    let mut pending = pending_auths()
        .lock()
        .expect("pending auth store should not be poisoned");
    prune_pending_auths(&mut pending);
    pending.remove(state)
}

fn prune_pending_auths(pending: &mut HashMap<String, PendingAuth>) {
    let now = Instant::now();
    pending.retain(|_, entry| now.duration_since(entry.created_at) <= PENDING_AUTH_TTL);
}

fn random_url_token(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    URL_SAFE_NO_PAD.encode(buffer)
}

fn session_cookie(name: &str, value: &str) -> String {
    format!("{name}={value}; Path=/; Max-Age=600; HttpOnly; SameSite=Lax")
}

/// True only when the OS-set auth-state cookie is present AND disagrees with the
/// callback state. Absent cookie is not a mismatch (the server-side single-use
/// state store remains the authoritative CSRF defense and must not be bypassed
/// by simply dropping a cookie).
fn auth_state_cookie_mismatches(headers: &HeaderMap, state: &str) -> bool {
    match headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| cookie_value(cookies, "goblins_os_auth_state"))
    {
        Some(cookie_state) => cookie_state != state,
        None => false,
    }
}

fn cookie_value<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    cookie_header
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn with_query_params(base_url: &str, params: &[(&str, &str)]) -> String {
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let encoded = params
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                utf8_percent_encode(key, NON_ALPHANUMERIC),
                utf8_percent_encode(value, NON_ALPHANUMERIC)
            )
        })
        .collect::<Vec<_>>()
        .join("&");

    format!("{base_url}{separator}{encoded}")
}

#[cfg(test)]
mod tests {
    use super::{
        auth_state_cookie_mismatches, classify_device_poll, cookie_value, random_url_token,
        refresh_form, remember_pending_auth, session_is_active, take_pending_auth,
        validate_auth_config, with_query_params, AuthConfig, DevicePollOutcome,
    };
    use axum::http::{header, HeaderMap, HeaderValue};

    #[test]
    fn refresh_form_uses_the_refresh_grant() {
        let form = refresh_form("client-x", "rt-123");
        assert!(form.contains(&("grant_type", "refresh_token")));
        assert!(form.contains(&("client_id", "client-x")));
        assert!(form.contains(&("refresh_token", "rt-123")));
    }

    #[test]
    fn device_poll_classifies_rfc8628_outcomes() {
        // Still pending: RFC 8628 reports this as an HTTP 400 OAuth error.
        assert_eq!(
            classify_device_poll(400, &serde_json::json!({"error": "authorization_pending"})),
            DevicePollOutcome::Pending
        );
        assert_eq!(
            classify_device_poll(400, &serde_json::json!({"error": "slow_down"})),
            DevicePollOutcome::SlowDown
        );
        assert_eq!(
            classify_device_poll(400, &serde_json::json!({"error": "access_denied"})),
            DevicePollOutcome::AccessDenied
        );
        assert_eq!(
            classify_device_poll(400, &serde_json::json!({"error": "expired_token"})),
            DevicePollOutcome::Expired
        );
        // Success: a real token body authorizes the session.
        assert!(matches!(
            classify_device_poll(
                200,
                &serde_json::json!({"access_token": "tok", "token_type": "Bearer"})
            ),
            DevicePollOutcome::Authorized(_)
        ));
        // A 2xx with no usable token is a failure, not a silent authorization.
        assert_eq!(
            classify_device_poll(200, &serde_json::json!({"access_token": ""})),
            DevicePollOutcome::Failed
        );
    }

    #[test]
    fn cookie_value_parses_named_cookie_among_others() {
        let header = "theme=dark; goblins_os_auth_state=abc123; locale=en";
        assert_eq!(
            cookie_value(header, "goblins_os_auth_state"),
            Some("abc123")
        );
        assert_eq!(cookie_value(header, "missing"), None);
    }

    #[test]
    fn auth_state_cookie_only_rejects_a_present_mismatch() {
        let with_cookie = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::COOKIE,
                HeaderValue::from_str(&format!("goblins_os_auth_state={value}")).unwrap(),
            );
            headers
        };
        // Matching cookie: not a mismatch.
        assert!(!auth_state_cookie_mismatches(&with_cookie("s-1"), "s-1"));
        // Tampered cookie: a mismatch, refused.
        assert!(auth_state_cookie_mismatches(
            &with_cookie("attacker"),
            "s-1"
        ));
        // Absent cookie: not treated as a mismatch (server-side state store is authoritative).
        assert!(!auth_state_cookie_mismatches(&HeaderMap::new(), "s-1"));
    }

    #[test]
    fn session_expiry_re_locks_only_a_definitively_expired_token() {
        // Active: created at t=1000, 3600s lifetime, now well inside the window.
        assert!(session_is_active(1_000, Some(3_600), 2_000, 60));
        // Expired: now past creation + lifetime.
        assert!(!session_is_active(1_000, Some(3_600), 5_000, 60));
        // Skew re-locks slightly early (now+skew crosses the boundary).
        assert!(!session_is_active(1_000, Some(3_600), 4_580, 60));
        // No advertised lifetime → presence is authoritative.
        assert!(session_is_active(1_000, None, 9_999_999, 60));
        // Unknown creation time (legacy session) → presence is authoritative.
        assert!(session_is_active(0, Some(1), u64::MAX, 60));
    }

    #[test]
    fn auth_config_rejects_non_https_provider_urls() {
        let config = AuthConfig {
            auth_url: "http://auth.invalid/openai".to_string(),
            token_url: "https://auth.invalid/token".to_string(),
            client_id: "client".to_string(),
            client_secret: None,
            redirect_uri: "http://127.0.0.1:8787/v1/auth/openai/callback".to_string(),
            scope: "openid profile email".to_string(),
            device_auth_url: None,
        };

        assert_eq!(
            validate_auth_config(&config),
            Err("Configured OpenAI account auth URL must use HTTPS.")
        );
    }

    #[test]
    fn auth_config_rejects_non_https_token_urls() {
        let config = AuthConfig {
            auth_url: "https://auth.invalid/openai".to_string(),
            token_url: "http://auth.invalid/token".to_string(),
            client_id: "client".to_string(),
            client_secret: None,
            redirect_uri: "http://127.0.0.1:8787/v1/auth/openai/callback".to_string(),
            scope: "openid profile email".to_string(),
            device_auth_url: None,
        };

        assert_eq!(
            validate_auth_config(&config),
            Err("Configured OpenAI account token URL must use HTTPS.")
        );
    }

    #[test]
    fn auth_config_rejects_non_https_device_auth_urls() {
        let config = AuthConfig {
            auth_url: "https://auth.invalid/openai".to_string(),
            token_url: "https://auth.invalid/token".to_string(),
            client_id: "client".to_string(),
            client_secret: None,
            redirect_uri: "http://127.0.0.1:8787/v1/auth/openai/callback".to_string(),
            scope: "openid profile email".to_string(),
            device_auth_url: Some("http://auth.invalid/device".to_string()),
        };

        assert_eq!(
            validate_auth_config(&config),
            Err("Configured OpenAI account device auth URL must use HTTPS.")
        );
    }

    #[test]
    fn pending_auth_state_is_single_use_and_server_side() {
        let state = random_url_token(16);
        let verifier = random_url_token(32);

        remember_pending_auth(state.clone(), verifier.clone());

        let pending = take_pending_auth(&state).expect("pending auth should exist");
        assert_eq!(pending.verifier, verifier);
        assert!(take_pending_auth(&state).is_none());
    }

    #[test]
    fn auth_redirect_query_is_percent_encoded() {
        let destination = with_query_params(
            "https://auth.invalid/openai",
            &[("scope", "openid profile"), ("state", "a+b")],
        );

        assert!(destination.contains("scope=openid%20profile"));
        assert!(destination.contains("state=a%2Bb"));
    }
}
