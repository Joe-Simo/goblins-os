//! Keyboard and pointer preferences for Settings.
//!
//! Goblins OS keeps desktop input preferences behind an allowlisted settings
//! bridge so the Settings GUI cannot mutate arbitrary schemas or keys.

use std::process::Command;

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEYBOARD_SCHEMA: &str = "org.gnome.desktop.peripherals.keyboard";
const MOUSE_SCHEMA: &str = "org.gnome.desktop.peripherals.mouse";
const TOUCHPAD_SCHEMA: &str = "org.gnome.desktop.peripherals.touchpad";

#[derive(Serialize)]
pub struct InputStatus {
    source: &'static str,
    gsettings_available: bool,
    keyboard: KeyboardInputStatus,
    mouse: MouseInputStatus,
    touchpad: TouchpadInputStatus,
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

#[derive(Deserialize)]
pub struct SetInputPreferenceRequest {
    target: InputPreferenceTarget,
    value: Value,
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

pub async fn input_status() -> Json<InputStatus> {
    Json(build_input_status())
}

pub async fn set_input_preference(
    Json(request): Json<SetInputPreferenceRequest>,
) -> (StatusCode, Json<InputPreferenceOutcome>) {
    set_input_preference_outcome(request)
}

fn build_input_status() -> InputStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let keyboard_schema = schema_snapshot(gsettings_available, KEYBOARD_SCHEMA);
    let mouse_schema = schema_snapshot(gsettings_available, MOUSE_SCHEMA);
    let touchpad_schema = schema_snapshot(gsettings_available, TOUCHPAD_SCHEMA);

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
        detail: input_status_detail(gsettings_available),
    }
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
        encode_preference_value, input_target_spec, parse_gsettings_bool, parse_gsettings_f64,
        parse_gsettings_u32, parse_preference_value, InputPreferenceTarget, InputPreferenceValue,
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
}
