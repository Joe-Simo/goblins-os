//! Capability-specific local control-plane listeners.
//!
//! The browser-facing TCP listener is intentionally separate from these Unix
//! sockets.  Each shipped client receives a socket in a different Unix group
//! and an exact method/path allowlist.  Directory access is the capability;
//! requests outside the capability still fail closed at the HTTP boundary.

use std::{
    collections::BTreeSet,
    ffi::CString,
    fs, io,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::UnixStream as StdUnixStream,
    },
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use tokio::{
    net::{unix::SocketAddr, TcpListener, UnixListener, UnixStream},
    sync::watch,
    task::JoinSet,
};

const PRODUCTION_ROOT: &str = "/run/goblins-os-core";
const SOCKET_NAME: &str = "control.sock";
const REQUIRED_DIRECTORY_MODE: u32 = 0o2750;
const REQUIRED_SOCKET_MODE: u32 = 0o660;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClientKind {
    ControlCenter,
    Dictate,
    FileBuilder,
    FocusTick,
    Installer,
    Launcher,
    Login,
    Markup,
    Open,
    /// Root-operated release gates only. There is deliberately no desktop
    /// client enum, setgid binary, or login-user group membership for this
    /// capability.
    ReleaseProof,
    Resident,
    ScreenshotContext,
    Settings,
    Shell,
    Today,
    VisualLookup,
    VoiceControl,
}

