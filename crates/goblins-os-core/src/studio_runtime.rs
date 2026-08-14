//! Networkless, read-only execution for a Build Studio project's fixed local
//! preview entrypoint. Generated source is untrusted: it receives only the
//! project directory, the immutable `/usr` runtime, private temporary storage,
//! and a short lifetime. It never inherits the core service environment.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    os::{
        fd::AsRawFd,
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use rand::{rngs::OsRng, RngCore as _};

use crate::bounded::{bounded_output_of, BoundedCommandError};

#[cfg(target_os = "linux")]
const BWRAP: &str = "/usr/bin/bwrap";
#[cfg(not(target_os = "linux"))]
const BWRAP: &str = "/nonexistent/goblins-os-bwrap";
#[cfg(target_os = "linux")]
const PYTHON: &str = "/usr/bin/python3";
#[cfg(not(target_os = "linux"))]
const PYTHON: &str = "/nonexistent/goblins-os-python3";
const SANDBOX_WORKSPACE: &str = "/workspace";
const RUN_TIMEOUT: Duration = Duration::from_secs(20);
const LOG_BYTES: usize = 64 * 1024;
const PREVIEW_LIFETIME: Duration = Duration::from_secs(10 * 60);
const PREVIEW_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const PREVIEW_REQUEST_BYTES: u64 = 16 * 1024;
const PREVIEW_REQUEST_LIMIT: usize = 512;
const MAX_ACTIVE_PREVIEWS: usize = 4;

static ACTIVE_PREVIEWS: OnceLock<Mutex<BTreeMap<String, ActivePreview>>> = OnceLock::new();
static PREVIEW_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const PYTHON_ENTRYPOINTS: &[&str] = &["main.py", "app.py", "src/main.py"];
const WEB_ENTRYPOINTS: &[&str] = &["index.html", "public/index.html"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StudioRuntimeStatus {
    pub(crate) available: bool,
    pub(crate) kind: &'static str,
    pub(crate) entrypoint: Option<String>,
    pub(crate) detail: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct StudioRuntimeRun {
    pub(crate) state: &'static str,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) logs_truncated: bool,
    pub(crate) detail: String,
}

struct ActivePreview {
    token: String,
    sequence: u64,
    stop: Arc<AtomicBool>,
    server: thread::JoinHandle<()>,
}

pub(crate) fn runtime_status(files: &[String]) -> StudioRuntimeStatus {
    if let Some(entrypoint) = web_entrypoint(files) {
        return StudioRuntimeStatus {
            available: true,
            kind: "static-web",
            entrypoint: Some(entrypoint.to_string()),
            detail: format!(
                "Previews {entrypoint} from a private, ten-minute loopback snapshot with scripts sandboxed and all external network access blocked."
            ),
        };
    }
    let Some(entrypoint) = python_entrypoint(files) else {
        return StudioRuntimeStatus {
            available: false,
            kind: "unavailable",
            entrypoint: None,
            detail: "Add index.html, public/index.html, main.py, app.py, or src/main.py to preview this project locally."
                .to_string(),
        };
    };
    if !executable_is_ready(BWRAP) || !executable_is_ready(PYTHON) {
        return StudioRuntimeStatus {
            available: false,
            kind: "python",
            entrypoint: Some(entrypoint.to_string()),
            detail: "The networkless local preview runtime is not installed in this Goblins OS image. The project remains available for review and export."
                .to_string(),
        };
    }
    StudioRuntimeStatus {
        available: true,
        kind: "python",
        entrypoint: Some(entrypoint.to_string()),
        detail: format!(
            "Runs {entrypoint} for at most 20 seconds in a networkless sandbox with the project mounted read-only."
        ),
    }
}

/// Serve an immutable in-memory snapshot of a generated static site through a
/// tokenized loopback URL. The response sandbox blocks navigation, forms,
/// downloads, workers, frames, and every external network request; the server
/// has a short fixed lifetime and request count even if the browser remains
/// open. No generated code runs in the privileged core process.
pub(crate) fn open_web_preview(
    app_id: &str,
    files: BTreeMap<String, Vec<u8>>,
    entrypoint: &str,
) -> StudioRuntimeRun {
    let Some(site_root) = web_site_root(entrypoint) else {
        return unavailable_run("That static web entrypoint is not allowlisted for preview.");
    };
    if !files.contains_key(entrypoint) {
        return unavailable_run("The static web entrypoint is missing from the reviewed snapshot.");
    }
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(_) => return unavailable_run("The private local preview server could not start."),
    };
    let port = match listener.local_addr() {
        Ok(address) => address.port(),
        Err(_) => return unavailable_run("The private local preview address is unavailable."),
    };
    if listener.set_nonblocking(true).is_err() {
        return unavailable_run("The private local preview server could not be bounded safely.");
    }

    let mut token_bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut token_bytes);
    let token = hex_bytes(&token_bytes);
    let url = format!("http://127.0.0.1:{port}/{token}/");
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server_token = token.clone();
    let site_root = site_root.to_string();
    let server = thread::spawn(move || {
        serve_preview(
            listener,
            &server_token,
            &site_root,
            files,
            server_stop,
            PREVIEW_REQUEST_LIMIT,
        )
    });
    register_active_preview(
        app_id,
        ActivePreview {
            token: token.clone(),
            sequence: PREVIEW_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            stop,
            server,
        },
    );

    match crate::session_bridge::open_studio_web_preview(&url) {
        crate::session_bridge::SessionBridgeResult::Success(_) => {
            StudioRuntimeRun {
                state: "preview-opened",
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                logs_truncated: false,
                detail: "Opened a sandboxed local preview. Its private snapshot stays available for up to ten minutes."
                    .to_string(),
            }
        }
        crate::session_bridge::SessionBridgeResult::Failed(detail) => {
            stop_active_preview(app_id, &token);
            unavailable_run(&format!(
                "The desktop could not open the static preview: {detail}"
            ))
        }
        crate::session_bridge::SessionBridgeResult::Unavailable => {
            stop_active_preview(app_id, &token);
            unavailable_run("The desktop preview service is unavailable in this session.")
        }
    }
}

