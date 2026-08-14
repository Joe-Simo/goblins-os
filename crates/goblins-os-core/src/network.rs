//! Network connectivity for first boot and daily use.
//!
//! Goblins OS needs the internet early: the GPT-OSS model weights are never
//! bundled in the image, and the on-device model builds apps and fetches
//! packages on request. So the OS exposes a small NetworkManager-backed surface —
//! connectivity status, a Wi-Fi scan, and a Wi-Fi connect — used by the
//! onboarding network step. Everything runs server-side through `nmcli`; a Wi-Fi
//! password is handed straight to NetworkManager and is never stored by the OS
//! nor returned to any client. When NetworkManager is unavailable the surface
//! degrades calmly rather than failing.

use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::{bounded_command_output, probe_timeout, BoundedCommandError};
use crate::policy::{policy_state_for_control, PolicyControlState};
use crate::session_bridge::{self, SessionBridgeResult};

const PROXY_SCHEMA: &str = "org.gnome.system.proxy";
const HTTP_PROXY_SCHEMA: &str = "org.gnome.system.proxy.http";
const HTTPS_PROXY_SCHEMA: &str = "org.gnome.system.proxy.https";
const FTP_PROXY_SCHEMA: &str = "org.gnome.system.proxy.ftp";
const SOCKS_PROXY_SCHEMA: &str = "org.gnome.system.proxy.socks";

#[derive(Serialize)]
pub struct NetworkStatus {
    source: &'static str,
    manager_available: bool,
    online: bool,
    connectivity: String,
    state: String,
    active: Option<ActiveConnection>,
    proxy: ProxyStatus,
    detail: String,
}

#[derive(Serialize, PartialEq, Eq, Debug)]
pub struct ActiveConnection {
    name: String,
    kind: String,
    device: String,
}

#[derive(Serialize, PartialEq, Eq, Debug)]
pub struct WifiNetwork {
    ssid: String,
    signal: u8,
    security: String,
    in_use: bool,
}

#[derive(Serialize)]
pub struct WifiScan {
    source: &'static str,
    manager_available: bool,
    networks: Vec<WifiNetwork>,
    detail: String,
}

#[derive(Deserialize)]
pub struct WifiConnectRequest {
    ssid: String,
    password: Option<String>,
}

#[derive(Serialize)]
pub struct WifiConnectOutcome {
    ok: bool,
    ssid: String,
    text: String,
}

#[derive(Serialize)]
pub struct ProxyStatus {
    gsettings_available: bool,
    schema_available: bool,
    mode_available: bool,
    editor_available: bool,
    mode: String,
    autoconfig_url: Option<String>,
    ignore_hosts: Vec<String>,
    http: ProxyEndpoint,
    https: ProxyEndpoint,
    ftp: ProxyEndpoint,
    socks: ProxyEndpoint,
    detail: String,
}