const ALL_CLIENTS: [ClientKind; 17] = [
    ClientKind::ControlCenter,
    ClientKind::Dictate,
    ClientKind::FileBuilder,
    ClientKind::FocusTick,
    ClientKind::Installer,
    ClientKind::Launcher,
    ClientKind::Login,
    ClientKind::Markup,
    ClientKind::Open,
    ClientKind::ReleaseProof,
    ClientKind::Resident,
    ClientKind::ScreenshotContext,
    ClientKind::Settings,
    ClientKind::Shell,
    ClientKind::Today,
    ClientKind::VisualLookup,
    ClientKind::VoiceControl,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Permission {
    method: &'static str,
    path: &'static str,
}

macro_rules! permissions {
    ($(($method:ident, $path:literal)),* $(,)?) => {
        &[
            $(Permission {
                method: stringify!($method),
                path: $path,
            }),*
        ]
    };
}

// These compile-time manifests mirror the literal routes used by each shipped
// client.  Query strings are intentionally excluded: authorization is against
// `Uri::path()`, so a caller cannot widen a capability with query data.
const CONTROL_CENTER_PERMISSIONS: &[Permission] = permissions![
    (GET, "/v1/models/openai-key"),
    (GET, "/v1/ai/actions"),
    (GET, "/v1/focus/status"),
    (POST, "/v1/models/engine"),
];

const DICTATE_PERMISSIONS: &[Permission] = permissions![(POST, "/v1/voice/dictate")];

const FILE_BUILDER_PERMISSIONS: &[Permission] =
    permissions![(POST, "/v1/apps/builds"), (POST, "/v1/ai/file-context"),];

const FOCUS_TICK_PERMISSIONS: &[Permission] = permissions![(POST, "/v1/focus/tick")];

const INSTALLER_PERMISSIONS: &[Permission] = permissions![
    (GET, "/health"),
    (GET, "/v1/auth/openai/status"),
    (GET, "/v1/codex/status"),
    (GET, "/v1/policy/status"),
    (GET, "/v1/network/status"),
    (GET, "/v1/installer/readiness"),
    (GET, "/v1/installer/install-targets"),
    (GET, "/v1/installer/install-targets/progress"),
    (GET, "/v1/local-models"),
    (GET, "/v1/services"),
    (GET, "/v1/network/wifi/scan"),
    (GET, "/v1/auth/openai/start"),
    (GET, "/v1/codex/login/url"),
    (POST, "/v1/network/wifi/connect"),
    (POST, "/v1/installer/install-targets/prepare"),
    (POST, "/v1/codex/login"),
    (POST, "/v1/models/engine"),
    (POST, "/v1/policy/permissions/grant"),
    (POST, "/v1/apps/builds"),
    (POST, "/v1/installer/complete"),
    (POST, "/v1/session/unlock"),
    (POST, "/v1/privacy"),
    (POST, "/v1/local-models/install"),
];

const LAUNCHER_PERMISSIONS: &[Permission] = permissions![
    (GET, "/v1/apps"),
    (GET, "/v1/ai/actions"),
    (POST, "/v1/input/switch-next"),
    (POST, "/v1/apps/builds"),
    (POST, "/v1/ai/runtime"),
    (POST, "/v1/ai/selected-text-context"),
    (POST, "/v1/ai/write-selected-text"),
    (POST, "/v1/ai/screen-context"),
];

const LOGIN_PERMISSIONS: &[Permission] = permissions![
    (GET, "/health"),
    (GET, "/v1/auth/openai/status"),
    (GET, "/v1/session/gate"),
    (GET, "/v1/auth/openai/start"),
    (POST, "/v1/session/unlock"),
];

const MARKUP_PERMISSIONS: &[Permission] = permissions![(POST, "/v1/ocr/recognize")];

const OPEN_PERMISSIONS: &[Permission] = permissions![
    (GET, "/health"),
    (GET, "/v1/session/gate"),
    (GET, "/v1/services"),
];

// Canonical image, render, runtime, and hardware gates run as root and exercise
// backend contracts that have no interactive desktop caller. This separate
// server-only socket keeps those probes off TCP without widening any shipped
// application's capability manifest.
const RELEASE_PROOF_PERMISSIONS: &[Permission] = permissions![
    (GET, "/health"),
    (GET, "/v1/readiness"),
    (GET, "/v1/ai/actions"),
    (GET, "/v1/ai/action-history"),
    (GET, "/v1/ai/runtime/status"),
    (GET, "/v1/apps"),
    (GET, "/v1/apps/build-catalog"),
    (GET, "/v1/audio/status"),
    (GET, "/v1/auth/openai/status"),
    (GET, "/v1/codex/resident/status"),
    (GET, "/v1/codex/status"),
    (GET, "/v1/displays/status"),
    (GET, "/v1/firewall/status"),
    (GET, "/v1/focus/status"),
    (GET, "/v1/installer/install-targets"),
    (GET, "/v1/local-models"),
    (GET, "/v1/models/openai-key"),
    (GET, "/v1/network/status"),
    (GET, "/v1/policy/status"),
    (GET, "/v1/preview/status"),
    (GET, "/v1/system/hardware"),
    (GET, "/v1/system/services"),
    (GET, "/v1/text-shortcuts"),
    (GET, "/v1/text-shortcuts/preview"),
    (POST, "/v1/ai/open-settings-panel"),
    (POST, "/v1/ai/screen-context"),
    (POST, "/v1/ai/selected-text-context"),
    (POST, "/v1/ai/settings-context"),
    (POST, "/v1/ai/system-status"),
    (POST, "/v1/ai/write-selected-text"),
    (POST, "/v1/app-privacy/revoke"),
    (POST, "/v1/apps/builds"),
    (POST, "/v1/displays/apply"),
    (POST, "/v1/firewall/enabled"),
    (POST, "/v1/focus/activate"),
    (POST, "/v1/focus/deactivate"),
    (POST, "/v1/focus/mode"),
    (POST, "/v1/input/sources"),
    (POST, "/v1/input/switch-next"),
    (POST, "/v1/installer/complete"),
    (POST, "/v1/keyboard/modifier-remap"),
    (POST, "/v1/keyboard/shortcuts/binding"),
    (POST, "/v1/models/engine"),
    (POST, "/v1/policy/permissions/grant"),
    (POST, "/v1/preview/open"),
    (POST, "/v1/privacy"),
    (POST, "/v1/release-proof/storage/voice"),
    (POST, "/v1/session/unlock"),
    (POST, "/v1/text-shortcuts"),
];

const RESIDENT_PERMISSIONS: &[Permission] = permissions![(POST, "/v1/codex/resident")];

const SCREENSHOT_CONTEXT_PERMISSIONS: &[Permission] = permissions![(POST, "/v1/ocr/recognize")];

const SETTINGS_PERMISSIONS: &[Permission] = permissions![
    (GET, "/health"),
    (GET, "/v1/settings/system"),
    (GET, "/v1/system/image"),
    (GET, "/v1/auth/openai/status"),
    (GET, "/v1/system/services"),
    (GET, "/v1/system/hardware"),
    (GET, "/v1/recovery/status"),
    (GET, "/v1/security/encryption"),
    (GET, "/v1/snapshots/status"),
    (GET, "/v1/local-models"),
    (GET, "/v1/ai/runtime/status"),
    (GET, "/v1/ai/actions"),
    (GET, "/v1/ai/action-history"),
    (GET, "/v1/policy/status"),
    (GET, "/v1/models/openai-key"),
    (GET, "/v1/privacy/status"),
    (GET, "/v1/vision/status"),
    (GET, "/v1/voice/status"),
    (GET, "/v1/accessibility/switch-control/status"),
    (GET, "/v1/sound-recognition/status"),
    (GET, "/v1/live-captions/status"),
    (GET, "/v1/codex/status"),
    (GET, "/v1/appearance/status"),
    (GET, "/v1/network/status"),
    (GET, "/v1/notifications/status"),
    (GET, "/v1/focus/status"),
    (GET, "/v1/displays/status"),
    (GET, "/v1/bluetooth/status"),
    (GET, "/v1/audio/status"),
    (GET, "/v1/input/status"),
    (GET, "/v1/text-shortcuts"),
    (GET, "/v1/accessibility/status"),
    (GET, "/v1/firewall/status"),
    (GET, "/v1/hotspot/status"),
    (GET, "/v1/window-management/status"),
    (GET, "/v1/shortcuts/status"),
    (GET, "/v1/keychain/status"),
    (GET, "/v1/keychain/collections"),
    (GET, "/v1/fingerprint/status"),
    (GET, "/v1/app-privacy/status"),
    (GET, "/v1/network/wifi/scan"),
    (GET, "/v1/auth/openai/start"),
    (GET, "/v1/codex/login/url"),
    (POST, "/v1/policy/permissions/grant"),
    (POST, "/v1/ai/settings-context"),
    (POST, "/v1/models/engine"),
    (POST, "/v1/local-models/install"),
    (POST, "/v1/privacy"),
    (POST, "/v1/network/wifi/connect"),
    (POST, "/v1/hotspot/enabled"),
    (POST, "/v1/window-management/hot-corner"),
    (POST, "/v1/network/proxy/mode"),
    (POST, "/v1/bluetooth/power"),
    (POST, "/v1/firewall/enabled"),
    (POST, "/v1/app-privacy/revoke"),
    (POST, "/v1/audio/volume"),
    (POST, "/v1/audio/mute"),
    (POST, "/v1/audio/default-device"),
    (POST, "/v1/audio/preference"),
    (POST, "/v1/notifications/preference"),
    (POST, "/v1/focus/deactivate"),
    (POST, "/v1/focus/activate"),
    (POST, "/v1/appearance/color-scheme"),
    (POST, "/v1/appearance/wallpaper-placement"),
    (POST, "/v1/appearance/wallpaper-shading"),
    (POST, "/v1/privacy/desktop"),
    (POST, "/v1/accessibility/preference"),
    (POST, "/v1/accessibility/switch-control/preference"),
    (POST, "/v1/sound-recognition/preference"),
    (POST, "/v1/sound-recognition/sound-toggle"),
    (POST, "/v1/input/preference"),
    (POST, "/v1/input/sources"),
    (POST, "/v1/input/source"),
    (POST, "/v1/text-shortcuts"),
    (POST, "/v1/codex/login"),
    (DELETE, "/v1/auth/openai/session"),
    (DELETE, "/v1/codex/login"),
];

const SHELL_PERMISSIONS: &[Permission] = permissions![
    (GET, "/health"),
    (GET, "/v1/auth/openai/status"),
    (GET, "/v1/session/gate"),
    (GET, "/v1/installer/readiness"),
    (GET, "/v1/services"),
    (GET, "/v1/local-models"),
    (GET, "/v1/ai/runtime/status"),
    (GET, "/v1/apps"),
    (GET, "/v1/voice/status"),
    (GET, "/v1/models/openai-key"),
    (GET, "/v1/codex/status"),
    (GET, "/v1/studio/session"),
    (GET, "/v1/studio/sessions"),
    (GET, "/v1/auth/openai/start"),
    (POST, "/v1/voice/converse"),
    (POST, "/v1/studio/turn"),
    (POST, "/v1/models/engine"),
    (POST, "/v1/apps/builds"),
];

const TODAY_PERMISSIONS: &[Permission] = permissions![(GET, "/v1/today/status")];

const VISUAL_LOOKUP_PERMISSIONS: &[Permission] =
    permissions![(GET, "/v1/vision/status"), (POST, "/v1/ai/visual-lookup"),];

const VOICE_CONTROL_PERMISSIONS: &[Permission] = permissions![(POST, "/v1/voice/control")];

impl ClientKind {
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::ControlCenter => "control-center",
            Self::Dictate => "dictate",
            Self::FileBuilder => "file-builder",
            Self::FocusTick => "focus-tick",
            Self::Installer => "installer",
            Self::Launcher => "launcher",
            Self::Login => "login",
            Self::Markup => "markup",
            Self::Open => "open",
            Self::ReleaseProof => "release-proof",
            Self::Resident => "resident",
            Self::ScreenshotContext => "screenshot-context",
            Self::Settings => "settings",
            Self::Shell => "shell",
            Self::Today => "today",
            Self::VisualLookup => "visual-lookup",
            Self::VoiceControl => "voice-control",
        }
    }

    const fn permissions(self) -> &'static [Permission] {
        match self {
            Self::ControlCenter => CONTROL_CENTER_PERMISSIONS,
            Self::Dictate => DICTATE_PERMISSIONS,
            Self::FileBuilder => FILE_BUILDER_PERMISSIONS,
            Self::FocusTick => FOCUS_TICK_PERMISSIONS,
            Self::Installer => INSTALLER_PERMISSIONS,
            Self::Launcher => LAUNCHER_PERMISSIONS,
            Self::Login => LOGIN_PERMISSIONS,
            Self::Markup => MARKUP_PERMISSIONS,
            Self::Open => OPEN_PERMISSIONS,
            Self::ReleaseProof => RELEASE_PROOF_PERMISSIONS,
            Self::Resident => RESIDENT_PERMISSIONS,
            Self::ScreenshotContext => SCREENSHOT_CONTEXT_PERMISSIONS,
            Self::Settings => SETTINGS_PERMISSIONS,
            Self::Shell => SHELL_PERMISSIONS,
            Self::Today => TODAY_PERMISSIONS,
            Self::VisualLookup => VISUAL_LOOKUP_PERMISSIONS,
            Self::VoiceControl => VOICE_CONTROL_PERMISSIONS,
        }
    }

    fn allows(self, method: &Method, path: &str) -> bool {
        self.permissions()
            .iter()
            .any(|permission| permission.method == method.as_str() && permission.path == path)
    }

    fn socket_path(self) -> PathBuf {
        Path::new(PRODUCTION_ROOT).join(self.id()).join(SOCKET_NAME)
    }

    fn group_name(self) -> String {
        format!("goblins-core-{}", self.id())
    }
}