fn active_previews() -> &'static Mutex<BTreeMap<String, ActivePreview>> {
    ACTIVE_PREVIEWS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn register_active_preview(app_id: &str, preview: ActivePreview) {
    register_active_preview_in(active_previews(), app_id, preview);
}

fn register_active_preview_in(
    registry: &Mutex<BTreeMap<String, ActivePreview>>,
    app_id: &str,
    preview: ActivePreview,
) {
    let mut retired = Vec::new();
    {
        let mut active = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let finished = active
            .iter()
            .filter_map(|(id, preview)| preview.server.is_finished().then_some(id.clone()))
            .collect::<Vec<_>>();
        for id in finished {
            if let Some(preview) = active.remove(&id) {
                retired.push(preview);
            }
        }
        if let Some(previous) = active.remove(app_id) {
            retired.push(previous);
        }
        if active.len() >= MAX_ACTIVE_PREVIEWS {
            let oldest = active
                .iter()
                .min_by_key(|(_, preview)| preview.sequence)
                .map(|(id, _)| id.clone());
            if let Some(oldest) = oldest.and_then(|id| active.remove(&id)) {
                retired.push(oldest);
            }
        }
        active.insert(app_id.to_string(), preview);
    }
    retire_previews(retired);
}

fn stop_active_preview(app_id: &str, token: &str) {
    let preview = {
        let mut active = active_previews()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active
            .get(app_id)
            .is_some_and(|preview| preview.token == token)
        {
            active.remove(app_id)
        } else {
            None
        }
    };
    if let Some(preview) = preview {
        retire_previews(vec![preview]);
    }
}

fn retire_previews(previews: Vec<ActivePreview>) {
    for preview in &previews {
        preview.stop.store(true, Ordering::Release);
    }
    for preview in previews {
        let _ = preview.server.join();
    }
}

