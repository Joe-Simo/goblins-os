use std::{
    env,
    ffi::CString,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::fd::AsRawFd,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use goblins_os_textshortcuts_engine::{TableLoadStatus, TextShortcut, TextShortcutTableStore};
use serde::{Deserialize, Serialize};

const DEFAULT_SOCKET: &str = "/run/goblins-os-session/session-bridge.sock";
const SOCKET_GROUP: &str = "goblins-session-bridge";
const CORE_SERVICE_USER: &str = "goblins-os";
const MAX_REQUEST_BYTES: usize = 24 * 1024 * 1024;
const MAX_CAPTURE_WAV_BYTES: usize = 512 * 1024;
const MAX_PLAYBACK_WAV_BYTES: usize = 16 * 1024 * 1024;
const MAX_CAPTURE_DURATION_SECONDS: u64 = 7;
const MAX_PLAYBACK_DURATION_SECONDS: u64 = 90;
const VOICE_CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);
const VOICE_PLAYBACK_TIMEOUT: Duration = Duration::from_secs(120);
const TEXT_SHORTCUTS_RUNTIME_STATUS_PATH: &str =
    "/run/goblins-os-session/text-shortcuts-runtime-status.json";
const TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA: &str = "goblins-os.text-shortcuts-runtime-status.v1";
const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES: usize = 4 * 1024;
const TEXT_SHORTCUTS_RUNTIME_STATUS_MODE: u32 = 0o600;
const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS: u64 = 5_000_000_000;
const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS: u64 = 250_000_000;

const KEYBOARD_SCHEMA: &str = "org.gnome.desktop.peripherals.keyboard";
const MOUSE_SCHEMA: &str = "org.gnome.desktop.peripherals.mouse";
const TOUCHPAD_SCHEMA: &str = "org.gnome.desktop.peripherals.touchpad";
const INPUT_SOURCES_SCHEMA: &str = "org.gnome.desktop.input-sources";
const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
const A11Y_APPS_SCHEMA: &str = "org.gnome.desktop.a11y.applications";
const A11Y_INTERFACE_SCHEMA: &str = "org.gnome.desktop.a11y.interface";
const A11Y_KEYBOARD_SCHEMA: &str = "org.gnome.desktop.a11y.keyboard";
const A11Y_MAGNIFIER_SCHEMA: &str = "org.gnome.desktop.a11y.magnifier";
const A11Y_MOUSE_SCHEMA: &str = "org.gnome.desktop.a11y.mouse";
const COLOR_SCHEMA: &str = "org.gnome.settings-daemon.plugins.color";
const FOCUS_SCHEMA: &str = "org.goblins.os.focus";
const NOTIFICATIONS_SCHEMA: &str = "org.gnome.desktop.notifications";
const NOTIFICATION_APPLICATION_SCHEMA: &str = "org.gnome.desktop.notifications.application";
const NOTIFICATION_APPLICATION_BASE_PATH: &str = "/org/gnome/desktop/notifications/application/";
const WM_SCHEMA: &str = "org.goblins.shell.extensions.wm";
const PERMISSION_STORE_DEST: &str = "org.freedesktop.impl.portal.PermissionStore";
const PERMISSION_STORE_PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
const PERMISSION_STORE_DELETE_PERMISSION: &str =
    "org.freedesktop.impl.portal.PermissionStore.DeletePermission";
const MUTTER_DISPLAY_CONFIG_DEST: &str = "org.gnome.Mutter.DisplayConfig";
const MUTTER_DISPLAY_CONFIG_PATH: &str = "/org/gnome/Mutter/DisplayConfig";
const MUTTER_DISPLAY_CONFIG_GET_CURRENT_STATE: &str =
    "org.gnome.Mutter.DisplayConfig.GetCurrentState";
const MUTTER_DISPLAY_CONFIG_APPLY_MONITORS: &str =
    "org.gnome.Mutter.DisplayConfig.ApplyMonitorsConfig";
const SOUND_SCHEMA: &str = "org.gnome.desktop.sound";
const DEFAULT_SINK: &str = "@DEFAULT_AUDIO_SINK@";
const DEFAULT_SOURCE: &str = "@DEFAULT_AUDIO_SOURCE@";
const GSETTINGS_TIMEOUT: Duration = Duration::from_millis(1_500);
const WPCTL_TIMEOUT: Duration = Duration::from_millis(1_500);
const IBUS_TIMEOUT: Duration = Duration::from_millis(1_500);

const KEYBOARD_KEYS: &[&str] = &[
    "repeat",
    "delay",
    "repeat-interval",
    "remember-numlock-state",
];
const MOUSE_KEYS: &[&str] = &[
    "speed",
    "natural-scroll",
    "left-handed",
    "middle-click-emulation",
];
const TOUCHPAD_KEYS: &[&str] = &[
    "speed",
    "tap-to-click",
    "natural-scroll",
    "two-finger-scrolling-enabled",
    "disable-while-typing",
];
const INPUT_SOURCE_KEYS: &[&str] = &["sources", "current", "xkb-options"];
const INTERFACE_KEYS: &[&str] = &["enable-animations", "text-scaling-factor"];
const A11Y_APPS_KEYS: &[&str] = &[
    "screen-reader-enabled",
    "screen-keyboard-enabled",
    "screen-magnifier-enabled",
];
const A11Y_INTERFACE_KEYS: &[&str] = &["high-contrast"];
const A11Y_KEYBOARD_KEYS: &[&str] = &[
    "stickykeys-enable",
    "slowkeys-enable",
    "bouncekeys-enable",
    "mousekeys-enable",
];
const A11Y_MAGNIFIER_KEYS: &[&str] = &["mag-factor", "lens-mode"];
const A11Y_MOUSE_KEYS: &[&str] = &["dwell-click-enabled"];
const COLOR_KEYS: &[&str] = &[
    "night-light-enabled",
    "night-light-schedule-automatic",
    "night-light-temperature",
];
const FOCUS_KEYS: &[&str] = &[
    "active-mode",
    "modes",
    "schedules",
    "armed-by-schedule",
    "restore-banners",
    "restore-apps",
];
const NOTIFICATION_KEYS: &[&str] = &[
    "show-banners",
    "show-in-lock-screen",
    "application-children",
];
const NOTIFICATION_APPLICATION_KEYS: &[&str] = &[
    "application-id",
    "enable",
    "show-banners",
    "enable-sound-alerts",
    "show-in-lock-screen",
    "details-in-lock-screen",
    "force-expanded",
];
const SOUND_KEYS: &[&str] = &[
    "event-sounds",
    "input-feedback-sounds",
    "allow-volume-above-100-percent",
    "theme-name",
];
const WM_KEYS: &[&str] = &[
    "mission-control",
    "app-expose",
    "window-switcher",
    "window-hud",
    "snap-left",
    "snap-right",
    "snap-top-left",
    "snap-top-right",
    "snap-bottom-left",
    "snap-bottom-right",
    "restore-window",
    "center-window",
    "space-left",
    "space-right",
];

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
enum BridgeRequest {
    Ping,
    GSettings {
        args: Vec<String>,
    },
    OpenPreview {
        path: String,
        kind: String,
    },
    Wpctl {
        args: Vec<String>,
    },
    VoiceAudioStatus {},
    VoiceCapture {},
    VoicePlayback {
        wav_base64: String,
    },
    PermissionStoreDelete {
        table: String,
        id: String,
        app: String,
    },
    DisplayConfigGetCurrentState,
    DisplayConfigGetApplyAllowed,
    DisplayConfigApplyMonitors {
        serial: u32,
        method: u32,
        logical_monitors: Vec<DisplayConfigLogicalMonitor>,
    },
    IbusEngine,
    TextShortcutsRuntimeStatus {},
    TextShortcutsRead {},
    TextShortcutsWrite {
        shortcuts: Vec<TextShortcut>,
    },
}

#[derive(Clone, Debug, Deserialize)]
struct DisplayConfigLogicalMonitor {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<DisplayConfigMonitor>,
}

#[derive(Clone, Debug, Deserialize)]
struct DisplayConfigMonitor {
    connector: String,
    mode_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct TextShortcutsRuntimeStatus {
    schema: String,
    instance_id: String,
    focus_generation: u64,
    runtime_generation: u64,
    sequence: u64,
    monotonic_ns: u64,
    focused: bool,
    enabled: bool,
    surrounding_text_supported: bool,
    snapshot_valid: bool,
    child_alive: bool,
    last_response_ok: bool,
}

#[derive(Serialize)]
struct BridgeResponse {
    ok: bool,
    stdout: String,
    detail: String,
}

#[derive(Serialize)]
struct VoiceAudioStatus {
    capture_ready: bool,
    playback_ready: bool,
    capture_detail: &'static str,
    playback_detail: &'static str,
}

fn main() {
    if env::args().any(|arg| arg == "--self-test") {
        match self_test() {
            Ok(()) => {
                println!("goblins-os-session-bridge self-test passed");
                return;
            }
            Err(error) => {
                eprintln!("goblins-os-session-bridge self-test failed: {error}");
                std::process::exit(1);
            }
        }
    }

    if let Err(error) = run_server() {
        eprintln!("goblins-os-session-bridge: {error}");
        std::process::exit(1);
    }
}

fn run_server() -> Result<(), String> {
    let socket = socket_path();
    prepare_socket_parent(&socket)?;
    remove_stale_socket(&socket)?;
    let listener = UnixListener::bind(&socket)
        .map_err(|error| format!("could not bind {}: {error}", socket.display()))?;
    set_group_mode(&socket, 0o660)?;

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                // One thread per connection: requests are independent bounded
                // command runs, and serializing them behind a single slow
                // probe would queue every other caller past its own client
                // timeout. Socket I/O is bounded so a stalled client can
                // never pin a handler thread.
                thread::spawn(move || {
                    let response = handle_stream(&mut stream);
                    let _ = write_response(&mut stream, &response);
                });
            }
            Err(error) => eprintln!("goblins-os-session-bridge: connection failed: {error}"),
        }
    }
    Ok(())
}

const STREAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

fn handle_stream(stream: &mut UnixStream) -> BridgeResponse {
    let peer_uid = match unix_peer_uid(stream) {
        Ok(uid) => uid,
        Err(()) => return failure("session bridge peer authentication failed."),
    };
    let body = match read_to_end_before(
        stream,
        MAX_REQUEST_BYTES,
        Instant::now() + STREAM_REQUEST_TIMEOUT,
    ) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            return failure("session bridge request exceeds the fixed size limit.")
        }
        Err(_) => return failure("session bridge request did not finish before its deadline."),
    };
    let request = match serde_json::from_slice::<BridgeRequest>(&body) {
        Ok(request) => request,
        Err(_) => {
            return failure("session bridge request did not match an allowlisted operation.");
        }
    };
    handle_request(request, peer_uid)
}

fn handle_request(request: BridgeRequest, peer_uid: u32) -> BridgeResponse {
    if matches!(
        &request,
        BridgeRequest::VoiceAudioStatus {}
            | BridgeRequest::VoiceCapture {}
            | BridgeRequest::VoicePlayback { .. }
    ) && resolve_user_id(CORE_SERVICE_USER) != Some(peer_uid)
    {
        return failure("voice operations require the authenticated core service peer.");
    }
    match request {
        BridgeRequest::Ping => success("pong".to_string()),
        BridgeRequest::GSettings { args } => gsettings_response(args),
        BridgeRequest::OpenPreview { path, kind } => open_preview_response(&path, &kind),
        BridgeRequest::Wpctl { args } => wpctl_response(args),
        BridgeRequest::VoiceAudioStatus {} => voice_audio_status_response(),
        BridgeRequest::VoiceCapture {} => voice_capture_response(),
        BridgeRequest::VoicePlayback { wav_base64 } => voice_playback_response(&wav_base64),
        BridgeRequest::PermissionStoreDelete { table, id, app } => {
            permission_store_delete_response(&table, &id, &app)
        }
        BridgeRequest::DisplayConfigGetCurrentState => display_config_get_current_state_response(),
        BridgeRequest::DisplayConfigGetApplyAllowed => display_config_apply_allowed_response(),
        BridgeRequest::DisplayConfigApplyMonitors {
            serial,
            method,
            logical_monitors,
        } => display_config_apply_monitors_response(serial, method, &logical_monitors),
        BridgeRequest::IbusEngine => ibus_engine_response(),
        BridgeRequest::TextShortcutsRuntimeStatus {} => text_shortcuts_runtime_status_response(),
        BridgeRequest::TextShortcutsRead {} => text_shortcuts_read_response(),
        BridgeRequest::TextShortcutsWrite { shortcuts } => text_shortcuts_write_response(shortcuts),
    }
}

fn unix_peer_uid(stream: &UnixStream) -> Result<u32, ()> {
    #[cfg(target_os = "linux")]
    {
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: credentials and length are writable storage matching the sizes
        // passed to getsockopt; stream owns a live Unix-domain descriptor.
        let status = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        };
        if status == 0 && length as usize == std::mem::size_of::<libc::ucred>() {
            Ok(credentials.uid)
        } else {
            Err(())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut uid = 0;
        let mut gid = 0;
        // SAFETY: uid and gid are writable pointers and stream owns a live
        // Unix-domain descriptor.
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0 {
            Ok(uid)
        } else {
            Err(())
        }
    }
}

fn resolve_user_id(name: &str) -> Option<u32> {
    let name = CString::new(name).ok()?;
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 16 * 1024];
    // SAFETY: every pointer references valid writable storage of the supplied
    // size, and name is NUL-terminated for the lifetime of the call.
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }
    // SAFETY: getpwnam_r returned success and initialized our record storage.
    Some(unsafe { record.assume_init() }.pw_uid)
}

fn text_shortcuts_runtime_status_response() -> BridgeResponse {
    let now_ns = match monotonic_now_ns() {
        Ok(now_ns) => now_ns,
        Err(error) => return failure(error),
    };
    let status = match read_text_shortcuts_runtime_status(
        Path::new(TEXT_SHORTCUTS_RUNTIME_STATUS_PATH),
        now_ns,
        effective_user_id(),
    ) {
        Ok(status) => status,
        Err(error) => return failure(error),
    };
    match serde_json::to_string(&status) {
        Ok(status) => success(status),
        Err(_) => failure("Text Shortcuts runtime status could not be encoded."),
    }
}

fn read_text_shortcuts_runtime_status(
    path: &Path,
    now_ns: u64,
    expected_owner: u32,
) -> Result<TextShortcutsRuntimeStatus, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "Text Shortcuts runtime status is unavailable.".to_string())?;
    let before = file
        .metadata()
        .map_err(|_| "Text Shortcuts runtime status metadata is unavailable.".to_string())?;
    validate_text_shortcuts_runtime_status_metadata(&before, expected_owner)?;
    if before.len() > TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES as u64 {
        return Err("Text Shortcuts runtime status exceeds the fixed size limit.".to_string());
    }

    let mut encoded = Vec::with_capacity(before.len() as usize);
    Read::by_ref(&mut file)
        .take((TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES + 1) as u64)
        .read_to_end(&mut encoded)
        .map_err(|_| "Text Shortcuts runtime status could not be read.".to_string())?;
    if encoded.len() > TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES {
        return Err("Text Shortcuts runtime status exceeds the fixed size limit.".to_string());
    }

    let after = file
        .metadata()
        .map_err(|_| "Text Shortcuts runtime status metadata is unavailable.".to_string())?;
    validate_text_shortcuts_runtime_status_metadata(&after, expected_owner)?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || after.len() != encoded.len() as u64
    {
        return Err("Text Shortcuts runtime status changed while it was read.".to_string());
    }

    let status = serde_json::from_slice::<TextShortcutsRuntimeStatus>(&encoded)
        .map_err(|_| "Text Shortcuts runtime status is not strict v1 JSON.".to_string())?;
    validate_text_shortcuts_runtime_status(&status, now_ns)?;
    Ok(status)
}

fn validate_text_shortcuts_runtime_status_metadata(
    metadata: &fs::Metadata,
    expected_owner: u32,
) -> Result<(), String> {
    if !metadata.is_file()
        || metadata.uid() != expected_owner
        || metadata.mode() & 0o7777 != TEXT_SHORTCUTS_RUNTIME_STATUS_MODE
    {
        return Err(
            "Text Shortcuts runtime status must be an owner-only regular file.".to_string(),
        );
    }
    Ok(())
}

fn validate_text_shortcuts_runtime_status(
    status: &TextShortcutsRuntimeStatus,
    now_ns: u64,
) -> Result<(), String> {
    if status.schema != TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA
        || status.instance_id.len() != 32
        || !status
            .instance_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || status.focus_generation == 0
        || status.runtime_generation == 0
        || status.sequence == 0
        || status.monotonic_ns == 0
    {
        return Err("Text Shortcuts runtime status fields are invalid.".to_string());
    }
    if status.monotonic_ns > now_ns.saturating_add(TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS) {
        return Err("Text Shortcuts runtime status timestamp is in the future.".to_string());
    }
    if now_ns.saturating_sub(status.monotonic_ns) > TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS {
        return Err("Text Shortcuts runtime status is stale.".to_string());
    }
    Ok(())
}

fn effective_user_id() -> u32 {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    unsafe { libc::geteuid() }
}

fn monotonic_now_ns() -> Result<u64, String> {
    let mut timestamp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: timestamp points to writable storage for one timespec and
    // CLOCK_MONOTONIC is supported by the target Unix session runtime.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut timestamp) } != 0 {
        return Err("Text Shortcuts monotonic clock is unavailable.".to_string());
    }
    let seconds = u64::try_from(timestamp.tv_sec)
        .map_err(|_| "Text Shortcuts monotonic clock returned an invalid value.".to_string())?;
    let nanoseconds = u64::try_from(timestamp.tv_nsec)
        .map_err(|_| "Text Shortcuts monotonic clock returned an invalid value.".to_string())?;
    if nanoseconds >= 1_000_000_000 {
        return Err("Text Shortcuts monotonic clock returned an invalid value.".to_string());
    }
    seconds
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(nanoseconds))
        .ok_or_else(|| "Text Shortcuts monotonic clock overflowed.".to_string())
}

fn text_shortcuts_read_response() -> BridgeResponse {
    let store = match TextShortcutTableStore::from_environment() {
        Ok(store) => store,
        Err(_) => return failure("Text Shortcuts private storage is unavailable."),
    };
    let outcome = store.load();
    match outcome.status() {
        TableLoadStatus::Loaded { .. } | TableLoadStatus::Missing => {
            text_shortcuts_success(outcome.table().shortcuts())
        }
        TableLoadStatus::InvalidJson => failure("Text Shortcuts private table is not valid JSON."),
        TableLoadStatus::TooLarge => {
            failure("Text Shortcuts private table exceeds the fixed size limit.")
        }
        TableLoadStatus::UnsafeFile => {
            failure("Text Shortcuts private table is not a regular file.")
        }
        TableLoadStatus::Unreadable => failure("Text Shortcuts private table could not be read."),
    }
}

fn text_shortcuts_write_response(shortcuts: Vec<TextShortcut>) -> BridgeResponse {
    let store = match TextShortcutTableStore::from_environment() {
        Ok(store) => store,
        Err(_) => return failure("Text Shortcuts private storage is unavailable."),
    };
    match store.save(shortcuts) {
        Ok(shortcuts) => text_shortcuts_success(&shortcuts),
        Err(error) => failure(error.to_string()),
    }
}

