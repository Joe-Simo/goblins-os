use std::{
    env,
    error::Error,
    fmt,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";
const DEFAULT_CORE_WAIT_SECS: u64 = 45;
const MAX_CORE_BODY_BYTES: usize = 1024 * 1024;

type LauncherResult<T> = Result<T, LauncherError>;

#[derive(Clone)]
struct LauncherConfig {
    core_url: String,
    core_wait: Duration,
    service_id: String,
}

#[derive(Clone, Deserialize)]
struct SessionGateStatus {
    unlocked: bool,
    lock: SessionLock,
}

#[derive(Clone, Deserialize)]
struct SessionLock {
    reason: String,
}

#[derive(Clone, Deserialize)]
struct ServiceCatalog {
    services: Vec<ServiceCatalogEntry>,
}

#[derive(Clone, Deserialize)]
struct ServiceCatalogEntry {
    id: String,
    name: String,
    launch: String,
    status: String,
}

#[derive(Debug, PartialEq, Eq)]
struct HttpEndpoint {
    host: String,
    port: u16,
}

#[derive(Debug, PartialEq, Eq)]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
enum LaunchTarget {
    ExternalOpenAI(String),
    LocalAction(String),
}

#[derive(Debug, PartialEq, Eq)]
enum LauncherError {
    Usage,
    InvalidCoreUrl(String),
    CoreUnavailable,
    CoreFetch(String),
    SessionLocked(String),
    UnknownService(String),
    UnsupportedTarget(String),
    UnsafeOpenAITarget(String),
    PolicyBlocked(String),
    NoDesktopHandler(String),
    SpawnFailed(String),
}

fn main() {
    match run() {
        Ok(message) => println!("{message}"),
        Err(error) => {
            eprintln!("goblins-os-open: {error}");
            std::process::exit(error.exit_code());
        }
    }
}

fn run() -> LauncherResult<String> {
    let config = LauncherConfig::from_env_and_args()?;

    if !wait_for_core(&config.core_url, config.core_wait) {
        return Err(LauncherError::CoreUnavailable);
    }

    let gate: SessionGateStatus = get_core_json(&config.core_url, "/v1/session/gate")?;
    if !gate.unlocked {
        return Err(LauncherError::SessionLocked(gate.lock.reason));
    }

    let catalog: ServiceCatalog = get_core_json(&config.core_url, "/v1/services")?;
    let service = service_by_id(&catalog, &config.service_id)
        .ok_or_else(|| LauncherError::UnknownService(config.service_id.clone()))?;
    if service.status == "policy-blocked" {
        return Err(LauncherError::PolicyBlocked(config.service_id.clone()));
    }
    let target = classify_launch_target(&service.launch)?;

    launch_target(&target)?;

    Ok(format!(
        "Opened {} ({}) through the Goblins OS native launcher.",
        service.name, service.status
    ))
}

impl LauncherConfig {
    fn from_env_and_args() -> LauncherResult<Self> {
        let mut args = env::args().skip(1);
        let Some(service_id) = args.next() else {
            return Err(LauncherError::Usage);
        };

        if service_id == "--help" || service_id == "-h" || args.next().is_some() {
            return Err(LauncherError::Usage);
        }

        let core_url = validate_core_url(
            &env::var("GOBLINS_OS_CORE_URL")
                .or_else(|_| env::var("OPENAI_OS_CORE_URL"))
                .unwrap_or_else(|_| DEFAULT_CORE_URL.to_string()),
        )?;

        Ok(Self {
            core_url,
            core_wait: Duration::from_secs(env_u64(
                "GOBLINS_OS_OPEN_CORE_WAIT_SECS",
                DEFAULT_CORE_WAIT_SECS,
            )),
            service_id,
        })
    }
}

impl LauncherError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Usage => 64,
            Self::InvalidCoreUrl(_) | Self::UnsupportedTarget(_) | Self::UnsafeOpenAITarget(_) => {
                65
            }
            Self::CoreUnavailable | Self::CoreFetch(_) => 69,
            Self::SessionLocked(_) => 77,
            Self::UnknownService(_) => 66,
            Self::PolicyBlocked(_) => 77,
            Self::NoDesktopHandler(_) | Self::SpawnFailed(_) => 70,
        }
    }
}

