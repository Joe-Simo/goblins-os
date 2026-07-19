//! Capability-scoped client for the Goblins OS core.
//!
//! Each installed desktop entrypoint is setgid to one narrow per-application
//! group. [`initialize`] consumes that one-time capability by opening the exact
//! AF_UNIX socket, permanently dropping the elevated group, and re-executing a
//! fixed root-owned regular payload with only that authenticated descriptor.
//! The second exec clears Linux `AT_SECURE` before GTK/GIO starts, so the normal
//! GNOME session bus remains available without retaining group privilege. The
//! resulting HTTP/1.1 connection is serialized, persistent, bounded, and never
//! reconnected. A broken connection requires relaunching the application.

#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    env,
    error::Error as StdError,
    fmt,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

#[cfg(any(
    target_os = "linux",
    test,
    all(feature = "test-transport", debug_assertions)
))]
use socket2::{Domain, SockAddr, Socket, Type};

#[cfg(target_os = "linux")]
use std::os::{
    fd::{AsRawFd, FromRawFd},
    unix::process::CommandExt,
};
#[cfg(any(
    target_os = "linux",
    test,
    all(feature = "test-transport", debug_assertions)
))]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicI32;

pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_READ_TIMEOUT: Duration = Duration::from_secs(65 * 60);

#[cfg(any(
    target_os = "linux",
    test,
    all(feature = "test-transport", debug_assertions)
))]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(any(
    target_os = "linux",
    test,
    all(feature = "test-transport", debug_assertions)
))]
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(any(target_os = "linux", test))]
const STARTUP_CONNECT_WINDOW: Duration = Duration::from_secs(5);
#[cfg(any(target_os = "linux", test))]
const STARTUP_RETRY_BACKOFF: Duration = Duration::from_millis(50);
const HTTP_HOST: &str = "localhost";
const FIXED_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
#[cfg(target_os = "linux")]
const TRANSFERRED_CAPABILITY_FD: libc::c_int = 3;
#[cfg(target_os = "linux")]
const DESKTOP_PAYLOAD_ROOT: &str = "/usr/libexec/goblins-os/ui";

static INITIALIZATION_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static FORKED_CHILD: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "linux")]
static ACTIVE_FD: AtomicI32 = AtomicI32::new(-1);
static GLOBAL: OnceLock<Arc<SharedConnection>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientKind {
    ControlCenter,
    Dictate,
    FileBuilder,
    FocusTick,
    Installer,
    Launcher,
    Login,
    Markup,
    Open,
    Resident,
    ScreenshotContext,
    Settings,
    Shell,
    Today,
    VisualLookup,
    VoiceControl,
}

impl ClientKind {
    #[must_use]
    pub const fn slug(self) -> &'static str {
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
            Self::Resident => "resident",
            Self::ScreenshotContext => "screenshot-context",
            Self::Settings => "settings",
            Self::Shell => "shell",
            Self::Today => "today",
            Self::VisualLookup => "visual-lookup",
            Self::VoiceControl => "voice-control",
        }
    }

    #[must_use]
    pub fn socket_path(self) -> PathBuf {
        PathBuf::from("/run/goblins-os-core")
            .join(self.slug())
            .join("control.sock")
    }

    const fn user_agent(self) -> &'static str {
        match self {
            Self::ControlCenter => "goblins-os-control-center",
            Self::Dictate => "goblins-os-dictate",
            Self::FileBuilder => "goblins-os-file-builder",
            Self::FocusTick => "goblins-os-focus-tick",
            Self::Installer => "goblins-os-installer",
            Self::Launcher => "goblins-os-launcher",
            Self::Login => "goblins-os-login",
            Self::Markup => "goblins-os-markup",
            Self::Open => "goblins-os-open",
            Self::Resident => "goblins-os-resident",
            Self::ScreenshotContext => "goblins-os-screenshot-context",
            Self::Settings => "goblins-os-settings",
            Self::Shell => "goblins-os-shell",
            Self::Today => "goblins-os-today",
            Self::VisualLookup => "goblins-os-visual-lookup",
            Self::VoiceControl => "goblins-os-voice-control",
        }
    }

    #[cfg(target_os = "linux")]
    const fn executable_name(self) -> &'static str {
        match self {
            Self::ControlCenter => "goblins-os-control-center",
            Self::Dictate => "goblins-os-dictate",
            Self::FileBuilder => "goblins-os-file-builder",
            Self::FocusTick => "goblins-os-focus-tick",
            Self::Installer => "goblins-os-installer",
            Self::Launcher => "goblins-os-launcher",
            Self::Login => "goblins-os-login",
            Self::Markup => "goblins-os-markup",
            Self::Open => "goblins-os-open",
            Self::Resident => "goblins-os-resident",
            Self::ScreenshotContext => "goblins-os-screenshot-context",
            Self::Settings => "goblins-os-settings",
            Self::Shell => "goblins-os-shell",
            Self::Today => "goblins-os-today",
            Self::VisualLookup => "goblins-os-visual-lookup",
            Self::VoiceControl => "goblins-os-voice-control",
        }
    }

    #[cfg(target_os = "linux")]
    fn desktop_payload_path(self) -> PathBuf {
        PathBuf::from(DESKTOP_PAYLOAD_ROOT).join(self.executable_name())
    }

    /// Only the non-interactive Resident service may enable no-new-privileges.
    /// Desktop clients need later setgid execs to open other Goblins OS apps.
    #[must_use]
    pub const fn requires_no_new_privs(self) -> bool {
        matches!(self, Self::Resident)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Delete,
}

impl Method {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

impl Response {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    }

    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }
}

#[derive(Clone)]
pub struct CoreClient {
    shared: Arc<SharedConnection>,
}

impl fmt::Debug for CoreClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CoreClient")
            .field("kind", &self.shared.kind)
            .finish_non_exhaustive()
    }
}

impl CoreClient {
    pub fn get(&self, path: &str, read_timeout: Duration) -> Result<Response, Error> {
        self.request(Method::Get, path, None, read_timeout)
    }

