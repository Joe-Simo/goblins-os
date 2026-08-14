use std::{
    env,
    error::Error,
    fmt, io,
    process::{Command, Stdio},
    time::Duration,
};

use goblins_os_core_client::{initialize, ClientKind, CoreClient};
use serde::Deserialize;

const CORE_READ_TIMEOUT: Duration = Duration::from_secs(2);
const FIXED_DESKTOP_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
const SAFE_DESKTOP_ENVIRONMENT: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_ADDRESS",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_IDENTIFICATION",
    "LC_MEASUREMENT",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NAME",
    "LC_NUMERIC",
    "LC_PAPER",
    "LC_TELEPHONE",
    "LC_TIME",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "WAYLAND_SOCKET",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "AT_SPI_BUS_ADDRESS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_TYPE",
    "XDG_SESSION_CLASS",
    "XDG_SEAT",
    "XDG_VTNR",
    "DESKTOP_SESSION",
    "GNOME_DESKTOP_SESSION_ID",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_DATA_DIRS",
    "XDG_CONFIG_DIRS",
    "XDG_ACTIVATION_TOKEN",
    "DESKTOP_STARTUP_ID",
    "PULSE_SERVER",
    "PIPEWIRE_REMOTE",
    "GTK_THEME",
    "GDK_SCALE",
    "GDK_DPI_SCALE",
    "GTK_USE_PORTAL",
];

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
    CompatibleCodexApp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CompatibleCodexLaunch {
    program: &'static str,
    args: &'static [&'static str],
    fallback_uri: &'static str,
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
            notify_launcher_error(&error);
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
    if !service_status_allows_launch(&service.status) {
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
                    "Goblins OS service {service_id} is not allowed by the active policy or permission state"
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

fn service_status_allows_launch(status: &str) -> bool {
    matches!(
        status,
        "external" | "server-gated" | "local" | "not-configured"
    )
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
            match action {
                "openai/codex" => Ok(LaunchTarget::CompatibleCodexApp),
                _ if action.starts_with("openai/") => {
                    Err(LauncherError::UnsupportedTarget(target.to_string()))
                }
                _ => Ok(LaunchTarget::LocalAction(action.to_string())),
            }
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
        LaunchTarget::CompatibleCodexApp => launch_compatible_codex_app(),
    }
}

fn launch_compatible_codex_app() -> LauncherResult<()> {
    launch_compatible_codex_app_with(
        |program, args| {
            let mut command = reviewed_desktop_command(program, args);
            command.stdout(Stdio::null()).stderr(Stdio::null());
            command.spawn().map(|_| ())
        },
        launch_uri,
    )
}

fn reviewed_desktop_command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    command.args(args);
    command.current_dir("/");
    command.stdin(Stdio::null());
    command.env("PATH", FIXED_DESKTOP_PATH);
    for variable in SAFE_DESKTOP_ENVIRONMENT {
        if let Some(value) = env::var_os(variable) {
            command.env(variable, value);
        }
    }
    command
}

fn launch_compatible_codex_app_with<Spawn, Fallback>(
    spawn: Spawn,
    fallback: Fallback,
) -> LauncherResult<()>
where
    Spawn: FnOnce(&str, &[&str]) -> io::Result<()>,
    Fallback: FnOnce(&str) -> LauncherResult<()>,
{
    let launch = compatible_codex_launch();
    match spawn(launch.program, launch.args) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fallback(launch.fallback_uri),
        Err(_) => Err(LauncherError::SpawnFailed(launch.program.to_string())),
    }
}

fn compatible_codex_launch() -> CompatibleCodexLaunch {
    CompatibleCodexLaunch {
        program: "/usr/bin/chatgpt",
        args: &["codex:"],
        fallback_uri: "https://chatgpt.com/codex",
    }
}

