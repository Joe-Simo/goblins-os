use std::{
    env,
    ffi::CString,
    io::{self, Read, Write},
    net::Shutdown,
    os::fd::AsRawFd,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use goblins_os_textshortcuts_engine::{
    sanitize_shortcuts, text_shortcuts_table_is_within_size_limit, TextShortcut,
};
use serde::{Deserialize, Serialize};
use socket2::{Domain, SockAddr, Socket, Type};

const DEFAULT_SOCKET: &str = "/run/goblins-os-session/session-bridge.sock";
const DESKTOP_SESSION_USER: &str = "goblin";
const MAX_REQUEST_BYTES: usize = 24 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const BRIDGE_IO_TIMEOUT: Duration = Duration::from_millis(2_000);
const VOICE_STATUS_TIMEOUT: Duration = Duration::from_secs(8);
const VOICE_CAPTURE_TIMEOUT: Duration = Duration::from_secs(35);
const VOICE_PLAYBACK_TIMEOUT: Duration = Duration::from_secs(130);
const MAX_CAPTURE_WAV_BYTES: usize = 512 * 1024;
const MAX_PLAYBACK_WAV_BYTES: usize = 16 * 1024 * 1024;
const MAX_CAPTURE_DURATION_SECONDS: u64 = 7;
const MAX_PLAYBACK_DURATION_SECONDS: u64 = 90;
const TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA: &str = "goblins-os.text-shortcuts-runtime-status.v1";
const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS: u64 = 5_000_000_000;
const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS: u64 = 250_000_000;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SessionBridgeResult {
    Unavailable,
    Success(String),
    Failed(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VoiceBridgeResult<T> {
    Unavailable,
    Success(T),
    Failed(String),
    InvalidResponse,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct VoiceAudioStatus {
    pub(crate) capture_ready: bool,
    pub(crate) playback_ready: bool,
    pub(crate) capture_detail: String,
    pub(crate) playback_detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PcmFormat {
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    block_align: u16,
    byte_rate: u32,
}

const CAPTURE_PCM_FORMAT: PcmFormat = PcmFormat {
    channels: 1,
    sample_rate: 16_000,
    bits_per_sample: 16,
    block_align: 2,
    byte_rate: 32_000,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TextShortcutsBridgeResult {
    Unavailable,
    Success(Vec<TextShortcut>),
    Rejected,
    InvalidResponse,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextShortcutsRuntimeStatus {
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

impl TextShortcutsRuntimeStatus {
    pub(crate) fn ready(&self) -> bool {
        self.focused
            && self.enabled
            && self.surrounding_text_supported
            && self.snapshot_valid
            && self.child_alive
            && self.last_response_ok
    }

    fn valid_at(&self, now_ns: u64) -> bool {
        self.schema == TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA
            && self.instance_id.len() == 32
            && self
                .instance_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && self.focus_generation > 0
            && self.runtime_generation > 0
            && self.sequence > 0
            && self.monotonic_ns > 0
            && self.monotonic_ns
                <= now_ns.saturating_add(TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS)
            && now_ns.saturating_sub(self.monotonic_ns) <= TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TextShortcutsRuntimeStatusResult {
    Unavailable,
    Success(TextShortcutsRuntimeStatus),
    Rejected(String),
    InvalidResponse,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum BridgeRequest<'a> {
    GSettings {
        args: Vec<&'a str>,
    },
    OpenPreview {
        path: String,
        kind: &'a str,
    },
    Wpctl {
        args: Vec<&'a str>,
    },
    VoiceAudioStatus,
    VoiceCapture,
    VoicePlayback {
        wav_base64: String,
    },
    PermissionStoreDelete {
        table: &'a str,
        id: &'a str,
        app: &'a str,
    },
    DisplayConfigGetCurrentState,
    DisplayConfigGetApplyAllowed,
    DisplayConfigApplyMonitors {
        serial: u32,
        method: u32,
        logical_monitors: Vec<DisplayConfigLogicalMonitor<'a>>,
    },
    IbusEngine,
    TextShortcutsRuntimeStatus,
    TextShortcutsRead,
    TextShortcutsWrite {
        shortcuts: &'a [TextShortcut],
    },
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DisplayConfigLogicalMonitor<'a> {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) scale: f64,
    pub(crate) transform: u32,
    pub(crate) primary: bool,
    pub(crate) monitors: Vec<DisplayConfigMonitor<'a>>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DisplayConfigMonitor<'a> {
    pub(crate) connector: &'a str,
    pub(crate) mode_id: &'a str,
}

#[derive(Deserialize)]
struct BridgeResponse {
    ok: bool,
    stdout: String,
    detail: String,
}

pub(crate) fn gsettings(args: &[&str]) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::GSettings {
        args: args.to_vec(),
    })
}

pub(crate) fn open_preview(path: &Path, kind: &'static str) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::OpenPreview {
        path: path.display().to_string(),
        kind,
    })
}

pub(crate) fn wpctl(args: &[&str]) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::Wpctl {
        args: args.to_vec(),
    })
}

/// Probe the real default source and sink inside the logged-in desktop audio
/// session. The operation has no caller-controlled command, device, or path.
pub(crate) fn voice_audio_status() -> VoiceBridgeResult<VoiceAudioStatus> {
    match call_bridge_detailed(&BridgeRequest::VoiceAudioStatus) {
        DetailedBridgeResult::Unavailable | DetailedBridgeResult::TransportUnavailable(_) => {
            VoiceBridgeResult::Unavailable
        }
        DetailedBridgeResult::Rejected(detail) => VoiceBridgeResult::Failed(detail),
        DetailedBridgeResult::ProtocolFailure(_) => VoiceBridgeResult::InvalidResponse,
        DetailedBridgeResult::Success(raw) => {
            let Ok(status) = serde_json::from_str::<VoiceAudioStatus>(&raw) else {
                return VoiceBridgeResult::InvalidResponse;
            };
            if status.capture_detail.is_empty()
                || status.playback_detail.is_empty()
                || status.capture_detail.len() > 512
                || status.playback_detail.len() > 512
            {
                return VoiceBridgeResult::InvalidResponse;
            }
            VoiceBridgeResult::Success(status)
        }
    }
}

/// Capture one fixed six-second, 16 kHz mono WAV in the desktop session. No
/// path or audio parameters cross the privilege boundary.
pub(crate) fn voice_capture() -> VoiceBridgeResult<Vec<u8>> {
    match call_bridge_detailed(&BridgeRequest::VoiceCapture) {
        DetailedBridgeResult::Unavailable | DetailedBridgeResult::TransportUnavailable(_) => {
            VoiceBridgeResult::Unavailable
        }
        DetailedBridgeResult::Rejected(detail) => VoiceBridgeResult::Failed(detail),
        DetailedBridgeResult::ProtocolFailure(_) => VoiceBridgeResult::InvalidResponse,
        DetailedBridgeResult::Success(raw) => {
            let Ok(wav) = BASE64_STANDARD.decode(raw) else {
                return VoiceBridgeResult::InvalidResponse;
            };
            if wav.len() > MAX_CAPTURE_WAV_BYTES
                || !valid_pcm_wave(&wav, MAX_CAPTURE_DURATION_SECONDS, Some(CAPTURE_PCM_FORMAT))
            {
                return VoiceBridgeResult::InvalidResponse;
            }
            VoiceBridgeResult::Success(wav)
        }
    }
}

/// Play one bounded WAV through the desktop session's real default sink. The
/// bridge decodes, validates, and streams the typed audio payload to fixed
/// `aplay` stdin; callers cannot select a command, device, or filesystem path.
pub(crate) fn voice_playback(wav: &[u8]) -> VoiceBridgeResult<()> {
    if wav.len() > MAX_PLAYBACK_WAV_BYTES
        || !valid_pcm_wave(wav, MAX_PLAYBACK_DURATION_SECONDS, None)
    {
        return VoiceBridgeResult::InvalidResponse;
    }
    let request = BridgeRequest::VoicePlayback {
        wav_base64: BASE64_STANDARD.encode(wav),
    };
    match call_bridge_detailed(&request) {
        DetailedBridgeResult::Unavailable | DetailedBridgeResult::TransportUnavailable(_) => {
            VoiceBridgeResult::Unavailable
        }
        DetailedBridgeResult::Rejected(detail) => VoiceBridgeResult::Failed(detail),
        DetailedBridgeResult::ProtocolFailure(_) => VoiceBridgeResult::InvalidResponse,
        DetailedBridgeResult::Success(_) => VoiceBridgeResult::Success(()),
    }
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
    if declared_size != wav.len()
        || wav.get(12..16) != Some(b"fmt ")
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

pub(crate) fn permission_store_delete_permission(
    table: &str,
    id: &str,
    app: &str,
) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::PermissionStoreDelete { table, id, app })
}

pub(crate) fn display_config_get_current_state() -> SessionBridgeResult {
    call_bridge(&BridgeRequest::DisplayConfigGetCurrentState)
}

/// Read-only probe of the session's active IBus engine name.
pub(crate) fn ibus_engine() -> SessionBridgeResult {
    call_bridge(&BridgeRequest::IbusEngine)
}

/// Read one fixed adapter-owned runtime heartbeat. The request carries no path
/// or payload, and both bridge processes independently enforce the strict v1
/// shape and monotonic freshness window before readiness can use it.
pub(crate) fn text_shortcuts_runtime_status() -> TextShortcutsRuntimeStatusResult {
    text_shortcuts_runtime_status_result(call_bridge_detailed(
        &BridgeRequest::TextShortcutsRuntimeStatus,
    ))
}

/// Read the desktop user's one fixed Text Shortcuts table. The request has no
/// path or payload, and the response must be the canonical bounded table shape.
pub(crate) fn text_shortcuts_read() -> TextShortcutsBridgeResult {
    text_shortcuts_result(call_bridge_detailed(&BridgeRequest::TextShortcutsRead))
}

/// Atomically replace the desktop user's one fixed Text Shortcuts table. Only
/// the typed table crosses the bridge; callers cannot choose a path or raw blob.
pub(crate) fn text_shortcuts_write(shortcuts: &[TextShortcut]) -> TextShortcutsBridgeResult {
    text_shortcuts_result(call_bridge_detailed(&BridgeRequest::TextShortcutsWrite {
        shortcuts,
    }))
}

pub(crate) fn display_config_get_apply_allowed() -> SessionBridgeResult {
    call_bridge(&BridgeRequest::DisplayConfigGetApplyAllowed)
}

pub(crate) fn display_config_apply_monitors(
    serial: u32,
    method: u32,
    logical_monitors: Vec<DisplayConfigLogicalMonitor<'_>>,
) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::DisplayConfigApplyMonitors {
        serial,
        method,
        logical_monitors,
    })
}

fn call_bridge(request: &BridgeRequest<'_>) -> SessionBridgeResult {
    match call_bridge_detailed(request) {
        DetailedBridgeResult::Unavailable => SessionBridgeResult::Unavailable,
        DetailedBridgeResult::Success(stdout) => SessionBridgeResult::Success(stdout),
        DetailedBridgeResult::TransportUnavailable(detail)
        | DetailedBridgeResult::Rejected(detail)
        | DetailedBridgeResult::ProtocolFailure(detail) => SessionBridgeResult::Failed(detail),
    }
}

enum DetailedBridgeResult {
    Unavailable,
    TransportUnavailable(String),
    Success(String),
    Rejected(String),
    ProtocolFailure(String),
}

fn call_bridge_detailed(request: &BridgeRequest<'_>) -> DetailedBridgeResult {
    let socket = socket_path();
    if !socket.exists() {
        return DetailedBridgeResult::Unavailable;
    }
    let expected_peer_uid = match validate_session_bridge_socket(&socket) {
        Ok(uid) => uid,
        Err(detail) => return DetailedBridgeResult::TransportUnavailable(detail),
    };
    let timeout = match request {
        BridgeRequest::VoiceAudioStatus => VOICE_STATUS_TIMEOUT,
        BridgeRequest::VoiceCapture => VOICE_CAPTURE_TIMEOUT,
        BridgeRequest::VoicePlayback { .. } => VOICE_PLAYBACK_TIMEOUT,
        _ => BRIDGE_IO_TIMEOUT,
    };
    let request = match serde_json::to_vec(request) {
        Ok(request) => request,
        Err(_) => {
            return DetailedBridgeResult::ProtocolFailure(
                "Goblins OS session bridge request could not be encoded.".to_string(),
            );
        }
    };
    if request.len() > MAX_REQUEST_BYTES {
        return DetailedBridgeResult::Rejected(
            "Goblins OS session bridge request exceeds the fixed size limit.".to_string(),
        );
    }
    let deadline = Instant::now() + timeout;
    let mut stream = match connect_before(&socket, deadline) {
        Ok(stream) => stream,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return DetailedBridgeResult::Unavailable;
        }
        Err(error) => {
            return DetailedBridgeResult::TransportUnavailable(format!(
                "Goblins OS session bridge is not reachable: {error}"
            ));
        }
    };
    if session_bridge_peer_uid(&stream) != Ok(expected_peer_uid) {
        return DetailedBridgeResult::TransportUnavailable(
            "Goblins OS session bridge peer authentication failed.".to_string(),
        );
    }
    if let Err(error) = write_all_before(&mut stream, &request, deadline) {
        return DetailedBridgeResult::TransportUnavailable(format!(
            "Goblins OS session bridge request failed: {error}"
        ));
    }
    let _ = stream.shutdown(Shutdown::Write);

    let response = match read_to_end_before(&mut stream, MAX_RESPONSE_BYTES, deadline) {
        Ok(response) => response,
        Err(error) => {
            if error.kind() == io::ErrorKind::InvalidData {
                return DetailedBridgeResult::ProtocolFailure(
                    "Goblins OS session bridge response exceeds the fixed size limit.".to_string(),
                );
            }
            return DetailedBridgeResult::TransportUnavailable(format!(
                "Goblins OS session bridge did not answer before the core bridge timeout: {error}"
            ));
        }
    };
    match serde_json::from_slice::<BridgeResponse>(&response) {
        Ok(response) if response.ok => DetailedBridgeResult::Success(response.stdout),
        Ok(response) => DetailedBridgeResult::Rejected(if response.detail.is_empty() {
            "Goblins OS session bridge rejected the request.".to_string()
        } else {
            response.detail
        }),
        Err(_) => DetailedBridgeResult::ProtocolFailure(
            "Goblins OS session bridge returned an invalid response.".to_string(),
        ),
    }
}

fn connect_before(path: &Path, deadline: Instant) -> io::Result<UnixStream> {
    let address = SockAddr::unix(path)?;
    #[cfg(target_os = "linux")]
    let socket_type = Type::STREAM.cloexec();
    #[cfg(not(target_os = "linux"))]
    let socket_type = Type::STREAM;
    let socket = Socket::new(Domain::UNIX, socket_type, None)?;
    #[cfg(not(target_os = "linux"))]
    socket.set_cloexec(true)?;
    socket.connect_timeout(&address, remaining_before(deadline)?)?;
    // connect_timeout restores blocking mode on success. Set it explicitly as
    // a fail-closed invariant before the bounded blocking read/write helpers.
    socket.set_nonblocking(false)?;
    socket.peer_addr()?;
    Ok(socket.into())
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
                        "bridge response exceeded its fixed size limit",
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

fn validate_session_bridge_socket(socket: &Path) -> Result<u32, String> {
    let expected_uid = resolve_user_id(DESKTOP_SESSION_USER)
        .ok_or_else(|| "The Goblins OS desktop session identity is unavailable.".to_string())?;
    let parent = socket
        .parent()
        .ok_or_else(|| "Goblins OS session bridge socket has no parent directory.".to_string())?;
    let parent_metadata = std::fs::symlink_metadata(parent)
        .map_err(|_| "Goblins OS session bridge directory is unavailable.".to_string())?;
    let socket_metadata = std::fs::symlink_metadata(socket)
        .map_err(|_| "Goblins OS session bridge socket is unavailable.".to_string())?;
    if !parent_metadata.is_dir()
        || parent_metadata.uid() != expected_uid
        || parent_metadata.permissions().mode() & 0o7777 != 0o770
        || !socket_metadata.file_type().is_socket()
        || socket_metadata.uid() != expected_uid
        || socket_metadata.permissions().mode() & 0o7777 != 0o660
    {
        return Err(
            "Goblins OS session bridge path failed ownership or mode validation.".to_string(),
        );
    }
    Ok(expected_uid)
}

fn session_bridge_peer_uid(stream: &UnixStream) -> Result<u32, ()> {
    #[cfg(target_os = "linux")]
    {
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: credentials and length point to writable storage of the exact
        // sizes passed to getsockopt; stream owns a live Unix-domain descriptor.
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        };
        if result == 0 && length as usize == std::mem::size_of::<libc::ucred>() {
            Ok(credentials.uid)
        } else {
            Err(())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut uid = 0;
        let mut gid = 0;
        // SAFETY: uid and gid are valid writable pointers and stream owns a
        // live Unix-domain descriptor.
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
    // SAFETY: every pointer refers to valid writable storage for the supplied
    // size; name is NUL-terminated and lives through the call.
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
    // SAFETY: getpwnam_r returned success and result points at the initialized
    // record storage supplied above.
    Some(unsafe { record.assume_init() }.pw_uid)
}

fn text_shortcuts_result(result: DetailedBridgeResult) -> TextShortcutsBridgeResult {
    match result {
        DetailedBridgeResult::Unavailable | DetailedBridgeResult::TransportUnavailable(_) => {
            TextShortcutsBridgeResult::Unavailable
        }
        DetailedBridgeResult::Rejected(_) => TextShortcutsBridgeResult::Rejected,
        DetailedBridgeResult::ProtocolFailure(_) => TextShortcutsBridgeResult::InvalidResponse,
        DetailedBridgeResult::Success(raw) => {
            let Ok(shortcuts) = serde_json::from_str::<Vec<TextShortcut>>(&raw) else {
                return TextShortcutsBridgeResult::InvalidResponse;
            };
            if !text_shortcuts_table_is_within_size_limit(&shortcuts)
                || sanitize_shortcuts(shortcuts.clone()) != shortcuts
            {
                return TextShortcutsBridgeResult::InvalidResponse;
            }
            TextShortcutsBridgeResult::Success(shortcuts)
        }
    }
}

fn text_shortcuts_runtime_status_result(
    result: DetailedBridgeResult,
) -> TextShortcutsRuntimeStatusResult {
    match result {
        DetailedBridgeResult::Unavailable | DetailedBridgeResult::TransportUnavailable(_) => {
            TextShortcutsRuntimeStatusResult::Unavailable
        }
        DetailedBridgeResult::Rejected(detail) => {
            TextShortcutsRuntimeStatusResult::Rejected(detail)
        }
        DetailedBridgeResult::ProtocolFailure(_) => {
            TextShortcutsRuntimeStatusResult::InvalidResponse
        }
        DetailedBridgeResult::Success(raw) => {
            let Ok(status) = serde_json::from_str::<TextShortcutsRuntimeStatus>(&raw) else {
                return TextShortcutsRuntimeStatusResult::InvalidResponse;
            };
            let Ok(now_ns) = monotonic_now_ns() else {
                return TextShortcutsRuntimeStatusResult::InvalidResponse;
            };
            if !status.valid_at(now_ns) {
                return TextShortcutsRuntimeStatusResult::InvalidResponse;
            }
            TextShortcutsRuntimeStatusResult::Success(status)
        }
    }
}

fn monotonic_now_ns() -> Result<u64, ()> {
    let mut timestamp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: timestamp points to writable storage for one timespec and
    // CLOCK_MONOTONIC is supported by the target Unix core runtime.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut timestamp) } != 0 {
        return Err(());
    }
    let seconds = u64::try_from(timestamp.tv_sec).map_err(|_| ())?;
    let nanoseconds = u64::try_from(timestamp.tv_nsec).map_err(|_| ())?;
    if nanoseconds >= 1_000_000_000 {
        return Err(());
    }
    seconds
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(nanoseconds))
        .ok_or(())
}

fn socket_path() -> PathBuf {
    env::var_os("GOBLINS_OS_SESSION_BRIDGE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET))
}

#[cfg(test)]
mod tests {
    use goblins_os_textshortcuts_engine::TextShortcut;

    #[cfg(target_os = "linux")]
    use super::connect_before;
    use super::{
        gsettings, monotonic_now_ns, read_to_end_before, text_shortcuts_result,
        text_shortcuts_runtime_status_result, valid_pcm_wave, wpctl, BridgeRequest,
        DetailedBridgeResult, PcmFormat, SessionBridgeResult, TextShortcutsBridgeResult,
        TextShortcutsRuntimeStatusResult, CAPTURE_PCM_FORMAT, MAX_CAPTURE_DURATION_SECONDS,
        MAX_PLAYBACK_DURATION_SECONDS, TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS,
        TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA,
    };
    use std::{
        io::Write,
        os::unix::net::UnixStream,
        time::{Duration, Instant},
    };

    #[cfg(target_os = "linux")]
    use socket2::{Domain, SockAddr, Socket, Type};

    fn pcm_wave(format: PcmFormat, data_bytes: usize) -> Vec<u8> {
        let padded = data_bytes + (data_bytes & 1);
        let mut wav = Vec::with_capacity(44 + padded);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + padded) as u32).to_le_bytes());
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
        wav.resize(44 + padded, 0);
        wav
    }

    #[test]
    fn voice_requests_expose_only_fixed_typed_audio_fields() {
        assert_eq!(
            serde_json::to_value(BridgeRequest::VoiceAudioStatus).unwrap(),
            serde_json::json!({"op": "voice-audio-status"})
        );
        assert_eq!(
            serde_json::to_value(BridgeRequest::VoiceCapture).unwrap(),
            serde_json::json!({"op": "voice-capture"})
        );
        let playback = serde_json::to_value(BridgeRequest::VoicePlayback {
            wav_base64: "UklGRg==".to_string(),
        })
        .unwrap();
        assert_eq!(
            playback.get("wav_base64").and_then(|value| value.as_str()),
            Some("UklGRg==")
        );
        for forbidden in ["path", "command", "args", "device"] {
            assert!(playback.get(forbidden).is_none());
        }
    }

    #[test]
    fn core_revalidates_untrusted_capture_wave_canonically() {
        let canonical = pcm_wave(CAPTURE_PCM_FORMAT, CAPTURE_PCM_FORMAT.byte_rate as usize);
        assert!(valid_pcm_wave(
            &canonical,
            MAX_CAPTURE_DURATION_SECONDS,
            Some(CAPTURE_PCM_FORMAT),
        ));
        assert!(valid_pcm_wave(
            &canonical,
            MAX_PLAYBACK_DURATION_SECONDS,
            None,
        ));

        let mut duplicate_data = canonical.clone();
        duplicate_data.extend_from_slice(b"data\x02\0\0\0\0\0");
        let size = (duplicate_data.len() - 8) as u32;
        duplicate_data[4..8].copy_from_slice(&size.to_le_bytes());
        assert!(!valid_pcm_wave(
            &duplicate_data,
            MAX_PLAYBACK_DURATION_SECONDS,
            None,
        ));

        let mut wrong_rate = canonical;
        wrong_rate[24..28].copy_from_slice(&48_000u32.to_le_bytes());
        wrong_rate[28..32].copy_from_slice(&96_000u32.to_le_bytes());
        assert!(!valid_pcm_wave(
            &wrong_rate,
            MAX_CAPTURE_DURATION_SECONDS,
            Some(CAPTURE_PCM_FORMAT),
        ));
    }

    #[test]
    fn core_bridge_deadline_rejects_trickle_response_by_total_wall_time() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
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

    #[cfg(target_os = "linux")]
    #[test]
    fn core_bridge_connect_deadline_bounds_saturated_unix_listener() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("saturated.sock");
        let address = SockAddr::unix(&path).unwrap();
        let listener = Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
        listener.bind(&address).unwrap();
        listener.listen(0).unwrap();

        // Linux permits one queued AF_UNIX connection even with backlog zero.
        // Leaving it unaccepted saturates the desktop-owned listener queue.
        let _queued = UnixStream::connect(&path).unwrap();
        let started = Instant::now();
        let result = connect_before(&path, started + Duration::from_millis(110));
        let elapsed = started.elapsed();

        assert!(result.is_err());
        // Linux may reject the saturated nonblocking connect immediately with
        // EAGAIN; fail-fast and deadline expiry are both valid bounded outcomes.
        assert!(elapsed < Duration::from_millis(500));
    }

    #[test]
    fn absent_bridge_reports_unavailable_for_host_tests() {
        if std::env::var_os("GOBLINS_OS_SESSION_BRIDGE_SOCKET").is_none() {
            assert_eq!(
                gsettings(&["list-schemas"]),
                SessionBridgeResult::Unavailable
            );
            assert_eq!(wpctl(&["status"]), SessionBridgeResult::Unavailable);
        }
    }

    #[test]
    fn text_shortcuts_requests_contain_only_fixed_typed_fields() {
        let runtime_status =
            serde_json::to_value(BridgeRequest::TextShortcutsRuntimeStatus).unwrap();
        assert_eq!(
            runtime_status,
            serde_json::json!({"op": "text-shortcuts-runtime-status"})
        );
        assert!(runtime_status.get("path").is_none());
        assert!(runtime_status.get("raw").is_none());
        assert!(runtime_status.get("blob").is_none());

        let read = serde_json::to_value(BridgeRequest::TextShortcutsRead).unwrap();
        assert_eq!(read, serde_json::json!({"op": "text-shortcuts-read"}));

        let shortcuts = vec![TextShortcut::new("omw", "on my way")];
        let write = serde_json::to_value(BridgeRequest::TextShortcutsWrite {
            shortcuts: &shortcuts,
        })
        .unwrap();
        assert_eq!(
            write,
            serde_json::json!({
                "op": "text-shortcuts-write",
                "shortcuts": [{"replace": "omw", "with": "on my way"}]
            })
        );
        assert!(write.get("path").is_none());
        assert!(write.get("raw").is_none());
        assert!(write.get("blob").is_none());
    }

    #[test]
    fn text_shortcuts_bridge_results_fail_closed_by_failure_class() {
        assert_eq!(
            text_shortcuts_result(DetailedBridgeResult::Unavailable),
            TextShortcutsBridgeResult::Unavailable
        );
        assert_eq!(
            text_shortcuts_result(DetailedBridgeResult::TransportUnavailable(
                "connection refused".to_string()
            )),
            TextShortcutsBridgeResult::Unavailable
        );
        assert_eq!(
            text_shortcuts_result(DetailedBridgeResult::Rejected("rejected".to_string())),
            TextShortcutsBridgeResult::Rejected
        );
        assert_eq!(
            text_shortcuts_result(DetailedBridgeResult::ProtocolFailure(
                "malformed".to_string()
            )),
            TextShortcutsBridgeResult::InvalidResponse
        );
        assert_eq!(
            text_shortcuts_result(DetailedBridgeResult::Success("not-json".to_string())),
            TextShortcutsBridgeResult::InvalidResponse
        );

        let canonical = r#"[{"replace":"omw","with":"on my way"}]"#.to_string();
        assert_eq!(
            text_shortcuts_result(DetailedBridgeResult::Success(canonical)),
            TextShortcutsBridgeResult::Success(vec![TextShortcut::new("omw", "on my way")])
        );
        let unsanitized = r#"[{"replace":" omw ","with":" on my way "}]"#.to_string();
        assert_eq!(
            text_shortcuts_result(DetailedBridgeResult::Success(unsanitized)),
            TextShortcutsBridgeResult::InvalidResponse
        );
    }

    fn runtime_status_json(
        now_ns: u64,
        focused: bool,
        enabled: bool,
        surrounding_text_supported: bool,
        snapshot_valid: bool,
        child_alive: bool,
        last_response_ok: bool,
    ) -> String {
        serde_json::json!({
            "schema": TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA,
            "instance_id": "0123456789abcdef0123456789abcdef",
            "focus_generation": 7,
            "runtime_generation": 11,
            "sequence": 13,
            "monotonic_ns": now_ns,
            "focused": focused,
            "enabled": enabled,
            "surrounding_text_supported": surrounding_text_supported,
            "snapshot_valid": snapshot_valid,
            "child_alive": child_alive,
            "last_response_ok": last_response_ok,
        })
        .to_string()
    }

    #[test]
    fn text_shortcuts_runtime_status_result_is_typed_and_fail_closed() {
        let now_ns = monotonic_now_ns().unwrap();
        let live = text_shortcuts_runtime_status_result(DetailedBridgeResult::Success(
            runtime_status_json(now_ns, true, true, true, true, true, true),
        ));
        assert!(matches!(
            live,
            TextShortcutsRuntimeStatusResult::Success(status) if status.ready()
        ));

        for readiness_signals in [
            [false, true, true, true, true, true],
            [true, false, true, true, true, true],
            [true, true, false, true, true, true],
            [true, true, true, false, true, true],
            [true, true, true, true, false, true],
            [true, true, true, true, true, false],
        ] {
            let [focused, enabled, surrounding_text_supported, snapshot_valid, child_alive, last_response_ok] =
                readiness_signals;
            let not_ready = text_shortcuts_runtime_status_result(DetailedBridgeResult::Success(
                runtime_status_json(
                    now_ns,
                    focused,
                    enabled,
                    surrounding_text_supported,
                    snapshot_valid,
                    child_alive,
                    last_response_ok,
                ),
            ));
            assert!(matches!(
                not_ready,
                TextShortcutsRuntimeStatusResult::Success(status) if !status.ready()
            ));
        }

        assert_eq!(
            text_shortcuts_runtime_status_result(DetailedBridgeResult::Rejected(
                "runtime status is stale".to_string(),
            )),
            TextShortcutsRuntimeStatusResult::Rejected("runtime status is stale".to_string())
        );
        assert_eq!(
            text_shortcuts_runtime_status_result(DetailedBridgeResult::Success(
                "not-json".to_string(),
            )),
            TextShortcutsRuntimeStatusResult::InvalidResponse
        );
    }

    #[test]
    fn text_shortcuts_runtime_status_result_rechecks_freshness_and_shape() {
        let now_ns = monotonic_now_ns().unwrap();
        assert_eq!(
            text_shortcuts_runtime_status_result(DetailedBridgeResult::Success(
                runtime_status_json(
                    now_ns.saturating_sub(TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS + 1),
                    true,
                    true,
                    true,
                    true,
                    true,
                    true,
                ),
            )),
            TextShortcutsRuntimeStatusResult::InvalidResponse
        );

        let mut unknown: serde_json::Value = serde_json::from_str(&runtime_status_json(
            now_ns, true, true, true, true, true, true,
        ))
        .unwrap();
        unknown["unexpected"] = serde_json::json!(true);
        assert_eq!(
            text_shortcuts_runtime_status_result(DetailedBridgeResult::Success(
                unknown.to_string(),
            )),
            TextShortcutsRuntimeStatusResult::InvalidResponse
        );

        let mut missing: serde_json::Value = serde_json::from_str(&runtime_status_json(
            now_ns, true, true, true, true, true, true,
        ))
        .unwrap();
        missing.as_object_mut().unwrap().remove("snapshot_valid");
        assert_eq!(
            text_shortcuts_runtime_status_result(DetailedBridgeResult::Success(
                missing.to_string(),
            )),
            TextShortcutsRuntimeStatusResult::InvalidResponse
        );
    }
}
