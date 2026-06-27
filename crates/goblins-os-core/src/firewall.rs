//! Firewall status for Settings (firewalld).
//!
//! A read-only posture surface — the macOS "Firewall" altitude (is it on?), NOT a
//! rule editor. Goblins OS keeps this behind the core so the Settings GUI reads an
//! honest status instead of asserting one. The gated On/Off toggle is a deliberate
//! follow-up that needs a scoped polkit rule (never a blind `systemctl`), so this
//! ships status-only and honest-gated when firewalld isn't present.

use std::process::{Command, Stdio};

use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct FirewallStatus {
    source: &'static str,
    /// Whether the `firewall-cmd` tool (firewalld) is present at all.
    available: bool,
    /// Whether firewalld reports the firewall as running.
    active: bool,
    detail: String,
}

pub async fn firewall_status() -> Json<FirewallStatus> {
    Json(build_firewall_status())
}

fn build_firewall_status() -> FirewallStatus {
    if !firewall_cmd_present() {
        return FirewallStatus {
            source: "goblins-os-core",
            available: false,
            active: false,
            detail: "Firewall status is unavailable on this device (firewalld is not installed)."
                .to_string(),
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
    FirewallStatus {
        source: "goblins-os-core",
        available: true,
        active,
        detail: firewall_detail(active).to_string(),
    }
}

fn firewall_cmd_present() -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join("firewall-cmd").is_file())
    })
}

/// firewalld is "running" only when the command succeeded AND said so — pure,
/// unit-tested so the status can never silently report the wrong posture.
fn firewall_is_running(command_succeeded: bool, stdout: &str) -> bool {
    command_succeeded && stdout.trim() == "running"
}

fn firewall_detail(active: bool) -> &'static str {
    if active {
        "The firewall is on. Incoming connections are filtered by the active firewalld zone."
    } else {
        "The firewall is off. Turn it on to filter incoming network connections."
    }
}

#[cfg(test)]
mod tests {
    use super::{firewall_detail, firewall_is_running};

    #[test]
    fn running_requires_success_and_running_text() {
        assert!(firewall_is_running(true, "running\n"));
        assert!(!firewall_is_running(true, "not running")); // wrong text
        assert!(!firewall_is_running(false, "running")); // command failed
        assert!(!firewall_is_running(false, "")); // typical "off" case
    }

    #[test]
    fn detail_reflects_state() {
        assert!(firewall_detail(true).contains("on"));
        assert!(firewall_detail(false).contains("off"));
    }
}
