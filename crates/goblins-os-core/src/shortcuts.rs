//! Goblins keyboard shortcuts for Settings ▸ Keyboard.
//!
//! Reads the shortcuts Goblins OS owns — the `goblins-wm` window-management
//! keybindings (`org.goblins.shell.extensions.wm`) — and presents them as a
//! human-readable reference. Rebinding goes through the same allowlisted bridge:
//! only known Goblins WM actions can be set/reset, chords are grammar-checked,
//! conflicts against the owned action table are rejected, and Caps Lock remapping
//! edits only the reversible `ctrl:*`/`caps:*` token in GNOME's xkb options.

use std::process::{Command, Stdio};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

const WM_SCHEMA: &str = "org.goblins.shell.extensions.wm";
const INPUT_SOURCES_SCHEMA: &str = "org.gnome.desktop.input-sources";

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
    id: String,
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

#[derive(Deserialize)]
pub struct SetShortcutBindingRequest {
    action: String,
    #[serde(default)]
    bindings: Option<Vec<String>>,
    #[serde(default)]
    reset: bool,
}

#[derive(Deserialize)]
pub struct SetModifierRemapRequest {
    target: String,
    value: String,
}

#[derive(Serialize)]
pub struct KeyboardShortcutOutcome {
    ok: bool,
    text: String,
    action: String,
    bindings: Vec<String>,
}

#[derive(Serialize)]
pub struct ModifierRemapOutcome {
    ok: bool,
    text: String,
    target: String,
    value: String,
    xkb_options: Vec<String>,
}

pub async fn shortcuts_status() -> Json<ShortcutsStatus> {
    Json(build_shortcuts_status())
}

pub async fn set_shortcut_binding(
    Json(request): Json<SetShortcutBindingRequest>,
) -> (StatusCode, Json<KeyboardShortcutOutcome>) {
    let (status, outcome) = set_shortcut_binding_outcome(request);
    (status, Json(outcome))
}

pub async fn set_modifier_remap(
    Json(request): Json<SetModifierRemapRequest>,
) -> (StatusCode, Json<ModifierRemapOutcome>) {
    let (status, outcome) = set_modifier_remap_outcome(request);
    (status, Json(outcome))
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
                id: (*key).to_string(),
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

fn set_shortcut_binding_outcome(
    request: SetShortcutBindingRequest,
) -> (StatusCode, KeyboardShortcutOutcome) {
    let Some((key, label)) = shortcut_spec_by_id(&request.action) else {
        return shortcut_response(
            StatusCode::BAD_REQUEST,
            false,
            "Goblins OS only changes allowlisted keyboard shortcuts.",
            request.action,
            Vec::new(),
        );
    };
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available, WM_SCHEMA);
    if !schema.available || !schema.has_key(key) {
        return shortcut_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Keyboard shortcuts are read-only because the Goblins window-management schema is not installed.",
            key.to_string(),
            Vec::new(),
        );
    }
    if request.reset {
        return match gsettings(&["reset", WM_SCHEMA, key]) {
            Ok(_) => shortcut_response(
                StatusCode::OK,
                true,
                &format!("Reset {label} to the Goblins OS default shortcut."),
                key.to_string(),
                setting_strv(&schema_snapshot(true, WM_SCHEMA), WM_SCHEMA, key).unwrap_or_default(),
            ),
            Err(error) => shortcut_response(
                StatusCode::BAD_GATEWAY,
                false,
                &format!("Keyboard shortcut reset failed: {}", error.detail()),
                key.to_string(),
                Vec::new(),
            ),
        };
    }

    let bindings = match request.bindings {
        Some(bindings) => match normalize_accelerators(bindings) {
            Ok(bindings) => bindings,
            Err(message) => {
                return shortcut_response(
                    StatusCode::BAD_REQUEST,
                    false,
                    &message,
                    key.to_string(),
                    Vec::new(),
                )
            }
        },
        None => {
            return shortcut_response(
                StatusCode::BAD_REQUEST,
                false,
                "Keyboard shortcut changes need at least one binding or reset=true.",
                key.to_string(),
                Vec::new(),
            )
        }
    };

    if let Some(conflict) = shortcut_conflict(&schema, key, &bindings) {
        return shortcut_response(
            StatusCode::CONFLICT,
            false,
            &format!("That shortcut is already used by {}.", conflict),
            key.to_string(),
            bindings,
        );
    }
    let encoded = encode_gsettings_strv(&bindings);
    match gsettings(&["set", WM_SCHEMA, key, &encoded]) {
        Ok(_) => shortcut_response(
            StatusCode::OK,
            true,
            &format!("Updated {label}."),
            key.to_string(),
            bindings,
        ),
        Err(error) => shortcut_response(
            StatusCode::BAD_GATEWAY,
            false,
            &format!("Keyboard shortcut change failed: {}", error.detail()),
            key.to_string(),
            bindings,
        ),
    }
}

