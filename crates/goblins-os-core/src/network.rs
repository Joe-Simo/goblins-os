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

use std::process::Command;

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::policy::{policy_state_for_control, PolicyControlState};

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

#[derive(Deserialize)]
pub struct SetProxyModeRequest {
    mode: String,
}

#[derive(Serialize)]
pub struct ProxyModeOutcome {
    ok: bool,
    mode: String,
    text: String,
}

enum NmcliError {
    /// NetworkManager's CLI is not present (e.g. a container or pre-NM stage).
    Missing,
    /// The CLI ran but reported an error; the (credential-free) message is kept.
    Failed(String),
}

enum GSettingsError {
    Missing,
    Failed(String),
}

/// Run `nmcli` with the given arguments, capturing stdout on success. A Wi-Fi
/// password may be among the args; this function never logs the arguments.
fn nmcli(args: &[&str]) -> Result<String, NmcliError> {
    match Command::new("nmcli").args(args).output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(NmcliError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(NmcliError::Missing),
        Err(_) => Err(NmcliError::Missing),
    }
}

pub async fn network_status() -> Json<NetworkStatus> {
    Json(build_network_status())
}

pub async fn wifi_scan() -> Json<WifiScan> {
    // A rescan is best-effort; the cached list is returned regardless so a slow
    // or rate-limited radio still yields whatever NetworkManager already knows.
    let _ = nmcli(&["device", "wifi", "rescan"]);
    match nmcli(&[
        "-t",
        "-f",
        "IN-USE,SSID,SIGNAL,SECURITY",
        "device",
        "wifi",
        "list",
    ]) {
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

    match nmcli(&args) {
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

pub async fn set_proxy_mode(
    Json(request): Json<SetProxyModeRequest>,
) -> (StatusCode, Json<ProxyModeOutcome>) {
    set_proxy_mode_outcome(&request.mode)
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
        autoconfig_url: proxy_string(PROXY_SCHEMA, "autoconfig-url"),
        ignore_hosts: proxy_strv(PROXY_SCHEMA, "ignore-hosts").unwrap_or_default(),
        http: proxy_endpoint(HTTP_PROXY_SCHEMA),
        https: proxy_endpoint(HTTPS_PROXY_SCHEMA),
        ftp: proxy_endpoint(FTP_PROXY_SCHEMA),
        socks: proxy_endpoint(SOCKS_PROXY_SCHEMA),
        detail: proxy_detail(gsettings_available, schema_available, mode_available, &mode),
        mode,
    }
}

fn set_proxy_mode_outcome(mode: &str) -> (StatusCode, Json<ProxyModeOutcome>) {
    let normalized = normalize_proxy_mode(mode);
    if normalized == "invalid" {
        return proxy_mode_outcome(
            StatusCode::BAD_REQUEST,
            "none",
            "Proxy mode expects Off, Automatic, or Manual.",
        );
    }

    if gsettings(&["list-schemas"]).is_err() {
        return proxy_mode_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "Desktop preferences are not ready, so proxy mode cannot be changed in this session.",
        );
    }

    if !schema_available(PROXY_SCHEMA) || !key_available(PROXY_SCHEMA, "mode") {
        return proxy_mode_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "The standard proxy mode preference is not supported in this session.",
        );
    }

    match gsettings(&["set", PROXY_SCHEMA, "mode", normalized]) {
        Ok(_) => proxy_mode_outcome(StatusCode::OK, normalized, proxy_mode_detail(normalized)),
        Err(GSettingsError::Missing) => proxy_mode_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            normalized,
            "Desktop preferences are not ready, so proxy mode cannot be changed in this session.",
        ),
        Err(GSettingsError::Failed(detail)) => proxy_mode_outcome(
            StatusCode::BAD_GATEWAY,
            normalized,
            &if detail.is_empty() {
                "The desktop session could not save the proxy mode.".to_string()
            } else {
                format!("The desktop session could not save the proxy mode: {detail}")
            },
        ),
    }
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
    proxy_mode_detail(mode).to_string()
}

fn proxy_mode_detail(mode: &str) -> &'static str {
    match normalize_proxy_mode(mode) {
        "auto" => "Automatic proxy configuration is active.",
        "manual" => "Manual proxy endpoints are active.",
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
    Some(unquoted.to_string())
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
    match Command::new("gsettings").args(args).output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(GSettingsError::Failed(gsettings_error_detail(
            &String::from_utf8_lossy(&output.stderr),
            &String::from_utf8_lossy(&output.stdout),
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(GSettingsError::Missing),
        Err(_) => Err(GSettingsError::Missing),
    }
}

fn gsettings_error_detail(stderr: &str, stdout: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }
    stdout.trim().to_string()
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
        normalize_proxy_mode, parse_active_connection, parse_general_status, parse_gsettings_i32,
        parse_gsettings_string, parse_gsettings_strv, parse_wifi_list, sanitize_connect_error,
        split_terse, ssid_looks_like_option, ActiveConnection,
    };

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
}
