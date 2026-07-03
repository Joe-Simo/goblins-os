//! PipeWire/WirePlumber audio controls for Settings.
//!
//! The core owns `wpctl` access so UI processes do not shell out directly.
//! Device selection is constrained to sink/source IDs reported by WirePlumber,
//! so Settings can choose defaults without accepting arbitrary object names.

use std::{collections::BTreeMap, time::Duration};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::{bounded_command_output, BoundedCommandError};
use crate::session_bridge::{self, SessionBridgeResult};

const DEFAULT_SINK: &str = "@DEFAULT_AUDIO_SINK@";
const DEFAULT_SOURCE: &str = "@DEFAULT_AUDIO_SOURCE@";
const SOUND_SCHEMA: &str = "org.gnome.desktop.sound";
const WPCTL_TIMEOUT_MS_DEFAULT: u64 = 500;
const WPCTL_TIMEOUT_MS_MIN: u64 = 100;
const WPCTL_TIMEOUT_MS_MAX: u64 = 5_000;
const GSETTINGS_TIMEOUT_MS: u64 = 1_500;

#[derive(Serialize)]
pub struct AudioStatus {
    source: &'static str,
    wireplumber_available: bool,
    output: AudioEndpointStatus,
    input: AudioEndpointStatus,
    sound: SoundPreferencesStatus,
    detail: String,
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct AudioEndpointStatus {
    available: bool,
    volume_percent: Option<u8>,
    muted: Option<bool>,
    default_device_id: Option<String>,
    devices: Vec<AudioDeviceStatus>,
    detail: String,
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct AudioDeviceStatus {
    id: String,
    name: String,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    volume_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    muted: Option<bool>,
}

#[derive(Deserialize)]
pub struct AudioVolumeRequest {
    target: String,
    volume_percent: u8,
}

#[derive(Deserialize)]
pub struct AudioMuteRequest {
    target: String,
    muted: bool,
}

#[derive(Deserialize)]
pub struct AudioDefaultDeviceRequest {
    target: String,
    device_id: String,
}

#[derive(Serialize)]
pub struct AudioControlOutcome {
    ok: bool,
    target: String,
    text: String,
    volume_percent: Option<u8>,
    muted: Option<bool>,
}

#[derive(Serialize)]
pub struct SoundPreferencesStatus {
    gsettings_available: bool,
    schema_available: bool,
    event_sounds: Option<bool>,
    input_feedback_sounds: Option<bool>,
    volume_boost: Option<bool>,
    theme_name: Option<String>,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetSoundPreferenceRequest {
    target: SoundPreferenceTarget,
    value: bool,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SoundPreferenceTarget {
    EventSounds,
    InputFeedbackSounds,
    VolumeBoost,
}

#[derive(Serialize)]
pub struct SoundPreferenceOutcome {
    ok: bool,
    target: &'static str,
    text: String,
}

pub async fn audio_status() -> Json<AudioStatus> {
    Json(build_audio_status())
}

pub async fn set_audio_volume(
    Json(request): Json<AudioVolumeRequest>,
) -> (StatusCode, Json<AudioControlOutcome>) {
    audio_volume_outcome(&request.target, request.volume_percent)
}

pub async fn set_audio_mute(
    Json(request): Json<AudioMuteRequest>,
) -> (StatusCode, Json<AudioControlOutcome>) {
    audio_mute_outcome(&request.target, request.muted)
}

pub async fn set_audio_default_device(
    Json(request): Json<AudioDefaultDeviceRequest>,
) -> (StatusCode, Json<AudioControlOutcome>) {
    audio_default_device_outcome(&request.target, &request.device_id)
}

pub async fn set_sound_preference(
    Json(request): Json<SetSoundPreferenceRequest>,
) -> (StatusCode, Json<SoundPreferenceOutcome>) {
    sound_preference_outcome(request)
}

fn build_audio_status() -> AudioStatus {
    let wireplumber_available = executable_exists("wpctl") || executable_exists("wireplumber");
    let output = audio_endpoint_default_volume_status(AudioTarget::Output);
    let input = audio_endpoint_default_volume_status(AudioTarget::Input);
    let sound = build_sound_preferences_status();
    let detail = audio_status_detail(wireplumber_available, &output, &input);

    AudioStatus {
        source: "goblins-os-core",
        wireplumber_available,
        output,
        input,
        sound,
        detail,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AudioTarget {
    Output,
    Input,
}

impl AudioTarget {
    fn from_request(value: &str) -> Option<Self> {
        match value.trim() {
            "output" | "sink" | "speaker" | "speakers" => Some(Self::Output),
            "input" | "source" | "microphone" | "mic" => Some(Self::Input),
            _ => None,
        }
    }

    fn request_name(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Input => "input",
        }
    }

    fn wpctl_id(self) -> &'static str {
        match self {
            Self::Output => DEFAULT_SINK,
            Self::Input => DEFAULT_SOURCE,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Output => "default output",
            Self::Input => "default input",
        }
    }
}

#[cfg(test)]
fn audio_endpoint_status(
    target: AudioTarget,
    devices: Vec<AudioDeviceStatus>,
) -> AudioEndpointStatus {
    let default_device_id = devices
        .iter()
        .find(|device| device.active)
        .map(|device| device.id.clone());
    let default_device = default_device_id
        .as_deref()
        .and_then(|device_id| devices.iter().find(|device| device.id == device_id))
        .or_else(|| devices.iter().find(|device| device.active));

    if let Some(device) = default_device {
        let detail = match (device.volume_percent, device.muted) {
            (Some(volume_percent), Some(muted)) => {
                audio_endpoint_detail(target, volume_percent, muted)
            }
            _ => audio_endpoint_ready_without_volume_detail(target),
        };

        return AudioEndpointStatus {
            available: true,
            volume_percent: device.volume_percent,
            muted: device.muted,
            default_device_id,
            devices,
            detail,
        };
    }

    if !devices.is_empty() {
        return AudioEndpointStatus {
            available: true,
            volume_percent: None,
            muted: None,
            default_device_id,
            devices,
            detail: format!(
                "WirePlumber reported {} devices, but no default {} was marked.",
                target.label(),
                target.request_name()
            ),
        };
    }

    AudioEndpointStatus {
        available: false,
        volume_percent: None,
        muted: None,
        default_device_id,
        devices,
        detail: format!("WirePlumber did not report the {}.", target.label()),
    }
}

fn audio_devices(target: AudioTarget) -> Vec<AudioDeviceStatus> {
    let (output, input) = audio_device_snapshot();
    match target {
        AudioTarget::Output => output,
        AudioTarget::Input => input,
    }
}

fn audio_device_snapshot() -> (Vec<AudioDeviceStatus>, Vec<AudioDeviceStatus>) {
    match wpctl(&["status"]) {
        Ok(stdout) => (
            parse_wpctl_devices(&stdout, AudioTarget::Output),
            parse_wpctl_devices(&stdout, AudioTarget::Input),
        ),
        Err(_) => (Vec::new(), Vec::new()),
    }
}

fn audio_endpoint_default_volume_status(target: AudioTarget) -> AudioEndpointStatus {
    match wpctl(&["get-volume", target.wpctl_id()]) {
        Ok(stdout) => {
            audio_endpoint_status_from_default_volume(target, parse_wpctl_volume(&stdout))
        }
        Err(WpctlError::Missing) => AudioEndpointStatus {
            available: false,
            volume_percent: None,
            muted: None,
            default_device_id: None,
            devices: Vec::new(),
            detail: "WirePlumber control tooling is not ready in this session.".to_string(),
        },
        Err(WpctlError::Failed(detail)) => AudioEndpointStatus {
            available: false,
            volume_percent: None,
            muted: None,
            default_device_id: None,
            devices: Vec::new(),
            detail: wpctl_error_detail(&detail, target),
        },
    }
}

fn audio_endpoint_status_from_default_volume(
    target: AudioTarget,
    volume: Option<ParsedVolume>,
) -> AudioEndpointStatus {
    let detail = volume
        .map(|volume| audio_endpoint_detail(target, volume.volume_percent, volume.muted))
        .unwrap_or_else(|| audio_endpoint_ready_without_volume_detail(target));
    AudioEndpointStatus {
        available: true,
        volume_percent: volume.map(|volume| volume.volume_percent),
        muted: volume.map(|volume| volume.muted),
        default_device_id: None,
        devices: Vec::new(),
        detail,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ParsedVolume {
    volume_percent: u8,
    muted: bool,
}

fn parse_wpctl_volume(stdout: &str) -> Option<ParsedVolume> {
    let muted = stdout.to_ascii_lowercase().contains("muted");
    let value = stdout.split_whitespace().find_map(|token| {
        token
            .trim_end_matches(':')
            .trim_matches(|ch| ch == '[' || ch == ']')
            .parse::<f64>()
            .ok()
    })?;
    Some(ParsedVolume {
        volume_percent: normalized_audio_volume_percent((value * 100.0).round() as i64),
        muted,
    })
}

fn normalized_audio_volume_percent(value: i64) -> u8 {
    value.clamp(0, 150) as u8
}

fn audio_endpoint_detail(target: AudioTarget, volume_percent: u8, muted: bool) -> String {
    let muted = if muted { "muted" } else { "unmuted" };
    format!(
        "{} volume is {}% and {}.",
        title_case_audio_target(target),
        volume_percent,
        muted
    )
}

fn audio_endpoint_ready_without_volume_detail(target: AudioTarget) -> String {
    format!(
        "{} is ready. Volume read-back is not required for endpoint readiness.",
        title_case_audio_target(target)
    )
}

fn title_case_audio_target(target: AudioTarget) -> &'static str {
    match target {
        AudioTarget::Output => "Output",
        AudioTarget::Input => "Input",
    }
}

fn audio_status_detail(
    wireplumber_available: bool,
    output: &AudioEndpointStatus,
    input: &AudioEndpointStatus,
) -> String {
    if !wireplumber_available {
        return "Audio routing controls are not ready, so Settings cannot inspect or change audio here.".to_string();
    }
    match (output.available, input.available) {
        (true, true) => "Default output and input are available through WirePlumber.".to_string(),
        (true, false) => "Default output is available; default input is not reported.".to_string(),
        (false, true) => "Default input is available; default output is not reported.".to_string(),
        (false, false) => {
            "WirePlumber is present, but no default output or input is reported.".to_string()
        }
    }
}

fn audio_volume_outcome(
    target: &str,
    volume_percent: u8,
) -> (StatusCode, Json<AudioControlOutcome>) {
    let Some(target) = AudioTarget::from_request(target) else {
        return audio_control_outcome(
            StatusCode::BAD_REQUEST,
            false,
            "unknown",
            "Choose output or input before changing audio volume.",
            None,
            None,
        );
    };

    let volume_percent = normalized_audio_volume_percent(i64::from(volume_percent));
    let volume = format!("{:.2}", f64::from(volume_percent) / 100.0);
    match wpctl(&["set-volume", target.wpctl_id(), &volume]) {
        Ok(_) => audio_control_outcome(
            StatusCode::OK,
            true,
            target.request_name(),
            &format!(
                "{} volume set to {}%.",
                title_case_audio_target(target),
                volume_percent
            ),
            Some(volume_percent),
            None,
        ),
        Err(WpctlError::Missing) => audio_control_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            target.request_name(),
            "Audio routing controls are not ready, so Settings cannot change audio volume.",
            None,
            None,
        ),
        Err(WpctlError::Failed(detail)) => audio_control_outcome(
            StatusCode::BAD_GATEWAY,
            false,
            target.request_name(),
            &wpctl_error_detail(&detail, target),
            None,
            None,
        ),
    }
}

fn audio_mute_outcome(target: &str, muted: bool) -> (StatusCode, Json<AudioControlOutcome>) {
    let Some(target) = AudioTarget::from_request(target) else {
        return audio_control_outcome(
            StatusCode::BAD_REQUEST,
            false,
            "unknown",
            "Choose output or input before changing audio mute state.",
            None,
            None,
        );
    };

    let muted_arg = if muted { "1" } else { "0" };
    match wpctl(&["set-mute", target.wpctl_id(), muted_arg]) {
        Ok(_) => audio_control_outcome(
            StatusCode::OK,
            true,
            target.request_name(),
            &audio_mute_success_detail(target, muted),
            None,
            Some(muted),
        ),
        Err(WpctlError::Missing) => audio_control_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            target.request_name(),
            "Audio routing controls are not ready, so Settings cannot change mute state.",
            None,
            None,
        ),
        Err(WpctlError::Failed(detail)) => audio_control_outcome(
            StatusCode::BAD_GATEWAY,
            false,
            target.request_name(),
            &wpctl_error_detail(&detail, target),
            None,
            None,
        ),
    }
}

fn audio_default_device_outcome(
    target: &str,
    device_id: &str,
) -> (StatusCode, Json<AudioControlOutcome>) {
    let Some(target) = AudioTarget::from_request(target) else {
        return audio_control_outcome(
            StatusCode::BAD_REQUEST,
            false,
            "unknown",
            "Choose output or input before changing the default audio device.",
            None,
            None,
        );
    };

    let device_id = device_id.trim();
    if !is_wpctl_numeric_id(device_id) {
        return audio_control_outcome(
            StatusCode::BAD_REQUEST,
            false,
            target.request_name(),
            "Choose a reported WirePlumber audio device before changing the default.",
            None,
            None,
        );
    }

    let devices = audio_devices(target);
    let Some(device) = devices.iter().find(|device| device.id == device_id) else {
        return audio_control_outcome(
            StatusCode::NOT_FOUND,
            false,
            target.request_name(),
            "That audio device is not reported by WirePlumber in this session.",
            None,
            None,
        );
    };
    let device_name = device.name.clone();

    match wpctl(&["set-default", device_id]) {
        Ok(_) => audio_control_outcome(
            StatusCode::OK,
            true,
            target.request_name(),
            &format!(
                "{} device set to {}.",
                title_case_audio_target(target),
                device_name
            ),
            None,
            None,
        ),
        Err(WpctlError::Missing) => audio_control_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            target.request_name(),
            "Audio routing controls are not ready, so Settings cannot change the default audio device.",
            None,
            None,
        ),
        Err(WpctlError::Failed(detail)) => audio_control_outcome(
            StatusCode::BAD_GATEWAY,
            false,
            target.request_name(),
            &wpctl_error_detail(&detail, target),
            None,
            None,
        ),
    }
}

fn build_sound_preferences_status() -> SoundPreferencesStatus {
    SoundPreferencesStatus {
        gsettings_available: executable_exists("gsettings"),
        schema_available: false,
        event_sounds: None,
        input_feedback_sounds: None,
        volume_boost: None,
        theme_name: None,
        detail: "Audio endpoint readiness does not wait for desktop sound preference reads. Changing system sounds still checks the session bridge live.".to_string(),
    }
}

fn sound_preference_outcome(
    request: SetSoundPreferenceRequest,
) -> (StatusCode, Json<SoundPreferenceOutcome>) {
    let spec = sound_preference_spec(request.target);
    let (gsettings_available, schema) = sound_schema_snapshot();
    if !gsettings_available {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SoundPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so sound preferences cannot be changed in this session.".to_string(),
            }),
        );
    }

    if !schema.available || !schema.has_key(spec.key) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SoundPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: format!(
                    "{} is not ready because the required preference is not reported by this desktop session.",
                    spec.label
                ),
            }),
        );
    }

    let encoded_value = request.value.to_string();
    match gsettings(&["set", SOUND_SCHEMA, spec.key, &encoded_value]) {
        Ok(_) => (
            StatusCode::OK,
            Json(SoundPreferenceOutcome {
                ok: true,
                target: spec.target,
                text: sound_preference_success_detail(spec, request.value).to_string(),
            }),
        ),
        Err(GSettingsError::Missing) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(SoundPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so sound preferences cannot be changed in this session.".to_string(),
            }),
        ),
        Err(GSettingsError::Failed(detail)) => (
            StatusCode::BAD_GATEWAY,
            Json(SoundPreferenceOutcome {
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

fn audio_control_outcome(
    status: StatusCode,
    ok: bool,
    target: &str,
    text: &str,
    volume_percent: Option<u8>,
    muted: Option<bool>,
) -> (StatusCode, Json<AudioControlOutcome>) {
    (
        status,
        Json(AudioControlOutcome {
            ok,
            target: target.to_string(),
            text: text.to_string(),
            volume_percent,
            muted,
        }),
    )
}

struct SoundSchemaSnapshot {
    available: bool,
    values: BTreeMap<String, String>,
}

impl SoundSchemaSnapshot {
    fn unavailable() -> Self {
        Self {
            available: false,
            values: BTreeMap::new(),
        }
    }

    fn has_key(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }
}

#[derive(Clone, Copy)]
struct SoundPreferenceSpec {
    target: &'static str,
    key: &'static str,
    label: &'static str,
}

fn sound_schema_snapshot() -> (bool, SoundSchemaSnapshot) {
    match gsettings(&["list-recursively", SOUND_SCHEMA]) {
        Ok(stdout) => (true, parse_sound_schema_snapshot(&stdout)),
        Err(GSettingsError::Missing) => (false, SoundSchemaSnapshot::unavailable()),
        Err(GSettingsError::Failed(_)) => (true, SoundSchemaSnapshot::unavailable()),
    }
}

fn parse_sound_schema_snapshot(stdout: &str) -> SoundSchemaSnapshot {
    let mut values = BTreeMap::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some(rest) = line
            .strip_prefix(SOUND_SCHEMA)
            .map(str::trim_start)
            .filter(|rest| !rest.is_empty())
        else {
            continue;
        };
        let Some((key, value)) = rest.split_once(' ') else {
            continue;
        };
        if matches!(
            key,
            "event-sounds"
                | "input-feedback-sounds"
                | "allow-volume-above-100-percent"
                | "theme-name"
        ) {
            values.insert(key.to_string(), value.trim().to_string());
        }
    }
    SoundSchemaSnapshot {
        available: !values.is_empty(),
        values,
    }
}

fn sound_preference_spec(target: SoundPreferenceTarget) -> SoundPreferenceSpec {
    match target {
        SoundPreferenceTarget::EventSounds => SoundPreferenceSpec {
            target: "event-sounds",
            key: "event-sounds",
            label: "Interface sounds",
        },
        SoundPreferenceTarget::InputFeedbackSounds => SoundPreferenceSpec {
            target: "input-feedback-sounds",
            key: "input-feedback-sounds",
            label: "Input feedback sounds",
        },
        SoundPreferenceTarget::VolumeBoost => SoundPreferenceSpec {
            target: "volume-boost",
            key: "allow-volume-above-100-percent",
            label: "Allow volume above 100%",
        },
    }
}

fn sound_preference_success_detail(spec: SoundPreferenceSpec, enabled: bool) -> &'static str {
    match spec.target {
        "event-sounds" => interface_sounds_detail(enabled),
        "input-feedback-sounds" => input_feedback_sounds_detail(enabled),
        "volume-boost" => volume_boost_detail(enabled),
        _ => "Sound preference saved.",
    }
}

fn interface_sounds_detail(enabled: bool) -> &'static str {
    if enabled {
        "Interface event sounds use the configured desktop sound theme."
    } else {
        "Interface event sounds are off; visual feedback remains available."
    }
}

