//! Goblins window-management preferences owned by the session extension.
//!
//! The GNOME Shell extension owns the live behavior. Core only exposes a narrow,
//! allowlisted bridge for Settings so user-facing controls never write arbitrary
//! schemas or claim success when the Goblins WM schema is absent.

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::{bounded_session_command_output, probe_timeout};

const WM_SCHEMA: &str = "org.goblins.shell.extensions.wm";

const HOT_CORNERS: &[HotCornerSpec] = &[
    HotCornerSpec {
        id: "hot-corner-top-left",
        title: "Top-left corner",
    },
    HotCornerSpec {
        id: "hot-corner-top-right",
        title: "Top-right corner",
    },
    HotCornerSpec {
        id: "hot-corner-bottom-left",
        title: "Bottom-left corner",
    },
    HotCornerSpec {
        id: "hot-corner-bottom-right",
        title: "Bottom-right corner",
    },
];

const HOT_CORNER_ACTIONS: &[HotCornerActionSpec] = &[
    HotCornerActionSpec {
        id: "none",
        label: "Off",
    },
    HotCornerActionSpec {
        id: "mission-control",
        label: "Workspace Overview",
    },
    HotCornerActionSpec {
        id: "app-expose",
        label: "Focused App Windows",
    },
];

#[derive(Clone, Copy)]
struct HotCornerSpec {
    id: &'static str,
    title: &'static str,
}

#[derive(Clone, Copy)]
struct HotCornerActionSpec {
    id: &'static str,
    label: &'static str,
}

#[derive(Serialize)]
pub struct WindowManagementStatus {
    source: &'static str,
    gsettings_available: bool,
    available: bool,
    hot_corners: HotCornersStatus,
    detail: String,
}

#[derive(Serialize)]
pub struct HotCornersStatus {
    schema_available: bool,
    corners: Vec<HotCornerSetting>,
    detail: String,
}

#[derive(Serialize)]
pub struct HotCornerSetting {
    id: String,
    title: String,
    action: String,
    action_label: String,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetHotCornerRequest {
    corner: String,
    action: String,
}

#[derive(Serialize)]
pub struct HotCornerOutcome {
    ok: bool,
    corner: String,
    action: String,
    text: String,
}

pub async fn window_management_status() -> Json<WindowManagementStatus> {
    Json(build_window_management_status())
}

pub async fn set_hot_corner(
    Json(request): Json<SetHotCornerRequest>,
) -> (StatusCode, Json<HotCornerOutcome>) {
    let (status, outcome) = set_hot_corner_outcome(request);
    (status, Json(outcome))
}

fn build_window_management_status() -> WindowManagementStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available, WM_SCHEMA);
    if !schema.available {
        return WindowManagementStatus {
            source: "goblins-os-core",
            gsettings_available,
            available: false,
            hot_corners: HotCornersStatus {
                schema_available: false,
                corners: Vec::new(),
                detail: "Hot corners need the Goblins window manager session.".to_string(),
            },
            detail:
                "Window-management preferences are waiting for the Goblins window manager session."
                    .to_string(),
        };
    }

    let corners = HOT_CORNERS
        .iter()
        .filter(|corner| schema.has_key(corner.id))
        .map(|corner| {
            let action = setting_string(&schema, WM_SCHEMA, corner.id)
                .and_then(|value| hot_corner_action_by_id(&value).map(|action| action.id))
                .unwrap_or("none");
            let action_spec = hot_corner_action_by_id(action).expect("normalized action exists");
            HotCornerSetting {
                id: corner.id.to_string(),
                title: corner.title.to_string(),
                action: action_spec.id.to_string(),
                action_label: action_spec.label.to_string(),
                detail: hot_corner_action_detail(action_spec.id).to_string(),
            }
        })
        .collect::<Vec<_>>();
    let all_keys_available = HOT_CORNERS.iter().all(|corner| schema.has_key(corner.id));
    let detail = if all_keys_available {
        "Choose which Goblins desktop surface opens when the pointer reaches each corner."
    } else {
        "Hot corners are read-only because the Goblins window-management schema is incomplete."
    };

    WindowManagementStatus {
        source: "goblins-os-core",
        gsettings_available,
        available: all_keys_available,
        hot_corners: HotCornersStatus {
            schema_available: all_keys_available,
            corners,
            detail: detail.to_string(),
        },
        detail: detail.to_string(),
    }
}

fn set_hot_corner_outcome(request: SetHotCornerRequest) -> (StatusCode, HotCornerOutcome) {
    let Some(corner) = hot_corner_by_id(&request.corner) else {
        return hot_corner_response(
            StatusCode::BAD_REQUEST,
            false,
            "Goblins OS only changes allowlisted hot corners.",
            request.corner,
            request.action,
        );
    };
    let Some(action) = hot_corner_action_by_id(&request.action) else {
        return hot_corner_response(
            StatusCode::BAD_REQUEST,
            false,
            "Hot corners can open Workspace Overview, show Focused App Windows, or stay off.",
            corner.id.to_string(),
            request.action,
        );
    };

    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available, WM_SCHEMA);
    if !schema.available || !schema.has_key(corner.id) {
        return hot_corner_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Hot corners need the Goblins window manager session before they can be changed.",
            corner.id.to_string(),
            action.id.to_string(),
        );
    }

    let encoded = encode_gsettings_string(action.id);
    match gsettings(&["set", WM_SCHEMA, corner.id, &encoded]) {
        Ok(_) => hot_corner_response(
            StatusCode::OK,
            true,
            &hot_corner_saved_text(corner.title, action.id),
            corner.id.to_string(),
            action.id.to_string(),
        ),
        Err(error) => hot_corner_response(
            StatusCode::BAD_GATEWAY,
            false,
            &format!("Hot corner change failed: {}", error.detail()),
            corner.id.to_string(),
            action.id.to_string(),
        ),
    }
}