pub(crate) fn capability_router(private_router: Router, client: ClientKind) -> Router {
    private_router.layer(middleware::from_fn(move |request, next| {
        enforce_capability(client, request, next)
    }))
}

/// Apply the exact unauthenticated TCP method/path boundary. Axum normally
/// treats `HEAD` as implicitly allowed by a `GET` route; the OAuth callback is
/// intentionally GET-only, so the boundary is enforced before method routing.
pub(crate) fn tcp_surface_router(router: Router) -> Router {
    router.layer(middleware::from_fn(enforce_tcp_surface))
}

async fn enforce_tcp_surface(request: Request<Body>, next: Next) -> Response {
    let allowed = request.method() == Method::GET
        && matches!(request.uri().path(), "/health" | "/v1/auth/openai/callback");
    if !allowed {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}

async fn enforce_capability(client: ClientKind, request: Request<Body>, next: Next) -> Response {
    if !client.allows(request.method(), request.uri().path()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}

struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct BoundClientSocket {
    client: ClientKind,
    group_id: u32,
    listener: UnixListener,
    _guard: SocketGuard,
}

#[derive(Debug, Eq, PartialEq)]
struct PeerCredentials {
    process_id: i32,
    user_id: u32,
    primary_group_id: u32,
    supplementary_group_ids: Vec<u32>,
}

impl PeerCredentials {
    fn belongs_to_group(&self, required_group_id: u32) -> bool {
        self.primary_group_id == required_group_id
            || self.supplementary_group_ids.contains(&required_group_id)
    }
}

/// Axum listener that authorizes each accepted connection from immutable
/// credentials captured by the Linux socket layer. Filesystem DAC is the first
/// boundary; this second boundary prevents the shared core UID (and even root)
/// from crossing from one capability socket into another without proving the
/// exact capability group.
struct CapabilityListener {
    client: ClientKind,
    required_group_id: u32,
    listener: UnixListener,
}

impl CapabilityListener {
    fn new(client: ClientKind, required_group_id: u32, listener: UnixListener) -> Self {
        Self {
            client,
            required_group_id,
            listener,
        }
    }
}

impl axum::serve::Listener for CapabilityListener {
    type Io = UnixStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.listener.accept().await {
                Ok((stream, address)) => match peer_credentials(&stream) {
                    Ok(peer) if peer.belongs_to_group(self.required_group_id) => {
                        return (stream, address);
                    }
                    Ok(peer) => {
                        tracing::debug!(
                            client = self.client.id(),
                            peer_pid = peer.process_id,
                            peer_uid = peer.user_id,
                            peer_primary_gid = peer.primary_group_id,
                            required_gid = self.required_group_id,
                            "rejected capability connection without the required peer group"
                        );
                    }
                    Err(error) => {
                        tracing::debug!(
                            client = self.client.id(),
                            required_gid = self.required_group_id,
                            %error,
                            "rejected capability connection whose peer groups could not be proven"
                        );
                    }
                },
                Err(error) => {
                    tracing::warn!(
                        client = self.client.id(),
                        %error,
                        "capability listener accept failed"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

#[cfg(target_os = "linux")]
fn peer_credentials(stream: &UnixStream) -> io::Result<PeerCredentials> {
    const MAX_SUPPLEMENTARY_GROUPS: usize = 65_536;

    let descriptor = stream.as_raw_fd();
    let mut credential = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut credential_length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the descriptor is a live connected Unix stream and the output
    // buffer and length pointer remain valid for the complete getsockopt call.
    let result = unsafe {
        libc::getsockopt(
            descriptor,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credential.as_mut_ptr().cast(),
            &mut credential_length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if credential_length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an invalid credential size",
        ));
    }
    // SAFETY: getsockopt succeeded and reported the complete ucred size.
    let credential = unsafe { credential.assume_init() };

    let group_size = std::mem::size_of::<libc::gid_t>();
    let maximum_group_bytes = MAX_SUPPLEMENTARY_GROUPS * group_size;
    let mut group_bytes: libc::socklen_t = 0;
    // Linux reports the exact SO_PEERGROUPS buffer size through optlen. A
    // non-empty group list returns ERANGE for this zero-length sizing query;
    // an empty list succeeds with a zero length.
    // SAFETY: no output buffer is supplied and the valid length pointer is the
    // documented sizing-query form for SO_PEERGROUPS.
    let sizing_result = unsafe {
        libc::getsockopt(
            descriptor,
            libc::SOL_SOCKET,
            libc::SO_PEERGROUPS,
            std::ptr::null_mut(),
            &mut group_bytes,
        )
    };
    if sizing_result != 0 && io::Error::last_os_error().raw_os_error() != Some(libc::ERANGE) {
        return Err(io::Error::last_os_error());
    }
    let required_bytes = group_bytes as usize;
    if required_bytes > maximum_group_bytes || required_bytes % group_size != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERGROUPS returned an invalid group-list size",
        ));
    }

    let mut supplementary_group_ids = vec![0 as libc::gid_t; required_bytes / group_size];
    if required_bytes != 0 {
        // SAFETY: the vector has exactly `group_bytes` writable bytes and both
        // it and the length pointer remain live for the getsockopt call.
        let result = unsafe {
            libc::getsockopt(
                descriptor,
                libc::SOL_SOCKET,
                libc::SO_PEERGROUPS,
                supplementary_group_ids.as_mut_ptr().cast(),
                &mut group_bytes,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        if group_bytes as usize != required_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SO_PEERGROUPS changed size during credential inspection",
            ));
        }
    }

    Ok(PeerCredentials {
        process_id: credential.pid,
        user_id: credential.uid,
        primary_group_id: credential.gid,
        supplementary_group_ids,
    })
}

#[cfg(not(target_os = "linux"))]
fn peer_credentials(_stream: &UnixStream) -> io::Result<PeerCredentials> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "capability peer-group authorization requires Linux SO_PEERGROUPS",
    ))
}

