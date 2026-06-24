//! Bluetooth status for Settings.
//!
//! Pairing and trust changes need a privileged, policy-aware route before they
//! can be exposed safely. This endpoint only reports Bluetooth support and the
//! default adapter state from server-side tools.

use std::process::Command;

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct BluetoothStatus {
    source: &'static str,
    bluez_available: bool,
    service_active: bool,
    adapter_present: bool,
    powered: Option<bool>,
    discoverable: Option<bool>,
    pairable: Option<bool>,
    adapter: Option<BluetoothAdapter>,
    detail: String,
}

#[derive(Serialize, PartialEq, Eq, Debug)]
pub struct BluetoothAdapter {
    name: Option<String>,
    alias: Option<String>,
    address: String,
}

#[derive(Deserialize)]
pub struct BluetoothPowerRequest {
    powered: bool,
}

#[derive(Serialize)]
pub struct BluetoothPowerOutcome {
    ok: bool,
    powered: bool,
    text: String,
}

pub async fn bluetooth_status() -> Json<BluetoothStatus> {
    Json(build_bluetooth_status())
}

pub async fn set_bluetooth_power(
    Json(request): Json<BluetoothPowerRequest>,
) -> (StatusCode, Json<BluetoothPowerOutcome>) {
    bluetooth_power_outcome(request.powered)
}

fn build_bluetooth_status() -> BluetoothStatus {
    let service_active =
        command_success("systemctl", &["is-active", "--quiet", "bluetooth.service"]);
    let daemon_available = executable_exists("bluetoothd");
    let client_available = executable_exists("bluetoothctl");
    let bluez_available = service_active || daemon_available || client_available;

    if !client_available {
        return BluetoothStatus {
            source: "goblins-os-core",
            bluez_available,
            service_active,
            adapter_present: false,
            powered: None,
            discoverable: None,
            pairable: None,
            adapter: None,
            detail: "Bluetooth support is not ready on this device, so Settings cannot inspect adapters."
                .to_string(),
        };
    }

    match bluetoothctl_show() {
        Ok(stdout) => {
            let parsed = parse_bluetoothctl_show(&stdout);
            let adapter_present = parsed.adapter.is_some();
            BluetoothStatus {
                source: "goblins-os-core",
                bluez_available,
                service_active,
                adapter_present,
                powered: parsed.powered,
                discoverable: parsed.discoverable,
                pairable: parsed.pairable,
                adapter: parsed.adapter,
                detail: bluetooth_detail(
                    service_active,
                    bluez_available,
                    adapter_present,
                    parsed.powered,
                ),
            }
        }
        Err(detail) => BluetoothStatus {
            source: "goblins-os-core",
            bluez_available,
            service_active,
            adapter_present: false,
            powered: None,
            discoverable: None,
            pairable: None,
            adapter: None,
            detail,
        },
    }
}

struct ParsedBluetoothStatus {
    adapter: Option<BluetoothAdapter>,
    powered: Option<bool>,
    discoverable: Option<bool>,
    pairable: Option<bool>,
}

fn bluetoothctl_show() -> Result<String, String> {
    match Command::new("bluetoothctl").arg("show").output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Err(bluetoothctl_error_detail(&stderr, &stdout))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(
            "Bluetooth support is not ready on this device, so Settings cannot inspect adapters."
                .to_string(),
        ),
        Err(_) => Err("Bluetooth adapter status is not ready.".to_string()),
    }
}

fn bluetooth_power_outcome(powered: bool) -> (StatusCode, Json<BluetoothPowerOutcome>) {
    let power = if powered { "on" } else { "off" };
    match Command::new("bluetoothctl").args(["power", power]).output() {
        Ok(output) if output.status.success() => (
            StatusCode::OK,
            Json(BluetoothPowerOutcome {
                ok: true,
                powered,
                text: bluetooth_power_success_detail(powered),
            }),
        ),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (
                StatusCode::BAD_GATEWAY,
                Json(BluetoothPowerOutcome {
                    ok: false,
                    powered,
                    text: bluetoothctl_error_detail(&stderr, &stdout),
                }),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(BluetoothPowerOutcome {
                ok: false,
                powered,
                text: "Bluetooth support is not ready on this device, so Settings cannot change Bluetooth power."
                    .to_string(),
            }),
        ),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(BluetoothPowerOutcome {
                ok: false,
                powered,
                text: "Bluetooth power is not ready in this session.".to_string(),
            }),
        ),
    }
}

fn bluetooth_power_success_detail(powered: bool) -> String {
    if powered {
        "Bluetooth is powered on. Pairing and device changes will appear when supported controls are available.".to_string()
    } else {
        "Bluetooth is powered off. Existing device connections are not shown until it is turned on again.".to_string()
    }
}

