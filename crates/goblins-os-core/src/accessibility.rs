//! Accessibility and display-comfort preferences for Settings.
//!
//! Goblins OS keeps these desktop preferences behind an allowlisted settings
//! bridge so the Settings GUI cannot write arbitrary schemas or silently report
//! success for unavailable accessibility controls.

use std::process::Command;

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
const A11Y_APPS_SCHEMA: &str = "org.gnome.desktop.a11y.applications";
const COLOR_SCHEMA: &str = "org.gnome.settings-daemon.plugins.color";

#[derive(Serialize)]
pub struct AccessibilityStatus {
    source: &'static str,
    gsettings_available: bool,
    interface: InterfaceAccessibilityStatus,
    assistive: AssistiveTechnologyStatus,
    display_comfort: DisplayComfortStatus,
    detail: String,
}

#[derive(Serialize)]
pub struct InterfaceAccessibilityStatus {
    schema_available: bool,
    reduce_motion: Option<bool>,
    text_scale: Option<f64>,
    detail: String,
}

#[derive(Serialize)]
pub struct AssistiveTechnologyStatus {
    schema_available: bool,
    screen_reader: Option<bool>,
    screen_keyboard: Option<bool>,
    magnifier: Option<bool>,
    detail: String,
}

#[derive(Serialize)]
pub struct DisplayComfortStatus {
    schema_available: bool,
    night_light_enabled: Option<bool>,
    schedule_automatic: Option<bool>,
    temperature: Option<u32>,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetAccessibilityPreferenceRequest {
    target: AccessibilityPreferenceTarget,
    value: Value,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AccessibilityPreferenceTarget {
    ReduceMotion,
    TextScale,
    ScreenReader,
    ScreenKeyboard,
    Magnifier,
    NightLight,
    NightLightAutomaticSchedule,
    NightLightTemperature,
}

#[derive(Serialize)]
pub struct AccessibilityPreferenceOutcome {
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
enum AccessibilityValueKind {
    Bool,
    InvertedBool,
    U32(fn(u32) -> u32),
    F64(fn(f64) -> f64),
}

#[derive(Clone, Copy)]
struct AccessibilityTargetSpec {
    target: &'static str,
    schema: &'static str,
    key: &'static str,
    label: &'static str,
    kind: AccessibilityValueKind,
}

enum AccessibilityPreferenceValue {
    Bool(bool),
    U32(u32),
    F64(f64),
}

pub async fn accessibility_status() -> Json<AccessibilityStatus> {
    Json(build_accessibility_status())
}

pub async fn set_accessibility_preference(
    Json(request): Json<SetAccessibilityPreferenceRequest>,
) -> (StatusCode, Json<AccessibilityPreferenceOutcome>) {
    set_accessibility_preference_outcome(request)
}

pub(crate) fn apply_ai_reduce_motion(value: bool) -> (StatusCode, String) {
    let request = SetAccessibilityPreferenceRequest {
        target: AccessibilityPreferenceTarget::ReduceMotion,
        value: serde_json::json!(value),
    };
    let (status, Json(outcome)) = set_accessibility_preference_outcome(request);
    (status, outcome.text)
}

fn build_accessibility_status() -> AccessibilityStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let interface_schema = schema_snapshot(gsettings_available, INTERFACE_SCHEMA);
    let a11y_schema = schema_snapshot(gsettings_available, A11Y_APPS_SCHEMA);
    let color_schema = schema_snapshot(gsettings_available, COLOR_SCHEMA);

    AccessibilityStatus {
        source: "goblins-os-core",
        gsettings_available,
        interface: InterfaceAccessibilityStatus {
            schema_available: interface_schema.available,
            reduce_motion: setting_bool(&interface_schema, INTERFACE_SCHEMA, "enable-animations")
                .map(|enabled| !enabled),
            text_scale: setting_f64(&interface_schema, INTERFACE_SCHEMA, "text-scaling-factor")
                .map(normalized_text_scale),
            detail: schema_detail(
                gsettings_available,
                interface_schema.available,
                "Interface accessibility",
                INTERFACE_SCHEMA,
            ),
        },
        assistive: AssistiveTechnologyStatus {
            schema_available: a11y_schema.available,
            screen_reader: setting_bool(&a11y_schema, A11Y_APPS_SCHEMA, "screen-reader-enabled"),
            screen_keyboard: setting_bool(
                &a11y_schema,
                A11Y_APPS_SCHEMA,
                "screen-keyboard-enabled",
            ),
            magnifier: setting_bool(&a11y_schema, A11Y_APPS_SCHEMA, "screen-magnifier-enabled"),
            detail: schema_detail(
                gsettings_available,
                a11y_schema.available,
                "Assistive technologies",
                A11Y_APPS_SCHEMA,
            ),
        },
        display_comfort: DisplayComfortStatus {
            schema_available: color_schema.available,
            night_light_enabled: setting_bool(&color_schema, COLOR_SCHEMA, "night-light-enabled"),
            schedule_automatic: setting_bool(
                &color_schema,
                COLOR_SCHEMA,
                "night-light-schedule-automatic",
            ),
            temperature: setting_u32(&color_schema, COLOR_SCHEMA, "night-light-temperature")
                .map(normalized_night_light_temperature),
            detail: schema_detail(
                gsettings_available,
                color_schema.available,
                "Display comfort",
                COLOR_SCHEMA,
            ),
        },
        detail: accessibility_status_detail(gsettings_available),
    }
}

fn set_accessibility_preference_outcome(
    request: SetAccessibilityPreferenceRequest,
) -> (StatusCode, Json<AccessibilityPreferenceOutcome>) {
    let spec = accessibility_target_spec(request.target);
    let value = match parse_preference_value(spec, &request.value) {
        Ok(value) => value,
        Err(text) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AccessibilityPreferenceOutcome {
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
            Json(AccessibilityPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so accessibility preferences cannot be changed in this session.".to_string(),
            }),
        );
    }

    let schema = schema_snapshot(true, spec.schema);
    if !schema.available || !schema.has_key(spec.key) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AccessibilityPreferenceOutcome {
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
            Json(AccessibilityPreferenceOutcome {
                ok: true,
                target: spec.target,
                text: accessibility_preference_success_detail(spec, &value),
            }),
        ),
        Err(GSettingsError::Missing) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AccessibilityPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so accessibility preferences cannot be changed in this session.".to_string(),
            }),
        ),
        Err(GSettingsError::Failed(detail)) => (
            StatusCode::BAD_GATEWAY,
            Json(AccessibilityPreferenceOutcome {
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
    spec: AccessibilityTargetSpec,
    value: &Value,
) -> Result<AccessibilityPreferenceValue, String> {
    match spec.kind {
        AccessibilityValueKind::Bool => value
            .as_bool()
            .map(AccessibilityPreferenceValue::Bool)
            .ok_or_else(|| format!("{} expects a true or false value.", spec.label)),
        AccessibilityValueKind::InvertedBool => value
            .as_bool()
            .map(|value| AccessibilityPreferenceValue::Bool(!value))
            .ok_or_else(|| format!("{} expects a true or false value.", spec.label)),
        AccessibilityValueKind::U32(normalize) => json_u32(value)
            .map(normalize)
            .map(AccessibilityPreferenceValue::U32)
            .ok_or_else(|| format!("{} expects a non-negative whole number.", spec.label)),
        AccessibilityValueKind::F64(normalize) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(normalize)
            .map(AccessibilityPreferenceValue::F64)
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

fn encode_preference_value(value: &AccessibilityPreferenceValue) -> String {
    match value {
        AccessibilityPreferenceValue::Bool(value) => value.to_string(),
        AccessibilityPreferenceValue::U32(value) => value.to_string(),
        AccessibilityPreferenceValue::F64(value) => format!("{value:.2}"),
    }
}

fn accessibility_target_spec(target: AccessibilityPreferenceTarget) -> AccessibilityTargetSpec {
    match target {
        AccessibilityPreferenceTarget::ReduceMotion => AccessibilityTargetSpec {
            target: "reduce-motion",
            schema: INTERFACE_SCHEMA,
            key: "enable-animations",
            label: "Reduce motion",
            kind: AccessibilityValueKind::InvertedBool,
        },
        AccessibilityPreferenceTarget::TextScale => AccessibilityTargetSpec {
            target: "text-scale",
            schema: INTERFACE_SCHEMA,
            key: "text-scaling-factor",
            label: "Text size",
            kind: AccessibilityValueKind::F64(normalized_text_scale),
        },
        AccessibilityPreferenceTarget::ScreenReader => AccessibilityTargetSpec {
            target: "screen-reader",
            schema: A11Y_APPS_SCHEMA,
            key: "screen-reader-enabled",
            label: "Screen reader",
            kind: AccessibilityValueKind::Bool,
        },
        AccessibilityPreferenceTarget::ScreenKeyboard => AccessibilityTargetSpec {
            target: "screen-keyboard",
            schema: A11Y_APPS_SCHEMA,
            key: "screen-keyboard-enabled",
            label: "On-screen keyboard",
            kind: AccessibilityValueKind::Bool,
        },
        AccessibilityPreferenceTarget::Magnifier => AccessibilityTargetSpec {
            target: "magnifier",
            schema: A11Y_APPS_SCHEMA,
            key: "screen-magnifier-enabled",
            label: "Magnifier",
            kind: AccessibilityValueKind::Bool,
        },
        AccessibilityPreferenceTarget::NightLight => AccessibilityTargetSpec {
            target: "night-light",
            schema: COLOR_SCHEMA,
            key: "night-light-enabled",
            label: "Night Light",
            kind: AccessibilityValueKind::Bool,
        },
        AccessibilityPreferenceTarget::NightLightAutomaticSchedule => AccessibilityTargetSpec {
            target: "night-light-automatic-schedule",
            schema: COLOR_SCHEMA,
            key: "night-light-schedule-automatic",
            label: "Automatic schedule",
            kind: AccessibilityValueKind::Bool,
        },
        AccessibilityPreferenceTarget::NightLightTemperature => AccessibilityTargetSpec {
            target: "night-light-temperature",
            schema: COLOR_SCHEMA,
            key: "night-light-temperature",
            label: "Color temperature",
            kind: AccessibilityValueKind::U32(normalized_night_light_temperature),
        },
    }
}

fn accessibility_preference_success_detail(
    spec: AccessibilityTargetSpec,
    value: &AccessibilityPreferenceValue,
) -> String {
    match (spec.target, value) {
        ("reduce-motion", AccessibilityPreferenceValue::Bool(animations_enabled)) => {
            motion_preference_detail(!animations_enabled).to_string()
        }
        ("text-scale", AccessibilityPreferenceValue::F64(scale)) => {
            format!("Desktop text size is now {:.0}%.", scale * 100.0)
        }
        ("screen-reader", AccessibilityPreferenceValue::Bool(enabled)) => {
            screen_reader_detail(*enabled).to_string()
        }
        ("screen-keyboard", AccessibilityPreferenceValue::Bool(enabled)) => {
            screen_keyboard_detail(*enabled).to_string()
        }
        ("magnifier", AccessibilityPreferenceValue::Bool(enabled)) => {
            magnifier_detail(*enabled).to_string()
        }
        ("night-light", AccessibilityPreferenceValue::Bool(enabled)) => {
            night_light_detail(*enabled).to_string()
        }
        ("night-light-automatic-schedule", AccessibilityPreferenceValue::Bool(automatic)) => {
            night_light_schedule_detail(*automatic).to_string()
        }
        ("night-light-temperature", AccessibilityPreferenceValue::U32(temperature)) => {
            format!("Night Light color temperature is now {temperature} K.")
        }
        _ => format!("{} was saved.", spec.label),
    }
}

fn normalized_text_scale(scale: f64) -> f64 {
    if !scale.is_finite() {
        return 1.0;
    }
    ((scale.clamp(0.85, 1.35) * 20.0).round() / 20.0 * 100.0).round() / 100.0
}

fn normalized_night_light_temperature(temperature: u32) -> u32 {
    round_to_step(temperature.clamp(1000, 10000), 100)
}

fn round_to_step(value: u32, step: u32) -> u32 {
    ((value + (step / 2)) / step) * step
}

fn accessibility_status_detail(gsettings_available: bool) -> String {
    if gsettings_available {
        "Accessibility preferences are ready for this desktop.".to_string()
    } else {
        "Desktop preferences are not ready, so accessibility preferences are read-only in this session.".to_string()
    }
}

fn schema_detail(
    gsettings_available: bool,
    schema_available: bool,
    label: &str,
    _schema: &str,
) -> String {
    if !gsettings_available {
        return format!(
            "Desktop preferences are not ready, so {label} controls are read-only here."
        );
    }
    if !schema_available {
        return format!(
            "{label} is not ready because the required preference is not reported by this session."
        );
    }
    format!("{label} controls are ready.")
}

fn motion_preference_detail(reduce_motion: bool) -> &'static str {
    if reduce_motion {
        "Desktop animations are reduced. State changes stay direct and calm."
    } else {
        "Desktop animations are enabled for standard Goblins OS transitions."
    }
}

fn screen_reader_detail(enabled: bool) -> &'static str {
    if enabled {
        "Screen reader support is enabled for the desktop session."
    } else {
        "Screen reader support is off until you turn it on for this session."
    }
}

fn screen_keyboard_detail(enabled: bool) -> &'static str {
    if enabled {
        "The on-screen keyboard can appear for text entry when the desktop needs it."
    } else {
        "The on-screen keyboard stays hidden unless another accessibility tool enables it."
    }
}

fn magnifier_detail(enabled: bool) -> &'static str {
    if enabled {
        "Screen magnification is enabled for the desktop session."
    } else {
        "Screen magnification is off. Text size still follows the separate text-size setting."
    }
}