/// Bind all production capability sockets before serving any request.  A
/// partial setup is torn down, so startup never silently exposes only a subset
/// of client boundaries.
fn bind_production_sockets() -> io::Result<Vec<BoundClientSocket>> {
    // Resolve every exact capability group before creating any socket. A
    // missing, duplicated, or misconfigured group therefore fails startup
    // before a partial control plane can be exposed.
    let expected_groups: Vec<_> = ALL_CLIENTS
        .into_iter()
        .map(|client| resolve_expected_group_id(client).map(|group_id| (client, group_id)))
        .collect::<io::Result<_>>()?;
    let unique_expected_groups: BTreeSet<_> = expected_groups
        .iter()
        .map(|(_, group_id)| *group_id)
        .collect();
    if unique_expected_groups.len() != expected_groups.len() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "every client capability must resolve to a distinct Unix group",
        ));
    }

    let sockets: Vec<_> = expected_groups
        .into_iter()
        .map(|(client, expected_group_id)| bind_production_socket(client, expected_group_id))
        .collect::<io::Result<_>>()?;
    let unique_groups: BTreeSet<_> = sockets.iter().map(|socket| socket.group_id).collect();
    if unique_groups.len() != sockets.len() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "every client control directory must use a distinct Unix group",
        ));
    }
    Ok(sockets)
}

