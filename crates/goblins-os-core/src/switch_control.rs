//! Switch Control substrate (scanning input status, read-only).
//!
//! The macOS "Switch Control" altitude: operate the desktop hands-free with 1–3
//! switches via item/point scanning. GNOME has no scanner — this is an own surface.
//! The real-time scanning ENGINE (a gnome-shell extension that highlights, walks the
//! AT-SPI tree, and injects input) is the deliberate, highest-risk follow-up; this
//! module ships the status read over the `org.goblins.os.a11y.switch-control` schema
//! with the same value normalization the engine will trust, honest-gated when the
//! schema isn't installed. Nothing here injects input.

use axum::Json;
use serde::Serialize;

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

pub async fn switch_control_status() -> Json<SwitchControlStatus> {
    Json(build_status())
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
        format!("Switch Control is on — {mode} scan, {scanning}.")
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
    use super::{clamp_interval, clamp_ms, normalize_mode, normalize_scanning};

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
}
