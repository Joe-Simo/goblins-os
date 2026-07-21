//! Text Shortcuts substrate (the curated Replace→With table).
//!
//! The macOS "Text Replacement" altitude. Goblins owns a curated table stored as
//! JSON at the desktop user's `~/.config/goblins-os/text-shortcuts.json` (NOT a
//! gsetting — it is free-form user data). The system core never reads or writes
//! its service-account home: every table access uses two fixed, typed operations
//! on the allowlisted session bridge, which owns the desktop user's private file.

use std::{
    fs,
    path::{Path, PathBuf},
};

use axum::extract::Query;
use axum::http::StatusCode;
use axum::Json;
use goblins_os_textshortcuts_engine::{
    sanitize_shortcuts, text_shortcuts_table_is_within_size_limit, TextShortcut,
};
use serde::{Deserialize, Serialize};

use crate::session_bridge::{
    self, SessionBridgeResult, TextShortcutsBridgeResult, TextShortcutsRuntimeStatusResult,
};

const ENGINE_BINARY_PATH: &str = "/usr/libexec/goblins-os/goblins-textshortcuts-engine";
const ENGINE_COMPONENT_PATH: &str = "/usr/share/ibus/component/goblins-textshortcuts.xml";
const INPUT_SOURCES_SCHEMA: &str = "org.gnome.desktop.input-sources";
const TEXTSHORTCUTS_INPUT_KIND: &str = "ibus";
const TEXTSHORTCUTS_INPUT_ID: &str = "goblins-textshortcuts";
const AUTOCORRECT_MODEL_ENV: &str = "GOBLINS_TEXTSHORTCUTS_AUTOCORRECT_MODEL";
const AUTOCORRECT_MODEL_DIR: &str = "/usr/share/goblins-os/models/autocorrect";
const HUNSPELL_DICTIONARY_DIRS: &[&str] = &["/usr/share/hunspell", "/usr/share/myspell"];
const MAX_PREVIEW_TRIGGER_BYTES: usize = 256;

#[derive(Serialize)]
pub struct TextShortcutsStatus {
    source: &'static str,
    /// Whether the IBus engine that applies replacements system-wide is present.
    engine_available: bool,
    engine: TextShortcutsEngineStatus,
    autocorrect: TextShortcutsAutocorrectStatus,
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
    detail: String,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TextShortcutsAutocorrectStatus {
    available: bool,
    model_available: bool,
    dictionary_available: bool,
    enabled: bool,
    detail: String,
}

pub async fn text_shortcuts_status() -> (StatusCode, Json<TextShortcutsStatus>) {
    match session_bridge::text_shortcuts_read() {
        TextShortcutsBridgeResult::Success(table) => (StatusCode::OK, Json(build_status(table))),
        failure => table_failure_response(failure, TableOperation::Read),
    }
}

pub async fn set_text_shortcuts(
    Json(request): Json<SetTextShortcutsRequest>,
) -> (StatusCode, Json<TextShortcutsStatus>) {
    let table = sanitize_shortcuts(request.shortcuts);
    if !text_shortcuts_table_is_within_size_limit(&table) {
        let mut status = build_status(Vec::new());
        status.detail =
            "Text Shortcuts couldn't be saved because the private table exceeds 48 KiB."
                .to_string();
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(status));
    }
    match session_bridge::text_shortcuts_write(&table) {
        TextShortcutsBridgeResult::Success(committed) => {
            (StatusCode::OK, Json(build_status(committed)))
        }
        failure => table_failure_response(failure, TableOperation::Write),
    }
}

