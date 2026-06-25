//! The user's OpenAI account, via Codex CLI.
//!
//! Codex is OpenAI's own open-source coding agent (Rust, runs locally). Signing
//! in with a ChatGPT account uses that plan's included Codex usage, so this is the
//! honest way to put a user's OpenAI *account* — not just an API key — behind
//! Goblins OS. The OS detects the Codex CLI and its sign-in, drives it
//! non-interactively (`codex exec`) as a resident engine, and never stores the
//! account credentials itself — Codex owns them under an OS-set `CODEX_HOME`.

use std::{
    env,
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
};

use axum::Json;
use serde::Serialize;

const DEFAULT_CODEX_HOME: &str = "/var/lib/goblins-os/codex";

#[derive(Serialize)]
pub struct CodexStatus {
    source: &'static str,
    installed: bool,
    authenticated: bool,
    codex_home: String,
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
pub async fn codex_login_start() -> Json<CodexLoginStart> {
    Json(start_login())
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
    auth_file().is_file()
}

/// Usable as the resident engine only when the CLI is present and signed in.
pub(crate) fn codex_available() -> bool {
    codex_installed() && codex_authenticated()
}

fn build_status() -> CodexStatus {
    let installed = codex_installed();
    let authenticated = installed && codex_authenticated();
    CodexStatus {
        source: "goblins-os-core",
        installed,
        authenticated,
        codex_home: codex_home().display().to_string(),
        detail: status_detail(installed, authenticated),
    }
}

fn status_detail(installed: bool, authenticated: bool) -> String {
    if !installed {
        "Codex account support is not included in this build. Start from the full Goblins OS image to use your OpenAI account through Codex.".to_string()
    } else if !authenticated {
        "Codex account support is ready. Sign in with your OpenAI account to use it.".to_string()
    } else {
        "Signed in to Codex with your OpenAI account.".to_string()
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

    let workspace = work_dir();
    fs::create_dir_all(&workspace).map_err(|_| "Codex workspace could not be created")?;
    let last_message = workspace.join("last-message.txt");
    let _ = fs::remove_file(&last_message);

    let status = Command::new(codex_bin())
        .env("CODEX_HOME", codex_home())
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--output-last-message")
        .arg(&last_message)
        .args(exec_flags())
        .arg(prompt)
        .current_dir(&workspace)
        .status()
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

/// Run one Codex turn non-interactively inside `workspace`, returning the agent's
/// final message. Codex reads, writes, and (in its sandbox) runs files in the
/// workspace, so this is a true agent step — the Studio captures the result and
/// then lists whatever files the agent produced. Errors carry a calm message.
pub(crate) fn run_codex_in(workspace: &Path, prompt: &str) -> Result<String, String> {
    if !codex_installed() {
        return Err("Codex account support is not included in this build.".to_string());
    }
    if !codex_authenticated() {
        return Err("Codex is not signed in. Sign in with your OpenAI account first.".to_string());
    }

    fs::create_dir_all(workspace).map_err(|_| "Could not open the Codex workspace.".to_string())?;
    let last_message = workspace.join(".goblins-codex-last-message.txt");
    let _ = fs::remove_file(&last_message);

    let status = Command::new(codex_bin())
        .env("CODEX_HOME", codex_home())
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--output-last-message")
        .arg(&last_message)
        .args(exec_flags())
        .arg(prompt)
        .current_dir(workspace)
        .status()
        .map_err(|_| "Codex could not start.".to_string())?;
    if !status.success() {
        return Err("Codex did not complete the request.".to_string());
    }

    let text = fs::read_to_string(&last_message).unwrap_or_default();
    let _ = fs::remove_file(&last_message);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Ok("Codex finished building.".to_string())
    } else {
        Ok(trimmed.chars().take(4000).collect())
    }
}

/// Extra `codex exec` flags, overridable for the OS operator. Kept minimal by
/// default so the agent's own configured sandbox and approval policy apply.
fn exec_flags() -> Vec<String> {
    match env::var("GOBLINS_OS_CODEX_EXEC_FLAGS") {
        Ok(value) if !value.trim().is_empty() => {
            value.split_whitespace().map(str::to_string).collect()
        }
        _ => Vec::new(),
    }
}

fn codex_bin() -> String {
    env::var("GOBLINS_OS_CODEX_BIN").unwrap_or_else(|_| "codex".to_string())
}

fn codex_home() -> PathBuf {
    env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_CODEX_HOME).to_path_buf())
}

fn auth_file() -> PathBuf {
    codex_home().join("auth.json")
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
    detail: &str,
    started: bool,
    authenticated: bool,
    already_running: bool,
) -> CodexLoginStart {
    CodexLoginStart {
        started,
        authenticated,
        already_running,
        detail: detail.to_string(),
    }
}

fn start_login() -> CodexLoginStart {
    if !codex_installed() {
        return login_start(
            "Codex account support is not included in this build.",
            false,
            false,
            false,
        );
    }
    if codex_authenticated() {
        return login_start(
            "Already signed in to Codex with your OpenAI account.",
            false,
            true,
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
        Err(_) => return login_start("Codex sign-in could not open its log.", false, false, false),
    };
    let log_err = match log.try_clone() {
        Ok(file) => file,
        Err(_) => return login_start("Codex sign-in could not open its log.", false, false, false),
    };

    match Command::new(codex_bin())
        .env("CODEX_HOME", &home)
        .arg("login")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
    {
        Ok(child) => {
            *guard = Some(child);
            login_start(
                "Codex sign-in started. Opening your browser to finish.",
                true,
                false,
                false,
            )
        }
        Err(_) => login_start("Codex sign-in could not start.", false, false, false),
    }
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
    use super::{first_https_url, status_detail, CodexStatus};

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
        assert!(status_detail(false, false).contains("not included"));
        assert!(status_detail(true, false).contains("Sign in"));
        assert!(status_detail(true, true).contains("Signed in"));
    }

    #[test]
    fn status_serializes_without_leaking_credentials() {
        let status = CodexStatus {
            source: "goblins-os-core",
            installed: true,
            authenticated: true,
            codex_home: "/var/lib/goblins-os/codex".to_string(),
            detail: "ok".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        // Status reports only presence/sign-in + the home path, never the token.
        assert!(json.contains("\"authenticated\":true"));
        assert!(!json.contains("auth.json"));
        assert!(!json.to_lowercase().contains("token"));
    }
}
