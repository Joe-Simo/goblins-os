//! The user's OpenAI account, via Codex CLI.
//!
//! Codex is OpenAI's own open-source coding agent (Rust, runs locally). Signing
//! in with a ChatGPT account uses that plan's included Codex usage, so this is the
//! honest way to put a user's OpenAI *account* — not just an API key — behind
//! Goblins OS. The OS detects the Codex CLI and its sign-in, drives it
//! non-interactively (`codex exec`) as a resident engine, and never stores the
//! account credentials itself — Codex owns them under an OS-set `CODEX_HOME`.

use std::{
    collections::BTreeSet,
    env,
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use axum::{http::StatusCode, Json};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions as CapOpenOptions, Permissions as CapPermissions},
};
use serde::Serialize;

use crate::bounded::{bounded_output_of, probe_timeout};

const DEFAULT_CODEX_HOME: &str = "/var/lib/goblins-os/codex";
const CODEX_CHILD_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const CODEX_CHILD_ENV_ALLOWLIST: &[&str] = &[
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// A `codex exec` turn is a genuine agent run (reads, writes, and builds in its
/// workspace), so it gets a long explicit bound instead of the status-probe
/// timeout. The bound still guarantees a wedged Codex child cannot hold a
/// runtime worker hostage forever.
const CODEX_EXEC_TIMEOUT: Duration = Duration::from_secs(600);
const CODEX_LOGIN_TERMINATION_TIMEOUT: Duration = Duration::from_secs(2);
const PRIVATE_STORAGE_LABEL: &str = "OS-owned private storage";
const MAX_STUDIO_RESULT_BYTES: usize = 256 * 1024;
const RESIDENT_PERMISSION_PROFILE: &str = "goblins-resident";
const STUDIO_PERMISSION_PROFILE: &str = "goblins-studio";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexSandboxRole {
    Resident,
    Studio,
}

impl CodexSandboxRole {
    const fn profile(self) -> &'static str {
        match self {
            Self::Resident => RESIDENT_PERMISSION_PROFILE,
            Self::Studio => STUDIO_PERMISSION_PROFILE,
        }
    }

    const fn base_profile(self) -> &'static str {
        match self {
            Self::Resident => ":read-only",
            Self::Studio => ":workspace",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexAuthentication {
    Authenticated,
    SignedOut,
    Unavailable,
}

#[derive(Serialize)]
pub struct CodexStatus {
    source: &'static str,
    installed: bool,
    authenticated: bool,
    /// Compatibility field for existing clients. It intentionally names the
    /// storage boundary without serializing the credential directory path.
    codex_home: &'static str,
    detail: String,
}

pub async fn codex_status() -> Json<CodexStatus> {
    Json(build_status())
}

/// Result of asking the core to begin Codex sign-in. The GUI receives only
/// booleans + a status line — never the token.
#[derive(Serialize)]
pub struct CodexLoginStart {
    started: bool,
    authenticated: bool,
    already_running: bool,
    detail: String,
}

#[derive(Serialize)]
pub struct CodexLogout {
    ok: bool,
    authenticated: Option<bool>,
    detail: String,
}

/// The browser sign-in URL the core captured from `codex login`, handed to the
/// session to open. Carries no credential (the token is written to auth.json,
/// which lives only under the service-owned CODEX_HOME).
#[derive(Serialize)]
pub struct CodexLoginUrl {
    authenticated: bool,
    auth_url: Option<String>,
    detail: String,
}

/// Begin `codex login` as the core service user (`goblins-os`), so the OpenAI
/// account token is written under the 0700 service-owned CODEX_HOME and is never
/// reachable by the desktop session. The browser handoff URL Codex prints is
/// captured to a 0600 log the session reads back via [`codex_login_url`].
pub async fn codex_login_start() -> (StatusCode, Json<CodexLoginStart>) {
    let (status, outcome) = start_login();
    (status, Json(outcome))
}

/// Disconnect the OS-owned Codex account session. This is intentionally
/// idempotent: `codex logout` succeeds when no account is connected too. If
/// Codex is the selected engine, the persisted route is moved to GPT-OSS before
/// credentials are touched so concurrent AI requests fail locally and safely.
pub async fn codex_logout() -> (StatusCode, Json<CodexLogout>) {
    let (status, outcome) = perform_logout();
    (status, Json(outcome))
}

/// Report the captured browser sign-in URL (if Codex has emitted it yet) or that
/// the account is already connected. The session opens the returned URL.
pub async fn codex_login_url() -> Json<CodexLoginUrl> {
    Json(read_login_url())
}

pub(crate) fn codex_installed() -> bool {
    binary_present(&codex_bin())
}

pub(crate) fn codex_authenticated() -> bool {
    codex_authentication() == CodexAuthentication::Authenticated
}

/// Usable as the resident engine only when the CLI is present and signed in.
pub(crate) fn codex_available() -> bool {
    codex_installed() && codex_authenticated()
}

fn build_status() -> CodexStatus {
    let installed = codex_installed();
    let authentication = if installed {
        codex_authentication()
    } else {
        CodexAuthentication::Unavailable
    };
    let authenticated = authentication == CodexAuthentication::Authenticated;
    CodexStatus {
        source: "goblins-os-core",
        installed,
        authenticated,
        codex_home: PRIVATE_STORAGE_LABEL,
        detail: status_detail(installed, authentication),
    }
}

fn status_detail(installed: bool, authentication: CodexAuthentication) -> String {
    if !installed {
        "Codex account support is not included in this build. Start from the full Goblins OS image to use your OpenAI account through Codex.".to_string()
    } else {
        match authentication {
            CodexAuthentication::Authenticated => {
                "Signed in to Codex with your OpenAI account.".to_string()
            }
            CodexAuthentication::SignedOut => {
                "Codex account support is ready. Sign in with your OpenAI account to use it."
                    .to_string()
            }
            CodexAuthentication::Unavailable => {
                "Goblins OS could not check Codex account status. Try again before selecting Codex."
                    .to_string()
            }
        }
    }
}

/// Ask the supported Codex CLI status command instead of inferring readiness
/// from a credential-shaped file. A stale or malformed auth file must never
/// make the resident route look ready.
fn codex_authentication() -> CodexAuthentication {
    codex_authentication_with(&codex_bin(), &codex_home())
}

fn codex_authentication_with(binary: &str, home: &Path) -> CodexAuthentication {
    let mut command = isolated_codex_command(binary, home);
    command.arg("login").arg("status");
    match bounded_output_of(&mut command, probe_timeout()) {
        Ok(output) if output.status.success() => CodexAuthentication::Authenticated,
        Ok(_) => CodexAuthentication::SignedOut,
        Err(_) => CodexAuthentication::Unavailable,
    }
}

/// Run one prompt through Codex non-interactively and return its final message.
/// The agent runs in an OS-owned scratch workspace under the account's own
/// sign-in; Goblins OS never sees the credentials, only the answer.
pub(crate) fn run_codex(prompt: &str) -> Result<String, &'static str> {
    if !codex_installed() {
        return Err("Codex account support is not included in this build");
    }
    if !codex_authenticated() {
        return Err("Codex is not signed in to an OpenAI account");
    }
    if !crate::resident::hosted_execution_allowed() {
        return Err("Codex is blocked by Private mode or the active OS policy");
    }

    let workspace = work_dir();
    fs::create_dir_all(&workspace).map_err(|_| "Codex workspace could not be created")?;
    let last_message = workspace.join("last-message.txt");
    let _ = fs::remove_file(&last_message);

    let mut command = isolated_codex_command(&codex_bin(), &codex_home());
    command
        .arg("exec")
        .arg("--ephemeral")
        .arg("--ignore-user-config")
        .arg("--ignore-rules");
    configure_resident_sandbox(&mut command);
    command
        .arg("--skip-git-repo-check")
        .arg("--output-last-message")
        .arg(&last_message)
        .arg(prompt)
        .current_dir(&workspace);
    let status = bounded_output_of(&mut command, CODEX_EXEC_TIMEOUT)
        .map(|output| output.status)
        .map_err(|_| "Codex could not start")?;
    if !status.success() {
        return Err("Codex did not complete the request");
    }

    let text = fs::read_to_string(&last_message).map_err(|_| "Codex returned no message")?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Err("Codex returned an empty message")
    } else {
        Ok(trimmed.chars().take(4000).collect())
    }
}