impl fmt::Display for LauncherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => formatter.write_str(
                "usage: goblins-os-open <service-id>; services are resolved through the local OS core",
            ),
            Self::InvalidCoreUrl(value) => write!(
                formatter,
                "GOBLINS_OS_CORE_URL must be a local http endpoint, got {value}"
            ),
            Self::CoreUnavailable => {
                formatter.write_str("the local Goblins OS core did not become ready")
            }
            Self::CoreFetch(path) => write!(
                formatter,
                "could not read or decode local OS core path {path}"
            ),
            Self::SessionLocked(reason) => {
                write!(formatter, "the Goblins OS desktop session is locked: {reason}")
            }
            Self::UnknownService(service_id) => {
                write!(formatter, "unknown Goblins OS service id {service_id}")
            }
            Self::UnsupportedTarget(target) => {
                write!(formatter, "unsupported OS service launch target {target}")
            }
            Self::UnsafeOpenAITarget(target) => {
                write!(formatter, "refusing non-OpenAI https launch target {target}")
            }
            Self::PolicyBlocked(service_id) => {
                write!(
                    formatter,
                    "Goblins OS service {service_id} is blocked by the active Goblins OS policy"
                )
            }
            Self::NoDesktopHandler(target) => {
                write!(formatter, "no desktop handler could open {target}")
            }
            Self::SpawnFailed(program) => {
                write!(formatter, "could not start native OS program {program}")
            }
        }
    }
}

impl Error for LauncherError {}

fn wait_for_core(core_url: &str, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;

    loop {
        if get_core_status(core_url, "/health").is_some_and(|status| (200..=299).contains(&status))
        {
            return true;
        }

        if Instant::now() >= deadline {
            return false;
        }

        thread::sleep(Duration::from_millis(250));
    }
}

fn get_core_json<T: for<'de> Deserialize<'de>>(
    core_url: &str,
    path: &'static str,
) -> LauncherResult<T> {
    let response =
        http_get(core_url, path).map_err(|_| LauncherError::CoreFetch(path.to_string()))?;
    if !(200..=299).contains(&response.status) {
        return Err(LauncherError::CoreFetch(path.to_string()));
    }

    serde_json::from_slice(&response.body).map_err(|_| LauncherError::CoreFetch(path.to_string()))
}

fn get_core_status(core_url: &str, path: &str) -> Option<u16> {
    http_get(core_url, path)
        .ok()
        .map(|response| response.status)
}

fn service_by_id<'a>(
    catalog: &'a ServiceCatalog,
    service_id: &str,
) -> Option<&'a ServiceCatalogEntry> {
    catalog
        .services
        .iter()
        .find(|service| service.id == service_id)
}

fn validate_core_url(value: &str) -> LauncherResult<String> {
    let trimmed = value.trim_end_matches('/');
    let endpoint = parse_http_endpoint(trimmed)
        .ok_or_else(|| LauncherError::InvalidCoreUrl(value.to_string()))?;

    if !matches!(endpoint.host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        return Err(LauncherError::InvalidCoreUrl(value.to_string()));
    }

    Ok(trimmed.to_string())
}

fn classify_launch_target(target: &str) -> LauncherResult<LaunchTarget> {
    if target.starts_with("https://") {
        if openai_https_url(target) {
            Ok(LaunchTarget::ExternalOpenAI(target.to_string()))
        } else {
            Err(LauncherError::UnsafeOpenAITarget(target.to_string()))
        }
    } else if let Some(action) = target.strip_prefix("local://goblins-os/") {
        if action.is_empty() || action.contains("..") {
            Err(LauncherError::UnsupportedTarget(target.to_string()))
        } else {
            Ok(LaunchTarget::LocalAction(action.to_string()))
        }
    } else {
        Err(LauncherError::UnsupportedTarget(target.to_string()))
    }
}

