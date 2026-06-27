//! Firewall status and the scoped Settings toggle for firewalld.
//!
//! This is the macOS "Firewall" altitude (is it on?), not a rule editor. Reads go
//! through `firewall-cmd`; writes only ask systemd to start the fixed
//! `goblins-os-firewall@enable/disable.service` instances. The root helper owns
//! the actual firewalld unit write, and polkit scopes that system-bus request to
//! the `goblins-os` service user plus the two template instances.

use std::{
    path::Path,
    process::{Command, Stdio},
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

const FIREWALL_HELPER: &str = "/usr/libexec/goblins-os/goblins-os-firewall";
const FIREWALL_UNIT_TEMPLATE: &str = "/usr/lib/systemd/system/goblins-os-firewall@.service";
const FIREWALL_POLKIT_RULE: &str = "/etc/polkit-1/rules.d/60-goblins-os-firewall.rules";

#[derive(Serialize)]
pub struct FirewallStatus {
    source: &'static str,
    /// Whether the `firewall-cmd` tool (firewalld) is present at all.
    available: bool,
    /// Whether firewalld reports the firewall as running.
    active: bool,
    /// Whether Settings can reach the Goblins OS privileged bridge for On/Off.
    manageable: bool,
    detail: String,
    management_detail: String,
}

#[derive(Deserialize)]
pub struct FirewallEnabledRequest {
    enabled: bool,
}

#[derive(Serialize)]
pub struct FirewallToggleOutcome {
    ok: bool,
    enabled: bool,
    text: String,
}

pub async fn firewall_status() -> Json<FirewallStatus> {
    Json(build_firewall_status())
}

pub async fn set_firewall_enabled(
    Json(request): Json<FirewallEnabledRequest>,
) -> (StatusCode, Json<FirewallToggleOutcome>) {
    firewall_enabled_outcome(request.enabled)
}

fn build_firewall_status() -> FirewallStatus {
    if !firewall_cmd_present() {
        return FirewallStatus {
            source: "goblins-os-core",
            available: false,
            active: false,
            manageable: false,
            detail: "Firewall service is not ready on this device.".to_string(),
            management_detail: "Firewall service is not ready on this device.".to_string(),
        };
    }
    // `firewall-cmd --state` prints "running" and exits 0 when active; otherwise it
    // prints "not running" and exits non-zero. Parse both signals.
    let output = Command::new("firewall-cmd")
        .arg("--state")
        .stdin(Stdio::null())
        .output();
    let active = match output {
        Ok(output) => firewall_is_running(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
        ),
        Err(_) => false,
    };
    let manageable = firewall_bridge_ready();
    FirewallStatus {
        source: "goblins-os-core",
        available: true,
        active,
        manageable,
        detail: firewall_detail(active, firewall_default_zone().as_deref()),
        management_detail: firewall_management_detail(manageable).to_string(),
    }
}

fn firewall_cmd_present() -> bool {
    executable_exists("firewall-cmd")
}

fn executable_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

fn firewall_bridge_ready() -> bool {
    executable_exists("systemctl")
        && Path::new(FIREWALL_HELPER).is_file()
        && Path::new(FIREWALL_UNIT_TEMPLATE).is_file()
        && Path::new(FIREWALL_POLKIT_RULE).is_file()
}

/// firewalld is "running" only when the command succeeded AND said so — pure,
/// unit-tested so the status can never silently report the wrong posture.
fn firewall_is_running(command_succeeded: bool, stdout: &str) -> bool {
    command_succeeded && stdout.trim() == "running"
}

fn firewall_detail(active: bool, zone: Option<&str>) -> String {
    if active {
        format!(
            "The firewall blocks unwanted incoming connections. Zone: {}.",
            zone.filter(|value| !value.trim().is_empty())
                .unwrap_or("checking")
        )
    } else {
        "The firewall is off. Turn it on to filter incoming network connections.".to_string()
    }
}

fn firewall_management_detail(manageable: bool) -> &'static str {
    if manageable {
        "Settings can turn the firewall on or off through the Goblins OS firewall bridge."
    } else {
        "Turning the firewall on or off is managed by the system."
    }
}

fn firewall_default_zone() -> Option<String> {
    match Command::new("firewall-cmd")
        .arg("--get-default-zone")
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            let zone = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!zone.is_empty()).then_some(zone)
        }
        _ => None,
    }
}

