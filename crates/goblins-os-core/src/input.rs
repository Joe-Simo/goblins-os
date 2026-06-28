//! Keyboard and pointer preferences for Settings.
//!
//! Goblins OS keeps desktop input preferences behind an allowlisted settings
//! bridge so the Settings GUI cannot mutate arbitrary schemas or keys.

use std::{path::Path, process::Command};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEYBOARD_SCHEMA: &str = "org.gnome.desktop.peripherals.keyboard";
const MOUSE_SCHEMA: &str = "org.gnome.desktop.peripherals.mouse";
const TOUCHPAD_SCHEMA: &str = "org.gnome.desktop.peripherals.touchpad";
const INPUT_SOURCES_SCHEMA: &str = "org.gnome.desktop.input-sources";
const CJK_INPUT_ENGINE_SPECS: [InputEnginePackageSpec; 3] = [
    InputEnginePackageSpec {
        language: "Chinese",
        label: "Pinyin (Chinese)",
        abbreviation: "PY",
        kind: "ibus",
        id: "libpinyin",
        package: "ibus-libpinyin",
        component_xml: "/usr/share/ibus/component/libpinyin.xml",
        engine_binary: "/usr/libexec/ibus-engine-libpinyin",
    },
    InputEnginePackageSpec {
        language: "Japanese",
        label: "Japanese (Anthy)",
        abbreviation: "あ",
        kind: "ibus",
        id: "anthy",
        package: "ibus-anthy",
        component_xml: "/usr/share/ibus/component/anthy.xml",
        engine_binary: "/usr/libexec/ibus-engine-anthy",
    },
    InputEnginePackageSpec {
        language: "Korean",
        label: "Korean (Hangul)",
        abbreviation: "한",
        kind: "ibus",
        id: "hangul",
        package: "ibus-hangul",
        component_xml: "/usr/share/ibus/component/hangul.xml",
        engine_binary: "/usr/libexec/ibus-engine-hangul",
    },
];

#[derive(Serialize)]
pub struct InputStatus {
    source: &'static str,
    gsettings_available: bool,
    keyboard: KeyboardInputStatus,
    mouse: MouseInputStatus,
    touchpad: TouchpadInputStatus,
    input_sources: InputSourcesStatus,
    input_engine_packages: InputEnginePackagesStatus,
    detail: String,
}

#[derive(Serialize)]
pub struct KeyboardInputStatus {
    schema_available: bool,
    repeat: Option<bool>,
    delay_ms: Option<u32>,
    repeat_interval_ms: Option<u32>,
    remember_numlock_state: Option<bool>,
    detail: String,
}

#[derive(Serialize)]
pub struct MouseInputStatus {
    schema_available: bool,
    speed: Option<f64>,
    natural_scroll: Option<bool>,
    left_handed: Option<bool>,
    middle_click_emulation: Option<bool>,
    detail: String,
}

#[derive(Serialize)]
pub struct TouchpadInputStatus {
    schema_available: bool,
    speed: Option<f64>,
    tap_to_click: Option<bool>,
    natural_scroll: Option<bool>,
    two_finger_scrolling_enabled: Option<bool>,
    disable_while_typing: Option<bool>,
    detail: String,
}

/// One configured keyboard input source (IME / layout) from
/// `org.gnome.desktop.input-sources sources` — e.g. `("xkb", "us")` or
/// `("ibus", "libpinyin")`. The read substrate for IME/CJK; add/reorder + the
/// `Super+Space` switching are the deferred (boot/keybinding-sensitive) follow-up.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct InputSourceEntry {
    kind: String,
    id: String,
}

#[derive(Serialize)]
pub struct InputSourcesStatus {
    schema_available: bool,
    sources: Vec<InputSourceEntry>,
    addable_sources: Vec<InputSourceChoice>,
    add_detail: String,
    detail: String,
}

#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
pub struct InputSourceChoice {
    kind: String,
    id: String,
    label: String,
    detail: String,
}

#[derive(Serialize)]
pub struct InputEnginePackagesStatus {
    engines: Vec<InputEnginePackageStatus>,
    installed_count: usize,
    all_installed: bool,
    detail: String,
}

#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
pub struct InputEnginePackageStatus {
    language: &'static str,
    label: &'static str,
    abbreviation: &'static str,
    kind: &'static str,
    id: &'static str,
    package: &'static str,
    component_xml: &'static str,
    engine_binary: &'static str,
    installed: bool,
}

#[derive(Deserialize)]
pub struct SetInputPreferenceRequest {
    target: InputPreferenceTarget,
    value: Value,
}

#[derive(Deserialize)]
pub struct SetInputSourcesRequest {
    sources: Vec<InputSourceEntry>,
}

