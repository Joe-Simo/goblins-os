//! Text Shortcuts substrate (the curated Replace→With table).
//!
//! The macOS "Text Replacement" altitude. Goblins owns a curated table stored as
//! JSON at `~/.config/goblins-os/text-shortcuts.json` (NOT a gsetting — it is
//! free-form user data), edited through this allowlisted bridge. The table needs no
//! model and ships ready; the IBus engine that actually commits the replacement
//! over `text-input-v3` is the deliberate, boot/login-adjacent follow-up, so this
//! reports `engine_available` honestly and the matching logic here is exactly what
//! that engine will use.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use axum::extract::Query;
use axum::http::StatusCode;
use axum::Json;
use goblins_os_textshortcuts_engine::{sanitize_shortcuts, TextShortcut};
use serde::{Deserialize, Serialize};

const ENGINE_BINARY_PATH: &str = "/usr/libexec/goblins-os/goblins-textshortcuts-engine";
const ENGINE_COMPONENT_PATH: &str = "/usr/share/ibus/component/goblins-textshortcuts.xml";
const INPUT_SOURCES_SCHEMA: &str = "org.gnome.desktop.input-sources";
const TEXTSHORTCUTS_INPUT_KIND: &str = "ibus";
const TEXTSHORTCUTS_INPUT_ID: &str = "goblins-textshortcuts";

#[derive(Serialize)]
pub struct TextShortcutsStatus {
    source: &'static str,
    /// Whether the IBus engine that applies replacements system-wide is present.
    engine_available: bool,
    engine: TextShortcutsEngineStatus,
    shortcuts: Vec<TextShortcut>,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetTextShortcutsRequest {
    shortcuts: Vec<TextShortcut>,
}

#[derive(Deserialize)]
pub struct PreviewQuery {
    trigger: String,
}

#[derive(Serialize)]
pub struct PreviewResult {
    trigger: String,
    replacement: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TextShortcutsEngineStatus {
    ibus_available: bool,
    component_registered: bool,
    engine_binary_available: bool,
    input_source_configured: bool,
    runtime_loop_available: bool,
    ready: bool,
    detail: String,
}

pub async fn text_shortcuts_status() -> Json<TextShortcutsStatus> {
    Json(build_status(read_table()))
}

pub async fn set_text_shortcuts(
    Json(request): Json<SetTextShortcutsRequest>,
) -> (StatusCode, Json<TextShortcutsStatus>) {
    let table = sanitize_shortcuts(request.shortcuts);
    match write_table(&table) {
        Ok(()) => (StatusCode::OK, Json(build_status(table))),
        Err(_) => {
            let mut status = build_status(read_table());
            status.detail =
                "Couldn't save Text Shortcuts (the configuration file is not writable)."
                    .to_string();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(status))
        }
    }
}

/// Preview what a trigger expands to (the editor's "try it" affordance), using the
/// exact same match the engine performs.
pub async fn preview_text_shortcut(Query(query): Query<PreviewQuery>) -> Json<PreviewResult> {
    let table = read_table();
    let replacement = find_replacement(&query.trigger, &table).map(str::to_string);
    Json(PreviewResult {
        trigger: query.trigger,
        replacement,
    })
}

fn build_status(shortcuts: Vec<TextShortcut>) -> TextShortcutsStatus {
    let engine = probe_engine_status();
    let detail = if engine.ready {
        "Text Shortcuts expand as you type across the desktop.".to_string()
    } else {
        engine.detail.clone()
    };
    TextShortcutsStatus {
        source: "goblins-os-core",
        engine_available: engine.ready,
        engine,
        shortcuts,
        detail,
    }
}

fn probe_engine_status() -> TextShortcutsEngineStatus {
    text_shortcuts_engine_status(
        command_on_path("ibus"),
        PathBuf::from(ENGINE_COMPONENT_PATH).is_file(),
        PathBuf::from(ENGINE_BINARY_PATH).is_file(),
        text_shortcuts_input_source_configured(),
        false,
    )
}

fn text_shortcuts_engine_status(
    ibus_available: bool,
    component_registered: bool,
    engine_binary_available: bool,
    input_source_configured: bool,
    runtime_loop_available: bool,
) -> TextShortcutsEngineStatus {
    let ready = ibus_available
        && component_registered
        && engine_binary_available
        && input_source_configured
        && runtime_loop_available;
    let detail = if ready {
        "Text Shortcuts expand as you type across the desktop.".to_string()
    } else {
        let mut missing = Vec::new();
        if !ibus_available {
            missing.push("IBus is not installed for this session");
        }
        if !component_registered {
            missing.push("the Goblins Text Shortcuts IBus component is not registered");
        }
        if !engine_binary_available {
            missing.push("the Goblins Text Shortcuts engine binary is not installed");
        }
        if !input_source_configured {
            missing.push("the Goblins Text Shortcuts IBus input source is not enabled");
        }
        if !runtime_loop_available {
            missing.push("the live IBus runtime loop is still pending CI/qemu proof");
        }
        format!(
            "Text Shortcuts are saved, but the replacement engine is not active on this session yet: {}.",
            missing.join("; ")
        )
    };
    TextShortcutsEngineStatus {
        ibus_available,
        component_registered,
        engine_binary_available,
        input_source_configured,
        runtime_loop_available,
        ready,
        detail,
    }
}

fn text_shortcuts_input_source_configured() -> bool {
    gsettings_get(INPUT_SOURCES_SCHEMA, "sources").is_some_and(|raw| {
        input_sources_contains(&raw, TEXTSHORTCUTS_INPUT_KIND, TEXTSHORTCUTS_INPUT_ID)
    })
}

fn input_sources_contains(gvariant: &str, kind: &str, id: &str) -> bool {
    let mut rest = gvariant;
    while let Some(open) = rest.find('(') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else { break };
        let values = single_quoted_strings(&after[..close]);
        if values.len() == 2 && values[0] == kind && values[1] == id {
            return true;
        }
        rest = &after[close + 1..];
    }
    false
}