fn firewall_enabled_outcome(enabled: bool) -> (StatusCode, Json<FirewallToggleOutcome>) {
    if !firewall_cmd_present() {
        return firewall_toggle_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            enabled,
            "Firewall service is not ready on this device.",
        );
    }
    if !firewall_bridge_ready() {
        return firewall_toggle_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            enabled,
            "Turning the firewall on or off is managed by the system.",
        );
    }

    let unit = firewall_template_instance(enabled);
    match Command::new("systemctl")
        .args(["start", unit])
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            let status = build_firewall_status();
            if status.available && status.active == enabled {
                firewall_toggle_response(
                    StatusCode::OK,
                    true,
                    enabled,
                    &firewall_toggle_success_detail(enabled, status.detail.as_str()),
                )
            } else {
                firewall_toggle_response(
                    StatusCode::BAD_GATEWAY,
                    false,
                    enabled,
                    "Firewall control returned before firewalld reported the requested state.",
                )
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            firewall_toggle_response(
                StatusCode::BAD_GATEWAY,
                false,
                enabled,
                &firewall_command_error_detail(&stderr, &stdout),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => firewall_toggle_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            enabled,
            "Firewall control is not ready in this session.",
        ),
        Err(_) => firewall_toggle_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            enabled,
            "Firewall control is not ready in this session.",
        ),
    }
}

fn firewall_template_instance(enabled: bool) -> &'static str {
    if enabled {
        "goblins-os-firewall@enable.service"
    } else {
        "goblins-os-firewall@disable.service"
    }
}

fn firewall_toggle_success_detail(enabled: bool, status_detail: &str) -> String {
    if enabled {
        format!("Firewall is on. {status_detail}")
    } else {
        "Firewall is off. Incoming network connections are no longer filtered by firewalld."
            .to_string()
    }
}

fn firewall_command_error_detail(stderr: &str, stdout: &str) -> String {
    let raw = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    if raw.is_empty() {
        "Firewall control failed without a system message.".to_string()
    } else {
        format!("Firewall control failed: {raw}")
    }
}

fn firewall_toggle_response(
    status: StatusCode,
    ok: bool,
    enabled: bool,
    text: &str,
) -> (StatusCode, Json<FirewallToggleOutcome>) {
    (
        status,
        Json(FirewallToggleOutcome {
            ok,
            enabled,
            text: text.to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::{
        firewall_command_error_detail, firewall_detail, firewall_is_running,
        firewall_management_detail, firewall_template_instance, firewall_toggle_response,
    };

    #[test]
    fn running_requires_success_and_running_text() {
        assert!(firewall_is_running(true, "running\n"));
        assert!(!firewall_is_running(true, "not running")); // wrong text
        assert!(!firewall_is_running(false, "running")); // command failed
        assert!(!firewall_is_running(false, "")); // typical "off" case
    }

    #[test]
    fn detail_reflects_state() {
        assert!(firewall_detail(true, Some("public")).contains("Zone: public"));
        assert!(firewall_detail(false, None).contains("off"));
    }

    #[test]
    fn toggle_instances_are_fixed() {
        assert_eq!(
            firewall_template_instance(true),
            "goblins-os-firewall@enable.service"
        );
        assert_eq!(
            firewall_template_instance(false),
            "goblins-os-firewall@disable.service"
        );
    }

    #[test]
    fn management_detail_is_honest_when_bridge_missing() {
        assert_eq!(
            firewall_management_detail(false),
            "Turning the firewall on or off is managed by the system."
        );
        assert!(firewall_management_detail(true).contains("Goblins OS firewall bridge"));
    }

    #[test]
    fn toggle_response_status_matches_outcome() {
        let (status, outcome) = firewall_toggle_response(
            StatusCode::BAD_GATEWAY,
            false,
            true,
            "Firewall control failed.",
        );
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(!outcome.ok);
        assert!(outcome.text.contains("failed"));
    }

    #[test]
    fn command_error_prefers_stderr_then_stdout() {
        assert_eq!(
            firewall_command_error_detail("denied\n", "ignored"),
            "Firewall control failed: denied"
        );
        assert_eq!(
            firewall_command_error_detail("", "stdout"),
            "Firewall control failed: stdout"
        );
        assert!(firewall_command_error_detail("", "").contains("without a system message"));
    }
}
