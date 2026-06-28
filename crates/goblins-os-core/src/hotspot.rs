//! Personal Hotspot status (NetworkManager).
//!
//! Goblins OS keeps Personal Hotspot behind core so Settings reads and writes an
//! honest NetworkManager-backed state. The write path is policy-gated, validates
//! the hotspot profile before calling `nmcli`, uses a non-persistent connection,
//! and never returns a password or raw `nmcli` command line.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::policy::{policy_state_for_control, PolicyControlState};

const HOTSPOT_CONNECTION_NAME: &str = "Goblins Hotspot";
const DEFAULT_HOTSPOT_SSID: &str = "Goblins OS";

#[derive(Serialize)]
pub struct HotspotStatus {
    source: &'static str,
    /// Whether the `nmcli` tool (NetworkManager) is present at all.
    available: bool,
    /// Whether a Wi-Fi connection is currently active in access-point (hotspot) mode.
    active: bool,
    /// The device sharing the connection, when a hotspot is active.
    device: Option<String>,
    /// Whether the local substrate can attempt to start the hotspot.
    can_start: bool,
    ssid: Option<String>,
    connected_clients_known: bool,
    connected_client_count: Option<usize>,
    connected_clients: Vec<HotspotClient>,
    detail: String,
}

#[derive(Serialize)]
pub struct HotspotClient {
    ip_address: String,
    hostname: Option<String>,
}

#[derive(Deserialize)]
pub struct SetHotspotRequest {
    enabled: bool,
    ssid: Option<String>,
    password: Option<String>,
}

#[derive(Serialize)]
pub struct HotspotOutcome {
    ok: bool,
    active: bool,
    ssid: Option<String>,
    text: String,
}

#[derive(Debug, PartialEq, Eq)]
struct NetworkDevice {
    device: String,
    kind: String,
    state: String,
}

#[derive(Debug, PartialEq, Eq)]
struct ActiveConnection {
    name: String,
    kind: String,
    device: String,
}

enum NmcliError {
    Missing,
    Failed(String),
}

pub async fn hotspot_status() -> Json<HotspotStatus> {
    Json(build_hotspot_status())
}

pub async fn set_hotspot_enabled(
    Json(request): Json<SetHotspotRequest>,
) -> (StatusCode, Json<HotspotOutcome>) {
    set_hotspot_enabled_outcome(request)
}

fn build_hotspot_status() -> HotspotStatus {
    if !nmcli_present() {
        return HotspotStatus {
            source: "goblins-os-core",
            available: false,
            active: false,
            device: None,
            can_start: false,
            ssid: None,
            connected_clients_known: false,
            connected_client_count: None,
            connected_clients: Vec::new(),
            detail: "Personal Hotspot status is unavailable on this device (NetworkManager is not installed).".to_string(),
        };
    }

    // Active connections as TYPE:DEVICE:UUID. UUID/DEVICE never contain colons, so the
    // terse output parses cleanly without nmcli's name-escaping rules.
    let active = nmcli(&[
        "-t",
        "-f",
        "TYPE,DEVICE,UUID",
        "connection",
        "show",
        "--active",
    ]);
    let hotspot = active.as_deref().and_then(|output| {
        active_wifi_devices(output).into_iter().find(|(_, uuid)| {
            // Each active Wi-Fi connection: is it an access point (hotspot)?
            nmcli(&[
                "-t",
                "-f",
                "802-11-wireless.mode",
                "connection",
                "show",
                uuid,
            ])
            .as_deref()
            .is_some_and(mode_is_ap)
        })
    });

    match hotspot {
        Some((device, _)) => {
            let clients = hotspot_clients_for_device(&device);
            HotspotStatus {
                source: "goblins-os-core",
                active: true,
                available: true,
                can_start: dnsmasq_present(),
                ssid: current_hotspot_ssid(),
                connected_clients_known: clients.is_some(),
                connected_client_count: clients.as_ref().map(Vec::len),
                connected_clients: clients.unwrap_or_default(),
                detail: format!("Personal Hotspot is on, sharing this connection over {device}."),
                device: Some(device),
            }
        }
        None => HotspotStatus {
            source: "goblins-os-core",
            available: true,
            active: false,
            device: None,
            can_start: dnsmasq_present(),
            ssid: None,
            connected_clients_known: false,
            connected_client_count: None,
            connected_clients: Vec::new(),
            detail: "Personal Hotspot is off. No Wi-Fi access point is currently active."
                .to_string(),
        },
    }
}