#[derive(Deserialize)]
pub struct AddInputSourceRequest {
    kind: String,
    id: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InputPreferenceTarget {
    KeyboardRepeat,
    KeyboardDelayMs,
    KeyboardRepeatIntervalMs,
    KeyboardRememberNumlockState,
    MouseSpeed,
    MouseNaturalScroll,
    MouseLeftHanded,
    MouseMiddleClickEmulation,
    TouchpadSpeed,
    TouchpadTapToClick,
    TouchpadNaturalScroll,
    TouchpadTwoFingerScrolling,
    TouchpadDisableWhileTyping,
}

#[derive(Serialize)]
pub struct InputPreferenceOutcome {
    ok: bool,
    target: &'static str,
    text: String,
}

#[derive(Serialize)]
pub struct InputSourcesOutcome {
    ok: bool,
    sources: Vec<InputSourceEntry>,
    text: String,
}

#[derive(Serialize)]
pub struct SwitchInputSourceOutcome {
    ok: bool,
    switched: bool,
    source_count: usize,
    current_index: Option<u32>,
    current: Option<InputSourceEntry>,
    text: String,
}

struct IbusEngineProbe {
    available: bool,
    engine_ids: Vec<String>,
    detail: String,
}

enum GSettingsError {
    Missing,
    Failed(String),
}

struct SchemaSnapshot {
    available: bool,
    keys: Vec<String>,
}

#[derive(Clone, Copy)]
enum InputValueKind {
    Bool,
    U32(fn(u32) -> u32),
    F64(fn(f64) -> f64),
}

#[derive(Clone, Copy)]
struct InputTargetSpec {
    target: &'static str,
    schema: &'static str,
    key: &'static str,
    label: &'static str,
    kind: InputValueKind,
}

enum InputPreferenceValue {
    Bool(bool),
    U32(u32),
    F64(f64),
}

#[derive(Clone, Copy)]
struct InputEnginePackageSpec {
    language: &'static str,
    label: &'static str,
    abbreviation: &'static str,
    kind: &'static str,
    id: &'static str,
    package: &'static str,
    component_xml: &'static str,
    engine_binary: &'static str,
}

pub async fn input_status() -> Json<InputStatus> {
    Json(build_input_status())
}

pub async fn set_input_preference(
    Json(request): Json<SetInputPreferenceRequest>,
) -> (StatusCode, Json<InputPreferenceOutcome>) {
    set_input_preference_outcome(request)
}

pub async fn set_input_sources(
    Json(request): Json<SetInputSourcesRequest>,
) -> (StatusCode, Json<InputSourcesOutcome>) {
    set_input_sources_outcome(request)
}

pub async fn add_input_source(
    Json(request): Json<AddInputSourceRequest>,
) -> (StatusCode, Json<InputSourcesOutcome>) {
    add_input_source_outcome(request)
}

pub async fn switch_to_next_input_source() -> (StatusCode, Json<SwitchInputSourceOutcome>) {
    switch_to_next_input_source_outcome()
}

fn build_input_status() -> InputStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let keyboard_schema = schema_snapshot(gsettings_available, KEYBOARD_SCHEMA);
    let mouse_schema = schema_snapshot(gsettings_available, MOUSE_SCHEMA);
    let touchpad_schema = schema_snapshot(gsettings_available, TOUCHPAD_SCHEMA);
    let input_sources_schema = schema_snapshot(gsettings_available, INPUT_SOURCES_SCHEMA);
    let sources = setting_raw(&input_sources_schema, INPUT_SOURCES_SCHEMA, "sources")
        .map(|raw| parse_input_sources(&raw))
        .unwrap_or_default();
    let input_engine_packages = input_engine_packages_status();
    let ibus_probe = ibus_engine_probe();
    let (addable_sources, add_detail) =
        addable_input_source_choices(&sources, &input_engine_packages.engines, &ibus_probe);

    InputStatus {
        source: "goblins-os-core",
        gsettings_available,
        keyboard: KeyboardInputStatus {
            schema_available: keyboard_schema.available,
            repeat: setting_bool(&keyboard_schema, KEYBOARD_SCHEMA, "repeat"),
            delay_ms: setting_u32(&keyboard_schema, KEYBOARD_SCHEMA, "delay")
                .map(normalized_keyboard_delay),
            repeat_interval_ms: setting_u32(&keyboard_schema, KEYBOARD_SCHEMA, "repeat-interval")
                .map(normalized_keyboard_repeat_interval),
            remember_numlock_state: setting_bool(
                &keyboard_schema,
                KEYBOARD_SCHEMA,
                "remember-numlock-state",
            ),
            detail: schema_detail(
                gsettings_available,
                keyboard_schema.available,
                "Keyboard",
                KEYBOARD_SCHEMA,
            ),
        },
        mouse: MouseInputStatus {
            schema_available: mouse_schema.available,
            speed: setting_f64(&mouse_schema, MOUSE_SCHEMA, "speed").map(normalized_unit_speed),
            natural_scroll: setting_bool(&mouse_schema, MOUSE_SCHEMA, "natural-scroll"),
            left_handed: setting_bool(&mouse_schema, MOUSE_SCHEMA, "left-handed"),
            middle_click_emulation: setting_bool(
                &mouse_schema,
                MOUSE_SCHEMA,
                "middle-click-emulation",
            ),
            detail: schema_detail(
                gsettings_available,
                mouse_schema.available,
                "Mouse",
                MOUSE_SCHEMA,
            ),
        },
        touchpad: TouchpadInputStatus {
            schema_available: touchpad_schema.available,
            speed: setting_f64(&touchpad_schema, TOUCHPAD_SCHEMA, "speed")
                .map(normalized_unit_speed),
            tap_to_click: setting_bool(&touchpad_schema, TOUCHPAD_SCHEMA, "tap-to-click"),
            natural_scroll: setting_bool(&touchpad_schema, TOUCHPAD_SCHEMA, "natural-scroll"),
            two_finger_scrolling_enabled: setting_bool(
                &touchpad_schema,
                TOUCHPAD_SCHEMA,
                "two-finger-scrolling-enabled",
            ),
            disable_while_typing: setting_bool(
                &touchpad_schema,
                TOUCHPAD_SCHEMA,
                "disable-while-typing",
            ),
            detail: schema_detail(
                gsettings_available,
                touchpad_schema.available,
                "Trackpad",
                TOUCHPAD_SCHEMA,
            ),
        },
        input_sources: InputSourcesStatus {
            schema_available: input_sources_schema.available,
            sources,
            addable_sources,
            add_detail,
            detail: schema_detail(
                gsettings_available,
                input_sources_schema.available,
                "Input sources",
                INPUT_SOURCES_SCHEMA,
            ),
        },
        input_engine_packages,
        detail: input_status_detail(gsettings_available),
    }
}

fn input_engine_packages_status() -> InputEnginePackagesStatus {
    let engines = cjk_engine_package_statuses_with(|path| Path::new(path).is_file());
    let installed_count = engines.iter().filter(|engine| engine.installed).count();
    let all_installed = installed_count == engines.len();
    InputEnginePackagesStatus {
        detail: input_engine_packages_detail(installed_count, engines.len()),
        engines,
        installed_count,
        all_installed,
    }
}

