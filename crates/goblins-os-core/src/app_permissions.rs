//! Per-app privacy (xdg `PermissionStore`) substrate.
//!
//! The macOS "Privacy & Security ▸ per-app permissions" altitude. GNOME/portals
//! persist which apps were granted Location, Background/autostart, Notifications,
//! and device (camera/microphone) access in the xdg-desktop-portal PermissionStore
//! D-Bus service. This module reads it via `gdbus` (no new package — the portal
//! already ships) and revokes explicit app-keyed grants through the allowlisted
//! `DeletePermission` method, honest-gated when the service isn't reachable.
//!
//! D-Bus (web-verified against the xdg-desktop-portal spec):
//! - dest/iface `org.freedesktop.impl.portal.PermissionStore`
//! - path `/org/freedesktop/impl/portal/PermissionStore`
//! - `List(in s table, out as ids)`
//! - `DeletePermission(in s table, in s id, in s app)`

use std::process::{Command, Stdio};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::session_bridge::{self, SessionBridgeResult};

const PERMISSION_STORE_DEST: &str = "org.freedesktop.impl.portal.PermissionStore";
const PERMISSION_STORE_PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
const PERMISSION_STORE_LIST: &str = "org.freedesktop.impl.portal.PermissionStore.List";
const PERMISSION_STORE_DELETE_PERMISSION: &str =
    "org.freedesktop.impl.portal.PermissionStore.DeletePermission";

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
    entries: Vec<PermissionEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PermissionEntry {
    id: String,
    app: String,
    label: String,
}

#[derive(Serialize)]
pub struct AppPrivacyStatus {
    source: &'static str,
    available: bool,
    tables: Vec<PermissionTable>,
    detail: String,
}

#[derive(Deserialize)]
pub struct RevokePermissionRequest {
    table: String,
    id: String,
    app: String,
}

#[derive(Serialize)]
pub struct AppPrivacyRevokeOutcome {
    ok: bool,
    text: String,
    table: String,
    id: String,
    app: String,
}

pub async fn app_privacy_status() -> Json<AppPrivacyStatus> {
    Json(build_app_privacy_status())
}