    pub fn post_json(
        &self,
        path: &str,
        body: &[u8],
        read_timeout: Duration,
    ) -> Result<Response, Error> {
        self.request(Method::Post, path, Some(body), read_timeout)
    }

    pub fn delete(&self, path: &str, read_timeout: Duration) -> Result<Response, Error> {
        self.request(Method::Delete, path, None, read_timeout)
    }

    pub fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&[u8]>,
        read_timeout: Duration,
    ) -> Result<Response, Error> {
        validate_request(method, path, body, read_timeout)?;
        if FORKED_CHILD.load(Ordering::Acquire) {
            return Err(Error::RelaunchRequired);
        }

        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| Error::RelaunchRequired)?;
        if state.broken {
            return Err(Error::RelaunchRequired);
        }
        match state.connection.exchange(
            method,
            path,
            body.unwrap_or_default(),
            read_timeout,
            self.shared.kind.user_agent(),
        ) {
            Ok(response) => Ok(response),
            Err(failure) => {
                // Retain the one descriptor in a shut-down state until process
                // exit. This lets the atfork child hook always close the exact
                // registered descriptor without any stale-fd reuse race.
                let _ = state
                    .connection
                    .reader
                    .get_mut()
                    .shutdown(std::net::Shutdown::Both);
                state.broken = true;
                Err(Error::ConnectionBroken(failure.to_string()))
            }
        }
    }

    /// Creates an explicit unprivileged transport for integration tests. It is
    /// unavailable to production builds unless `test-transport` is deliberately
    /// enabled and never consults an environment variable.
    #[cfg(any(test, all(feature = "test-transport", debug_assertions)))]
    pub fn connect_test(kind: ClientKind, socket_path: &Path) -> Result<Self, Error> {
        let stream = connect_socket_with_timeout(socket_path, CONNECT_TIMEOUT)
            .map_err(Error::Initialization)?;
        Ok(Self {
            shared: Arc::new(SharedConnection {
                kind,
                state: Mutex::new(ConnectionState {
                    connection: PersistentConnection::new(stream),
                    broken: false,
                }),
            }),
        })
    }
}

/// Consumes the installed entrypoint's per-application setgid capability.
///
/// This must be the first action in `main`, before argument/environment parsing
/// or any GTK/GIO initialization.
pub fn initialize(kind: ClientKind) -> Result<CoreClient, Error> {
    if INITIALIZATION_ATTEMPTED.swap(true, Ordering::AcqRel) {
        return GLOBAL
            .get()
            .filter(|shared| shared.kind == kind)
            .cloned()
            .map(|shared| CoreClient { shared })
            .ok_or(Error::InitializationAlreadyAttempted);
    }

    sanitize_environment();

    #[cfg(not(target_os = "linux"))]
    {
        let _ = kind;
        Err(Error::UnsupportedPlatform)
    }

    #[cfg(target_os = "linux")]
    {
        initialize_linux(kind)
    }
}

#[cfg(target_os = "linux")]
fn initialize_linux(kind: ClientKind) -> Result<CoreClient, Error> {
    let ids = process_ids().map_err(Error::Initialization)?;
    if !kind.requires_no_new_privs() && ids.real == ids.effective && ids.effective == ids.saved {
        return initialize_linux_desktop_payload(kind, ids);
    }

    initialize_linux_capability_entry(kind, ids)
}

#[cfg(target_os = "linux")]
fn initialize_linux_capability_entry(
    kind: ClientKind,
    ids: ProcessIds,
) -> Result<CoreClient, Error> {
    validate_initial_groups(kind, ids)?;
    let real_groups = supplementary_groups().map_err(Error::Initialization)?;
    if !kind.requires_no_new_privs() && real_groups.contains(&ids.effective) {
        return Err(Error::PrivilegeContract(
            "the desktop user must not be a member of the core capability group",
        ));
    }
    disable_dumpability().map_err(Error::Initialization)?;

    let socket_path = kind.socket_path();
    let stream = connect_initial_socket(&socket_path).map_err(Error::Initialization)?;
    verify_socket_path(kind, &socket_path, ids, &real_groups)?;
    verify_peer_owner(&stream, &socket_path).map_err(Error::Initialization)?;

    if !kind.requires_no_new_privs() {
        permanently_drop_group(ids.real).map_err(Error::Initialization)?;
        harden_process(kind).map_err(Error::Initialization)?;
        return reexec_desktop_payload(kind, stream).map_err(Error::Initialization);
    }

    finish_initialization(kind, stream)
}

#[cfg(target_os = "linux")]
fn initialize_linux_desktop_payload(
    kind: ClientKind,
    ids: ProcessIds,
) -> Result<CoreClient, Error> {
    if process_is_at_secure() {
        return Err(Error::PrivilegeContract(
            "desktop payload must start from a non-privileged exec",
        ));
    }

    if ids.real != ids.effective || ids.effective != ids.saved {
        return Err(Error::PrivilegeContract(
            "desktop payload must run only after the capability group is permanently dropped",
        ));
    }

    let real_groups = supplementary_groups().map_err(Error::Initialization)?;
    let payload_path = kind.desktop_payload_path();
    verify_desktop_payload_executable(&payload_path, ids, &real_groups)?;
    let stream = take_transferred_capability().map_err(Error::Initialization)?;
    let socket_path = kind.socket_path();
    verify_transferred_capability(kind, &stream, &socket_path, ids, &real_groups)?;
    harden_process(kind).map_err(Error::Initialization)?;
    finish_initialization(kind, stream)
}

#[cfg(target_os = "linux")]
fn finish_initialization(kind: ClientKind, stream: UnixStream) -> Result<CoreClient, Error> {
    ACTIVE_FD.store(stream.as_raw_fd(), Ordering::Release);
    if let Err(error) = register_child_close_handler() {
        ACTIVE_FD.store(-1, Ordering::Release);
        return Err(Error::Initialization(error));
    }

    let shared = Arc::new(SharedConnection {
        kind,
        state: Mutex::new(ConnectionState {
            connection: PersistentConnection::new(stream),
            broken: false,
        }),
    });
    if GLOBAL.set(Arc::clone(&shared)).is_err() {
        ACTIVE_FD.store(-1, Ordering::Release);
        return Err(Error::InitializationAlreadyAttempted);
    }
    Ok(CoreClient { shared })
}