fn cjk_engine_package_statuses_with(
    file_exists: impl Fn(&str) -> bool,
) -> Vec<InputEnginePackageStatus> {
    CJK_INPUT_ENGINE_SPECS
        .iter()
        .map(|spec| {
            let installed = file_exists(spec.component_xml) && file_exists(spec.engine_binary);
            InputEnginePackageStatus {
                language: spec.language,
                label: spec.label,
                abbreviation: spec.abbreviation,
                kind: spec.kind,
                id: spec.id,
                package: spec.package,
                component_xml: spec.component_xml,
                engine_binary: spec.engine_binary,
                installed,
            }
        })
        .collect()
}

fn input_engine_packages_detail(installed_count: usize, total_count: usize) -> String {
    if installed_count == total_count {
        format!(
            "CJK input engines are installed for this image. {installed_count} of {total_count} engine packages are ready."
        )
    } else {
        format!(
            "CJK input engines are not fully installed in this runtime. {installed_count} of {total_count} engine packages are ready."
        )
    }
}

fn ibus_engine_probe() -> IbusEngineProbe {
    match Command::new("ibus").arg("list-engine").output() {
        Ok(output) if output.status.success() => {
            let engine_ids = parse_ibus_list_engine(&String::from_utf8_lossy(&output.stdout));
            IbusEngineProbe {
                available: true,
                detail: if engine_ids.is_empty() {
                    "IBus is available, but it did not report any addable engines.".to_string()
                } else {
                    format!("IBus reported {} installed engines.", engine_ids.len())
                },
                engine_ids,
            }
        }
        Ok(output) => IbusEngineProbe {
            available: false,
            engine_ids: Vec::new(),
            detail: {
                let detail = gsettings_error_detail(
                    &String::from_utf8_lossy(&output.stderr),
                    &String::from_utf8_lossy(&output.stdout),
                );
                if detail.is_empty() {
                    "IBus did not report installed engines in this session.".to_string()
                } else {
                    format!("IBus did not report installed engines: {detail}")
                }
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => IbusEngineProbe {
            available: false,
            engine_ids: Vec::new(),
            detail: "IBus is not installed in this session, so input sources cannot be added here."
                .to_string(),
        },
        Err(_) => IbusEngineProbe {
            available: false,
            engine_ids: Vec::new(),
            detail: "IBus could not be inspected in this session, so input sources cannot be added here."
                .to_string(),
        },
    }
}

fn parse_ibus_list_engine(stdout: &str) -> Vec<String> {
    let mut engine_ids = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some(first) = line.split_whitespace().next() else {
            continue;
        };
        let token = first
            .trim_matches(|ch: char| ch == '*' || ch == '-' || ch == ':')
            .trim();
        if token.eq_ignore_ascii_case("language") || token.eq_ignore_ascii_case("engines") {
            continue;
        }
        if input_source_id_is_safe(token) && !engine_ids.iter().any(|id| id == token) {
            engine_ids.push(token.to_string());
        }
    }
    engine_ids
}

fn addable_input_source_choices(
    configured: &[InputSourceEntry],
    packages: &[InputEnginePackageStatus],
    ibus_probe: &IbusEngineProbe,
) -> (Vec<InputSourceChoice>, String) {
    if !ibus_probe.available {
        return (Vec::new(), ibus_probe.detail.clone());
    }

    let choices = packages
        .iter()
        .filter(|engine| engine.installed)
        .filter(|engine| engine.kind == "ibus")
        .filter(|engine| ibus_probe.engine_ids.iter().any(|id| id == engine.id))
        .filter(|engine| {
            !configured
                .iter()
                .any(|source| source.kind == engine.kind && source.id == engine.id)
        })
        .map(|engine| InputSourceChoice {
            kind: engine.kind.to_string(),
            id: engine.id.to_string(),
            label: engine.label.to_string(),
            detail: format!(
                "{} input method from {}. It is listed by IBus and can be added to this session.",
                engine.language, engine.package
            ),
        })
        .collect::<Vec<_>>();

    let detail = if !choices.is_empty() {
        format!(
            "{} installed CJK input method{} can be added.",
            choices.len(),
            if choices.len() == 1 { "" } else { "s" }
        )
    } else if packages
        .iter()
        .filter(|engine| engine.installed)
        .filter(|engine| engine.kind == "ibus")
        .all(|engine| {
            configured
                .iter()
                .any(|source| source.kind == engine.kind && source.id == engine.id)
        })
    {
        "All installed CJK input methods are already configured for this session.".to_string()
    } else {
        "No installed CJK input methods are currently reported by IBus for this session."
            .to_string()
    };

    (choices, detail)
}

/// Read a raw gsettings value (unparsed) when the key exists — used for the
/// `a(ss)` input-sources array that the scalar getters don't cover.
fn setting_raw(schema: &SchemaSnapshot, schema_name: &str, key: &str) -> Option<String> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", schema_name, key]).ok()
}

/// Parse the `a(ss)` GVariant from `org.gnome.desktop.input-sources sources`
/// (e.g. `[('xkb', 'us'), ('ibus', 'libpinyin')]`) into ordered entries. Pure —
/// unit-tested. Tolerant of spacing and of an empty / `@a(ss) []` value.
fn parse_input_sources(gvariant: &str) -> Vec<InputSourceEntry> {
    let mut out = Vec::new();
    let mut rest = gvariant;
    while let Some(open) = rest.find('(') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else { break };
        let strings = single_quoted_strings(&after[..close]);
        if strings.len() == 2 {
            out.push(InputSourceEntry {
                kind: strings[0].clone(),
                id: strings[1].clone(),
            });
        }
        rest = &after[close + 1..];
    }
    out
}

