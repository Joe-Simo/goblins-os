use axum::{
    extract::Query,
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
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
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use crate::{credentials::openai_credential, http_error::error_response};

const PENDING_AUTH_TTL: Duration = Duration::from_secs(10 * 60);

/// Serializes the complete local OAuth lifecycle. Every external exchange
/// captures the current generation before it starts and may commit a session
/// only while that generation is still current. Sign-out advances the
/// generation and clears both pending-flow stores under the same lock, so a
/// callback, device poll, or refresh that was already in flight cannot restore
/// the session after the person signed out.
#[derive(Default)]
struct AuthLifecycle {
    generation: u64,
    pending_auths: HashMap<String, PendingAuth>,
    device_auths: HashMap<String, DevicePending>,
}

impl AuthLifecycle {
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending_auths.clear();
        self.device_auths.clear();
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation == generation
    }
}

fn auth_lifecycle() -> &'static Mutex<AuthLifecycle> {
    static AUTH_LIFECYCLE: OnceLock<Mutex<AuthLifecycle>> = OnceLock::new();
    AUTH_LIFECYCLE.get_or_init(|| Mutex::new(AuthLifecycle::default()))
}

fn lock_auth_lifecycle() -> MutexGuard<'static, AuthLifecycle> {
    auth_lifecycle()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn current_auth_generation() -> u64 {
    lock_auth_lifecycle().generation
}

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

    Json(OpenAIAuthStatus {
        configured,
        authenticated,
        provider: if configured {
            "openai-oidc"
        } else {
            "unconfigured"
        },
        session_storage: "OS-owned private storage".to_string(),
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

#[derive(Serialize)]
pub struct ForgetOpenAIAuthResponse {
    ok: bool,
    authenticated: bool,
    text: &'static str,
}

/// Remove this device's OpenAI OAuth session without claiming to revoke remote
/// account sessions. Pending browser/device flows are discarded at the same
/// boundary. A cloud-identity desktop immediately becomes locked because the
/// session gate always re-checks `openai_account_authenticated`.
pub async fn forget_openai_auth_session() -> Response {
    let removal = {
        let mut lifecycle = lock_auth_lifecycle();
        lifecycle.invalidate();
        remove_secret_file(&auth_session_path())
    };
    if let Err(error) = removal {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("OpenAI account session could not be removed from this device: {error}."),
        );
    }

    Json(ForgetOpenAIAuthResponse {
        ok: true,
        authenticated: false,
        text: "OpenAI account session removed from this device. This does not revoke other OpenAI sessions.",
    })
    .into_response()
}

pub fn openai_auth_provider_configured() -> bool {
    auth_config()
        .as_ref()
        .is_some_and(|config| validate_auth_config(config).is_ok())
}

/// Re-authenticate this many seconds before the token's nominal expiry so the
/// session re-locks slightly early rather than mid-request.
const SESSION_CLOCK_SKEW_SECS: u64 = 60;
const SESSION_REFRESH_WINDOW_SECS: u64 = 5 * 60;
const SESSION_REFRESH_CHECK_INTERVAL: Duration = Duration::from_secs(60);
const SESSION_REFRESH_MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredSessionState {
    Active,
    Refreshable,
    Invalid,
}

pub fn openai_account_authenticated() -> bool {
    let _lifecycle = lock_auth_lifecycle();
    let path = auth_session_path();
    let Ok(bytes) = fs::read(&path) else {
        return false;
    };
    match stored_session_state(&bytes, now_unix()) {
        StoredSessionState::Active => true,
        StoredSessionState::Refreshable => false,
        StoredSessionState::Invalid => {
            // Malformed, incomplete, and non-refreshable expired sessions are
            // never credentials. Keep an expired session only when it carries
            // a valid refresh token, so the OS-owned background lifecycle can
            // renew it without asking the person to sign in again.
            let _ = fs::remove_file(path);
            false
        }
    }
}

#[cfg(test)]
fn stored_session_is_authenticated(bytes: &[u8], now: u64) -> bool {
    stored_session_state(bytes, now) == StoredSessionState::Active
}

fn stored_session_state(bytes: &[u8], now: u64) -> StoredSessionState {
    let Ok(stored) = serde_json::from_slice::<StoredAuthSessionRead>(bytes) else {
        return StoredSessionState::Invalid;
    };
    if stored.provider != "openai-oidc"
        || stored.created_at_unix == 0
        || stored.token.access_token.trim().is_empty()
    {
        return StoredSessionState::Invalid;
    }
    if session_is_active(
        stored.created_at_unix,
        stored.token.expires_in,
        now,
        SESSION_CLOCK_SKEW_SECS,
    ) {
        return StoredSessionState::Active;
    }
    if stored
        .token
        .refresh_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty())
    {
        StoredSessionState::Refreshable
    } else {
        StoredSessionState::Invalid
    }
}