fn text_shortcuts_success(shortcuts: &[TextShortcut]) -> BridgeResponse {
    match serde_json::to_string(shortcuts) {
        Ok(shortcuts) => success(shortcuts),
        Err(_) => failure("Text Shortcuts private table could not be encoded."),
    }
}

/// Read-only probe of the session's active IBus engine. Takes no arguments by
/// construction, so there is nothing to validate: the bridge only ever runs
/// the fixed `ibus engine` read. The ibus CLI derives its socket path from the
/// display environment; when the bridge unit was started without one, fall
/// back to the session's real defaults — a wrong guess only fails the probe,
/// which degrades readiness honestly.
fn ibus_engine_response() -> BridgeResponse {
    let mut command = Command::new("ibus");
    command.arg("engine");
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        command.env("WAYLAND_DISPLAY", "wayland-0");
        command.env("DISPLAY", ":0");
    }
    match bounded_output_of(command, IBUS_TIMEOUT) {
        Ok(output) if output.status.success() => {
            success(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => failure(command_error_detail(&output.stderr, &output.stdout)),
        Err(BoundedCommandError::Missing) => {
            failure("IBus is unavailable in this desktop session.")
        }
        Err(BoundedCommandError::TimedOut) => {
            failure("IBus did not answer before the session bridge input timeout.")
        }
        Err(BoundedCommandError::Failed) => failure("IBus is not ready in this desktop session."),
    }
}

fn voice_audio_status_response() -> BridgeResponse {
    let (capture_tool_ready, playback_tool_ready, capture_endpoint_ready, playback_endpoint_ready) =
        thread::scope(|scope| {
            let capture_tool = scope.spawn(|| fixed_audio_tool_ready("arecord"));
            let playback_tool = scope.spawn(|| fixed_audio_tool_ready("aplay"));
            let capture_endpoint = scope.spawn(|| fixed_audio_endpoint_ready(DEFAULT_SOURCE));
            let playback_endpoint = scope.spawn(|| fixed_audio_endpoint_ready(DEFAULT_SINK));
            (
                capture_tool.join().unwrap_or(false),
                playback_tool.join().unwrap_or(false),
                capture_endpoint.join().unwrap_or(false),
                playback_endpoint.join().unwrap_or(false),
            )
        });
    let status = VoiceAudioStatus {
        capture_ready: capture_tool_ready && capture_endpoint_ready,
        playback_ready: playback_tool_ready && playback_endpoint_ready,
        capture_detail: if !capture_tool_ready {
            "Desktop-session microphone capture runtime is not ready."
        } else if !capture_endpoint_ready {
            "The desktop session has no reachable default microphone."
        } else {
            "Desktop-session microphone and capture runtime are ready."
        },
        playback_detail: if !playback_tool_ready {
            "Desktop-session audio playback runtime is not ready."
        } else if !playback_endpoint_ready {
            "The desktop session has no reachable default speaker."
        } else {
            "Desktop-session speaker and playback runtime are ready."
        },
    };
    match serde_json::to_string(&status) {
        Ok(encoded) => success(encoded),
        Err(_) => failure("Desktop-session voice status could not be encoded."),
    }
}

fn fixed_audio_tool_ready(binary: &str) -> bool {
    matches!(
        bounded_command_output(binary, &["--version".to_string()], WPCTL_TIMEOUT),
        Ok(output) if output.status.success()
    )
}

fn fixed_audio_endpoint_ready(target: &str) -> bool {
    matches!(
        bounded_command_output(
            "wpctl",
            &["get-volume".to_string(), target.to_string()],
            WPCTL_TIMEOUT,
        ),
        Ok(output) if output.status.success()
    )
}

fn voice_capture_response() -> BridgeResponse {
    let mut command = Command::new("arecord");
    command.args([
        "-q", "-d", "6", "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "wav",
    ]);
    let wav = match bounded_output_of(command, VOICE_CAPTURE_TIMEOUT) {
        Ok(output) if output.status.success() => output.stdout,
        Ok(_) => return failure("Desktop-session microphone capture failed."),
        Err(BoundedCommandError::Missing) => {
            return failure("Desktop-session microphone capture runtime is not ready.")
        }
        Err(BoundedCommandError::TimedOut) => {
            return failure("Desktop-session microphone capture did not finish in time.")
        }
        Err(BoundedCommandError::Failed) => {
            return failure("Desktop-session microphone capture could not start.")
        }
    };
    if wav.is_empty() || wav.len() > MAX_CAPTURE_WAV_BYTES {
        return failure("Desktop-session microphone capture exceeded its fixed size limit.");
    }
    if !valid_pcm_wave(
        &wav,
        MAX_CAPTURE_DURATION_SECONDS,
        Some(PcmFormat {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            block_align: 2,
            byte_rate: 32_000,
        }),
    ) {
        return failure("Desktop-session microphone capture did not produce a valid PCM WAV.");
    }
    success(BASE64_STANDARD.encode(wav))
}

fn voice_playback_response(wav_base64: &str) -> BridgeResponse {
    if wav_base64.len() > MAX_PLAYBACK_WAV_BYTES.saturating_mul(4).div_ceil(3) + 4 {
        return failure("Voice playback audio exceeds the fixed size limit.");
    }
    let wav = match BASE64_STANDARD.decode(wav_base64) {
        Ok(wav)
            if wav.len() <= MAX_PLAYBACK_WAV_BYTES
                && valid_pcm_wave(&wav, MAX_PLAYBACK_DURATION_SECONDS, None) =>
        {
            wav
        }
        _ => return failure("Voice playback requires one bounded PCM WAV payload."),
    };
    let mut command = Command::new("aplay");
    command.arg("-q");
    match bounded_input_output_of(command, wav, VOICE_PLAYBACK_TIMEOUT) {
        Ok(output) if output.status.success() => success("played".to_string()),
        Ok(_) => failure("Desktop-session audio playback failed."),
        Err(BoundedCommandError::Missing) => {
            failure("Desktop-session audio playback runtime is not ready.")
        }
        Err(BoundedCommandError::TimedOut) => {
            failure("Desktop-session audio playback did not finish in time.")
        }
        Err(BoundedCommandError::Failed) => {
            failure("Desktop-session audio playback could not start.")
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PcmFormat {
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    block_align: u16,
    byte_rate: u32,
}

fn valid_pcm_wave(
    wav: &[u8],
    max_duration_seconds: u64,
    required_format: Option<PcmFormat>,
) -> bool {
    if wav.len() < 44 || !wav.starts_with(b"RIFF") || wav.get(8..12) != Some(b"WAVE") {
        return false;
    }
    let Some(declared_size) = wav
        .get(4..8)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_le_bytes)
        .and_then(|size| usize::try_from(size).ok())
        .and_then(|size| size.checked_add(8))
    else {
        return false;
    };
    if declared_size != wav.len() {
        return false;
    }

    // Accept one canonical RIFF layout only: a single exact 16-byte PCM fmt
    // chunk followed by one non-empty data chunk. This keeps aplay away from
    // duplicate/ambiguous chunk graphs while retaining RIFF's one-byte pad for
    // an odd data length.
    if wav.get(12..16) != Some(b"fmt ")
        || wav.get(16..20) != Some(&16u32.to_le_bytes())
        || wav.get(36..40) != Some(b"data")
    {
        return false;
    }
    let Some(data_bytes) = wav
        .get(40..44)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_le_bytes)
        .and_then(|size| usize::try_from(size).ok())
    else {
        return false;
    };
    let Some(data_end) = 44usize.checked_add(data_bytes) else {
        return false;
    };
    let Some(canonical_end) = data_end.checked_add(data_bytes & 1) else {
        return false;
    };
    if data_bytes == 0 || canonical_end != wav.len() {
        return false;
    }

    let audio_format = u16::from_le_bytes([wav[20], wav[21]]);
    let channels = u16::from_le_bytes([wav[22], wav[23]]);
    let sample_rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    let byte_rate = u32::from_le_bytes([wav[28], wav[29], wav[30], wav[31]]);
    let block_align = u16::from_le_bytes([wav[32], wav[33]]);
    let bits_per_sample = u16::from_le_bytes([wav[34], wav[35]]);
    let format = PcmFormat {
        channels,
        sample_rate,
        bits_per_sample,
        block_align,
        byte_rate,
    };
    let expected_block_align = channels.checked_mul(bits_per_sample / 8);
    let expected_byte_rate = sample_rate.checked_mul(u32::from(block_align));
    if audio_format != 1
        || !(1..=2).contains(&channels)
        || !(8_000..=192_000).contains(&sample_rate)
        || bits_per_sample != 16
        || expected_block_align != Some(block_align)
        || expected_byte_rate != Some(byte_rate)
        || required_format.is_some_and(|required| required != format)
    {
        return false;
    }
    let max_data_bytes = u64::from(format.byte_rate).saturating_mul(max_duration_seconds);
    data_bytes % usize::from(format.block_align) == 0 && data_bytes as u64 <= max_data_bytes
}

fn wpctl_response(args: Vec<String>) -> BridgeResponse {
    if let Err(error) = validate_wpctl_args(&args) {
        return failure(error);
    }
    match bounded_command_output("wpctl", &args, WPCTL_TIMEOUT) {
        Ok(output) if output.status.success() => {
            success(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => failure(command_error_detail(&output.stderr, &output.stdout)),
        Err(BoundedCommandError::Missing) => {
            failure("wpctl is unavailable in this desktop session.")
        }
        Err(BoundedCommandError::TimedOut) => {
            failure("WirePlumber did not answer before the session bridge audio timeout.")
        }
        Err(BoundedCommandError::Failed) => {
            failure("Audio routing controls are not ready in this desktop session.")
        }
    }
}

fn gsettings_response(args: Vec<String>) -> BridgeResponse {
    if let Err(error) = validate_gsettings_args(&args) {
        return failure(error);
    }
    match bounded_command_output("gsettings", &args, GSETTINGS_TIMEOUT) {
        Ok(output) if output.status.success() => {
            success(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => failure(command_error_detail(&output.stderr, &output.stdout)),
        Err(BoundedCommandError::Missing) => {
            failure("gsettings is unavailable in this desktop session.")
        }
        Err(BoundedCommandError::TimedOut) => {
            failure("gsettings did not answer before the session bridge preference timeout.")
        }
        Err(BoundedCommandError::Failed) => {
            failure("gsettings could not run in this desktop session.")
        }
    }
}

fn open_preview_response(path: &str, kind: &str) -> BridgeResponse {
    let path = PathBuf::from(path.trim());
    if let Err(error) = validate_preview_open(&path, kind) {
        return failure(error);
    }
    match Command::new("xdg-open")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => success("opened".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            failure("xdg-open is unavailable in this desktop session.")
        }
        Err(_) => failure("the desktop session could not open that preview file."),
    }
}

fn permission_store_delete_response(table: &str, id: &str, app: &str) -> BridgeResponse {
    if let Err(error) = validate_permission_store_delete(table, id, app) {
        return failure(error);
    }
    match Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            PERMISSION_STORE_DEST,
            "--object-path",
            PERMISSION_STORE_PATH,
            "--method",
            PERMISSION_STORE_DELETE_PERMISSION,
            table,
            id,
            app,
        ])
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            success(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => failure(command_error_detail(&output.stderr, &output.stdout)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            failure("gdbus is unavailable in this desktop session.")
        }
        Err(_) => failure("the desktop session could not update app permissions."),
    }
}

fn display_config_get_current_state_response() -> BridgeResponse {
    gdbus_session_response(
        &[
            "call",
            "--session",
            "--dest",
            MUTTER_DISPLAY_CONFIG_DEST,
            "--object-path",
            MUTTER_DISPLAY_CONFIG_PATH,
            "--method",
            MUTTER_DISPLAY_CONFIG_GET_CURRENT_STATE,
        ],
        "gdbus is unavailable in this desktop session.",
        "the desktop session could not read display configuration.",
    )
}

fn display_config_apply_allowed_response() -> BridgeResponse {
    gdbus_session_response(
        &[
            "call",
            "--session",
            "--dest",
            MUTTER_DISPLAY_CONFIG_DEST,
            "--object-path",
            MUTTER_DISPLAY_CONFIG_PATH,
            "--method",
            "org.freedesktop.DBus.Properties.Get",
            MUTTER_DISPLAY_CONFIG_DEST,
            "ApplyMonitorsConfigAllowed",
        ],
        "gdbus is unavailable in this desktop session.",
        "the desktop session could not read display apply permission.",
    )
}

fn display_config_apply_monitors_response(
    serial: u32,
    method: u32,
    logical_monitors: &[DisplayConfigLogicalMonitor],
) -> BridgeResponse {
    if serial == 0 {
        return failure("Display changes require the current compositor serial.");
    }
    if method > 2 {
        return failure("Display apply method must be verify, temporary, or persistent.");
    }
    if let Err(error) = validate_display_config_logical_monitors(logical_monitors) {
        return failure(error);
    }
    let serial = serial.to_string();
    let method = method.to_string();
    let logical_monitors = encode_display_config_logical_monitors(logical_monitors);
    gdbus_session_response(
        &[
            "call",
            "--session",
            "--dest",
            MUTTER_DISPLAY_CONFIG_DEST,
            "--object-path",
            MUTTER_DISPLAY_CONFIG_PATH,
            "--method",
            MUTTER_DISPLAY_CONFIG_APPLY_MONITORS,
            &serial,
            &method,
            &logical_monitors,
            "{}",
        ],
        "gdbus is unavailable in this desktop session.",
        "the desktop session could not apply display configuration.",
    )
}

fn gdbus_session_response(
    args: &[&str],
    missing_message: &'static str,
    generic_message: &'static str,
) -> BridgeResponse {
    match Command::new("gdbus")
        .args(args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            success(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => failure(command_error_detail(&output.stderr, &output.stdout)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => failure(missing_message),
        Err(_) => failure(generic_message),
    }
}

fn validate_wpctl_args(args: &[String]) -> Result<(), String> {
    match args {
        [command] if command == "status" => Ok(()),
        [command, target] if command == "get-volume" => validate_wpctl_default_target(target),
        [command, target, volume] if command == "set-volume" => {
            validate_wpctl_default_target(target)?;
            validate_wpctl_volume(volume)
        }
        [command, target, muted] if command == "set-mute" => {
            validate_wpctl_default_target(target)?;
            if muted == "0" || muted == "1" {
                Ok(())
            } else {
                Err("wpctl mute writes must be encoded as 0 or 1.".to_string())
            }
        }
        [command, device_id] if command == "set-default" => validate_wpctl_numeric_id(device_id),
        _ => Err("unsupported session bridge wpctl operation.".to_string()),
    }
}

fn validate_wpctl_default_target(target: &str) -> Result<(), String> {
    if target == DEFAULT_SINK || target == DEFAULT_SOURCE {
        Ok(())
    } else {
        Err("wpctl audio target must be the default sink or source token.".to_string())
    }
}

fn validate_wpctl_volume(volume: &str) -> Result<(), String> {
    let parsed = volume
        .parse::<f64>()
        .map_err(|_| "wpctl volume must be a bounded decimal value.".to_string())?;
    if parsed.is_finite() && (0.0..=1.5).contains(&parsed) {
        Ok(())
    } else {
        Err("wpctl volume must stay between 0% and 150%.".to_string())
    }
}

fn validate_wpctl_numeric_id(device_id: &str) -> Result<(), String> {
    if !device_id.is_empty()
        && device_id.len() <= 12
        && device_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        Ok(())
    } else {
        Err("wpctl default-device writes require a reported numeric device id.".to_string())
    }
}

fn validate_gsettings_args(args: &[String]) -> Result<(), String> {
    match args {
        [command] if command == "list-schemas" => Ok(()),
        [command, schema] if command == "list-keys" => validate_list_keys_schema(schema),
        [command, schema] if command == "list-recursively" => {
            validate_list_recursively_schema(schema)
        }
        [command, schema, key] if command == "get" => validate_schema_key(schema, key),
        [command, schema, key] if command == "reset" => {
            let (base_schema, path) = validate_schema_arg(schema)?;
            if !path.is_empty() {
                return Err("gsettings reset is not allowed for path-scoped schemas.".to_string());
            }
            if base_schema != WM_SCHEMA {
                return Err(
                    "gsettings reset is only allowed for Goblins-owned shortcuts.".to_string(),
                );
            }
            validate_schema_key(schema, key)
        }
        [command, schema, key, value] if command == "set" => {
            validate_schema_key(schema, key)?;
            validate_gsettings_value(value)
        }
        _ => Err("unsupported session bridge gsettings operation.".to_string()),
    }
}

fn validate_schema_key(schema_arg: &str, key: &str) -> Result<(), String> {
    let (schema, path) = validate_schema_arg(schema_arg)?;
    let allowed = match schema {
        KEYBOARD_SCHEMA => KEYBOARD_KEYS,
        MOUSE_SCHEMA => MOUSE_KEYS,
        TOUCHPAD_SCHEMA => TOUCHPAD_KEYS,
        INPUT_SOURCES_SCHEMA => INPUT_SOURCE_KEYS,
        INTERFACE_SCHEMA => INTERFACE_KEYS,
        A11Y_APPS_SCHEMA => A11Y_APPS_KEYS,
        A11Y_INTERFACE_SCHEMA => A11Y_INTERFACE_KEYS,
        A11Y_KEYBOARD_SCHEMA => A11Y_KEYBOARD_KEYS,
        A11Y_MAGNIFIER_SCHEMA => A11Y_MAGNIFIER_KEYS,
        A11Y_MOUSE_SCHEMA => A11Y_MOUSE_KEYS,
        COLOR_SCHEMA => COLOR_KEYS,
        FOCUS_SCHEMA => FOCUS_KEYS,
        NOTIFICATIONS_SCHEMA => NOTIFICATION_KEYS,
        NOTIFICATION_APPLICATION_SCHEMA => NOTIFICATION_APPLICATION_KEYS,
        SOUND_SCHEMA => SOUND_KEYS,
        WM_SCHEMA => WM_KEYS,
        _ => return Err(format!("{schema} is not an allowlisted session schema.")),
    };
    if !allowed.iter().any(|candidate| candidate == &key) {
        return Err(format!(
            "{schema}.{key} is not allowlisted for the session bridge."
        ));
    }
    if schema != NOTIFICATION_APPLICATION_SCHEMA && !path.is_empty() {
        return Err(
            "path-scoped gsettings access is only allowed for notification applications."
                .to_string(),
        );
    }
    Ok(())
}

fn validate_list_keys_schema(schema: &str) -> Result<(), String> {
    if schema.contains(':') {
        return Err("gsettings list-keys must use an allowlisted base schema.".to_string());
    }
    let (base_schema, path) = validate_schema_arg_for_list_keys(schema)?;
    if !path.is_empty() {
        return Err("gsettings list-keys must not use path-scoped access.".to_string());
    }
    match base_schema {
        KEYBOARD_SCHEMA
        | MOUSE_SCHEMA
        | TOUCHPAD_SCHEMA
        | INPUT_SOURCES_SCHEMA
        | INTERFACE_SCHEMA
        | A11Y_APPS_SCHEMA
        | A11Y_INTERFACE_SCHEMA
        | A11Y_KEYBOARD_SCHEMA
        | A11Y_MAGNIFIER_SCHEMA
        | A11Y_MOUSE_SCHEMA
        | COLOR_SCHEMA
        | FOCUS_SCHEMA
        | NOTIFICATIONS_SCHEMA
        | NOTIFICATION_APPLICATION_SCHEMA
        | SOUND_SCHEMA
        | WM_SCHEMA => Ok(()),
        _ => Err(format!(
            "{base_schema} is not an allowlisted session schema."
        )),
    }
}

fn validate_list_recursively_schema(schema: &str) -> Result<(), String> {
    validate_list_keys_schema(schema)
        .map_err(|error| error.replace("list-keys", "list-recursively"))
}

fn validate_schema_arg_for_list_keys(schema_arg: &str) -> Result<(&str, &str), String> {
    let (schema, path) = schema_arg.split_once(':').unwrap_or((schema_arg, ""));
    if schema.trim() != schema || schema.is_empty() {
        return Err("session bridge schema names must be exact.".to_string());
    }
    Ok((schema, path))
}

fn validate_schema_arg(schema_arg: &str) -> Result<(&str, &str), String> {
    let (schema, path) = schema_arg.split_once(':').unwrap_or((schema_arg, ""));
    if schema.trim() != schema || schema.is_empty() {
        return Err("session bridge schema names must be exact.".to_string());
    }
    match schema {
        KEYBOARD_SCHEMA
        | MOUSE_SCHEMA
        | TOUCHPAD_SCHEMA
        | INPUT_SOURCES_SCHEMA
        | INTERFACE_SCHEMA
        | A11Y_APPS_SCHEMA
        | A11Y_INTERFACE_SCHEMA
        | A11Y_KEYBOARD_SCHEMA
        | A11Y_MAGNIFIER_SCHEMA
        | A11Y_MOUSE_SCHEMA
        | COLOR_SCHEMA
        | FOCUS_SCHEMA
        | NOTIFICATIONS_SCHEMA
        | SOUND_SCHEMA
        | WM_SCHEMA => {
            if path.is_empty() {
                Ok((schema, path))
            } else {
                Err("this session schema does not support path-scoped access.".to_string())
            }
        }
        NOTIFICATION_APPLICATION_SCHEMA => {
            if notification_path_is_valid(path) {
                Ok((schema, path))
            } else {
                Err("notification application path is not allowlisted.".to_string())
            }
        }
        _ => Err(format!("{schema} is not an allowlisted session schema.")),
    }
}

fn notification_path_is_valid(path: &str) -> bool {
    let Some(child) = path
        .strip_prefix(NOTIFICATION_APPLICATION_BASE_PATH)
        .and_then(|value| value.strip_suffix('/'))
    else {
        return false;
    };
    !child.is_empty()
        && child.len() <= 240
        && child
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn validate_gsettings_value(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 16 * 1024 || value.contains('\0') {
        return Err("gsettings values must be non-empty, bounded, and NUL-free.".to_string());
    }
    if value.chars().any(|ch| ch.is_control() && ch != '\t') {
        return Err("gsettings values cannot contain control characters.".to_string());
    }
    Ok(())
}

fn validate_preview_open(path: &Path, kind: &str) -> Result<(), String> {
    if kind != "pdf" && kind != "image" {
        return Err("Preview opens only allowlisted PDF or image files.".to_string());
    }
    if !path.is_absolute() || !path.is_file() {
        return Err("Preview paths must be existing absolute regular files.".to_string());
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .ok_or_else(|| "Preview files must have a supported extension.".to_string())?;
    let allowed = match kind {
        "pdf" => matches!(extension.as_str(), "pdf" | "ps"),
        "image" => matches!(
            extension.as_str(),
            "bmp" | "gif" | "jpeg" | "jpg" | "png" | "tif" | "tiff" | "webp"
        ),
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err("Preview file extension does not match the requested preview kind.".to_string())
    }
}

fn validate_permission_store_delete(table: &str, id: &str, app: &str) -> Result<(), String> {
    if !matches!(table, "location" | "background" | "notifications") {
        return Err("PermissionStore deletes are limited to app-keyed tables.".to_string());
    }
    if !permission_id_is_safe(id) || !permission_id_is_safe(app) {
        return Err("PermissionStore app and resource ids must be safe desktop ids.".to_string());
    }
    if id != app {
        return Err(
            "PermissionStore deletes from Settings must target app-keyed grants.".to_string(),
        );
    }
    Ok(())
}

fn validate_display_config_logical_monitors(
    monitors: &[DisplayConfigLogicalMonitor],
) -> Result<(), String> {
    if monitors.is_empty() {
        return Err("At least one logical monitor is required.".to_string());
    }
    if monitors.len() > 8 {
        return Err("Display layout changes are limited to eight logical monitors.".to_string());
    }
    let primary_count = monitors.iter().filter(|monitor| monitor.primary).count();
    if primary_count != 1 {
        return Err("Exactly one logical monitor must be primary.".to_string());
    }
    let mut seen_connectors = std::collections::HashSet::new();
    for monitor in monitors {
        if !(-65535..=65535).contains(&monitor.x) || !(-65535..=65535).contains(&monitor.y) {
            return Err("Display positions must stay within compositor layout bounds.".to_string());
        }
        if !monitor.scale.is_finite() || monitor.scale < 1.0 || monitor.scale > 4.0 {
            return Err("Display scale must be between 1.0 and 4.0.".to_string());
        }
        if monitor.transform > 7 {
            return Err(
                "Display transform must be a Wayland transform value from 0 through 7.".to_string(),
            );
        }
        if monitor.monitors.is_empty() {
            return Err("Each logical monitor needs at least one physical monitor.".to_string());
        }
        if monitor.monitors.len() > 4 {
            return Err("Each logical monitor is limited to four mirrored outputs.".to_string());
        }
        for physical in &monitor.monitors {
            if !display_connector_is_safe(&physical.connector) {
                return Err("Display connector names must be safe desktop IDs.".to_string());
            }
            if !display_mode_id_is_safe(&physical.mode_id) {
                return Err("Display mode IDs must be safe compositor mode IDs.".to_string());
            }
            if !seen_connectors.insert(physical.connector.clone()) {
                return Err(
                    "A physical display can appear in only one logical monitor.".to_string()
                );
            }
        }
    }
    Ok(())
}

fn encode_display_config_logical_monitors(monitors: &[DisplayConfigLogicalMonitor]) -> String {
    let encoded = monitors
        .iter()
        .map(|monitor| {
            let physical = monitor
                .monitors
                .iter()
                .map(|physical| {
                    format!(
                        "('{}', '{}', {{}})",
                        escape_gvariant_string(&physical.connector),
                        escape_gvariant_string(&physical.mode_id)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "({}, {}, {}, uint32 {}, {}, [{}])",
                monitor.x,
                monitor.y,
                encode_display_scale(monitor.scale),
                monitor.transform,
                if monitor.primary { "true" } else { "false" },
                physical
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{encoded}]")
}

fn display_connector_is_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn display_mode_id_is_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'@'))
}

fn encode_display_scale(scale: f64) -> String {
    let mut text = format!("{scale:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.push('0');
    }
    text
}

fn escape_gvariant_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn permission_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

enum BoundedCommandError {
    Missing,
    TimedOut,
    Failed,
}

fn bounded_command_output(
    binary: &str,
    args: &[String],
    timeout: Duration,
) -> Result<Output, BoundedCommandError> {
    let mut command = Command::new(binary);
    command.args(args);
    bounded_output_of(command, timeout)
}

fn bounded_output_of(
    mut command: Command,
    timeout: Duration,
) -> Result<Output, BoundedCommandError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BoundedCommandError::Missing
            } else {
                BoundedCommandError::Failed
            }
        })?;
    // Drain both pipes on background threads while the child runs: a kernel
    // pipe buffer holds ~64KB, so polling `try_wait` without reading would
    // block any chattier child on its own write and falsely kill it at the
    // bound. Captured output is capped so a runaway child cannot exhaust
    // memory inside its window; the overflow is drained and discarded.
    let stdout_reader = spawn_capped_drain(child.stdout.take());
    let stderr_reader = spawn_capped_drain(child.stderr.take());
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Do NOT join the drain threads here: a killed child may
                    // leave a grandchild holding the pipe write end open, and
                    // joining would block this thread until that grandchild
                    // exits. The detached drains cost at most the output cap
                    // each and end when the pipes finally close.
                    drop(stdout_reader);
                    drop(stderr_reader);
                    return Err(BoundedCommandError::TimedOut);
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout_reader);
                drop(stderr_reader);
                return Err(BoundedCommandError::Failed);
            }
        }
    };

    Ok(Output {
        status,
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

fn bounded_input_output_of(
    mut command: Command,
    input: Vec<u8>,
    timeout: Duration,
) -> Result<Output, BoundedCommandError> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BoundedCommandError::Missing
            } else {
                BoundedCommandError::Failed
            }
        })?;
    let mut stdin = child.stdin.take().ok_or(BoundedCommandError::Failed)?;
    let input_writer = thread::spawn(move || stdin.write_all(&input).is_ok());
    let stdout_reader = spawn_capped_drain(child.stdout.take());
    let stderr_reader = spawn_capped_drain(child.stderr.take());
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    drop(input_writer);
                    drop(stdout_reader);
                    drop(stderr_reader);
                    return Err(BoundedCommandError::TimedOut);
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(input_writer);
                drop(stdout_reader);
                drop(stderr_reader);
                return Err(BoundedCommandError::Failed);
            }
        }
    };
    if !input_writer.join().unwrap_or(false) {
        drop(stdout_reader);
        drop(stderr_reader);
        return Err(BoundedCommandError::Failed);
    }
    Ok(Output {
        status,
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

const CAPTURED_OUTPUT_CAP_BYTES: u64 = 8 * 1024 * 1024;

fn spawn_capped_drain<R>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let Some(pipe) = pipe else {
            return Vec::new();
        };
        let mut limited = pipe.take(CAPTURED_OUTPUT_CAP_BYTES);
        let mut captured = Vec::new();
        let _ = limited.read_to_end(&mut captured);
        let _ = std::io::copy(&mut limited.into_inner(), &mut std::io::sink());
        captured
    })
}

