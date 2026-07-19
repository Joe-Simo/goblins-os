//! Live Captions substrate (caption config + STT capability status).
//!
//! Live Captions is a local accessibility surface: capture audio, chunk it on
//! silence, transcribe with a local Whisper model, and stream text to a shell
//! overlay. The overlay/streaming engine is CI/qemu work; this module ships the
//! host-testable substrate first: caption preferences, STT/model/capture gates,
//! and the pure audio helpers the engine will use. It never reports live captions
//! as active when the model, runtime, PipeWire tools, capture command, or schema is
//! missing.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use serde_json::Value;

use crate::bounded::{bounded_session_command_output, probe_timeout};

const SCHEMA: &str = "org.goblins.shell.extensions.captions";
const DEFAULT_MODEL_DIR: &str = "/var/lib/goblins-os/voice/stt";
const MODEL_DIR_ENV: &str = "GOBLINS_OS_LIVE_CAPTIONS_MODEL_DIR";
const VOICE_DIR_ENV: &str = "GOBLINS_OS_VOICE_DIR";
const WHISPER_BIN_ENV: &str = "GOBLINS_OS_WHISPER_BIN";
const CAPTURE_BIN_ENV: &str = "GOBLINS_OS_LIVE_CAPTIONS_CAPTURE_BIN";
const DEFAULT_WHISPER_BIN: &str = "whisper-cli";
const DEFAULT_CAPTURE_BIN: &str = "pw-record";
const DEFAULT_SILENCE_RMS: f32 = 450.0;
const DEFAULT_MIN_SEGMENT_MS: u64 = 500;
const DEFAULT_MAX_SEGMENT_MS: u64 = 2_000;
const DEFAULT_TRAILING_SILENCE_MS: u64 = 450;
const CAPTURE_TARGET_PLACEHOLDER: &str = "<pipewire-monitor-target>";
const CAPTION_CHUNK_PLACEHOLDER: &str = "/run/user/UID/goblins-os/live-captions/chunk.wav";

#[derive(Serialize)]
pub struct LiveCaptionsStatus {
    source: &'static str,
    schema_available: bool,
    enabled: bool,
    available: bool,
    active: bool,
    offline_safe: bool,
    audio_source: &'static str,
    text_size: &'static str,
    position: &'static str,
    auto_hide: bool,
    keep_onscreen: bool,
    stt_runtime: Capability,
    stt_model: Capability,
    pipewire: Capability,
    capture: Capability,
    capture_plan: CaptionCapturePlan,
    segment: SegmentConfig,
    detail: String,
}

#[derive(Serialize)]
struct Capability {
    ready: bool,
    component: String,
    detail: String,
}

#[derive(Serialize)]
struct SegmentConfig {
    silence_rms: f32,
    min_segment_ms: u64,
    max_segment_ms: u64,
    trailing_silence_ms: u64,
}

#[derive(Serialize)]
struct CaptionCapturePlan {
    source: &'static str,
    monitor_target: Option<String>,
    capture_command: Vec<String>,
    runtime_ready_claim: bool,
    capture_runtime_ready: bool,
    transcription_ready_claim: bool,
    detail: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct PipeWireMonitorTarget {
    target: String,
    description: String,
}

pub async fn live_captions_status() -> Json<LiveCaptionsStatus> {
    Json(build_status())
}

pub async fn live_captions_stream() -> impl IntoResponse {
    let status = build_status();
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        caption_stream_event(&status),
    )
}

fn caption_stream_event(status: &LiveCaptionsStatus) -> String {
    let event = if status.active {
        "caption-ready"
    } else {
        "caption-status"
    };
    let data = serde_json::to_string(status).unwrap_or_else(|_| {
        "{\"active\":false,\"detail\":\"Live Captions status could not be serialized.\"}"
            .to_string()
    });
    format!("event: {event}\ndata: {data}\n\n")
}