fn set_hotspot_enabled_outcome(request: SetHotspotRequest) -> (StatusCode, Json<HotspotOutcome>) {
    match policy_state_for_control("settings-control") {
        PolicyControlState::Allowed => {}
        PolicyControlState::Denied => {
            return hotspot_outcome(
                StatusCode::FORBIDDEN,
                false,
                None,
                "Changing Personal Hotspot is blocked by the active Goblins OS policy profile.",
            );
        }
        PolicyControlState::PermissionGated => {
            return hotspot_outcome(
                StatusCode::FORBIDDEN,
                false,
                None,
                "Changing Personal Hotspot requires an explicit Goblins OS permission review first.",
            );
        }
    }

    if !nmcli_present() {
        return hotspot_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            None,
            "Networking is not ready in this session, so Personal Hotspot cannot be changed here.",
        );
    }

    if request.enabled {
        start_hotspot(request)
    } else {
        stop_hotspot()
    }
}

fn start_hotspot(request: SetHotspotRequest) -> (StatusCode, Json<HotspotOutcome>) {
    if !dnsmasq_present() {
        return hotspot_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            request.ssid.as_deref().map(normalize_hotspot_ssid),
            "Personal Hotspot needs dnsmasq in the image before NetworkManager shared mode can provide DHCP.",
        );
    }

    let ssid = match validate_hotspot_ssid(request.ssid.as_deref().unwrap_or(DEFAULT_HOTSPOT_SSID))
    {
        Ok(ssid) => ssid,
        Err(text) => return hotspot_outcome(StatusCode::BAD_REQUEST, false, None, &text),
    };
    let password = match validate_hotspot_password(request.password.as_deref().unwrap_or("")) {
        Ok(password) => password,
        Err(text) => {
            return hotspot_outcome(StatusCode::BAD_REQUEST, false, Some(ssid), &text);
        }
    };

    let devices = match nmcli_output(&["-t", "-f", "DEVICE,TYPE,STATE", "device", "status"]) {
        Ok(output) => parse_device_status(&output),
        Err(NmcliError::Missing) => {
            return hotspot_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                Some(ssid),
                "Networking is not ready in this session, so Personal Hotspot cannot be changed here.",
            );
        }
        Err(NmcliError::Failed(detail)) => {
            return hotspot_outcome(
                StatusCode::BAD_GATEWAY,
                false,
                Some(ssid),
                &sanitize_hotspot_error(&detail, &password),
            );
        }
    };
    let active = nmcli_output(&[
        "-t",
        "-f",
        "NAME,TYPE,DEVICE",
        "connection",
        "show",
        "--active",
    ])
    .map(|output| parse_active_connections(&output))
    .unwrap_or_default();
    let ap_devices = ap_capable_devices(&devices);
    let device = match choose_hotspot_device(&devices, &active, &ap_devices) {
        Ok(device) => device,
        Err(text) => {
            return hotspot_outcome(StatusCode::SERVICE_UNAVAILABLE, false, Some(ssid), text)
        }
    };

    let _ = nmcli_output(&["connection", "down", "id", HOTSPOT_CONNECTION_NAME]);
    let _ = nmcli_output(&["connection", "delete", "id", HOTSPOT_CONNECTION_NAME]);

    let add_args = vec![
        "connection".to_string(),
        "add".to_string(),
        "save".to_string(),
        "no".to_string(),
        "type".to_string(),
        "wifi".to_string(),
        "ifname".to_string(),
        device.clone(),
        "con-name".to_string(),
        HOTSPOT_CONNECTION_NAME.to_string(),
        "autoconnect".to_string(),
        "no".to_string(),
        "ssid".to_string(),
        ssid.clone(),
    ];
    if let Err(error) = nmcli_output_owned(&add_args) {
        return hotspot_nmcli_failure(error, &ssid, &password);
    }

    let modify_args = vec![
        "connection".to_string(),
        "modify".to_string(),
        "id".to_string(),
        HOTSPOT_CONNECTION_NAME.to_string(),
        "802-11-wireless.mode".to_string(),
        "ap".to_string(),
        "802-11-wireless.band".to_string(),
        "bg".to_string(),
        "ipv4.method".to_string(),
        "shared".to_string(),
        "ipv6.method".to_string(),
        "ignore".to_string(),
        "wifi-sec.key-mgmt".to_string(),
        "wpa-psk".to_string(),
        "wifi-sec.psk".to_string(),
        password.clone(),
    ];
    if let Err(error) = nmcli_output_owned(&modify_args) {
        let _ = nmcli_output(&["connection", "delete", "id", HOTSPOT_CONNECTION_NAME]);
        return hotspot_nmcli_failure(error, &ssid, &password);
    }

    let up_args = vec![
        "connection".to_string(),
        "up".to_string(),
        "id".to_string(),
        HOTSPOT_CONNECTION_NAME.to_string(),
        "ifname".to_string(),
        device,
    ];
    match nmcli_output_owned(&up_args) {
        Ok(_) => hotspot_outcome(
            StatusCode::OK,
            true,
            Some(ssid.clone()),
            &format!(
                "Personal Hotspot is on as {ssid}. Passwords are used only to configure NetworkManager and are never returned by Goblins OS."
            ),
        ),
        Err(error) => {
            let _ = nmcli_output(&["connection", "delete", "id", HOTSPOT_CONNECTION_NAME]);
            hotspot_nmcli_failure(error, &ssid, &password)
        }
    }
}