#[derive(Serialize, PartialEq, Eq, Debug)]
pub struct ProxyEndpoint {
    host: Option<String>,
    port: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SetProxySettingsRequest {
    mode: String,
    autoconfig_url: Option<String>,
    ignore_hosts: Vec<String>,
    http: ProxyEndpointRequest,
    https: ProxyEndpointRequest,
    ftp: ProxyEndpointRequest,
    socks: ProxyEndpointRequest,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProxyEndpointRequest {
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Serialize)]
pub struct ProxyModeOutcome {
    ok: bool,
    mode: String,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ValidatedProxySettings {
    mode: &'static str,
    autoconfig_url: String,
    ignore_hosts: Vec<String>,
    http: ValidatedProxyEndpoint,
    https: ValidatedProxyEndpoint,
    ftp: ValidatedProxyEndpoint,
    socks: ValidatedProxyEndpoint,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ValidatedProxyEndpoint {
    host: String,
    port: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProxyKey {
    Mode,
    AutoconfigUrl,
    IgnoreHosts,
    HttpHost,
    HttpPort,
    HttpsHost,
    HttpsPort,
    FtpHost,
    FtpPort,
    SocksHost,
    SocksPort,
}

const PROXY_KEYS: [ProxyKey; 11] = [
    ProxyKey::Mode,
    ProxyKey::AutoconfigUrl,
    ProxyKey::IgnoreHosts,
    ProxyKey::HttpHost,
    ProxyKey::HttpPort,
    ProxyKey::HttpsHost,
    ProxyKey::HttpsPort,
    ProxyKey::FtpHost,
    ProxyKey::FtpPort,
    ProxyKey::SocksHost,
    ProxyKey::SocksPort,
];

const PROXY_URL_MAX_BYTES: usize = 2_048;
const PROXY_HOST_MAX_BYTES: usize = 253;
const PROXY_BYPASS_MAX_ENTRIES: usize = 64;
const PROXY_BYPASS_ENTRY_MAX_BYTES: usize = 253;
const PROXY_BYPASS_TOTAL_MAX_BYTES: usize = 4_096;

enum NmcliError {
    /// NetworkManager's CLI is not present (e.g. a container or pre-NM stage).
    Missing,
    /// The CLI ran but reported an error; the (credential-free) message is kept.
    Failed(String),
}

enum GSettingsError {
    Missing,
    Failed,
}

/// NetworkManager's own connection-activation wait is 90 seconds, so connection
/// writes (joining a Wi-Fi network) get that full window instead of the short
/// status-probe bound, which would kill a legitimately slow activation.
const NETWORK_CONTROL_TIMEOUT: Duration = Duration::from_secs(90);

/// A Wi-Fi list read can block behind the just-requested rescan on slow radios,
/// so it gets a wider bound than an ordinary status probe.
const WIFI_LIST_TIMEOUT: Duration = Duration::from_secs(15);

/// Honest detail for an `nmcli` that is installed but was killed at its bound:
/// this must never be reported as the service being missing. Shared with the
/// hotspot module so the copy stays identical everywhere nmcli is bounded.
pub(crate) const NETWORK_TIMEOUT_DETAIL: &str =
    "NetworkManager did not answer before the network timeout.";

/// Run `nmcli` for a read-only status probe, capturing stdout on success.
fn nmcli(args: &[&str]) -> Result<String, NmcliError> {
    nmcli_bounded(args, probe_timeout())
}

/// Run `nmcli` for a connection write, which may legitimately take as long as
/// NetworkManager's own activation wait. A Wi-Fi password may be among the
/// args; this function never logs the arguments.
fn nmcli_control(args: &[&str]) -> Result<String, NmcliError> {
    nmcli_bounded(args, NETWORK_CONTROL_TIMEOUT)
}

fn nmcli_bounded(args: &[&str], timeout: Duration) -> Result<String, NmcliError> {
    nmcli_result(bounded_command_output("nmcli", args, timeout))
}

/// Map a bounded `nmcli` run onto the module error. Only a missing binary is
/// `Missing`; a timeout means NetworkManager IS present but did not answer, so
/// it is reported as a failure with honest copy, never as an absent service.
fn nmcli_result(
    output: Result<std::process::Output, BoundedCommandError>,
) -> Result<String, NmcliError> {
    match output {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(NmcliError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        )),
        Err(BoundedCommandError::Missing) => Err(NmcliError::Missing),
        Err(BoundedCommandError::TimedOut) => {
            Err(NmcliError::Failed(NETWORK_TIMEOUT_DETAIL.to_string()))
        }
        Err(BoundedCommandError::Failed) => Err(NmcliError::Failed(
            "NetworkManager could not be run in this session.".to_string(),
        )),
    }
}

pub async fn network_status() -> Json<NetworkStatus> {
    Json(build_network_status())
}

pub async fn wifi_scan() -> Json<WifiScan> {
    // A rescan is best-effort; the cached list is returned regardless so a slow
    // or rate-limited radio still yields whatever NetworkManager already knows.
    let _ = nmcli(&["device", "wifi", "rescan"]);
    // The list read can wait on the rescan just issued above, so it gets the
    // wider `WIFI_LIST_TIMEOUT` rather than the ordinary probe bound.
    match nmcli_bounded(
        &[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
        ],
        WIFI_LIST_TIMEOUT,
    ) {
        Ok(stdout) => Json(WifiScan {
            source: "goblins-os-core",
            manager_available: true,
            networks: parse_wifi_list(&stdout),
            detail: "Wi-Fi networks reported by the Goblins OS network service.".to_string(),
        }),
        Err(NmcliError::Missing) => Json(WifiScan {
            source: "goblins-os-core",
            manager_available: false,
            networks: Vec::new(),
            detail: "The Goblins OS network service is not ready in this environment.".to_string(),
        }),
        Err(NmcliError::Failed(detail)) => Json(WifiScan {
            source: "goblins-os-core",
            manager_available: true,
            networks: Vec::new(),
            detail: if detail.is_empty() {
                "No Wi-Fi device is available to scan.".to_string()
            } else {
                detail
            },
        }),
    }
}

pub async fn wifi_connect(
    Json(request): Json<WifiConnectRequest>,
) -> (StatusCode, Json<WifiConnectOutcome>) {
    // Joining a network mutates system state, so it is gated by the active policy
    // profile exactly like the AI settings-control and app-builder write paths.
    match policy_state_for_control("settings-control") {
        PolicyControlState::Allowed => {}
        PolicyControlState::Denied => {
            return outcome(
                StatusCode::FORBIDDEN,
                request.ssid.trim(),
                "Changing the network is blocked by the active Goblins OS policy profile.",
            );
        }
        PolicyControlState::PermissionGated => {
            return outcome(
                StatusCode::FORBIDDEN,
                request.ssid.trim(),
                "Changing the network requires an explicit Goblins OS permission review first.",
            );
        }
    }

    let ssid = request.ssid.trim();
    if ssid.is_empty() {
        return outcome(
            StatusCode::BAD_REQUEST,
            ssid,
            "A Wi-Fi network name is required.",
        );
    }
    // `nmcli device wifi connect` takes the SSID as a positional argument; a name
    // beginning with '-' would be parsed as an option, so reject it as the
    // root-cause fix for argument injection. (A `--` terminator is intentionally
    // not used here: its effect on this shorthand subcommand is version-dependent
    // and would not, on its own, prevent an option-shaped SSID from being misread.)
    if ssid_looks_like_option(ssid) {
        return outcome(
            StatusCode::BAD_REQUEST,
            ssid,
            "A Wi-Fi network name cannot start with a dash.",
        );
    }

    let password = request.password.as_deref().map(str::trim).unwrap_or("");
    let mut args: Vec<&str> = vec!["device", "wifi", "connect", ssid];
    if !password.is_empty() {
        args.push("password");
        args.push(password);
    }

    match nmcli_control(&args) {
        Ok(_) => outcome(
            StatusCode::OK,
            ssid,
            &format!("Connected to {ssid}. Goblins OS is online."),
        ),
        Err(NmcliError::Missing) => outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            ssid,
            "The Goblins OS network service is not ready in this environment, so Wi-Fi cannot be managed here.",
        ),
        Err(NmcliError::Failed(detail)) => outcome(
            StatusCode::BAD_GATEWAY,
            ssid,
            &sanitize_connect_error(&detail, password),
        ),
    }
}

pub async fn set_proxy_settings(
    Json(request): Json<SetProxySettingsRequest>,
) -> (StatusCode, Json<ProxyModeOutcome>) {
    match tokio::task::spawn_blocking(move || set_proxy_settings_outcome(request)).await {
        Ok(outcome) => outcome,
        Err(error) => std::panic::resume_unwind(error.into_panic()),
    }
}

fn outcome(status: StatusCode, ssid: &str, text: &str) -> (StatusCode, Json<WifiConnectOutcome>) {
    (
        status,
        Json(WifiConnectOutcome {
            ok: status == StatusCode::OK,
            ssid: ssid.to_string(),
            text: text.to_string(),
        }),
    )
}

/// True when an SSID would be misread by `nmcli` as an option rather than a value,
/// i.e. it starts with '-'. Passing such a name as a positional argument is the
/// argument-injection vector this rejects at the root.
fn ssid_looks_like_option(ssid: &str) -> bool {
    ssid.starts_with('-')
}

/// Defensive: never let a Wi-Fi password leak through an error string, and give a
/// calm default when NetworkManager's message is empty or noisy.
fn sanitize_connect_error(detail: &str, password: &str) -> String {
    if detail.is_empty() {
        return "Goblins OS could not join that network. Check the password and try again."
            .to_string();
    }
    if !password.is_empty() && detail.contains(password) {
        return "Goblins OS could not join that network. Check the password and try again."
            .to_string();
    }
    format!("Goblins OS could not join that network: {detail}")
}

fn build_network_status() -> NetworkStatus {
    let proxy = build_proxy_status();
    match nmcli(&["-t", "-f", "STATE,CONNECTIVITY", "general"]) {
        Ok(stdout) => {
            let (state, connectivity) = parse_general_status(&stdout);
            let active = nmcli(&["-t", "-f", "NAME,TYPE,DEVICE", "connection", "show", "--active"])
                .ok()
                .and_then(|stdout| parse_active_connection(&stdout));
            NetworkStatus {
                source: "goblins-os-core",
                manager_available: true,
                online: connectivity == "full",
                detail: status_detail(&connectivity, active.as_ref()),
                connectivity,
                state,
                active,
                proxy,
            }
        }
        Err(NmcliError::Missing) => NetworkStatus {
            source: "goblins-os-core",
            manager_available: false,
            online: false,
            connectivity: "unknown".to_string(),
            state: "unmanaged".to_string(),
            active: None,
            proxy,
            detail: "The Goblins OS network service is not ready in this environment, so connectivity cannot be reported or changed here.".to_string(),
        },
        Err(NmcliError::Failed(detail)) => NetworkStatus {
            source: "goblins-os-core",
            manager_available: true,
            online: false,
            connectivity: "unknown".to_string(),
            state: "unknown".to_string(),
            active: None,
            proxy,
            detail: if detail.is_empty() {
                "The Goblins OS network service did not report connectivity.".to_string()
            } else {
                detail
            },
        },
    }
}

fn build_proxy_status() -> ProxyStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema_available = gsettings_available && schema_available(PROXY_SCHEMA);
    let mode_available = schema_available && key_available(PROXY_SCHEMA, "mode");
    let editor_available = gsettings_available
        && PROXY_KEYS
            .iter()
            .copied()
            .all(|key| proxy_key_available(key) && proxy_key_writable(key));
    let mode = if mode_available {
        proxy_string(PROXY_SCHEMA, "mode")
            .map(|mode| normalize_proxy_mode(&mode).to_string())
            .unwrap_or_else(|| "none".to_string())
    } else {
        "none".to_string()
    };

