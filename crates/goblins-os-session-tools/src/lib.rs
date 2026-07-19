use serde::Deserialize;

const MAX_PANEL_ID_BYTES: usize = 64;

#[derive(Deserialize)]
struct FocusOutcome {
    text: Option<String>,
}

#[derive(Deserialize)]
struct DictateOutcome {
    ok: bool,
    transcript: String,
}

#[derive(Deserialize)]
struct VoiceControlOutcome {
    ok: bool,
    transcript: String,
    fall_through_to_dictation: bool,
    launch_argument: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VoiceControlAction {
    OpenSettings(String),
    TypeTranscript(String),
}

#[must_use]
pub fn focus_log_text(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<FocusOutcome>(body)
        .ok()?
        .text
        .filter(|text| !text.is_empty())
}

#[must_use]
pub fn dictation_transcript(success: bool, body: &[u8]) -> Option<String> {
    if !success {
        return None;
    }
    let outcome = serde_json::from_slice::<DictateOutcome>(body).ok()?;
    (outcome.ok && !outcome.transcript.is_empty()).then_some(outcome.transcript)
}

#[must_use]
pub fn voice_control_action(success: bool, body: &[u8]) -> Option<VoiceControlAction> {
    if !success {
        return None;
    }
    let outcome = serde_json::from_slice::<VoiceControlOutcome>(body).ok()?;
    if !outcome.ok {
        return None;
    }

    if let Some(argument) = outcome.launch_argument {
        return valid_settings_panel_argument(&argument)
            .then_some(VoiceControlAction::OpenSettings(argument));
    }

    (outcome.fall_through_to_dictation && !outcome.transcript.is_empty())
        .then_some(VoiceControlAction::TypeTranscript(outcome.transcript))
}

fn valid_settings_panel_argument(argument: &str) -> bool {
    let Some(panel) = argument.strip_prefix("--panel=") else {
        return false;
    };
    !panel.is_empty()
        && panel.len() <= MAX_PANEL_ID_BYTES
        && panel.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_text_is_optional_and_json_bounded_by_the_shared_transport() {
        assert_eq!(
            focus_log_text(br#"{"ok":true,"text":"Focus schedule tick made no changes."}"#),
            Some("Focus schedule tick made no changes.".to_string())
        );
        assert_eq!(focus_log_text(br#"{"ok":true,"text":""}"#), None);
        assert_eq!(focus_log_text(b"not-json"), None);
    }

    #[test]
    fn dictation_requires_a_successful_ok_response_and_nonempty_transcript() {
        let body = br#"{"ok":true,"transcript":"hello","text":"Transcribed."}"#;
        assert_eq!(dictation_transcript(true, body), Some("hello".to_string()));
        assert_eq!(dictation_transcript(false, body), None);
        assert_eq!(
            dictation_transcript(true, br#"{"ok":false,"transcript":"hello"}"#),
            None
        );
        assert_eq!(
            dictation_transcript(true, br#"{"ok":true,"transcript":""}"#),
            None
        );
    }

    #[test]
    fn voice_control_accepts_only_one_validated_action() {
        assert_eq!(
            voice_control_action(
                true,
                br#"{"ok":true,"transcript":"open settings","fall_through_to_dictation":false,"launch_argument":"--panel=updates-about"}"#,
            ),
            Some(VoiceControlAction::OpenSettings(
                "--panel=updates-about".to_string()
            ))
        );
        assert_eq!(
            voice_control_action(
                true,
                br#"{"ok":true,"transcript":"write this","fall_through_to_dictation":true,"launch_argument":null}"#,
            ),
            Some(VoiceControlAction::TypeTranscript("write this".to_string()))
        );
        for invalid in [
            "--help",
            "--panel=",
            "--panel=network --help",
            "--panel=../network",
            "--panel=Network",
            "--panel=network_advanced",
        ] {
            let body = format!(
                r#"{{"ok":true,"transcript":"discard","fall_through_to_dictation":true,"launch_argument":"{invalid}"}}"#
            );
            assert_eq!(
                voice_control_action(true, body.as_bytes()),
                None,
                "{invalid}"
            );
        }
    }
}