pub(crate) fn run_python(workspace: File, files: &[String]) -> StudioRuntimeRun {
    let status = runtime_status(files);
    if !status.available {
        return StudioRuntimeRun {
            state: "unavailable",
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            logs_truncated: false,
            detail: status.detail,
        };
    }
    let entrypoint = status
        .entrypoint
        .expect("available Studio runtime has an entrypoint");
    let workspace_fd = workspace.as_raw_fd();
    let mut command = Command::new(BWRAP);
    command.env_clear();
    command
        .args([
            "--ro-bind",
            "/usr",
            "/usr",
            "--symlink",
            "usr/bin",
            "/bin",
            "--symlink",
            "usr/lib",
            "/lib",
            "--symlink",
            "usr/lib64",
            "/lib64",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--dir",
            "/home",
            "--dir",
            "/home/studio",
        ])
        .arg("--ro-bind-fd")
        .arg(workspace_fd.to_string())
        .arg(SANDBOX_WORKSPACE)
        .args([
            "--remount-ro",
            "/",
            "--unshare-all",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
            "--clearenv",
            "--setenv",
            "PATH",
            "/usr/bin:/bin",
            "--setenv",
            "HOME",
            "/home/studio",
            "--setenv",
            "LANG",
            "C.UTF-8",
            "--chdir",
            SANDBOX_WORKSPACE,
            "--",
            PYTHON,
            "-I",
            "-B",
            "-c",
        ])
        .arg(format!(
            "import runpy,sys; sys.path.insert(0, '/workspace'); runpy.run_path('/workspace/{entrypoint}', run_name='__main__')"
        ));

    // SAFETY: the child-side closure performs only fixed `prctl` and `fcntl`
    // syscalls against a descriptor owned by this function and allocates
    // nothing. The workspace file remains alive through `spawn` and the bound.
    unsafe {
        command.pre_exec(move || {
            #[cfg(target_os = "linux")]
            {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                apply_run_limits()?;
            }
            let flags = libc::fcntl(workspace_fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(workspace_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    match bounded_output_of(&mut command, RUN_TIMEOUT) {
        Ok(output) => {
            let (stdout, stdout_truncated) = bounded_text(&output.stdout);
            let (stderr, stderr_truncated) = bounded_text(&output.stderr);
            let succeeded = output.status.success();
            StudioRuntimeRun {
                state: if succeeded { "completed" } else { "failed" },
                exit_code: output.status.code(),
                stdout,
                stderr,
                logs_truncated: stdout_truncated || stderr_truncated,
                detail: if succeeded {
                    "Local run completed in the networkless Studio sandbox.".to_string()
                } else {
                    "The project exited with an error. Review the captured logs below.".to_string()
                },
            }
        }
        Err(BoundedCommandError::TimedOut) => StudioRuntimeRun {
            state: "timed-out",
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            logs_truncated: false,
            detail:
                "The local run reached its 20-second limit and was stopped with its descendants."
                    .to_string(),
        },
        Err(BoundedCommandError::Missing) => StudioRuntimeRun {
            state: "unavailable",
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            logs_truncated: false,
            detail: "The networkless local preview runtime is not installed in this image."
                .to_string(),
        },
        Err(BoundedCommandError::Failed) => StudioRuntimeRun {
            state: "failed",
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            logs_truncated: false,
            detail: "The local Studio sandbox could not start safely.".to_string(),
        },
    }
}

fn unavailable_run(detail: &str) -> StudioRuntimeRun {
    StudioRuntimeRun {
        state: "unavailable",
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        logs_truncated: false,
        detail: detail.to_string(),
    }
}

fn web_site_root(entrypoint: &str) -> Option<&'static str> {
    match entrypoint {
        "index.html" => Some(""),
        "public/index.html" => Some("public"),
        _ => None,
    }
}

fn serve_preview(
    listener: TcpListener,
    token: &str,
    site_root: &str,
    files: BTreeMap<String, Vec<u8>>,
    stop: Arc<AtomicBool>,
    request_limit: usize,
) {
    let started = Instant::now();
    let expected_host = listener
        .local_addr()
        .map(|address| format!("127.0.0.1:{}", address.port()))
        .unwrap_or_default();
    let mut served = 0_usize;
    while !stop.load(Ordering::Acquire)
        && started.elapsed() < PREVIEW_LIFETIME
        && served < request_limit
    {
        match listener.accept() {
            Ok((mut stream, peer)) if peer.ip().is_loopback() => {
                let _ = stream.set_read_timeout(Some(PREVIEW_REQUEST_TIMEOUT));
                let _ = stream.set_write_timeout(Some(PREVIEW_REQUEST_TIMEOUT));
                serve_preview_request(&mut stream, token, site_root, &files, &expected_host);
                served = served.saturating_add(1);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break,
        }
    }
}

fn serve_preview_request(
    stream: &mut TcpStream,
    token: &str,
    site_root: &str,
    files: &BTreeMap<String, Vec<u8>>,
    expected_host: &str,
) {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while request.len() < PREVIEW_REQUEST_BYTES as usize {
        let remaining = PREVIEW_REQUEST_BYTES as usize - request.len();
        let capacity = remaining.min(chunk.len());
        match stream.read(&mut chunk[..capacity]) {
            Ok(0) => break,
            Ok(read) => {
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n")
                    || request.windows(2).any(|window| window == b"\n\n")
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let Ok(request) = std::str::from_utf8(&request) else {
        write_preview_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"Bad request",
            false,
        );
        return;
    };
    let Some(header_end) = request.find("\r\n\r\n").or_else(|| request.find("\n\n")) else {
        write_preview_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"Bad request",
            false,
        );
        return;
    };
    let headers = &request[..header_end];
    let mut lines = headers.lines();
    let Some(request_line) = lines.next() else {
        write_preview_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"Bad request",
            false,
        );
        return;
    };
    let mut request_parts = request_line.split_ascii_whitespace();
    let Some(method) = request_parts.next() else {
        write_preview_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"Bad request",
            false,
        );
        return;
    };
    let Some(target) = request_parts.next() else {
        write_preview_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"Bad request",
            false,
        );
        return;
    };
    if request_parts.next().is_none() || request_parts.next().is_some() {
        write_preview_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"Bad request",
            false,
        );
        return;
    }
    let header_lines = lines.collect::<Vec<_>>();
    let mut hosts = header_lines.iter().filter_map(|line| {
        line.split_once(':')
            .filter(|(name, _)| name.trim().eq_ignore_ascii_case("host"))
            .map(|(_, value)| value.trim())
    });
    if hosts.next() != Some(expected_host) || hosts.next().is_some() {
        write_preview_response(
            stream,
            421,
            "text/plain; charset=utf-8",
            b"Misdirected request",
            false,
        );
        return;
    }
    let head_only = method == "HEAD";
    if method != "GET" && !head_only {
        write_preview_response(
            stream,
            405,
            "text/plain; charset=utf-8",
            b"Method not allowed",
            false,
        );
        return;
    }
    let token_prefix = format!("/{token}/");
    let cookie_name = preview_cookie_name(token);
    let cookie_authorized = header_lines.iter().filter_map(|line| {
        line.split_once(':')
            .filter(|(name, _)| name.trim().eq_ignore_ascii_case("cookie"))
            .map(|(_, value)| value.trim())
    });
    let cookie_authorized = cookie_authorized
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .any(|(name, value)| name == cookie_name && value == token);
    let (encoded_path, set_cookie) = if let Some(encoded_path) = target.strip_prefix(&token_prefix)
    {
        (encoded_path, true)
    } else if cookie_authorized {
        let Some(encoded_path) = target.strip_prefix('/') else {
            write_preview_response(
                stream,
                400,
                "text/plain; charset=utf-8",
                b"Bad request",
                head_only,
            );
            return;
        };
        (encoded_path, false)
    } else {
        write_preview_response(
            stream,
            404,
            "text/plain; charset=utf-8",
            b"Not found",
            head_only,
        );
        return;
    };
    if encoded_path.contains(['?', '#', '\\']) {
        write_preview_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"Bad request",
            head_only,
        );
        return;
    }
    let Some(relative) = decode_preview_path(encoded_path) else {
        write_preview_response(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"Bad request",
            head_only,
        );
        return;
    };
    let relative = if relative.is_empty() {
        "index.html".to_string()
    } else if relative.ends_with('/') {
        format!("{relative}index.html")
    } else {
        relative
    };
    let workspace_path = if site_root.is_empty() {
        relative
    } else {
        format!("{site_root}/{relative}")
    };
    let Some(content) = files.get(&workspace_path) else {
        write_preview_response(
            stream,
            404,
            "text/plain; charset=utf-8",
            b"Not found",
            head_only,
        );
        return;
    };
    write_preview_response_with_cookie(
        stream,
        200,
        preview_content_type(&workspace_path),
        content,
        head_only,
        set_cookie.then_some((cookie_name.as_str(), token)),
    );
}

fn preview_cookie_name(token: &str) -> String {
    let suffix = token.get(..16).unwrap_or(token);
    format!("goblins_studio_{suffix}")
}

fn decode_preview_path(encoded: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .ok()?;
    if decoded.starts_with('/') || decoded.contains('\0') || decoded.contains('\\') {
        return None;
    }
    let mut clean = Vec::new();
    for component in decoded.split('/') {
        if component.is_empty() {
            continue;
        }
        if component == "." || component == ".." || component.chars().any(char::is_control) {
            return None;
        }
        clean.push(component);
    }
    let mut path = clean.join("/");
    if decoded.ends_with('/') && !path.is_empty() {
        path.push('/');
    }
    Some(path)
}

fn preview_content_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn write_preview_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    content: &[u8],
    head_only: bool,
) {
    write_preview_response_with_cookie(stream, status, content_type, content, head_only, None);
}

