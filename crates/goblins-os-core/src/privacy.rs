//! Offline / private mode.
//!
//! Privacy-conscious users can run Goblins OS fully on this device with no
//! internet egress: the AI uses only the local GPT-OSS engine, hosted-OpenAI and
//! server relays are refused, and the OS does not reach the network for the
//! model. The choice is persisted in OS-owned state and is the authoritative
//! gate the resident relay consults before any network call. GPT-OSS is the
//! heart of the OS, so going fully offline costs the user nothing but reach.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_OFFLINE_PATH: &str = "/var/lib/goblins-os/policy/offline";
const DESKTOP_PRIVACY_SCHEMA: &str = "org.gnome.desktop.privacy";

#[derive(Deserialize)]
pub struct SetPrivacyRequest {
    offline: bool,
}

#[derive(Serialize)]
pub struct PrivacyStatus {
    source: &'static str,
    offline: bool,
    storage: String,
    detail: String,
    desktop: DesktopPrivacyStatus,
    facilities: Vec<crate::hardware::SystemFacility>,
}

#[derive(Serialize)]
pub struct DesktopPrivacyStatus {
    gsettings_available: bool,
    schema_available: bool,
    remember_recent_files: Option<bool>,
    remember_app_usage: Option<bool>,
    remove_old_trash_files: Option<bool>,
    remove_old_temp_files: Option<bool>,
    old_files_age_days: Option<u32>,
    disable_microphone: Option<bool>,
    disable_camera: Option<bool>,
    disable_sound_output: Option<bool>,
    usb_protection: Option<bool>,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetDesktopPrivacyRequest {
    target: DesktopPrivacyTarget,
    value: Value,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DesktopPrivacyTarget {
    RememberRecentFiles,
    RememberAppUsage,
    RemoveOldTrashFiles,
    RemoveOldTempFiles,
    OldFilesAgeDays,
    DisableMicrophone,
    DisableCamera,
    DisableSoundOutput,
    UsbProtection,
}

#[derive(Serialize)]
pub struct DesktopPrivacyOutcome {
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
enum DesktopPrivacyValueKind {
    Bool,
    U32(fn(u32) -> u32),
}

#[derive(Clone, Copy)]
struct DesktopPrivacySpec {
    target: &'static str,
    key: &'static str,
    label: &'static str,
    kind: DesktopPrivacyValueKind,
}

enum DesktopPrivacyValue {
    Bool(bool),
    U32(u32),
}

pub async fn privacy_status() -> Json<PrivacyStatus> {
    Json(build_status())
}

pub async fn set_privacy(Json(request): Json<SetPrivacyRequest>) -> Response {
    if write_flag_to(&offline_path(), request.offline).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "text": "The privacy setting could not be saved to OS-owned state.",
            })),
        )
            .into_response();
    }
    Json(build_status()).into_response()
}

pub async fn set_desktop_privacy(
    Json(request): Json<SetDesktopPrivacyRequest>,
) -> (StatusCode, Json<DesktopPrivacyOutcome>) {
    desktop_privacy_outcome(request)
}

/// True when the OS is in offline / private mode: no network egress for the AI.
/// A persisted choice (from onboarding or Settings) is authoritative; absent one,
/// an ops env override is honored; otherwise the OS is online-capable.
pub(crate) fn offline_enabled() -> bool {
    if let Some(flag) = read_flag_from(&offline_path()) {
        return flag;
    }
    match env::var("GOBLINS_OS_OFFLINE").ok().as_deref() {
        Some(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        None => false,
    }
}

fn build_status() -> PrivacyStatus {
    let offline = offline_enabled();
    PrivacyStatus {
        source: "goblins-os-core",
        offline,
        storage: offline_path().display().to_string(),
        detail: if offline {
            "Private mode is on. The AI runs only on this device with the local GPT-OSS engine; hosted OpenAI models and network downloads are refused.".to_string()
        } else {
            "Private mode is off. Goblins OS may reach the internet to download models and, if you choose hosted models, to call OpenAI.".to_string()
        },
        desktop: build_desktop_privacy_status(),
        facilities: crate::hardware::privacy_facility_checks(),
    }
}

fn build_desktop_privacy_status() -> DesktopPrivacyStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available);

    DesktopPrivacyStatus {
        gsettings_available,
        schema_available: schema.available,
        remember_recent_files: setting_bool(&schema, "remember-recent-files"),
        remember_app_usage: setting_bool(&schema, "remember-app-usage"),
        remove_old_trash_files: setting_bool(&schema, "remove-old-trash-files"),
        remove_old_temp_files: setting_bool(&schema, "remove-old-temp-files"),
        old_files_age_days: setting_u32(&schema, "old-files-age").map(normalized_old_files_age),
        disable_microphone: setting_bool(&schema, "disable-microphone"),
        disable_camera: setting_bool(&schema, "disable-camera"),
        disable_sound_output: setting_bool(&schema, "disable-sound-output"),
        usb_protection: setting_bool(&schema, "usb-protection"),
        detail: desktop_privacy_detail(gsettings_available, schema.available),
    }
}