fn write_response(stream: &mut UnixStream, response: &BridgeResponse) -> Result<(), String> {
    let json = serde_json::to_vec(response).map_err(|error| error.to_string())?;
    let deadline = Instant::now() + STREAM_RESPONSE_TIMEOUT;
    write_all_before(stream, &json, deadline).map_err(|error| error.to_string())?;
    write_all_before(stream, b"\n", deadline).map_err(|error| error.to_string())
}

fn write_all_before(
    stream: &mut UnixStream,
    mut bytes: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining_before(deadline)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "bridge closed")),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn read_to_end_before(
    stream: &mut UnixStream,
    limit: usize,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        stream.set_read_timeout(Some(remaining_before(deadline)?))?;
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(bytes),
            Ok(read) => {
                if bytes.len().saturating_add(read) > limit {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "bridge request exceeded its fixed size limit",
                    ));
                }
                bytes.extend_from_slice(&buffer[..read]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn remaining_before(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "bridge deadline elapsed"))
}

fn socket_path() -> PathBuf {
    env::var_os("GOBLINS_OS_SESSION_BRIDGE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET))
}

fn prepare_socket_parent(socket: &Path) -> Result<(), String> {
    let parent = socket
        .parent()
        .ok_or_else(|| "session bridge socket needs a parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    set_group_mode(parent, 0o770)
}

fn remove_stale_socket(socket: &Path) -> Result<(), String> {
    if !socket.exists() {
        return Ok(());
    }
    let metadata = fs::metadata(socket)
        .map_err(|error| format!("could not inspect {}: {error}", socket.display()))?;
    if !metadata.file_type().is_socket() {
        return Err(format!(
            "refusing to replace non-socket {}",
            socket.display()
        ));
    }
    fs::remove_file(socket).map_err(|error| format!("could not remove stale socket: {error}"))
}

fn set_group_mode(path: &Path, mode: u32) -> Result<(), String> {
    let _ = Command::new("chgrp")
        .args([SOCKET_GROUP, path.to_string_lossy().as_ref()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("could not chmod {}: {error}", path.display()))
}

fn command_error_detail(stderr: &[u8], stdout: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    if stdout.is_empty() {
        "session command failed without a message.".to_string()
    } else {
        stdout
    }
}

fn success(stdout: String) -> BridgeResponse {
    BridgeResponse {
        ok: true,
        stdout,
        detail: String::new(),
    }
}

fn failure(detail: impl Into<String>) -> BridgeResponse {
    BridgeResponse {
        ok: false,
        stdout: String::new(),
        detail: detail.into(),
    }
}

fn self_test() -> Result<(), String> {
    validate_gsettings_args(&["list-schemas".to_string()])?;
    validate_gsettings_args(&["list-keys".to_string(), INPUT_SOURCES_SCHEMA.to_string()])?;
    validate_gsettings_args(&["list-recursively".to_string(), SOUND_SCHEMA.to_string()])?;
    validate_gsettings_args(&[
        "set".to_string(),
        INPUT_SOURCES_SCHEMA.to_string(),
        "sources".to_string(),
        "[('xkb', 'us')]".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        FOCUS_SCHEMA.to_string(),
        "active-mode".to_string(),
        "'work'".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        INTERFACE_SCHEMA.to_string(),
        "enable-animations".to_string(),
        "false".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        KEYBOARD_SCHEMA.to_string(),
        "repeat".to_string(),
        "true".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        MOUSE_SCHEMA.to_string(),
        "speed".to_string(),
        "0.25".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        TOUCHPAD_SCHEMA.to_string(),
        "tap-to-click".to_string(),
        "true".to_string(),
    ])?;
    validate_gsettings_args(&[
        "get".to_string(),
        SOUND_SCHEMA.to_string(),
        "event-sounds".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        SOUND_SCHEMA.to_string(),
        "allow-volume-above-100-percent".to_string(),
        "false".to_string(),
    ])?;
    validate_gsettings_args(&[
        "get".to_string(),
        format!("{NOTIFICATION_APPLICATION_SCHEMA}:{NOTIFICATION_APPLICATION_BASE_PATH}org-gnome-Console/"),
        "enable".to_string(),
    ])?;
    validate_gsettings_args(&[
        "reset".to_string(),
        WM_SCHEMA.to_string(),
        "window-hud".to_string(),
    ])?;
    validate_permission_store_delete(
        "location",
        "org.goblins.GatePrivacyProof",
        "org.goblins.GatePrivacyProof",
    )?;
    validate_wpctl_args(&["status".to_string()])?;
    validate_wpctl_args(&["get-volume".to_string(), DEFAULT_SINK.to_string()])?;
    validate_wpctl_args(&[
        "set-volume".to_string(),
        DEFAULT_SOURCE.to_string(),
        "0.62".to_string(),
    ])?;
    validate_wpctl_args(&[
        "set-mute".to_string(),
        DEFAULT_SINK.to_string(),
        "1".to_string(),
    ])?;
    validate_wpctl_args(&["set-default".to_string(), "42".to_string()])?;
    validate_display_config_logical_monitors(&[DisplayConfigLogicalMonitor {
        x: 0,
        y: 0,
        scale: 1.25,
        transform: 0,
        primary: true,
        monitors: vec![DisplayConfigMonitor {
            connector: "eDP-1".to_string(),
            mode_id: "2560x1440@60.000".to_string(),
        }],
    }])?;
    if validate_display_config_logical_monitors(&[DisplayConfigLogicalMonitor {
        x: 0,
        y: 0,
        scale: 0.5,
        transform: 0,
        primary: true,
        monitors: vec![DisplayConfigMonitor {
            connector: "eDP-1".to_string(),
            mode_id: "2560x1440@60.000".to_string(),
        }],
    }])
    .is_ok()
    {
        return Err("unsafe display scale was accepted.".to_string());
    }
    if validate_gsettings_args(&[
        "set".to_string(),
        "org.gnome.desktop.background".to_string(),
        "picture-uri".to_string(),
        "'file:///tmp/x'".to_string(),
    ])
    .is_ok()
    {
        return Err("non-allowlisted schema was accepted.".to_string());
    }
    if validate_gsettings_args(&[
        "set".to_string(),
        WM_SCHEMA.to_string(),
        "window-hud".to_string(),
        "bad\nvalue".to_string(),
    ])
    .is_ok()
    {
        return Err("control-character gsettings value was accepted.".to_string());
    }
    for request in [
        r#"{"op":"voice-audio-status"}"#,
        r#"{"op":"voice-capture"}"#,
        r#"{"op":"voice-playback","wav_base64":"UklGRg=="}"#,
    ] {
        serde_json::from_str::<BridgeRequest>(request)
            .map_err(|_| "typed voice bridge operation was rejected.".to_string())?;
    }
    for request in [
        r#"{"op":"voice-audio-status","device":"42"}"#,
        r#"{"op":"voice-capture","path":"/tmp/mic.wav"}"#,
        r#"{"op":"voice-playback","path":"/tmp/reply.wav","wav_base64":"UklGRg=="}"#,
        r#"{"op":"voice-playback","command":"sh","wav_base64":"UklGRg=="}"#,
    ] {
        if serde_json::from_str::<BridgeRequest>(request).is_ok() {
            return Err("voice bridge accepted an untyped command, device, or path.".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::CString,
        fs,
        io::Write,
        os::unix::{
            ffi::OsStrExt,
            fs::{symlink, MetadataExt, PermissionsExt},
        },
        path::Path,
        time::{Duration, Instant},
    };

    use super::{
        effective_user_id, encode_display_config_logical_monitors, handle_stream, monotonic_now_ns,
        read_text_shortcuts_runtime_status, read_to_end_before, valid_pcm_wave,
        validate_display_config_logical_monitors, validate_gsettings_args,
        validate_permission_store_delete, validate_schema_arg,
        validate_text_shortcuts_runtime_status, validate_text_shortcuts_runtime_status_metadata,
        validate_wpctl_args, BridgeRequest, DisplayConfigLogicalMonitor, DisplayConfigMonitor,
        PcmFormat, TextShortcutsRuntimeStatus, DEFAULT_SINK, DEFAULT_SOURCE, FOCUS_SCHEMA,
        INPUT_SOURCES_SCHEMA, KEYBOARD_SCHEMA, MAX_CAPTURE_DURATION_SECONDS,
        MAX_PLAYBACK_DURATION_SECONDS, MAX_REQUEST_BYTES, MOUSE_SCHEMA,
        NOTIFICATION_APPLICATION_BASE_PATH, NOTIFICATION_APPLICATION_SCHEMA, SOUND_SCHEMA,
        TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS, TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES,
        TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS, TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA,
        TOUCHPAD_SCHEMA, WM_SCHEMA,
    };

    fn pcm_wave(format: PcmFormat, data_bytes: usize) -> Vec<u8> {
        assert!(data_bytes <= u32::MAX as usize);
        let padded_data_bytes = data_bytes + (data_bytes & 1);
        let riff_size = 36usize + padded_data_bytes;
        let mut wav = Vec::with_capacity(riff_size + 8);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(riff_size as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&format.channels.to_le_bytes());
        wav.extend_from_slice(&format.sample_rate.to_le_bytes());
        wav.extend_from_slice(&format.byte_rate.to_le_bytes());
        wav.extend_from_slice(&format.block_align.to_le_bytes());
        wav.extend_from_slice(&format.bits_per_sample.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data_bytes as u32).to_le_bytes());
        wav.resize(44 + padded_data_bytes, 0);
        wav
    }

    fn mono_16khz_format() -> PcmFormat {
        PcmFormat {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            block_align: 2,
            byte_rate: 32_000,
        }
    }

    #[test]
    fn voice_protocol_is_typed_and_rejects_command_device_and_path_fields() {
        assert!(matches!(
            serde_json::from_str::<BridgeRequest>(r#"{"op":"voice-audio-status"}"#).unwrap(),
            BridgeRequest::VoiceAudioStatus {}
        ));
        assert!(matches!(
            serde_json::from_str::<BridgeRequest>(r#"{"op":"voice-capture"}"#).unwrap(),
            BridgeRequest::VoiceCapture {}
        ));
        assert!(matches!(
            serde_json::from_str::<BridgeRequest>(
                r#"{"op":"voice-playback","wav_base64":"UklGRg=="}"#
            )
            .unwrap(),
            BridgeRequest::VoicePlayback { .. }
        ));
        for request in [
            r#"{"op":"voice-audio-status","device":"42"}"#,
            r#"{"op":"voice-capture","path":"/tmp/mic.wav"}"#,
            r#"{"op":"voice-capture","seconds":60}"#,
            r#"{"op":"voice-playback","path":"/tmp/reply.wav","wav_base64":"UklGRg=="}"#,
            r#"{"op":"voice-playback","command":"sh","wav_base64":"UklGRg=="}"#,
        ] {
            assert!(serde_json::from_str::<BridgeRequest>(request).is_err());
        }
    }

    #[test]
    fn canonical_pcm_wave_enforces_format_and_duration_bounds() {
        let format = mono_16khz_format();
        let one_second = pcm_wave(format, format.byte_rate as usize);
        assert!(valid_pcm_wave(
            &one_second,
            MAX_CAPTURE_DURATION_SECONDS,
            Some(format),
        ));
        assert!(valid_pcm_wave(
            &one_second,
            MAX_PLAYBACK_DURATION_SECONDS,
            None,
        ));

        let too_long = pcm_wave(
            format,
            format.byte_rate as usize * (MAX_CAPTURE_DURATION_SECONDS as usize + 1),
        );
        assert!(!valid_pcm_wave(
            &too_long,
            MAX_CAPTURE_DURATION_SECONDS,
            Some(format),
        ));

        for invalid_format in [
            PcmFormat {
                channels: 3,
                block_align: 6,
                byte_rate: 96_000,
                ..format
            },
            PcmFormat {
                sample_rate: 1_000,
                byte_rate: 2_000,
                ..format
            },
            PcmFormat {
                bits_per_sample: 8,
                block_align: 1,
                byte_rate: 16_000,
                ..format
            },
            PcmFormat {
                block_align: 4,
                byte_rate: 64_000,
                ..format
            },
        ] {
            assert!(!valid_pcm_wave(
                &pcm_wave(invalid_format, invalid_format.byte_rate as usize),
                MAX_PLAYBACK_DURATION_SECONDS,
                None,
            ));
        }
    }

    #[test]
    fn canonical_pcm_wave_rejects_duplicate_extended_and_trailing_chunks() {
        let format = mono_16khz_format();
        let canonical = pcm_wave(format, format.byte_rate as usize);

        let mut duplicate_format = canonical.clone();
        duplicate_format.splice(36..36, canonical[12..36].iter().copied());
        let size = (duplicate_format.len() - 8) as u32;
        duplicate_format[4..8].copy_from_slice(&size.to_le_bytes());
        assert!(!valid_pcm_wave(
            &duplicate_format,
            MAX_PLAYBACK_DURATION_SECONDS,
            None,
        ));

        let mut duplicate_data = canonical.clone();
        duplicate_data.extend_from_slice(b"data");
        duplicate_data.extend_from_slice(&2u32.to_le_bytes());
        duplicate_data.extend_from_slice(&[0, 0]);
        let size = (duplicate_data.len() - 8) as u32;
        duplicate_data[4..8].copy_from_slice(&size.to_le_bytes());
        assert!(!valid_pcm_wave(
            &duplicate_data,
            MAX_PLAYBACK_DURATION_SECONDS,
            None,
        ));

        let mut extended_format = canonical.clone();
        extended_format[16..20].copy_from_slice(&18u32.to_le_bytes());
        assert!(!valid_pcm_wave(
            &extended_format,
            MAX_PLAYBACK_DURATION_SECONDS,
            None,
        ));

        let mut trailing = canonical;
        trailing.extend_from_slice(b"JUNK\0\0\0\0");
        let size = (trailing.len() - 8) as u32;
        trailing[4..8].copy_from_slice(&size.to_le_bytes());
        assert!(!valid_pcm_wave(
            &trailing,
            MAX_PLAYBACK_DURATION_SECONDS,
            None,
        ));
    }

    #[test]
    fn bridge_read_deadline_rejects_trickle_input_by_total_wall_time() {
        let (mut writer, mut reader) = std::os::unix::net::UnixStream::pair().unwrap();
        let sender = std::thread::spawn(move || {
            for _ in 0..6 {
                if writer.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        });
        let started = Instant::now();
        let result = read_to_end_before(&mut reader, 64, started + Duration::from_millis(110));
        let elapsed = started.elapsed();
        assert!(result.is_err());
        assert!(elapsed >= Duration::from_millis(90));
        assert!(elapsed < Duration::from_millis(500));
        sender.join().unwrap();
    }

    fn runtime_status(monotonic_ns: u64) -> TextShortcutsRuntimeStatus {
        TextShortcutsRuntimeStatus {
            schema: TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA.to_string(),
            instance_id: "0123456789abcdef0123456789abcdef".to_string(),
            focus_generation: 7,
            runtime_generation: 11,
            sequence: 13,
            monotonic_ns,
            focused: true,
            enabled: true,
            surrounding_text_supported: true,
            snapshot_valid: true,
            child_alive: true,
            last_response_ok: true,
        }
    }

    fn write_runtime_status(path: &Path, encoded: &[u8]) {
        fs::write(path, encoded).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn write_json_runtime_status(path: &Path, status: &TextShortcutsRuntimeStatus) {
        write_runtime_status(path, &serde_json::to_vec(status).unwrap());
    }

    #[test]
    fn text_shortcuts_runtime_status_protocol_is_fixed_pathless_and_strict() {
        let request =
            serde_json::from_str::<BridgeRequest>(r#"{"op":"text-shortcuts-runtime-status"}"#)
                .expect("fixed runtime status operation");
        assert!(matches!(
            request,
            BridgeRequest::TextShortcutsRuntimeStatus {}
        ));
        for request in [
            r#"{"op":"text-shortcuts-runtime-status","path":"/tmp/status"}"#,
            r#"{"op":"text-shortcuts-runtime-status","raw":"{}"}"#,
        ] {
            assert!(serde_json::from_str::<BridgeRequest>(request).is_err());
        }
    }

    #[test]
    fn text_shortcuts_runtime_status_accepts_fresh_owner_only_regular_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("status.json");
        let now_ns = 20_000_000_000;
        let status = runtime_status(now_ns - 1);
        write_json_runtime_status(&path, &status);

        assert_eq!(
            read_text_shortcuts_runtime_status(&path, now_ns, effective_user_id()).unwrap(),
            status
        );
        assert!(monotonic_now_ns().unwrap() > 0);
    }

    #[test]
    fn text_shortcuts_runtime_status_rejects_stale_and_materially_future_timestamps() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("status.json");
        let now_ns = 20_000_000_000;

        let stale = runtime_status(now_ns - TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS - 1);
        write_json_runtime_status(&path, &stale);
        assert!(
            read_text_shortcuts_runtime_status(&path, now_ns, effective_user_id())
                .unwrap_err()
                .contains("stale")
        );

        let future = runtime_status(now_ns + TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS + 1);
        write_json_runtime_status(&path, &future);
        assert!(
            read_text_shortcuts_runtime_status(&path, now_ns, effective_user_id())
                .unwrap_err()
                .contains("future")
        );
    }

    #[test]
    fn text_shortcuts_runtime_status_accepts_exact_freshness_boundaries() {
        let now_ns = 20_000_000_000;
        assert!(validate_text_shortcuts_runtime_status(
            &runtime_status(now_ns - TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS),
            now_ns,
        )
        .is_ok());
        assert!(validate_text_shortcuts_runtime_status(
            &runtime_status(now_ns + TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS),
            now_ns,
        )
        .is_ok());
    }

    #[test]
    fn text_shortcuts_runtime_status_rejects_malformed_and_unknown_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("status.json");
        let now_ns = 20_000_000_000;

        write_runtime_status(&path, b"{");
        assert!(
            read_text_shortcuts_runtime_status(&path, now_ns, effective_user_id())
                .unwrap_err()
                .contains("strict v1 JSON")
        );

        let mut value = serde_json::to_value(runtime_status(now_ns)).unwrap();
        value["path"] = serde_json::json!("/tmp/spoof");
        write_runtime_status(&path, &serde_json::to_vec(&value).unwrap());
        assert!(
            read_text_shortcuts_runtime_status(&path, now_ns, effective_user_id())
                .unwrap_err()
                .contains("strict v1 JSON")
        );

        let mut value = serde_json::to_value(runtime_status(now_ns)).unwrap();
        value.as_object_mut().unwrap().remove("snapshot_valid");
        write_runtime_status(&path, &serde_json::to_vec(&value).unwrap());
        assert!(
            read_text_shortcuts_runtime_status(&path, now_ns, effective_user_id())
                .unwrap_err()
                .contains("strict v1 JSON")
        );
    }

    #[test]
    fn text_shortcuts_runtime_status_preserves_all_readiness_signals() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("status.json");
        let now_ns = 20_000_000_000;
        let mut status = runtime_status(now_ns - 1);
        status.focused = false;
        status.enabled = false;
        status.surrounding_text_supported = false;
        status.snapshot_valid = false;
        status.child_alive = false;
        status.last_response_ok = false;
        write_json_runtime_status(&path, &status);

        assert_eq!(
            read_text_shortcuts_runtime_status(&path, now_ns, effective_user_id()).unwrap(),
            status
        );
    }

    #[test]
    fn text_shortcuts_runtime_status_rejects_invalid_identity_and_zero_generations() {
        let now_ns = 20_000_000_000;
        let mut uppercase = runtime_status(now_ns);
        uppercase.instance_id = "0123456789ABCDEF0123456789ABCDEF".to_string();
        assert!(validate_text_shortcuts_runtime_status(&uppercase, now_ns).is_err());

        let mut zero_generation = runtime_status(now_ns);
        zero_generation.runtime_generation = 0;
        assert!(validate_text_shortcuts_runtime_status(&zero_generation, now_ns).is_err());
    }

    #[test]
    fn text_shortcuts_runtime_status_rejects_wrong_mode_and_owner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("status.json");
        let now_ns = 20_000_000_000;
        write_json_runtime_status(&path, &runtime_status(now_ns));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(
            read_text_shortcuts_runtime_status(&path, now_ns, effective_user_id())
                .unwrap_err()
                .contains("owner-only regular file")
        );

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert!(validate_text_shortcuts_runtime_status_metadata(
            &metadata,
            metadata.uid().wrapping_add(1),
        )
        .is_err());
    }

    #[test]
    fn text_shortcuts_runtime_status_rejects_symlinks_and_oversize_files() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.json");
        let link = directory.path().join("status.json");
        let now_ns = 20_000_000_000;
        write_json_runtime_status(&target, &runtime_status(now_ns));
        symlink(&target, &link).unwrap();
        assert!(read_text_shortcuts_runtime_status(&link, now_ns, effective_user_id()).is_err());

        fs::remove_file(&link).unwrap();
        write_runtime_status(
            &link,
            &vec![b' '; TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES + 1],
        );
        assert!(
            read_text_shortcuts_runtime_status(&link, now_ns, effective_user_id())
                .unwrap_err()
                .contains("size limit")
        );
    }

    #[test]
    fn text_shortcuts_runtime_status_rejects_fifo_without_blocking() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("status.json");
        let encoded_path = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: encoded_path is NUL-terminated and points to a valid pathname.
        assert_eq!(unsafe { libc::mkfifo(encoded_path.as_ptr(), 0o600) }, 0);

        let started = Instant::now();
        assert!(
            read_text_shortcuts_runtime_status(&path, 20_000_000_000, effective_user_id(),)
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn text_shortcuts_protocol_is_typed_pathless_and_strict() {
        let read = serde_json::from_str::<BridgeRequest>(r#"{"op":"text-shortcuts-read"}"#)
            .expect("fixed read operation");
        assert!(matches!(read, BridgeRequest::TextShortcutsRead {}));
        assert!(serde_json::from_str::<BridgeRequest>(
            r#"{"op":"text-shortcuts-read","path":"/tmp/table"}"#
        )
        .is_err());

        let write = serde_json::from_str::<BridgeRequest>(
            r#"{"op":"text-shortcuts-write","shortcuts":[{"replace":"omw","with":"on my way"}]}"#,
        )
        .expect("typed write operation");
        match write {
            BridgeRequest::TextShortcutsWrite { shortcuts } => {
                assert_eq!(shortcuts.len(), 1);
                assert_eq!(shortcuts[0].replace(), "omw");
                assert_eq!(shortcuts[0].with_text(), "on my way");
            }
            _ => panic!("expected typed Text Shortcuts write"),
        }
        for request in [
            r#"{"op":"text-shortcuts-write","path":"/tmp/table","shortcuts":[]}"#,
            r#"{"op":"text-shortcuts-write","raw":"[]","shortcuts":[]}"#,
            r#"{"op":"text-shortcuts-write","blob":"W10=","shortcuts":[]}"#,
        ] {
            assert!(serde_json::from_str::<BridgeRequest>(request).is_err());
        }
    }

    #[test]
    fn session_bridge_rejects_cap_plus_one_before_parsing() {
        let (mut writer, mut reader) = std::os::unix::net::UnixStream::pair().unwrap();
        let sender = std::thread::spawn(move || {
            use std::io::Write;
            writer
                .write_all(&vec![b'x'; MAX_REQUEST_BYTES + 1])
                .unwrap();
            writer.shutdown(std::net::Shutdown::Write).unwrap();
        });
        let response = handle_stream(&mut reader);
        sender.join().unwrap();
        assert!(!response.ok);
        assert!(response.stdout.is_empty());
        assert!(response.detail.contains("size limit"));
    }

    #[test]
    fn gsettings_allowlist_accepts_owned_session_keys() {
        assert!(validate_gsettings_args(&["list-schemas".to_string()]).is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            INPUT_SOURCES_SCHEMA.to_string(),
            "current".to_string(),
            "1".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            FOCUS_SCHEMA.to_string(),
            "modes".to_string(),
            "'[]'".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            KEYBOARD_SCHEMA.to_string(),
            "repeat-interval".to_string(),
            "uint32 30".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            MOUSE_SCHEMA.to_string(),
            "left-handed".to_string(),
            "false".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            TOUCHPAD_SCHEMA.to_string(),
            "disable-while-typing".to_string(),
            "true".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "get".to_string(),
            SOUND_SCHEMA.to_string(),
            "theme-name".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "list-recursively".to_string(),
            SOUND_SCHEMA.to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            SOUND_SCHEMA.to_string(),
            "event-sounds".to_string(),
            "false".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "reset".to_string(),
            WM_SCHEMA.to_string(),
            "mission-control".to_string(),
        ])
        .is_ok());
    }

    #[test]
    fn gsettings_allowlist_rejects_arbitrary_session_writes() {
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            "org.gnome.desktop.background".to_string(),
            "picture-uri".to_string(),
            "'file:///tmp/wallpaper'".to_string(),
        ])
        .is_err());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            INPUT_SOURCES_SCHEMA.to_string(),
            "sources".to_string(),
            "bad\nvalue".to_string(),
        ])
        .is_err());
    }

    #[test]
    fn wpctl_allowlist_accepts_audio_shapes() {
        assert!(validate_wpctl_args(&["status".to_string()]).is_ok());
        assert!(validate_wpctl_args(&["get-volume".to_string(), DEFAULT_SINK.to_string()]).is_ok());
        assert!(
            validate_wpctl_args(&["get-volume".to_string(), DEFAULT_SOURCE.to_string()]).is_ok()
        );
        assert!(validate_wpctl_args(&[
            "set-volume".to_string(),
            DEFAULT_SINK.to_string(),
            "1.50".to_string(),
        ])
        .is_ok());
        assert!(validate_wpctl_args(&[
            "set-mute".to_string(),
            DEFAULT_SOURCE.to_string(),
            "0".to_string(),
        ])
        .is_ok());
        assert!(validate_wpctl_args(&["set-default".to_string(), "58".to_string()]).is_ok());
    }

    #[test]
    fn wpctl_allowlist_rejects_arbitrary_commands() {
        assert!(validate_wpctl_args(&["inspect".to_string(), "0".to_string()]).is_err());
        assert!(validate_wpctl_args(&[
            "set-volume".to_string(),
            "42".to_string(),
            "0.5".to_string(),
        ])
        .is_err());
        assert!(validate_wpctl_args(&[
            "set-volume".to_string(),
            DEFAULT_SINK.to_string(),
            "2.0".to_string(),
        ])
        .is_err());
        assert!(validate_wpctl_args(&[
            "set-mute".to_string(),
            DEFAULT_SINK.to_string(),
            "muted".to_string(),
        ])
        .is_err());
        assert!(validate_wpctl_args(&["set-default".to_string(), "../58".to_string()]).is_err());
    }

    #[test]
    fn path_scoped_schema_is_only_for_notification_apps() {
        let path = format!(
            "{NOTIFICATION_APPLICATION_SCHEMA}:{NOTIFICATION_APPLICATION_BASE_PATH}org-gnome-Console/"
        );
        assert!(validate_schema_arg(&path).is_ok());
        assert!(validate_schema_arg(&format!(
            "{INPUT_SOURCES_SCHEMA}:{NOTIFICATION_APPLICATION_BASE_PATH}org-gnome-Console/"
        ))
        .is_err());
    }

    #[test]
    fn permission_store_delete_is_limited_to_app_keyed_safe_grants() {
        assert!(validate_permission_store_delete(
            "location",
            "org.goblins.GatePrivacyProof",
            "org.goblins.GatePrivacyProof",
        )
        .is_ok());
        assert!(validate_permission_store_delete(
            "devices",
            "camera",
            "org.goblins.GatePrivacyProof",
        )
        .is_err());
        assert!(validate_permission_store_delete(
            "location",
            "camera",
            "org.goblins.GatePrivacyProof",
        )
        .is_err());
        assert!(validate_permission_store_delete(
            "location",
            "org.goblins.GatePrivacyProof;rm",
            "org.goblins.GatePrivacyProof;rm",
        )
        .is_err());
    }

    #[test]
    fn display_config_bridge_is_limited_to_safe_monitor_layouts() {
        let valid = vec![DisplayConfigLogicalMonitor {
            x: 0,
            y: 0,
            scale: 1.25,
            transform: 0,
            primary: true,
            monitors: vec![DisplayConfigMonitor {
                connector: "eDP-1".to_string(),
                mode_id: "2560x1440@60.000".to_string(),
            }],
        }];
        assert!(validate_display_config_logical_monitors(&valid).is_ok());
        assert_eq!(
            encode_display_config_logical_monitors(&valid),
            "[(0, 0, 1.25, uint32 0, true, [('eDP-1', '2560x1440@60.000', {})])]"
        );

        let mut duplicate = valid.clone();
        duplicate.push(DisplayConfigLogicalMonitor {
            x: 2560,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: false,
            monitors: vec![DisplayConfigMonitor {
                connector: "eDP-1".to_string(),
                mode_id: "2560x1440@60.000".to_string(),
            }],
        });
        assert!(validate_display_config_logical_monitors(&duplicate).is_err());

        let mut unsafe_mode = valid;
        unsafe_mode[0].monitors[0].mode_id = "2560x1440;rm".to_string();
        assert!(validate_display_config_logical_monitors(&unsafe_mode).is_err());
    }
}