struct StudioResultFile {
    directory: Dir,
    name: OsString,
    path: PathBuf,
}

impl StudioResultFile {
    fn create() -> io::Result<Self> {
        let apps_path = crate::app_builder::apps_dir();
        Self::create_at(&apps_path)
    }

    fn create_at(apps_path: &Path) -> io::Result<Self> {
        fs::create_dir_all(apps_path)?;
        let apps = Dir::open_ambient_dir(apps_path, ambient_authority())?;
        let directory_name = OsStr::new("codex-results");
        let directory = match apps.open_dir_nofollow(directory_name) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match apps.create_dir(directory_name) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                apps.open_dir_nofollow(directory_name)?
            }
            Err(error) => return Err(error),
        };
        directory.set_permissions(
            ".",
            CapPermissions::from_std(fs::Permissions::from_mode(0o700)),
        )?;

        for _ in 0..8 {
            let name = OsString::from(format!(
                ".studio-result-{:016x}{:016x}.txt",
                rand::random::<u64>(),
                rand::random::<u64>()
            ));
            let mut options = CapOpenOptions::new();
            options
                .read(true)
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No);
            match directory.open_with(&name, &options) {
                Ok(file) => {
                    file.set_permissions(CapPermissions::from_std(fs::Permissions::from_mode(
                        0o600,
                    )))?;
                    return Ok(Self {
                        path: apps_path.join("codex-results").join(&name),
                        directory,
                        name,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a private Codex result file",
        ))
    }

    fn read(&self) -> io::Result<String> {
        let metadata = self.directory.symlink_metadata(&self.name)?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Codex result is not a private regular file",
            ));
        }
        let mut options = CapOpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = self.directory.open_with(&self.name, &options)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Codex result is not a private regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take((MAX_STUDIO_RESULT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_STUDIO_RESULT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Codex result exceeds the Studio response limit",
            ));
        }
        String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Codex returned invalid text"))
    }
}