fn build_status() -> LiveCaptionsStatus {
    let schema_available = schema_available(SCHEMA);
    let enabled = schema_available && get_bool("enabled").unwrap_or(false);
    let audio_source = if schema_available {
        normalize_audio_source(get_string("source").as_deref().unwrap_or("system"))
    } else {
        "system"
    };
    let text_size = if schema_available {
        normalize_text_size(get_string("text-size").as_deref().unwrap_or("medium"))
    } else {
        "medium"
    };
    let position = if schema_available {
        normalize_position(get_string("position").as_deref().unwrap_or("bottom"))
    } else {
        "bottom"
    };
    let auto_hide = !schema_available || get_bool("auto-hide").unwrap_or(true);
    let keep_onscreen = !schema_available || get_bool("keep-onscreen").unwrap_or(true);

    let stt_runtime = stt_runtime_capability();
    let stt_model = stt_model_capability();
    let pipewire = pipewire_capability();
    let capture = capture_capability();
    let capture_plan = pending_caption_capture_plan(&capture.component);
    let available = stt_runtime.ready && stt_model.ready && pipewire.ready && capture.ready;
    let active = enabled && available;

    LiveCaptionsStatus {
        source: "goblins-os-core",
        schema_available,
        enabled,
        available,
        active,
        offline_safe: true,
        audio_source,
        text_size,
        position,
        auto_hide,
        keep_onscreen,
        stt_runtime,
        stt_model,
        pipewire,
        capture,
        capture_plan,
        segment: SegmentConfig {
            silence_rms: DEFAULT_SILENCE_RMS,
            min_segment_ms: DEFAULT_MIN_SEGMENT_MS,
            max_segment_ms: DEFAULT_MAX_SEGMENT_MS,
            trailing_silence_ms: DEFAULT_TRAILING_SILENCE_MS,
        },
        detail: status_detail(schema_available, enabled, available),
    }
}

fn status_detail(schema_available: bool, enabled: bool, available: bool) -> String {
    if !schema_available {
        return "Live Captions is unavailable here (its preferences schema is not installed)."
            .to_string();
    }
    if !enabled {
        return "Live Captions is off. Captions stay local and start only after you turn them on."
            .to_string();
    }
    if !available {
        return "Add a speech model to turn on Live Captions, and make sure PipeWire system-audio capture is ready. Capture planning is source-gated; no live captions start until an installed session proves the monitor target and transcription loop.".to_string();
    }
    "Live Captions is ready to caption local audio on this device.".to_string()
}

fn normalize_audio_source(value: &str) -> &'static str {
    match value.trim() {
        "microphone" => "microphone",
        "both" => "both",
        _ => "system",
    }
}

fn normalize_text_size(value: &str) -> &'static str {
    match value.trim() {
        "small" => "small",
        "large" => "large",
        _ => "medium",
    }
}

fn normalize_position(value: &str) -> &'static str {
    match value.trim() {
        "top" => "top",
        "floating" => "floating",
        _ => "bottom",
    }
}

fn stt_runtime_capability() -> Capability {
    let binary = whisper_bin();
    let ready = binary_present(&binary);
    Capability {
        ready,
        component: binary.clone(),
        detail: if ready {
            "Local Whisper runtime is available.".to_string()
        } else {
            "Local Whisper runtime not found.".to_string()
        },
    }
}

fn stt_model_capability() -> Capability {
    match first_model(&model_dir(), &["bin", "gguf", "ggml"]) {
        Some(model) => Capability {
            ready: true,
            component: model.display().to_string(),
            detail: format!("Speech model ready at {}.", model.display()),
        },
        None => Capability {
            ready: false,
            component: model_dir().display().to_string(),
            detail: format!(
                "No Whisper model in {} — add a speech model to enable Live Captions.",
                model_dir().display()
            ),
        },
    }
}

