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

use axum::Json;
use serde::Serialize;

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

pub async fn sound_recognition_status() -> Json<SoundRecognitionStatus> {
    Json(build_status())
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
    let ready = binary_present(&listener);
    Capability {
        ready,
        component: listener.clone(),
        detail: if ready {
            "Sound Recognition listener is installed.".to_string()
        } else {
            "Sound Recognition listener is not installed in this session.".to_string()
        },
    }
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
        clamp_min_confidence, normalize_enabled_sounds, normalize_sensitivity,
        parse_gsettings_strv, SoundCategoryStatus, SoundRecognitionStatus, SOUND_CATEGORIES,
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
