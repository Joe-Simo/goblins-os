//! Per-app privacy (xdg `PermissionStore`) read substrate.
//!
//! The macOS "Privacy & Security ▸ per-app permissions" altitude. GNOME/portals
//! persist which apps were granted Location, Background/autostart, Notifications,
//! and device (camera/microphone) access in the xdg-desktop-portal PermissionStore
//! D-Bus service. This module reads it read-only via `gdbus` (no new package — the
//! portal already ships) and reports the entries per table, honest-gated when the
//! service isn't reachable. Revoking a grant is the deliberate follow-up (a
//! `DeletePermission` write behind the policy bridge); nothing here writes.
//!
//! D-Bus (web-verified against the xdg-desktop-portal spec):
//! - dest/iface `org.freedesktop.impl.portal.PermissionStore`
//! - path `/org/freedesktop/impl/portal/PermissionStore`
//! - `List(in s table, out as ids)`

use std::process::{Command, Stdio};

use axum::Json;
use serde::Serialize;

const PERMISSION_STORE_DEST: &str = "org.freedesktop.impl.portal.PermissionStore";
const PERMISSION_STORE_PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
const PERMISSION_STORE_LIST: &str = "org.freedesktop.impl.portal.PermissionStore.List";

/// Standard portal tables Goblins surfaces. `location`/`background`/`notifications`
/// are keyed by application id; `devices` is keyed by device (camera/microphone).
const TABLES: &[(&str, &str, bool)] = &[
    ("location", "Location", true),
    ("background", "Background & autostart", true),
    ("notifications", "Notifications", true),
    ("devices", "Camera & microphone", false),
];

#[derive(Serialize)]
pub struct PermissionTable {
    table: &'static str,
    label: &'static str,
    /// Whether `entries` are application ids (vs device ids for the `devices` table).
    app_keyed: bool,
    entries: Vec<String>,
}

#[derive(Serialize)]
pub struct AppPrivacyStatus {
    source: &'static str,
    available: bool,
    tables: Vec<PermissionTable>,
    detail: String,
}

pub async fn app_privacy_status() -> Json<AppPrivacyStatus> {
    Json(build_app_privacy_status())
}

fn build_app_privacy_status() -> AppPrivacyStatus {
    if !gdbus_present() {
        return AppPrivacyStatus {
            source: "goblins-os-core",
            available: false,
            tables: Vec::new(),
            detail: "App permissions are unavailable here (the desktop portal D-Bus tools are not present).".to_string(),
        };
    }

    let mut tables = Vec::new();
    let mut any_reachable = false;
    for (table, label, app_keyed) in TABLES {
        match list_table(table) {
            Some(entries) => {
                any_reachable = true;
                tables.push(PermissionTable {
                    table,
                    label,
                    app_keyed: *app_keyed,
                    entries,
                });
            }
            None => continue,
        }
    }

    if !any_reachable {
        return AppPrivacyStatus {
            source: "goblins-os-core",
            available: false,
            tables: Vec::new(),
            detail:
                "App permissions are unavailable here (the portal PermissionStore isn't running)."
                    .to_string(),
        };
    }

    AppPrivacyStatus {
        source: "goblins-os-core",
        available: true,
        tables,
        detail: "Apps that have requested Location, Background, Notification, or device access are listed here.".to_string(),
    }
}

fn list_table(table: &str) -> Option<Vec<String>> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            PERMISSION_STORE_DEST,
            "--object-path",
            PERMISSION_STORE_PATH,
            "--method",
            PERMISSION_STORE_LIST,
            table,
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_list_reply(&String::from_utf8_lossy(&output.stdout)))
}

/// Parse a `gdbus` reply for `List` — `(['app.one', 'app.two'],)` — into its ids.
/// Pure + unit-tested so the privacy list never misreports what the store holds.
fn parse_list_reply(reply: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = reply.chars();
    while let Some(c) = chars.next() {
        if c != '\'' {
            continue;
        }
        let mut value = String::new();
        loop {
            match chars.next() {
                None | Some('\'') => break,
                Some('\\') => {
                    if let Some(escaped) = chars.next() {
                        value.push(escaped);
                    }
                }
                Some(ch) => value.push(ch),
            }
        }
        out.push(value);
    }
    out
}

fn gdbus_present() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join("gdbus").is_file()))
}

#[cfg(test)]
mod tests {
    use super::parse_list_reply;

    #[test]
    fn parses_gdbus_list_reply() {
        assert_eq!(
            parse_list_reply("(['org.gnome.Calculator', 'firefox'],)"),
            vec!["org.gnome.Calculator".to_string(), "firefox".to_string()]
        );
        assert_eq!(parse_list_reply("([],)"), Vec::<String>::new());
        // Escaped quote inside an id is preserved.
        assert_eq!(parse_list_reply("(['it\\'s'],)"), vec!["it's".to_string()]);
    }
}