    ProxyStatus {
        gsettings_available,
        schema_available,
        mode_available,
        editor_available,
        autoconfig_url: proxy_string(PROXY_SCHEMA, "autoconfig-url"),
        ignore_hosts: proxy_strv(PROXY_SCHEMA, "ignore-hosts").unwrap_or_default(),
        http: proxy_endpoint(HTTP_PROXY_SCHEMA),
        https: proxy_endpoint(HTTPS_PROXY_SCHEMA),
        ftp: proxy_endpoint(FTP_PROXY_SCHEMA),
        socks: proxy_endpoint(SOCKS_PROXY_SCHEMA),
        detail: proxy_detail(
            gsettings_available,
            schema_available,
            mode_available,
            editor_available,
            &mode,
        ),
        mode,
    }
}

fn set_proxy_settings_outcome(
    request: SetProxySettingsRequest,
) -> (StatusCode, Json<ProxyModeOutcome>) {
    if let Some(outcome) = proxy_policy_denial(&request.mode) {
        return outcome;
    }

    let settings = match validate_proxy_settings(request) {
        Ok(settings) => settings,
        Err(detail) => {
            return proxy_mode_outcome(StatusCode::BAD_REQUEST, "none", &detail);
        }
    };

    let _guard = proxy_write_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if gsettings(&["list-schemas"]).is_err() {
        return proxy_mode_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            settings.mode,
            "Desktop preferences are not ready, so proxy settings cannot be changed in this session.",
        );
    }

    for key in PROXY_KEYS {
        if !proxy_key_available(key) {
            return proxy_mode_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                settings.mode,
                "This desktop does not provide every standard proxy setting required by the editor, so no proxy settings were changed.",
            );
        }
        if !proxy_key_writable(key) {
            return proxy_mode_outcome(
                StatusCode::FORBIDDEN,
                settings.mode,
                "One or more proxy settings are managed by the system administrator, so no proxy settings were changed.",
            );
        }
    }

    let previous = match read_proxy_snapshot() {
        Ok(previous) => previous,
        Err(_) => {
            return proxy_mode_outcome(
                StatusCode::BAD_GATEWAY,
                settings.mode,
                "The desktop session could not read the current proxy settings, so no proxy settings were changed.",
            );
        }
    };
    let desired = proxy_settings_values(&settings);

    if proxy_snapshot_matches(&previous, &desired) {
        return proxy_mode_outcome(
            StatusCode::OK,
            settings.mode,
            "Proxy settings already match this configuration.",
        );
    }

    if let Err(failed_key) = apply_proxy_values(&previous, &desired) {
        let restored = restore_proxy_snapshot(&previous).is_ok();
        let detail = if restored {
            format!(
                "The desktop session could not save the {} setting. The previous proxy configuration was restored.",
                failed_key.display_name()
            )
        } else {
            "The desktop session could not finish or fully restore the proxy configuration. Open the system network tool to review the current proxy state before trying again."
                .to_string()
        };
        return proxy_mode_outcome(StatusCode::BAD_GATEWAY, settings.mode, &detail);
    }

    if !proxy_settings_match(&settings) {
        let restored = restore_proxy_snapshot(&previous).is_ok();
        let detail = if restored {
            "The desktop did not confirm the complete proxy configuration. The previous proxy configuration was restored."
        } else {
            "The desktop did not confirm or fully restore the proxy configuration. Open the system network tool to review the current proxy state before trying again."
        };
        return proxy_mode_outcome(StatusCode::BAD_GATEWAY, settings.mode, detail);
    }

    proxy_mode_outcome(
        StatusCode::OK,
        settings.mode,
        match settings.mode {
            "auto" => "Automatic proxy configuration saved and activated.",
            "manual" => "Manual proxy configuration saved and activated.",
            _ => "Proxy configuration saved. Direct network connections are active.",
        },
    )
}

fn proxy_policy_denial(mode: &str) -> Option<(StatusCode, Json<ProxyModeOutcome>)> {
    let normalized = normalize_proxy_mode(mode);
    let reported_mode = if normalized == "invalid" {
        "none"
    } else {
        normalized
    };
    match policy_state_for_control("settings-control") {
        PolicyControlState::Allowed => None,
        PolicyControlState::Denied => Some(proxy_mode_outcome(
            StatusCode::FORBIDDEN,
            reported_mode,
            "Changing proxy settings is blocked by the active Goblins OS policy profile.",
        )),
        PolicyControlState::PermissionGated => Some(proxy_mode_outcome(
            StatusCode::FORBIDDEN,
            reported_mode,
            "Changing proxy settings requires an explicit Goblins OS permission review first.",
        )),
    }
}