fn write_preview_response_with_cookie(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    content: &[u8],
    head_only: bool,
    cookie: Option<(&str, &str)>,
) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        421 => "Misdirected Request",
        _ => "Error",
    };
    let set_cookie = cookie
        .map(|(name, value)| {
            format!(
                "Set-Cookie: {name}={value}; Path=/; Max-Age=600; HttpOnly; SameSite=Strict\r\n"
            )
        })
        .unwrap_or_default();
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{set_cookie}Content-Security-Policy: sandbox allow-scripts allow-modals allow-same-origin; default-src 'self' data: blob:; connect-src 'self'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; worker-src 'none'; script-src 'self' 'unsafe-inline' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' data: blob:; font-src 'self' data:\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Resource-Policy: same-origin\r\nPermissions-Policy: camera=(), microphone=(), geolocation=(), usb=(), payment=()\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        content.len()
    );
    let _ = stream.write_all(headers.as_bytes());
    if !head_only {
        let _ = stream.write_all(content);
    }
    let _ = stream.flush();
}

fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(target_os = "linux")]
fn apply_run_limits() -> io::Result<()> {
    let limits = [
        (libc::RLIMIT_CPU, 10),
        (libc::RLIMIT_AS, 512 * 1024 * 1024),
        (libc::RLIMIT_FSIZE, 8 * 1024 * 1024),
        (libc::RLIMIT_NOFILE, 128),
        (libc::RLIMIT_NPROC, 64),
        (libc::RLIMIT_CORE, 0),
    ];
    for (resource, value) in limits {
        let limit = libc::rlimit {
            rlim_cur: value,
            rlim_max: value,
        };
        // SAFETY: `limit` points to a fully initialized rlimit structure and
        // each resource identifier is a fixed platform constant.
        if unsafe { libc::setrlimit(resource, &limit) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn python_entrypoint(files: &[String]) -> Option<&'static str> {
    PYTHON_ENTRYPOINTS
        .iter()
        .copied()
        .find(|candidate| files.iter().any(|file| file == candidate))
}

pub(crate) fn web_entrypoint(files: &[String]) -> Option<&'static str> {
    WEB_ENTRYPOINTS
        .iter()
        .copied()
        .find(|candidate| files.iter().any(|file| file == candidate))
}

fn executable_is_ready(path: &str) -> bool {
    Path::new(path)
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn bounded_text(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() > LOG_BYTES;
    let end = bytes.len().min(LOG_BYTES);
    (
        String::from_utf8_lossy(&bytes[..end]).into_owned(),
        truncated,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        decode_preview_path, preview_cookie_name, python_entrypoint, register_active_preview_in,
        retire_previews, runtime_status, serve_preview, web_entrypoint, ActivePreview,
        MAX_ACTIVE_PREVIEWS, PREVIEW_REQUEST_LIMIT,
    };
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::{Shutdown, TcpListener, TcpStream},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread,
        time::Duration,
    };

    #[test]
    fn fixed_entrypoints_are_deterministic_and_never_manifest_commands() {
        let files = vec![
            "package.json".to_string(),
            "src/main.py".to_string(),
            "app.py".to_string(),
        ];
        assert_eq!(python_entrypoint(&files), Some("app.py"));
        assert_eq!(web_entrypoint(&files), None);
    }

    #[test]
    fn web_projects_select_the_bounded_static_preview() {
        let status = runtime_status(&["index.html".to_string()]);
        assert!(status.available);
        assert_eq!(status.kind, "static-web");
        assert_eq!(status.entrypoint.as_deref(), Some("index.html"));
        assert!(status.detail.contains("loopback snapshot"));
        assert!(status.detail.contains("network access blocked"));
    }

    #[test]
    fn preview_paths_decode_assets_but_reject_traversal_and_ambiguity() {
        assert_eq!(
            decode_preview_path("assets/hello%20world.css").as_deref(),
            Some("assets/hello world.css")
        );
        assert_eq!(decode_preview_path("docs/").as_deref(), Some("docs/"));
        assert!(decode_preview_path("../secret").is_none());
        assert!(decode_preview_path("%2e%2e/secret").is_none());
        assert!(decode_preview_path("assets%2f..%2fsecret").is_none());
        assert!(decode_preview_path("%2Fabsolute").is_none());
        assert!(decode_preview_path("bad%00path").is_none());
    }

    #[test]
    fn preview_http_serves_same_origin_assets_and_blocks_external_connections() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("preview listener");
        let port = listener.local_addr().expect("preview address").port();
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let token = "a".repeat(64);
        let files = BTreeMap::from([
            (
                "index.html".to_string(),
                b"<link rel=\"stylesheet\" href=\"assets/app.css\"><script src=\"assets/app.js\"></script>"
                    .to_vec(),
            ),
            (
                "assets/app.js".to_string(),
                b"document.body.dataset.ready='yes';".to_vec(),
            ),
            (
                "assets/app.css".to_string(),
                b"body { color: green; }".to_vec(),
            ),
            (
                "assets/root.js".to_string(),
                b"document.body.dataset.root='yes';".to_vec(),
            ),
        ]);
        let stop = Arc::new(AtomicBool::new(false));
        let server_token = token.clone();
        let server_stop = Arc::clone(&stop);
        let server = thread::spawn(move || {
            serve_preview(
                listener,
                &server_token,
                "",
                files,
                server_stop,
                PREVIEW_REQUEST_LIMIT,
            )
        });

        let index = preview_test_get(port, &token, "");
        assert!(index.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(index.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(index.contains("default-src 'self' data: blob:"));
        assert!(index.contains("sandbox allow-scripts allow-modals allow-same-origin"));
        assert!(index.contains("connect-src 'self'"));
        assert!(index.contains("form-action 'none'"));
        assert!(index.contains("worker-src 'none'"));
        assert!(index.contains("script-src 'self' 'unsafe-inline' blob:"));
        assert!(index.contains("style-src 'self' 'unsafe-inline'"));
        assert!(index.contains("href=\"assets/app.css\""));
        let cookie = index
            .lines()
            .find_map(|line| line.strip_prefix("Set-Cookie: "))
            .and_then(|cookie| cookie.split(';').next())
            .expect("preview capability cookie");
        assert!(cookie.starts_with(&format!("{}=", preview_cookie_name(&token))));
        assert!(index.contains("HttpOnly; SameSite=Strict"));

        let javascript = preview_test_get(port, &token, "assets/app.js");
        assert!(javascript.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(javascript.contains("Content-Type: text/javascript; charset=utf-8\r\n"));
        assert!(javascript.ends_with("document.body.dataset.ready='yes';"));

        let css = preview_test_get(port, &token, "assets/app.css");
        assert!(css.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(css.contains("Content-Type: text/css; charset=utf-8\r\n"));
        assert!(css.ends_with("body { color: green; }"));

        let wrong_token = preview_test_get(port, &"b".repeat(64), "assets/app.js");
        assert!(wrong_token.starts_with("HTTP/1.1 404 Not Found\r\n"));
        let root_without_cookie = preview_test_request(port, "/assets/root.js", None);
        assert!(root_without_cookie.starts_with("HTTP/1.1 404 Not Found\r\n"));
        let root_asset = preview_test_request(port, "/assets/root.js", Some(cookie));
        assert!(root_asset.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(root_asset.ends_with("document.body.dataset.root='yes';"));
        stop.store(true, std::sync::atomic::Ordering::Release);
        server.join().expect("preview server stops");
    }

    #[test]
    fn active_preview_registry_replaces_each_app_and_caps_total_snapshots() {
        let registry = Mutex::new(BTreeMap::new());
        let (first, first_stop, first_finished) = test_active_preview("first", 0);
        register_active_preview_in(&registry, "same-app", first);
        let (replacement, replacement_stop, _) = test_active_preview("replacement", 1);
        register_active_preview_in(&registry, "same-app", replacement);
        assert!(first_stop.load(Ordering::Acquire));
        assert!(first_finished.load(Ordering::Acquire));

        for sequence in 2..=MAX_ACTIVE_PREVIEWS as u64 {
            let id = format!("app-{sequence}");
            let (preview, _, _) = test_active_preview(&id, sequence);
            register_active_preview_in(&registry, &id, preview);
        }
        let (overflow, _, _) = test_active_preview("overflow", MAX_ACTIVE_PREVIEWS as u64 + 1);
        register_active_preview_in(&registry, "overflow-app", overflow);
        assert_eq!(
            registry
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            MAX_ACTIVE_PREVIEWS
        );
        assert!(replacement_stop.load(Ordering::Acquire));

        let remaining = {
            let mut active = registry
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *active).into_values().collect()
        };
        retire_previews(remaining);
    }

    #[test]
    fn preview_server_honors_the_documented_request_cap_exactly() {
        assert_eq!(PREVIEW_REQUEST_LIMIT, 512);
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("preview listener");
        let port = listener.local_addr().expect("preview address").port();
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let token = "c".repeat(64);
        let files = BTreeMap::from([("index.html".to_string(), b"ready".to_vec())]);
        let server_token = token.clone();
        let server = thread::spawn(move || {
            serve_preview(
                listener,
                &server_token,
                "",
                files,
                Arc::new(AtomicBool::new(false)),
                2,
            )
        });

        for _ in 0..2 {
            let response = preview_test_get(port, &token, "");
            assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        }
        server.join().expect("request-limited server stops");
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "the listener must close immediately after serving its exact cap"
        );
    }

    fn preview_test_get(port: u16, token: &str, path: &str) -> String {
        preview_test_request(port, &format!("/{token}/{path}"), None)
    }

    fn preview_test_request(port: u16, target: &str, cookie: Option<&str>) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("preview connection");
        let cookie = cookie
            .map(|cookie| format!("Cookie: {cookie}\r\n"))
            .unwrap_or_default();
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{cookie}Connection: close\r\n\r\n"
        )
        .expect("preview request");
        stream
            .shutdown(Shutdown::Write)
            .expect("preview request complete");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("preview response");
        response
    }

    fn test_active_preview(
        token: &str,
        sequence: u64,
    ) -> (ActivePreview, Arc<AtomicBool>, Arc<AtomicBool>) {
        let stop = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let server_finished = Arc::clone(&finished);
        let server = thread::spawn(move || {
            while !server_stop.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
            server_finished.store(true, Ordering::Release);
        });
        (
            ActivePreview {
                token: token.to_string(),
                sequence,
                stop: Arc::clone(&stop),
                server,
            },
            stop,
            finished,
        )
    }
}
