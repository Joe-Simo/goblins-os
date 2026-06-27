//! Sound Recognition substrate (category registry + honest capability status).
//!
//! The macOS "Sound Recognition" altitude: an opt-in accessibility listener that
//! identifies a fixed catalog of safety/attention sounds on-device. The real-time
//! listener is a session-user daemon so it can reach PipeWire; this module ships
//! the host-testable substrate first: sound ids, preference normalization, local
//! model/listener/capture gates, and the status route. It never fabricates a
//! listening state when the model, listener, capture path, or schema is absent.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SCHEMA: &str = "org.goblins.SoundRecognition";
const DEFAULT_MODEL_DIR: &str = "/var/lib/goblins-os/sound-recognition";
const MODEL_DIR_ENV: &str = "GOBLINS_OS_SOUND_RECOGNITION_MODEL_DIR";
const LISTENER_BIN_ENV: &str = "GOBLINS_OS_SOUND_RECOGNITION_LISTENER";
const CAPTURE_BIN_ENV: &str = "GOBLINS_OS_SOUND_RECOGNITION_CAPTURE_BIN";
const DEFAULT_LISTENER: &str = "/usr/libexec/goblins-os/goblins-os-sound-listener";
const DEFAULT_CAPTURE_BIN: &str = "arecord";
const RELIABILITY_DETAIL: &str = "This recognizes sounds approximately and on-device only. Do not rely on it in emergencies or high-risk situations.";

#[derive(Clone, Copy)]
struct SoundCategory {
    id: &'static str,
    name: &'static str,
    group: &'static str,
    description: &'static str,
    audioset_classes: &'static [&'static str],
}

const SOUND_CATEGORIES: &[SoundCategory] = &[
    SoundCategory {
        id: "smoke-fire-alarm",
        name: "Smoke or fire alarm",
        group: "Safety",
        description:
            "High-priority building alarms such as smoke, fire, or carbon monoxide alarms.",
        audioset_classes: &["Smoke detector, smoke alarm", "Fire alarm"],
    },
    SoundCategory {
        id: "siren",
        name: "Siren",
        group: "Safety",
        description: "Emergency vehicle or public warning sirens.",
        audioset_classes: &["Siren", "Emergency vehicle"],
    },
    SoundCategory {
        id: "doorbell",
        name: "Doorbell",
        group: "Home",
        description: "Doorbell chimes or buzzer-style entrance alerts.",
        audioset_classes: &["Doorbell"],
    },
    SoundCategory {
        id: "knock",
        name: "Knock",
        group: "Home",
        description: "Knocking on a door or hard surface.",
        audioset_classes: &["Knock"],
    },
    SoundCategory {
        id: "baby-crying",
        name: "Baby crying",
        group: "People",
        description: "Infant crying or distress vocalizations.",
        audioset_classes: &["Baby cry, infant cry"],
    },
    SoundCategory {
        id: "dog-bark",
        name: "Dog bark",
        group: "Animals",
        description: "Dog barking and similar alert barks.",
        audioset_classes: &["Bark"],
    },
    SoundCategory {
        id: "car-horn",
        name: "Car horn",
        group: "Outside",
        description: "Vehicle horn sounds.",
        audioset_classes: &["Vehicle horn, car horn, honking"],
    },
    SoundCategory {
        id: "appliance-beep",
        name: "Appliance beep",
        group: "Home",
        description: "Beeps from timers, microwaves, washers, and similar appliances.",
        audioset_classes: &["Beep, bleep", "Microwave oven", "Alarm clock"],
    },
    SoundCategory {
        id: "running-water",
        name: "Running water",
        group: "Home",
        description: "Sustained faucet, shower, or water-flow sounds.",
        audioset_classes: &["Water", "Liquid", "Pour", "Splash, splatter"],
    },
    SoundCategory {
        id: "shouting",
        name: "Shouting",
        group: "People",
        description: "Raised human voices that may need attention.",
        audioset_classes: &["Shout", "Yell"],
    },
];