fn validate_proxy_settings(
    request: SetProxySettingsRequest,
) -> Result<ValidatedProxySettings, String> {
    let mode = normalize_proxy_mode(&request.mode);
    if mode == "invalid" {
        return Err(
            "Proxy mode expects Off, Automatic, or Manual. No proxy settings were changed."
                .to_string(),
        );
    }

    let autoconfig_url = request
        .autoconfig_url
        .unwrap_or_default()
        .trim()
        .to_string();
    validate_proxy_url(&autoconfig_url)?;

    let ignore_hosts = validate_proxy_bypass_hosts(request.ignore_hosts)?;
    let http = validate_proxy_endpoint("HTTP", request.http)?;
    let https = validate_proxy_endpoint("HTTPS", request.https)?;
    let ftp = validate_proxy_endpoint("FTP", request.ftp)?;
    let socks = validate_proxy_endpoint("SOCKS", request.socks)?;

    if mode == "auto" && autoconfig_url.is_empty() {
        return Err(
            "Automatic mode requires an HTTP or HTTPS configuration URL. No proxy settings were changed."
                .to_string(),
        );
    }
    if mode == "manual"
        && [&http, &https, &ftp, &socks]
            .iter()
            .all(|endpoint| endpoint.host.is_empty())
    {
        return Err(
            "Manual mode requires at least one complete proxy host and port. No proxy settings were changed."
                .to_string(),
        );
    }

    Ok(ValidatedProxySettings {
        mode,
        autoconfig_url,
        ignore_hosts,
        http,
        https,
        ftp,
        socks,
    })
}

fn validate_proxy_url(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > PROXY_URL_MAX_BYTES || value.chars().any(char::is_control) {
        return Err(format!(
            "The automatic configuration URL must be {PROXY_URL_MAX_BYTES} bytes or fewer and cannot contain control characters. No proxy settings were changed."
        ));
    }
    if value.chars().any(char::is_whitespace) {
        return Err(
            "The automatic configuration URL cannot contain whitespace. No proxy settings were changed."
                .to_string(),
        );
    }

    let uri = value.parse::<axum::http::Uri>().map_err(|_| {
        "Enter a complete HTTP or HTTPS automatic configuration URL. No proxy settings were changed."
            .to_string()
    })?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_none() {
        return Err(
            "Automatic configuration URLs must use HTTP or HTTPS and include a host. No proxy settings were changed."
                .to_string(),
        );
    }
    if uri
        .authority()
        .is_some_and(|authority| authority.as_str().contains('@'))
        || uri.query().is_some()
    {
        return Err(
            "Automatic configuration URLs cannot contain credentials or query secrets. Use the system network tool for authenticated proxies. No proxy settings were changed."
                .to_string(),
        );
    }
    Ok(())
}

fn validate_proxy_bypass_hosts(values: Vec<String>) -> Result<Vec<String>, String> {
    if values.len() > PROXY_BYPASS_MAX_ENTRIES {
        return Err(format!(
            "The bypass list accepts at most {PROXY_BYPASS_MAX_ENTRIES} entries. No proxy settings were changed."
        ));
    }

    let mut normalized = Vec::with_capacity(values.len());
    let mut total_bytes = 0usize;
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            return Err(
                "Bypass entries cannot be empty. No proxy settings were changed.".to_string(),
            );
        }
        if value.len() > PROXY_BYPASS_ENTRY_MAX_BYTES || !proxy_bypass_entry_is_safe(value) {
            return Err(format!(
                "Each bypass entry must be a host, domain pattern, IP address, or CIDR no longer than {PROXY_BYPASS_ENTRY_MAX_BYTES} bytes. No proxy settings were changed."
            ));
        }
        total_bytes = total_bytes.saturating_add(value.len());
        if total_bytes > PROXY_BYPASS_TOTAL_MAX_BYTES {
            return Err(format!(
                "The bypass list must be {PROXY_BYPASS_TOTAL_MAX_BYTES} bytes or fewer. No proxy settings were changed."
            ));
        }
        if normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(value))
        {
            return Err(format!(
                "The bypass entry {value} is listed more than once. No proxy settings were changed."
            ));
        }
        normalized.push(value.to_string());
    }
    Ok(normalized)
}

fn proxy_bypass_entry_is_safe(value: &str) -> bool {
    if value == "<local>" {
        return true;
    }

    if let Some((address, prefix)) = value.split_once('/') {
        if prefix.is_empty() || prefix.bytes().any(|byte| !byte.is_ascii_digit()) {
            return false;
        }
        let Ok(prefix) = prefix.parse::<u8>() else {
            return false;
        };
        return address
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| match address {
                std::net::IpAddr::V4(_) => prefix <= 32,
                std::net::IpAddr::V6(_) => prefix <= 128,
            });
    }

    let candidate = value
        .strip_prefix("*.")
        .or_else(|| value.strip_prefix('.'))
        .unwrap_or(value);
    if candidate.starts_with('[') && candidate.ends_with(']') {
        return candidate[1..candidate.len() - 1]
            .parse::<std::net::Ipv6Addr>()
            .is_ok();
    }
    proxy_host_is_safe(candidate)
}

fn validate_proxy_endpoint(
    label: &str,
    request: ProxyEndpointRequest,
) -> Result<ValidatedProxyEndpoint, String> {
    let mut host = request.host.unwrap_or_default().trim().to_string();
    match (host.is_empty(), request.port) {
        (true, None) => return Ok(ValidatedProxyEndpoint::default()),
        (true, Some(_)) => {
            return Err(format!(
                "The {label} proxy needs a host when a port is set. No proxy settings were changed."
            ));
        }
        (false, None) => {
            return Err(format!(
                "The {label} proxy needs a port when a host is set. No proxy settings were changed."
            ));
        }
        (false, Some(0)) => {
            return Err(format!(
                "The {label} proxy port must be between 1 and 65535. No proxy settings were changed."
            ));
        }
        (false, Some(_)) => {}
    }

    if host.starts_with('[') && host.ends_with(']') {
        let candidate = &host[1..host.len() - 1];
        if candidate.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(format!(
                "The {label} proxy host is not a valid hostname or IP address. No proxy settings were changed."
            ));
        }
        host = candidate.to_string();
    }
    if !proxy_host_is_safe(&host) {
        return Err(format!(
            "The {label} proxy host is not a valid hostname or IP address. Enter the host without a scheme, path, or credentials. No proxy settings were changed."
        ));
    }

    Ok(ValidatedProxyEndpoint {
        host,
        port: request.port.unwrap_or_default(),
    })
}

fn proxy_host_is_safe(host: &str) -> bool {
    if host.is_empty()
        || host.len() > PROXY_HOST_MAX_BYTES
        || host.chars().any(char::is_control)
        || host.chars().any(char::is_whitespace)
    {
        return false;
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    if host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    })
}