fn stop_hotspot() -> (StatusCode, Json<HotspotOutcome>) {
    let _ = nmcli_output(&["connection", "down", "id", HOTSPOT_CONNECTION_NAME]);
    match nmcli_output(&["connection", "delete", "id", HOTSPOT_CONNECTION_NAME]) {
        Ok(_) | Err(NmcliError::Failed(_)) => hotspot_outcome(
            StatusCode::OK,
            false,
            None,
            "Personal Hotspot is off. The temporary NetworkManager hotspot profile has been removed.",
        ),
        Err(NmcliError::Missing) => hotspot_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            None,
            "Networking is not ready in this session, so Personal Hotspot cannot be changed here.",
        ),
    }
}

fn hotspot_nmcli_failure(
    error: NmcliError,
    ssid: &str,
    password: &str,
) -> (StatusCode, Json<HotspotOutcome>) {
    match error {
        NmcliError::Missing => hotspot_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            Some(ssid.to_string()),
            "Networking is not ready in this session, so Personal Hotspot cannot be changed here.",
        ),
        NmcliError::Failed(detail) => hotspot_outcome(
            StatusCode::BAD_GATEWAY,
            false,
            Some(ssid.to_string()),
            &sanitize_hotspot_error(&detail, password),
        ),
    }
}

fn hotspot_outcome(
    status: StatusCode,
    active: bool,
    ssid: Option<String>,
    text: &str,
) -> (StatusCode, Json<HotspotOutcome>) {
    (
        status,
        Json(HotspotOutcome {
            ok: status == StatusCode::OK,
            active,
            ssid,
            text: text.to_string(),
        }),
    )
}

fn nmcli_present() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join("nmcli").is_file()))
}

fn dnsmasq_present() -> bool {
    binary_present("dnsmasq") || std::path::Path::new("/usr/sbin/dnsmasq").is_file()
}

fn binary_present(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

fn nmcli(args: &[&str]) -> Option<String> {
    nmcli_output(args).ok()
}

fn nmcli_output(args: &[&str]) -> Result<String, NmcliError> {
    let output = Command::new("nmcli")
        .args(args)
        .stdin(Stdio::null())
        .output();
    nmcli_result(output)
}

fn nmcli_output_owned(args: &[String]) -> Result<String, NmcliError> {
    let output = Command::new("nmcli")
        .args(args)
        .stdin(Stdio::null())
        .output();
    nmcli_result(output)
}

fn nmcli_result(output: std::io::Result<std::process::Output>) -> Result<String, NmcliError> {
    match output {
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

/// Parse `nmcli -t -f TYPE,DEVICE,UUID connection show --active` into the active
/// Wi-Fi connections as `(device, uuid)`. Pure + unit-tested.
fn active_wifi_devices(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ':');
            let kind = parts.next()?;
            let device = parts.next()?;
            let uuid = parts.next()?;
            (kind == "802-11-wireless" && !uuid.is_empty())
                .then(|| (device.to_string(), uuid.to_string()))
        })
        .collect()
}