fn set_modifier_remap_outcome(
    request: SetModifierRemapRequest,
) -> (StatusCode, ModifierRemapOutcome) {
    if request.target.trim() != "caps-lock" {
        return modifier_response(
            StatusCode::BAD_REQUEST,
            false,
            "Only the Caps Lock modifier remap is supported.",
            request.target,
            request.value,
            Vec::new(),
        );
    }
    let value = request.value.trim();
    if value != "control" && value != "default" && value != "caps-lock" {
        return modifier_response(
            StatusCode::BAD_REQUEST,
            false,
            "Caps Lock can be remapped to control or restored to default.",
            request.target,
            request.value,
            Vec::new(),
        );
    }
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available, INPUT_SOURCES_SCHEMA);
    if !schema.available || !schema.has_key("xkb-options") {
        return modifier_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Modifier keys are read-only because input-source options are not reported by this session.",
            request.target,
            request.value,
            Vec::new(),
        );
    }
    let current = setting_strv(&schema, INPUT_SOURCES_SCHEMA, "xkb-options").unwrap_or_default();
    let options = remap_caps_lock_options(&current, value == "control");
    let encoded = encode_gsettings_strv(&options);
    match gsettings(&["set", INPUT_SOURCES_SCHEMA, "xkb-options", &encoded]) {
        Ok(_) => modifier_response(
            StatusCode::OK,
            true,
            if value == "control" {
                "Caps Lock now works as Control."
            } else {
                "Caps Lock modifier behavior was restored to the desktop default."
            },
            request.target,
            request.value,
            options,
        ),
        Err(error) => modifier_response(
            StatusCode::BAD_GATEWAY,
            false,
            &format!("Modifier remap failed: {}", error.detail()),
            request.target,
            request.value,
            options,
        ),
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

fn shortcut_spec_by_id(id: &str) -> Option<(&'static str, &'static str)> {
    SHORTCUTS.iter().copied().find(|(key, _)| *key == id.trim())
}

fn normalize_accelerators(bindings: Vec<String>) -> Result<Vec<String>, String> {
    if bindings.is_empty() || bindings.len() > 4 {
        return Err("Keyboard shortcuts need one to four bindings.".to_string());
    }
    let mut out = Vec::new();
    for binding in bindings {
        let binding = normalize_accelerator(&binding)?;
        if out.iter().any(|existing| existing == &binding) {
            return Err("Keyboard shortcut bindings cannot repeat.".to_string());
        }
        out.push(binding);
    }
    Ok(out)
}

fn normalize_accelerator(binding: &str) -> Result<String, String> {
    let binding = binding.trim();
    if binding.is_empty() || binding.len() > 80 || binding.chars().any(char::is_control) {
        return Err("Keyboard shortcut bindings must be short printable accelerators.".to_string());
    }
    let mut rest = binding;
    let mut modifiers = Vec::new();
    while let Some(after_start) = rest.strip_prefix('<') {
        let Some(end) = after_start.find('>') else {
            return Err("Keyboard shortcut modifiers must use <Modifier> syntax.".to_string());
        };
        let modifier = &after_start[..end];
        if !matches!(
            modifier,
            "Super" | "Shift" | "Control" | "Primary" | "Alt" | "Meta"
        ) {
            return Err("Keyboard shortcut modifier is not allowlisted.".to_string());
        }
        if modifiers.iter().any(|existing| existing == &modifier) {
            return Err("Keyboard shortcut modifiers cannot repeat.".to_string());
        }
        modifiers.push(modifier);
        rest = &after_start[end + 1..];
    }
    if !shortcut_key_is_safe(rest) {
        return Err("Keyboard shortcut key is not allowlisted.".to_string());
    }
    if modifiers.is_empty() && !plain_shortcut_key_allowed(rest) {
        return Err("Plain shortcuts are limited to function and navigation keys.".to_string());
    }
    Ok(format!(
        "{}{}",
        modifiers
            .iter()
            .map(|modifier| format!("<{modifier}>"))
            .collect::<Vec<_>>()
            .join(""),
        rest
    ))
}

fn shortcut_key_is_safe(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn plain_shortcut_key_allowed(key: &str) -> bool {
    matches!(
        key,
        "Print"
            | "Escape"
            | "Tab"
            | "BackSpace"
            | "Delete"
            | "Insert"
            | "Home"
            | "End"
            | "Page_Up"
            | "Page_Down"
            | "Left"
            | "Right"
            | "Up"
            | "Down"
    ) || key.strip_prefix('F').is_some_and(|rest| {
        rest.parse::<u8>()
            .is_ok_and(|value| (1..=24).contains(&value))
    })
}

fn shortcut_conflict(schema: &SchemaSnapshot, action: &str, bindings: &[String]) -> Option<String> {
    for (key, label) in SHORTCUTS {
        if *key == action {
            continue;
        }
        let existing = setting_strv(schema, WM_SCHEMA, key)?;
        if existing
            .iter()
            .any(|candidate| bindings.iter().any(|binding| binding == candidate))
        {
            return Some((*label).to_string());
        }
    }
    None
}

fn remap_caps_lock_options(options: &[String], control: bool) -> Vec<String> {
    let mut out = options
        .iter()
        .filter(|option| !option.starts_with("ctrl:") && !option.starts_with("caps:"))
        .cloned()
        .collect::<Vec<_>>();
    if control {
        out.push("ctrl:nocaps".to_string());
    }
    out
}

fn encode_gsettings_strv(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
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
        Err(GSettingsError::Failed(
            String::from_utf8_lossy(&output.stderr)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        ))
    }
}

fn shortcut_response(
    status: StatusCode,
    ok: bool,
    text: &str,
    action: String,
    bindings: Vec<String>,
) -> (StatusCode, KeyboardShortcutOutcome) {
    (
        status,
        KeyboardShortcutOutcome {
            ok,
            text: text.to_string(),
            action,
            bindings,
        },
    )
}

fn modifier_response(
    status: StatusCode,
    ok: bool,
    text: &str,
    target: String,
    value: String,
    xkb_options: Vec<String>,
) -> (StatusCode, ModifierRemapOutcome) {
    (
        status,
        ModifierRemapOutcome {
            ok,
            text: text.to_string(),
            target,
            value,
            xkb_options,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        encode_gsettings_strv, humanize_accelerator, normalize_accelerators, parse_gsettings_strv,
        remap_caps_lock_options,
    };

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

    #[test]
    fn keyboard_shortcut_bindings_are_validated_and_encoded() {
        assert_eq!(
            normalize_accelerators(vec![" <Super><Shift>Left ".to_string()]).unwrap(),
            vec!["<Super><Shift>Left".to_string()]
        );
        assert!(normalize_accelerators(vec!["a".to_string()]).is_err());
        assert!(normalize_accelerators(vec!["<Super><Super>a".to_string()]).is_err());
        assert!(normalize_accelerators(vec!["<Hyper>a".to_string()]).is_err());
        assert!(normalize_accelerators(vec!["F13".to_string()]).is_ok());
        assert_eq!(
            encode_gsettings_strv(&["<Super>Up".to_string(), "F9".to_string()]),
            "['<Super>Up', 'F9']"
        );
    }

    #[test]
    fn caps_lock_remap_preserves_unrelated_xkb_options() {
        let current = vec![
            "grp:alt_shift_toggle".to_string(),
            "ctrl:nocaps".to_string(),
            "compose:ralt".to_string(),
        ];
        assert_eq!(
            remap_caps_lock_options(&current, true),
            vec![
                "grp:alt_shift_toggle".to_string(),
                "compose:ralt".to_string(),
                "ctrl:nocaps".to_string(),
            ]
        );
        assert_eq!(
            remap_caps_lock_options(&current, false),
            vec![
                "grp:alt_shift_toggle".to_string(),
                "compose:ralt".to_string(),
            ]
        );
    }
}