fn proxy_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn proxy_settings_values(settings: &ValidatedProxySettings) -> Vec<(ProxyKey, String)> {
    vec![
        (ProxyKey::Mode, encode_gvariant_string(settings.mode)),
        (
            ProxyKey::AutoconfigUrl,
            encode_gvariant_string(&settings.autoconfig_url),
        ),
        (
            ProxyKey::IgnoreHosts,
            encode_gvariant_strv(&settings.ignore_hosts),
        ),
        (
            ProxyKey::HttpHost,
            encode_gvariant_string(&settings.http.host),
        ),
        (ProxyKey::HttpPort, settings.http.port.to_string()),
        (
            ProxyKey::HttpsHost,
            encode_gvariant_string(&settings.https.host),
        ),
        (ProxyKey::HttpsPort, settings.https.port.to_string()),
        (
            ProxyKey::FtpHost,
            encode_gvariant_string(&settings.ftp.host),
        ),
        (ProxyKey::FtpPort, settings.ftp.port.to_string()),
        (
            ProxyKey::SocksHost,
            encode_gvariant_string(&settings.socks.host),
        ),
        (ProxyKey::SocksPort, settings.socks.port.to_string()),
    ]
}

fn read_proxy_snapshot() -> Result<Vec<(ProxyKey, String)>, GSettingsError> {
    PROXY_KEYS
        .iter()
        .copied()
        .map(|key| read_proxy_key(key).map(|value| (key, value)))
        .collect()
}

fn proxy_snapshot_matches(previous: &[(ProxyKey, String)], desired: &[(ProxyKey, String)]) -> bool {
    desired.iter().all(|(key, value)| {
        previous
            .iter()
            .find(|(candidate, _)| candidate == key)
            .is_some_and(|(_, previous)| previous == value)
    })
}

fn apply_proxy_values(
    previous: &[(ProxyKey, String)],
    desired: &[(ProxyKey, String)],
) -> Result<(), ProxyKey> {
    let non_mode_changed = desired.iter().any(|(key, value)| {
        *key != ProxyKey::Mode
            && previous
                .iter()
                .find(|(candidate, _)| candidate == key)
                .is_none_or(|(_, previous)| previous != value)
    });
    let previous_mode = previous
        .iter()
        .find(|(key, _)| *key == ProxyKey::Mode)
        .map(|(_, value)| normalize_proxy_mode(value));
    let paused = non_mode_changed && previous_mode != Some("none");
    if paused {
        write_proxy_key(ProxyKey::Mode, "'none'").map_err(|_| ProxyKey::Mode)?;
    }

    for (key, value) in desired.iter().filter(|(key, _)| *key != ProxyKey::Mode) {
        let unchanged = previous
            .iter()
            .find(|(candidate, _)| candidate == key)
            .is_some_and(|(_, previous)| previous == value);
        if !unchanged {
            write_proxy_key(*key, value).map_err(|_| *key)?;
        }
    }

    let desired_mode = desired
        .iter()
        .find(|(key, _)| *key == ProxyKey::Mode)
        .expect("proxy settings always include mode");
    let mode_unchanged = previous
        .iter()
        .find(|(key, _)| *key == ProxyKey::Mode)
        .is_some_and(|(_, previous)| previous == &desired_mode.1);
    if paused || !mode_unchanged {
        write_proxy_key(ProxyKey::Mode, &desired_mode.1).map_err(|_| ProxyKey::Mode)?;
    }
    Ok(())
}

fn restore_proxy_snapshot(previous: &[(ProxyKey, String)]) -> Result<(), ()> {
    let _ = write_proxy_key(ProxyKey::Mode, "'none'");
    let mut restored = true;
    for (key, value) in previous.iter().filter(|(key, _)| *key != ProxyKey::Mode) {
        restored &= write_proxy_key(*key, value).is_ok();
    }
    if let Some((_, mode)) = previous.iter().find(|(key, _)| *key == ProxyKey::Mode) {
        restored &= write_proxy_key(ProxyKey::Mode, mode).is_ok();
    } else {
        restored = false;
    }
    restored.then_some(()).ok_or(())
}

fn proxy_settings_match(settings: &ValidatedProxySettings) -> bool {
    proxy_string(PROXY_SCHEMA, "mode")
        .is_some_and(|mode| normalize_proxy_mode(&mode) == settings.mode)
        && proxy_string(PROXY_SCHEMA, "autoconfig-url").unwrap_or_default()
            == settings.autoconfig_url
        && proxy_strv(PROXY_SCHEMA, "ignore-hosts").unwrap_or_default() == settings.ignore_hosts
        && proxy_endpoint_matches(HTTP_PROXY_SCHEMA, &settings.http)
        && proxy_endpoint_matches(HTTPS_PROXY_SCHEMA, &settings.https)
        && proxy_endpoint_matches(FTP_PROXY_SCHEMA, &settings.ftp)
        && proxy_endpoint_matches(SOCKS_PROXY_SCHEMA, &settings.socks)
}

fn proxy_endpoint_matches(schema: &str, expected: &ValidatedProxyEndpoint) -> bool {
    proxy_string(schema, "host").unwrap_or_default() == expected.host
        && proxy_int(schema, "port").unwrap_or_default() == i32::from(expected.port)
}

fn proxy_mode_outcome(
    status: StatusCode,
    mode: &str,
    text: &str,
) -> (StatusCode, Json<ProxyModeOutcome>) {
    (
        status,
        Json(ProxyModeOutcome {
            ok: status == StatusCode::OK,
            mode: mode.to_string(),
            text: text.to_string(),
        }),
    )
}

fn status_detail(connectivity: &str, active: Option<&ActiveConnection>) -> String {
    match connectivity {
        "full" => match active {
            Some(active) => format!("Online via {} ({}).", active.name, active.kind),
            None => "Online.".to_string(),
        },
        "limited" => "Connected, but the internet is not reachable yet.".to_string(),
        "portal" => "A sign-in page must be completed to reach the internet.".to_string(),
        "none" => "Not connected to the internet.".to_string(),
        _ => "Connectivity is unknown.".to_string(),
    }
}

fn proxy_endpoint(schema: &str) -> ProxyEndpoint {
    ProxyEndpoint {
        host: proxy_string(schema, "host"),
        port: proxy_int(schema, "port"),
    }
}

impl ProxyKey {
    fn schema(self) -> &'static str {
        match self {
            Self::Mode | Self::AutoconfigUrl | Self::IgnoreHosts => PROXY_SCHEMA,
            Self::HttpHost | Self::HttpPort => HTTP_PROXY_SCHEMA,
            Self::HttpsHost | Self::HttpsPort => HTTPS_PROXY_SCHEMA,
            Self::FtpHost | Self::FtpPort => FTP_PROXY_SCHEMA,
            Self::SocksHost | Self::SocksPort => SOCKS_PROXY_SCHEMA,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Mode => "mode",
            Self::AutoconfigUrl => "autoconfig-url",
            Self::IgnoreHosts => "ignore-hosts",
            Self::HttpHost | Self::HttpsHost | Self::FtpHost | Self::SocksHost => "host",
            Self::HttpPort | Self::HttpsPort | Self::FtpPort | Self::SocksPort => "port",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Mode => "proxy mode",
            Self::AutoconfigUrl => "automatic configuration URL",
            Self::IgnoreHosts => "bypass list",
            Self::HttpHost | Self::HttpPort => "HTTP proxy",
            Self::HttpsHost | Self::HttpsPort => "HTTPS proxy",
            Self::FtpHost | Self::FtpPort => "FTP proxy",
            Self::SocksHost | Self::SocksPort => "SOCKS proxy",
        }
    }
}