fn input_feedback_sounds_detail(enabled: bool) -> &'static str {
    if enabled {
        "Input feedback sounds can play for text entry and desktop controls."
    } else {
        "Input feedback sounds are muted."
    }
}

fn volume_boost_detail(enabled: bool) -> &'static str {
    if enabled {
        "Output volume may exceed 100% when PipeWire and the desktop session permit it."
    } else {
        "Output volume is capped at 100% for normal playback."
    }
}

fn audio_mute_success_detail(target: AudioTarget, muted: bool) -> String {
    if muted {
        format!("{} muted.", title_case_audio_target(target))
    } else {
        format!("{} unmuted.", title_case_audio_target(target))
    }
}

fn wpctl_error_detail(detail: &str, target: AudioTarget) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        format!("WirePlumber could not change the {}.", target.label())
    } else {
        format!(
            "WirePlumber could not change the {}: {detail}",
            target.label()
        )
    }
}

fn parse_wpctl_devices(stdout: &str, target: AudioTarget) -> Vec<AudioDeviceStatus> {
    let section_name = match target {
        AudioTarget::Output => "sinks",
        AudioTarget::Input => "sources",
    };
    let mut in_section = false;
    let mut devices = Vec::new();

    for line in stdout.lines() {
        let trimmed = trim_wpctl_tree_prefix(line);
        let lower = trimmed.to_ascii_lowercase();

        if lower.ends_with(':') {
            in_section = lower.trim_end_matches(':') == section_name;
            continue;
        }

        if !in_section || trimmed.is_empty() {
            continue;
        }

        if let Some(device) = parse_wpctl_device_line(trimmed) {
            devices.push(device);
        }
    }

    devices
}

