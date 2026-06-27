//! Today / Desktop Widgets substrate (widget registry + layout model).
//!
//! The macOS "Today view / widgets" altitude: a right-edge Today panel of glance
//! widgets. GNOME has no desktop widgets — this is an own surface. The layer-shell
//! panel itself (a new GTK crate + `gtk4-layer-shell`/`libgweather4`/`geoclue2`) is
//! the deliberate XL follow-up; this module ships the host-testable core — the widget
//! registry (with each widget's honest capability requirement) and the layout model
//! (normalize/order/dedup) stored in `org.goblins.os.today`. Nothing here renders.

use axum::Json;
use serde::{Deserialize, Serialize};

const SCHEMA: &str = "org.goblins.os.today";

/// The available glance widgets: `(id, name, requires)`. `requires` names the
/// capability the live widget needs so the panel can honest-gate it (never faking
/// weather/agenda/brief data when the source is absent).
const WIDGETS: &[(&str, &str, &str)] = &[
    ("date", "Date & agenda", "none"),
    ("world-clock", "World Clock", "none"),
    ("weather", "Weather", "location"),
    ("calendar", "Calendar", "calendar account"),
    ("brief", "Daily Brief", "on-device model"),
];

#[derive(Serialize)]
pub struct WidgetInfo {
    id: &'static str,
    name: &'static str,
    requires: &'static str,
}

#[derive(Serialize)]
pub struct TodayStatus {
    source: &'static str,
    schema_available: bool,
    widgets: Vec<WidgetInfo>,
    layout: Vec<String>,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetLayoutRequest {
    widgets: Vec<String>,
}

pub async fn today_status() -> Json<TodayStatus> {
    Json(build_status())
}

pub async fn set_today_layout(Json(request): Json<SetLayoutRequest>) -> Json<TodayStatus> {
    let layout = normalize_layout(request.widgets);
    let _ = write_layout(&layout);
    Json(build_status())
}

fn build_status() -> TodayStatus {
    let schema_available = schema_available(SCHEMA);
    let layout = if schema_available {
        normalize_layout(read_layout())
    } else {
        normalize_layout(default_layout())
    };
    TodayStatus {
        source: "goblins-os-core",
        schema_available,
        widgets: WIDGETS
            .iter()
            .map(|(id, name, requires)| WidgetInfo { id, name, requires })
            .collect(),
        layout,
        detail: if schema_available {
            "Today widgets are configured. Each widget shows live data or an honest empty state."
                .to_string()
        } else {
            "Today is using its default widget layout (preferences schema not installed)."
                .to_string()
        },
    }
}

/// Keep only known widget ids, drop duplicates, and preserve the requested order.
/// Pure + unit-tested so a malformed layout can never persist an unknown widget.
fn normalize_layout(requested: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    requested
        .into_iter()
        .filter(|id| is_known_widget(id))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn is_known_widget(id: &str) -> bool {
    WIDGETS.iter().any(|(known, _, _)| *known == id)
}

fn default_layout() -> Vec<String> {
    WIDGETS.iter().map(|(id, _, _)| (*id).to_string()).collect()
}

fn read_layout() -> Vec<String> {
    gsettings(&["get", SCHEMA, "enabled-widgets"])
        .ok()
        .map(|value| parse_gsettings_strv(&value))
        .unwrap_or_else(default_layout)
}

fn write_layout(layout: &[String]) -> Result<(), ()> {
    let quoted: Vec<String> = layout.iter().map(|id| format!("'{id}'")).collect();
    let value = format!("[{}]", quoted.join(", "));
    gsettings(&["set", SCHEMA, "enabled-widgets", &value]).map(|_| ())
}

/// Parse a gsettings `as` array (`['date', 'weather']`) into its strings.
fn parse_gsettings_strv(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\'' {
            continue;
        }
        let mut item = String::new();
        loop {
            match chars.next() {
                None | Some('\'') => break,
                Some('\\') => {
                    if let Some(escaped) = chars.next() {
                        item.push(escaped);
                    }
                }
                Some(ch) => item.push(ch),
            }
        }
        out.push(item);
    }
    out
}

fn schema_available(schema: &str) -> bool {
    gsettings(&["list-keys", schema])
        .map(|out| !out.trim().is_empty())
        .unwrap_or(false)
}

fn gsettings(args: &[&str]) -> Result<String, ()> {
    let output = std::process::Command::new("gsettings")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| ())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_layout, parse_gsettings_strv};

    #[test]
    fn normalize_keeps_known_order_and_dedupes() {
        let layout = normalize_layout(vec![
            "weather".to_string(),
            "date".to_string(),
            "weather".to_string(), // duplicate → dropped
            "bogus".to_string(),   // unknown → dropped
            "world-clock".to_string(),
        ]);
        assert_eq!(
            layout,
            vec![
                "weather".to_string(),
                "date".to_string(),
                "world-clock".to_string()
            ]
        );
    }

    #[test]
    fn parses_gsettings_string_arrays() {
        assert_eq!(
            parse_gsettings_strv("['date', 'weather']"),
            vec!["date".to_string(), "weather".to_string()]
        );
        assert_eq!(parse_gsettings_strv("@as []"), Vec::<String>::new());
    }
}