fn hot_corner_by_id(id: &str) -> Option<HotCornerSpec> {
    HOT_CORNERS
        .iter()
        .copied()
        .find(|corner| corner.id == id.trim())
}

fn hot_corner_action_by_id(id: &str) -> Option<HotCornerActionSpec> {
    HOT_CORNER_ACTIONS
        .iter()
        .copied()
        .find(|action| action.id == id.trim())
}

fn hot_corner_action_detail(action: &str) -> &'static str {
    match action {
        "mission-control" => "Pointing to this corner opens Workspace Overview.",
        "app-expose" => "Pointing to this corner shows windows from the focused app.",
        _ => "This corner is off.",
    }
}

fn hot_corner_saved_text(corner_title: &str, action: &str) -> String {
    match hot_corner_action_by_id(action) {
        Some(action) if action.id != "none" => {
            format!("{corner_title} now opens {}.", action.label)
        }
        _ => format!("{corner_title} is off."),
    }
}

fn encode_gsettings_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn parse_gsettings_string(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut escaped = false;
    let mut out = String::new();
    for ch in inner.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    if escaped {
        return None;
    }
    Some(out)
}

struct SchemaSnapshot {
    available: bool,
    keys: Vec<String>,
}

impl SchemaSnapshot {
    fn unavailable() -> Self {
        Self {
            available: false,
            keys: Vec::new(),
        }
    }

    fn has_key(&self, key: &str) -> bool {
        self.keys.iter().any(|candidate| candidate == key)
    }
}

enum GSettingsError {
    Missing,
    Failed(String),
}

impl GSettingsError {
    fn detail(&self) -> String {
        match self {
            GSettingsError::Missing => "desktop preferences are missing".to_string(),
            GSettingsError::Failed(detail) if !detail.is_empty() => detail.clone(),
            GSettingsError::Failed(_) => "desktop preferences rejected the change".to_string(),
        }
    }
}

fn schema_snapshot(gsettings_available: bool, schema: &str) -> SchemaSnapshot {
    if !gsettings_available {
        return SchemaSnapshot::unavailable();
    }
    match gsettings(&["list-keys", schema]) {
        Ok(stdout) => SchemaSnapshot {
            available: true,
            keys: stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect(),
        },
        Err(_) => SchemaSnapshot::unavailable(),
    }
}

fn setting_string(schema: &SchemaSnapshot, schema_name: &str, key: &str) -> Option<String> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", schema_name, key])
        .ok()
        .and_then(|value| parse_gsettings_string(&value))
}

fn gsettings(args: &[&str]) -> Result<String, GSettingsError> {
    let output = bounded_session_command_output("gsettings", args, probe_timeout())
        .map_err(|_| GSettingsError::Missing)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(GSettingsError::Failed(
            String::from_utf8_lossy(&output.stderr)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        ))
    }
}

fn hot_corner_response(
    status: StatusCode,
    ok: bool,
    text: &str,
    corner: String,
    action: String,
) -> (StatusCode, HotCornerOutcome) {
    (
        status,
        HotCornerOutcome {
            ok,
            corner,
            action,
            text: text.to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        encode_gsettings_string, hot_corner_action_by_id, hot_corner_action_detail,
        hot_corner_by_id, hot_corner_saved_text, parse_gsettings_string,
    };

    #[test]
    fn hot_corner_targets_are_allowlisted() {
        assert_eq!(
            hot_corner_by_id("hot-corner-top-left").unwrap().title,
            "Top-left corner"
        );
        assert!(hot_corner_by_id("org.gnome.desktop.interface").is_none());
        assert!(hot_corner_by_id("hot-corner-top-left\n").is_some());
    }

    #[test]
    fn hot_corner_actions_are_allowlisted() {
        assert_eq!(
            hot_corner_action_by_id("mission-control").unwrap().label,
            "Workspace Overview"
        );
        assert_eq!(
            hot_corner_action_detail("app-expose"),
            "Pointing to this corner shows windows from the focused app."
        );
        assert!(hot_corner_action_by_id("lock-screen").is_none());
    }

    #[test]
    fn hot_corner_strings_round_trip_for_gsettings() {
        assert_eq!(
            encode_gsettings_string("mission-control"),
            "'mission-control'"
        );
        assert_eq!(
            parse_gsettings_string("'app-expose'").as_deref(),
            Some("app-expose")
        );
        assert_eq!(
            parse_gsettings_string("'mission\\'control'").as_deref(),
            Some("mission'control")
        );
        assert!(parse_gsettings_string("mission-control").is_none());
    }

    #[test]
    fn hot_corner_saved_copy_is_specific() {
        assert_eq!(
            hot_corner_saved_text("Top-left corner", "mission-control"),
            "Top-left corner now opens Workspace Overview."
        );
        assert_eq!(
            hot_corner_saved_text("Top-left corner", "none"),
            "Top-left corner is off."
        );
    }
}