fn trim_wpctl_tree_prefix(line: &str) -> &str {
    line.trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '│' | '├' | '└' | '─'))
        .trim()
}

fn parse_wpctl_device_line(line: &str) -> Option<AudioDeviceStatus> {
    let active = line.starts_with('*');
    let line = line.trim_start_matches('*').trim();
    let (id, rest) = line.split_once('.')?;
    let id = id.trim();
    if !is_wpctl_numeric_id(id) {
        return None;
    }
    let name = rest.split(" [").next().unwrap_or(rest).trim().to_string();
    if name.is_empty() {
        return None;
    }
    let parsed_volume = rest
        .split_once(" [")
        .and_then(|(_, suffix)| parse_wpctl_volume(suffix));
    Some(AudioDeviceStatus {
        id: id.to_string(),
        name,
        active,
        volume_percent: parsed_volume.map(|volume| volume.volume_percent),
        muted: parsed_volume.map(|volume| volume.muted),
    })
}

fn is_wpctl_numeric_id(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
}

enum WpctlError {
    Missing,
    Failed(String),
}

enum GSettingsError {
    Missing,
    Failed(String),
}

fn wpctl(args: &[&str]) -> Result<String, WpctlError> {
    match session_bridge::wpctl(args) {
        SessionBridgeResult::Success(stdout) => return Ok(stdout),
        SessionBridgeResult::Failed(detail) => return Err(WpctlError::Failed(detail)),
        SessionBridgeResult::Unavailable => {}
    }

    match bounded_command_output("wpctl", args, wpctl_timeout_duration()) {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Err(WpctlError::Failed(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }))
        }
        Err(BoundedCommandError::Missing) => Err(WpctlError::Missing),
        Err(BoundedCommandError::TimedOut) => Err(WpctlError::Failed(
            "WirePlumber did not answer before the audio status timeout.".to_string(),
        )),
        Err(BoundedCommandError::Failed) => Err(WpctlError::Failed(
            "Audio routing controls are not ready in this session.".to_string(),
        )),
    }
}

