//! Personal Hotspot status (NetworkManager).
//!
//! A read-only posture surface — the macOS "Personal Hotspot" altitude (is the
//! machine currently sharing its connection over Wi-Fi?), NOT a hotspot creator.
//! Goblins OS keeps this behind the core so Settings reads an honest status. The
//! gated "start hotspot" action is a deliberate follow-up (it writes a new AP
//! connection with an SSID/password), so this ships status-only and honest-gated
//! when NetworkManager's `nmcli` is absent.

use std::process::{Command, Stdio};

use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct HotspotStatus {
    source: &'static str,
    /// Whether the `nmcli` tool (NetworkManager) is present at all.
    available: bool,
    /// Whether a Wi-Fi connection is currently active in access-point (hotspot) mode.
    active: bool,
    /// The device sharing the connection, when a hotspot is active.
    device: Option<String>,
    detail: String,
}

pub async fn hotspot_status() -> Json<HotspotStatus> {
    Json(build_hotspot_status())
}

fn build_hotspot_status() -> HotspotStatus {
    if !nmcli_present() {
        return HotspotStatus {
            source: "goblins-os-core",
            available: false,
            active: false,
            device: None,
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
        Some((device, _)) => HotspotStatus {
            source: "goblins-os-core",
            active: true,
            available: true,
            detail: format!("Personal Hotspot is on, sharing this connection over {device}."),
            device: Some(device),
        },
        None => HotspotStatus {
            source: "goblins-os-core",
            available: true,
            active: false,
            device: None,
            detail: "Personal Hotspot is off. No Wi-Fi access point is currently active."
                .to_string(),
        },
    }
}

fn nmcli_present() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join("nmcli").is_file()))
}

fn nmcli(args: &[&str]) -> Option<String> {
    let output = Command::new("nmcli")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
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

/// True when `nmcli -t -f 802-11-wireless.mode connection show <uuid>` reports the
/// access-point (hotspot) mode. Pure + unit-tested.
fn mode_is_ap(output: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.split_once(':'))
        .any(|(key, value)| key == "802-11-wireless.mode" && value.trim() == "ap")
}

#[cfg(test)]
mod tests {
    use super::{active_wifi_devices, mode_is_ap};

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
}