/// Preview what a trigger expands to (the editor's "try it" affordance), using the
/// exact same match the engine performs.
pub async fn preview_text_shortcut(
    Query(query): Query<PreviewQuery>,
) -> (StatusCode, Json<PreviewResult>) {
    if query.trigger.len() > MAX_PREVIEW_TRIGGER_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(PreviewResult {
                trigger: String::new(),
                replacement: None,
                detail: "Text Shortcut preview triggers are limited to 256 bytes.".to_string(),
            }),
        );
    }
    match session_bridge::text_shortcuts_read() {
        TextShortcutsBridgeResult::Success(table) => {
            let replacement = find_replacement(&query.trigger, &table).map(str::to_string);
            (
                StatusCode::OK,
                Json(PreviewResult {
                    trigger: query.trigger,
                    replacement,
                    detail: "Previewed from your private desktop Text Shortcuts table.".to_string(),
                }),
            )
        }
        failure => {
            let (status, detail) = table_failure(failure, TableOperation::Read);
            (
                status,
                Json(PreviewResult {
                    trigger: query.trigger,
                    replacement: None,
                    detail: detail.to_string(),
                }),
            )
        }
    }
}

#[derive(Clone, Copy)]
enum TableOperation {
    Read,
    Write,
}

fn table_failure_response(
    failure: TextShortcutsBridgeResult,
    operation: TableOperation,
) -> (StatusCode, Json<TextShortcutsStatus>) {
    let (status_code, detail) = table_failure(failure, operation);
    let mut status = build_status(Vec::new());
    status.detail = detail.to_string();
    (status_code, Json(status))
}

fn table_failure(
    failure: TextShortcutsBridgeResult,
    operation: TableOperation,
) -> (StatusCode, &'static str) {
    match failure {
        TextShortcutsBridgeResult::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Text Shortcuts are waiting for the private desktop session storage bridge.",
        ),
        TextShortcutsBridgeResult::InvalidResponse => (
            StatusCode::BAD_GATEWAY,
            "Text Shortcuts couldn't verify the desktop session storage response.",
        ),
        TextShortcutsBridgeResult::Rejected => match operation {
            TableOperation::Read => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Text Shortcuts couldn't read the private desktop table.",
            ),
            TableOperation::Write => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Text Shortcuts couldn't save the private desktop table.",
            ),
        },
        TextShortcutsBridgeResult::Success(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Text Shortcuts encountered an internal storage-state error.",
        ),
    }
}

fn build_status(shortcuts: Vec<TextShortcut>) -> TextShortcutsStatus {
    let engine = probe_engine_status();
    let autocorrect = probe_autocorrect_status();
    let detail = if engine.ready {
        "Text Shortcuts expand as you type across the desktop.".to_string()
    } else {
        engine.detail.clone()
    };
    TextShortcutsStatus {
        source: "goblins-os-core",
        engine_available: engine.ready,
        engine,
        autocorrect,
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
        text_shortcuts_runtime_loop_live(),
    )
}

/// Live readiness comes only from two independent facts in the real session:
/// the allowlisted bridge must see the Goblins IBus engine selected, and the
/// adapter must have published a fresh heartbeat after a successful child
/// protocol response while focused. Either probe failing degrades honestly.
fn text_shortcuts_runtime_loop_live() -> bool {
    text_shortcuts_runtime_loop_live_from(
        session_bridge::ibus_engine(),
        session_bridge::text_shortcuts_runtime_status(),
    )
}

fn text_shortcuts_runtime_loop_live_from(
    active_engine: SessionBridgeResult,
    runtime_status: TextShortcutsRuntimeStatusResult,
) -> bool {
    matches!(
        active_engine,
        SessionBridgeResult::Success(engine) if engine.trim() == TEXTSHORTCUTS_INPUT_ID
    ) && matches!(
        runtime_status,
        TextShortcutsRuntimeStatusResult::Success(status) if status.ready()
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
            missing.push(
                "the Goblins Text Shortcuts engine lacks an active, fresh focused runtime response in this session",
            );
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

fn probe_autocorrect_status() -> TextShortcutsAutocorrectStatus {
    text_shortcuts_autocorrect_status(
        autocorrect_model_available(),
        hunspell_dictionary_available(),
    )
}

fn text_shortcuts_autocorrect_status(
    model_available: bool,
    dictionary_available: bool,
) -> TextShortcutsAutocorrectStatus {
    let available = model_available || dictionary_available;
    let detail = if available {
        "Autocorrect resources are present, but live autocorrect remains off until it is deliberately enabled and proven."
            .to_string()
    } else {
        "Autocorrect is off because no local model or Hunspell dictionary is installed.".to_string()
    };
    TextShortcutsAutocorrectStatus {
        available,
        model_available,
        dictionary_available,
        enabled: false,
        detail,
    }
}

fn autocorrect_model_available() -> bool {
    std::env::var_os(AUTOCORRECT_MODEL_ENV)
        .map(PathBuf::from)
        .is_some_and(|path| path.is_file())
        || directory_has_extension(Path::new(AUTOCORRECT_MODEL_DIR), "gguf")
}

fn hunspell_dictionary_available() -> bool {
    HUNSPELL_DICTIONARY_DIRS
        .iter()
        .any(|path| directory_has_extension(Path::new(path), "dic"))
}

fn directory_has_extension(path: &Path, extension: &str) -> bool {
    fs::read_dir(path).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        })
    })
}

