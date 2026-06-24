//! Local, on-device voice for Goblins OS.
//!
//! Voice is assembled from local, offline-capable parts so it works the same
//! whether the engine is GPT-OSS or a bring-your-own key, and stays fully private
//! in offline mode: speech-to-text with a local Whisper runtime, the resident
//! model for the reply, text-to-speech with a local Piper voice, captured and
//! played through the OS audio stack (ALSA over PipeWire). The Whisper and Piper
//! models are weights, so — like GPT-OSS — they are never bundled in the image;
//! the OS reports what is present and what to add, and greys voice out until then.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use axum::{http::StatusCode, Json};
use serde::Serialize;

const DEFAULT_VOICE_DIR: &str = "/var/lib/goblins-os/voice";
const CAPTURE_SECONDS: &str = "6";
const VOICE_WAKE_WORD: &str = "Goblin";
const VOICE_WAKE_PHRASES: &[&str] = &["Goblin", "Hey Goblin"];
const VOICE_WAKE_LISTENER_DETAIL: &str = "Press the voice button, then say Goblin. Background wake listening is not ready until the local wake-word listener is available.";

#[derive(Serialize)]
pub struct VoiceStatus {
    source: &'static str,
    available: bool,
    offline_safe: bool,
    wake_word: &'static str,
    wake_phrases: &'static [&'static str],
    wake_listening: Capability,
    speech_to_text: Capability,
    text_to_speech: Capability,
    capture: Capability,
    playback: Capability,
    detail: String,
}

#[derive(Serialize)]
struct Capability {
    ready: bool,
    component: String,
    detail: String,
}

#[derive(Serialize)]
pub struct ConverseOutcome {
    ok: bool,
    transcript: String,
    reply: String,
    text: String,
}

pub async fn voice_status() -> Json<VoiceStatus> {
    Json(build_status())
}

pub async fn voice_converse() -> (StatusCode, Json<ConverseOutcome>) {
    match run_converse() {
        Ok((transcript, reply)) => (
            StatusCode::OK,
            Json(ConverseOutcome {
                ok: true,
                text: "Heard you, replied out loud.".to_string(),
                transcript,
                reply,
            }),
        ),
        Err(detail) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ConverseOutcome {
                ok: false,
                transcript: String::new(),
                reply: String::new(),
                text: detail,
            }),
        ),
    }
}

fn build_status() -> VoiceStatus {
    let stt = stt_capability();
    let tts = tts_capability();
    let capture = binary_capability(&capture_bin(), "Microphone capture (arecord)");
    let playback = binary_capability(&playback_bin(), "Audio playback (aplay)");
    let available = stt.ready && tts.ready && capture.ready && playback.ready;

    VoiceStatus {
        source: "goblins-os-core",
        available,
        // Every part is local, so voice never needs the network — it is safe in
        // offline / private mode.
        offline_safe: true,
        wake_word: VOICE_WAKE_WORD,
        wake_phrases: VOICE_WAKE_PHRASES,
        wake_listening: wake_listening_capability(),
        detail: if available {
            "Goblin voice is ready. Press the voice button, say Goblin, and it answers out loud on this device. Background wake listening is not ready until the local wake-word listener is available."
                .to_string()
        } else {
            "Goblin voice runs on local Whisper and Piper models. Add the missing voice components; background wake listening stays off until the local wake-word listener is available.".to_string()
        },
        speech_to_text: stt,
        text_to_speech: tts,
        capture,
        playback,
    }
}

fn wake_listening_capability() -> Capability {
    Capability {
        ready: false,
        component: "local wake-word listener".to_string(),
        detail: VOICE_WAKE_LISTENER_DETAIL.to_string(),
    }
}

fn stt_capability() -> Capability {
    let binary = whisper_bin();
    if !binary_present(&binary) {
        return Capability {
            ready: false,
            component: binary,
            detail: "Local Whisper runtime not found.".to_string(),
        };
    }
    match first_model(&stt_dir(), &["bin", "gguf", "ggml"]) {
        Some(model) => Capability {
            ready: true,
            component: binary,
            detail: format!("Speech-to-text ready with {}.", model.display()),
        },
        None => Capability {
            ready: false,
            component: binary,
            detail: format!(
                "No Whisper model in {} — add one to enable speech-to-text.",
                stt_dir().display()
            ),
        },
    }
}