fn desktop_privacy_outcome(
    request: SetDesktopPrivacyRequest,
) -> (StatusCode, Json<DesktopPrivacyOutcome>) {
    let spec = desktop_privacy_spec(request.target);
    let value = match parse_desktop_privacy_value(spec, &request.value) {
        Ok(value) => value,
        Err(text) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(DesktopPrivacyOutcome {
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
            Json(DesktopPrivacyOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so privacy preferences cannot be changed in this session.".to_string(),
            }),
        );
    }

    let schema = schema_snapshot(true);
    if !schema.available || !schema.has_key(spec.key) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(DesktopPrivacyOutcome {
                ok: false,
                target: spec.target,
                text: format!(
                    "{} is not ready because the required preference is not reported by this desktop session.",
                    spec.label
                ),
            }),
        );
    }

    let encoded = encode_desktop_privacy_value(&value);
    match gsettings(&["set", DESKTOP_PRIVACY_SCHEMA, spec.key, &encoded]) {
        Ok(_) => (
            StatusCode::OK,
            Json(DesktopPrivacyOutcome {
                ok: true,
                target: spec.target,
                text: desktop_privacy_success_detail(spec, &value),
            }),
        ),
        Err(GSettingsError::Missing) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(DesktopPrivacyOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so privacy preferences cannot be changed in this session.".to_string(),
            }),
        ),
        Err(GSettingsError::Failed(detail)) => (
            StatusCode::BAD_GATEWAY,
            Json(DesktopPrivacyOutcome {
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

fn offline_path() -> PathBuf {
    env::var("GOBLINS_OS_OFFLINE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_OFFLINE_PATH).to_path_buf())
}

/// Read the persisted offline flag. `None` means no explicit choice has been
/// recorded yet (the caller then falls back to the env override / default).
fn read_flag_from(path: &Path) -> Option<bool> {
    let value = fs::read_to_string(path).ok()?;
    match value.trim() {
        "on" | "1" | "true" => Some(true),
        "off" | "0" | "false" => Some(false),
        _ => None,
    }
}

fn write_flag_to(path: &Path, offline: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, if offline { "on" } else { "off" })?;
    fs::rename(tmp, path)
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

fn schema_snapshot(gsettings_available: bool) -> SchemaSnapshot {
    if !gsettings_available {
        return SchemaSnapshot::unavailable();
    }

    match gsettings(&["list-keys", DESKTOP_PRIVACY_SCHEMA]) {
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

fn setting_bool(schema: &SchemaSnapshot, key: &str) -> Option<bool> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", DESKTOP_PRIVACY_SCHEMA, key])
        .ok()
        .and_then(|value| parse_gsettings_bool(&value))
}

fn setting_u32(schema: &SchemaSnapshot, key: &str) -> Option<u32> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", DESKTOP_PRIVACY_SCHEMA, key])
        .ok()
        .and_then(|value| parse_gsettings_u32(&value))
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

fn parse_desktop_privacy_value(
    spec: DesktopPrivacySpec,
    value: &Value,
) -> Result<DesktopPrivacyValue, String> {
    match spec.kind {
        DesktopPrivacyValueKind::Bool => {
            value
                .as_bool()
                .map(DesktopPrivacyValue::Bool)
                .ok_or_else(|| {
                    format!(
                        "{} expects a true or false value from Settings.",
                        spec.label
                    )
                })
        }
        DesktopPrivacyValueKind::U32(normalize) => json_u32(value)
            .map(normalize)
            .map(DesktopPrivacyValue::U32)
            .ok_or_else(|| format!("{} expects a non-negative whole number.", spec.label)),
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

fn encode_desktop_privacy_value(value: &DesktopPrivacyValue) -> String {
    match value {
        DesktopPrivacyValue::Bool(value) => value.to_string(),
        DesktopPrivacyValue::U32(value) => value.to_string(),
    }
}

fn desktop_privacy_spec(target: DesktopPrivacyTarget) -> DesktopPrivacySpec {
    match target {
        DesktopPrivacyTarget::RememberRecentFiles => DesktopPrivacySpec {
            target: "remember-recent-files",
            key: "remember-recent-files",
            label: "Remember recent files",
            kind: DesktopPrivacyValueKind::Bool,
        },
        DesktopPrivacyTarget::RememberAppUsage => DesktopPrivacySpec {
            target: "remember-app-usage",
            key: "remember-app-usage",
            label: "Remember app usage",
            kind: DesktopPrivacyValueKind::Bool,
        },
        DesktopPrivacyTarget::RemoveOldTrashFiles => DesktopPrivacySpec {
            target: "remove-old-trash-files",
            key: "remove-old-trash-files",
            label: "Remove aged Trash items",
            kind: DesktopPrivacyValueKind::Bool,
        },
        DesktopPrivacyTarget::RemoveOldTempFiles => DesktopPrivacySpec {
            target: "remove-old-temp-files",
            key: "remove-old-temp-files",
            label: "Remove aged temporary files",
            kind: DesktopPrivacyValueKind::Bool,
        },
        DesktopPrivacyTarget::OldFilesAgeDays => DesktopPrivacySpec {
            target: "old-files-age-days",
            key: "old-files-age",
            label: "Cleanup age",
            kind: DesktopPrivacyValueKind::U32(normalized_old_files_age),
        },
        DesktopPrivacyTarget::DisableMicrophone => DesktopPrivacySpec {
            target: "disable-microphone",
            key: "disable-microphone",
            label: "Block microphone access",
            kind: DesktopPrivacyValueKind::Bool,
        },
        DesktopPrivacyTarget::DisableCamera => DesktopPrivacySpec {
            target: "disable-camera",
            key: "disable-camera",
            label: "Block camera access",
            kind: DesktopPrivacyValueKind::Bool,
        },
        DesktopPrivacyTarget::DisableSoundOutput => DesktopPrivacySpec {
            target: "disable-sound-output",
            key: "disable-sound-output",
            label: "Block sound output",
            kind: DesktopPrivacyValueKind::Bool,
        },
        DesktopPrivacyTarget::UsbProtection => DesktopPrivacySpec {
            target: "usb-protection",
            key: "usb-protection",
            label: "Protect new USB devices",
            kind: DesktopPrivacyValueKind::Bool,
        },
    }
}

fn desktop_privacy_success_detail(spec: DesktopPrivacySpec, value: &DesktopPrivacyValue) -> String {
    match (spec.target, value) {
        ("remember-recent-files", DesktopPrivacyValue::Bool(enabled)) => {
            recent_files_detail(*enabled).to_string()
        }
        ("remember-app-usage", DesktopPrivacyValue::Bool(enabled)) => {
            app_usage_detail(*enabled).to_string()
        }
        ("remove-old-trash-files", DesktopPrivacyValue::Bool(enabled)) => {
            cleanup_trash_detail(*enabled).to_string()
        }
        ("remove-old-temp-files", DesktopPrivacyValue::Bool(enabled)) => {
            cleanup_temp_detail(*enabled).to_string()
        }
        ("old-files-age-days", DesktopPrivacyValue::U32(days)) => {
            format!("Trash and temporary files become eligible for cleanup after {days} days.")
        }
        ("disable-microphone", DesktopPrivacyValue::Bool(blocked)) => {
            microphone_access_detail(*blocked).to_string()
        }
        ("disable-camera", DesktopPrivacyValue::Bool(blocked)) => {
            camera_access_detail(*blocked).to_string()
        }
        ("disable-sound-output", DesktopPrivacyValue::Bool(blocked)) => {
            sound_output_access_detail(*blocked).to_string()
        }
        ("usb-protection", DesktopPrivacyValue::Bool(enabled)) => {
            usb_protection_detail(*enabled).to_string()
        }
        _ => format!("{} was saved.", spec.label),
    }
}

fn desktop_privacy_detail(gsettings_available: bool, schema_available: bool) -> String {
    if !gsettings_available {
        return "Desktop preferences are not ready, so privacy controls are read-only in this session.".to_string();
    }
    if !schema_available {
        return "The standard privacy preferences are not supported in this session.".to_string();
    }
    "Privacy controls are ready for this desktop.".to_string()
}

fn normalized_old_files_age(days: u32) -> u32 {
    days.clamp(1, 365)
}

fn recent_files_detail(enabled: bool) -> &'static str {
    if enabled {
        "Applications can keep a recent-files list for faster reopening."
    } else {
        "Applications should not keep a recent-files list in this desktop session."
    }
}

fn app_usage_detail(enabled: bool) -> &'static str {
    if enabled {
        "The desktop can remember application usage for launchers and suggestions."
    } else {
        "Application usage should not be monitored or recorded by the desktop."
    }
}

fn cleanup_trash_detail(enabled: bool) -> &'static str {
    if enabled {
        "Trash items older than the cleanup age are removed automatically."
    } else {
        "Trash is kept until you empty it or another cleanup tool removes it."
    }
}

fn cleanup_temp_detail(enabled: bool) -> &'static str {
    if enabled {
        "Temporary files older than the cleanup age are removed automatically."
    } else {
        "Temporary files are not removed automatically by desktop privacy cleanup."
    }
}

fn microphone_access_detail(blocked: bool) -> &'static str {
    if blocked {
        "Applications should not use the microphone while this desktop privacy setting is on."
    } else {
        "Applications may request microphone access through the desktop session."
    }
}

fn camera_access_detail(blocked: bool) -> &'static str {
    if blocked {
        "Applications should not use the camera while this desktop privacy setting is on."
    } else {
        "Applications may request camera access through the desktop session."
    }
}

fn sound_output_access_detail(blocked: bool) -> &'static str {
    if blocked {
        "Applications should not produce sound while this desktop privacy setting is on."
    } else {
        "Applications may play sound through the desktop session."
    }
}

