//! Switch Control substrate (scanning input status, read-only).
//!
//! The macOS "Switch Control" altitude: operate the desktop hands-free with 1–3
//! switches via item/point scanning. GNOME has no scanner — this is an own surface.
//! The real-time scanning ENGINE (a gnome-shell extension that highlights, walks the
//! AT-SPI tree, and injects input) is the deliberate, highest-risk follow-up; this
//! module ships the status read over the `org.goblins.os.a11y.switch-control` schema
//! with the same value normalization the engine will trust, honest-gated when the
//! schema isn't installed. Nothing here injects input.

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bounded::{bounded_session_command_output, probe_timeout};

const SCHEMA: &str = "org.goblins.os.a11y.switch-control";

#[derive(Serialize)]
pub struct SwitchControlStatus {
    source: &'static str,
    schema_available: bool,
    enabled: bool,
    mode: &'static str,
    scanning: &'static str,
    auto_interval_ms: i64,
    dwell_ms: i64,
    debounce_ms: i64,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetSwitchControlPreferenceRequest {
    target: SwitchControlPreferenceTarget,
    value: Value,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SwitchControlPreferenceTarget {
    Enabled,
    Mode,
    Scanning,
    AutoIntervalMs,
    DwellMs,
    DebounceMs,
}

#[derive(Serialize)]
pub struct SwitchControlPreferenceOutcome {
    ok: bool,
    target: &'static str,
    text: String,
}

#[derive(Clone, Copy)]
enum SwitchControlValueKind {
    Bool,
    Mode,
    Scanning,
    AutoIntervalMs,
    DwellMs,
    DebounceMs,
}

#[derive(Clone, Copy)]
struct SwitchControlTargetSpec {
    target: &'static str,
    key: &'static str,
    label: &'static str,
    kind: SwitchControlValueKind,
}

enum SwitchControlPreferenceValue {
    Bool(bool),
    Text(&'static str),
    Int(i64),
}

pub async fn switch_control_status() -> Json<SwitchControlStatus> {
    Json(build_status())
}

pub async fn set_switch_control_preference(
    Json(request): Json<SetSwitchControlPreferenceRequest>,
) -> (StatusCode, Json<SwitchControlPreferenceOutcome>) {
    set_switch_control_preference_outcome(request)
}

fn build_status() -> SwitchControlStatus {
    let available = schema_available(SCHEMA);
    if !available {
        return SwitchControlStatus {
            source: "goblins-os-core",
            schema_available: false,
            enabled: false,
            mode: "item",
            scanning: "auto",
            auto_interval_ms: 1200,
            dwell_ms: 800,
            debounce_ms: 60,
            detail: "Switch Control is unavailable here (its preferences schema is not installed)."
                .to_string(),
        };
    }

    let enabled = get_bool("enabled").unwrap_or(false);
    let mode = normalize_mode(get_string("mode").as_deref().unwrap_or("item"));
    let scanning = normalize_scanning(get_string("scanning").as_deref().unwrap_or("auto"));
    let auto_interval_ms = clamp_interval(get_int("auto-interval-ms").unwrap_or(1200));
    let dwell_ms = clamp_ms(get_int("dwell-ms").unwrap_or(800), 5000);
    let debounce_ms = clamp_ms(get_int("debounce-ms").unwrap_or(60), 2000);

    let detail = if enabled {
        format!(
            "Switch Control preferences are on for {mode} scan, {scanning}. The scanner engine must be active before highlighting or selection can run."
        )
    } else {
        "Switch Control is off.".to_string()
    };

    SwitchControlStatus {
        source: "goblins-os-core",
        schema_available: true,
        enabled,
        mode,
        scanning,
        auto_interval_ms,
        dwell_ms,
        debounce_ms,
        detail,
    }
}

fn set_switch_control_preference_outcome(
    request: SetSwitchControlPreferenceRequest,
) -> (StatusCode, Json<SwitchControlPreferenceOutcome>) {
    let spec = switch_control_target_spec(request.target);
    let value = match parse_switch_control_value(spec, &request.value) {
        Ok(value) => value,
        Err(text) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SwitchControlPreferenceOutcome {
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
            Json(SwitchControlPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so Switch Control cannot be changed in this session.".to_string(),
            }),
        );
    }

    if !schema_has_key(spec.key) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SwitchControlPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: format!(
                    "{} is not ready because the Switch Control preference is not installed.",
                    spec.label
                ),
            }),
        );
    }

    let encoded = encode_switch_control_value(&value);
    match gsettings(&["set", SCHEMA, spec.key, &encoded]) {
        Ok(_) => (
            StatusCode::OK,
            Json(SwitchControlPreferenceOutcome {
                ok: true,
                target: spec.target,
                text: switch_control_success_detail(spec, &value),
            }),
        ),
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(SwitchControlPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: format!("{} could not be saved by the desktop session.", spec.label),
            }),
        ),
    }
}

