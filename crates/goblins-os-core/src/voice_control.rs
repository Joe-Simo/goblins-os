//! Voice Control substrate (spoken command → registered action, resolve-only).
//!
//! The macOS "Voice Control" altitude, v1 = the command vocabulary. Goblins already
//! owns a curated, policy-gated action registry; this maps a normalized spoken
//! phrase to one of those actions. This module ships the host-testable core — phrase
//! normalization, the deterministic match, and the no-match→dictation fall-through —
//! plus resolve-only routes. It NEVER executes: capture → transcribe → dispatch
//! through the existing gated `change_safe_setting`/`open_settings_panel` handlers is
//! the deliberate engine follow-up. Deterministic match first; an LLM may later
//! *propose* but never auto-execute a state change.

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A spoken command and the registered action it resolves to. `action_id` names an
/// existing gated handler; execution still flows through that handler's policy.
#[derive(Clone, Copy)]
enum VoiceCommandTarget {
    OpenSettings {
        query: &'static str,
    },
    SafeSetting {
        setting_id: &'static str,
        value: VoiceSettingValue,
    },
    Deferred {
        reason: &'static str,
    },
}

#[derive(Clone, Copy)]
enum VoiceSettingValue {
    Bool(bool),
    String(&'static str),
}

impl VoiceSettingValue {
    fn to_json(self) -> Value {
        match self {
            Self::Bool(value) => Value::Bool(value),
            Self::String(value) => Value::String(value.to_string()),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
pub struct VoiceCommand {
    phrase: &'static str,
    action_id: &'static str,
    action_title: &'static str,
    #[serde(skip_serializing)]
    target: VoiceCommandTarget,
}

/// The curated v1 command vocabulary. Phrases are already normalized (lowercase,
/// single-spaced) so a transcript matches after `normalize_phrase`.
const VOCABULARY: &[VoiceCommand] = &[
    VoiceCommand {
        phrase: "turn on dark mode",
        action_id: "change-safe-setting",
        action_title: "Turn on Dark Mode",
        target: VoiceCommandTarget::SafeSetting {
            setting_id: "appearance.color-scheme",
            value: VoiceSettingValue::String("dark"),
        },
    },
    VoiceCommand {
        phrase: "turn off dark mode",
        action_id: "change-safe-setting",
        action_title: "Turn off Dark Mode",
        target: VoiceCommandTarget::SafeSetting {
            setting_id: "appearance.color-scheme",
            value: VoiceSettingValue::String("light"),
        },
    },
    VoiceCommand {
        phrase: "open settings",
        action_id: "open-settings-panel",
        action_title: "Open Settings",
        target: VoiceCommandTarget::OpenSettings { query: "settings" },
    },
    VoiceCommand {
        phrase: "open accessibility settings",
        action_id: "open-settings-panel",
        action_title: "Open Accessibility",
        target: VoiceCommandTarget::OpenSettings {
            query: "accessibility",
        },
    },
    VoiceCommand {
        phrase: "open network settings",
        action_id: "open-settings-panel",
        action_title: "Open Network",
        target: VoiceCommandTarget::OpenSettings { query: "network" },
    },
    VoiceCommand {
        phrase: "increase text size",
        action_id: "change-safe-setting",
        action_title: "Increase text size",
        target: VoiceCommandTarget::Deferred {
            reason:
                "Text-size changes need the confirmation card before Voice Control can run them.",
        },
    },
    VoiceCommand {
        phrase: "decrease text size",
        action_id: "change-safe-setting",
        action_title: "Decrease text size",
        target: VoiceCommandTarget::Deferred {
            reason:
                "Text-size changes need the confirmation card before Voice Control can run them.",
        },
    },
    VoiceCommand {
        phrase: "turn on do not disturb",
        action_id: "change-safe-setting",
        action_title: "Turn on Do Not Disturb",
        target: VoiceCommandTarget::SafeSetting {
            setting_id: "notifications.show-banners",
            value: VoiceSettingValue::Bool(false),
        },
    },
    VoiceCommand {
        phrase: "turn off do not disturb",
        action_id: "change-safe-setting",
        action_title: "Turn off Do Not Disturb",
        target: VoiceCommandTarget::SafeSetting {
            setting_id: "notifications.show-banners",
            value: VoiceSettingValue::Bool(true),
        },
    },
];

#[derive(Serialize)]
pub struct VoiceControlVocabulary {
    source: &'static str,
    /// Whether the speech engine (whisper) is available to transcribe commands.
    engine_available: bool,
    commands: Vec<VoiceCommand>,
    detail: String,
}

#[derive(Deserialize)]
pub struct ResolveRequest {
    transcript: String,
}

#[derive(Deserialize)]
pub struct VoiceControlRequest {
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    confirmed: Option<bool>,
}

#[derive(Serialize)]
pub struct ResolveOutcome {
    transcript: String,
    normalized: String,
    matched: bool,
    matched_action_id: Option<&'static str>,
    action_title: Option<&'static str>,
    /// When nothing matches we never guess — the engine types the words instead.
    fall_through_to_dictation: bool,
    detail: String,
}

#[derive(Serialize)]
pub struct VoiceControlOutcome {
    ok: bool,
    transcript: String,
    normalized: String,
    matched_action_id: Option<&'static str>,
    action_title: Option<&'static str>,
    executed: bool,
    needs_confirmation: bool,
    fall_through_to_dictation: bool,
    launch_argument: Option<String>,
    text: String,
}

pub async fn voice_control_vocabulary() -> Json<VoiceControlVocabulary> {
    let engine_available = engine_present();
    Json(VoiceControlVocabulary {
        source: "goblins-os-core",
        engine_available,
        commands: VOCABULARY.to_vec(),
        detail: if engine_available {
            "Hold the Voice Control key and say a command.".to_string()
        } else {
            "Voice Control needs an on-device speech model. Add one to enable spoken commands."
                .to_string()
        },
    })
}

pub async fn resolve_voice_command(Json(request): Json<ResolveRequest>) -> Json<ResolveOutcome> {
    let normalized = normalize_phrase(&request.transcript);
    match match_command(&normalized, VOCABULARY) {
        Some(command) => Json(ResolveOutcome {
            transcript: request.transcript,
            normalized,
            matched: true,
            matched_action_id: Some(command.action_id),
            action_title: Some(command.action_title),
            fall_through_to_dictation: false,
            detail: format!("Heard: {} → {}", command.phrase, command.action_title),
        }),
        None => Json(ResolveOutcome {
            transcript: request.transcript,
            normalized,
            matched: false,
            matched_action_id: None,
            action_title: None,
            fall_through_to_dictation: true,
            detail: "No command matched — the words will be typed instead.".to_string(),
        }),
    }
}

pub async fn voice_control(
    Json(request): Json<VoiceControlRequest>,
) -> (StatusCode, Json<VoiceControlOutcome>) {
    let transcript = match request
        .transcript
        .as_deref()
        .map(sanitize_transcript)
        .filter(|value| !value.is_empty())
    {
        Some(transcript) => transcript,
        None => match crate::voice::capture_voice_command_transcript() {
            Ok(transcript) => transcript,
            Err(detail) => {
                crate::ai::audit_ai_action(
                    "voice-control",
                    Some("voice"),
                    crate::ai::AiActionOutcome::Blocked,
                );
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(VoiceControlOutcome {
                        ok: false,
                        transcript: String::new(),
                        normalized: String::new(),
                        matched_action_id: None,
                        action_title: None,
                        executed: false,
                        needs_confirmation: false,
                        fall_through_to_dictation: false,
                        launch_argument: None,
                        text: detail,
                    }),
                );
            }
        },
    };

    voice_control_for_transcript(transcript, request.confirmed == Some(true))
}

/// Lowercase, strip surrounding punctuation, and collapse whitespace so a transcript
/// matches the curated phrases. Pure + unit-tested.
fn normalize_phrase(transcript: &str) -> String {
    transcript
        .to_lowercase()
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The exact command for a normalized transcript, if any. Deterministic match only —
/// never a guess. Pure + unit-tested.
fn match_command<'a>(normalized: &str, vocabulary: &'a [VoiceCommand]) -> Option<&'a VoiceCommand> {
    vocabulary
        .iter()
        .find(|command| command.phrase == normalized)
}

fn voice_control_for_transcript(
    transcript: String,
    confirmed: bool,
) -> (StatusCode, Json<VoiceControlOutcome>) {
    let normalized = normalize_phrase(&transcript);
    let Some(command) = match_command(&normalized, VOCABULARY) else {
        crate::ai::audit_ai_action(
            "voice-control",
            Some("voice"),
            crate::ai::AiActionOutcome::Succeeded,
        );
        return (
            StatusCode::OK,
            Json(VoiceControlOutcome {
                ok: true,
                transcript,
                normalized,
                matched_action_id: None,
                action_title: None,
                executed: false,
                needs_confirmation: false,
                fall_through_to_dictation: true,
                launch_argument: None,
                text: "No command matched — the words will be typed instead.".to_string(),
            }),
        );
    };

    match command.target {
        VoiceCommandTarget::OpenSettings { query } => {
            let (status, dispatch) = crate::ai::dispatch_voice_open_settings(query);
            let ok = status.is_success();
            crate::ai::audit_ai_action(
                "voice-control",
                Some("voice"),
                if ok {
                    crate::ai::AiActionOutcome::Succeeded
                } else {
                    crate::ai::AiActionOutcome::Blocked
                },
            );
            (
                status,
                Json(VoiceControlOutcome {
                    ok,
                    transcript,
                    normalized,
                    matched_action_id: Some(command.action_id),
                    action_title: dispatch.panel_title.or(Some(command.action_title)),
                    executed: false,
                    needs_confirmation: false,
                    fall_through_to_dictation: false,
                    launch_argument: dispatch.launch_argument,
                    text: format!("Heard: {} → {}", command.phrase, dispatch.text),
                }),
            )
        }
        VoiceCommandTarget::SafeSetting { setting_id, value } => {
            let (status, dispatch) = crate::ai::dispatch_voice_safe_setting_change(
                setting_id,
                value.to_json(),
                confirmed,
            );
            let ok = status.is_success();
            crate::ai::audit_ai_action(
                "voice-control",
                Some("voice"),
                if dispatch.needs_confirmation {
                    crate::ai::AiActionOutcome::ConfirmationRequired
                } else if ok {
                    crate::ai::AiActionOutcome::Succeeded
                } else {
                    crate::ai::AiActionOutcome::Blocked
                },
            );
            (
                status,
                Json(VoiceControlOutcome {
                    ok,
                    transcript,
                    normalized,
                    matched_action_id: Some(command.action_id),
                    action_title: Some(command.action_title),
                    executed: dispatch.applied,
                    needs_confirmation: dispatch.needs_confirmation,
                    fall_through_to_dictation: false,
                    launch_argument: None,
                    text: format!("Heard: {} → {}", command.phrase, dispatch.text),
                }),
            )
        }
        VoiceCommandTarget::Deferred { reason } => {
            crate::ai::audit_ai_action(
                "voice-control",
                Some("voice"),
                crate::ai::AiActionOutcome::ConfirmationRequired,
            );
            (
                StatusCode::PRECONDITION_REQUIRED,
                Json(VoiceControlOutcome {
                    ok: false,
                    transcript,
                    normalized,
                    matched_action_id: Some(command.action_id),
                    action_title: Some(command.action_title),
                    executed: false,
                    needs_confirmation: true,
                    fall_through_to_dictation: false,
                    launch_argument: None,
                    text: format!("Heard: {} → {}", command.phrase, reason),
                }),
            )
        }
    }
}

fn sanitize_transcript(transcript: &str) -> String {
    transcript.trim().chars().take(600).collect()
}

fn engine_present() -> bool {
    let binary =
        std::env::var("GOBLINS_OS_WHISPER_BIN").unwrap_or_else(|_| "whisper-cli".to_string());
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(&binary).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{
        match_command, normalize_phrase, VoiceCommandTarget, VoiceSettingValue, VOCABULARY,
    };

    #[test]
    fn normalizes_case_punctuation_and_spacing() {
        assert_eq!(normalize_phrase("  Open   Settings! "), "open settings");
        assert_eq!(normalize_phrase("Turn ON, dark mode."), "turn on dark mode");
        assert_eq!(normalize_phrase(""), "");
    }

    #[test]
    fn matches_only_exact_normalized_phrases() {
        assert!(match_command("open settings", VOCABULARY).is_some());
        assert_eq!(
            match_command("turn on dark mode", VOCABULARY)
                .unwrap()
                .action_id,
            "change-safe-setting"
        );
        // A near-miss does not guess — caller falls through to dictation.
        assert!(match_command("open the settings please", VOCABULARY).is_none());
    }

    #[test]
    fn vocabulary_never_names_the_apple_assistant() {
        for command in VOCABULARY {
            let lowered = command.action_title.to_lowercase();
            assert!(!lowered.contains("siri"));
        }
    }

    #[test]
    fn voice_control_targets_only_allowlisted_dispatches() {
        let command = match_command("turn on dark mode", VOCABULARY).unwrap();
        match command.target {
            VoiceCommandTarget::SafeSetting { setting_id, value } => {
                assert_eq!(setting_id, "appearance.color-scheme");
                assert!(matches!(value, VoiceSettingValue::String("dark")));
            }
            _ => panic!("dark mode should be a safe-setting command"),
        }

        let command = match_command("open network settings", VOCABULARY).unwrap();
        assert!(matches!(
            command.target,
            VoiceCommandTarget::OpenSettings { query: "network" }
        ));
    }

    #[test]
    fn unsupported_matched_commands_are_deferred_not_guessed() {
        let command = match_command("increase text size", VOCABULARY).unwrap();
        assert!(matches!(
            command.target,
            VoiceCommandTarget::Deferred { .. }
        ));
        assert!(match_command("open the settings please", VOCABULARY).is_none());
    }
}
