//! The user's OpenAI account, via Codex CLI.
//!
//! Codex is OpenAI's own open-source coding agent (Rust, runs locally). Signing
//! in with a ChatGPT account uses that plan's included Codex usage, so this is the
//! honest way to put a user's OpenAI *account* — not just an API key — behind
//! Goblins OS. The OS detects the Codex CLI and its sign-in, drives it
//! non-interactively (`codex exec`) as a resident engine, and never stores the
//! account credentials itself — Codex owns them under an OS-set `CODEX_HOME`.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
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
    codex_home().join("work")
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
    use super::{status_detail, CodexStatus};

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