fn launch_uri(uri: &str) -> LauncherResult<()> {
    if command_status("/usr/bin/gio", &["open", uri])? {
        return Ok(());
    }

    if command_status("/usr/bin/xdg-open", &[uri])? {
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

    reviewed_desktop_command(program, args)
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
    match reviewed_desktop_command(program, args).status() {
        Ok(status) => Ok(status.success()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(LauncherError::SpawnFailed(program.to_string())),
    }
}

fn notify_launcher_error(error: &LauncherError) {
    let (title, body) = match error {
        LauncherError::CoreUnavailable | LauncherError::CoreFetch(_) => (
            "Goblins OS isn't ready",
            "The local Goblins OS service is unavailable. Try again in a moment or open Recovery.",
        ),
        LauncherError::SessionLocked(_) => (
            "Unlock Goblins OS",
            "Unlock your desktop, then try opening this again.",
        ),
        LauncherError::PolicyBlocked(_) => (
            "OpenAI is unavailable",
            "Your current Goblins OS policy or permission state does not allow this cloud service.",
        ),
        LauncherError::NoDesktopHandler(_) => (
            "Couldn't open this service",
            "No supported desktop handler is available. Open Recovery for next steps.",
        ),
        LauncherError::SpawnFailed(program) if program == "/usr/bin/chatgpt" => (
            "Couldn't open Codex",
            "The compatible installed Codex app could not start. Repair the app or use Codex on the web.",
        ),
        LauncherError::SpawnFailed(_) => (
            "Couldn't open this service",
            "The selected Goblins OS service could not start. Try again or open Recovery.",
        ),
        LauncherError::UnknownService(_)
        | LauncherError::UnsupportedTarget(_)
        | LauncherError::UnsafeOpenAITarget(_) => (
            "Launch blocked",
            "Goblins OS blocked an unsupported launch request for your safety.",
        ),
        LauncherError::Usage => return,
    };
    notify_user(title, body);
}

fn notify_user(title: &str, body: &str) {
    let _ = reviewed_desktop_command(
        "/usr/bin/notify-send",
        &[
            "--app-name=Goblins OS",
            "--icon=dialog-warning-symbolic",
            "--urgency=normal",
            "--expire-time=8000",
            title,
            body,
        ],
    )
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .spawn();
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
    use std::{cell::Cell, io};

    use super::{
        classify_launch_target, compatible_codex_launch, https_host,
        launch_compatible_codex_app_with, local_action_command, openai_https_url,
        reviewed_desktop_command, service_by_id, service_status_allows_launch, LaunchTarget,
        LauncherError, ServiceCatalog, ServiceCatalogEntry, FIXED_DESKTOP_PATH,
        SAFE_DESKTOP_ENVIRONMENT,
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
        assert_eq!(
            classify_launch_target("local://goblins-os/openai/codex").unwrap(),
            LaunchTarget::CompatibleCodexApp
        );
        for unsupported in ["openai/chatgpt", "openai/agents", "openai/../codex"] {
            assert!(matches!(
                classify_launch_target(&format!("local://goblins-os/{unsupported}")),
                Err(LauncherError::UnsupportedTarget(_))
            ));
        }
    }

    #[test]
    fn compatible_codex_uses_the_fixed_binary_deep_link_and_web_fallback() {
        let codex = compatible_codex_launch();
        assert_eq!(codex.program, "/usr/bin/chatgpt");
        assert_eq!(codex.args, ["codex:"]);
        assert_eq!(codex.fallback_uri, "https://chatgpt.com/codex");
    }

    #[test]
    fn native_app_success_does_not_open_the_web_fallback() {
        let fallback_opened = Cell::new(false);
        launch_compatible_codex_app_with(
            |program, args| {
                assert_eq!(program, "/usr/bin/chatgpt");
                assert_eq!(args, ["codex:"]);
                Ok(())
            },
            |_| {
                fallback_opened.set(true);
                Ok(())
            },
        )
        .unwrap();

        assert!(!fallback_opened.get());
    }

    #[test]
    fn missing_native_app_falls_back_to_its_fixed_official_web_surface() {
        launch_compatible_codex_app_with(
            |_, _| Err(io::Error::from(io::ErrorKind::NotFound)),
            |uri| {
                assert_eq!(uri, "https://chatgpt.com/codex");
                Ok(())
            },
        )
        .unwrap();
    }

    #[test]
    fn native_app_spawn_failures_other_than_missing_fail_closed() {
        let error = launch_compatible_codex_app_with(
            |_, _| Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            |_| panic!("a broken native executable must not silently open the web fallback"),
        )
        .unwrap_err();

        assert_eq!(
            error,
            LauncherError::SpawnFailed("/usr/bin/chatgpt".to_string())
        );
    }

    #[test]
    fn compatible_app_and_web_commands_start_from_a_reviewed_empty_environment() {
        let output = reviewed_desktop_command("/usr/bin/env", &[])
            .output()
            .expect("run env with reviewed environment");
        assert!(output.status.success());

        let environment = String::from_utf8(output.stdout).expect("environment is UTF-8");
        for line in environment.lines() {
            let (name, value) = line.split_once('=').expect("name=value");
            if name == "PATH" {
                assert_eq!(value, FIXED_DESKTOP_PATH);
            } else {
                assert!(
                    SAFE_DESKTOP_ENVIRONMENT.contains(&name),
                    "unexpected inherited variable {name}"
                );
            }
        }
        for secret in [
            "OPENAI_API_KEY",
            "GITHUB_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "CODEX_HOME",
            "NODE_OPTIONS",
            "HTTPS_PROXY",
        ] {
            assert!(!environment
                .lines()
                .any(|line| line.starts_with(&format!("{secret}="))));
        }

        let working_directory = reviewed_desktop_command("/bin/pwd", &[])
            .output()
            .expect("read reviewed working directory");
        assert!(working_directory.status.success());
        assert_eq!(working_directory.stdout, b"/\n");
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
        assert!(!service_status_allows_launch("policy-blocked"));
        assert!(!service_status_allows_launch("permission-gated"));
        assert!(!service_status_allows_launch(""));
        assert!(!service_status_allows_launch("future-state"));
        for allowed in ["external", "server-gated", "local", "not-configured"] {
            assert!(service_status_allows_launch(allowed), "{allowed}");
        }

        let error = LauncherError::PolicyBlocked("chatgpt".to_string());
        assert_eq!(error.exit_code(), 77);
        assert!(error.to_string().contains("not allowed by the active"));
    }
}