fn proxy_key_available(key: ProxyKey) -> bool {
    schema_available(key.schema()) && key_available(key.schema(), key.key())
}

fn proxy_key_writable(key: ProxyKey) -> bool {
    gsettings(&["writable", key.schema(), key.key()])
        .map(|value| value.trim() == "true")
        .unwrap_or(false)
}

fn read_proxy_key(key: ProxyKey) -> Result<String, GSettingsError> {
    gsettings(&["get", key.schema(), key.key()]).map(|value| value.trim().to_string())
}

fn write_proxy_key(key: ProxyKey, encoded_value: &str) -> Result<(), GSettingsError> {
    gsettings(&["set", key.schema(), key.key(), encoded_value]).map(|_| ())
}

fn encode_gvariant_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len().saturating_add(2));
    encoded.push('\'');
    for character in value.chars() {
        match character {
            '\\' => encoded.push_str("\\\\"),
            '\'' => encoded.push_str("\\'"),
            _ => encoded.push(character),
        }
    }
    encoded.push('\'');
    encoded
}

fn encode_gvariant_strv(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| encode_gvariant_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn proxy_string(schema: &str, key: &str) -> Option<String> {
    key_available(schema, key)
        .then(|| gsettings(&["get", schema, key]).ok())
        .flatten()
        .and_then(|value| parse_gsettings_string(&value))
}

fn proxy_int(schema: &str, key: &str) -> Option<i32> {
    key_available(schema, key)
        .then(|| gsettings(&["get", schema, key]).ok())
        .flatten()
        .and_then(|value| parse_gsettings_i32(&value))
}

fn proxy_strv(schema: &str, key: &str) -> Option<Vec<String>> {
    key_available(schema, key)
        .then(|| gsettings(&["get", schema, key]).ok())
        .flatten()
        .map(|value| parse_gsettings_strv(&value))
}

fn schema_available(schema: &str) -> bool {
    gsettings(&["list-schemas"])
        .map(|stdout| stdout.lines().any(|line| line.trim() == schema))
        .unwrap_or(false)
}

fn key_available(schema: &str, key: &str) -> bool {
    gsettings(&["list-keys", schema])
        .map(|stdout| stdout.lines().any(|line| line.trim() == key))
        .unwrap_or(false)
}

fn normalize_proxy_mode(mode: &str) -> &'static str {
    match mode.trim().trim_matches('\'') {
        "none" | "off" | "direct" => "none",
        "auto" | "automatic" => "auto",
        "manual" => "manual",
        _ => "invalid",
    }
}

fn proxy_detail(
    gsettings_available: bool,
    schema_available: bool,
    mode_available: bool,
    editor_available: bool,
    mode: &str,
) -> String {
    if !gsettings_available {
        return "Desktop preferences are not ready, so proxy settings are read-only in this session."
            .to_string();
    }
    if !schema_available {
        return "The standard proxy preferences are not supported in this session.".to_string();
    }
    if !mode_available {
        return "The standard proxy mode preference is not supported in this session.".to_string();
    }
    if !editor_available {
        return "Proxy settings can be viewed, but one or more standard fields are missing or managed by the system administrator."
            .to_string();
    }
    proxy_mode_detail(mode).to_string()
}

fn proxy_mode_detail(mode: &str) -> &'static str {
    match normalize_proxy_mode(mode) {
        "auto" => "Automatic proxy configuration is active.",
        "manual" => "Manual proxy addresses are active.",
        _ => "Direct network connections are active; no desktop proxy is configured.",
    }
}

fn parse_gsettings_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let unquoted = trimmed
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .unwrap_or(trimmed);
    let mut decoded = String::with_capacity(unquoted.len());
    let mut escaping = false;
    for character in unquoted.chars() {
        if escaping {
            decoded.push(character);
            escaping = false;
        } else if character == '\\' {
            escaping = true;
        } else {
            decoded.push(character);
        }
    }
    if escaping {
        decoded.push('\\');
    }
    Some(decoded)
}

fn parse_gsettings_i32(value: &str) -> Option<i32> {
    value
        .split_whitespace()
        .rev()
        .find_map(|token| token.trim_matches('\'').parse::<i32>().ok())
}

fn parse_gsettings_strv(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "[]" || trimmed.starts_with("@as []") {
        return Vec::new();
    }

    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaping = false;
    for ch in trimmed.chars() {
        if !in_string {
            if ch == '\'' {
                in_string = true;
            }
            continue;
        }
        if escaping {
            current.push(ch);
            escaping = false;
            continue;
        }
        match ch {
            '\\' => escaping = true,
            '\'' => {
                values.push(std::mem::take(&mut current));
                in_string = false;
            }
            _ => current.push(ch),
        }
    }
    values
}

fn gsettings(args: &[&str]) -> Result<String, GSettingsError> {
    match session_bridge::gsettings(args) {
        SessionBridgeResult::Success(stdout) => Ok(stdout),
        SessionBridgeResult::Failed(_) => Err(GSettingsError::Failed),
        SessionBridgeResult::Unavailable => Err(GSettingsError::Missing),
    }
}

/// Parse `nmcli -t -f STATE,CONNECTIVITY general` ("connected:full").
fn parse_general_status(stdout: &str) -> (String, String) {
    let line = stdout.lines().next().unwrap_or("");
    let fields = split_terse(line);
    let state = fields.first().cloned().unwrap_or_default();
    let connectivity = fields
        .get(1)
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    (
        if state.is_empty() {
            "unknown".to_string()
        } else {
            state
        },
        if connectivity.is_empty() {
            "unknown".to_string()
        } else {
            connectivity
        },
    )
}

/// Parse `nmcli -t -f NAME,TYPE,DEVICE connection show --active`, returning the
/// first active non-loopback connection mapped to a friendly kind.
fn parse_active_connection(stdout: &str) -> Option<ActiveConnection> {
    for line in stdout.lines() {
        let fields = split_terse(line);
        let name = fields.first().cloned().unwrap_or_default();
        let kind = fields.get(1).cloned().unwrap_or_default();
        let device = fields.get(2).cloned().unwrap_or_default();
        if name.is_empty() || kind == "loopback" {
            continue;
        }
        return Some(ActiveConnection {
            name,
            kind: friendly_kind(&kind),
            device,
        });
    }
    None
}

