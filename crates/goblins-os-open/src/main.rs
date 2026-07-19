use std::{env, error::Error, fmt, process::Command, time::Duration};

use goblins_os_core_client::{initialize, ClientKind, CoreClient};
use serde::Deserialize;

const CORE_READ_TIMEOUT: Duration = Duration::from_secs(2);

type LauncherResult<T> = Result<T, LauncherError>;

#[derive(Clone)]
struct LauncherConfig {
    core: CoreClient,
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
enum LaunchTarget {
    ExternalOpenAI(String),
    LocalAction(String),
}

#[derive(Debug, PartialEq, Eq)]
enum LauncherError {
    Usage,
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
    let core = match initialize(ClientKind::Open) {
        Ok(core) => core,
        Err(error) => {
            eprintln!("goblins-os-open: {error}");
            std::process::exit(69);
        }
    };
    match run(core) {
        Ok(message) => println!("{message}"),
        Err(error) => {
            eprintln!("goblins-os-open: {error}");
            std::process::exit(error.exit_code());
        }
    }
}

fn run(core: CoreClient) -> LauncherResult<String> {
    let config = LauncherConfig::from_env_and_args(core)?;

    if !get_core_status(&config.core, "/health").is_some_and(|status| (200..=299).contains(&status))
    {
        return Err(LauncherError::CoreUnavailable);
    }

    let gate: SessionGateStatus = get_core_json(&config.core, "/v1/session/gate")?;
    if !gate.unlocked {
        return Err(LauncherError::SessionLocked(gate.lock.reason));
    }

    let catalog: ServiceCatalog = get_core_json(&config.core, "/v1/services")?;
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
    fn from_env_and_args(core: CoreClient) -> LauncherResult<Self> {
        let mut args = env::args().skip(1);
        let Some(service_id) = args.next() else {
            return Err(LauncherError::Usage);
        };

        if service_id == "--help" || service_id == "-h" || args.next().is_some() {
            return Err(LauncherError::Usage);
        }

        Ok(Self { core, service_id })
    }
}

impl LauncherError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Usage => 64,
            Self::UnsupportedTarget(_) | Self::UnsafeOpenAITarget(_) => 65,
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

fn get_core_json<T: for<'de> Deserialize<'de>>(
    core: &CoreClient,
    path: &'static str,
) -> LauncherResult<T> {
    let response = core
        .get(path, CORE_READ_TIMEOUT)
        .map_err(|_| LauncherError::CoreFetch(path.to_string()))?;
    if !(200..=299).contains(&response.status) {
        return Err(LauncherError::CoreFetch(path.to_string()));
    }

    serde_json::from_slice(&response.body).map_err(|_| LauncherError::CoreFetch(path.to_string()))
}

fn get_core_status(core: &CoreClient, path: &str) -> Option<u16> {
    core.get(path, CORE_READ_TIMEOUT)
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
        classify_launch_target, https_host, local_action_command, openai_https_url, service_by_id,
        LaunchTarget, LauncherError, ServiceCatalog, ServiceCatalogEntry,
    };

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
}