fn bluetoothctl_error_detail(stderr: &str, stdout: &str) -> String {
    let raw = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    let lower = raw.to_ascii_lowercase();

    if raw.is_empty() {
        return "No Bluetooth adapter was reported.".to_string();
    }
    if lower.contains("dbus")
        || lower.contains("d-bus")
        || lower.contains("host is down")
        || lower.contains("no medium found")
    {
        return "Bluetooth adapter status is not ready from this session.".to_string();
    }
    if lower.contains("no default controller") || lower.contains("no controller available") {
        return "Bluetooth adapter status is not ready because no default Bluetooth adapter is present.".to_string();
    }

    format!("Bluetooth adapter status is not ready: {raw}")
}

fn parse_bluetoothctl_show(stdout: &str) -> ParsedBluetoothStatus {
    let mut adapter = None;
    let mut name = None;
    let mut alias = None;
    let mut powered = None;
    let mut discoverable = None;
    let mut pairable = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(address) = trimmed
            .strip_prefix("Controller ")
            .and_then(|rest| rest.split_whitespace().next())
        {
            adapter = Some(BluetoothAdapter {
                name: None,
                alias: None,
                address: address.to_string(),
            });
            continue;
        }

        if let Some(value) = trimmed.strip_prefix("Name: ") {
            name = Some(value.to_string());
        } else if let Some(value) = trimmed.strip_prefix("Alias: ") {
            alias = Some(value.to_string());
        } else if let Some(value) = trimmed.strip_prefix("Powered: ") {
            powered = parse_yes_no(value);
        } else if let Some(value) = trimmed.strip_prefix("Discoverable: ") {
            discoverable = parse_yes_no(value);
        } else if let Some(value) = trimmed.strip_prefix("Pairable: ") {
            pairable = parse_yes_no(value);
        }
    }

    if let Some(adapter) = &mut adapter {
        adapter.name = name;
        adapter.alias = alias;
    }

    ParsedBluetoothStatus {
        adapter,
        powered,
        discoverable,
        pairable,
    }
}

fn bluetooth_detail(
    service_active: bool,
    bluez_available: bool,
    adapter_present: bool,
    powered: Option<bool>,
) -> String {
    if !bluez_available {
        return "Bluetooth support is not ready, so Bluetooth cannot be managed on this device."
            .to_string();
    }
    if !service_active {
        return "Bluetooth support is present, but Bluetooth is not running.".to_string();
    }
    if !adapter_present {
        return "No Bluetooth adapter is connected.".to_string();
    }
    match powered {
        Some(true) => "Bluetooth is powered on and ready to discover or pair devices when supported controls are available.".to_string(),
        Some(false) => "A Bluetooth adapter is present, but it is powered off.".to_string(),
        None => "A Bluetooth adapter is present, but its power state was not reported.".to_string(),
    }
}

fn parse_yes_no(value: &str) -> Option<bool> {
    match value.trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

fn command_success(binary: &str, args: &[&str]) -> bool {
    Command::new(binary)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn executable_exists(binary: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            let candidate = path.join(binary);
            std::fs::metadata(candidate)
                .map(|metadata| metadata.is_file())
                .unwrap_or(false)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{
        bluetooth_detail, bluetooth_power_success_detail, bluetoothctl_error_detail,
        parse_bluetoothctl_show, BluetoothAdapter,
    };

    #[test]
    fn parses_default_controller_status() {
        let parsed = parse_bluetoothctl_show(
            "Controller 00:11:22:33:44:55 (public)\n\
             \tName: goblins\n\
             \tAlias: Goblins Workstation\n\
             \tPowered: yes\n\
             \tDiscoverable: no\n\
             \tPairable: yes\n",
        );

        assert_eq!(
            parsed.adapter,
            Some(BluetoothAdapter {
                name: Some("goblins".to_string()),
                alias: Some("Goblins Workstation".to_string()),
                address: "00:11:22:33:44:55".to_string(),
            })
        );
        assert_eq!(parsed.powered, Some(true));
        assert_eq!(parsed.discoverable, Some(false));
        assert_eq!(parsed.pairable, Some(true));
    }

    #[test]
    fn bluetooth_copy_distinguishes_missing_service_and_adapter() {
        assert!(bluetooth_detail(false, true, false, None).contains("not running"));
        assert!(bluetooth_detail(true, true, false, None).contains("No Bluetooth adapter"));
        assert!(bluetooth_detail(true, true, true, Some(false)).contains("powered off"));
        assert!(bluetooth_power_success_detail(true).contains("powered on"));
        assert!(bluetooth_power_success_detail(false).contains("powered off"));
        assert!(bluetooth_power_success_detail(false).contains("not shown"));
    }

    #[test]
    fn bluetoothctl_errors_are_sanitized_for_settings() {
        assert_eq!(
            bluetoothctl_error_detail("dbus assertion connection != NULL", ""),
            "Bluetooth adapter status is not ready from this session."
        );
        assert_eq!(
            bluetoothctl_error_detail("", "No default controller available"),
            "Bluetooth adapter status is not ready because no default Bluetooth adapter is present."
        );
    }
}