fn openai_https_url(target: &str) -> bool {
    let Some(host) = https_host(target) else {
        return false;
    };

    host == "chatgpt.com"
        || host.ends_with(".chatgpt.com")
        || host == "openai.com"
        || host.ends_with(".openai.com")
}

fn launch_target(target: &LaunchTarget) -> LauncherResult<()> {
    match target {
        LaunchTarget::ExternalOpenAI(uri) => launch_uri(uri),
        LaunchTarget::LocalAction(action) => launch_local_action(action),
    }
}

fn launch_uri(uri: &str) -> LauncherResult<()> {
    if command_status("gio", &["open", uri])? {
        return Ok(());
    }

    if command_status("xdg-open", &[uri])? {
        return Ok(());
    }

    Err(LauncherError::NoDesktopHandler(uri.to_string()))
}

fn launch_local_action(action: &str) -> LauncherResult<()> {
    let Some((program, args)) = local_action_command(action) else {
        return Err(LauncherError::UnsupportedTarget(format!(
            "local://goblins-os/{action}"
        )));
    };

    Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|_| LauncherError::SpawnFailed(program.to_string()))
}

fn local_action_command(action: &str) -> Option<(&'static str, &'static [&'static str])> {
    match action {
        "settings" => Some(("/usr/libexec/goblins-os/goblins-os-settings", &[])),
        "recovery" => Some((
            "/usr/libexec/goblins-os/goblins-os-settings",
            &["--panel=recovery"],
        )),
        "policy" => Some((
            "/usr/libexec/goblins-os/goblins-os-settings",
            &["--panel=policy"],
        )),
        "apps/build" => Some(("/usr/libexec/goblins-os/goblins-os-shell", &["--studio"])),
        _ => None,
    }
}

fn command_status(program: &str, args: &[&str]) -> LauncherResult<bool> {
    match Command::new(program).args(args).status() {
        Ok(status) => Ok(status.success()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(LauncherError::SpawnFailed(program.to_string())),
    }
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn http_get(core_url: &str, path: &str) -> Result<HttpResponse, ()> {
    let endpoint = parse_http_endpoint(core_url).ok_or(())?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| ())?
        .next()
        .ok_or(())?;
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(2)).map_err(|_| ())?;

    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ())?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nUser-Agent: goblins-os-open\r\nConnection: close\r\n\r\n",
        endpoint.host
    );
    stream.write_all(request.as_bytes()).map_err(|_| ())?;

    let mut bytes = Vec::new();
    stream
        .take((MAX_CORE_BODY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;

    if bytes.len() > MAX_CORE_BODY_BYTES {
        return Err(());
    }

    parse_http_response(&bytes).ok_or(())
}

fn parse_http_response(bytes: &[u8]) -> Option<HttpResponse> {
    let split = bytes.windows(4).position(|window| window == b"\r\n\r\n")?;
    let headers = std::str::from_utf8(&bytes[..split]).ok()?;
    let status_line = headers.lines().next()?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse::<u16>().ok())?;

    Some(HttpResponse {
        status,
        body: bytes[split + 4..].to_vec(),
    })
}

fn parse_http_endpoint(url: &str) -> Option<HttpEndpoint> {
    let authority_and_path = url.strip_prefix("http://")?;
    let authority = authority_and_path.split('/').next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }

    if let Some(rest) = authority.strip_prefix('[') {
        let (host, suffix) = rest.split_once(']')?;
        let port = if let Some(port) = suffix.strip_prefix(':') {
            port.parse().ok()?
        } else if suffix.is_empty() {
            80
        } else {
            return None;
        };

        return Some(HttpEndpoint {
            host: host.to_string(),
            port,
        });
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, port.parse().ok()?),
        Some(_) => return None,
        None => (authority, 80),
    };

    Some(HttpEndpoint {
        host: host.to_string(),
        port,
    })
}