/// Extract single-quoted string literals from a GVariant fragment, honoring
/// backslash escapes (input-source ids are ASCII, but escapes are handled safely).
fn single_quoted_strings(fragment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = fragment.chars();
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

fn set_input_preference_outcome(
    request: SetInputPreferenceRequest,
) -> (StatusCode, Json<InputPreferenceOutcome>) {
    let spec = input_target_spec(request.target);
    let value = match parse_preference_value(spec, &request.value) {
        Ok(value) => value,
        Err(text) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(InputPreferenceOutcome {
                    ok: false,
                    target: spec.target,
                    text,
                }),
            );
        }
    };

    if gsettings(&["list-schemas"]).is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(InputPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so input preferences cannot be changed in this session.".to_string(),
            }),
        );
    }

    let schema = schema_snapshot(true, spec.schema);
    if !schema.available || !schema.has_key(spec.key) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(InputPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: format!(
                    "{} is not ready because the required preference is not reported by this desktop session.",
                    spec.label
                ),
            }),
        );
    }

    let encoded = encode_preference_value(&value);
    match gsettings(&["set", spec.schema, spec.key, &encoded]) {
        Ok(_) => (
            StatusCode::OK,
            Json(InputPreferenceOutcome {
                ok: true,
                target: spec.target,
                text: input_preference_success_detail(spec, &value),
            }),
        ),
        Err(GSettingsError::Missing) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(InputPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so input preferences cannot be changed in this session.".to_string(),
            }),
        ),
        Err(GSettingsError::Failed(detail)) => (
            StatusCode::BAD_GATEWAY,
            Json(InputPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: if detail.is_empty() {
                    format!("{} could not be saved by the desktop session.", spec.label)
                } else {
                    format!("{} could not be saved: {detail}", spec.label)
                },
            }),
        ),
    }
}

fn set_input_sources_outcome(
    request: SetInputSourcesRequest,
) -> (StatusCode, Json<InputSourcesOutcome>) {
    let sources = match normalize_input_sources(request.sources) {
        Ok(sources) => sources,
        Err(text) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(InputSourcesOutcome {
                    ok: false,
                    sources: Vec::new(),
                    text,
                }),
            );
        }
    };

    if gsettings(&["list-schemas"]).is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(InputSourcesOutcome {
                ok: false,
                sources,
                text: "Desktop preferences are not ready, so input sources cannot be changed in this session.".to_string(),
            }),
        );
    }

    let schema = schema_snapshot(true, INPUT_SOURCES_SCHEMA);
    if !schema.available || !schema.has_key("sources") {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(InputSourcesOutcome {
                ok: false,
                sources,
                text: "Input sources are not ready because the desktop session does not report the input source list.".to_string(),
            }),
        );
    }

    let encoded = encode_input_sources(&sources);
    match gsettings(&["set", INPUT_SOURCES_SCHEMA, "sources", &encoded]) {
        Ok(_) => {
            let count = sources.len();
            (
                StatusCode::OK,
                Json(InputSourcesOutcome {
                    ok: true,
                    sources,
                    text: if count == 1 {
                        "Input sources were updated. 1 source is configured.".to_string()
                    } else {
                        format!("Input sources were updated. {count} sources are configured.")
                    },
                }),
            )
        }
        Err(GSettingsError::Missing) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(InputSourcesOutcome {
                ok: false,
                sources,
                text: "Desktop preferences are not ready, so input sources cannot be changed in this session.".to_string(),
            }),
        ),
        Err(GSettingsError::Failed(detail)) => (
            StatusCode::BAD_GATEWAY,
            Json(InputSourcesOutcome {
                ok: false,
                sources,
                text: if detail.is_empty() {
                    "Input sources could not be saved by the desktop session.".to_string()
                } else {
                    format!("Input sources could not be saved: {detail}")
                },
            }),
        ),
    }
}

fn add_input_source_outcome(
    request: AddInputSourceRequest,
) -> (StatusCode, Json<InputSourcesOutcome>) {
    if gsettings(&["list-schemas"]).is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(InputSourcesOutcome {
                ok: false,
                sources: Vec::new(),
                text: "Desktop preferences are not ready, so input sources cannot be added in this session.".to_string(),
            }),
        );
    }

    let schema = schema_snapshot(true, INPUT_SOURCES_SCHEMA);
    if !schema.available || !schema.has_key("sources") {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(InputSourcesOutcome {
                ok: false,
                sources: Vec::new(),
                text: "Input sources are not ready because the desktop session does not report the input source list.".to_string(),
            }),
        );
    }

    let current = setting_raw(&schema, INPUT_SOURCES_SCHEMA, "sources")
        .map(|raw| parse_input_sources(&raw))
        .unwrap_or_default();
    let packages = input_engine_packages_status();
    let ibus_probe = ibus_engine_probe();
    let requested = InputSourceEntry {
        kind: request.kind,
        id: request.id,
    };
    let sources = match input_sources_with_added_choice(
        &current,
        requested,
        &packages.engines,
        &ibus_probe,
    ) {
        Ok(sources) => sources,
        Err(text) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(InputSourcesOutcome {
                    ok: false,
                    sources: current,
                    text,
                }),
            );
        }
    };

    set_input_sources_outcome(SetInputSourcesRequest { sources })
}