fn tts_capability() -> Capability {
    let binary = piper_bin();
    if !binary_present(&binary) {
        return Capability {
            ready: false,
            component: binary,
            detail: "Local Piper runtime not found.".to_string(),
        };
    }
    match first_model(&tts_dir(), &["onnx"]) {
        Some(model) => Capability {
            ready: true,
            component: binary,
            detail: format!("Text-to-speech ready with {}.", model.display()),
        },
        None => Capability {
            ready: false,
            component: binary,
            detail: format!(
                "No Piper voice in {} — add one to enable text-to-speech.",
                tts_dir().display()
            ),
        },
    }
}

fn binary_capability(binary: &str, label: &str) -> Capability {
    Capability {
        ready: binary_present(binary),
        component: binary.to_string(),
        detail: if binary_present(binary) {
            format!("{label} is available.")
        } else {
            format!("{label} is not ready.")
        },
    }
}

/// The full local voice loop: capture the microphone, transcribe with Whisper,
/// answer with the Goblins AI runtime (GPT-OSS or the user's key), synthesize the
/// reply with Piper, and play it. Every step degrades to a clear message.
fn run_converse() -> Result<(String, String), String> {
    let stt = stt_capability();
    let tts = tts_capability();
    if !stt.ready {
        return Err(stt.detail);
    }
    if !tts.ready {
        return Err(tts.detail);
    }
    if !binary_present(&capture_bin()) {
        return Err("Microphone capture is not ready on this device.".to_string());
    }

    let work = work_dir();
    fs::create_dir_all(&work)
        .map_err(|_| "Could not open the voice work directory.".to_string())?;
    let input = work.join("input.wav");
    let reply_wav = work.join("reply.wav");

    record_audio(&input)?;
    let transcript = transcribe(&input)?;
    if transcript.is_empty() {
        return Err("Goblins OS didn’t catch that — try again.".to_string());
    }

    let reply = crate::resident::resident_generate(&transcript)
        .map_err(|detail| format!("The on-device model could not answer: {detail}."))?;

    synthesize(&reply, &reply_wav)?;
    play_audio(&reply_wav);

    Ok((transcript, reply))
}

fn record_audio(path: &Path) -> Result<(), String> {
    // 16 kHz mono PCM is what Whisper expects; a fixed window keeps the loop
    // simple and predictable without a voice-activity detector.
    let status = Command::new(capture_bin())
        .args([
            "-q",
            "-d",
            CAPTURE_SECONDS,
            "-f",
            "S16_LE",
            "-r",
            "16000",
            "-c",
            "1",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .status()
        .map_err(|_| "Microphone capture could not start.".to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Microphone capture failed.".to_string())
    }
}

fn transcribe(input: &Path) -> Result<String, String> {
    let model = first_model(&stt_dir(), &["bin", "gguf", "ggml"])
        .ok_or_else(|| "No Whisper model is installed.".to_string())?;
    let prefix = input.with_extension("");
    let output = Command::new(whisper_bin())
        .args(["-m"])
        .arg(&model)
        .args(["-f"])
        .arg(input)
        .args(["-otxt", "-nt", "-of"])
        .arg(&prefix)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "The Whisper runtime could not start.".to_string())?;
    if !output.status.success() {
        return Err("Speech-to-text failed.".to_string());
    }
    let transcript_path = prefix.with_extension("txt");
    let text = fs::read_to_string(&transcript_path).unwrap_or_default();
    Ok(text.trim().to_string())
}

fn synthesize(text: &str, output: &Path) -> Result<(), String> {
    use std::io::Write;

    let voice = first_model(&tts_dir(), &["onnx"])
        .ok_or_else(|| "No Piper voice is installed.".to_string())?;
    let mut child = Command::new(piper_bin())
        .args(["-q", "-m"])
        .arg(&voice)
        .args(["-f"])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "The Piper runtime could not start.".to_string())?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|_| "Text-to-speech input failed.".to_string())?;
    }
    let status = child
        .wait()
        .map_err(|_| "Text-to-speech failed.".to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Text-to-speech failed.".to_string())
    }
}

fn play_audio(path: &Path) {
    let _ = Command::new(playback_bin())
        .arg("-q")
        .arg(path)
        .stdin(Stdio::null())
        .status();
}