/// A session is active when its required creation time is valid and either its
/// token has no advertised lifetime or the lifetime (minus a small skew) has
/// not elapsed. Unknown creation time fails closed.
fn session_is_active(
    created_at_unix: u64,
    expires_in: Option<u64>,
    now_unix: u64,
    skew_secs: u64,
) -> bool {
    if created_at_unix == 0 {
        return false;
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
    provider: String,
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
    if !callback_host_matches_redirect(&headers, &config.redirect_uri) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Authorization callback host did not match the configured Goblins OS loopback address.",
        );
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

    match persist_auth_session_if_current(pending.generation, &token_response) {
        Ok(true) => {}
        Ok(false) => {
            return error_response(
                StatusCode::CONFLICT,
                "OpenAI account sign-in was cancelled on this device.",
            )
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "OpenAI account session could not be written to OS-owned secret storage.",
            )
        }
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
    generation: u64,
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
        auth_url: openai_credential("OPENAI_ACCOUNT_AUTH_URL")?,
        token_url: openai_credential("OPENAI_ACCOUNT_TOKEN_URL")?,
        client_id: openai_credential("OPENAI_ACCOUNT_CLIENT_ID")?,
        client_secret: openai_credential("OPENAI_ACCOUNT_CLIENT_SECRET")
            .filter(|secret| !secret.trim().is_empty()),
        redirect_uri: openai_credential("OPENAI_ACCOUNT_REDIRECT_URI")?,
        scope: openai_credential("OPENAI_ACCOUNT_SCOPE")
            .unwrap_or_else(|| "openid profile email".to_string()),
        device_auth_url: openai_credential("OPENAI_ACCOUNT_DEVICE_AUTH_URL")
            .filter(|url| !url.trim().is_empty()),
    })
}

fn validate_auth_config(config: &AuthConfig) -> Result<(), &'static str> {
    if !valid_https_provider_url(&config.auth_url) {
        return Err("Configured OpenAI account auth URL must use HTTPS.");
    }
    if !valid_https_provider_url(&config.token_url) {
        return Err("Configured OpenAI account token URL must use HTTPS.");
    }
    if config.client_id.trim().is_empty() {
        return Err("Configured OpenAI account client ID must not be empty.");
    }
    if config.scope.trim().is_empty() {
        return Err("Configured OpenAI account scope must not be empty.");
    }
    if !valid_loopback_redirect(&config.redirect_uri) {
        return Err(
            "Configured OpenAI account redirect URI must use the Goblins OS loopback callback.",
        );
    }
    if let Some(device_auth_url) = &config.device_auth_url {
        if !valid_https_provider_url(device_auth_url) {
            return Err("Configured OpenAI account device auth URL must use HTTPS.");
        }
    }

    Ok(())
}

fn valid_https_provider_url(value: &str) -> bool {
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

fn valid_loopback_redirect(value: &str) -> bool {
    let Ok(uri) = value.parse::<Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    let host = authority.host().trim_matches(['[', ']']);
    uri.scheme_str() == Some("http")
        && matches!(host, "127.0.0.1" | "::1" | "localhost")
        && !authority.as_str().contains('@')
        && uri.path() == "/v1/auth/openai/callback"
        && uri.query().is_none()
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
    if token_response.access_token.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "OpenAI token response did not contain an access token",
        ));
    }
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