fn single_quoted_strings(fragment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = fragment.chars();
    while let Some(value) = chars.next() {
        if value != '\'' {
            continue;
        }
        let mut field = String::new();
        loop {
            match chars.next() {
                None | Some('\'') => break,
                Some('\\') => {
                    if let Some(escaped) = chars.next() {
                        field.push(escaped);
                    }
                }
                Some(character) => field.push(character),
            }
        }
        out.push(field);
    }
    out
}

fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The replacement for an exactly-typed trigger, if the table has one. This is the
/// match the IBus engine performs on a word boundary. Pure + unit-tested.
fn find_replacement<'a>(trigger: &str, table: &'a [TextShortcut]) -> Option<&'a str> {
    table
        .iter()
        .find(|entry| entry.replace() == trigger)
        .map(TextShortcut::with_text)
}

fn read_table() -> Vec<TextShortcut> {
    let Some(path) = table_path() else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    sanitize_shortcuts(serde_json::from_str(&raw).unwrap_or_default())
}

fn write_table(table: &[TextShortcut]) -> std::io::Result<()> {
    let path = table_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no config home"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(table)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    fs::write(path, json)
}

fn table_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("goblins-os").join("text-shortcuts.json"))
}

fn command_on_path(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{
        find_replacement, input_sources_contains, text_shortcuts_engine_status, TextShortcut,
    };

    fn s(replace: &str, with: &str) -> TextShortcut {
        TextShortcut::new(replace, with)
    }

    #[test]
    fn finds_exact_trigger() {
        let table = vec![s("omw", "on my way"), s("teh", "the")];
        assert_eq!(find_replacement("omw", &table), Some("on my way"));
        assert_eq!(find_replacement("teh", &table), Some("the"));
        assert_eq!(find_replacement("nope", &table), None);
    }

    #[test]
    fn sanitize_trims_dedupes_and_drops_noise() {
        let table = super::sanitize_shortcuts(vec![
            s("  omw ", " on my way "), // trimmed
            s("x", "x"),                // trigger == replacement → dropped
            s("", "y"),                 // empty trigger → dropped
            s("z", ""),                 // empty replacement → dropped
            s("omw", "omw — updated"),  // duplicate trigger → last wins
        ]);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].replace(), "omw");
        assert_eq!(table[0].with_text(), "omw — updated");
    }

    #[test]
    fn engine_status_requires_ibus_component_and_binary() {
        let missing_all = text_shortcuts_engine_status(false, false, false, false, false);
        assert!(!missing_all.ready);
        assert!(!missing_all.ibus_available);
        assert!(!missing_all.component_registered);
        assert!(!missing_all.engine_binary_available);
        assert!(!missing_all.input_source_configured);
        assert!(!missing_all.runtime_loop_available);
        assert!(missing_all.detail.contains("IBus is not installed"));
        assert!(missing_all.detail.contains("component is not registered"));
        assert!(missing_all
            .detail
            .contains("engine binary is not installed"));
        assert!(missing_all.detail.contains("input source is not enabled"));
        assert!(missing_all.detail.contains("runtime loop is still pending"));

        let ibus_only = text_shortcuts_engine_status(true, false, false, false, false);
        assert!(!ibus_only.ready);
        assert!(ibus_only.ibus_available);
        assert!(ibus_only.detail.contains("component is not registered"));
        assert!(ibus_only.detail.contains("engine binary is not installed"));

        let installed_but_not_enabled =
            text_shortcuts_engine_status(true, true, true, false, false);
        assert!(!installed_but_not_enabled.ready);
        assert!(!installed_but_not_enabled.input_source_configured);
        assert!(!installed_but_not_enabled.runtime_loop_available);
        assert!(installed_but_not_enabled
            .detail
            .contains("input source is not enabled"));

        let ready = text_shortcuts_engine_status(true, true, true, true, true);
        assert!(ready.ready);
        assert_eq!(
            ready.detail,
            "Text Shortcuts expand as you type across the desktop."
        );
    }

    #[test]
    fn input_source_detection_requires_the_goblins_ibus_engine() {
        assert!(input_sources_contains(
            "[('xkb', 'us'), ('ibus', 'goblins-textshortcuts')]",
            "ibus",
            "goblins-textshortcuts"
        ));
        assert!(!input_sources_contains(
            "[('xkb', 'us'), ('ibus', 'libpinyin')]",
            "ibus",
            "goblins-textshortcuts"
        ));
        assert!(!input_sources_contains(
            "@a(ss) []",
            "ibus",
            "goblins-textshortcuts"
        ));
    }
}