fn switch_to_next_input_source_outcome() -> (StatusCode, Json<SwitchInputSourceOutcome>) {
    if gsettings(&["list-schemas"]).is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SwitchInputSourceOutcome {
                ok: false,
                switched: false,
                source_count: 0,
                current_index: None,
                current: None,
                text: "Desktop preferences are not ready, so input sources cannot be switched in this session.".to_string(),
            }),
        );
    }

    let schema = schema_snapshot(true, INPUT_SOURCES_SCHEMA);
    if !schema.available || !schema.has_key("sources") || !schema.has_key("current") {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SwitchInputSourceOutcome {
                ok: false,
                switched: false,
                source_count: 0,
                current_index: None,
                current: None,
                text: "Input source switching is not ready because this desktop session does not report both the source list and current source.".to_string(),
            }),
        );
    }

    let sources = setting_raw(&schema, INPUT_SOURCES_SCHEMA, "sources")
        .map(|raw| parse_input_sources(&raw))
        .unwrap_or_default();
    let current = match setting_u32(&schema, INPUT_SOURCES_SCHEMA, "current") {
        Some(current) => current,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(SwitchInputSourceOutcome {
                    ok: false,
                    switched: false,
                    source_count: sources.len(),
                    current_index: None,
                    current: None,
                    text: "Input source switching is not ready because the current source index is not reported clearly by this session.".to_string(),
                }),
            );
        }
    };

    let next = match next_input_source_index(&sources, current) {
        Ok(Some(next)) => next,
        Ok(None) => {
            return (
                StatusCode::OK,
                Json(SwitchInputSourceOutcome {
                    ok: true,
                    switched: false,
                    source_count: sources.len(),
                    current_index: Some(current),
                    current: sources.get(current as usize).cloned(),
                    text: "Only one input source is configured, so Super+Space opens the launcher."
                        .to_string(),
                }),
            );
        }
        Err(text) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SwitchInputSourceOutcome {
                    ok: false,
                    switched: false,
                    source_count: sources.len(),
                    current_index: Some(current),
                    current: None,
                    text,
                }),
            );
        }
    };

    let encoded = next.to_string();
    match gsettings(&["set", INPUT_SOURCES_SCHEMA, "current", &encoded]) {
        Ok(_) => (
            StatusCode::OK,
            Json(SwitchInputSourceOutcome {
                ok: true,
                switched: true,
                source_count: sources.len(),
                current_index: Some(next),
                current: sources.get(next as usize).cloned(),
                text: format!("Switched to input source {} of {}.", next + 1, sources.len()),
            }),
        ),
        Err(GSettingsError::Missing) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SwitchInputSourceOutcome {
                ok: false,
                switched: false,
                source_count: sources.len(),
                current_index: Some(current),
                current: sources.get(current as usize).cloned(),
                text: "Desktop preferences are not ready, so input sources cannot be switched in this session.".to_string(),
            }),
        ),
        Err(GSettingsError::Failed(detail)) => (
            StatusCode::BAD_GATEWAY,
            Json(SwitchInputSourceOutcome {
                ok: false,
                switched: false,
                source_count: sources.len(),
                current_index: Some(current),
                current: sources.get(current as usize).cloned(),
                text: if detail.is_empty() {
                    "The current input source could not be changed by the desktop session.".to_string()
                } else {
                    format!("The current input source could not be changed: {detail}")
                },
            }),
        ),
    }
}

fn next_input_source_index(
    sources: &[InputSourceEntry],
    current: u32,
) -> Result<Option<u32>, String> {
    if sources.len() < 2 {
        return Ok(None);
    }
    let current = current as usize;
    if current >= sources.len() {
        return Err(
            "Input source switching is paused because the current source index is outside the configured source list."
                .to_string(),
        );
    }
    Ok(Some(((current + 1) % sources.len()) as u32))
}

fn input_sources_with_added_choice(
    current: &[InputSourceEntry],
    requested: InputSourceEntry,
    packages: &[InputEnginePackageStatus],
    ibus_probe: &IbusEngineProbe,
) -> Result<Vec<InputSourceEntry>, String> {
    let requested = normalize_input_sources(vec![requested])?
        .into_iter()
        .next()
        .expect("normalize_input_sources returns one entry for one valid source");
    let (choices, _) = addable_input_source_choices(current, packages, ibus_probe);
    if !choices
        .iter()
        .any(|choice| choice.kind == requested.kind && choice.id == requested.id)
    {
        return Err(
            "Input sources can be added only when their installed IBus engine is reported by this session and they are not already configured."
                .to_string(),
        );
    }

    let mut updated = current.to_vec();
    updated.push(requested);
    normalize_input_sources(updated)
}

fn normalize_input_sources(
    sources: Vec<InputSourceEntry>,
) -> Result<Vec<InputSourceEntry>, String> {
    if sources.is_empty() {
        return Err("At least one input source must remain configured.".to_string());
    }
    if sources.len() > 12 {
        return Err("Input sources are limited to 12 entries.".to_string());
    }

    let mut normalized = Vec::with_capacity(sources.len());
    for source in sources {
        let kind = source.kind.trim().to_ascii_lowercase();
        let id = source.id.trim().to_string();
        if kind != "xkb" && kind != "ibus" {
            return Err(format!(
                "Input source kind '{kind}' is not supported by Goblins OS."
            ));
        }
        if !input_source_id_is_safe(&id) {
            return Err(
                "Input source ids must be 1-80 ASCII letters, numbers, '.', '-', '_', '+', ':', or '@'."
                    .to_string(),
            );
        }
        let entry = InputSourceEntry { kind, id };
        if normalized.iter().any(|candidate| candidate == &entry) {
            return Err("Input sources cannot contain duplicates.".to_string());
        }
        normalized.push(entry);
    }

    Ok(normalized)
}

fn input_source_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+' | b':' | b'@')
        })
}

