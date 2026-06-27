//! Goblins keyboard shortcuts (read-only) for Settings ▸ Keyboard.
//!
//! Reads the shortcuts Goblins OS owns — the `goblins-wm` window-management
//! keybindings (`org.goblins.shell.extensions.wm`) — and presents them as a
//! human-readable reference. The macOS "Keyboard ▸ Shortcuts" altitude. Rebinding
//! is a deliberate follow-up (a gated write to the extension schema), so this ships
//! read-only and honest-gated when the window-management schema isn't installed.

use std::process::{Command, Stdio};

use axum::Json;
use serde::Serialize;

const WM_SCHEMA: &str = "org.goblins.shell.extensions.wm";

/// The owned shortcuts, in display order: `(gsettings key, human label)`.
const SHORTCUTS: &[(&str, &str)] = &[
    ("mission-control", "Mission Control"),
    ("app-expose", "App Exposé"),
    ("window-switcher", "App switcher"),
    ("window-hud", "Window actions"),
    ("snap-left", "Snap left"),
    ("snap-right", "Snap right"),
    ("snap-top-left", "Snap top-left"),
    ("snap-top-right", "Snap top-right"),
    ("snap-bottom-left", "Snap bottom-left"),
    ("snap-bottom-right", "Snap bottom-right"),
    ("restore-window", "Restore window"),
    ("center-window", "Center window"),
    ("space-left", "Previous space"),
    ("space-right", "Next space"),
];

#[derive(Serialize)]
pub struct ShortcutEntry {
    action: String,
    bindings: Vec<String>,
}

#[derive(Serialize)]
pub struct ShortcutsStatus {
    source: &'static str,
    available: bool,
    shortcuts: Vec<ShortcutEntry>,
    detail: String,
}

pub async fn shortcuts_status() -> Json<ShortcutsStatus> {
    Json(build_shortcuts_status())
}

fn build_shortcuts_status() -> ShortcutsStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available, WM_SCHEMA);
    if !schema.available {
        return ShortcutsStatus {
            source: "goblins-os-core",
            available: false,
            shortcuts: Vec::new(),
            detail: "Keyboard shortcuts are unavailable here (the Goblins window-management schema is not installed).".to_string(),
        };
    }

    let shortcuts = SHORTCUTS
        .iter()
        .filter_map(|(key, label)| {
            let bindings = setting_strv(&schema, WM_SCHEMA, key)?;
            Some(ShortcutEntry {
                action: (*label).to_string(),
                bindings: bindings.iter().map(|b| humanize_accelerator(b)).collect(),
            })
        })
        .collect();

    ShortcutsStatus {
        source: "goblins-os-core",
        available: true,
        shortcuts,
        detail: "These are the Goblins window-management shortcuts for this desktop.".to_string(),
    }
}

/// Turn a GTK accelerator (`<Super><Shift>Left`) into a readable label
/// (`Super + Shift + Left`). Pure + unit-tested so the reference never misleads.
fn humanize_accelerator(accel: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut rest = accel;
    while let Some(start) = rest.find('<') {
        let Some(offset) = rest[start..].find('>') else {
            break;
        };
        let end = start + offset;
        let modifier = &rest[start + 1..end];
        let label = match modifier {
            "Primary" | "Control" => "Ctrl",
            other => other,
        };
        parts.push(label.to_string());
        rest = &rest[end + 1..];
    }
    let key = rest.trim();
    if !key.is_empty() {
        parts.push(key.to_string());
    }
    parts.join(" + ")
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
    Failed,
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

fn setting_strv(schema: &SchemaSnapshot, schema_name: &str, key: &str) -> Option<Vec<String>> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", schema_name, key])
        .ok()
        .and_then(|value| parse_gsettings_strv(&value))
}

/// Parse a gsettings `as` array (`['<Super>Up', 'F9']`) into its strings.
fn parse_gsettings_strv(value: &str) -> Option<Vec<String>> {
    let mut text = value.trim();
    if let Some(rest) = text.strip_prefix("@as ") {
        text = rest.trim();
    }
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    let mut values = Vec::new();
    let mut rest = inner.trim();

    while !rest.is_empty() {
        if rest.starts_with(',') {
            rest = rest[1..].trim_start();
            continue;
        }
        if !rest.starts_with('\'') {
            return None;
        }
        let mut escaped = false;
        let mut value = String::new();
        let mut end_index = None;
        for (index, ch) in rest[1..].char_indices() {
            if escaped {
                value.push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '\'' {
                end_index = Some(index + 2);
                break;
            }
            value.push(ch);
        }
        let end_index = end_index?;
        values.push(value);
        rest = rest[end_index..].trim_start();
        if rest.starts_with(',') {
            rest = rest[1..].trim_start();
        } else if !rest.is_empty() {
            return None;
        }
    }

    Some(values)
}

fn gsettings(args: &[&str]) -> Result<String, GSettingsError> {
    let output = Command::new("gsettings")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| GSettingsError::Missing)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(GSettingsError::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::{humanize_accelerator, parse_gsettings_strv};

    #[test]
    fn humanizes_accelerators() {
        assert_eq!(humanize_accelerator("<Super>Up"), "Super + Up");
        assert_eq!(
            humanize_accelerator("<Super><Shift>Left"),
            "Super + Shift + Left"
        );
        assert_eq!(humanize_accelerator("<Primary>space"), "Ctrl + space");
        assert_eq!(humanize_accelerator("F9"), "F9");
        assert_eq!(humanize_accelerator(""), "");
    }

    #[test]
    fn parses_gsettings_string_arrays() {
        assert_eq!(
            parse_gsettings_strv("['<Super>Up', 'F9']"),
            Some(vec!["<Super>Up".to_string(), "F9".to_string()])
        );
        assert_eq!(parse_gsettings_strv("@as []"), Some(Vec::new()));
        assert_eq!(parse_gsettings_strv("not-an-array"), None);
    }
}