#[derive(Serialize)]
pub struct SoundRecognitionStatus {
    source: &'static str,
    schema_available: bool,
    enabled: bool,
    available: bool,
    active: bool,
    offline_safe: bool,
    sensitivity: &'static str,
    min_confidence: f64,
    alert_sound: bool,
    alert_flash: bool,
    notify_in_lock_screen: bool,
    classifier_model: Capability,
    listener: Capability,
    capture: Capability,
    sounds: Vec<SoundCategoryStatus>,
    reliability_detail: &'static str,
    detail: String,
}

#[derive(Serialize)]
struct Capability {
    ready: bool,
    component: String,
    detail: String,
}

#[derive(Serialize)]
struct SoundCategoryStatus {
    id: &'static str,
    name: &'static str,
    group: &'static str,
    enabled: bool,
    description: &'static str,
    audioset_classes: &'static [&'static str],
}

#[derive(Deserialize)]
pub struct SetSoundRecognitionPreferenceRequest {
    target: SoundRecognitionPreferenceTarget,
    value: Value,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SoundRecognitionPreferenceTarget {
    Enabled,
    Sensitivity,
    MinConfidence,
    AlertSound,
    AlertFlash,
    NotifyInLockScreen,
}

#[derive(Deserialize)]
pub struct SetSoundToggleRequest {
    id: String,
    enabled: bool,
}

#[derive(Serialize)]
pub struct SoundRecognitionPreferenceOutcome {
    ok: bool,
    target: &'static str,
    text: String,
}

#[derive(Serialize)]
pub struct SoundRecognitionSoundToggleOutcome {
    ok: bool,
    id: String,
    enabled: bool,
    text: String,
}

#[derive(Clone, Copy)]
enum SoundRecognitionValueKind {
    Bool,
    Sensitivity,
    MinConfidence,
}

#[derive(Clone, Copy)]
struct SoundRecognitionTargetSpec {
    target: &'static str,
    key: &'static str,
    label: &'static str,
    kind: SoundRecognitionValueKind,
}

#[derive(Deserialize)]
struct ListenerCapabilityPayload {
    ready: bool,
    component: Option<String>,
    detail: Option<String>,
}

enum SoundRecognitionPreferenceValue {
    Bool(bool),
    Sensitivity(&'static str),
    MinConfidence(f64),
}

pub async fn sound_recognition_status() -> Json<SoundRecognitionStatus> {
    Json(build_status())
}

pub async fn set_sound_recognition_preference(
    Json(request): Json<SetSoundRecognitionPreferenceRequest>,
) -> (StatusCode, Json<SoundRecognitionPreferenceOutcome>) {
    set_sound_recognition_preference_outcome(request)
}

pub async fn set_sound_toggle(
    Json(request): Json<SetSoundToggleRequest>,
) -> (StatusCode, Json<SoundRecognitionSoundToggleOutcome>) {
    set_sound_toggle_outcome(request)
}

fn build_status() -> SoundRecognitionStatus {
    let schema_available = schema_available(SCHEMA);
    let enabled = schema_available && get_bool("enabled").unwrap_or(false);
    let enabled_sounds = if schema_available {
        normalize_enabled_sounds(read_sound_ids())
    } else {
        Vec::new()
    };
    let sensitivity = if schema_available {
        normalize_sensitivity(get_string("sensitivity").as_deref().unwrap_or("medium"))
    } else {
        "medium"
    };
    let min_confidence = if schema_available {
        clamp_min_confidence(get_double("min-confidence").unwrap_or(0.70))
    } else {
        0.70
    };
    let alert_sound = schema_available && get_bool("alert-sound").unwrap_or(false);
    let alert_flash = schema_available && get_bool("alert-flash").unwrap_or(false);
    let notify_in_lock_screen =
        schema_available && get_bool("notify-in-lock-screen").unwrap_or(false);

    let classifier_model = classifier_model_capability();
    let listener = listener_capability();
    let capture = capture_capability();
    let available = classifier_model.ready && listener.ready && capture.ready;
    let active = enabled && available && !enabled_sounds.is_empty();
    let sounds = SOUND_CATEGORIES
        .iter()
        .map(|category| SoundCategoryStatus {
            id: category.id,
            name: category.name,
            group: category.group,
            enabled: enabled_sounds.iter().any(|id| id == category.id),
            description: category.description,
            audioset_classes: category.audioset_classes,
        })
        .collect();

    SoundRecognitionStatus {
        source: "goblins-os-core",
        schema_available,
        enabled,
        available,
        active,
        offline_safe: true,
        sensitivity,
        min_confidence,
        alert_sound,
        alert_flash,
        notify_in_lock_screen,
        classifier_model,
        listener,
        capture,
        sounds,
        reliability_detail: RELIABILITY_DETAIL,
        detail: status_detail(
            schema_available,
            enabled,
            available,
            active,
            &enabled_sounds,
        ),
    }
}

fn set_sound_recognition_preference_outcome(
    request: SetSoundRecognitionPreferenceRequest,
) -> (StatusCode, Json<SoundRecognitionPreferenceOutcome>) {
    let spec = sound_recognition_target_spec(request.target);
    let value = match parse_sound_recognition_value(spec, &request.value) {
        Ok(value) => value,
        Err(text) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SoundRecognitionPreferenceOutcome {
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
            Json(SoundRecognitionPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so Sound Recognition cannot be changed in this session.".to_string(),
            }),
        );
    }