fn current_hotspot_ssid() -> Option<String> {
    nmcli(&[
        "-t",
        "-f",
        "802-11-wireless.ssid",
        "connection",
        "show",
        HOTSPOT_CONNECTION_NAME,
    ])
    .and_then(|output| {
        output.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key == "802-11-wireless.ssid" && !value.trim().is_empty())
                .then(|| value.trim().to_string())
        })
    })
}

fn hotspot_clients_for_device(device: &str) -> Option<Vec<HotspotClient>> {
    let now_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    hotspot_lease_candidates(device)
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .map(|text| parse_dnsmasq_leases(&text, now_epoch))
}

fn hotspot_lease_candidates(device: &str) -> Vec<PathBuf> {
    if let Ok(path) = std::env::var("GOBLINS_OS_HOTSPOT_LEASE_FILE") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return vec![PathBuf::from(trimmed)];
        }
    }

    let mut candidates = Vec::new();
    if !device.trim().is_empty() {
        candidates
            .push(Path::new("/var/lib/NetworkManager").join(format!("dnsmasq-{device}.leases")));
    }
    candidates.push(PathBuf::from("/var/lib/NetworkManager/dnsmasq.leases"));
    candidates.push(PathBuf::from("/var/lib/misc/dnsmasq.leases"));
    candidates
}

fn parse_dnsmasq_leases(text: &str, now_epoch: u64) -> Vec<HotspotClient> {
    text.lines()
        .filter_map(|line| parse_dnsmasq_lease_line(line, now_epoch))
        .collect()
}

fn parse_dnsmasq_lease_line(line: &str, now_epoch: u64) -> Option<HotspotClient> {
    let mut fields = line.split_whitespace();
    let expiry = fields.next()?.parse::<u64>().ok()?;
    let _mac = fields.next()?;
    let ip_address = fields.next()?.trim();
    let hostname = fields.next().unwrap_or("*").trim();
    if expiry != 0 && expiry <= now_epoch {
        return None;
    }
    if ip_address.is_empty() || ip_address == "*" {
        return None;
    }

    Some(HotspotClient {
        ip_address: ip_address.to_string(),
        hostname: (!hostname.is_empty() && hostname != "*").then(|| hostname.to_string()),
    })
}

/// True when `nmcli -t -f 802-11-wireless.mode connection show <uuid>` reports the
/// access-point (hotspot) mode. Pure + unit-tested.
fn mode_is_ap(output: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.split_once(':'))
        .any(|(key, value)| key == "802-11-wireless.mode" && value.trim() == "ap")
}

fn validate_hotspot_ssid(value: &str) -> Result<String, String> {
    let ssid = normalize_hotspot_ssid(value);
    if ssid.is_empty() {
        return Err("A hotspot network name is required.".to_string());
    }
    if ssid.starts_with('-') {
        return Err("A hotspot network name cannot start with a dash.".to_string());
    }
    if ssid.len() > 32 {
        return Err("A hotspot network name must be 32 bytes or fewer.".to_string());
    }
    if ssid.chars().any(|ch| ch.is_control()) {
        return Err("A hotspot network name cannot contain control characters.".to_string());
    }
    Ok(ssid)
}

fn normalize_hotspot_ssid(value: &str) -> String {
    value.trim().to_string()
}

fn validate_hotspot_password(value: &str) -> Result<String, String> {
    let password = value.trim().to_string();
    if password.len() < 8 {
        return Err("Personal Hotspot passwords must be at least 8 characters.".to_string());
    }
    if password.len() > 63 {
        return Err("Personal Hotspot passwords must be 63 characters or fewer.".to_string());
    }
    if password.chars().any(|ch| ch.is_control()) {
        return Err("Personal Hotspot passwords cannot contain control characters.".to_string());
    }
    Ok(password)
}

