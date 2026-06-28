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
const DEFAULT_LISTENER: &str = "/usr/libexec/goblins-os/goblins-os-sound-listener";
const RELIABILITY_DETAIL: &str = "This recognizes sounds approximately and on-device only. Do not rely on it in emergencies or high-risk situations.";
const NOTIFICATION_APP_NAME: &str = "Goblins OS Sound Recognition";
const NOTIFICATION_APP_ID: &str = "org.goblins.OS.SoundRecognition";
const NOTIFICATION_APP_ICON: &str = "goblins-os";
const NOTIFICATION_DESKTOP_ENTRY: &str = "org.goblins.OS.Settings";
const NOTIFICATION_CATEGORY_HINT: &str = "device.sound-recognition";

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
    decision_engine: Capability,
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
    runtime_ready_claim: Option<bool>,
    capture_driver: Option<String>,
    capture_driver_name: Option<String>,
    capture_runtime_ready: Option<bool>,
    capture_detail: Option<String>,
}

struct ListenerRuntimeCapabilities {
    listener: Capability,
    capture: Capability,
}

#[derive(Clone, Copy)]
struct ClassifierScore<'a> {
    audioset_class: &'a str,
    confidence: f64,
}

#[derive(Clone, Copy)]
struct SoundRecognitionLastAlert<'a> {
    id: &'a str,
    last_ms: u64,
}

#[derive(Clone, Copy)]
struct SoundRecognitionDecisionContext<'a> {
    enabled_sound_ids: &'a [String],
    sensitivity: &'a str,
    min_confidence: f64,
    alert_sound: bool,
    alert_flash: bool,
    notify_in_lock_screen: bool,
    now_ms: u64,
    last_alerts: &'a [SoundRecognitionLastAlert<'a>],
    debounce_ms: u64,
}

#[derive(Debug, PartialEq)]
struct SoundRecognitionAlert {
    category_id: &'static str,
    category_name: &'static str,
    audioset_class: String,
    confidence: f64,
    notification_title: String,
    notification_body: String,
    alert_sound: bool,
    alert_flash: bool,
    delivery_plan: SoundRecognitionNotificationDeliveryPlan,
}

#[derive(Debug, PartialEq)]
struct SoundRecognitionNotificationDeliveryPlan {
    app_name: &'static str,
    app_id: &'static str,
    app_icon: &'static str,
    desktop_entry: &'static str,
    summary: String,
    body: String,
    urgency: &'static str,
    category_hint: &'static str,
    expire_timeout_ms: i32,
    alert_sound: bool,
    alert_flash: bool,
    show_on_lock_screen: bool,
    delivery_ready_claim: bool,
}