pub async fn revoke_app_permission(
    Json(request): Json<RevokePermissionRequest>,
) -> (StatusCode, Json<AppPrivacyRevokeOutcome>) {
    let (status, outcome) = revoke_app_permission_outcome(request);
    (status, Json(outcome))
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
            Some(ids) => {
                any_reachable = true;
                tables.push(PermissionTable {
                    table,
                    label,
                    app_keyed: *app_keyed,
                    entries: permission_entries_from_ids(ids, *app_keyed),
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

fn revoke_app_permission_outcome(
    request: RevokePermissionRequest,
) -> (StatusCode, AppPrivacyRevokeOutcome) {
    let Some((table, label, app_keyed)) = permission_table_spec(&request.table) else {
        return app_privacy_revoke_outcome(
            StatusCode::BAD_REQUEST,
            false,
            "Goblins OS only revokes known desktop permission tables.",
            request,
        );
    };
    if !app_keyed {
        return app_privacy_revoke_outcome(
            StatusCode::BAD_REQUEST,
            false,
            "This permission table is resource-keyed, so Goblins OS needs an app mapping before it can revoke a grant safely.",
            request,
        );
    }
    let normalized = normalize_revoke_request(table, request);
    let request = match normalized {
        Ok(request) => request,
        Err((status, request, message)) => {
            return app_privacy_revoke_outcome(status, false, message, request);
        }
    };
    if !gdbus_present() {
        return app_privacy_revoke_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "App permissions are read-only here because the desktop portal D-Bus tools are not present.",
            request,
        );
    }

    match delete_permission(&request.table, &request.id, &request.app) {
        Ok(()) => app_privacy_revoke_outcome(
            StatusCode::OK,
            true,
            &format!(
                "Revoked {label} access for {}.",
                friendly_app_name(&request.app)
            ),
            request,
        ),
        Err(detail) => app_privacy_revoke_outcome(
            StatusCode::BAD_GATEWAY,
            false,
            &format!("The desktop portal rejected that permission revoke: {detail}"),
            request,
        ),
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

fn delete_permission(table: &str, id: &str, app: &str) -> Result<(), String> {
    match session_bridge::permission_store_delete_permission(table, id, app) {
        SessionBridgeResult::Success(_) => return Ok(()),
        SessionBridgeResult::Failed(detail) => return Err(detail),
        SessionBridgeResult::Unavailable => {}
    }

    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            PERMISSION_STORE_DEST,
            "--object-path",
            PERMISSION_STORE_PATH,
            "--method",
            PERMISSION_STORE_DELETE_PERMISSION,
            table,
            id,
            app,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "desktop bridge is missing".to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if stderr.is_empty() {
        Err("permission store returned a failure without details".to_string())
    } else {
        Err(stderr)
    }
}

fn permission_entries_from_ids(ids: Vec<String>, app_keyed: bool) -> Vec<PermissionEntry> {
    ids.into_iter()
        .filter(|id| permission_id_is_safe(id))
        .map(|id| {
            let app = if app_keyed { id.clone() } else { String::new() };
            PermissionEntry {
                label: friendly_app_name(&id),
                id,
                app,
            }
        })
        .collect()
}

fn permission_table_spec(table: &str) -> Option<(&'static str, &'static str, bool)> {
    TABLES
        .iter()
        .copied()
        .find(|(candidate, _, _)| *candidate == table)
}

fn normalize_revoke_request(
    canonical_table: &str,
    request: RevokePermissionRequest,
) -> Result<RevokePermissionRequest, (StatusCode, RevokePermissionRequest, &'static str)> {
    let table = request.table.trim().to_string();
    let id = request.id.trim().to_string();
    let app = request.app.trim().to_string();
    let normalized = RevokePermissionRequest { table, id, app };
    if normalized.table != canonical_table {
        return Err((
            StatusCode::BAD_REQUEST,
            normalized,
            "Permission table names must be exact.",
        ));
    }
    if !permission_id_is_safe(&normalized.id) || !permission_id_is_safe(&normalized.app) {
        return Err((
            StatusCode::BAD_REQUEST,
            normalized,
            "Permission app and resource identifiers must be safe desktop IDs.",
        ));
    }
    if normalized.id != normalized.app {
        return Err((
            StatusCode::BAD_REQUEST,
            normalized,
            "Only app-keyed grants can be revoked from this Settings surface.",
        ));
    }
    Ok(normalized)
}

fn permission_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn friendly_app_name(app_id: &str) -> String {
    let raw = app_id
        .rsplit('.')
        .next()
        .unwrap_or(app_id)
        .replace(['-', '_'], " ");
    let words = raw
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>();
    if words.is_empty() {
        app_id.to_string()
    } else {
        words.join(" ")
    }
}

fn app_privacy_revoke_outcome(
    status: StatusCode,
    ok: bool,
    text: &str,
    request: RevokePermissionRequest,
) -> (StatusCode, AppPrivacyRevokeOutcome) {
    (
        status,
        AppPrivacyRevokeOutcome {
            ok,
            text: text.to_string(),
            table: request.table,
            id: request.id,
            app: request.app,
        },
    )
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
    use axum::http::StatusCode;

    use super::{
        friendly_app_name, normalize_revoke_request, parse_list_reply, permission_entries_from_ids,
        permission_id_is_safe, revoke_app_permission_outcome, PermissionEntry,
        RevokePermissionRequest,
    };

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

    #[test]
    fn permission_entries_label_app_keyed_grants() {
        assert_eq!(
            permission_entries_from_ids(vec!["org.gnome.Calculator".to_string()], true),
            vec![PermissionEntry {
                id: "org.gnome.Calculator".to_string(),
                app: "org.gnome.Calculator".to_string(),
                label: "Calculator".to_string(),
            }]
        );
        assert!(permission_entries_from_ids(vec!["bad/id".to_string()], true).is_empty());
        assert_eq!(
            friendly_app_name("com.example.photo-editor"),
            "Photo Editor"
        );
    }

    #[test]
    fn revoke_request_validation_is_allowlisted() {
        assert!(permission_id_is_safe("org.gnome.Calculator"));
        assert!(permission_id_is_safe("com.example.photo-editor"));
        assert!(!permission_id_is_safe(""));
        assert!(!permission_id_is_safe("org.gnome.Calculator;rm"));
        assert!(!permission_id_is_safe("bad/path"));

        let request = RevokePermissionRequest {
            table: "location".to_string(),
            id: "org.gnome.Calculator".to_string(),
            app: "org.gnome.Calculator".to_string(),
        };
        assert!(normalize_revoke_request("location", request).is_ok());

        let request = RevokePermissionRequest {
            table: "location".to_string(),
            id: "camera".to_string(),
            app: "org.gnome.Calculator".to_string(),
        };
        assert!(normalize_revoke_request("location", request).is_err());
    }

    #[test]
    fn revoke_rejects_unknown_and_resource_keyed_tables_before_dbus() {
        let request = RevokePermissionRequest {
            table: "unknown".to_string(),
            id: "org.gnome.Calculator".to_string(),
            app: "org.gnome.Calculator".to_string(),
        };
        let (status, outcome) = revoke_app_permission_outcome(request);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!outcome.ok);

        let request = RevokePermissionRequest {
            table: "devices".to_string(),
            id: "camera".to_string(),
            app: "org.gnome.Calculator".to_string(),
        };
        let (status, outcome) = revoke_app_permission_outcome(request);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!outcome.ok);
        assert!(outcome.text.contains("resource-keyed"));
    }
}