fn pipewire_capability() -> Capability {
    let ready = binary_present("wpctl") && binary_present("pw-cli") && binary_present("pw-dump");
    Capability {
        ready,
        component: "PipeWire monitor source".to_string(),
        detail: if ready {
            "PipeWire control tools are available for monitor-source planning.".to_string()
        } else {
            "PipeWire audio routing is not ready in this session.".to_string()
        },
    }
}

fn capture_capability() -> Capability {
    let binary = capture_bin();
    let command_present = binary_present(&binary);
    Capability {
        ready: false,
        component: binary.clone(),
        detail: if command_present {
            "System audio capture command is installed; live capture remains disabled until a PipeWire monitor target is proven in CI/qemu.".to_string()
        } else {
            "System audio capture is not ready on this device.".to_string()
        },
    }
}

fn pending_caption_capture_plan(capture_binary: &str) -> CaptionCapturePlan {
    caption_capture_plan_from_pw_dump_with_binary(
        capture_binary,
        "",
        Path::new(CAPTION_CHUNK_PLACEHOLDER),
    )
}

#[cfg(test)]
fn caption_capture_plan_from_pw_dump(pw_dump_json: &str, output: &Path) -> CaptionCapturePlan {
    caption_capture_plan_from_pw_dump_with_binary(DEFAULT_CAPTURE_BIN, pw_dump_json, output)
}

fn caption_capture_plan_from_pw_dump_with_binary(
    capture_binary: &str,
    pw_dump_json: &str,
    output: &Path,
) -> CaptionCapturePlan {
    let target = pipewire_monitor_targets_from_dump(pw_dump_json)
        .into_iter()
        .next();
    let monitor_target = target.map(|target| target.target);
    let target_arg = monitor_target
        .as_deref()
        .unwrap_or(CAPTURE_TARGET_PLACEHOLDER);
    CaptionCapturePlan {
        source: "pipewire-default-sink-monitor",
        monitor_target: monitor_target.clone(),
        capture_command: caption_capture_args(capture_binary, target_arg, output),
        runtime_ready_claim: false,
        capture_runtime_ready: false,
        transcription_ready_claim: false,
        detail: if monitor_target.is_some() {
            "PipeWire monitor target can be resolved from pw-dump; live capture and transcription remain CI/qemu-pending.".to_string()
        } else {
            "No PipeWire monitor target was resolved from pw-dump; Live Captions stays waiting."
                .to_string()
        },
    }
}

fn pipewire_monitor_targets_from_dump(pw_dump_json: &str) -> Vec<PipeWireMonitorTarget> {
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(pw_dump_json) else {
        return Vec::new();
    };

    let mut targets: Vec<PipeWireMonitorTarget> = entries
        .iter()
        .filter_map(pipewire_monitor_target_from_entry)
        .collect();
    targets.sort_by(|left, right| {
        left.description
            .cmp(&right.description)
            .then(left.target.cmp(&right.target))
    });
    targets.dedup_by(|left, right| left.target == right.target);
    targets
}

fn pipewire_monitor_target_from_entry(entry: &Value) -> Option<PipeWireMonitorTarget> {
    let props = entry.get("info")?.get("props")?;
    if prop_str(props, "media.class")? != "Audio/Source" {
        return None;
    }
    let node_name = prop_str(props, "node.name")?;
    let description = prop_str(props, "node.description").unwrap_or(node_name);
    let name_is_monitor = node_name.ends_with(".monitor");
    let description_is_monitor = description.to_ascii_lowercase().contains("monitor");
    if !name_is_monitor && !description_is_monitor {
        return None;
    }
    Some(PipeWireMonitorTarget {
        target: node_name.to_string(),
        description: description.to_string(),
    })
}

fn prop_str<'a>(props: &'a Value, key: &str) -> Option<&'a str> {
    props.get(key)?.as_str()
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
    if let Ok(dir) = env::var(MODEL_DIR_ENV) {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = env::var(VOICE_DIR_ENV) {
        return Path::new(&dir).join("stt");
    }
    Path::new(DEFAULT_MODEL_DIR).to_path_buf()
}