fn bind_production_socket(
    client: ClientKind,
    expected_group_id: u32,
) -> io::Result<BoundClientSocket> {
    let path = client.socket_path();
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "control socket path has no parent directory",
        )
    })?;
    let directory_metadata = validate_production_directory(directory, client, expected_group_id)?;
    remove_stale_socket(&path, &directory_metadata)?;

    let listener = UnixListener::bind(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not bind {} control socket: {error}", client.id()),
        )
    })?;
    let guard = SocketGuard(path.clone());
    fs::set_permissions(&path, fs::Permissions::from_mode(REQUIRED_SOCKET_MODE))?;

    let socket_metadata = fs::symlink_metadata(&path)?;
    let socket_mode = socket_metadata.mode() & 0o7777;
    if !socket_metadata.file_type().is_socket()
        || socket_metadata.uid() != effective_uid()
        || socket_metadata.gid() != directory_metadata.gid()
        || socket_mode != REQUIRED_SOCKET_MODE
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} control socket did not inherit the required owner, group, and 0660 mode",
                client.id()
            ),
        ));
    }

    Ok(BoundClientSocket {
        client,
        group_id: directory_metadata.gid(),
        listener,
        _guard: guard,
    })
}

fn validate_production_directory(
    directory: &Path,
    client: ClientKind,
    expected_group_id: u32,
) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "required {} control directory {} is unavailable: {error}",
                client.id(),
                directory.display()
            ),
        )
    })?;
    let mode = metadata.mode() & 0o7777;
    if !metadata.file_type().is_dir()
        || metadata.uid() != effective_uid()
        || mode != REQUIRED_DIRECTORY_MODE
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be a real directory owned by the core user with mode 2750",
                directory.display()
            ),
        ));
    }
    validate_expected_group_id(client, metadata.gid(), expected_group_id)?;
    Ok(metadata)
}

fn validate_expected_group_id(
    client: ClientKind,
    actual_group_id: u32,
    expected_group_id: u32,
) -> io::Result<()> {
    if actual_group_id == expected_group_id {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "{} control directory must use exact group {} (gid {expected_group_id}), not gid {actual_group_id}",
            client.id(),
            client.group_name()
        ),
    ))
}