fn binary_present(binary: &str) -> bool {
    if binary.contains('/') {
        return Path::new(binary).exists();
    }
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

/// The first model file in `dir` whose extension is one of `extensions`, chosen
/// deterministically (sorted) so the same model is used across calls.
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

fn voice_dir() -> PathBuf {
    env::var("GOBLINS_OS_VOICE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_VOICE_DIR).to_path_buf())
}

fn stt_dir() -> PathBuf {
    voice_dir().join("stt")
}

fn tts_dir() -> PathBuf {
    voice_dir().join("tts")
}

fn work_dir() -> PathBuf {
    voice_dir().join("work")
}

fn whisper_bin() -> String {
    env::var("GOBLINS_OS_WHISPER_BIN").unwrap_or_else(|_| "whisper-cli".to_string())
}

fn piper_bin() -> String {
    env::var("GOBLINS_OS_PIPER_BIN").unwrap_or_else(|_| "piper".to_string())
}

fn capture_bin() -> String {
    env::var("GOBLINS_OS_AUDIO_CAPTURE_BIN").unwrap_or_else(|_| "arecord".to_string())
}

fn playback_bin() -> String {
    env::var("GOBLINS_OS_AUDIO_PLAYBACK_BIN").unwrap_or_else(|_| "aplay".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        binary_capability, first_model, Capability, VoiceStatus, VOICE_WAKE_LISTENER_DETAIL,
        VOICE_WAKE_PHRASES, VOICE_WAKE_WORD,
    };
    use std::path::PathBuf;

    fn unique_tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    #[test]
    fn first_model_picks_a_matching_file_deterministically() {
        let dir = unique_tmp("goblins-os-voice-stt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // No models yet => none.
        assert_eq!(first_model(&dir, &["bin", "gguf"]), None);

        std::fs::write(dir.join("readme.txt"), b"x").unwrap();
        std::fs::write(dir.join("b-model.gguf"), b"x").unwrap();
        std::fs::write(dir.join("a-model.bin"), b"x").unwrap();

        // Wrong extension is ignored; the alphabetically-first match is chosen.
        let chosen = first_model(&dir, &["bin", "gguf"]).unwrap();
        assert_eq!(chosen.file_name().unwrap(), "a-model.bin");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_serializes_for_the_native_ui() {
        let status = VoiceStatus {
            source: "goblins-os-core",
            available: false,
            offline_safe: true,
            wake_word: VOICE_WAKE_WORD,
            wake_phrases: VOICE_WAKE_PHRASES,
            wake_listening: Capability {
                ready: false,
                component: "local wake-word listener".to_string(),
                detail: VOICE_WAKE_LISTENER_DETAIL.to_string(),
            },
            speech_to_text: Capability {
                ready: false,
                component: "whisper-cli".to_string(),
                detail: "missing".to_string(),
            },
            text_to_speech: Capability {
                ready: false,
                component: "piper".to_string(),
                detail: "missing".to_string(),
            },
            capture: Capability {
                ready: true,
                component: "arecord".to_string(),
                detail: "ok".to_string(),
            },
            playback: Capability {
                ready: true,
                component: "aplay".to_string(),
                detail: "ok".to_string(),
            },
            detail: "add models".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"available\":false"));
        assert!(json.contains("\"offline_safe\":true"));
        assert!(json.contains("\"wake_word\":\"Goblin\""));
        assert!(json.contains("Hey Goblin"));
        assert!(json.contains("wake_listening"));
        assert!(json.contains("speech_to_text"));
    }

    #[test]
    fn wake_word_copy_is_goblins_native_and_truthful() {
        let source = include_str!("voice.rs");

        assert!(source.contains("VOICE_WAKE_WORD: &str = \"Goblin\""));
        assert!(source.contains("\"Hey Goblin\""));
        assert!(source.contains("Background wake listening is not ready"));
        let apple_assistant = ["si", "ri"].join("");
        let passive_claim = ["always", " listening"].join("");
        let lower = source.to_ascii_lowercase();
        assert!(!lower.contains(&apple_assistant));
        assert!(!lower.contains(&passive_claim));
    }

    #[test]
    fn missing_voice_components_use_not_ready_copy() {
        let capability = binary_capability("goblins-os-test-missing-binary", "Microphone capture");

        assert!(!capability.ready);
        assert_eq!(capability.detail, "Microphone capture is not ready.");
    }
}