fn encode_input_sources(sources: &[InputSourceEntry]) -> String {
    let entries = sources
        .iter()
        .map(|source| {
            format!(
                "('{}', '{}')",
                escape_gvariant_string(&source.kind),
                escape_gvariant_string(&source.id)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

fn escape_gvariant_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
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

fn setting_bool(schema: &SchemaSnapshot, schema_name: &str, key: &str) -> Option<bool> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", schema_name, key])
        .ok()
        .and_then(|value| parse_gsettings_bool(&value))
}

fn setting_u32(schema: &SchemaSnapshot, schema_name: &str, key: &str) -> Option<u32> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", schema_name, key])
        .ok()
        .and_then(|value| parse_gsettings_u32(&value))
}

fn setting_f64(schema: &SchemaSnapshot, schema_name: &str, key: &str) -> Option<f64> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", schema_name, key])
        .ok()
        .and_then(|value| parse_gsettings_f64(&value))
}

fn parse_gsettings_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_gsettings_u32(value: &str) -> Option<u32> {
    value
        .split_whitespace()
        .rev()
        .find_map(|token| token.trim_matches('\'').parse::<u32>().ok())
}

fn parse_gsettings_f64(value: &str) -> Option<f64> {
    value.split_whitespace().rev().find_map(|token| {
        token
            .trim_matches('\'')
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    })
}

fn parse_preference_value(
    spec: InputTargetSpec,
    value: &Value,
) -> Result<InputPreferenceValue, String> {
    match spec.kind {
        InputValueKind::Bool => value
            .as_bool()
            .map(InputPreferenceValue::Bool)
            .ok_or_else(|| {
                format!(
                    "{} expects a true or false value from Settings.",
                    spec.label
                )
            }),
        InputValueKind::U32(normalize) => json_u32(value)
            .map(normalize)
            .map(InputPreferenceValue::U32)
            .ok_or_else(|| format!("{} expects a non-negative whole number.", spec.label)),
        InputValueKind::F64(normalize) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(normalize)
            .map(InputPreferenceValue::F64)
            .ok_or_else(|| format!("{} expects a finite number.", spec.label)),
    }
}

fn json_u32(value: &Value) -> Option<u32> {
    if let Some(value) = value.as_u64() {
        return u32::try_from(value).ok();
    }
    if let Some(value) = value.as_i64() {
        return u32::try_from(value).ok();
    }
    value.as_f64().and_then(|value| {
        value
            .is_finite()
            .then_some(value.round())
            .filter(|rounded| (*rounded - value).abs() < f64::EPSILON)
            .and_then(|rounded| u32::try_from(rounded as i64).ok())
    })
}

fn encode_preference_value(value: &InputPreferenceValue) -> String {
    match value {
        InputPreferenceValue::Bool(value) => value.to_string(),
        InputPreferenceValue::U32(value) => value.to_string(),
        InputPreferenceValue::F64(value) => format!("{value:.2}"),
    }
}

fn input_target_spec(target: InputPreferenceTarget) -> InputTargetSpec {
    match target {
        InputPreferenceTarget::KeyboardRepeat => InputTargetSpec {
            target: "keyboard-repeat",
            schema: KEYBOARD_SCHEMA,
            key: "repeat",
            label: "Key repeat",
            kind: InputValueKind::Bool,
        },
        InputPreferenceTarget::KeyboardDelayMs => InputTargetSpec {
            target: "keyboard-delay-ms",
            schema: KEYBOARD_SCHEMA,
            key: "delay",
            label: "Repeat delay",
            kind: InputValueKind::U32(normalized_keyboard_delay),
        },
        InputPreferenceTarget::KeyboardRepeatIntervalMs => InputTargetSpec {
            target: "keyboard-repeat-interval-ms",
            schema: KEYBOARD_SCHEMA,
            key: "repeat-interval",
            label: "Repeat interval",
            kind: InputValueKind::U32(normalized_keyboard_repeat_interval),
        },
        InputPreferenceTarget::KeyboardRememberNumlockState => InputTargetSpec {
            target: "keyboard-remember-numlock-state",
            schema: KEYBOARD_SCHEMA,
            key: "remember-numlock-state",
            label: "Remember Num Lock",
            kind: InputValueKind::Bool,
        },
        InputPreferenceTarget::MouseSpeed => InputTargetSpec {
            target: "mouse-speed",
            schema: MOUSE_SCHEMA,
            key: "speed",
            label: "Mouse tracking speed",
            kind: InputValueKind::F64(normalized_unit_speed),
        },
        InputPreferenceTarget::MouseNaturalScroll => InputTargetSpec {
            target: "mouse-natural-scroll",
            schema: MOUSE_SCHEMA,
            key: "natural-scroll",
            label: "Mouse natural scrolling",
            kind: InputValueKind::Bool,
        },
        InputPreferenceTarget::MouseLeftHanded => InputTargetSpec {
            target: "mouse-left-handed",
            schema: MOUSE_SCHEMA,
            key: "left-handed",
            label: "Primary mouse button",
            kind: InputValueKind::Bool,
        },
        InputPreferenceTarget::MouseMiddleClickEmulation => InputTargetSpec {
            target: "mouse-middle-click-emulation",
            schema: MOUSE_SCHEMA,
            key: "middle-click-emulation",
            label: "Middle-click emulation",
            kind: InputValueKind::Bool,
        },
        InputPreferenceTarget::TouchpadSpeed => InputTargetSpec {
            target: "touchpad-speed",
            schema: TOUCHPAD_SCHEMA,
            key: "speed",
            label: "Trackpad tracking speed",
            kind: InputValueKind::F64(normalized_unit_speed),
        },
        InputPreferenceTarget::TouchpadTapToClick => InputTargetSpec {
            target: "touchpad-tap-to-click",
            schema: TOUCHPAD_SCHEMA,
            key: "tap-to-click",
            label: "Tap to click",
            kind: InputValueKind::Bool,
        },
        InputPreferenceTarget::TouchpadNaturalScroll => InputTargetSpec {
            target: "touchpad-natural-scroll",
            schema: TOUCHPAD_SCHEMA,
            key: "natural-scroll",
            label: "Trackpad natural scrolling",
            kind: InputValueKind::Bool,
        },
        InputPreferenceTarget::TouchpadTwoFingerScrolling => InputTargetSpec {
            target: "touchpad-two-finger-scrolling",
            schema: TOUCHPAD_SCHEMA,
            key: "two-finger-scrolling-enabled",
            label: "Two-finger scrolling",
            kind: InputValueKind::Bool,
        },
        InputPreferenceTarget::TouchpadDisableWhileTyping => InputTargetSpec {
            target: "touchpad-disable-while-typing",
            schema: TOUCHPAD_SCHEMA,
            key: "disable-while-typing",
            label: "Ignore trackpad while typing",
            kind: InputValueKind::Bool,
        },
    }
}

fn input_preference_success_detail(spec: InputTargetSpec, value: &InputPreferenceValue) -> String {
    let value = match value {
        InputPreferenceValue::Bool(true) => "on".to_string(),
        InputPreferenceValue::Bool(false) => "off".to_string(),
        InputPreferenceValue::U32(value) => format!("{value} ms"),
        InputPreferenceValue::F64(value) => pointer_speed_label(*value).to_string(),
    };
    format!("{} is now {value}.", spec.label)
}

fn schema_detail(
    gsettings_available: bool,
    schema_available: bool,
    label: &str,
    _schema: &str,
) -> String {
    if !gsettings_available {
        return format!(
            "{label} preferences are not ready because desktop preferences are not ready in this session."
        );
    }
    if !schema_available {
        return format!(
            "{label} preferences are not ready because the required preference is not reported."
        );
    }
    format!("{label} preferences are ready.")
}

fn input_status_detail(gsettings_available: bool) -> String {
    if gsettings_available {
        "Keyboard, mouse, and trackpad preferences are ready for this desktop.".to_string()
    } else {
        "Keyboard, mouse, and trackpad preferences are not ready in this session.".to_string()
    }
}

fn normalized_keyboard_delay(delay: u32) -> u32 {
    round_u32_to_step(delay.clamp(150, 1000), 25)
}

fn normalized_keyboard_repeat_interval(interval: u32) -> u32 {
    round_u32_to_step(interval.clamp(15, 120), 5)
}

fn normalized_unit_speed(speed: f64) -> f64 {
    if !speed.is_finite() {
        return 0.0;
    }
    (speed.clamp(-1.0, 1.0) * 20.0).round() / 20.0
}

fn pointer_speed_label(speed: f64) -> &'static str {
    let speed = normalized_unit_speed(speed);
    if speed < -0.05 {
        "slower"
    } else if speed > 0.05 {
        "faster"
    } else {
        "normal"
    }
}