fn https_host(target: &str) -> Option<String> {
    let rest = target.strip_prefix("https://")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return None;
    }

    let host = if let Some(rest) = authority.strip_prefix('[') {
        let (host, _) = rest.split_once(']')?;
        host
    } else {
        authority.split(':').next()?
    };

    Some(host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::{
        classify_launch_target, https_host, local_action_command, openai_https_url,
        parse_http_endpoint, parse_http_response, service_by_id, validate_core_url, HttpEndpoint,
        LaunchTarget, LauncherError, ServiceCatalog, ServiceCatalogEntry,
    };

    #[test]
    fn core_url_must_be_local_http() {
        assert_eq!(
            validate_core_url("http://127.0.0.1:8787").unwrap(),
            "http://127.0.0.1:8787"
        );
        assert!(matches!(
            validate_core_url("https://127.0.0.1:8787"),
            Err(LauncherError::InvalidCoreUrl(_))
        ));
        assert!(matches!(
            validate_core_url("http://example.com:8787"),
            Err(LauncherError::InvalidCoreUrl(_))
        ));
        assert_eq!(
            parse_http_endpoint("http://[::1]:8787").unwrap(),
            HttpEndpoint {
                host: "::1".to_string(),
                port: 8787
            }
        );
    }

    #[test]
    fn only_openai_https_targets_are_external_launches() {
        assert_eq!(
            classify_launch_target("https://chatgpt.com/codex").unwrap(),
            LaunchTarget::ExternalOpenAI("https://chatgpt.com/codex".to_string())
        );
        assert!(openai_https_url("https://platform.openai.com/docs"));
        assert!(!openai_https_url("https://notopenai.com"));
        assert_eq!(
            https_host("https://platform.openai.com/docs").unwrap(),
            "platform.openai.com"
        );
        assert!(matches!(
            classify_launch_target("https://example.com"),
            Err(LauncherError::UnsafeOpenAITarget(_))
        ));
    }

    #[test]
    fn local_targets_are_limited_to_known_native_actions() {
        assert_eq!(
            classify_launch_target("local://goblins-os/settings").unwrap(),
            LaunchTarget::LocalAction("settings".to_string())
        );
        assert_eq!(
            local_action_command("settings").unwrap().0,
            "/usr/libexec/goblins-os/goblins-os-settings"
        );
        assert_eq!(
            local_action_command("recovery").unwrap().1,
            ["--panel=recovery"]
        );
        assert_eq!(
            local_action_command("apps/build").unwrap(),
            (
                "/usr/libexec/goblins-os/goblins-os-shell",
                &["--studio"][..]
            )
        );
        assert!(matches!(
            classify_launch_target("local://goblins-os/../secrets"),
            Err(LauncherError::UnsupportedTarget(_))
        ));
    }

    #[test]
    fn service_lookup_uses_catalog_id() {
        let catalog = ServiceCatalog {
            services: vec![ServiceCatalogEntry {
                id: "chatgpt".to_string(),
                name: "ChatGPT".to_string(),
                launch: "https://chatgpt.com".to_string(),
                status: "external".to_string(),
            }],
        };

        assert_eq!(service_by_id(&catalog, "chatgpt").unwrap().name, "ChatGPT");
        assert!(service_by_id(&catalog, "missing").is_none());
    }

    #[test]
    fn policy_blocked_services_are_refused_by_launcher_contract() {
        let error = LauncherError::PolicyBlocked("chatgpt".to_string());

        assert_eq!(error.exit_code(), 77);
        assert!(error.to_string().contains("blocked by the active"));
    }

    #[test]
    fn parses_core_http_response_body() {
        let response =
            parse_http_response(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}")
                .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{}");
    }
}