struct SharedConnection {
    kind: ClientKind,
    state: Mutex<ConnectionState>,
}

struct ConnectionState {
    connection: PersistentConnection,
    broken: bool,
}

struct PersistentConnection {
    reader: BufReader<UnixStream>,
}

impl PersistentConnection {
    #[cfg(any(
        target_os = "linux",
        test,
        all(feature = "test-transport", debug_assertions)
    ))]
    fn new(stream: UnixStream) -> Self {
        Self {
            reader: BufReader::new(stream),
        }
    }

    fn exchange(
        &mut self,
        method: Method,
        path: &str,
        body: &[u8],
        read_timeout: Duration,
        user_agent: &str,
    ) -> Result<Response, TransportFailure> {
        self.reader
            .get_mut()
            .set_read_timeout(Some(read_timeout))
            .map_err(TransportFailure::Io)?;
        let mut request = format!(
            "{} {path} HTTP/1.1\r\nHost: {HTTP_HOST}\r\nUser-Agent: {user_agent}\r\nAccept: application/json\r\nConnection: keep-alive\r\n",
            method.as_str(),
        );
        if method == Method::Post {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        request.push_str("\r\n");

        let stream = self.reader.get_mut();
        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.write_all(body))
            .and_then(|()| stream.flush())
            .map_err(TransportFailure::Io)?;

        let response = read_response(&mut self.reader)?;
        if response
            .header("connection")
            .is_some_and(|value| value.eq_ignore_ascii_case("close"))
        {
            return Err(TransportFailure::Protocol(
                "core closed the persistent connection",
            ));
        }
        Ok(response)
    }
}

#[derive(Debug)]
enum TransportFailure {
    Io(io::Error),
    Protocol(&'static str),
    ResponseTooLarge,
}

impl fmt::Display for TransportFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "transport error: {error}"),
            Self::Protocol(detail) => write!(formatter, "HTTP protocol error: {detail}"),
            Self::ResponseTooLarge => formatter.write_str("response exceeded its size limit"),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    UnsupportedPlatform,
    InitializationAlreadyAttempted,
    PrivilegeContract(&'static str),
    Initialization(io::Error),
    InvalidPath,
    InvalidReadTimeout,
    RequestBodyTooLarge,
    BodyNotAllowed(Method),
    ConnectionBroken(String),
    RelaunchRequired,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("the Goblins OS core capability client requires Linux")
            }
            Self::InitializationAlreadyAttempted => formatter
                .write_str("Goblins OS core capability initialization was already attempted"),
            Self::PrivilegeContract(detail) => {
                write!(
                    formatter,
                    "invalid Goblins OS core privilege contract: {detail}"
                )
            }
            Self::Initialization(error) => {
                write!(
                    formatter,
                    "could not initialize Goblins OS core capability: {error}"
                )
            }
            Self::InvalidPath => formatter.write_str("Goblins OS core request path is invalid"),
            Self::InvalidReadTimeout => formatter.write_str(
                "Goblins OS core read timeout must be non-zero and no more than 65 minutes",
            ),
            Self::RequestBodyTooLarge => write!(
                formatter,
                "Goblins OS core request body exceeds {MAX_REQUEST_BODY_BYTES} bytes"
            ),
            Self::BodyNotAllowed(method) => {
                write!(
                    formatter,
                    "{} requests cannot carry a body",
                    method.as_str()
                )
            }
            Self::ConnectionBroken(detail) => write!(
                formatter,
                "Goblins OS core connection broke ({detail}); relaunch is required"
            ),
            Self::RelaunchRequired => formatter
                .write_str("Goblins OS core connection is no longer usable; relaunch is required"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Initialization(error) => Some(error),
            _ => None,
        }
    }
}

fn validate_request(
    method: Method,
    path: &str,
    body: Option<&[u8]>,
    read_timeout: Duration,
) -> Result<(), Error> {
    let valid_path = path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('#')
        && path
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\\');
    if !valid_path {
        return Err(Error::InvalidPath);
    }
    if read_timeout.is_zero() || read_timeout > MAX_READ_TIMEOUT {
        return Err(Error::InvalidReadTimeout);
    }
    let body = body.unwrap_or_default();
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return Err(Error::RequestBodyTooLarge);
    }
    if method != Method::Post && !body.is_empty() {
        return Err(Error::BodyNotAllowed(method));
    }
    Ok(())
}

#[cfg(any(
    target_os = "linux",
    test,
    all(feature = "test-transport", debug_assertions)
))]
fn connect_socket_with_timeout(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    let address = SockAddr::unix(path)?;
    let socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    socket.set_cloexec(true)?;
    socket.connect_timeout(&address, timeout)?;
    socket.set_write_timeout(Some(WRITE_TIMEOUT))?;
    Ok(socket.into())
}

#[cfg(any(target_os = "linux", test))]
fn connect_initial_socket(path: &Path) -> io::Result<UnixStream> {
    let deadline = std::time::Instant::now() + STARTUP_CONNECT_WINDOW;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Goblins OS core capability socket did not become ready",
            ));
        }
        match connect_socket_with_timeout(path, CONNECT_TIMEOUT.min(remaining)) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) =>
            {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(error);
                }
                std::thread::sleep(STARTUP_RETRY_BACKOFF.min(remaining));
            }
            Err(error) => return Err(error),
        }
    }
}

fn read_response(reader: &mut BufReader<UnixStream>) -> Result<Response, TransportFailure> {
    for _ in 0..=4 {
        let (status, headers) = read_response_head(reader)?;
        if (100..200).contains(&status) {
            continue;
        }
        let body = read_response_body(reader, status, &headers)?;
        return Ok(Response {
            status,
            headers,
            body,
        });
    }
    Err(TransportFailure::Protocol(
        "too many informational responses",
    ))
}