fn sanitize_hotspot_error(detail: &str, password: &str) -> String {
    if detail.trim().is_empty() || (!password.is_empty() && detail.contains(password)) {
        return "Personal Hotspot could not be changed. Check the network device and try again."
            .to_string();
    }
    format!("Personal Hotspot could not be changed: {}", detail.trim())
}

fn parse_device_status(output: &str) -> Vec<NetworkDevice> {
    output
        .lines()
        .filter_map(|line| {
            let fields = split_terse(line);
            let device = fields.first()?.trim();
            let kind = fields.get(1)?.trim();
            let state = fields.get(2).map(String::as_str).unwrap_or("").trim();
            (!device.is_empty()).then(|| NetworkDevice {
                device: device.to_string(),
                kind: kind.to_string(),
                state: state.to_string(),
            })
        })
        .collect()
}

fn parse_active_connections(output: &str) -> Vec<ActiveConnection> {
    output
        .lines()
        .filter_map(|line| {
            let fields = split_terse(line);
            let name = fields.first()?.trim();
            let kind = fields.get(1)?.trim();
            let device = fields.get(2).map(String::as_str).unwrap_or("").trim();
            (!name.is_empty()).then(|| ActiveConnection {
                name: name.to_string(),
                kind: kind.to_string(),
                device: device.to_string(),
            })
        })
        .collect()
}

fn parse_ap_capability(output: &str) -> bool {
    output.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(key, value)| key == "WIFI-PROPERTIES.AP" && value.trim() == "yes")
    })
}

fn ap_capable_devices(devices: &[NetworkDevice]) -> Vec<String> {
    devices
        .iter()
        .filter(|device| is_wifi_kind(&device.kind))
        .filter(|device| {
            nmcli(&[
                "-t",
                "-f",
                "WIFI-PROPERTIES.AP",
                "device",
                "show",
                &device.device,
            ])
            .as_deref()
            .is_some_and(parse_ap_capability)
        })
        .map(|device| device.device.clone())
        .collect()
}

fn choose_hotspot_device(
    devices: &[NetworkDevice],
    active: &[ActiveConnection],
    ap_devices: &[String],
) -> Result<String, &'static str> {
    if ap_devices.is_empty() {
        return Err("This device has no Wi-Fi adapter that can broadcast a hotspot.");
    }

    let non_wifi_uplink = active
        .iter()
        .any(|connection| !connection.device.is_empty() && !is_wifi_kind(&connection.kind));
    if !non_wifi_uplink {
        let wifi_active = active.iter().any(|connection| {
            is_wifi_kind(&connection.kind)
                && ap_devices.iter().any(|device| device == &connection.device)
        });
        if wifi_active
            && devices
                .iter()
                .filter(|device| is_wifi_kind(&device.kind) && device.state != "unavailable")
                .count()
                <= 1
        {
            return Err("Connect to the internet over Ethernet to share it over Wi-Fi.");
        }
        return Err(
            "Connect this device to the internet over Ethernet before starting Personal Hotspot.",
        );
    }

    ap_devices
        .iter()
        .find(|device| {
            !active.iter().any(|connection| {
                is_wifi_kind(&connection.kind) && connection.device == device.as_str()
            })
        })
        .or_else(|| ap_devices.first())
        .cloned()
        .ok_or("This device has no Wi-Fi adapter that can broadcast a hotspot.")
}