impl Drop for StudioResultFile {
    fn drop(&mut self) {
        let _ = self.directory.remove_file(&self.name);
    }
}

/// Run one Codex turn non-interactively inside `workspace`, returning the agent's
/// final message. Codex reads, writes, and (in its sandbox) runs files in the
/// workspace, so this is a true agent step — the Studio captures the result and
/// then lists whatever files the agent produced. Errors carry a calm message.
pub(crate) fn run_codex_in(workspace: &Dir, prompt: &str) -> Result<String, String> {
    if !codex_installed() {
        return Err("Codex account support is not included in this build.".to_string());
    }
    if !codex_authenticated() {
        return Err("Codex is not signed in. Sign in with your OpenAI account first.".to_string());
    }
    if !crate::resident::hosted_execution_allowed() {
        return Err("Codex is blocked by Private mode or the active OS policy.".to_string());
    }

    let result = StudioResultFile::create()
        .map_err(|_| "Could not create a private Codex result file.".to_string())?;
    let workspace_directory = workspace
        .try_clone()
        .map_err(|_| "Could not open the Codex workspace.".to_string())?
        .into_std_file();
    let workspace_fd = workspace_directory.as_raw_fd();

    let mut command = isolated_codex_command(&codex_bin(), &codex_home());
    command
        .arg("exec")
        .arg("--ephemeral")
        .arg("--ignore-user-config")
        .arg("--ignore-rules");
    configure_studio_sandbox(&mut command);
    command
        .arg("--skip-git-repo-check")
        .arg("--output-last-message")
        .arg(&result.path)
        .arg(prompt);
    // SAFETY: `fchdir` is async-signal-safe and receives a valid directory fd
    // kept alive until after the child is spawned. This binds the child to the
    // already-open capability instead of resolving an attacker-swappable path.
    unsafe {
        command.pre_exec(move || {
            if libc::fchdir(workspace_fd) == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    }
    let status = bounded_output_of(&mut command, CODEX_EXEC_TIMEOUT)
        .map(|output| output.status)
        .map_err(|_| "Codex could not start.".to_string())?;
    drop(workspace_directory);
    if !status.success() {
        return Err("Codex did not complete the request.".to_string());
    }

    let text = result
        .read()
        .map_err(|_| "Codex returned no final Studio message.".to_string())?;
    validated_studio_message(&text)
}

fn configure_studio_sandbox(command: &mut Command) {
    configure_codex_sandbox(command, CodexSandboxRole::Studio);
}

fn configure_resident_sandbox(command: &mut Command) {
    configure_codex_sandbox(command, CodexSandboxRole::Resident);
}

fn configure_codex_sandbox(command: &mut Command, role: CodexSandboxRole) {
    let apps = crate::app_builder::apps_dir();
    let codex = codex_home();
    let credentials = env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from);
    let auth_session = env::var_os("OPENAI_ACCOUNT_SESSION_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/goblins-os/secrets/openai/session.json"));
    let filesystem = codex_filesystem_policy_override(
        role,
        &apps,
        &codex,
        credentials.as_deref(),
        &auth_session,
    );
    append_codex_permission_profile(command, role, filesystem, true);
    command.arg("--strict-config");
}

fn append_codex_permission_profile(
    command: &mut Command,
    role: CodexSandboxRole,
    filesystem: String,
    select_as_default: bool,
) {
    let profile = role.profile();
    let shell_environment = format!(
        "shell_environment_policy.set={{PATH={},LANG=\"C.UTF-8\"}}",
        serde_json::to_string(CODEX_CHILD_PATH).expect("static PATH is valid text")
    );

    if select_as_default {
        command
            .arg("-c")
            .arg(format!("default_permissions=\"{profile}\""));
    }
    command
        .arg("-c")
        .arg(format!(
            "permissions.{profile}.extends=\"{}\"",
            role.base_profile()
        ))
        .arg("-c")
        .arg(filesystem)
        .arg("-c")
        .arg("approval_policy=\"never\"")
        .arg("-c")
        .arg("shell_environment_policy.inherit=\"none\"")
        .arg("-c")
        .arg(shell_environment)
        .arg("-c")
        .arg("allow_login_shell=false")
        .arg("-c")
        .arg("project_doc_max_bytes=0");
}