    if !schema_has_key(spec.key) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SoundRecognitionPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: format!(
                    "{} is not ready because the Sound Recognition preference is not installed.",
                    spec.label
                ),
            }),
        );
    }

    let encoded = encode_sound_recognition_value(&value);
    match gsettings(&["set", SCHEMA, spec.key, &encoded]) {
        Ok(_) => (
            StatusCode::OK,
            Json(SoundRecognitionPreferenceOutcome {
                ok: true,
                target: spec.target,
                text: sound_recognition_preference_success_detail(spec, &value),
            }),
        ),
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(SoundRecognitionPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: format!("{} could not be saved by the desktop session.", spec.label),
            }),
        ),
    }
}

fn set_sound_toggle_outcome(
    request: SetSoundToggleRequest,
) -> (StatusCode, Json<SoundRecognitionSoundToggleOutcome>) {
    let requested_id = request.id.trim();
    let Some(category) = sound_category_by_id(requested_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(SoundRecognitionSoundToggleOutcome {
                ok: false,
                id: requested_id.to_string(),
                enabled: request.enabled,
                text: "Unknown sound category. Sound Recognition only accepts its fixed on-device sound registry.".to_string(),
            }),
        );
    };

    if gsettings(&["list-schemas"]).is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SoundRecognitionSoundToggleOutcome {
                ok: false,
                id: category.id.to_string(),
                enabled: request.enabled,
                text: "Desktop preferences are not ready, so Sound Recognition categories cannot be changed in this session.".to_string(),
            }),
        );
    }

    if !schema_has_key("sounds") {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SoundRecognitionSoundToggleOutcome {
                ok: false,
                id: category.id.to_string(),
                enabled: request.enabled,
                text: "Sound Recognition categories are not ready because the preference is not installed.".to_string(),
            }),
        );
    }

    let current = normalize_enabled_sounds(read_sound_ids());
    let next = toggled_sound_ids(&current, category.id, request.enabled);
    let encoded = encode_sound_ids(&next);
    match gsettings(&["set", SCHEMA, "sounds", &encoded]) {
        Ok(_) => (
            StatusCode::OK,
            Json(SoundRecognitionSoundToggleOutcome {
                ok: true,
                id: category.id.to_string(),
                enabled: request.enabled,
                text: sound_toggle_success_detail(category, request.enabled),
            }),
        ),
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(SoundRecognitionSoundToggleOutcome {
                ok: false,
                id: category.id.to_string(),
                enabled: request.enabled,
                text: format!(
                    "{} could not be saved by the desktop session.",
                    category.name
                ),
            }),
        ),
    }
}