fn persist_auth_session_if_current(
    generation: u64,
    token_response: &TokenResponse,
) -> std::io::Result<bool> {
    let lifecycle = lock_auth_lifecycle();
    if !lifecycle.is_current(generation) {
        return Ok(false);
    }
    persist_auth_session(token_response)?;
    Ok(true)
}

// ── Refresh-token rotation ───────────────────────────────────────────────────
// Completes the session lifecycle: an OS-owned refresh keeps the desktop
// authenticated without re-login, and a failed refresh leaves the (possibly
// expired) session to re-lock via `session_is_active`. The refresh exchange runs
// entirely inside the core; the refresh token never leaves OS-owned storage.
pub async fn openai_auth_refresh() -> Response {
    if let Err(error) = refresh_openai_auth_session().await {
        let (status, text) = match error {
            RefreshSessionError::Unconfigured => (
                StatusCode::NOT_IMPLEMENTED,
                "OpenAI account login is not configured.",
            ),
            RefreshSessionError::InvalidConfig(text) => {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, text)
            }
            RefreshSessionError::NoSession => (
                StatusCode::CONFLICT,
                "No refreshable OpenAI session is stored.",
            ),
            RefreshSessionError::Busy => (
                StatusCode::TOO_MANY_REQUESTS,
                "OpenAI account session refresh is already in progress.",
            ),
            RefreshSessionError::Exchange => (
                StatusCode::BAD_GATEWAY,
                "OpenAI account session refresh failed inside Goblins OS.",
            ),
            RefreshSessionError::Cancelled => (
                StatusCode::CONFLICT,
                "OpenAI account session was removed while refresh was in progress.",
            ),
            RefreshSessionError::Storage => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Refreshed OpenAI session could not be written to OS-owned secret storage.",
            ),
        };
        return error_response(status, text);
    }

    Json(AuthCallbackSuccess {
        ok: true,
        provider: "openai-oidc",
        message: "OpenAI account session refreshed.",
    })
    .into_response()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RefreshSessionError {
    Unconfigured,
    InvalidConfig(&'static str),
    NoSession,
    Busy,
    Exchange,
    Cancelled,
    Storage,
}

fn auth_refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static AUTH_REFRESH_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    AUTH_REFRESH_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn refresh_openai_auth_session() -> Result<(), RefreshSessionError> {
    let _refresh = auth_refresh_lock()
        .try_lock()
        .map_err(|_| RefreshSessionError::Busy)?;
    let config = auth_config().ok_or(RefreshSessionError::Unconfigured)?;
    validate_auth_config(&config).map_err(RefreshSessionError::InvalidConfig)?;
    let (refresh_token, generation) =
        stored_refresh_token().ok_or(RefreshSessionError::NoSession)?;

    let request_refresh_token = refresh_token.clone();
    let mut token = tokio::task::spawn_blocking(move || {
        refresh_session_blocking(&config, &request_refresh_token)
    })
    .await
    .map_err(|_| RefreshSessionError::Exchange)?
    .map_err(|_| RefreshSessionError::Exchange)?;

    preserve_refresh_token(&mut token, refresh_token);
    match persist_auth_session_if_current(generation, &token) {
        Ok(true) => Ok(()),
        Ok(false) => Err(RefreshSessionError::Cancelled),
        Err(_) => Err(RefreshSessionError::Storage),
    }
}

/// Maintain an OS-owned account session without involving a desktop client.
/// The first check runs immediately at core startup; later checks use a calm
/// cadence and bounded exponential backoff after provider/network failures.
pub async fn maintain_openai_auth_session() {
    let mut delay = Duration::ZERO;
    let mut failure_backoff = SESSION_REFRESH_CHECK_INTERVAL;
    loop {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        delay = SESSION_REFRESH_CHECK_INTERVAL;

        if !auth_session_needs_refresh() {
            failure_backoff = SESSION_REFRESH_CHECK_INTERVAL;
            continue;
        }
        match refresh_openai_auth_session().await {
            Ok(()) => failure_backoff = SESSION_REFRESH_CHECK_INTERVAL,
            Err(RefreshSessionError::Busy) => {}
            Err(_) => {
                tracing::warn!(
                    "OpenAI account session refresh will retry without exposing credentials"
                );
                failure_backoff = failure_backoff
                    .saturating_mul(2)
                    .min(SESSION_REFRESH_MAX_BACKOFF);
                delay = failure_backoff;
            }
        }
    }
}

fn auth_session_needs_refresh() -> bool {
    let _lifecycle = lock_auth_lifecycle();
    let Ok(bytes) = fs::read(auth_session_path()) else {
        return false;
    };
    stored_session_needs_refresh(&bytes, now_unix())
}

fn stored_session_needs_refresh(bytes: &[u8], now: u64) -> bool {
    let Ok(stored) = serde_json::from_slice::<StoredAuthSessionRead>(bytes) else {
        return false;
    };
    if stored.provider != "openai-oidc"
        || stored.created_at_unix == 0
        || stored.token.access_token.trim().is_empty()
        || stored
            .token
            .refresh_token
            .as_deref()
            .is_none_or(|token| token.trim().is_empty())
    {
        return false;
    }
    stored.token.expires_in.is_some_and(|ttl| {
        now.saturating_add(SESSION_REFRESH_WINDOW_SECS)
            >= stored.created_at_unix.saturating_add(ttl)
    })
}

fn stored_refresh_token() -> Option<(String, u64)> {
    let lifecycle = lock_auth_lifecycle();
    let bytes = fs::read(auth_session_path()).ok()?;
    let stored: StoredAuthSessionRead = serde_json::from_slice(&bytes).ok()?;
    if stored.provider != "openai-oidc"
        || stored.created_at_unix == 0
        || stored.token.access_token.trim().is_empty()
    {
        return None;
    }
    let refresh_token = stored
        .token
        .refresh_token
        .filter(|token| !token.trim().is_empty())?;
    Some((refresh_token, lifecycle.generation))
}

/// OAuth providers commonly rotate refresh tokens but are allowed to omit a
/// replacement. Preserve the last valid token in that case so a successful
/// refresh does not silently make the next refresh impossible.
fn preserve_refresh_token(token: &mut TokenResponse, previous: String) {
    if token
        .refresh_token
        .as_deref()
        .is_none_or(|refresh| refresh.trim().is_empty())
    {
        token.refresh_token = Some(previous);
    }
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
    generation: u64,
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
            "OpenAI device login is not configured for this device.",
        );
    }

    let generation = current_auth_generation();
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
    if !remember_device_auth(handle.clone(), device.device_code, generation) {
        return error_response(
            StatusCode::CONFLICT,
            "OpenAI device sign-in was cancelled on this device.",
        );
    }

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
    let Some((device_code, generation)) = device_code_for(&request.handle) else {
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

    if !device_auth_is_current(&request.handle, generation) {
        return device_status(StatusCode::GONE, "expired");
    }

    match outcome {
        DevicePollOutcome::Authorized(token) => {
            match persist_device_auth_if_current(&request.handle, generation, &token) {
                Ok(true) => {}
                Ok(false) => return device_status(StatusCode::GONE, "expired"),
                Err(_) => {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "OpenAI account session could not be written to OS-owned secret storage.",
                    )
                }
            }
            device_status(StatusCode::OK, "authorized")
        }
        DevicePollOutcome::Pending => device_status(StatusCode::ACCEPTED, "authorization-pending"),
        DevicePollOutcome::SlowDown => device_status(StatusCode::ACCEPTED, "slow-down"),
        DevicePollOutcome::AccessDenied => {
            forget_device_auth(&request.handle, generation);
            device_status(StatusCode::FORBIDDEN, "access-denied")
        }
        DevicePollOutcome::Expired => {
            forget_device_auth(&request.handle, generation);
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

fn remember_device_auth(handle: String, device_code: String, generation: u64) -> bool {
    let mut lifecycle = lock_auth_lifecycle();
    if !lifecycle.is_current(generation) {
        return false;
    }
    prune_device_auths(&mut lifecycle.device_auths);
    lifecycle.device_auths.insert(
        handle,
        DevicePending {
            device_code,
            created_at: Instant::now(),
            generation,
        },
    );
    true
}

fn device_code_for(handle: &str) -> Option<(String, u64)> {
    let mut lifecycle = lock_auth_lifecycle();
    prune_device_auths(&mut lifecycle.device_auths);
    lifecycle
        .device_auths
        .get(handle)
        .map(|entry| (entry.device_code.clone(), entry.generation))
}

fn device_auth_is_current(handle: &str, generation: u64) -> bool {
    let lifecycle = lock_auth_lifecycle();
    lifecycle.is_current(generation)
        && lifecycle
            .device_auths
            .get(handle)
            .is_some_and(|entry| entry.generation == generation)
}

fn persist_device_auth_if_current(
    handle: &str,
    generation: u64,
    token: &TokenResponse,
) -> std::io::Result<bool> {
    let mut lifecycle = lock_auth_lifecycle();
    if !lifecycle.is_current(generation)
        || lifecycle
            .device_auths
            .get(handle)
            .is_none_or(|entry| entry.generation != generation)
    {
        return Ok(false);
    }
    persist_auth_session(token)?;
    lifecycle.device_auths.remove(handle);
    Ok(true)
}

fn forget_device_auth(handle: &str, generation: u64) {
    let mut lifecycle = lock_auth_lifecycle();
    if lifecycle
        .device_auths
        .get(handle)
        .is_some_and(|entry| entry.generation == generation)
    {
        lifecycle.device_auths.remove(handle);
    }
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
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("secret file path has no parent"));
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("secret");
    let tmp = parent.join(format!(".{file_name}.{}.tmp", random_url_token(8)));

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;

        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&tmp)?
    };

    #[cfg(not(unix))]
    let mut file = OpenOptions::new().create_new(true).write(true).open(&tmp)?;

    let result = (|| {
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn remove_secret_file(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn remember_pending_auth(state: String, verifier: String) {
    let mut lifecycle = lock_auth_lifecycle();
    prune_pending_auths(&mut lifecycle.pending_auths);
    let generation = lifecycle.generation;
    lifecycle.pending_auths.insert(
        state,
        PendingAuth {
            verifier,
            created_at: Instant::now(),
            generation,
        },
    );
}

fn take_pending_auth(state: &str) -> Option<PendingAuth> {
    let mut lifecycle = lock_auth_lifecycle();
    prune_pending_auths(&mut lifecycle.pending_auths);
    lifecycle.pending_auths.remove(state)
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

/// Refuse DNS-rebinding and alternate-Host requests at the one unauthenticated
/// browser-facing route. The Host header must name the exact loopback authority
/// registered as the OAuth redirect, including its port.
fn callback_host_matches_redirect(headers: &HeaderMap, redirect_uri: &str) -> bool {
    let expected = redirect_uri.parse::<Uri>().ok().and_then(|uri| {
        uri.authority()
            .map(|authority| authority.as_str().to_ascii_lowercase())
    });
    let actual = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_ascii_lowercase());
    matches!((actual, expected), (Some(actual), Some(expected)) if actual == expected)
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
        auth_state_cookie_mismatches, callback_host_matches_redirect, classify_device_poll,
        cookie_value, persist_auth_session, preserve_refresh_token, random_url_token, refresh_form,
        remember_pending_auth, remove_secret_file, session_is_active,
        stored_session_is_authenticated, stored_session_needs_refresh, stored_session_state,
        take_pending_auth, validate_auth_config, with_query_params, write_secret_file, AuthConfig,
        AuthLifecycle, DevicePending, DevicePollOutcome, PendingAuth, StoredSessionState,
        TokenResponse,
    };
    use axum::http::{header, HeaderMap, HeaderValue};
    use std::time::Instant;

    #[test]
    fn sign_out_generation_cancels_every_in_flight_auth_flow() {
        let mut lifecycle = AuthLifecycle::default();
        let generation = lifecycle.generation;
        lifecycle.pending_auths.insert(
            "browser".to_string(),
            PendingAuth {
                verifier: "verifier".to_string(),
                created_at: Instant::now(),
                generation,
            },
        );
        lifecycle.device_auths.insert(
            "device".to_string(),
            DevicePending {
                device_code: "device-code".to_string(),
                created_at: Instant::now(),
                generation,
            },
        );

        lifecycle.invalidate();

        assert!(!lifecycle.is_current(generation));
        assert!(lifecycle.pending_auths.is_empty());
        assert!(lifecycle.device_auths.is_empty());
    }

    #[test]
    fn refresh_form_uses_the_refresh_grant() {
        let form = refresh_form("client-x", "rt-123");
        assert!(form.contains(&("grant_type", "refresh_token")));
        assert!(form.contains(&("client_id", "client-x")));
        assert!(form.contains(&("refresh_token", "rt-123")));
    }

    #[test]
    fn refresh_preserves_or_rotates_the_refresh_token() {
        let mut omitted = TokenResponse {
            access_token: "new-access".to_string(),
            token_type: Some("Bearer".to_string()),
            expires_in: Some(3600),
            refresh_token: None,
            id_token: None,
            scope: None,
        };
        preserve_refresh_token(&mut omitted, "old-refresh".to_string());
        assert_eq!(omitted.refresh_token.as_deref(), Some("old-refresh"));

        let mut rotated = TokenResponse {
            refresh_token: Some("new-refresh".to_string()),
            ..omitted
        };
        preserve_refresh_token(&mut rotated, "old-refresh".to_string());
        assert_eq!(rotated.refresh_token.as_deref(), Some("new-refresh"));
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
    fn empty_access_tokens_are_never_persisted_as_sessions() {
        let token = TokenResponse {
            access_token: "  ".to_string(),
            token_type: Some("Bearer".to_string()),
            expires_in: Some(3600),
            refresh_token: None,
            id_token: None,
            scope: None,
        };
        let error = persist_auth_session(&token).expect_err("empty token must fail closed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
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
    fn oauth_callback_host_must_match_the_registered_loopback_authority() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8787"));
        assert!(callback_host_matches_redirect(
            &headers,
            "http://127.0.0.1:8787/v1/auth/openai/callback"
        ));

        headers.insert(header::HOST, HeaderValue::from_static("attacker.invalid"));
        assert!(!callback_host_matches_redirect(
            &headers,
            "http://127.0.0.1:8787/v1/auth/openai/callback"
        ));
        assert!(!callback_host_matches_redirect(
            &HeaderMap::new(),
            "http://127.0.0.1:8787/v1/auth/openai/callback"
        ));
    }

    #[test]
    fn session_expiry_and_incomplete_state_fail_closed() {
        // Active: created at t=1000, 3600s lifetime, now well inside the window.
        assert!(session_is_active(1_000, Some(3_600), 2_000, 60));
        // Expired: now past creation + lifetime.
        assert!(!session_is_active(1_000, Some(3_600), 5_000, 60));
        // Skew re-locks slightly early (now+skew crosses the boundary).
        assert!(!session_is_active(1_000, Some(3_600), 4_580, 60));
        // No advertised lifetime → presence is authoritative.
        assert!(session_is_active(1_000, None, 9_999_999, 60));
        // Unknown creation time cannot establish authentication.
        assert!(!session_is_active(0, Some(1), u64::MAX, 60));

        for invalid in [
            b"".as_slice(),
            b"{".as_slice(),
            br#"{"provider":"openai-oidc","created_at_unix":1000,"token":{}}"#,
            br#"{"provider":"openai-oidc","created_at_unix":1000,"token":{"access_token":"","token_type":"Bearer","expires_in":3600,"refresh_token":null,"id_token":null,"scope":null}}"#,
            br#"{"provider":"other","created_at_unix":1000,"token":{"access_token":"secret","token_type":"Bearer","expires_in":3600,"refresh_token":null,"id_token":null,"scope":null}}"#,
        ] {
            assert!(!stored_session_is_authenticated(invalid, 2_000));
        }

        let valid = br#"{"provider":"openai-oidc","created_at_unix":1000,"token":{"access_token":"secret","token_type":"Bearer","expires_in":3600,"refresh_token":null,"id_token":null,"scope":null}}"#;
        assert!(stored_session_is_authenticated(valid, 2_000));
    }

    #[test]
    fn expired_refreshable_sessions_are_preserved_for_os_owned_renewal() {
        let refreshable = br#"{"provider":"openai-oidc","created_at_unix":1000,"token":{"access_token":"access","token_type":"Bearer","expires_in":3600,"refresh_token":"refresh","id_token":null,"scope":null}}"#;
        assert_eq!(
            stored_session_state(refreshable, 5_000),
            StoredSessionState::Refreshable
        );
        assert!(stored_session_needs_refresh(refreshable, 5_000));
        assert!(!stored_session_is_authenticated(refreshable, 5_000));

        let non_refreshable = br#"{"provider":"openai-oidc","created_at_unix":1000,"token":{"access_token":"access","token_type":"Bearer","expires_in":3600,"refresh_token":null,"id_token":null,"scope":null}}"#;
        assert_eq!(
            stored_session_state(non_refreshable, 5_000),
            StoredSessionState::Invalid
        );
        assert!(!stored_session_needs_refresh(non_refreshable, 5_000));
    }

    #[test]
    fn auth_session_writes_are_owner_only_and_atomic() {
        let dir = std::env::temp_dir().join(format!(
            "goblins-auth-write-{}-{}",
            std::process::id(),
            random_url_token(6)
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let path = dir.join("session.json");
        write_secret_file(&path, br#"{"ok":true}"#).expect("write secret atomically");
        assert_eq!(
            std::fs::read(&path).expect("read secret"),
            br#"{"ok":true}"#
        );
        assert_eq!(
            std::fs::read_dir(&dir)
                .expect("list test dir")
                .filter_map(Result::ok)
                .count(),
            1,
            "no sibling temp file remains"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path)
                    .expect("secret metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        remove_secret_file(&path).expect("remove secret");
        assert!(!path.exists());
        remove_secret_file(&path).expect("removal is idempotent");
        std::fs::remove_dir_all(dir).expect("remove test dir");
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
    fn auth_config_rejects_empty_identity_fields_and_non_loopback_redirects() {
        let valid = AuthConfig {
            auth_url: "https://auth.openai.com/authorize".to_string(),
            token_url: "https://auth.openai.com/token".to_string(),
            client_id: "client".to_string(),
            client_secret: None,
            redirect_uri: "http://127.0.0.1:8787/v1/auth/openai/callback".to_string(),
            scope: "openid profile email".to_string(),
            device_auth_url: None,
        };

        let mut config = valid.clone();
        config.client_id = "  ".to_string();
        assert_eq!(
            validate_auth_config(&config),
            Err("Configured OpenAI account client ID must not be empty.")
        );

        let mut config = valid.clone();
        config.scope.clear();
        assert_eq!(
            validate_auth_config(&config),
            Err("Configured OpenAI account scope must not be empty.")
        );

        for redirect in [
            "https://example.com/v1/auth/openai/callback",
            "http://example.com/v1/auth/openai/callback",
            "http://127.0.0.1:8787/wrong-path",
            "http://127.0.0.1:8787/v1/auth/openai/callback?next=elsewhere",
        ] {
            let mut config = valid.clone();
            config.redirect_uri = redirect.to_string();
            assert_eq!(
                validate_auth_config(&config),
                Err(
                    "Configured OpenAI account redirect URI must use the Goblins OS loopback callback."
                ),
                "accepted redirect {redirect}"
            );
        }
    }

    #[test]
    fn auth_config_rejects_provider_urls_with_userinfo() {
        let config = AuthConfig {
            auth_url: "https://trusted.example@attacker.invalid/authorize".to_string(),
            token_url: "https://auth.openai.com/token".to_string(),
            client_id: "client".to_string(),
            client_secret: None,
            redirect_uri: "http://localhost:8787/v1/auth/openai/callback".to_string(),
            scope: "openid profile email".to_string(),
            device_auth_url: None,
        };
        assert_eq!(
            validate_auth_config(&config),
            Err("Configured OpenAI account auth URL must use HTTPS.")
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