fn read_response_head(reader: &mut impl BufRead) -> Result<(u16, Vec<Header>), TransportFailure> {
    let mut total = 0;
    let status_line = read_line_bounded(reader, &mut total, MAX_RESPONSE_HEADER_BYTES)?;
    let status_text = line_text(&status_line)?;
    let mut parts = status_text.split_whitespace();
    if parts.next() != Some("HTTP/1.1") {
        return Err(TransportFailure::Protocol("response is not HTTP/1.1"));
    }
    let status = parts
        .next()
        .filter(|value| value.len() == 3)
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (100..=599).contains(value))
        .ok_or(TransportFailure::Protocol("invalid status code"))?;

    let mut headers = Vec::new();
    loop {
        let line = read_line_bounded(reader, &mut total, MAX_RESPONSE_HEADER_BYTES)?;
        if line == b"\r\n" {
            break;
        }
        let text = line_text(&line)?;
        if text.starts_with([' ', '\t']) {
            return Err(TransportFailure::Protocol(
                "folded headers are not supported",
            ));
        }
        let (name, value) = text
            .split_once(':')
            .ok_or(TransportFailure::Protocol("malformed response header"))?;
        if !valid_header_name(name)
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() && byte != b'\t')
        {
            return Err(TransportFailure::Protocol("invalid response header"));
        }
        headers.push(Header {
            name: name.to_ascii_lowercase(),
            value: value.trim_matches([' ', '\t']).to_string(),
        });
    }
    Ok((status, headers))
}

fn read_response_body(
    reader: &mut BufReader<UnixStream>,
    status: u16,
    headers: &[Header],
) -> Result<Vec<u8>, TransportFailure> {
    let content_length = unique_header(headers, "content-length")?;
    let transfer_encoding = unique_header(headers, "transfer-encoding")?;
    if content_length.is_some() && transfer_encoding.is_some() {
        return Err(TransportFailure::Protocol(
            "ambiguous response body framing",
        ));
    }

    if matches!(status, 204 | 304) {
        if transfer_encoding.is_some() || content_length.is_some_and(|value| value != "0") {
            return Err(TransportFailure::Protocol(
                "body framing is invalid for a bodyless status",
            ));
        }
        return Ok(Vec::new());
    }

    if let Some(value) = content_length {
        let length = value
            .parse::<usize>()
            .map_err(|_| TransportFailure::Protocol("invalid content length"))?;
        if length > MAX_RESPONSE_BODY_BYTES {
            return Err(TransportFailure::ResponseTooLarge);
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).map_err(TransportFailure::Io)?;
        return Ok(body);
    }

    if transfer_encoding.is_some_and(|value| value.eq_ignore_ascii_case("chunked")) {
        return read_chunked_body(reader);
    }
    if transfer_encoding.is_some() {
        return Err(TransportFailure::Protocol("unsupported transfer encoding"));
    }
    Err(TransportFailure::Protocol(
        "persistent response has no body framing",
    ))
}

fn read_chunked_body(reader: &mut BufReader<UnixStream>) -> Result<Vec<u8>, TransportFailure> {
    let mut body = Vec::new();
    loop {
        let mut size_line_bytes = 0;
        let size_line = read_line_bounded(reader, &mut size_line_bytes, 1024)?;
        let size_text = line_text(&size_line)?;
        let size_token = size_text.split(';').next().unwrap_or_default().trim();
        if size_token.is_empty() || size_token.len() > 16 {
            return Err(TransportFailure::Protocol("invalid chunk size"));
        }
        let size = usize::from_str_radix(size_token, 16)
            .map_err(|_| TransportFailure::Protocol("invalid chunk size"))?;
        if size == 0 {
            read_chunk_trailers(reader)?;
            return Ok(body);
        }
        if body.len().saturating_add(size) > MAX_RESPONSE_BODY_BYTES {
            return Err(TransportFailure::ResponseTooLarge);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader
            .read_exact(&mut body[start..])
            .map_err(TransportFailure::Io)?;
        let mut terminator = [0_u8; 2];
        reader
            .read_exact(&mut terminator)
            .map_err(TransportFailure::Io)?;
        if terminator != *b"\r\n" {
            return Err(TransportFailure::Protocol("invalid chunk terminator"));
        }
    }
}

fn read_chunk_trailers(reader: &mut BufReader<UnixStream>) -> Result<(), TransportFailure> {
    let mut total = 0;
    loop {
        let line = read_line_bounded(reader, &mut total, MAX_RESPONSE_HEADER_BYTES)?;
        if line == b"\r\n" {
            return Ok(());
        }
        let text = line_text(&line)?;
        let (name, value) = text
            .split_once(':')
            .ok_or(TransportFailure::Protocol("malformed chunk trailer"))?;
        if !valid_header_name(name)
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() && byte != b'\t')
        {
            return Err(TransportFailure::Protocol("invalid chunk trailer"));
        }
    }
}

fn read_line_bounded(
    reader: &mut impl BufRead,
    total: &mut usize,
    limit: usize,
) -> Result<Vec<u8>, TransportFailure> {
    let mut line = Vec::new();
    loop {
        if *total >= limit {
            return Err(TransportFailure::ResponseTooLarge);
        }
        let available = reader.fill_buf().map_err(TransportFailure::Io)?;
        if available.is_empty() {
            return Err(TransportFailure::Protocol("unexpected response EOF"));
        }
        let remaining = limit - *total;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let wanted = newline.map_or(available.len(), |index| index + 1);
        let take = wanted.min(remaining);
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        *total += take;
        if newline.is_some_and(|index| index < take) {
            return Ok(line);
        }
        if take < wanted || *total >= limit {
            return Err(TransportFailure::ResponseTooLarge);
        }
    }
}