fn night_light_detail(enabled: bool) -> &'static str {
    if enabled {
        "Night Light is on. The display shifts warmer when the schedule says it should."
    } else {
        "Night Light is off. The display keeps its normal color temperature."
    }
}

fn night_light_schedule_detail(automatic: bool) -> &'static str {
    if automatic {
        "Uses the desktop location and time zone to schedule warmer color automatically."
    } else {
        "Uses the manual Night Light schedule stored by the desktop session."
    }
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
        accessibility_target_spec, encode_preference_value, normalized_night_light_temperature,
        normalized_text_scale, parse_gsettings_bool, parse_gsettings_f64, parse_gsettings_u32,
        parse_preference_value, AccessibilityPreferenceTarget, AccessibilityPreferenceValue,
    };
    use serde_json::json;

    #[test]
    fn gsettings_values_parse() {
        assert_eq!(parse_gsettings_bool("true\n"), Some(true));
        assert_eq!(parse_gsettings_bool("false"), Some(false));
        assert_eq!(parse_gsettings_bool("'false'"), None);
        assert_eq!(parse_gsettings_u32("uint32 3400"), Some(3400));
        assert_eq!(parse_gsettings_f64("1.25"), Some(1.25));
    }

    #[test]
    fn accessibility_values_are_type_checked_and_normalized() {
        let reduce_motion = accessibility_target_spec(AccessibilityPreferenceTarget::ReduceMotion);
        assert!(matches!(
            parse_preference_value(reduce_motion, &json!(true)),
            Ok(AccessibilityPreferenceValue::Bool(false))
        ));
        assert!(parse_preference_value(reduce_motion, &json!("true")).is_err());

        let text_scale = accessibility_target_spec(AccessibilityPreferenceTarget::TextScale);
        assert!(matches!(
            parse_preference_value(text_scale, &json!(1.234)),
            Ok(AccessibilityPreferenceValue::F64(1.25))
        ));

        let temperature =
            accessibility_target_spec(AccessibilityPreferenceTarget::NightLightTemperature);
        assert!(matches!(
            parse_preference_value(temperature, &json!(3449)),
            Ok(AccessibilityPreferenceValue::U32(3400))
        ));
        assert!(parse_preference_value(temperature, &json!(-1)).is_err());
    }

    #[test]
    fn accessibility_values_encode_for_gsettings() {
        assert_eq!(
            encode_preference_value(&AccessibilityPreferenceValue::Bool(true)),
            "true"
        );
        assert_eq!(
            encode_preference_value(&AccessibilityPreferenceValue::U32(3400)),
            "3400"
        );
        assert_eq!(
            encode_preference_value(&AccessibilityPreferenceValue::F64(1.25)),
            "1.25"
        );
    }

    #[test]
    fn bounds_are_stable() {
        assert_eq!(normalized_text_scale(0.1), 0.85);
        assert_eq!(normalized_text_scale(1.123), 1.1);
        assert_eq!(normalized_text_scale(f64::NAN), 1.0);
        assert_eq!(normalized_night_light_temperature(1), 1000);
        assert_eq!(normalized_night_light_temperature(3449), 3400);
        assert_eq!(normalized_night_light_temperature(20000), 10000);
    }
}