fn friendly_kind(kind: &str) -> String {
    match kind {
        "802-11-wireless" | "wifi" => "Wi-Fi".to_string(),
        "802-3-ethernet" | "ethernet" => "Ethernet".to_string(),
        other => other.to_string(),
    }
}

/// Parse `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY device wifi list`. Hidden
/// (empty-SSID) entries are skipped; duplicates collapse to the strongest signal;
/// the result is sorted strongest-first with the active network pinned on top.
fn parse_wifi_list(stdout: &str) -> Vec<WifiNetwork> {
    let mut networks: Vec<WifiNetwork> = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_terse(line);
        let in_use = fields.first().map(|f| f == "*").unwrap_or(false);
        let ssid = fields.get(1).cloned().unwrap_or_default();
        if ssid.is_empty() {
            continue;
        }
        let signal = fields
            .get(2)
            .and_then(|s| s.trim().parse::<u8>().ok())
            .unwrap_or(0);
        let security = normalize_security(fields.get(3).map(String::as_str).unwrap_or(""));

        match networks.iter_mut().find(|n| n.ssid == ssid) {
            Some(existing) => {
                if signal > existing.signal {
                    existing.signal = signal;
                    existing.security = security;
                }
                existing.in_use = existing.in_use || in_use;
            }
            None => networks.push(WifiNetwork {
                ssid,
                signal,
                security,
                in_use,
            }),
        }
    }
    networks.sort_by(|a, b| {
        b.in_use
            .cmp(&a.in_use)
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
    });
    networks
}

fn normalize_security(security: &str) -> String {
    let trimmed = security.trim();
    if trimmed.is_empty() || trimmed == "--" {
        String::new()
    } else {
        trimmed.to_string()
    }
}

/// Split one `nmcli -t` terse line into fields, honoring NetworkManager's
/// backslash escaping of `:` and `\` within a field.
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(&next) = chars.peek() {
                    current.push(next);
                    chars.next();
                }
            }
            ':' => fields.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    fields.push(current);
    fields
}

#[cfg(test)]
mod tests {
    use super::{
        encode_gvariant_string, encode_gvariant_strv, nmcli_result, normalize_proxy_mode,
        parse_active_connection, parse_general_status, parse_gsettings_i32, parse_gsettings_string,
        parse_gsettings_strv, parse_wifi_list, proxy_settings_values, sanitize_connect_error,
        split_terse, ssid_looks_like_option, validate_proxy_settings, ActiveConnection, NmcliError,
        ProxyEndpointRequest, ProxyKey, SetProxySettingsRequest, FTP_PROXY_SCHEMA,
        HTTPS_PROXY_SCHEMA, HTTP_PROXY_SCHEMA, NETWORK_TIMEOUT_DETAIL, PROXY_SCHEMA,
        SOCKS_PROXY_SCHEMA,
    };
    use crate::bounded::BoundedCommandError;

    #[test]
    fn nmcli_timeout_reports_failure_not_missing() {
        // A missing binary is the only honest "Missing"; a timeout means the
        // tool IS present but did not answer, so it must surface as a failure.
        assert!(matches!(
            nmcli_result(Err(BoundedCommandError::Missing)),
            Err(NmcliError::Missing)
        ));
        match nmcli_result(Err(BoundedCommandError::TimedOut)) {
            Err(NmcliError::Failed(detail)) => assert_eq!(detail, NETWORK_TIMEOUT_DETAIL),
            _ => panic!("a timed-out nmcli must be reported as a failure, not a missing tool"),
        }
        match nmcli_result(Err(BoundedCommandError::Failed)) {
            Err(NmcliError::Failed(detail)) => assert!(!detail.is_empty()),
            _ => panic!("an unrunnable nmcli must be reported as a failure, not a missing tool"),
        }
    }

    #[test]
    fn terse_lines_honor_backslash_escapes() {
        assert_eq!(split_terse("connected:full"), vec!["connected", "full"]);
        // An escaped colon inside an SSID must stay one field.
        assert_eq!(
            split_terse(r"*:Cafe\: Free:72:WPA2"),
            vec!["*", "Cafe: Free", "72", "WPA2"]
        );
    }

    #[test]
    fn general_status_parses_state_and_connectivity() {
        assert_eq!(
            parse_general_status("connected:full\n"),
            ("connected".to_string(), "full".to_string())
        );
        assert_eq!(
            parse_general_status(""),
            ("unknown".to_string(), "unknown".to_string())
        );
    }

    #[test]
    fn active_connection_skips_loopback_and_maps_kind() {
        let stdout = "lo:loopback:lo\nHome Wi-Fi:802-11-wireless:wlan0\n";
        assert_eq!(
            parse_active_connection(stdout),
            Some(ActiveConnection {
                name: "Home Wi-Fi".to_string(),
                kind: "Wi-Fi".to_string(),
                device: "wlan0".to_string(),
            })
        );
    }

    #[test]
    fn wifi_list_dedupes_sorts_and_pins_active() {
        let stdout = "\
 :FarNet:30:WPA2
*:HomeNet:64:WPA2
 :HomeNet:80:WPA2
 :OpenCafe:55:
";
        let networks = parse_wifi_list(stdout);
        // Active pinned first even though its raw signal (64) is lower than its
        // own stronger duplicate (80), which is merged into it.
        assert_eq!(networks[0].ssid, "HomeNet");
        assert!(networks[0].in_use);
        assert_eq!(networks[0].signal, 80);
        // Remaining sorted strongest-first; open network reports empty security.
        assert_eq!(networks[1].ssid, "OpenCafe");
        assert_eq!(networks[1].security, "");
        assert_eq!(networks[2].ssid, "FarNet");
    }

    #[test]
    fn option_shaped_ssids_are_rejected() {
        // A leading '-' would let an SSID be parsed by nmcli as an option, so it is
        // rejected before ever reaching the command line.
        assert!(ssid_looks_like_option("-x"));
        assert!(ssid_looks_like_option("--rescan"));
        // Ordinary names — including ones that merely contain a dash — are fine.
        assert!(!ssid_looks_like_option("HomeNet"));
        assert!(!ssid_looks_like_option("Cafe-Free"));
        assert!(!ssid_looks_like_option("password"));
    }

    #[test]
    fn connect_errors_never_leak_the_password() {
        let leaked =
            sanitize_connect_error("Error: 802-11-wireless-security.psk: 'hunter2'", "hunter2");
        assert!(!leaked.contains("hunter2"));
        // An empty / missing detail still yields calm guidance.
        assert!(sanitize_connect_error("", "hunter2").contains("Check the password"));
    }