fn codex_filesystem_policy_override(
    role: CodexSandboxRole,
    apps: &Path,
    codex: &Path,
    credentials: Option<&Path>,
    auth_session: &Path,
) -> String {
    let mut denied = BTreeSet::from([
        PathBuf::from("/etc/goblins-os"),
        PathBuf::from("/run/credentials"),
        codex.to_path_buf(),
        auth_session.to_path_buf(),
        apps.join("codex-results"),
    ]);
    if let Some(credentials) = credentials {
        denied.insert(credentials.to_path_buf());
    }
    if let Some(state_root) = apps.parent().filter(|parent| parent.parent().is_some()) {
        for private_leaf in [
            ".config",
            ".local",
            "ai",
            "installer",
            "models",
            "policy",
            "resident",
            "secrets",
            "session",
            "sound-recognition",
            "voice",
        ] {
            denied.insert(state_root.join(private_leaf));
        }
    }
    match role {
        CodexSandboxRole::Resident => {
            denied.insert(apps.join("workspace"));
        }
        CodexSandboxRole::Studio => {
            denied.insert(apps.join("codex-work"));
        }
    }

    let entries = denied
        .into_iter()
        .filter(|path| path.is_absolute() && path.parent().is_some())
        .map(|path| {
            let path = path.to_string_lossy();
            format!(
                "{}=\"deny\"",
                serde_json::to_string(path.as_ref()).expect("filesystem path is valid text")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("permissions.{}.filesystem={{{entries}}}", role.profile())
}

fn validated_studio_message(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Err("Codex returned an empty final Studio message.".to_string())
    } else {
        Ok(trimmed.chars().take(4000).collect())
    }
}

/// Construct a Codex child with a closed environment. The daemon may hold relay
/// and OIDC secrets, so inheritance is opt-in for a tiny non-secret runtime
/// allowlist; HOME is deliberately redirected to the Codex-owned directory.
fn isolated_codex_command(binary: &str, home: &Path) -> Command {
    let mut command = Command::new(binary);
    command.env_clear();
    command.env("PATH", CODEX_CHILD_PATH);
    command.env("CODEX_HOME", home);
    command.env("HOME", home);
    for name in CODEX_CHILD_ENV_ALLOWLIST {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

fn codex_bin() -> String {
    env::var("GOBLINS_OS_CODEX_BIN").unwrap_or_else(|_| "codex".to_string())
}

fn codex_home() -> PathBuf {
    env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_CODEX_HOME).to_path_buf())
}

fn work_dir() -> PathBuf {
    // The agent's scratch workspace lives OUTSIDE CODEX_HOME so a `codex exec` step
    // never reads or writes next to the account credential (auth.json).
    crate::app_builder::apps_dir().join("codex-work")
}

fn login_log_path() -> PathBuf {
    codex_home().join("login.log")
}

/// Tracks the in-flight `codex login` child so a repeated request does not spawn a
/// duplicate sign-in, and so the process is reaped once it finishes.
fn login_child() -> &'static Mutex<Option<Child>> {
    static CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    CHILD.get_or_init(|| Mutex::new(None))
}

fn login_start(
    status: StatusCode,
    detail: &str,
    started: bool,
    authenticated: bool,
    already_running: bool,
) -> (StatusCode, CodexLoginStart) {
    (
        status,
        CodexLoginStart {
            started,
            authenticated,
            already_running,
            detail: detail.to_string(),
        },
    )
}

fn start_login() -> (StatusCode, CodexLoginStart) {
    if !codex_installed() {
        return login_start(
            StatusCode::SERVICE_UNAVAILABLE,
            "Codex account support is not included in this build.",
            false,
            false,
            false,
        );
    }
    match codex_authentication() {
        CodexAuthentication::Authenticated => {
            return login_start(
                StatusCode::OK,
                "Already signed in to Codex with your OpenAI account.",
                false,
                true,
                false,
            );
        }
        CodexAuthentication::Unavailable => {
            return login_start(
                StatusCode::SERVICE_UNAVAILABLE,
                "Goblins OS could not check Codex account status.",
                false,
                false,
                false,
            );
        }
        CodexAuthentication::SignedOut => {}
    }
    if !crate::resident::hosted_execution_allowed() {
        return login_start(
            StatusCode::FORBIDDEN,
            "Codex sign-in is blocked by Private mode or the active Goblins OS policy.",
            false,
            false,
            false,
        );
    }

    let mut guard = login_child()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Reap a finished prior attempt; a still-running one is reported in progress.
    if let Some(child) = guard.as_mut() {
        match child.try_wait() {
            Ok(None) => {
                return login_start(
                    StatusCode::OK,
                    "Codex sign-in is already in progress. Finish it in your browser.",
                    false,
                    false,
                    true,
                );
            }
            _ => *guard = None,
        }
    }

    let home = codex_home();
    if fs::create_dir_all(&home).is_err() {
        return login_start(
            StatusCode::SERVICE_UNAVAILABLE,
            "Codex sign-in could not prepare its home directory.",
            false,
            false,
            false,
        );
    }

    // Capture Codex's stdout+stderr (which carries the browser URL) to an
    // owner-only log; the token itself is written separately to auth.json.
    let log = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(login_log_path())
    {
        Ok(file) => file,
        Err(_) => {
            return login_start(
                StatusCode::SERVICE_UNAVAILABLE,
                "Codex sign-in could not open its log.",
                false,
                false,
                false,
            );
        }
    };
    let log_err = match log.try_clone() {
        Ok(file) => file,
        Err(_) => {
            return login_start(
                StatusCode::SERVICE_UNAVAILABLE,
                "Codex sign-in could not open its log.",
                false,
                false,
                false,
            );
        }
    };

    let mut command = isolated_codex_command(&codex_bin(), &home);
    match command
        .arg("login")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
    {
        Ok(child) => {
            *guard = Some(child);
            login_start(
                StatusCode::ACCEPTED,
                "Codex sign-in started. Opening your browser to finish.",
                true,
                false,
                false,
            )
        }
        Err(_) => login_start(
            StatusCode::SERVICE_UNAVAILABLE,
            "Codex sign-in could not start.",
            false,
            false,
            false,
        ),
    }
}

fn logout_outcome(
    status: StatusCode,
    ok: bool,
    authenticated: Option<bool>,
    detail: &str,
) -> (StatusCode, CodexLogout) {
    (
        status,
        CodexLogout {
            ok,
            authenticated,
            detail: detail.to_string(),
        },
    )
}

fn known_authentication() -> Option<bool> {
    match codex_authentication() {
        CodexAuthentication::Authenticated => Some(true),
        CodexAuthentication::SignedOut => Some(false),
        CodexAuthentication::Unavailable => None,
    }
}

fn perform_logout() -> (StatusCode, CodexLogout) {
    if crate::openai_key::fail_safe_from_codex_to_local().is_err() {
        return logout_outcome(
            StatusCode::INTERNAL_SERVER_ERROR,
            false,
            known_authentication(),
            "Goblins OS could not switch safely to the on-device engine, so Codex stayed connected.",
        );
    }
    if terminate_login_child().is_err() {
        return logout_outcome(
            StatusCode::INTERNAL_SERVER_ERROR,
            false,
            known_authentication(),
            "Goblins OS could not stop the active Codex sign-in safely.",
        );
    }
    if clear_login_log().is_err() {
        return logout_outcome(
            StatusCode::INTERNAL_SERVER_ERROR,
            false,
            known_authentication(),
            "Goblins OS could not clear the Codex sign-in handoff safely.",
        );
    }
    if !codex_installed() {
        return logout_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            None,
            "Codex account support is not included in this build.",
        );
    }
    if !run_codex_logout(&codex_bin(), &codex_home()) {
        return logout_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            known_authentication(),
            "Codex could not disconnect the OpenAI account.",
        );
    }
    logout_outcome(
        StatusCode::OK,
        true,
        Some(false),
        "Codex is signed out on this device.",
    )
}