fn resolve_expected_group_id(client: ClientKind) -> io::Result<u32> {
    const FALLBACK_BUFFER_SIZE: usize = 16 * 1024;
    const MAX_BUFFER_SIZE: usize = 1024 * 1024;

    let group_name = client.group_name();
    let c_group_name = CString::new(group_name.as_str()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid capability group name {group_name}"),
        )
    })?;
    // SAFETY: sysconf has no pointer arguments and `_SC_GETGR_R_SIZE_MAX` is a
    // valid selector on supported Unix targets.
    let configured_size = unsafe { libc::sysconf(libc::_SC_GETGR_R_SIZE_MAX) };
    let mut buffer_size = usize::try_from(configured_size)
        .ok()
        .filter(|size| *size > 0)
        .unwrap_or(FALLBACK_BUFFER_SIZE)
        .min(MAX_BUFFER_SIZE);

    loop {
        let mut group = std::mem::MaybeUninit::<libc::group>::uninit();
        let mut result = std::ptr::null_mut();
        let mut buffer = vec![0_u8; buffer_size];
        // SAFETY: all pointers reference live, writable storage for the full
        // call; `c_group_name` is NUL-terminated; and the result is read only
        // when libc reports success with a non-null pointer.
        let code = unsafe {
            libc::getgrnam_r(
                c_group_name.as_ptr(),
                group.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if code == libc::ERANGE && buffer_size < MAX_BUFFER_SIZE {
            buffer_size = (buffer_size * 2).min(MAX_BUFFER_SIZE);
            continue;
        }
        if code != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "could not resolve required capability group {group_name}: {}",
                    io::Error::from_raw_os_error(code)
                ),
            ));
        }
        if result.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("required capability group {group_name} does not exist"),
            ));
        }
        // SAFETY: getgrnam_r returned success and `result` points to `group`.
        let group = unsafe { group.assume_init() };
        return Ok(group.gr_gid);
    }
}

fn remove_stale_socket(path: &Path, directory: &fs::Metadata) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket()
        || metadata.uid() != effective_uid()
        || metadata.gid() != directory.gid()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to replace an untrusted control socket entry at {}",
                path.display()
            ),
        ));
    }

    match StdUnixStream::connect(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("a core listener is already active at {}", path.display()),
        )),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            fs::remove_file(path)
        }
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!(
                "could not verify whether {} is a stale socket: {error}",
                path.display()
            ),
        )),
    }
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    unsafe { libc::geteuid() }
}