fn switch_control_target_spec(target: SwitchControlPreferenceTarget) -> SwitchControlTargetSpec {
    match target {
        SwitchControlPreferenceTarget::Enabled => SwitchControlTargetSpec {
            target: "enabled",
            key: "enabled",
            label: "Switch Control",
            kind: SwitchControlValueKind::Bool,
        },
        SwitchControlPreferenceTarget::Mode => SwitchControlTargetSpec {
            target: "mode",
            key: "mode",
            label: "Switch Control mode",
            kind: SwitchControlValueKind::Mode,
        },
        SwitchControlPreferenceTarget::Scanning => SwitchControlTargetSpec {
            target: "scanning",
            key: "scanning",
            label: "Switch Control scanning",
            kind: SwitchControlValueKind::Scanning,
        },
        SwitchControlPreferenceTarget::AutoIntervalMs => SwitchControlTargetSpec {
            target: "auto-interval-ms",
            key: "auto-interval-ms",
            label: "Switch Control auto interval",
            kind: SwitchControlValueKind::AutoIntervalMs,
        },
        SwitchControlPreferenceTarget::DwellMs => SwitchControlTargetSpec {
            target: "dwell-ms",
            key: "dwell-ms",
            label: "Switch Control dwell time",
            kind: SwitchControlValueKind::DwellMs,
        },
        SwitchControlPreferenceTarget::DebounceMs => SwitchControlTargetSpec {
            target: "debounce-ms",
            key: "debounce-ms",
            label: "Switch Control debounce",
            kind: SwitchControlValueKind::DebounceMs,
        },
    }
}

fn parse_switch_control_value(
    spec: SwitchControlTargetSpec,
    value: &Value,
) -> Result<SwitchControlPreferenceValue, String> {
    match spec.kind {
        SwitchControlValueKind::Bool => value
            .as_bool()
            .map(SwitchControlPreferenceValue::Bool)
            .ok_or_else(|| format!("{} expects true or false.", spec.label)),
        SwitchControlValueKind::Mode => {
            let Some(value) = value.as_str() else {
                return Err("Switch Control mode must be item or point.".to_string());
            };
            match value.trim() {
                "item" => Ok(SwitchControlPreferenceValue::Text("item")),
                "point" => Ok(SwitchControlPreferenceValue::Text("point")),
                _ => Err("Switch Control mode must be item or point.".to_string()),
            }
        }
        SwitchControlValueKind::Scanning => {
            let Some(value) = value.as_str() else {
                return Err("Switch Control scanning must be auto or step.".to_string());
            };
            match value.trim() {
                "auto" => Ok(SwitchControlPreferenceValue::Text("auto")),
                "step" => Ok(SwitchControlPreferenceValue::Text("step")),
                _ => Err("Switch Control scanning must be auto or step.".to_string()),
            }
        }
        SwitchControlValueKind::AutoIntervalMs => {
            let Some(value) = value.as_i64() else {
                return Err("Switch Control auto interval expects milliseconds.".to_string());
            };
            Ok(SwitchControlPreferenceValue::Int(clamp_interval(value)))
        }
        SwitchControlValueKind::DwellMs => {
            let Some(value) = value.as_i64() else {
                return Err("Switch Control dwell time expects milliseconds.".to_string());
            };
            Ok(SwitchControlPreferenceValue::Int(clamp_ms(value, 5000)))
        }
        SwitchControlValueKind::DebounceMs => {
            let Some(value) = value.as_i64() else {
                return Err("Switch Control debounce expects milliseconds.".to_string());
            };
            Ok(SwitchControlPreferenceValue::Int(clamp_ms(value, 2000)))
        }
    }
}

fn encode_switch_control_value(value: &SwitchControlPreferenceValue) -> String {
    match value {
        SwitchControlPreferenceValue::Bool(value) => value.to_string(),
        SwitchControlPreferenceValue::Text(value) => format!("'{value}'"),
        SwitchControlPreferenceValue::Int(value) => value.to_string(),
    }
}

fn switch_control_success_detail(
    spec: SwitchControlTargetSpec,
    value: &SwitchControlPreferenceValue,
) -> String {
    match (spec.target, value) {
        ("enabled", SwitchControlPreferenceValue::Bool(false)) => {
            "Switch Control is off. No scanning or selection input runs.".to_string()
        }
        ("enabled", SwitchControlPreferenceValue::Bool(true)) => {
            "Switch Control preferences are on. The scanner engine must be active before highlighting, selection, or input injection can run.".to_string()
        }
        ("mode", SwitchControlPreferenceValue::Text(value)) => {
            format!("Switch Control mode is set to {value}. The scanner engine must be active before highlighting or selection can run.")
        }
        ("scanning", SwitchControlPreferenceValue::Text(value)) => {
            format!("Switch Control scanning is set to {value}. The scanner engine must be active before highlighting or selection can run.")
        }
        (_, SwitchControlPreferenceValue::Int(value)) => {
            format!("{} is set to {value} ms. The scanner engine must be active before highlighting or selection can run.", spec.label)
        }
        _ => format!("{} saved.", spec.label),
    }
}

/// Normalize the scanning mode to a known value. Pure + unit-tested.
fn normalize_mode(value: &str) -> &'static str {
    match value.trim() {
        "point" => "point",
        _ => "item",
    }
}