fn terminate_login_child() -> std::io::Result<()> {
    let mut guard = login_child()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    terminate_child(&mut guard)
}

fn terminate_child(child_slot: &mut Option<Child>) -> std::io::Result<()> {
    let Some(child) = child_slot.as_mut() else {
        return Ok(());
    };
    if child.try_wait()?.is_none() {
        if let Err(error) = child.kill() {
            if child.try_wait()?.is_none() {
                return Err(error);
            }
        } else {
            let started = Instant::now();
            while child.try_wait()?.is_none() {
                if started.elapsed() >= CODEX_LOGIN_TERMINATION_TIMEOUT {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Codex login child did not stop",
                    ));
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
    *child_slot = None;
    Ok(())
}

fn clear_login_log() -> std::io::Result<()> {
    clear_login_log_at(&login_log_path())
}

fn clear_login_log_at(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn run_codex_logout(binary: &str, home: &Path) -> bool {
    let mut command = isolated_codex_command(binary, home);
    command.arg("logout");
    bounded_output_of(&mut command, probe_timeout()).is_ok_and(|output| output.status.success())
}

fn read_login_url() -> CodexLoginUrl {
    if codex_authenticated() {
        return CodexLoginUrl {
            authenticated: true,
            auth_url: None,
            detail: "Signed in to Codex with your OpenAI account.".to_string(),
        };
    }
    let log = fs::read_to_string(login_log_path()).unwrap_or_default();
    match first_https_url(&log) {
        Some(url) => CodexLoginUrl {
            authenticated: false,
            auth_url: Some(url),
            detail: "Open the sign-in link to finish connecting your OpenAI account.".to_string(),
        },
        None => CodexLoginUrl {
            authenticated: false,
            auth_url: None,
            detail: "Codex sign-in is starting. The browser link will appear shortly.".to_string(),
        },
    }
}

/// Extract the first `https://` sign-in link Codex prints, trimming surrounding
/// quotes/punctuation. Never returns a bare scheme and never exposes a credential
/// (the token is in auth.json, not the captured log).
fn first_https_url(text: &str) -> Option<String> {
    text.split(char::is_whitespace).find_map(|token| {
        let start = token.find("https://")?;
        let url = token[start..].trim_end_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | '.' | ';'
            )
        });
        (url.len() > "https://".len()).then(|| url.to_string())
    })
}