fn round_u32_to_step(value: u32, step: u32) -> u32 {
    ((value + step / 2) / step) * step
}

fn gsettings(args: &[&str]) -> Result<String, GSettingsError> {
    match Command::new("gsettings").args(args).output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(GSettingsError::Failed(gsettings_error_detail(
            &String::from_utf8_lossy(&output.stderr),
            &String::from_utf8_lossy(&output.stdout),
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(GSettingsError::Missing),
        Err(_) => Err(GSettingsError::Missing),
    }
}

fn gsettings_error_detail(stderr: &str, stdout: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }
    stdout.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        addable_input_source_choices, cjk_engine_package_statuses_with, encode_input_sources,
        encode_preference_value, input_engine_packages_detail, input_sources_with_added_choice,
        input_target_spec, next_input_source_index, normalize_input_sources, parse_gsettings_bool,
        parse_gsettings_f64, parse_gsettings_u32, parse_ibus_list_engine, parse_preference_value,
        IbusEngineProbe, InputPreferenceTarget, InputPreferenceValue, InputSourceEntry,
    };
    use serde_json::json;

    #[test]
    fn parses_gsettings_scalar_values() {
        assert_eq!(parse_gsettings_bool("true\n"), Some(true));
        assert_eq!(parse_gsettings_bool("false"), Some(false));
        assert_eq!(parse_gsettings_bool("'true'"), None);
        assert_eq!(parse_gsettings_u32("uint32 500"), Some(500));
        assert_eq!(parse_gsettings_u32("25"), Some(25));
        assert_eq!(parse_gsettings_f64("-0.25"), Some(-0.25));
        assert_eq!(parse_gsettings_f64("0.300000"), Some(0.3));
    }

    #[test]
    fn preference_values_are_type_checked_and_normalized() {
        let repeat = input_target_spec(InputPreferenceTarget::KeyboardRepeat);
        assert!(matches!(
            parse_preference_value(repeat, &json!(true)),
            Ok(InputPreferenceValue::Bool(true))
        ));
        assert!(parse_preference_value(repeat, &json!("true")).is_err());

        let delay = input_target_spec(InputPreferenceTarget::KeyboardDelayMs);
        assert!(matches!(
            parse_preference_value(delay, &json!(163)),
            Ok(InputPreferenceValue::U32(175))
        ));
        assert!(parse_preference_value(delay, &json!(-1)).is_err());

        let speed = input_target_spec(InputPreferenceTarget::MouseSpeed);
        assert!(matches!(
            parse_preference_value(speed, &json!(2.5)),
            Ok(InputPreferenceValue::F64(1.0))
        ));
    }

    #[test]
    fn parses_input_sources_a_ss_gvariant() {
        use super::parse_input_sources;
        let parsed = parse_input_sources("[('xkb', 'us'), ('ibus', 'libpinyin')]");
        assert_eq!(
            parsed,
            vec![
                InputSourceEntry {
                    kind: "xkb".into(),
                    id: "us".into()
                },
                InputSourceEntry {
                    kind: "ibus".into(),
                    id: "libpinyin".into()
                },
            ]
        );
        // Empty / typed-empty values yield no sources, never a panic.
        assert!(parse_input_sources("@a(ss) []").is_empty());
        assert!(parse_input_sources("[]").is_empty());
    }

    #[test]
    fn input_preference_values_encode_for_gsettings() {
        assert_eq!(
            encode_preference_value(&InputPreferenceValue::Bool(false)),
            "false"
        );
        assert_eq!(
            encode_preference_value(&InputPreferenceValue::U32(500)),
            "500"
        );
        assert_eq!(
            encode_preference_value(&InputPreferenceValue::F64(-0.25)),
            "-0.25"
        );
    }

    #[test]
    fn normalizes_input_sources_for_gsettings_writes() {
        let sources = normalize_input_sources(vec![
            InputSourceEntry {
                kind: " XKB ".into(),
                id: " us ".into(),
            },
            InputSourceEntry {
                kind: "ibus".into(),
                id: "table:wubi".into(),
            },
        ])
        .expect("valid sources");
        assert_eq!(
            sources,
            vec![
                InputSourceEntry {
                    kind: "xkb".into(),
                    id: "us".into()
                },
                InputSourceEntry {
                    kind: "ibus".into(),
                    id: "table:wubi".into()
                },
            ]
        );
        assert!(normalize_input_sources(Vec::new()).is_err());
        assert!(normalize_input_sources(vec![InputSourceEntry {
            kind: "xkb".into(),
            id: "us;rm".into(),
        }])
        .is_err());
        assert!(normalize_input_sources(vec![
            InputSourceEntry {
                kind: "xkb".into(),
                id: "us".into(),
            },
            InputSourceEntry {
                kind: "xkb".into(),
                id: "us".into(),
            },
        ])
        .is_err());
    }

    #[test]
    fn encodes_input_sources_as_a_ss_gvariant() {
        let sources = vec![
            InputSourceEntry {
                kind: "xkb".into(),
                id: "us".into(),
            },
            InputSourceEntry {
                kind: "ibus".into(),
                id: "libpinyin".into(),
            },
        ];
        assert_eq!(
            encode_input_sources(&sources),
            "[('xkb', 'us'), ('ibus', 'libpinyin')]"
        );
    }

    #[test]
    fn cjk_engine_package_registry_names_fedora_packages_and_ibus_ids() {
        let engines = cjk_engine_package_statuses_with(|_| true);
        let pairs = engines
            .iter()
            .map(|engine| (engine.package, engine.kind, engine.id))
            .collect::<Vec<_>>();

        assert_eq!(
            pairs,
            vec![
                ("ibus-libpinyin", "ibus", "libpinyin"),
                ("ibus-anthy", "ibus", "anthy"),
                ("ibus-hangul", "ibus", "hangul"),
            ]
        );
        assert!(engines.iter().all(|engine| engine.installed));
        assert!(engines.iter().all(|engine| engine
            .component_xml
            .starts_with("/usr/share/ibus/component/")));
        assert!(engines.iter().all(|engine| engine
            .engine_binary
            .starts_with("/usr/libexec/ibus-engine-")));
    }

    #[test]
    fn cjk_engine_package_readiness_requires_component_and_binary() {
        let engines = cjk_engine_package_statuses_with(|path| {
            path.ends_with("libpinyin.xml") || path.ends_with("ibus-engine-hangul")
        });

        assert_eq!(engines.len(), 3);
        assert!(!engines[0].installed);
        assert!(!engines[1].installed);
        assert!(!engines[2].installed);

        let all_ready = cjk_engine_package_statuses_with(|path| {
            path.ends_with(".xml") || path.contains("/ibus-engine-")
        });
        assert!(all_ready.iter().all(|engine| engine.installed));
        assert!(input_engine_packages_detail(3, 3).contains("are installed"));
        assert!(input_engine_packages_detail(2, 3).contains("not fully installed"));
    }

    #[test]
    fn parses_ibus_engine_list_without_guessing_languages() {
        let engines = parse_ibus_list_engine(
            "
            language: Chinese
              libpinyin - Intelligent Pinyin
              table:wubi - Wubi
            language: Korean
              hangul - Korean
            ",
        );

        assert_eq!(engines, vec!["libpinyin", "table:wubi", "hangul"]);
        assert!(parse_ibus_list_engine("language: English\nengines:\n").is_empty());
    }

    #[test]
    fn addable_input_sources_require_installed_runtime_and_not_configured() {
        let packages = cjk_engine_package_statuses_with(|path| {
            path.ends_with(".xml") || path.contains("/ibus-engine-")
        });
        let configured = vec![InputSourceEntry {
            kind: "ibus".into(),
            id: "libpinyin".into(),
        }];
        let probe = IbusEngineProbe {
            available: true,
            engine_ids: vec!["libpinyin".into(), "hangul".into()],
            detail: "IBus reported engines.".into(),
        };

        let (choices, detail) = addable_input_source_choices(&configured, &packages, &probe);
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].id, "hangul");
        assert!(choices[0].detail.contains("listed by IBus"));
        assert!(detail.contains("1 installed CJK input method can be added"));

        let updated = input_sources_with_added_choice(
            &configured,
            InputSourceEntry {
                kind: "ibus".into(),
                id: "hangul".into(),
            },
            &packages,
            &probe,
        )
        .expect("hangul is addable");
        assert_eq!(
            updated,
            vec![
                InputSourceEntry {
                    kind: "ibus".into(),
                    id: "libpinyin".into(),
                },
                InputSourceEntry {
                    kind: "ibus".into(),
                    id: "hangul".into(),
                },
            ]
        );

        assert!(input_sources_with_added_choice(
            &configured,
            InputSourceEntry {
                kind: "ibus".into(),
                id: "anthy".into(),
            },
            &packages,
            &probe,
        )
        .is_err());

        let missing_probe = IbusEngineProbe {
            available: false,
            engine_ids: Vec::new(),
            detail: "IBus is not installed.".into(),
        };
        let (choices, detail) =
            addable_input_source_choices(&configured, &packages, &missing_probe);
        assert!(choices.is_empty());
        assert_eq!(detail, "IBus is not installed.");
    }

    #[test]
    fn super_space_switching_requires_multiple_sources_and_known_current() {
        let one_source = vec![InputSourceEntry {
            kind: "xkb".into(),
            id: "us".into(),
        }];
        assert_eq!(next_input_source_index(&one_source, 0).unwrap(), None);

        let sources = vec![
            InputSourceEntry {
                kind: "xkb".into(),
                id: "us".into(),
            },
            InputSourceEntry {
                kind: "ibus".into(),
                id: "libpinyin".into(),
            },
            InputSourceEntry {
                kind: "ibus".into(),
                id: "hangul".into(),
            },
        ];
        assert_eq!(next_input_source_index(&sources, 0).unwrap(), Some(1));
        assert_eq!(next_input_source_index(&sources, 2).unwrap(), Some(0));
        assert!(next_input_source_index(&sources, 3).is_err());
    }
}