fn wpctl_timeout_duration() -> Duration {
    Duration::from_millis(clamp_wpctl_timeout_ms(
        std::env::var("GOBLINS_OS_WPCTL_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
    ))
}

fn clamp_wpctl_timeout_ms(parsed: Option<u64>) -> u64 {
    parsed
        .unwrap_or(WPCTL_TIMEOUT_MS_DEFAULT)
        .clamp(WPCTL_TIMEOUT_MS_MIN, WPCTL_TIMEOUT_MS_MAX)
}

fn gsettings(args: &[&str]) -> Result<String, GSettingsError> {
    match session_bridge::gsettings(args) {
        SessionBridgeResult::Success(stdout) => return Ok(stdout),
        SessionBridgeResult::Failed(detail) => return Err(GSettingsError::Failed(detail)),
        SessionBridgeResult::Unavailable => {}
    }

    match bounded_command_output(
        "gsettings",
        args,
        Duration::from_millis(GSETTINGS_TIMEOUT_MS),
    ) {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(GSettingsError::Failed(gsettings_error_detail(
            &String::from_utf8_lossy(&output.stderr),
            &String::from_utf8_lossy(&output.stdout),
        ))),
        Err(BoundedCommandError::Missing) => Err(GSettingsError::Missing),
        Err(BoundedCommandError::TimedOut) => Err(GSettingsError::Failed(
            "Desktop preferences did not answer before the sound preference timeout.".to_string(),
        )),
        Err(BoundedCommandError::Failed) => Err(GSettingsError::Failed(
            "Desktop preferences are not ready in this session.".to_string(),
        )),
    }
}

fn gsettings_error_detail(stderr: &str, stdout: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }
    stdout.trim().to_string()
}