fn is_wifi_kind(kind: &str) -> bool {
    matches!(kind, "wifi" | "802-11-wireless")
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
        active_wifi_devices, choose_hotspot_device, mode_is_ap, parse_active_connections,
        parse_ap_capability, parse_device_status, parse_dnsmasq_leases, sanitize_hotspot_error,
        split_terse, validate_hotspot_password, validate_hotspot_ssid,
    };

    #[test]
    fn active_wifi_devices_keeps_only_wifi_with_a_uuid() {
        let output = "802-11-wireless:wlan0:abc-123\n\
                      802-3-ethernet:eth0:def-456\n\
                      802-11-wireless:wlan1:\n";
        assert_eq!(
            active_wifi_devices(output),
            vec![("wlan0".to_string(), "abc-123".to_string())]
        );
    }

    #[test]
    fn mode_is_ap_only_for_access_point() {
        assert!(mode_is_ap("802-11-wireless.mode:ap"));
        assert!(!mode_is_ap("802-11-wireless.mode:infrastructure"));
        assert!(!mode_is_ap("connection.id:home")); // unrelated field
        assert!(!mode_is_ap(""));
    }

    #[test]
    fn hotspot_inputs_are_validated_before_nmcli() {
        assert_eq!(validate_hotspot_ssid(" Goblins OS ").unwrap(), "Goblins OS");
        assert!(validate_hotspot_ssid("-option").is_err());
        assert!(validate_hotspot_ssid(&"x".repeat(33)).is_err());
        assert!(validate_hotspot_ssid("bad\nssid").is_err());

        assert_eq!(
            validate_hotspot_password("correct horse").unwrap(),
            "correct horse"
        );
        assert!(validate_hotspot_password("short").is_err());
        assert!(validate_hotspot_password(&"x".repeat(64)).is_err());
        assert!(validate_hotspot_password("bad\npassword").is_err());
    }

    #[test]
    fn hotspot_error_sanitizer_never_echoes_the_psk() {
        assert_eq!(
            sanitize_hotspot_error("failed with secret-passphrase", "secret-passphrase"),
            "Personal Hotspot could not be changed. Check the network device and try again."
        );
        assert_eq!(
            sanitize_hotspot_error("device does not support AP mode", "secret-passphrase"),
            "Personal Hotspot could not be changed: device does not support AP mode"
        );
    }

    #[test]
    fn hotspot_parses_networkmanager_terse_inventory() {
        assert_eq!(
            split_terse(r"Home\:main:802-11-wireless:wlan0"),
            vec!["Home:main", "802-11-wireless", "wlan0"]
        );
        assert_eq!(
            parse_device_status("wlan0:wifi:connected\neth0:ethernet:connected\n"),
            vec![
                super::NetworkDevice {
                    device: "wlan0".to_string(),
                    kind: "wifi".to_string(),
                    state: "connected".to_string(),
                },
                super::NetworkDevice {
                    device: "eth0".to_string(),
                    kind: "ethernet".to_string(),
                    state: "connected".to_string(),
                },
            ]
        );
        assert_eq!(
            parse_active_connections("Wired:802-3-ethernet:eth0\nHome:802-11-wireless:wlan0\n"),
            vec![
                super::ActiveConnection {
                    name: "Wired".to_string(),
                    kind: "802-3-ethernet".to_string(),
                    device: "eth0".to_string(),
                },
                super::ActiveConnection {
                    name: "Home".to_string(),
                    kind: "802-11-wireless".to_string(),
                    device: "wlan0".to_string(),
                },
            ]
        );
        assert!(parse_ap_capability("WIFI-PROPERTIES.AP:yes"));
        assert!(!parse_ap_capability("WIFI-PROPERTIES.AP:no"));
    }

    #[test]
    fn hotspot_start_gate_rejects_single_radio_wifi_uplink() {
        let devices = parse_device_status("wlan0:wifi:connected\n");
        let active = parse_active_connections("Home:802-11-wireless:wlan0\n");
        assert_eq!(
            choose_hotspot_device(&devices, &active, &["wlan0".to_string()]).unwrap_err(),
            "Connect to the internet over Ethernet to share it over Wi-Fi."
        );

        let devices = parse_device_status("wlan0:wifi:disconnected\neth0:ethernet:connected\n");
        let active = parse_active_connections("Wired:802-3-ethernet:eth0\n");
        assert_eq!(
            choose_hotspot_device(&devices, &active, &["wlan0".to_string()]).unwrap(),
            "wlan0"
        );
    }

    #[test]
    fn hotspot_lease_parser_reports_only_current_clients() {
        let leases = "\
1800 00:11:22:33:44:55 10.42.0.10 phone *
900 00:11:22:33:44:66 10.42.0.11 expired *
0 00:11:22:33:44:77 10.42.0.12 * *
bad line
";
        let clients = parse_dnsmasq_leases(leases, 1000);
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0].ip_address, "10.42.0.10");
        assert_eq!(clients[0].hostname.as_deref(), Some("phone"));
        assert_eq!(clients[1].ip_address, "10.42.0.12");
        assert_eq!(clients[1].hostname, None);
    }
}