fn whisper_bin() -> String {
    env::var(WHISPER_BIN_ENV).unwrap_or_else(|_| DEFAULT_WHISPER_BIN.to_string())
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

/// Build the Whisper CLI argv for a completed caption chunk. Pure + unit-tested so
/// the engine invokes the local STT runtime consistently.
#[cfg(test)]
fn whisper_caption_args(model: &Path, input: &Path, output_prefix: &Path) -> Vec<String> {
    vec![
        "-m".to_string(),
        model.to_string_lossy().to_string(),
        "-f".to_string(),
        input.to_string_lossy().to_string(),
        "-otxt".to_string(),
        "-nt".to_string(),
        "-of".to_string(),
        output_prefix.to_string_lossy().to_string(),
    ]
}

fn caption_capture_args(capture_binary: &str, target: &str, output: &Path) -> Vec<String> {
    vec![
        capture_binary.to_string(),
        "--target".to_string(),
        target.to_string(),
        "--rate".to_string(),
        "16000".to_string(),
        "--channels".to_string(),
        "1".to_string(),
        "--format".to_string(),
        "s16".to_string(),
        output.to_string_lossy().to_string(),
    ]
}

#[cfg(test)]
fn rms_i16(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f64 = samples
        .iter()
        .map(|sample| {
            let value = f64::from(*sample);
            value * value
        })
        .sum();
    (sum_squares / samples.len() as f64).sqrt() as f32
}

#[cfg(test)]
fn is_silence(samples: &[i16], threshold: f32) -> bool {
    rms_i16(samples) <= threshold
}

#[cfg(test)]
fn segment_should_flush(
    audio_ms: u64,
    trailing_silence_ms: u64,
    min_segment_ms: u64,
    max_segment_ms: u64,
) -> bool {
    audio_ms >= max_segment_ms
        || (audio_ms >= min_segment_ms && trailing_silence_ms >= DEFAULT_TRAILING_SILENCE_MS)
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
        caption_capture_args, caption_capture_plan_from_pw_dump, caption_stream_event, is_silence,
        normalize_audio_source, normalize_position, normalize_text_size,
        pipewire_monitor_targets_from_dump, rms_i16, segment_should_flush, whisper_caption_args,
        Capability, CaptionCapturePlan, LiveCaptionsStatus, SegmentConfig, DEFAULT_MAX_SEGMENT_MS,
        DEFAULT_MIN_SEGMENT_MS, DEFAULT_SILENCE_RMS, DEFAULT_TRAILING_SILENCE_MS,
    };
    use std::path::Path;

    #[test]
    fn normalizes_caption_config_to_known_values() {
        assert_eq!(normalize_audio_source("microphone"), "microphone");
        assert_eq!(normalize_audio_source("both"), "both");
        assert_eq!(normalize_audio_source("display"), "system");
        assert_eq!(normalize_text_size("small"), "small");
        assert_eq!(normalize_text_size("large"), "large");
        assert_eq!(normalize_text_size("huge"), "medium");
        assert_eq!(normalize_position("top"), "top");
        assert_eq!(normalize_position("floating"), "floating");
        assert_eq!(normalize_position("center"), "bottom");
    }

    #[test]
    fn builds_stable_whisper_argv() {
        assert_eq!(
            whisper_caption_args(
                Path::new("/models/base.gguf"),
                Path::new("/tmp/chunk.wav"),
                Path::new("/tmp/chunk")
            ),
            vec![
                "-m",
                "/models/base.gguf",
                "-f",
                "/tmp/chunk.wav",
                "-otxt",
                "-nt",
                "-of",
                "/tmp/chunk",
            ]
        );
    }

    #[test]
    fn builds_stable_pipewire_capture_argv() {
        assert_eq!(
            caption_capture_args(
                "pw-record",
                "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
                Path::new("/run/user/1000/goblins-os/live-captions/chunk.wav"),
            ),
            vec![
                "pw-record",
                "--target",
                "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "s16",
                "/run/user/1000/goblins-os/live-captions/chunk.wav",
            ]
        );
    }

    #[test]
    fn parses_pipewire_monitor_targets_from_pw_dump() {
        let dump = r#"[
            {
                "id": 41,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "props": {
                        "media.class": "Audio/Sink",
                        "node.name": "alsa_output.pci-0000_00_1f.3.analog-stereo",
                        "node.description": "Built-in Audio Analog Stereo"
                    }
                }
            },
            {
                "id": 42,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "props": {
                        "media.class": "Audio/Source",
                        "node.name": "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
                        "node.description": "Monitor of Built-in Audio Analog Stereo"
                    }
                }
            },
            {
                "id": 43,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "props": {
                        "media.class": "Audio/Source",
                        "node.name": "alsa_input.pci-0000_00_1f.3.analog-stereo",
                        "node.description": "Built-in Microphone"
                    }
                }
            }
        ]"#;
        let targets = pipewire_monitor_targets_from_dump(dump);
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].target,
            "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor"
        );
    }

    #[test]
    fn capture_plan_resolves_target_but_keeps_live_claims_false() {
        let dump = r#"[{
            "id": 51,
            "type": "PipeWire:Interface:Node",
            "info": {
                "props": {
                    "media.class": "Audio/Source",
                    "node.name": "bluez_output.00_11_22_33_44_55.a2dp-sink.monitor",
                    "node.description": "Monitor of Headphones"
                }
            }
        }]"#;
        let plan = caption_capture_plan_from_pw_dump(
            dump,
            Path::new("/run/user/1000/goblins-os/live-captions/chunk.wav"),
        );
        assert_eq!(
            plan.monitor_target.as_deref(),
            Some("bluez_output.00_11_22_33_44_55.a2dp-sink.monitor")
        );
        assert!(!plan.runtime_ready_claim);
        assert!(!plan.capture_runtime_ready);
        assert!(!plan.transcription_ready_claim);
        assert!(plan.detail.contains("CI/qemu-pending"));
    }

    #[test]
    fn rms_and_silence_detection_are_stable() {
        assert_eq!(rms_i16(&[]), 0.0);
        assert!(is_silence(&[0, 12, -12], DEFAULT_SILENCE_RMS));
        assert!(!is_silence(&[3000, -3000], DEFAULT_SILENCE_RMS));
    }

    #[test]
    fn segment_flushes_on_silence_or_max_duration() {
        assert!(!segment_should_flush(
            400,
            DEFAULT_TRAILING_SILENCE_MS,
            DEFAULT_MIN_SEGMENT_MS,
            DEFAULT_MAX_SEGMENT_MS
        ));
        assert!(segment_should_flush(
            DEFAULT_MIN_SEGMENT_MS,
            DEFAULT_TRAILING_SILENCE_MS,
            DEFAULT_MIN_SEGMENT_MS,
            DEFAULT_MAX_SEGMENT_MS
        ));
        assert!(segment_should_flush(
            DEFAULT_MAX_SEGMENT_MS,
            0,
            DEFAULT_MIN_SEGMENT_MS,
            DEFAULT_MAX_SEGMENT_MS
        ));
    }

    #[test]
    fn status_serializes_for_the_shell_overlay() {
        let status = LiveCaptionsStatus {
            source: "goblins-os-core",
            schema_available: true,
            enabled: false,
            available: false,
            active: false,
            offline_safe: true,
            audio_source: "system",
            text_size: "medium",
            position: "bottom",
            auto_hide: true,
            keep_onscreen: true,
            stt_runtime: Capability {
                ready: false,
                component: "whisper-cli".to_string(),
                detail: "missing".to_string(),
            },
            stt_model: Capability {
                ready: false,
                component: "/var/lib/goblins-os/voice/stt".to_string(),
                detail: "missing".to_string(),
            },
            pipewire: Capability {
                ready: false,
                component: "PipeWire monitor source".to_string(),
                detail: "missing".to_string(),
            },
            capture: Capability {
                ready: false,
                component: "pw-record".to_string(),
                detail: "missing".to_string(),
            },
            capture_plan: CaptionCapturePlan {
                source: "pipewire-default-sink-monitor",
                monitor_target: None,
                capture_command: caption_capture_args(
                    "pw-record",
                    "<pipewire-monitor-target>",
                    Path::new("/run/user/UID/goblins-os/live-captions/chunk.wav"),
                ),
                runtime_ready_claim: false,
                capture_runtime_ready: false,
                transcription_ready_claim: false,
                detail: "Live Captions has a deterministic PipeWire capture plan, but no live monitor target, capture stream, or transcription loop is claimed yet.".to_string(),
            },
            segment: SegmentConfig {
                silence_rms: DEFAULT_SILENCE_RMS,
                min_segment_ms: DEFAULT_MIN_SEGMENT_MS,
                max_segment_ms: DEFAULT_MAX_SEGMENT_MS,
                trailing_silence_ms: DEFAULT_TRAILING_SILENCE_MS,
            },
            detail: "Live Captions is off.".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"offline_safe\":true"));
        assert!(json.contains("\"audio_source\":\"system\""));
        assert!(json.contains("\"stt_model\""));
        assert!(json.contains("\"position\":\"bottom\""));
        assert!(json.contains("\"capture_runtime_ready\":false"));
        assert!(json.contains("\"transcription_ready_claim\":false"));
    }

    #[test]
    fn stream_event_reports_status_without_fake_captions() {
        let status = LiveCaptionsStatus {
            source: "goblins-os-core",
            schema_available: true,
            enabled: false,
            available: false,
            active: false,
            offline_safe: true,
            audio_source: "system",
            text_size: "medium",
            position: "bottom",
            auto_hide: true,
            keep_onscreen: true,
            stt_runtime: Capability {
                ready: false,
                component: "whisper-cli".to_string(),
                detail: "missing".to_string(),
            },
            stt_model: Capability {
                ready: false,
                component: "/var/lib/goblins-os/voice/stt".to_string(),
                detail: "missing".to_string(),
            },
            pipewire: Capability {
                ready: false,
                component: "PipeWire monitor source".to_string(),
                detail: "missing".to_string(),
            },
            capture: Capability {
                ready: false,
                component: "pw-record".to_string(),
                detail: "missing".to_string(),
            },
            capture_plan: CaptionCapturePlan {
                source: "pipewire-default-sink-monitor",
                monitor_target: None,
                capture_command: caption_capture_args(
                    "pw-record",
                    "<pipewire-monitor-target>",
                    Path::new("/run/user/UID/goblins-os/live-captions/chunk.wav"),
                ),
                runtime_ready_claim: false,
                capture_runtime_ready: false,
                transcription_ready_claim: false,
                detail: "Live Captions has a deterministic PipeWire capture plan, but no live monitor target, capture stream, or transcription loop is claimed yet.".to_string(),
            },
            segment: SegmentConfig {
                silence_rms: DEFAULT_SILENCE_RMS,
                min_segment_ms: DEFAULT_MIN_SEGMENT_MS,
                max_segment_ms: DEFAULT_MAX_SEGMENT_MS,
                trailing_silence_ms: DEFAULT_TRAILING_SILENCE_MS,
            },
            detail: "Live Captions is off.".to_string(),
        };
        let event = caption_stream_event(&status);
        assert!(event.starts_with("event: caption-status\n"));
        assert!(event.contains("\"active\":false"));
        assert!(event.contains("\"runtime_ready_claim\":false"));
        assert!(!event.contains("captioned speech"));
    }
}
