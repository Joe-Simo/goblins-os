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

use axum::extract::Query;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

const MAX_SHORTCUTS: usize = 500;
const ENGINE_BINARY_PATH: &str = "/usr/libexec/goblins-os/goblins-textshortcuts-engine";
const ENGINE_COMPONENT_PATH: &str = "/usr/share/ibus/component/goblins-textshortcuts.xml";

#[derive(Clone, Serialize, Deserialize)]
pub struct TextShortcut {
    replace: String,
    with: String,
}

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
    ready: bool,
    detail: String,
}

pub async fn text_shortcuts_status() -> Json<TextShortcutsStatus> {
    Json(build_status(read_table()))
}

pub async fn set_text_shortcuts(
    Json(request): Json<SetTextShortcutsRequest>,
) -> (StatusCode, Json<TextShortcutsStatus>) {
    let table = sanitize_table(request.shortcuts);
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
    )
}

fn text_shortcuts_engine_status(
    ibus_available: bool,
    component_registered: bool,
    engine_binary_available: bool,
) -> TextShortcutsEngineStatus {
    let ready = ibus_available && component_registered && engine_binary_available;
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
        format!(
            "Text Shortcuts are saved, but the replacement engine isn't running on this session yet: {}.",
            missing.join("; ")
        )
    };
    TextShortcutsEngineStatus {
        ibus_available,
        component_registered,
        engine_binary_available,
        ready,
        detail,
    }
}

/// The replacement for an exactly-typed trigger, if the table has one. This is the
/// match the IBus engine performs on a word boundary. Pure + unit-tested.
fn find_replacement<'a>(trigger: &str, table: &'a [TextShortcut]) -> Option<&'a str> {
    table
        .iter()
        .find(|entry| entry.replace == trigger)
        .map(|entry| entry.with.as_str())
}

/// Trim, drop empties, drop a trigger that equals its replacement, de-duplicate by
/// trigger (last write wins), and cap the table. Pure + unit-tested so a malformed
/// edit can never persist a table the engine would choke on.
fn sanitize_table(shortcuts: Vec<TextShortcut>) -> Vec<TextShortcut> {
    let mut seen = std::collections::HashMap::new();
    let mut order = Vec::new();
    for entry in shortcuts {
        let replace = entry.replace.trim().to_string();
        let with = entry.with.trim().to_string();
        if replace.is_empty() || with.is_empty() || replace == with {
            continue;
        }
        if !seen.contains_key(&replace) {
            order.push(replace.clone());
        }
        seen.insert(replace, with);
    }
    order
        .into_iter()
        .take(MAX_SHORTCUTS)
        .map(|replace| {
            let with = seen.remove(&replace).unwrap_or_default();
            TextShortcut { replace, with }
        })
        .collect()
}

fn read_table() -> Vec<TextShortcut> {
    let Some(path) = table_path() else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    sanitize_table(serde_json::from_str(&raw).unwrap_or_default())
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
    use super::{find_replacement, sanitize_table, text_shortcuts_engine_status, TextShortcut};

    fn s(replace: &str, with: &str) -> TextShortcut {
        TextShortcut {
            replace: replace.to_string(),
            with: with.to_string(),
        }
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
        let table = sanitize_table(vec![
            s("  omw ", " on my way "), // trimmed
            s("x", "x"),                // trigger == replacement → dropped
            s("", "y"),                 // empty trigger → dropped
            s("z", ""),                 // empty replacement → dropped
            s("omw", "omw — updated"),  // duplicate trigger → last wins
        ]);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].replace, "omw");
        assert_eq!(table[0].with, "omw — updated");
    }

    #[test]
    fn engine_status_requires_ibus_component_and_binary() {
        let missing_all = text_shortcuts_engine_status(false, false, false);
        assert!(!missing_all.ready);
        assert!(!missing_all.ibus_available);
        assert!(!missing_all.component_registered);
        assert!(!missing_all.engine_binary_available);
        assert!(missing_all.detail.contains("IBus is not installed"));
        assert!(missing_all.detail.contains("component is not registered"));
        assert!(missing_all
            .detail
            .contains("engine binary is not installed"));

        let ibus_only = text_shortcuts_engine_status(true, false, false);
        assert!(!ibus_only.ready);
        assert!(ibus_only.ibus_available);
        assert!(ibus_only.detail.contains("component is not registered"));
        assert!(ibus_only.detail.contains("engine binary is not installed"));

        let ready = text_shortcuts_engine_status(true, true, true);
        assert!(ready.ready);
        assert_eq!(
            ready.detail,
            "Text Shortcuts expand as you type across the desktop."
        );
    }
}