    #[test]
    fn proxy_modes_are_normalized_to_gnome_values() {
        assert_eq!(normalize_proxy_mode("none"), "none");
        assert_eq!(normalize_proxy_mode("'none'"), "none");
        assert_eq!(normalize_proxy_mode("off"), "none");
        assert_eq!(normalize_proxy_mode("auto"), "auto");
        assert_eq!(normalize_proxy_mode("automatic"), "auto");
        assert_eq!(normalize_proxy_mode("manual"), "manual");
        assert_eq!(normalize_proxy_mode("pac"), "invalid");
    }

    #[test]
    fn proxy_gsettings_values_parse() {
        assert_eq!(
            parse_gsettings_string("'https://proxy.example/proxy.pac'\n"),
            Some("https://proxy.example/proxy.pac".to_string())
        );
        assert_eq!(parse_gsettings_string("''"), Some(String::new()));
        assert_eq!(parse_gsettings_i32("uint32 8080"), Some(8080));
        assert_eq!(parse_gsettings_i32("8080"), Some(8080));
        assert_eq!(
            parse_gsettings_strv("['localhost', '127.0.0.0/8', '::1']"),
            vec!["localhost", "127.0.0.0/8", "::1"]
        );
        assert!(parse_gsettings_strv("@as []").is_empty());
    }

    fn proxy_request(mode: &str) -> SetProxySettingsRequest {
        SetProxySettingsRequest {
            mode: mode.to_string(),
            autoconfig_url: Some("https://proxy.example/proxy.pac".to_string()),
            ignore_hosts: vec!["localhost".to_string(), "127.0.0.0/8".to_string()],
            http: ProxyEndpointRequest {
                host: Some("proxy.example".to_string()),
                port: Some(8080),
            },
            https: ProxyEndpointRequest {
                host: Some("2001:db8::10".to_string()),
                port: Some(8443),
            },
            ftp: ProxyEndpointRequest {
                host: None,
                port: None,
            },
            socks: ProxyEndpointRequest {
                host: None,
                port: None,
            },
        }
    }

    #[test]
    fn proxy_editor_validates_the_complete_request_before_writes() {
        let automatic = validate_proxy_settings(proxy_request("auto")).unwrap();
        assert_eq!(automatic.mode, "auto");
        assert_eq!(automatic.http.port, 8080);
        assert_eq!(automatic.https.host, "2001:db8::10");

        let mut missing_port = proxy_request("manual");
        missing_port.http.port = None;
        assert!(validate_proxy_settings(missing_port)
            .unwrap_err()
            .contains("HTTP proxy needs a port"));

        let mut secret_url = proxy_request("auto");
        secret_url.autoconfig_url = Some("https://user:secret@proxy.example/proxy.pac".to_string());
        assert!(validate_proxy_settings(secret_url)
            .unwrap_err()
            .contains("credentials or query secrets"));

        let mut duplicate_bypass = proxy_request("manual");
        duplicate_bypass.ignore_hosts = vec!["localhost".to_string(), "LOCALHOST".to_string()];
        assert!(validate_proxy_settings(duplicate_bypass)
            .unwrap_err()
            .contains("listed more than once"));
    }

    #[test]
    fn automatic_and_manual_modes_require_an_usable_configuration() {
        let mut automatic = proxy_request("auto");
        automatic.autoconfig_url = None;
        assert!(validate_proxy_settings(automatic)
            .unwrap_err()
            .contains("Automatic mode requires"));

        let mut manual = proxy_request("manual");
        manual.http = ProxyEndpointRequest {
            host: None,
            port: None,
        };
        manual.https = manual.http.clone();
        assert!(validate_proxy_settings(manual)
            .unwrap_err()
            .contains("Manual mode requires"));
    }

    #[test]
    fn proxy_gvariant_encoding_escapes_strings_and_arrays() {
        assert_eq!(
            encode_gvariant_string("proxy\\branch's host"),
            "'proxy\\\\branch\\'s host'"
        );
        assert_eq!(
            encode_gvariant_strv(&["localhost".to_string(), "branch's host".to_string()]),
            "['localhost', 'branch\\'s host']"
        );
    }

    #[test]
    fn proxy_key_allowlist_is_exact_and_excludes_authentication() {
        let matrix = [
            (ProxyKey::Mode, PROXY_SCHEMA, "mode"),
            (ProxyKey::AutoconfigUrl, PROXY_SCHEMA, "autoconfig-url"),
            (ProxyKey::IgnoreHosts, PROXY_SCHEMA, "ignore-hosts"),
            (ProxyKey::HttpHost, HTTP_PROXY_SCHEMA, "host"),
            (ProxyKey::HttpPort, HTTP_PROXY_SCHEMA, "port"),
            (ProxyKey::HttpsHost, HTTPS_PROXY_SCHEMA, "host"),
            (ProxyKey::HttpsPort, HTTPS_PROXY_SCHEMA, "port"),
            (ProxyKey::FtpHost, FTP_PROXY_SCHEMA, "host"),
            (ProxyKey::FtpPort, FTP_PROXY_SCHEMA, "port"),
            (ProxyKey::SocksHost, SOCKS_PROXY_SCHEMA, "host"),
            (ProxyKey::SocksPort, SOCKS_PROXY_SCHEMA, "port"),
        ];
        for (key, schema, name) in matrix {
            assert_eq!(key.schema(), schema);
            assert_eq!(key.key(), name);
            assert_ne!(name, "authentication-user");
            assert_ne!(name, "authentication-password");
            assert_ne!(name, "use-authentication");
        }
    }

    #[test]
    fn proxy_write_plan_contains_only_the_allowlisted_non_secret_fields() {
        let settings = validate_proxy_settings(proxy_request("manual")).unwrap();
        let values = proxy_settings_values(&settings);
        assert_eq!(values.len(), 11);
        assert!(values.iter().all(|(key, value)| {
            !key.key().contains("authentication")
                && !value.contains("password")
                && !value.contains("secret")
        }));
    }

    #[test]
    fn proxy_request_rejects_unknown_or_credential_fields() {
        let unknown = serde_json::from_str::<SetProxySettingsRequest>(
            r#"{
                "mode":"none",
                "autoconfig_url":null,
                "ignore_hosts":[],
                "http":{"host":null,"port":null,"password":"secret"},
                "https":{"host":null,"port":null},
                "ftp":{"host":null,"port":null},
                "socks":{"host":null,"port":null}
            }"#,
        );
        assert!(unknown.is_err());
    }
}