fn line_text(line: &[u8]) -> Result<&str, TransportFailure> {
    let without_newline = line
        .strip_suffix(b"\r\n")
        .ok_or(TransportFailure::Protocol(
            "response line is not CRLF terminated",
        ))?;
    std::str::from_utf8(without_newline)
        .map_err(|_| TransportFailure::Protocol("response header is not UTF-8"))
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn unique_header<'a>(
    headers: &'a [Header],
    name: &str,
) -> Result<Option<&'a str>, TransportFailure> {
    let mut values = headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str());
    let first = values.next();
    if values.next().is_some() {
        return Err(TransportFailure::Protocol(
            "duplicate response framing header",
        ));
    }
    Ok(first)
}

fn sanitize_environment() {
    let names = env::vars_os()
        .map(|(name, _)| name)
        .filter(|name| sanitized_environment_name(name.to_string_lossy().as_ref()))
        .collect::<Vec<_>>();
    for name in names {
        env::remove_var(name);
    }
    env::set_var("PATH", FIXED_PATH);
}

fn sanitized_environment_name(name: &str) -> bool {
    dangerous_environment_name(name)
        || name.starts_with("GOBLINS_OS_CORE_")
        || name.starts_with("OPENAI_OS_CORE_")
}

fn dangerous_environment_name(name: &str) -> bool {
    name.starts_with("LD_")
        || name.starts_with("DYLD_")
        || matches!(
            name,
            "GLIBC_TUNABLES"
                | "GCONV_PATH"
                | "LOCPATH"
                | "NLSPATH"
                | "GTK_PATH"
                | "GTK_EXE_PREFIX"
                | "GTK_DATA_PREFIX"
                | "GTK_MODULES"
                | "GTK_IM_MODULE_FILE"
                | "GDK_PIXBUF_MODULE_FILE"
                | "GDK_PIXBUF_MODULEDIR"
                | "GIO_MODULE_DIR"
                | "GIO_EXTRA_MODULES"
                | "GI_TYPELIB_PATH"
                | "GSETTINGS_SCHEMA_DIR"
                | "QT_PLUGIN_PATH"
                | "QT_QPA_PLATFORM_PLUGIN_PATH"
                | "QML_IMPORT_PATH"
                | "QML2_IMPORT_PATH"
        )
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct ProcessIds {
    real: libc::gid_t,
    effective: libc::gid_t,
    saved: libc::gid_t,
    uid: libc::uid_t,
}

#[cfg(target_os = "linux")]
fn process_ids() -> io::Result<ProcessIds> {
    let mut real = 0;
    let mut effective = 0;
    let mut saved = 0;
    // SAFETY: all pointers reference valid, writable `gid_t` values.
    if unsafe { libc::getresgid(&mut real, &mut effective, &mut saved) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    Ok(ProcessIds {
        real,
        effective,
        saved,
        uid,
    })
}

#[cfg(target_os = "linux")]
fn process_is_at_secure() -> bool {
    // SAFETY: `getauxval` reads the immutable process auxiliary vector.
    unsafe { libc::getauxval(libc::AT_SECURE) != 0 }
}

#[cfg(target_os = "linux")]
fn reexec_desktop_payload(kind: ClientKind, stream: UnixStream) -> Result<CoreClient, io::Error> {
    let ids = process_ids()?;
    let real_groups = supplementary_groups()?;
    let payload_path = kind.desktop_payload_path();
    verify_trusted_payload_path(&payload_path, ids, &real_groups)?;

    // Mark every non-standard descriptor close-on-exec, then deliberately pass
    // only the already-authenticated core connection at one fixed descriptor.
    // SAFETY: `close_range` changes descriptor flags in this single-threaded
    // pre-GTK bootstrap and does not dereference pointers.
    if unsafe {
        libc::close_range(
            TRANSFERRED_CAPABILITY_FD as libc::c_uint,
            libc::c_uint::MAX,
            libc::CLOSE_RANGE_CLOEXEC as libc::c_int,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }

    let source_fd = stream.as_raw_fd();
    if source_fd == TRANSFERRED_CAPABILITY_FD {
        set_close_on_exec(source_fd, false)?;
    } else {
        // SAFETY: both descriptors are numeric process-local descriptors;
        // `dup3` atomically replaces descriptor three without following paths.
        if unsafe { libc::dup3(source_fd, TRANSFERRED_CAPABILITY_FD, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }

    let mut command = std::process::Command::new(&payload_path);
    command.args(env::args_os().skip(1));
    let error = command.exec();
    if source_fd != TRANSFERRED_CAPABILITY_FD {
        // SAFETY: descriptor three is the duplicate created immediately above.
        unsafe {
            libc::close(TRANSFERRED_CAPABILITY_FD);
        }
    }
    Err(error)
}

#[cfg(target_os = "linux")]
fn take_transferred_capability() -> io::Result<UnixStream> {
    set_close_on_exec(TRANSFERRED_CAPABILITY_FD, true)?;
    // SAFETY: this is the first action in the regular payload. The trusted
    // bootstrap transferred sole ownership of descriptor three across exec.
    Ok(unsafe { UnixStream::from_raw_fd(TRANSFERRED_CAPABILITY_FD) })
}

#[cfg(target_os = "linux")]
fn set_close_on_exec(descriptor: libc::c_int, enabled: bool) -> io::Result<()> {
    // SAFETY: `F_GETFD` only inspects flags for the supplied descriptor.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let next = if enabled {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    // SAFETY: `F_SETFD` writes only descriptor flags.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFD, next) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_desktop_payload_executable(
    expected_path: &Path,
    ids: ProcessIds,
    real_groups: &[libc::gid_t],
) -> Result<(), Error> {
    let current = env::current_exe().map_err(Error::Initialization)?;
    if current != expected_path {
        return Err(Error::PrivilegeContract(
            "desktop payload must execute from its fixed system path",
        ));
    }
    verify_trusted_payload_path(expected_path, ids, real_groups).map_err(Error::Initialization)
}

#[cfg(target_os = "linux")]
fn verify_trusted_payload_path(
    path: &Path,
    ids: ProcessIds,
    real_groups: &[libc::gid_t],
) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    for (index, component) in path.ancestors().enumerate() {
        let metadata = std::fs::symlink_metadata(component)?;
        let valid = if index == 0 {
            metadata.file_type().is_file()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == 0
                && metadata.gid() == 0
                && metadata.permissions().mode() & 0o7777 == 0o755
        } else {
            metadata.file_type().is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == 0
                && !writable_by_real_user(&metadata, ids.uid, ids.real, real_groups)
        };
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "desktop payload and every ancestor must be fixed, root-owned, and inaccessible for replacement by the desktop user",
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_transferred_capability(
    _kind: ClientKind,
    stream: &UnixStream,
    socket_path: &Path,
    ids: ProcessIds,
    _real_groups: &[libc::gid_t],
) -> Result<(), Error> {
    // The bootstrap already authenticated the protected pathname, socket mode,
    // group, and peer before dropping its one-time group capability. The
    // regular payload must no longer be able to traverse that namespace. Its
    // proof therefore comes from the inherited kernel object itself, not from
    // reopening a path that should now be inaccessible.
    if !matches!(
        std::fs::symlink_metadata(socket_path),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied
    ) {
        return Err(Error::PrivilegeContract(
            "desktop payload must not retain access to the protected core socket namespace",
        ));
    }

    let peer = stream.peer_addr().map_err(Error::Initialization)?;
    if peer.as_pathname() != Some(socket_path) {
        return Err(Error::PrivilegeContract(
            "transferred core capability peer must be the fixed client socket",
        ));
    }
    verify_stream_socket(stream).map_err(Error::Initialization)?;
    let credentials = peer_credentials(stream).map_err(Error::Initialization)?;
    if credentials.uid == ids.uid {
        return Err(Error::PrivilegeContract(
            "transferred core capability must terminate at a distinct service identity",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_stream_socket(stream: &UnixStream) -> io::Result<()> {
    let mut socket_type: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&socket_type) as libc::socklen_t;
    // SAFETY: the output value and length are valid for `SO_TYPE`.
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            std::ptr::addr_of_mut!(socket_type).cast(),
            &mut length,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of_val(&socket_type) || socket_type != libc::SOCK_STREAM {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "transferred core capability is not a Unix stream socket",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_initial_groups(kind: ClientKind, ids: ProcessIds) -> Result<(), Error> {
    let valid = if kind.requires_no_new_privs() {
        ids.real == ids.effective && ids.effective == ids.saved
    } else {
        ids.real != ids.effective && ids.effective == ids.saved
    };
    valid
        .then_some(())
        .ok_or(Error::PrivilegeContract(if kind.requires_no_new_privs() {
            "resident must start with identical real, effective, and saved group IDs"
        } else {
            "desktop clients must start setgid with matching effective and saved group IDs"
        }))
}

#[cfg(target_os = "linux")]
fn supplementary_groups() -> io::Result<Vec<libc::gid_t>> {
    // SAFETY: a zero-sized query uses a null destination by contract.
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if count < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut groups = vec![0; count as usize];
    if count > 0 {
        // SAFETY: `groups` has capacity for exactly `count` gid values.
        let written = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
        if written < 0 {
            return Err(io::Error::last_os_error());
        }
        groups.truncate(written as usize);
    }
    Ok(groups)
}

#[cfg(target_os = "linux")]
fn verify_socket_path(
    kind: ClientKind,
    socket_path: &Path,
    ids: ProcessIds,
    real_groups: &[libc::gid_t],
) -> Result<(), Error> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

    let parent = socket_path.parent().ok_or(Error::PrivilegeContract(
        "core socket has no parent directory",
    ))?;
    for ancestor in parent.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor).map_err(Error::Initialization)?;
        if !metadata.file_type().is_dir()
            || metadata.file_type().is_symlink()
            || writable_by_real_user(&metadata, ids.uid, ids.real, real_groups)
        {
            return Err(Error::PrivilegeContract(
                "every core socket ancestor must be a non-symlink directory not writable by the real user",
            ));
        }
    }

    let socket_metadata = std::fs::symlink_metadata(socket_path).map_err(Error::Initialization)?;
    if !socket_metadata.file_type().is_socket()
        || socket_metadata.file_type().is_symlink()
        || socket_metadata.gid() != ids.effective
        || socket_metadata.permissions().mode() & 0o7777 != 0o660
    {
        return Err(Error::PrivilegeContract(
            "core capability socket must be a non-symlink socket in mode 0660 owned by the effective group",
        ));
    }
    if !kind.requires_no_new_privs()
        && writable_by_real_user(&socket_metadata, ids.uid, ids.real, real_groups)
    {
        return Err(Error::PrivilegeContract(
            "core capability socket must not be writable by the desktop user",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn writable_by_real_user(
    metadata: &std::fs::Metadata,
    uid: libc::uid_t,
    gid: libc::gid_t,
    groups: &[libc::gid_t],
) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let mode = metadata.permissions().mode();
    (metadata.uid() == uid && mode & 0o200 != 0)
        || ((metadata.gid() == gid || groups.contains(&metadata.gid())) && mode & 0o020 != 0)
        || mode & 0o002 != 0
}

#[cfg(target_os = "linux")]
fn verify_peer_owner(stream: &UnixStream, socket_path: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let credentials = peer_credentials(stream)?;
    let socket_owner = std::fs::symlink_metadata(socket_path)?.uid();
    if credentials.uid != socket_owner {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "core peer credentials do not match the capability socket owner",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn peer_credentials(stream: &UnixStream) -> io::Result<libc::ucred> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the output buffer and its length are valid for `SO_PEERCRED`.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::addr_of_mut!(credentials).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() || credentials.pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "core peer credentials are incomplete",
        ));
    }
    Ok(credentials)
}

#[cfg(target_os = "linux")]
fn permanently_drop_group(real_gid: libc::gid_t) -> io::Result<()> {
    // SAFETY: `setresgid` accepts numeric group IDs; all three are deliberately
    // set to the unprivileged real group so the capability cannot be regained.
    if unsafe { libc::setresgid(real_gid, real_gid, real_gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let ids = process_ids()?;
    if ids.real != real_gid || ids.effective != real_gid || ids.saved != real_gid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "group capability was not permanently dropped",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn harden_process(kind: ClientKind) -> io::Result<()> {
    disable_dumpability()?;
    if kind.requires_no_new_privs() {
        // SAFETY: `PR_SET_NO_NEW_PRIVS` requires the value one and zero padding.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: these getter operations have no pointer arguments.
    let dumpable = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
    // SAFETY: these getter operations have no pointer arguments.
    let no_new_privs = unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) };
    let expected_no_new_privs = i32::from(kind.requires_no_new_privs());
    // Desktop applications deliberately retain NNP=0: they have already
    // consumed and permanently dropped their own capability, but must still be
    // able to exec a different setgid Goblins OS application. Resident never
    // cross-launches and is the sole client hardened with NNP=1.
    if dumpable != 0 || no_new_privs != expected_no_new_privs {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "process hardening state could not be verified",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn disable_dumpability() -> io::Result<()> {
    // A setgid exec may reset dumpability according to system policy. Enforce
    // the private state before the capability socket retry window, then again
    // after permanently dropping the elevated group.
    // SAFETY: these `prctl` operations take integer constants and no pointers.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: this getter has no pointer arguments.
    if unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "process dumpability could not be disabled",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn register_child_close_handler() -> io::Result<()> {
    // SAFETY: the child callback only performs atomic stores and `close`, and
    // has the exact `pthread_atfork` C ABI signature.
    let result = unsafe { libc::pthread_atfork(None, None, Some(close_core_fd_in_child)) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result))
    }
}

#[cfg(target_os = "linux")]
extern "C" fn close_core_fd_in_child() {
    FORKED_CHILD.store(true, Ordering::Release);
    let descriptor = ACTIVE_FD.swap(-1, Ordering::AcqRel);
    if descriptor >= 0 {
        // SAFETY: the descriptor was captured from the live core UnixStream;
        // close is async-signal-safe and the child is forbidden from reusing it.
        unsafe {
            libc::close(descriptor);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        os::{fd::AsRawFd, unix::net::UnixListener},
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

    struct TestSocket(PathBuf);

    impl TestSocket {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            Self(env::temp_dir().join(format!(
                "goblins-core-client-{}-{nonce}-{}.sock",
                std::process::id(),
                NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }

    impl Drop for TestSocket {
        fn drop(&mut self) {
            match std::fs::remove_file(&self.0) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => panic!("remove test socket {}: {error}", self.0.display()),
            }
        }
    }

    fn read_request(mut stream: &UnixStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("server read timeout");
        let mut bytes = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let count = stream.read(&mut buffer).expect("read request");
            assert!(count > 0);
            bytes.extend_from_slice(&buffer[..count]);
            let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
            let body_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .map(|value| value.parse::<usize>().unwrap())
                .unwrap_or_default();
            if bytes.len() >= header_end + 4 + body_length {
                return String::from_utf8(bytes).unwrap();
            }
        }
    }

    #[test]
    fn client_kind_paths_are_fixed_and_complete() {
        let cases = [
            (ClientKind::ControlCenter, "control-center"),
            (ClientKind::Dictate, "dictate"),
            (ClientKind::FileBuilder, "file-builder"),
            (ClientKind::FocusTick, "focus-tick"),
            (ClientKind::Installer, "installer"),
            (ClientKind::Launcher, "launcher"),
            (ClientKind::Login, "login"),
            (ClientKind::Markup, "markup"),
            (ClientKind::Open, "open"),
            (ClientKind::Resident, "resident"),
            (ClientKind::ScreenshotContext, "screenshot-context"),
            (ClientKind::Settings, "settings"),
            (ClientKind::Shell, "shell"),
            (ClientKind::Today, "today"),
            (ClientKind::VisualLookup, "visual-lookup"),
            (ClientKind::VoiceControl, "voice-control"),
        ];
        for (kind, slug) in cases {
            assert_eq!(kind.slug(), slug);
            assert_eq!(
                kind.socket_path(),
                PathBuf::from(format!("/run/goblins-os-core/{slug}/control.sock"))
            );
        }
        assert!(ClientKind::Resident.requires_no_new_privs());
        for desktop in [
            ClientKind::ControlCenter,
            ClientKind::Dictate,
            ClientKind::FileBuilder,
            ClientKind::FocusTick,
            ClientKind::Installer,
            ClientKind::Launcher,
            ClientKind::Login,
            ClientKind::Markup,
            ClientKind::Open,
            ClientKind::ScreenshotContext,
            ClientKind::Settings,
            ClientKind::Shell,
            ClientKind::Today,
            ClientKind::VisualLookup,
            ClientKind::VoiceControl,
        ] {
            assert!(!desktop.requires_no_new_privs(), "{}", desktop.slug());
        }
    }

    #[test]
    fn persistent_connection_handles_get_post_delete_and_exposes_headers() {
        let socket = TestSocket::new();
        let listener = UnixListener::bind(&socket.0).expect("bind socket");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept once");
            let mut requests = Vec::new();
            for response in [
                b"HTTP/1.1 200 OK\r\nX-Core: ready\r\nContent-Length: 2\r\n\r\n{}".as_slice(),
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".as_slice(),
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ndone\r\n0\r\n\r\n"
                    .as_slice(),
            ] {
                requests.push(read_request(&stream));
                stream.write_all(response).expect("write response");
            }
            requests
        });

        let client = CoreClient::connect_test(ClientKind::Open, &socket.0).unwrap();
        let response = client.get("/health", Duration::from_secs(1)).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.header("X-CORE"), Some("ready"));
        assert_eq!(response.body, b"{}");
        assert!(client
            .post_json("/v1/value", br#"{"value":1}"#, Duration::from_secs(1))
            .unwrap()
            .is_success());
        assert_eq!(
            client
                .delete("/v1/value", Duration::from_secs(1))
                .unwrap()
                .body,
            b"done"
        );

        let requests = server.join().expect("server");
        assert!(requests[0].starts_with("GET /health HTTP/1.1\r\n"));
        assert!(requests[0].contains("\r\nHost: localhost\r\n"));
        assert!(requests[0].contains("\r\nUser-Agent: goblins-os-open\r\n"));
        assert!(requests[0].contains("\r\nConnection: keep-alive\r\n"));
        assert!(requests[1].starts_with("POST /v1/value HTTP/1.1\r\n"));
        assert!(requests[1].ends_with("\r\n\r\n{\"value\":1}"));
        assert!(requests[2].starts_with("DELETE /v1/value HTTP/1.1\r\n"));
    }

    #[test]
    fn broken_connection_never_reconnects() {
        let socket = TestSocket::new();
        let listener = UnixListener::bind(&socket.0).expect("bind socket");
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("only connection");
            let _ = read_request(&stream);
            drop(stream);
            listener.set_nonblocking(true).unwrap();
            thread::sleep(Duration::from_millis(100));
            assert!(
                matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
            );
        });
        let client = CoreClient::connect_test(ClientKind::Today, &socket.0).unwrap();
        assert!(matches!(
            client.get("/health", Duration::from_secs(1)),
            Err(Error::ConnectionBroken(_))
        ));
        assert!(matches!(
            client.get("/health", Duration::from_secs(1)),
            Err(Error::RelaunchRequired)
        ));
        server.join().expect("server");
    }

    #[test]
    fn timeouts_bounds_and_request_injection_fail_closed() {
        assert!(matches!(
            validate_request(
                Method::Get,
                "http://example.com",
                None,
                Duration::from_secs(1)
            ),
            Err(Error::InvalidPath)
        ));
        assert!(matches!(
            validate_request(
                Method::Get,
                "/ok\r\nX: injected",
                None,
                Duration::from_secs(1)
            ),
            Err(Error::InvalidPath)
        ));
        assert!(matches!(
            validate_request(Method::Delete, "/ok", Some(b"body"), Duration::from_secs(1)),
            Err(Error::BodyNotAllowed(Method::Delete))
        ));
        assert!(matches!(
            validate_request(Method::Get, "/ok", None, Duration::ZERO),
            Err(Error::InvalidReadTimeout)
        ));
        assert!(validate_request(Method::Get, "/ok", None, MAX_READ_TIMEOUT).is_ok());
        assert!(matches!(
            validate_request(
                Method::Get,
                "/ok",
                None,
                MAX_READ_TIMEOUT + Duration::from_secs(1)
            ),
            Err(Error::InvalidReadTimeout)
        ));
        assert!(matches!(
            validate_request(
                Method::Post,
                "/ok",
                Some(&vec![0; MAX_REQUEST_BODY_BYTES + 1]),
                Duration::from_secs(1)
            ),
            Err(Error::RequestBodyTooLarge)
        ));
    }

    #[test]
    fn response_header_and_body_limits_are_enforced() {
        let huge_header = vec![b'x'; MAX_RESPONSE_HEADER_BYTES];
        let mut reader = io::BufReader::new(huge_header.as_slice());
        assert!(matches!(
            read_response_head(&mut reader),
            Err(TransportFailure::ResponseTooLarge)
        ));

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_RESPONSE_BODY_BYTES + 1
        );
        let (client, mut server) = UnixStream::pair().unwrap();
        server.write_all(response.as_bytes()).unwrap();
        let mut reader = BufReader::new(client);
        assert!(matches!(
            read_response(&mut reader),
            Err(TransportFailure::ResponseTooLarge)
        ));
    }

    #[test]
    fn connected_socket_is_close_on_exec() {
        let socket = TestSocket::new();
        let listener = UnixListener::bind(&socket.0).expect("bind socket");
        let server = thread::spawn(move || listener.accept().unwrap().0);
        let stream = connect_socket_with_timeout(&socket.0, CONNECT_TIMEOUT).unwrap();
        // SAFETY: `F_GETFD` only reads flags for this valid live descriptor.
        let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
        drop(stream);
        drop(server.join().expect("server"));
    }

    #[test]
    fn initial_connect_retries_only_until_one_socket_becomes_ready() {
        let socket = TestSocket::new();
        let path = socket.0.clone();
        let server = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let listener = UnixListener::bind(&path).expect("bind delayed socket");
            listener.accept().expect("accept initial connection").0
        });
        let stream = connect_initial_socket(&socket.0).expect("retry initial connection");
        drop(stream);
        drop(server.join().expect("server"));
    }

    #[test]
    fn dangerous_environment_names_cover_loaders_frameworks_and_legacy_core_routes() {
        for name in [
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "GLIBC_TUNABLES",
            "GCONV_PATH",
            "GTK_MODULES",
            "GDK_PIXBUF_MODULE_FILE",
            "GIO_EXTRA_MODULES",
            "GI_TYPELIB_PATH",
            "QT_PLUGIN_PATH",
            "GOBLINS_OS_CORE_URL",
            "GOBLINS_OS_CORE_SOCKET",
            "GOBLINS_OS_CORE_PORT",
            "GOBLINS_OS_CORE_FUTURE_TRANSPORT",
            "OPENAI_OS_CORE_URL",
            "OPENAI_OS_CORE_SOCKET",
            "OPENAI_OS_CORE_PORT",
            "OPENAI_OS_CORE_FUTURE_TRANSPORT",
        ] {
            assert!(sanitized_environment_name(name), "{name}");
        }
        assert!(!sanitized_environment_name("LANG"));
        assert!(!sanitized_environment_name("WAYLAND_DISPLAY"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_and_resident_group_contracts_are_distinct() {
        let desktop = ProcessIds {
            real: 1000,
            effective: 2000,
            saved: 2000,
            uid: 1000,
        };
        assert!(validate_initial_groups(ClientKind::Settings, desktop).is_ok());
        assert!(validate_initial_groups(ClientKind::Resident, desktop).is_err());

        let resident = ProcessIds {
            real: 2000,
            effective: 2000,
            saved: 2000,
            uid: 2000,
        };
        assert!(validate_initial_groups(ClientKind::Resident, resident).is_ok());
        assert!(validate_initial_groups(ClientKind::Settings, resident).is_err());
    }
}