#[derive(Debug, PartialEq)]
enum SoundRecognitionDecision {
    Alert(Box<SoundRecognitionAlert>),
    Suppressed { reason: &'static str },
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
    let listener_runtime = listener_runtime_capabilities();
    let listener = listener_runtime.listener;
    let capture = listener_runtime.capture;
    let decision_engine = decision_engine_capability();
    let available =
        decision_engine.ready && classifier_model.ready && listener.ready && capture.ready;
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
        decision_engine,
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

fn evaluate_sound_recognition_window(
    scores: &[ClassifierScore<'_>],
    context: &SoundRecognitionDecisionContext<'_>,
) -> SoundRecognitionDecision {
    let enabled_sounds = normalize_enabled_sounds(context.enabled_sound_ids.to_vec());
    if enabled_sounds.is_empty() {
        return SoundRecognitionDecision::Suppressed {
            reason: "no-enabled-sounds",
        };
    }

    let threshold = sound_decision_threshold(context.sensitivity, context.min_confidence);
    let Some((category, score)) = scores
        .iter()
        .filter(|score| score.confidence.is_finite())
        .filter(|score| score.confidence >= threshold)
        .filter_map(|score| {
            let category = SOUND_CATEGORIES.iter().find(|category| {
                enabled_sounds.iter().any(|id| id == category.id)
                    && audioset_class_matches(category, score.audioset_class)
            })?;
            Some((category, score))
        })
        .max_by(|(_, left), (_, right)| left.confidence.total_cmp(&right.confidence))
    else {
        return SoundRecognitionDecision::Suppressed {
            reason: "below-threshold-or-unmapped",
        };
    };

    if context.last_alerts.iter().any(|last| {
        last.id == category.id && context.now_ms.saturating_sub(last.last_ms) < context.debounce_ms
    }) {
        return SoundRecognitionDecision::Suppressed {
            reason: "debounced",
        };
    }

    SoundRecognitionDecision::Alert(Box::new(sound_recognition_notification_payload(
        category,
        score.audioset_class,
        score.confidence,
        context.alert_sound,
        context.alert_flash,
        context.notify_in_lock_screen,
    )))
}

fn sound_recognition_notification_payload(
    category: &'static SoundCategory,
    audioset_class: &str,
    confidence: f64,
    alert_sound: bool,
    alert_flash: bool,
    notify_in_lock_screen: bool,
) -> SoundRecognitionAlert {
    let clamped_confidence = confidence.clamp(0.0, 1.0);
    let notification_title = format!("Sound recognized: {}", category.name);
    let notification_body = format!(
        "{} matched \"{}\" at {}% confidence. {RELIABILITY_DETAIL}",
        category.name,
        audioset_class,
        (clamped_confidence * 100.0).round() as u32
    );
    let delivery_plan = sound_recognition_notification_delivery_plan(
        category,
        &notification_title,
        &notification_body,
        alert_sound,
        alert_flash,
        notify_in_lock_screen,
    );

    SoundRecognitionAlert {
        category_id: category.id,
        category_name: category.name,
        audioset_class: audioset_class.to_string(),
        confidence: (clamped_confidence * 100.0).round() / 100.0,
        notification_title,
        notification_body,
        alert_sound,
        alert_flash,
        delivery_plan,
    }
}

fn sound_recognition_notification_delivery_plan(
    category: &'static SoundCategory,
    summary: &str,
    body: &str,
    alert_sound: bool,
    alert_flash: bool,
    show_on_lock_screen: bool,
) -> SoundRecognitionNotificationDeliveryPlan {
    SoundRecognitionNotificationDeliveryPlan {
        app_name: NOTIFICATION_APP_NAME,
        app_id: NOTIFICATION_APP_ID,
        app_icon: NOTIFICATION_APP_ICON,
        desktop_entry: NOTIFICATION_DESKTOP_ENTRY,
        summary: summary.to_string(),
        body: body.to_string(),
        urgency: sound_recognition_notification_urgency(category),
        category_hint: NOTIFICATION_CATEGORY_HINT,
        expire_timeout_ms: 10_000,
        alert_sound,
        alert_flash,
        show_on_lock_screen,
        delivery_ready_claim: false,
    }
}

fn sound_recognition_notification_urgency(category: &SoundCategory) -> &'static str {
    match category.group {
        "Safety" => "critical",
        _ => "normal",
    }
}

fn audioset_class_matches(category: &SoundCategory, audioset_class: &str) -> bool {
    let needle = normalize_audioset_label(audioset_class);
    if needle.is_empty() {
        return false;
    }
    category.audioset_classes.iter().any(|candidate| {
        let candidate = normalize_audioset_label(candidate);
        candidate == needle || candidate.contains(&needle) || needle.contains(&candidate)
    })
}

fn normalize_audioset_label(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn sound_decision_threshold(sensitivity: &str, min_confidence: f64) -> f64 {
    let base = clamp_min_confidence(min_confidence);
    let adjusted = match normalize_sensitivity(sensitivity) {
        "low" => base + 0.10,
        "high" => base - 0.10,
        _ => base,
    };
    clamp_min_confidence(adjusted)
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

fn decision_engine_capability() -> Capability {
    let enabled = vec!["doorbell".to_string()];
    let context = SoundRecognitionDecisionContext {
        enabled_sound_ids: &enabled,
        sensitivity: "medium",
        min_confidence: 0.70,
        alert_sound: false,
        alert_flash: false,
        notify_in_lock_screen: false,
        now_ms: 30_000,
        last_alerts: &[],
        debounce_ms: 30_000,
    };
    let ready = matches!(
        evaluate_sound_recognition_window(
            &[ClassifierScore {
                audioset_class: "Doorbell",
                confidence: 0.91,
            }],
            &context,
        ),
        SoundRecognitionDecision::Alert(alert) if alert.category_id == "doorbell"
    );
    Capability {
        ready,
        component: "goblins-os-core sound-recognition decision contract".to_string(),
        detail: if ready {
            "Classifier score mapping, thresholding, debounce, and notification payload construction are host-tested; live microphone capture is still gated.".to_string()
        } else {
            "Sound Recognition decision contract could not pass its local self-check.".to_string()
        },
    }
}

fn listener_runtime_capabilities() -> ListenerRuntimeCapabilities {
    let listener = listener_bin();
    if !binary_present(&listener) {
        return listener_unavailable_capabilities(
            listener,
            "Sound Recognition listener is not installed in this session.",
        );
    }

    match std::process::Command::new(&listener)
        .arg("--capability-check")
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            parse_listener_runtime_capabilities(&output.stdout, &listener).unwrap_or_else(|| {
                listener_unavailable_capabilities(
                    listener,
                    "Sound Recognition listener did not return a valid capability report.",
                )
            })
        }
        _ => listener_unavailable_capabilities(
            listener,
            "Sound Recognition listener could not run its capability check.",
        ),
    }
}

fn listener_unavailable_capabilities(
    listener: String,
    detail: &str,
) -> ListenerRuntimeCapabilities {
    ListenerRuntimeCapabilities {
        listener: Capability {
            ready: false,
            component: listener.clone(),
            detail: detail.to_string(),
        },
        capture: Capability {
            ready: false,
            component: listener,
            detail: "Microphone capture is controlled by the Sound Recognition listener, which is not ready in this session.".to_string(),
        },
    }
}

fn parse_listener_runtime_capabilities(
    raw: &[u8],
    fallback_component: &str,
) -> Option<ListenerRuntimeCapabilities> {
    let payload: ListenerCapabilityPayload = serde_json::from_slice(raw).ok()?;
    let runtime_ready_claim = payload.runtime_ready_claim.unwrap_or(false);
    let listener = Capability {
        ready: payload.ready && runtime_ready_claim,
        component: payload
            .component
            .as_ref()
            .filter(|component| !component.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| fallback_component.to_string()),
        detail: payload
            .detail
            .filter(|detail| !detail.trim().is_empty())
            .unwrap_or_else(|| {
                if payload.ready && runtime_ready_claim {
                    "Sound Recognition listener is ready.".to_string()
                } else {
                    "Sound Recognition listener is installed but not ready.".to_string()
                }
            }),
    };

    let capture_driver = payload
        .capture_driver
        .filter(|driver| !driver.trim().is_empty())
        .or_else(|| {
            payload
                .capture_driver_name
                .filter(|driver| !driver.trim().is_empty())
        })
        .unwrap_or_else(|| "session listener capture".to_string());
    let capture_runtime_ready = payload.capture_runtime_ready.unwrap_or(false);
    let capture = Capability {
        ready: payload.ready && runtime_ready_claim && capture_runtime_ready,
        component: capture_driver,
        detail: payload.capture_detail.unwrap_or_else(|| {
            if capture_runtime_ready {
                "Microphone capture runtime is ready.".to_string()
            } else {
                "Microphone capture is reported by the listener but has not been proven in this session.".to_string()
            }
        }),
    };

    Some(ListenerRuntimeCapabilities { listener, capture })
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
        audioset_class_matches, clamp_min_confidence, encode_sound_ids,
        evaluate_sound_recognition_window, normalize_audioset_label, normalize_enabled_sounds,
        normalize_sensitivity, parse_gsettings_strv, parse_listener_runtime_capabilities,
        parse_sound_recognition_value, sound_decision_threshold, sound_recognition_target_spec,
        toggled_sound_ids, ClassifierScore, SoundCategoryStatus, SoundRecognitionDecision,
        SoundRecognitionDecisionContext, SoundRecognitionLastAlert,
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
    fn classifier_output_maps_only_to_enabled_sound_categories() {
        let enabled = vec!["doorbell".to_string(), "siren".to_string()];
        let context = SoundRecognitionDecisionContext {
            enabled_sound_ids: &enabled,
            sensitivity: "medium",
            min_confidence: 0.70,
            alert_sound: true,
            alert_flash: true,
            notify_in_lock_screen: true,
            now_ms: 60_000,
            last_alerts: &[],
            debounce_ms: 30_000,
        };

        let decision = evaluate_sound_recognition_window(
            &[
                ClassifierScore {
                    audioset_class: "Vehicle horn, car horn, honking",
                    confidence: 0.99,
                },
                ClassifierScore {
                    audioset_class: "Doorbell",
                    confidence: 0.91,
                },
            ],
            &context,
        );

        let SoundRecognitionDecision::Alert(alert) = decision else {
            panic!("expected enabled doorbell alert");
        };
        assert_eq!(alert.category_id, "doorbell");
        assert_eq!(alert.category_name, "Doorbell");
        assert!(alert.notification_title.contains("Sound recognized"));
        assert!(alert
            .notification_body
            .contains("Do not rely on it in emergencies"));
        assert!(alert.alert_sound);
        assert!(alert.alert_flash);
        assert_eq!(
            alert.delivery_plan.app_id,
            "org.goblins.OS.SoundRecognition"
        );
        assert_eq!(alert.delivery_plan.app_icon, "goblins-os");
        assert_eq!(alert.delivery_plan.desktop_entry, "org.goblins.OS.Settings");
        assert_eq!(alert.delivery_plan.summary, alert.notification_title);
        assert_eq!(alert.delivery_plan.body, alert.notification_body);
        assert_eq!(alert.delivery_plan.urgency, "normal");
        assert_eq!(
            alert.delivery_plan.category_hint,
            "device.sound-recognition"
        );
        assert_eq!(alert.delivery_plan.expire_timeout_ms, 10_000);
        assert!(alert.delivery_plan.alert_sound);
        assert!(alert.delivery_plan.alert_flash);
        assert!(alert.delivery_plan.show_on_lock_screen);
        assert!(!alert.delivery_plan.delivery_ready_claim);

        let safety_enabled = vec!["siren".to_string()];
        let safety_context = SoundRecognitionDecisionContext {
            enabled_sound_ids: &safety_enabled,
            sensitivity: "medium",
            min_confidence: 0.70,
            alert_sound: false,
            alert_flash: false,
            notify_in_lock_screen: false,
            now_ms: 60_000,
            last_alerts: &[],
            debounce_ms: 30_000,
        };
        let SoundRecognitionDecision::Alert(safety_alert) = evaluate_sound_recognition_window(
            &[ClassifierScore {
                audioset_class: "Siren",
                confidence: 0.97,
            }],
            &safety_context,
        ) else {
            panic!("expected safety alert");
        };
        assert_eq!(safety_alert.delivery_plan.urgency, "critical");
        assert!(!safety_alert.delivery_plan.delivery_ready_claim);
    }

    #[test]
    fn classifier_output_respects_threshold_and_sensitivity() {
        assert_eq!(sound_decision_threshold("medium", 0.70), 0.70);
        assert_eq!(sound_decision_threshold("high", 0.70), 0.60);
        assert_eq!(sound_decision_threshold("low", 0.70), 0.80);

        let enabled = vec!["siren".to_string()];
        let high_sensitivity = SoundRecognitionDecisionContext {
            enabled_sound_ids: &enabled,
            sensitivity: "high",
            min_confidence: 0.70,
            alert_sound: false,
            alert_flash: false,
            notify_in_lock_screen: false,
            now_ms: 60_000,
            last_alerts: &[],
            debounce_ms: 30_000,
        };
        let low_sensitivity = SoundRecognitionDecisionContext {
            sensitivity: "low",
            ..high_sensitivity
        };
        let scores = [ClassifierScore {
            audioset_class: "Siren",
            confidence: 0.65,
        }];

        assert!(matches!(
            evaluate_sound_recognition_window(&scores, &high_sensitivity),
            SoundRecognitionDecision::Alert(_)
        ));
        assert_eq!(
            evaluate_sound_recognition_window(&scores, &low_sensitivity),
            SoundRecognitionDecision::Suppressed {
                reason: "below-threshold-or-unmapped"
            }
        );
    }

    #[test]
    fn classifier_output_debounces_repeated_alerts() {
        let enabled = vec!["baby-crying".to_string()];
        let last_alerts = [SoundRecognitionLastAlert {
            id: "baby-crying",
            last_ms: 45_000,
        }];
        let context = SoundRecognitionDecisionContext {
            enabled_sound_ids: &enabled,
            sensitivity: "medium",
            min_confidence: 0.70,
            alert_sound: false,
            alert_flash: false,
            notify_in_lock_screen: false,
            now_ms: 60_000,
            last_alerts: &last_alerts,
            debounce_ms: 30_000,
        };

        assert_eq!(
            evaluate_sound_recognition_window(
                &[ClassifierScore {
                    audioset_class: "Baby cry, infant cry",
                    confidence: 0.96,
                }],
                &context,
            ),
            SoundRecognitionDecision::Suppressed {
                reason: "debounced"
            }
        );
    }

    #[test]
    fn audioset_label_matching_is_stable_for_classifier_variants() {
        let car_horn = SOUND_CATEGORIES
            .iter()
            .find(|category| category.id == "car-horn")
            .unwrap();
        assert_eq!(
            normalize_audioset_label("Vehicle horn, car horn, honking"),
            "vehiclehorncarhornhonking"
        );
        assert!(audioset_class_matches(car_horn, "car horn"));
        assert!(!audioset_class_matches(car_horn, "Doorbell"));
    }

    #[test]
    fn listener_capability_check_preserves_not_ready_state() {
        let runtime = parse_listener_runtime_capabilities(
            br#"{"ready":false,"component":"/usr/libexec/goblins-os/goblins-os-sound-listener","detail":"installed, model pending","runtime_ready_claim":false,"capture_driver":"/usr/bin/arecord","capture_driver_name":"arecord","capture_runtime_ready":false,"capture_detail":"capture driver present, runtime unproven"}"#,
            "/fallback",
        )
        .unwrap();

        assert!(!runtime.listener.ready);
        assert_eq!(
            runtime.listener.component,
            "/usr/libexec/goblins-os/goblins-os-sound-listener"
        );
        assert_eq!(runtime.listener.detail, "installed, model pending");
        assert!(!runtime.capture.ready);
        assert_eq!(runtime.capture.component, "/usr/bin/arecord");
        assert_eq!(
            runtime.capture.detail,
            "capture driver present, runtime unproven"
        );

        let fallback =
            parse_listener_runtime_capabilities(br#"{"ready":false}"#, "/fallback").unwrap();
        assert_eq!(fallback.listener.component, "/fallback");
        assert_eq!(
            fallback.listener.detail,
            "Sound Recognition listener is installed but not ready."
        );
        assert_eq!(fallback.capture.component, "session listener capture");
        assert!(!fallback.capture.ready);

        let explicit_ready = parse_listener_runtime_capabilities(
            br#"{"ready":true,"runtime_ready_claim":true,"capture_driver":"/usr/bin/arecord","capture_runtime_ready":true}"#,
            "/fallback",
        )
        .unwrap();
        assert!(explicit_ready.listener.ready);
        assert!(explicit_ready.capture.ready);

        let no_runtime_claim = parse_listener_runtime_capabilities(
            br#"{"ready":true,"capture_driver":"/usr/bin/arecord","capture_runtime_ready":true}"#,
            "/fallback",
        )
        .unwrap();
        assert!(!no_runtime_claim.listener.ready);
        assert!(!no_runtime_claim.capture.ready);
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
            decision_engine: super::Capability {
                ready: true,
                component: "goblins-os-core sound-recognition decision contract".to_string(),
                detail: "ready".to_string(),
            },
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
        assert!(json.contains("\"decision_engine\""));
        assert!(json.contains("\"classifier_model\""));
        assert!(json.contains("\"doorbell\""));
        assert!(json.contains("Do not rely on it in emergencies"));
    }
}