fn executable_exists(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&paths).any(|path| {
        let candidate = path.join(binary);
        std::fs::metadata(candidate)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        audio_endpoint_detail, audio_endpoint_status, audio_endpoint_status_from_default_volume,
        audio_mute_success_detail, audio_status_detail, build_sound_preferences_status,
        clamp_wpctl_timeout_ms, interface_sounds_detail, is_wpctl_numeric_id,
        normalized_audio_volume_percent, parse_sound_schema_snapshot, parse_wpctl_devices,
        parse_wpctl_volume, sound_preference_spec, sound_preference_success_detail,
        title_case_audio_target, AudioDeviceStatus, AudioEndpointStatus, AudioTarget, ParsedVolume,
        SoundPreferenceTarget,
    };

    #[test]
    fn parses_wpctl_volume_and_mute_state() {
        let parsed = parse_wpctl_volume("Volume: 0.62").unwrap();
        assert_eq!(parsed.volume_percent, 62);
        assert!(!parsed.muted);

        let parsed = parse_wpctl_volume("Volume: 0.40 [MUTED]").unwrap();
        assert_eq!(parsed.volume_percent, 40);
        assert!(parsed.muted);

        assert!(parse_wpctl_volume("Volume: unavailable").is_none());
        assert_eq!(normalized_audio_volume_percent(-5), 0);
        assert_eq!(normalized_audio_volume_percent(151), 150);
    }

    #[test]
    fn audio_copy_stays_targeted_and_truthful() {
        assert_eq!(title_case_audio_target(AudioTarget::Output), "Output");
        assert!(audio_endpoint_detail(AudioTarget::Input, 55, true).contains("muted"));
        assert_eq!(
            audio_mute_success_detail(AudioTarget::Output, false),
            "Output unmuted."
        );

        let output = AudioEndpointStatus {
            available: true,
            volume_percent: Some(62),
            muted: Some(false),
            default_device_id: Some("55".to_string()),
            devices: Vec::new(),
            detail: "ready".to_string(),
        };
        let input = AudioEndpointStatus {
            available: false,
            volume_percent: None,
            muted: None,
            default_device_id: None,
            devices: Vec::new(),
            detail: "missing".to_string(),
        };
        assert!(audio_status_detail(true, &output, &input).contains("Default output"));
        assert!(audio_status_detail(false, &output, &input).contains("not ready"));
    }

    #[test]
    fn parses_wpctl_sinks_and_sources_for_default_device_selection() {
        let stdout = r#"
Audio
 ├─ Devices:
 │      42. Built-in Audio
 ├─ Sinks:
 │  *   55. Built-in Audio Analog Stereo [vol: 0.62]
 │      56. HDMI Audio [vol: 0.40]
 ├─ Sources:
 │  *   57. Built-in Audio Analog Stereo [vol: 0.52 MUTED]
 │      58. USB Microphone [vol: 1.00]
"#;

        let sinks = parse_wpctl_devices(stdout, AudioTarget::Output);
        assert_eq!(sinks.len(), 2);
        assert_eq!(sinks[0].id, "55");
        assert_eq!(sinks[0].name, "Built-in Audio Analog Stereo");
        assert!(sinks[0].active);
        assert_eq!(sinks[0].volume_percent, Some(62));
        assert_eq!(sinks[0].muted, Some(false));
        assert_eq!(sinks[1].id, "56");
        assert!(!sinks[1].active);

        let sources = parse_wpctl_devices(stdout, AudioTarget::Input);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].volume_percent, Some(52));
        assert_eq!(sources[0].muted, Some(true));
        assert_eq!(sources[1].name, "USB Microphone");
        assert!(is_wpctl_numeric_id("58"));
        assert!(!is_wpctl_numeric_id("../58"));
    }

    #[test]
    fn device_snapshot_marks_default_device_for_selection() {
        let devices = vec![AudioDeviceStatus {
            id: "55".to_string(),
            name: "Built-in Audio Analog Stereo".to_string(),
            active: true,
            volume_percent: None,
            muted: None,
        }];

        let status = audio_endpoint_status(AudioTarget::Output, devices);
        assert!(status.available);
        assert_eq!(status.default_device_id.as_deref(), Some("55"));
        assert_eq!(status.volume_percent, None);
        assert!(status
            .detail
            .contains("Volume read-back is not required for endpoint readiness."));
    }

    #[test]
    fn wpctl_timeout_defaults_and_clamps() {
        assert_eq!(clamp_wpctl_timeout_ms(None), 500);
        assert_eq!(clamp_wpctl_timeout_ms(Some(1)), 100);
        assert_eq!(clamp_wpctl_timeout_ms(Some(99)), 100);
        assert_eq!(clamp_wpctl_timeout_ms(Some(100)), 100);
        assert_eq!(clamp_wpctl_timeout_ms(Some(1_500)), 1_500);
        assert_eq!(clamp_wpctl_timeout_ms(Some(5_000)), 5_000);
        assert_eq!(clamp_wpctl_timeout_ms(Some(10_000)), 5_000);
    }

    #[test]
    fn sound_schema_snapshot_parses_single_recursive_gsettings_read() {
        let snapshot = parse_sound_schema_snapshot(
            "org.gnome.desktop.sound event-sounds true\n\
             org.gnome.desktop.sound input-feedback-sounds false\n\
             org.gnome.desktop.sound allow-volume-above-100-percent true\n\
             org.gnome.desktop.sound theme-name 'freedesktop'\n",
        );

        assert!(snapshot.available);
        assert!(snapshot.has_key("event-sounds"));
        assert!(snapshot.has_key("input-feedback-sounds"));
        assert!(snapshot.has_key("allow-volume-above-100-percent"));
        assert!(snapshot.has_key("theme-name"));
        assert!(!snapshot.has_key("unknown-key"));
    }

    #[test]
    fn audio_status_defers_sound_preference_reads() {
        let sound = build_sound_preferences_status();
        assert!(!sound.schema_available);
        assert_eq!(sound.event_sounds, None);
        assert!(sound.detail.contains("does not wait"));
        assert!(sound.detail.contains("checks the session bridge live"));
    }

    #[test]
    fn default_volume_read_marks_endpoint_ready_without_device_enumeration() {
        let endpoint = audio_endpoint_status_from_default_volume(
            AudioTarget::Output,
            Some(ParsedVolume {
                volume_percent: 42,
                muted: false,
            }),
        );
        assert!(endpoint.available);
        assert_eq!(endpoint.volume_percent, Some(42));
        assert_eq!(endpoint.muted, Some(false));
        assert_eq!(endpoint.default_device_id, None);
        assert!(endpoint.devices.is_empty());
        assert!(endpoint.detail.contains("42%"));
    }

    #[test]
    fn sound_preference_copy_stays_truthful() {
        let event_sounds = sound_preference_spec(SoundPreferenceTarget::EventSounds);
        assert_eq!(event_sounds.target, "event-sounds");
        assert_eq!(
            sound_preference_success_detail(event_sounds, true),
            interface_sounds_detail(true)
        );

        let volume_boost = sound_preference_spec(SoundPreferenceTarget::VolumeBoost);
        assert!(sound_preference_success_detail(volume_boost, true).contains("exceed 100%"));
        assert!(sound_preference_success_detail(volume_boost, false).contains("capped"));
    }
}