fn sound_recognition_target_spec(
    target: SoundRecognitionPreferenceTarget,
) -> SoundRecognitionTargetSpec {
    match target {
        SoundRecognitionPreferenceTarget::Enabled => SoundRecognitionTargetSpec {
            target: "enabled",
            key: "enabled",
            label: "Sound Recognition",
            kind: SoundRecognitionValueKind::Bool,
        },
        SoundRecognitionPreferenceTarget::Sensitivity => SoundRecognitionTargetSpec {
            target: "sensitivity",
            key: "sensitivity",
            label: "Sound Recognition sensitivity",
            kind: SoundRecognitionValueKind::Sensitivity,
        },
        SoundRecognitionPreferenceTarget::MinConfidence => SoundRecognitionTargetSpec {
            target: "min-confidence",
            key: "min-confidence",
            label: "Sound Recognition confidence",
            kind: SoundRecognitionValueKind::MinConfidence,
        },
        SoundRecognitionPreferenceTarget::AlertSound => SoundRecognitionTargetSpec {
            target: "alert-sound",
            key: "alert-sound",
            label: "Sound Recognition alert sound",
            kind: SoundRecognitionValueKind::Bool,
        },
        SoundRecognitionPreferenceTarget::AlertFlash => SoundRecognitionTargetSpec {
            target: "alert-flash",
            key: "alert-flash",
            label: "Sound Recognition screen flash",
            kind: SoundRecognitionValueKind::Bool,
        },
        SoundRecognitionPreferenceTarget::NotifyInLockScreen => SoundRecognitionTargetSpec {
            target: "notify-in-lock-screen",
            key: "notify-in-lock-screen",
            label: "Sound Recognition lock-screen alerts",
            kind: SoundRecognitionValueKind::Bool,
        },
    }
}

fn parse_sound_recognition_value(
    spec: SoundRecognitionTargetSpec,
    value: &Value,
) -> Result<SoundRecognitionPreferenceValue, String> {
    match spec.kind {
        SoundRecognitionValueKind::Bool => value
            .as_bool()
            .map(SoundRecognitionPreferenceValue::Bool)
            .ok_or_else(|| format!("{} expects true or false.", spec.label)),
        SoundRecognitionValueKind::Sensitivity => {
            let Some(value) = value.as_str() else {
                return Err(
                    "Sound Recognition sensitivity must be low, medium, or high.".to_string(),
                );
            };
            match value.trim() {
                "low" => Ok(SoundRecognitionPreferenceValue::Sensitivity("low")),
                "medium" => Ok(SoundRecognitionPreferenceValue::Sensitivity("medium")),
                "high" => Ok(SoundRecognitionPreferenceValue::Sensitivity("high")),
                _ => Err("Sound Recognition sensitivity must be low, medium, or high.".to_string()),
            }
        }
        SoundRecognitionValueKind::MinConfidence => {
            let Some(value) = value.as_f64() else {
                return Err("Sound Recognition confidence expects a number.".to_string());
            };
            Ok(SoundRecognitionPreferenceValue::MinConfidence(
                clamp_min_confidence(value),
            ))
        }
    }
}

fn encode_sound_recognition_value(value: &SoundRecognitionPreferenceValue) -> String {
    match value {
        SoundRecognitionPreferenceValue::Bool(value) => value.to_string(),
        SoundRecognitionPreferenceValue::Sensitivity(value) => format!("'{value}'"),
        SoundRecognitionPreferenceValue::MinConfidence(value) => format!("{value:.2}"),
    }
}

fn sound_recognition_preference_success_detail(
    spec: SoundRecognitionTargetSpec,
    value: &SoundRecognitionPreferenceValue,
) -> String {
    match (spec.target, value) {
        ("enabled", SoundRecognitionPreferenceValue::Bool(false)) => {
            "Sound Recognition is off. No microphone audio is captured by Sound Recognition."
                .to_string()
        }
        ("enabled", SoundRecognitionPreferenceValue::Bool(true)) => format!(
            "Sound Recognition is on, but it listens only when the local classifier model, listener, microphone capture path, and selected sounds are ready. {RELIABILITY_DETAIL}"
        ),
        ("sensitivity", SoundRecognitionPreferenceValue::Sensitivity(value)) => format!(
            "Sound Recognition sensitivity is set to {value}. It still requires the local classifier model, listener, and microphone capture path before it can listen."
        ),
        ("min-confidence", SoundRecognitionPreferenceValue::MinConfidence(value)) => format!(
            "Sound Recognition minimum confidence is set to {}%. It still requires the local classifier model, listener, and microphone capture path before it can listen.",
            (value * 100.0).round() as u32
        ),
        _ => format!(
            "{} saved. Sound Recognition still requires the local classifier model, listener, and microphone capture path before it can listen.",
            spec.label
        ),
    }
}