/// Serve the minimal browser/TCP surface and all capability sockets together.
/// Axum's HTTP/1 connection service keeps accepted Unix-stream connections
/// alive, allowing each native client to reuse its authorized connection.
pub(crate) async fn serve<F>(
    tcp_listener: TcpListener,
    tcp_router: Router,
    private_router: Router,
    shutdown: F,
) -> io::Result<()>
where
    F: std::future::Future<Output = ()> + Send,
{
    let client_sockets = bind_production_sockets()?;
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let mut tasks = JoinSet::new();

    tasks.spawn(serve_tcp(
        tcp_listener,
        tcp_router,
        shutdown_receiver.clone(),
    ));
    for BoundClientSocket {
        client,
        group_id,
        listener,
        _guard,
    } in client_sockets
    {
        let router = capability_router(private_router.clone(), client);
        let receiver = shutdown_receiver.clone();
        tasks.spawn(async move {
            let _guard = _guard;
            tracing::info!(
                client = client.id(),
                socket = %client.socket_path().display(),
                "Goblins OS core capability listener ready"
            );
            axum::serve(CapabilityListener::new(client, group_id, listener), router)
                .with_graceful_shutdown(wait_for_shutdown(receiver))
                .await
        });
    }

    tokio::pin!(shutdown);
    let unexpected = tokio::select! {
        () = &mut shutdown => None,
        result = tasks.join_next() => Some(result),
    };
    let _ = shutdown_sender.send(true);

    let mut first_error = match unexpected {
        Some(Some(Ok(Ok(())))) => Some(io::Error::other(
            "a Goblins OS core listener stopped unexpectedly",
        )),
        Some(Some(Ok(Err(error)))) => Some(error),
        Some(Some(Err(error))) => Some(io::Error::other(format!(
            "a Goblins OS core listener task failed: {error}"
        ))),
        Some(None) => Some(io::Error::other(
            "all Goblins OS core listeners stopped unexpectedly",
        )),
        None => None,
    };

    while let Some(result) = tasks.join_next().await {
        if first_error.is_none() {
            first_error = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(io::Error::other(format!(
                    "a Goblins OS core listener task failed: {error}"
                ))),
            };
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn serve_tcp(
    listener: TcpListener,
    router: Router,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await
}

async fn wait_for_shutdown(mut receiver: watch::Receiver<bool>) {
    while !*receiver.borrow() {
        if receiver.changed().await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        capability_router, validate_expected_group_id, validate_production_directory, ClientKind,
        PeerCredentials, ALL_CLIENTS, REQUIRED_DIRECTORY_MODE,
    };
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use tower::ServiceExt;

    fn literal_client_paths(source: &str) -> std::collections::BTreeSet<String> {
        source
            .split('"')
            .filter_map(|literal| {
                let literal = ["GET ", "POST ", "DELETE "]
                    .into_iter()
                    .find_map(|method| literal.strip_prefix(method))
                    .unwrap_or(literal);
                if !(literal.starts_with("/v1/") || literal.starts_with("/health")) {
                    return None;
                }
                let end = literal
                    .find(|character: char| character == '?' || character.is_whitespace())
                    .unwrap_or(literal.len());
                Some(literal[..end].to_string())
            })
            .collect()
    }

    fn assert_literal_inventory(client: ClientKind, source: &str) {
        let actual = literal_client_paths(source);
        let allowed = client
            .permissions()
            .iter()
            .map(|permission| permission.path.to_string())
            .collect();
        assert_eq!(
            actual,
            allowed,
            "literal route inventory drifted for {}",
            client.id()
        );
    }

    fn request(method: Method, path: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("valid test request")
    }

    #[test]
    fn production_client_ids_and_socket_paths_are_fixed_and_unique() {
        let mut ids = ALL_CLIENTS.map(ClientKind::id).to_vec();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALL_CLIENTS.len());
        for client in ALL_CLIENTS {
            assert_eq!(
                client.socket_path().to_string_lossy(),
                format!("/run/goblins-os-core/{}/control.sock", client.id())
            );
            assert_eq!(client.group_name(), format!("goblins-core-{}", client.id()));
            assert!(!client.permissions().is_empty());
            let unique: std::collections::BTreeSet<_> = client
                .permissions()
                .iter()
                .map(|permission| (permission.method, permission.path))
                .collect();
            assert_eq!(unique.len(), client.permissions().len());
            assert!(!client
                .permissions()
                .iter()
                .any(|permission| permission.path == "/v1/auth/openai/callback"));
        }
    }

    #[test]
    fn peer_group_authorization_accepts_primary_or_supplementary_membership() {
        let primary = PeerCredentials {
            process_id: 100,
            user_id: 200,
            primary_group_id: 300,
            supplementary_group_ids: vec![400],
        };
        assert!(primary.belongs_to_group(300));
        assert!(primary.belongs_to_group(400));
    }

    #[test]
    fn peer_group_authorization_never_bypasses_for_shared_or_root_uid() {
        for user_id in [0, super::effective_uid()] {
            let peer = PeerCredentials {
                process_id: 100,
                user_id,
                primary_group_id: 300,
                supplementary_group_ids: vec![400, 500],
            };
            assert!(!peer.belongs_to_group(600));
        }
    }

    #[test]
    fn peer_group_authorization_rejects_an_empty_wrong_group_set() {
        let peer = PeerCredentials {
            process_id: 100,
            user_id: 200,
            primary_group_id: 300,
            supplementary_group_ids: Vec::new(),
        };
        assert!(!peer.belongs_to_group(400));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn linux_peer_credentials_are_read_from_the_connected_socket_snapshot() {
        let (_peer, stream) = tokio::net::UnixStream::pair().unwrap();
        let credentials = super::peer_credentials(&stream).unwrap();
        assert_eq!(credentials.process_id, std::process::id() as i32);
        assert_eq!(credentials.user_id, unsafe { libc::geteuid() });
        assert!(credentials.belongs_to_group(unsafe { libc::getegid() }));
    }

    #[test]
    fn session_helpers_have_only_their_single_exact_post_capability() {
        for (client, allowed_path) in [
            (ClientKind::Dictate, "/v1/voice/dictate"),
            (ClientKind::FocusTick, "/v1/focus/tick"),
            (ClientKind::VoiceControl, "/v1/voice/control"),
        ] {
            assert_eq!(client.permissions().len(), 1, "{}", client.id());
            assert!(client.allows(&Method::POST, allowed_path));
            assert!(!client.allows(&Method::GET, allowed_path));
            assert!(!client.allows(&Method::POST, "/v1/settings/system"));
        }
    }

    #[test]
    fn release_proof_is_server_only_and_default_denied_outside_its_manifest() {
        let proof = ClientKind::ReleaseProof;
        assert_eq!(proof.id(), "release-proof");
        assert!(proof.allows(&Method::GET, "/health"));
        assert!(proof.allows(&Method::GET, "/v1/models/openai-key"));
        assert!(proof.allows(&Method::POST, "/v1/apps/builds"));
        assert!(proof.allows(&Method::POST, "/v1/models/engine"));
        assert!(!proof.allows(&Method::GET, "/v1/apps/builds"));
        assert!(!proof.allows(&Method::GET, "/v1/models/engine"));
        assert!(!proof.allows(&Method::GET, "/v1/auth/openai/callback"));
        assert!(!proof
            .permissions()
            .iter()
            .any(|permission| permission.method == "DELETE"));
    }

    #[test]
    fn production_directory_validation_fails_closed_for_missing_or_unsafe_paths() {
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

        let root = tempfile::tempdir().expect("temporary control root");
        let safe = root.path().join("settings");
        std::fs::create_dir(&safe).expect("safe directory");
        std::fs::set_permissions(
            &safe,
            std::fs::Permissions::from_mode(REQUIRED_DIRECTORY_MODE),
        )
        .expect("set safe directory mode");
        let expected_group_id = std::fs::symlink_metadata(&safe)
            .expect("safe directory metadata")
            .gid();
        validate_production_directory(&safe, ClientKind::Settings, expected_group_id)
            .expect("owner-only setgid directory should be accepted");

        let missing = root.path().join("missing");
        assert_eq!(
            validate_production_directory(&missing, ClientKind::Settings, expected_group_id)
                .expect_err("missing directory must fail")
                .kind(),
            std::io::ErrorKind::NotFound
        );

        std::fs::set_permissions(&safe, std::fs::Permissions::from_mode(0o2770))
            .expect("set unsafe directory mode");
        assert_eq!(
            validate_production_directory(&safe, ClientKind::Settings, expected_group_id)
                .expect_err("group-writable directory must fail")
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );

        let target = root.path().join("target");
        std::fs::create_dir(&target).expect("symlink target");
        let link = root.path().join("link");
        symlink(&target, &link).expect("directory symlink");
        assert_eq!(
            validate_production_directory(&link, ClientKind::Settings, expected_group_id)
                .expect_err("directory symlink must fail")
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn production_directory_validation_rejects_swapped_unique_group_ids() {
        let settings_group = 41_001;
        let shell_group = 41_002;
        let swapped = [
            (ClientKind::Settings, shell_group, settings_group),
            (ClientKind::Shell, settings_group, shell_group),
        ];
        let actual_groups: std::collections::BTreeSet<_> =
            swapped.iter().map(|(_, actual, _)| actual).collect();
        assert_eq!(actual_groups.len(), swapped.len());

        for (client, actual_group, expected_group) in swapped {
            let error = validate_expected_group_id(client, actual_group, expected_group)
                .expect_err("a distinct but swapped capability group must fail closed");
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
            assert!(error.to_string().contains(&client.group_name()));
        }
    }

    #[test]
    fn every_shipped_clients_literal_paths_match_its_capability_manifest() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("core crate must have a crates directory");
        for (client, crate_name) in [
            (ClientKind::ControlCenter, "goblins-os-control-center"),
            (ClientKind::FileBuilder, "goblins-os-file-builder"),
            (ClientKind::Installer, "goblins-os-installer"),
            (ClientKind::Launcher, "goblins-os-launcher"),
            (ClientKind::Login, "goblins-os-login"),
            (ClientKind::Markup, "goblins-os-markup"),
            (ClientKind::Open, "goblins-os-open"),
            (ClientKind::Resident, "goblins-os-resident"),
            (
                ClientKind::ScreenshotContext,
                "goblins-os-screenshot-context",
            ),
            (ClientKind::Settings, "goblins-os-settings"),
            (ClientKind::Shell, "goblins-os-shell"),
            (ClientKind::Today, "goblins-os-today"),
            (ClientKind::VisualLookup, "goblins-os-visual-lookup"),
        ] {
            let source = std::fs::read_to_string(crates.join(crate_name).join("src/main.rs"))
                .expect("shipped client source must be readable");
            assert_literal_inventory(client, &source);
        }
        for (client, binary) in [
            (ClientKind::Dictate, "goblins-os-dictate"),
            (ClientKind::FocusTick, "goblins-os-focus-tick"),
            (ClientKind::VoiceControl, "goblins-os-voice-control"),
        ] {
            let source = std::fs::read_to_string(
                crates
                    .join("goblins-os-session-tools/src/bin")
                    .join(format!("{binary}.rs")),
            )
            .expect("shipped session capability client source must be readable");
            assert_literal_inventory(client, &source);
        }
    }

    #[tokio::test]
    async fn cross_client_routes_are_denied_before_the_private_handler() {
        let today = capability_router(crate::private_router(), ClientKind::Today);
        let response = today
            .oneshot(request(Method::GET, "/v1/settings/system"))
            .await
            .expect("router response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let settings = capability_router(crate::private_router(), ClientKind::Settings);
        let response = settings
            .oneshot(request(Method::GET, "/v1/today/status"))
            .await
            .expect("router response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn tcp_surface_default_denies_every_native_api_route() {
        for (method, path) in [
            (Method::GET, "/v1/readiness"),
            (Method::GET, "/v1/auth/openai/status"),
            (Method::POST, "/v1/models/engine"),
            (Method::POST, "/v1/auth/openai/callback"),
            (Method::HEAD, "/v1/auth/openai/callback"),
            (Method::HEAD, "/health"),
            (Method::GET, "/v1/auth/openai/callback/extra"),
        ] {
            let response = crate::tcp_router()
                .oneshot(request(method, path))
                .await
                .expect("router response");
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }

        let health = crate::tcp_router()
            .oneshot(request(Method::GET, "/health"))
            .await
            .expect("health response");
        assert_eq!(health.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn oauth_callback_uses_only_the_exact_tcp_host_state_path() {
        let callback = Request::builder()
            .method(Method::GET)
            .uri("/v1/auth/openai/callback?code=discard&state=unknown")
            .header("host", "127.0.0.1:8787")
            .body(Body::empty())
            .expect("valid callback request");
        let response = crate::tcp_router()
            .oneshot(callback)
            .await
            .expect("callback response");

        // The callback handler may report an unconfigured provider (501) or an
        // unknown single-use state (400), depending on the test environment;
        // either proves the exact Host/state URI reached the guarded handler.
        assert!(matches!(
            response.status(),
            StatusCode::BAD_REQUEST | StatusCode::NOT_IMPLEMENTED
        ));
    }

    #[tokio::test]
    async fn wrong_method_on_an_allowed_path_is_default_denied() {
        let today = capability_router(crate::private_router(), ClientKind::Today);
        let response = today
            .oneshot(request(Method::POST, "/v1/today/status"))
            .await
            .expect("router response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn query_data_does_not_change_the_exact_path_capability() {
        let login = capability_router(crate::private_router(), ClientKind::Login);
        let response = login
            .oneshot(request(Method::GET, "/health?probe=capability"))
            .await
            .expect("router response");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