fn text_shortcuts_input_source_configured() -> bool {
    // The core is a system service and deliberately has no desktop-user D-Bus
    // session. Keep this read in the user service, behind the bridge's exact
    // gsettings schema/key allowlist, just like the active-engine probe.
    input_source_configured_from_bridge(session_bridge::gsettings(&[
        "get",
        INPUT_SOURCES_SCHEMA,
        "sources",
    ]))
}

fn input_source_configured_from_bridge(result: SessionBridgeResult) -> bool {
    matches!(
        result,
        SessionBridgeResult::Success(raw)
            if input_sources_contains(
                &raw,
                TEXTSHORTCUTS_INPUT_KIND,
                TEXTSHORTCUTS_INPUT_ID
            )
    )
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

/// The replacement for an exactly-typed trigger, if the table has one. This is the
/// match the IBus engine performs on a word boundary. Pure + unit-tested.
fn find_replacement<'a>(trigger: &str, table: &'a [TextShortcut]) -> Option<&'a str> {
    table
        .iter()
        .find(|entry| entry.replace() == trigger)
        .map(TextShortcut::with_text)
}

fn command_on_path(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{
        find_replacement, input_source_configured_from_bridge, input_sources_contains,
        table_failure, text_shortcuts_autocorrect_status, text_shortcuts_engine_status,
        text_shortcuts_runtime_loop_live_from, TableOperation, TextShortcut,
    };
    use crate::session_bridge::{
        SessionBridgeResult, TextShortcutsBridgeResult, TextShortcutsRuntimeStatus,
        TextShortcutsRuntimeStatusResult,
    };
    use axum::http::StatusCode;

    fn s(replace: &str, with: &str) -> TextShortcut {
        TextShortcut::new(replace, with)
    }

    fn runtime_status(
        focused: bool,
        enabled: bool,
        surrounding_text_supported: bool,
        snapshot_valid: bool,
        child_alive: bool,
        last_response_ok: bool,
    ) -> TextShortcutsRuntimeStatus {
        serde_json::from_value(serde_json::json!({
            "schema": "goblins-os.text-shortcuts-runtime-status.v1",
            "instance_id": "0123456789abcdef0123456789abcdef",
            "focus_generation": 7,
            "runtime_generation": 11,
            "sequence": 13,
            "monotonic_ns": 17,
            "focused": focused,
            "enabled": enabled,
            "surrounding_text_supported": surrounding_text_supported,
            "snapshot_valid": snapshot_valid,
            "child_alive": child_alive,
            "last_response_ok": last_response_ok,
        }))
        .unwrap()
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
            s("bad\0trigger", "value"), // NUL trigger → dropped
            s("bad", "value\0text"),    // NUL replacement → dropped
            s("omw", "omw — updated"),  // duplicate trigger → last wins
        ]);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].replace(), "omw");
        assert_eq!(table[0].with_text(), "omw — updated");
    }

    #[test]
    fn table_bridge_failures_have_stable_http_statuses() {
        assert_eq!(
            table_failure(TextShortcutsBridgeResult::Unavailable, TableOperation::Read).0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            table_failure(
                TextShortcutsBridgeResult::InvalidResponse,
                TableOperation::Read
            )
            .0,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            table_failure(TextShortcutsBridgeResult::Rejected, TableOperation::Read).0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            table_failure(TextShortcutsBridgeResult::Rejected, TableOperation::Write).0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
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
        assert!(missing_all
            .detail
            .contains("fresh focused runtime response"));

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
    fn runtime_loop_requires_active_engine_and_fresh_live_child_status() {
        assert!(text_shortcuts_runtime_loop_live_from(
            SessionBridgeResult::Success("goblins-textshortcuts".to_string()),
            TextShortcutsRuntimeStatusResult::Success(runtime_status(
                true, true, true, true, true, true,
            )),
        ));

        for readiness_signals in [
            [false, true, true, true, true, true],
            [true, false, true, true, true, true],
            [true, true, false, true, true, true],
            [true, true, true, false, true, true],
            [true, true, true, true, false, true],
            [true, true, true, true, true, false],
        ] {
            let [focused, enabled, surrounding_text_supported, snapshot_valid, child_alive, last_response_ok] =
                readiness_signals;
            assert!(!text_shortcuts_runtime_loop_live_from(
                SessionBridgeResult::Success("goblins-textshortcuts".to_string()),
                TextShortcutsRuntimeStatusResult::Success(runtime_status(
                    focused,
                    enabled,
                    surrounding_text_supported,
                    snapshot_valid,
                    child_alive,
                    last_response_ok,
                )),
            ));
        }

        assert!(!text_shortcuts_runtime_loop_live_from(
            SessionBridgeResult::Success("xkb:us::eng".to_string()),
            TextShortcutsRuntimeStatusResult::Success(runtime_status(
                true, true, true, true, true, true,
            )),
        ));
        assert!(!text_shortcuts_runtime_loop_live_from(
            SessionBridgeResult::Success("goblins-textshortcuts".to_string()),
            TextShortcutsRuntimeStatusResult::Rejected("runtime heartbeat is stale".to_string()),
        ));
        assert!(!text_shortcuts_runtime_loop_live_from(
            SessionBridgeResult::Unavailable,
            TextShortcutsRuntimeStatusResult::Unavailable,
        ));
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

    #[test]
    fn input_source_readiness_accepts_configured_bridge_success() {
        assert!(input_source_configured_from_bridge(
            SessionBridgeResult::Success(
                "[('xkb', 'us'), ('ibus', 'goblins-textshortcuts')]".to_string()
            )
        ));
        assert!(!input_source_configured_from_bridge(
            SessionBridgeResult::Success("[('xkb', 'us')]".to_string())
        ));
    }

    #[test]
    fn input_source_readiness_fails_closed_on_bridge_failure() {
        assert!(!input_source_configured_from_bridge(
            SessionBridgeResult::Failed("gsettings request failed".to_string())
        ));
    }

    #[test]
    fn input_source_readiness_fails_closed_on_malformed_bridge_success() {
        assert!(!input_source_configured_from_bridge(
            SessionBridgeResult::Success("not a gsettings sources value".to_string())
        ));
    }

    #[test]
    fn input_source_readiness_fails_closed_when_bridge_is_unavailable() {
        assert!(!input_source_configured_from_bridge(
            SessionBridgeResult::Unavailable
        ));
    }

    #[test]
    fn autocorrect_status_never_enables_without_live_engine_proof() {
        let missing = text_shortcuts_autocorrect_status(false, false);
        assert!(!missing.available);
        assert!(!missing.model_available);
        assert!(!missing.dictionary_available);
        assert!(!missing.enabled);
        assert!(missing
            .detail
            .contains("no local model or Hunspell dictionary"));

        let dictionary = text_shortcuts_autocorrect_status(false, true);
        assert!(dictionary.available);
        assert!(!dictionary.model_available);
        assert!(dictionary.dictionary_available);
        assert!(!dictionary.enabled);
        assert!(dictionary.detail.contains("remains off"));

        let model = text_shortcuts_autocorrect_status(true, false);
        assert!(model.available);
        assert!(model.model_available);
        assert!(!model.dictionary_available);
        assert!(!model.enabled);
    }
}