fn binary_present(binary: &str) -> bool {
    if binary.contains('/') {
        return Path::new(binary).exists();
    }
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{
        append_codex_permission_profile, binary_present, clear_login_log_at,
        codex_authentication_with, codex_filesystem_policy_override, configure_resident_sandbox,
        configure_studio_sandbox, first_https_url, isolated_codex_command, login_start,
        run_codex_logout, status_detail, terminate_child, validated_studio_message,
        CodexAuthentication, CodexSandboxRole, CodexStatus, StudioResultFile,
    };
    use axum::http::StatusCode;
    use std::path::Path;
    use std::process::Command;

    #[test]
    fn first_https_url_extracts_and_trims_browser_link() {
        assert_eq!(
            first_https_url(
                "To sign in, open this URL:\n  https://auth.openai.com/authorize?code=abc-123 \n"
            ),
            Some("https://auth.openai.com/authorize?code=abc-123".to_string())
        );
        assert_eq!(
            first_https_url("Visit (https://auth.openai.com/x)."),
            Some("https://auth.openai.com/x".to_string())
        );
        assert_eq!(first_https_url("no link in this output"), None);
        // Never returns a bare scheme.
        assert_eq!(first_https_url("https://"), None);
        // http:// is not surfaced — only the https sign-in link.
        assert_eq!(first_https_url("http://insecure.example/x"), None);
    }

    #[test]
    fn detail_tracks_install_and_sign_in_state() {
        assert!(status_detail(false, CodexAuthentication::Unavailable).contains("not included"));
        assert!(status_detail(true, CodexAuthentication::SignedOut).contains("Sign in"));
        assert!(status_detail(true, CodexAuthentication::Authenticated).contains("Signed in"));
        assert!(status_detail(true, CodexAuthentication::Unavailable).contains("could not check"));
    }

    #[test]
    fn readiness_comes_from_bounded_codex_login_status() {
        let home = Path::new("/tmp/goblins-os-codex-status-test-home");
        assert_eq!(
            codex_authentication_with("/usr/bin/true", home),
            CodexAuthentication::Authenticated
        );
        assert_eq!(
            codex_authentication_with("/usr/bin/false", home),
            CodexAuthentication::SignedOut
        );
        assert_eq!(
            codex_authentication_with("goblins-os-codex-missing-status-binary", home),
            CodexAuthentication::Unavailable
        );
    }

    #[test]
    fn login_response_status_matches_body_semantics() {
        let (status, started) = login_start(StatusCode::ACCEPTED, "started", true, false, false);
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(started.started);

        let (status, authenticated) = login_start(StatusCode::OK, "ready", false, true, false);
        assert_eq!(status, StatusCode::OK);
        assert!(authenticated.authenticated);

        let (status, running) = login_start(StatusCode::OK, "running", false, false, true);
        assert_eq!(status, StatusCode::OK);
        assert!(running.already_running);

        let (status, denied) = login_start(StatusCode::FORBIDDEN, "denied", false, false, false);
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!denied.started);

        let (status, unavailable) = login_start(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            false,
            false,
            false,
        );
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!unavailable.started);
    }

    #[test]
    fn supported_logout_is_idempotent_and_bounded() {
        let home = Path::new("/tmp/goblins-os-codex-logout-test-home");
        assert!(run_codex_logout("/usr/bin/true", home));
        assert!(run_codex_logout("/usr/bin/true", home));
        assert!(!run_codex_logout("/usr/bin/false", home));
    }

    #[test]
    fn logout_terminates_login_child_and_clears_log_idempotently() {
        let mut child = Some(
            Command::new("/bin/sleep")
                .arg("30")
                .spawn()
                .expect("spawn login stand-in"),
        );
        terminate_child(&mut child).expect("terminate active login child");
        assert!(child.is_none());

        let log =
            std::env::temp_dir().join(format!("goblins-os-codex-login-log-{}", std::process::id()));
        std::fs::write(&log, "https://auth.openai.example/codex").expect("write login log");
        clear_login_log_at(&log).expect("clear login log");
        assert!(!log.exists());
        clear_login_log_at(&log).expect("clearing an absent log stays idempotent");
    }

    #[test]
    fn studio_rejects_empty_final_output() {
        assert!(validated_studio_message("").is_err());
        assert!(validated_studio_message("  \n\t").is_err());
        assert_eq!(
            validated_studio_message("  Built the app. \n").unwrap(),
            "Built the app."
        );
    }

    #[cfg(unix)]
    #[test]
    fn studio_result_file_is_private_and_never_follows_a_secret_link() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let outside = root.path().join("outside");
        let secret = outside.join("auth.json");
        std::fs::create_dir_all(&outside).expect("outside directory");
        std::fs::write(&secret, b"OS-OWNED-SECRET").expect("secret");

        let result = StudioResultFile::create_at(&apps).expect("private result file");
        assert_eq!(
            result.path.parent(),
            Some(apps.join("codex-results").as_path())
        );
        assert_eq!(
            std::fs::metadata(apps.join("codex-results"))
                .expect("result directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        std::fs::write(&result.path, b"Built safely.").expect("Codex-style result write");
        assert_eq!(result.read().expect("safe result"), "Built safely.");

        std::fs::remove_file(&result.path).expect("replace result path");
        symlink(&secret, &result.path).expect("malicious result symlink");
        assert!(result.read().is_err());
        drop(result);
        assert_eq!(
            std::fs::read(&secret).expect("secret remains"),
            b"OS-OWNED-SECRET"
        );
    }

    #[cfg(unix)]
    #[test]
    fn studio_result_directory_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&apps).expect("apps directory");
        std::fs::create_dir_all(&outside).expect("outside directory");
        symlink(&outside, apps.join("codex-results")).expect("result directory symlink");

        assert!(StudioResultFile::create_at(&apps).is_err());
    }

    #[test]
    fn status_serializes_without_leaking_credentials() {
        let status = CodexStatus {
            source: "goblins-os-core",
            installed: true,
            authenticated: true,
            codex_home: super::PRIVATE_STORAGE_LABEL,
            detail: "ok".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        // Status reports only presence/sign-in, never credentials or their path.
        assert!(json.contains("\"authenticated\":true"));
        assert!(!json.contains("auth.json"));
        assert!(!json.contains("/var/lib/goblins-os/codex"));
        assert!(!json.to_lowercase().contains("token"));
    }

    #[test]
    fn codex_child_environment_strips_daemon_secrets() {
        const TEST_NAME: &str = "GOBLINS_OS_TEST_PARENT_SECRET";
        const TEST_VALUE: &str = "must-not-reach-codex-child";
        let prior = std::env::var_os(TEST_NAME);
        std::env::set_var(TEST_NAME, TEST_VALUE);

        let output =
            isolated_codex_command("/usr/bin/env", Path::new("/tmp/goblins-os-codex-test-home"))
                .output()
                .expect("run isolated environment probe");

        match prior {
            Some(value) => std::env::set_var(TEST_NAME, value),
            None => std::env::remove_var(TEST_NAME),
        }

        assert!(output.status.success());
        let environment = String::from_utf8(output.stdout).expect("environment is UTF-8");
        assert!(!environment.contains(TEST_NAME));
        assert!(!environment.contains(TEST_VALUE));
        for secret_name in [
            "AI_GATEWAY_API_KEY".to_string(),
            "OPENAI_API_KEY".to_string(),
            "OPENAI_ACCOUNT_CLIENT_SECRET".to_string(),
            ["GOBLINS_OS_CODEX", "EXEC_FLAGS"].join("_"),
        ] {
            assert!(
                !environment.contains(&secret_name),
                "secret environment variable reached Codex: {secret_name}"
            );
        }
        assert!(environment.contains("CODEX_HOME=/tmp/goblins-os-codex-test-home"));
        assert!(environment.contains("HOME=/tmp/goblins-os-codex-test-home"));
        assert!(environment.contains(&format!("PATH={}", super::CODEX_CHILD_PATH)));
    }

    #[test]
    fn codex_exec_sandboxes_are_fixed_in_source() {
        let source = include_str!("codex.rs");
        assert!(source.contains(".arg(\"--ignore-user-config\")"));
        assert!(source.contains(".arg(\"--ignore-rules\")"));
        assert!(source.contains("shell_environment_policy.inherit=\\\"none\\\""));
        assert!(source.contains("allow_login_shell=false"));
        assert!(!source.contains(".arg(\"--sandbox\")"));
        assert!(!source.contains(&["GOBLINS_OS_CODEX", "EXEC_FLAGS"].join("_")));
        assert!(!source.contains(&["danger", "full", "access"].join("-")));
        assert!(!source.contains(&["dangerously", "bypass"].join("-")));

        for (configure, profile, base) in [
            (
                configure_resident_sandbox as fn(&mut Command),
                "goblins-resident",
                ":read-only",
            ),
            (
                configure_studio_sandbox as fn(&mut Command),
                "goblins-studio",
                ":workspace",
            ),
        ] {
            let mut command = Command::new("/usr/bin/true");
            configure(&mut command);
            let args = command
                .get_args()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>();
            let joined = args.join(" ");
            assert!(args.iter().any(|arg| arg == "--strict-config"));
            assert!(!args.iter().any(|arg| arg == "--sandbox"));
            assert!(joined.contains(&format!("default_permissions=\"{profile}\"")));
            assert!(joined.contains(&format!("permissions.{profile}.extends=\"{base}\"")));
            assert!(joined.contains("approval_policy=\"never\""));
        }
    }

    #[test]
    fn codex_policies_deny_os_credentials_without_shadowing_the_workspace() {
        let studio_policy = codex_filesystem_policy_override(
            CodexSandboxRole::Studio,
            Path::new("/var/lib/goblins-os/apps"),
            Path::new("/var/lib/goblins-os/codex"),
            Some(Path::new("/run/credentials/goblins-os-core.service")),
            Path::new("/var/lib/goblins-os/secrets/openai/session.json"),
        );
        let resident_policy = codex_filesystem_policy_override(
            CodexSandboxRole::Resident,
            Path::new("/var/lib/goblins-os/apps"),
            Path::new("/var/lib/goblins-os/codex"),
            Some(Path::new("/run/credentials/goblins-os-core.service")),
            Path::new("/var/lib/goblins-os/secrets/openai/session.json"),
        );
        for denied in [
            "/etc/goblins-os",
            "/run/credentials",
            "/run/credentials/goblins-os-core.service",
            "/var/lib/goblins-os/ai",
            "/var/lib/goblins-os/codex",
            "/var/lib/goblins-os/policy",
            "/var/lib/goblins-os/secrets",
            "/var/lib/goblins-os/secrets/openai/session.json",
        ] {
            assert!(studio_policy.contains(&format!("\"{denied}\"=\"deny\"")));
            assert!(resident_policy.contains(&format!("\"{denied}\"=\"deny\"")));
        }
        assert!(studio_policy.contains("\"/var/lib/goblins-os/apps/codex-work\"=\"deny\""));
        assert!(!studio_policy.contains("\"/var/lib/goblins-os/apps/workspace\"=\"deny\""));
        assert!(resident_policy.contains("\"/var/lib/goblins-os/apps/workspace\"=\"deny\""));
        assert!(!resident_policy.contains("\"/var/lib/goblins-os/apps/codex-work\"=\"deny\""));
        for parent in ["/var/lib/goblins-os", "/var/lib/goblins-os/apps"] {
            let parent_deny = format!("\"{parent}\"=\"deny\"");
            assert!(!studio_policy.contains(&parent_deny));
            assert!(!resident_policy.contains(&parent_deny));
        }

        let mut command = Command::new("/usr/bin/true");
        configure_studio_sandbox(&mut command);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(args.contains("default_permissions=\"goblins-studio\""));
        assert!(args.contains("permissions.goblins-studio.extends=\":workspace\""));
        assert!(args.contains("shell_environment_policy.inherit=\"none\""));
        assert!(args.contains("allow_login_shell=false"));
        assert!(args.contains("project_doc_max_bytes=0"));
        assert!(!args.contains("OPENAI_API_KEY"));
        assert!(!args.contains("AI_GATEWAY_API_KEY"));
    }

    #[cfg(unix)]
    #[test]
    fn installed_codex_enforces_both_permission_profiles() {
        if !binary_present("codex") {
            eprintln!("codex_sandbox_test=skip reason=codex-unavailable");
            return;
        }

        let root = tempfile::tempdir().expect("temporary sandbox root");
        let state = root.path().join("state");
        let apps = state.join("apps");
        let studio = apps.join("workspace/live");
        let resident = apps.join("codex-work");
        let codex = state.join("codex");
        let credentials = root.path().join("credentials");
        let session = state.join("secrets/openai/session.json");
        let ai = state.join("ai");
        for directory in [
            studio.as_path(),
            resident.as_path(),
            codex.as_path(),
            credentials.as_path(),
            session.parent().expect("session parent"),
            ai.as_path(),
        ] {
            std::fs::create_dir_all(directory).expect("sandbox fixture directory");
        }
        for file in [
            studio.join("Cargo.toml"),
            resident.join("context.txt"),
            codex.join("auth.json"),
            credentials.join("openai-secrets.env"),
            session.clone(),
            ai.join("engine"),
        ] {
            std::fs::write(file, []).expect("sandbox canary");
        }

        let run_probe = |role: CodexSandboxRole, workspace: &Path, script: &str| {
            let filesystem =
                codex_filesystem_policy_override(role, &apps, &codex, Some(&credentials), &session);
            let mut command = Command::new("codex");
            command
                .arg("sandbox")
                .arg("-C")
                .arg(workspace)
                .arg("-P")
                .arg(role.profile());
            append_codex_permission_profile(&mut command, role, filesystem, false);
            command
                .arg("--")
                .arg("/bin/sh")
                .arg("-c")
                .arg(script)
                .arg("goblins-codex-sandbox-probe")
                .arg(root.path())
                .env("CODEX_HOME", &codex);
            let output = command.output().expect("run Codex sandbox probe");
            assert!(
                output.status.success(),
                "Codex sandbox probe failed for {role:?}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        };

        run_probe(
            CodexSandboxRole::Studio,
            &studio,
            "test -r Cargo.toml && touch built.txt && test ! -r \"$1/state/codex/auth.json\" && test ! -r \"$1/credentials/openai-secrets.env\" && test ! -r \"$1/state/secrets/openai/session.json\" && test ! -r \"$1/state/ai/engine\" && test ! -r \"$1/state/apps/codex-work/context.txt\"",
        );
        assert!(studio.join("built.txt").is_file());

        run_probe(
            CodexSandboxRole::Resident,
            &resident,
            "test -r context.txt && ! touch forbidden.txt && test ! -r \"$1/state/codex/auth.json\" && test ! -r \"$1/credentials/openai-secrets.env\" && test ! -r \"$1/state/secrets/openai/session.json\" && test ! -r \"$1/state/ai/engine\" && test ! -r \"$1/state/apps/workspace/live/Cargo.toml\"",
        );
        assert!(!resident.join("forbidden.txt").exists());
    }
}