fn usb_protection_detail(enabled: bool) -> &'static str {
    if enabled {
        "New USB devices are protected when the desktop and USBGuard support the policy."
    } else {
        "The desktop USB protection preference is off."
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
        build_status, desktop_privacy_spec, encode_desktop_privacy_value,
        parse_desktop_privacy_value, parse_gsettings_bool, parse_gsettings_u32, read_flag_from,
        write_flag_to, DesktopPrivacyTarget, DesktopPrivacyValue,
    };
    use serde_json::json;
    use std::path::PathBuf;

    fn unique_tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    #[test]
    fn offline_flag_round_trips_and_defaults_to_unset() {
        let dir = unique_tmp("goblins-os-offline");
        let path = dir.join("offline");
        let _ = std::fs::remove_dir_all(&dir);

        // No file yet => no explicit choice (caller falls back to env/default).
        assert_eq!(read_flag_from(&path), None);

        write_flag_to(&path, true).expect("persist offline=on");
        assert_eq!(read_flag_from(&path), Some(true));

        write_flag_to(&path, false).expect("persist offline=off");
        assert_eq!(read_flag_from(&path), Some(false));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn desktop_privacy_gsettings_values_parse() {
        assert_eq!(parse_gsettings_bool("true\n"), Some(true));
        assert_eq!(parse_gsettings_bool("false"), Some(false));
        assert_eq!(parse_gsettings_bool("'false'"), None);
        assert_eq!(parse_gsettings_u32("uint32 30"), Some(30));
        assert_eq!(parse_gsettings_u32("7"), Some(7));
    }

    #[test]
    fn desktop_privacy_values_are_type_checked_and_normalized() {
        let remember_recent = desktop_privacy_spec(DesktopPrivacyTarget::RememberRecentFiles);
        assert!(matches!(
            parse_desktop_privacy_value(remember_recent, &json!(false)),
            Ok(DesktopPrivacyValue::Bool(false))
        ));
        assert!(parse_desktop_privacy_value(remember_recent, &json!("false")).is_err());

        let cleanup_age = desktop_privacy_spec(DesktopPrivacyTarget::OldFilesAgeDays);
        assert!(matches!(
            parse_desktop_privacy_value(cleanup_age, &json!(900)),
            Ok(DesktopPrivacyValue::U32(365))
        ));
        assert!(parse_desktop_privacy_value(cleanup_age, &json!(-1)).is_err());
    }

    #[test]
    fn desktop_privacy_values_encode_for_gsettings() {
        assert_eq!(
            encode_desktop_privacy_value(&DesktopPrivacyValue::Bool(true)),
            "true"
        );
        assert_eq!(
            encode_desktop_privacy_value(&DesktopPrivacyValue::U32(30)),
            "30"
        );
    }

    #[test]
    fn privacy_status_includes_permission_infrastructure() {
        let status = build_status();
        let ids = status
            .facilities
            .iter()
            .map(|facility| facility.id)
            .collect::<Vec<_>>();

        assert!(ids.contains(&"desktop-portals"));
        assert!(ids.contains(&"keyring"));
        assert!(ids.contains(&"policy"));
    }
}