fn toggled_sound_ids(current: &[String], id: &str, enabled: bool) -> Vec<String> {
    let mut next = normalize_enabled_sounds(current.to_vec());
    if enabled {
        if !next.iter().any(|candidate| candidate == id) {
            next.push(id.to_string());
        }
    } else {
        next.retain(|candidate| candidate != id);
    }
    next
}

fn encode_sound_ids(ids: &[String]) -> String {
    let normalized = normalize_enabled_sounds(ids.to_vec());
    if normalized.is_empty() {
        return "@as []".to_string();
    }
    let quoted: Vec<String> = normalized.iter().map(|id| format!("'{id}'")).collect();
    format!("[{}]", quoted.join(", "))
}

fn sound_toggle_success_detail(category: &SoundCategory, enabled: bool) -> String {
    let state = if enabled { "enabled" } else { "disabled" };
    format!(
        "{} alerts are {state}. Sound Recognition still requires the local classifier model, listener, and microphone capture path before it can listen. {RELIABILITY_DETAIL}",
        category.name
    )
}

fn status_detail(
    schema_available: bool,
    enabled: bool,
    available: bool,
    active: bool,
    enabled_sounds: &[String],
) -> String {
    if !schema_available {
        return "Sound Recognition is unavailable here (its preferences schema is not installed)."
            .to_string();
    }
    if !enabled {
        return "Sound Recognition is off. It listens only after you turn it on and choose sounds."
            .to_string();
    }
    if enabled_sounds.is_empty() {
        return "Sound Recognition is on, but no sound categories are selected.".to_string();
    }
    if !available {
        return "Sound Recognition needs the local classifier model, listener, and microphone capture path before it can listen.".to_string();
    }
    if active {
        return format!(
            "Sound Recognition is listening for {} selected sound categories on this device.",
            enabled_sounds.len()
        );
    }
    "Sound Recognition is not active.".to_string()
}

/// Keep only allowlisted sound ids, drop duplicates, and preserve user order.
/// Pure + unit-tested so a malformed GSettings value never reaches the listener.
fn normalize_enabled_sounds(ids: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    ids.into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| sound_category_by_id(id).is_some())
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn sound_category_by_id(id: &str) -> Option<&'static SoundCategory> {
    SOUND_CATEGORIES.iter().find(|category| category.id == id)
}

fn normalize_sensitivity(value: &str) -> &'static str {
    match value.trim() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

fn clamp_min_confidence(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.70;
    }
    (value.clamp(0.30, 0.95) * 100.0).round() / 100.0
}

fn classifier_model_capability() -> Capability {
    match first_model(&model_dir(), &["onnx"]) {
        Some(model) => Capability {
            ready: true,
            component: model.display().to_string(),
            detail: format!("Classifier model ready at {}.", model.display()),
        },
        None => Capability {
            ready: false,
            component: model_dir().display().to_string(),
            detail: format!(
                "No recognition model in {} — add the classifier model to enable Sound Recognition.",
                model_dir().display()
            ),
        },
    }
}

fn listener_capability() -> Capability {
    let listener = listener_bin();
    if !binary_present(&listener) {
        return Capability {
            ready: false,
            component: listener,
            detail: "Sound Recognition listener is not installed in this session.".to_string(),
        };
    }

    match std::process::Command::new(&listener)
        .arg("--capability-check")
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            parse_listener_capability(&output.stdout, &listener).unwrap_or(Capability {
                ready: false,
                component: listener,
                detail: "Sound Recognition listener did not return a valid capability report."
                    .to_string(),
            })
        }
        _ => Capability {
            ready: false,
            component: listener,
            detail: "Sound Recognition listener could not run its capability check.".to_string(),
        },
    }
}

fn parse_listener_capability(raw: &[u8], fallback_component: &str) -> Option<Capability> {
    let payload: ListenerCapabilityPayload = serde_json::from_slice(raw).ok()?;
    Some(Capability {
        ready: payload.ready,
        component: payload
            .component
            .filter(|component| !component.trim().is_empty())
            .unwrap_or_else(|| fallback_component.to_string()),
        detail: payload
            .detail
            .filter(|detail| !detail.trim().is_empty())
            .unwrap_or_else(|| {
                if payload.ready {
                    "Sound Recognition listener is ready.".to_string()
                } else {
                    "Sound Recognition listener is installed but not ready.".to_string()
                }
            }),
    })
}