/// Normalize the scanning style to a known value. Pure + unit-tested.
fn normalize_scanning(value: &str) -> &'static str {
    match value.trim() {
        "step" => "step",
        _ => "auto",
    }
}

/// Clamp the auto-scan interval to the supported 300–5000 ms range. Pure + tested.
fn clamp_interval(value: i64) -> i64 {
    value.clamp(300, 5000)
}

/// Clamp a non-negative millisecond value to `0..=max`. Pure + unit-tested.
fn clamp_ms(value: i64, max: i64) -> i64 {
    value.clamp(0, max)
}

fn schema_available(schema: &str) -> bool {
    gsettings(&["list-keys", schema])
        .map(|out| !out.trim().is_empty())
        .unwrap_or(false)
}

fn schema_has_key(key: &str) -> bool {
    gsettings(&["list-keys", SCHEMA])
        .map(|out| out.lines().any(|candidate| candidate.trim() == key))
        .unwrap_or(false)
}

fn get_bool(key: &str) -> Option<bool> {
    match gsettings(&["get", SCHEMA, key]).ok()?.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn get_int(key: &str) -> Option<i64> {
    gsettings(&["get", SCHEMA, key]).ok()?.trim().parse().ok()
}

fn get_string(key: &str) -> Option<String> {
    let raw = gsettings(&["get", SCHEMA, key]).ok()?;
    let trimmed = raw.trim();
    Some(
        trimmed
            .strip_prefix('\'')
            .and_then(|r| r.strip_suffix('\''))
            .unwrap_or(trimmed)
            .to_string(),
    )
}

fn gsettings(args: &[&str]) -> Result<String, ()> {
    let output =
        bounded_session_command_output("gsettings", args, probe_timeout()).map_err(|_| ())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_interval, clamp_ms, normalize_mode, normalize_scanning, parse_switch_control_value,
        switch_control_success_detail, switch_control_target_spec, SwitchControlPreferenceTarget,
        SwitchControlPreferenceValue,
    };

    #[test]
    fn normalizes_enums_to_known_values() {
        assert_eq!(normalize_mode("point"), "point");
        assert_eq!(normalize_mode("item"), "item");
        assert_eq!(normalize_mode("garbage"), "item"); // unknown → safe default
        assert_eq!(normalize_scanning("step"), "step");
        assert_eq!(normalize_scanning("auto"), "auto");
        assert_eq!(normalize_scanning(""), "auto");
    }

    #[test]
    fn clamps_timings_to_supported_ranges() {
        assert_eq!(clamp_interval(1200), 1200);
        assert_eq!(clamp_interval(10), 300); // below min
        assert_eq!(clamp_interval(99999), 5000); // above max
        assert_eq!(clamp_ms(-5, 2000), 0); // negative → 0
        assert_eq!(clamp_ms(50, 2000), 50);
        assert_eq!(clamp_ms(9000, 2000), 2000); // above max
    }

    #[test]
    fn preference_values_are_type_checked_and_clamped() {
        let enabled = switch_control_target_spec(SwitchControlPreferenceTarget::Enabled);
        assert!(matches!(
            parse_switch_control_value(enabled, &serde_json::json!(true)).unwrap(),
            SwitchControlPreferenceValue::Bool(true)
        ));
        assert!(parse_switch_control_value(enabled, &serde_json::json!("true")).is_err());

        let mode = switch_control_target_spec(SwitchControlPreferenceTarget::Mode);
        assert!(matches!(
            parse_switch_control_value(mode, &serde_json::json!("point")).unwrap(),
            SwitchControlPreferenceValue::Text("point")
        ));
        assert!(parse_switch_control_value(mode, &serde_json::json!("free")).is_err());

        let scanning = switch_control_target_spec(SwitchControlPreferenceTarget::Scanning);
        assert!(matches!(
            parse_switch_control_value(scanning, &serde_json::json!("step")).unwrap(),
            SwitchControlPreferenceValue::Text("step")
        ));
        assert!(parse_switch_control_value(scanning, &serde_json::json!("fast")).is_err());

        let interval = switch_control_target_spec(SwitchControlPreferenceTarget::AutoIntervalMs);
        assert!(matches!(
            parse_switch_control_value(interval, &serde_json::json!(10)).unwrap(),
            SwitchControlPreferenceValue::Int(300)
        ));

        let debounce = switch_control_target_spec(SwitchControlPreferenceTarget::DebounceMs);
        assert!(matches!(
            parse_switch_control_value(debounce, &serde_json::json!(9000)).unwrap(),
            SwitchControlPreferenceValue::Int(2000)
        ));
    }

    #[test]
    fn enabled_success_copy_does_not_claim_the_engine_is_running() {
        let enabled = switch_control_target_spec(SwitchControlPreferenceTarget::Enabled);
        let detail =
            switch_control_success_detail(enabled, &SwitchControlPreferenceValue::Bool(true));
        assert!(detail.contains("preferences are on"));
        assert!(detail.contains("scanner engine must be active"));
        assert!(detail.contains("before highlighting"));
    }
}