fn capture_capability() -> Capability {
    let binary = capture_bin();
    let ready = binary_present(&binary);
    Capability {
        ready,
        component: binary.clone(),
        detail: if ready {
            "Microphone capture path is available.".to_string()
        } else {
            "Microphone capture is not ready on this device.".to_string()
        },
    }
}

fn first_model(dir: &Path, extensions: &[&str]) -> Option<PathBuf> {
    let mut matches: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
        })
        .collect();
    matches.sort();
    matches.into_iter().next()
}

fn model_dir() -> PathBuf {
    env::var(MODEL_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_MODEL_DIR).to_path_buf())
}

fn listener_bin() -> String {
    env::var(LISTENER_BIN_ENV).unwrap_or_else(|_| DEFAULT_LISTENER.to_string())
}

fn capture_bin() -> String {
    env::var(CAPTURE_BIN_ENV).unwrap_or_else(|_| DEFAULT_CAPTURE_BIN.to_string())
}

fn binary_present(binary: &str) -> bool {
    if binary.contains('/') {
        return Path::new(binary).is_file();
    }
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

fn read_sound_ids() -> Vec<String> {
    gsettings(&["get", SCHEMA, "sounds"])
        .ok()
        .map(|value| parse_gsettings_strv(&value))
        .unwrap_or_default()
}

fn parse_gsettings_strv(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\'' {
            continue;
        }
        let mut item = String::new();
        loop {
            match chars.next() {
                None | Some('\'') => break,
                Some('\\') => {
                    if let Some(escaped) = chars.next() {
                        item.push(escaped);
                    }
                }
                Some(ch) => item.push(ch),
            }
        }
        out.push(item);
    }
    out
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

fn get_double(key: &str) -> Option<f64> {
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
    use super::{
        clamp_min_confidence, encode_sound_ids, normalize_enabled_sounds, normalize_sensitivity,
        parse_gsettings_strv, parse_listener_capability, parse_sound_recognition_value,
        sound_recognition_target_spec, toggled_sound_ids, SoundCategoryStatus,
        SoundRecognitionPreferenceTarget, SoundRecognitionPreferenceValue, SoundRecognitionStatus,
        SOUND_CATEGORIES,
    };

    #[test]
    fn registry_has_stable_allowlisted_sound_ids() {
        let ids: Vec<&str> = SOUND_CATEGORIES
            .iter()
            .map(|category| category.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "smoke-fire-alarm",
                "siren",
                "doorbell",
                "knock",
                "baby-crying",
                "dog-bark",
                "car-horn",
                "appliance-beep",
                "running-water",
                "shouting",
            ]
        );
        assert!(SOUND_CATEGORIES
            .iter()
            .all(|category| !category.audioset_classes.is_empty()));
    }

    #[test]
    fn normalizes_enabled_sounds_to_the_catalog() {
        let normalized = normalize_enabled_sounds(vec![
            "doorbell".to_string(),
            "unknown".to_string(),
            "doorbell".to_string(),
            "  siren  ".to_string(),
        ]);
        assert_eq!(
            normalized,
            vec!["doorbell".to_string(), "siren".to_string()]
        );
    }

    #[test]
    fn normalizes_sensitivity_and_confidence() {
        assert_eq!(normalize_sensitivity("low"), "low");
        assert_eq!(normalize_sensitivity("high"), "high");
        assert_eq!(normalize_sensitivity("LOUD"), "medium");
        assert_eq!(clamp_min_confidence(0.1), 0.30);
        assert_eq!(clamp_min_confidence(0.705), 0.71);
        assert_eq!(clamp_min_confidence(2.0), 0.95);
        assert_eq!(clamp_min_confidence(f64::NAN), 0.70);
    }

    #[test]
    fn parses_gsettings_sound_arrays() {
        assert_eq!(
            parse_gsettings_strv("['doorbell', 'siren']"),
            vec!["doorbell".to_string(), "siren".to_string()]
        );
        assert_eq!(parse_gsettings_strv("@as []"), Vec::<String>::new());
    }

    #[test]
    fn encodes_sound_arrays_from_allowlisted_ids() {
        assert_eq!(
            encode_sound_ids(&[
                "doorbell".to_string(),
                "unknown".to_string(),
                "doorbell".to_string(),
                "siren".to_string(),
            ]),
            "['doorbell', 'siren']"
        );
        assert_eq!(encode_sound_ids(&[]), "@as []");
    }

    #[test]
    fn toggles_sound_ids_without_leaking_unknown_categories() {
        let current = vec!["doorbell".to_string(), "unknown".to_string()];
        assert_eq!(
            toggled_sound_ids(&current, "siren", true),
            vec!["doorbell".to_string(), "siren".to_string()]
        );
        assert_eq!(
            toggled_sound_ids(&current, "doorbell", false),
            Vec::<String>::new()
        );
    }

    #[test]
    fn preference_values_are_type_checked_and_normalized() {
        let enabled = sound_recognition_target_spec(SoundRecognitionPreferenceTarget::Enabled);
        assert!(matches!(
            parse_sound_recognition_value(enabled, &serde_json::json!(true)).unwrap(),
            SoundRecognitionPreferenceValue::Bool(true)
        ));
        assert!(parse_sound_recognition_value(enabled, &serde_json::json!("true")).is_err());

        let sensitivity =
            sound_recognition_target_spec(SoundRecognitionPreferenceTarget::Sensitivity);
        assert!(matches!(
            parse_sound_recognition_value(sensitivity, &serde_json::json!("high")).unwrap(),
            SoundRecognitionPreferenceValue::Sensitivity("high")
        ));
        assert!(parse_sound_recognition_value(sensitivity, &serde_json::json!("loud")).is_err());

        let confidence =
            sound_recognition_target_spec(SoundRecognitionPreferenceTarget::MinConfidence);
        assert!(matches!(
            parse_sound_recognition_value(confidence, &serde_json::json!(0.99)).unwrap(),
            SoundRecognitionPreferenceValue::MinConfidence(value) if value == 0.95
        ));
    }

    #[test]
    fn listener_capability_check_preserves_not_ready_state() {
        let capability = parse_listener_capability(
            br#"{"ready":false,"component":"/usr/libexec/goblins-os/goblins-os-sound-listener","detail":"installed, model pending","runtime_ready_claim":false}"#,
            "/fallback",
        )
        .unwrap();

        assert!(!capability.ready);
        assert_eq!(
            capability.component,
            "/usr/libexec/goblins-os/goblins-os-sound-listener"
        );
        assert_eq!(capability.detail, "installed, model pending");

        let fallback = parse_listener_capability(br#"{"ready":false}"#, "/fallback").unwrap();
        assert_eq!(fallback.component, "/fallback");
        assert_eq!(
            fallback.detail,
            "Sound Recognition listener is installed but not ready."
        );
    }

    #[test]
    fn status_serializes_for_native_settings() {
        let status = SoundRecognitionStatus {
            source: "goblins-os-core",
            schema_available: true,
            enabled: false,
            available: false,
            active: false,
            offline_safe: true,
            sensitivity: "medium",
            min_confidence: 0.70,
            alert_sound: false,
            alert_flash: false,
            notify_in_lock_screen: false,
            classifier_model: super::Capability {
                ready: false,
                component: "/var/lib/goblins-os/sound-recognition".to_string(),
                detail: "missing".to_string(),
            },
            listener: super::Capability {
                ready: false,
                component: "/usr/libexec/goblins-os/goblins-os-sound-listener".to_string(),
                detail: "missing".to_string(),
            },
            capture: super::Capability {
                ready: false,
                component: "arecord".to_string(),
                detail: "missing".to_string(),
            },
            sounds: vec![SoundCategoryStatus {
                id: "doorbell",
                name: "Doorbell",
                group: "Home",
                enabled: false,
                description: "Doorbell chimes or buzzer-style entrance alerts.",
                audioset_classes: &["Doorbell"],
            }],
            reliability_detail: super::RELIABILITY_DETAIL,
            detail: "Sound Recognition is off.".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"offline_safe\":true"));
        assert!(json.contains("\"classifier_model\""));
        assert!(json.contains("\"doorbell\""));
        assert!(json.contains("Do not rely on it in emergencies"));
    }
}
