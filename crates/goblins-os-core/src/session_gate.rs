use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{
    auth::{openai_account_authenticated, openai_auth_provider_configured},
    http_error::error_response,
    installer::first_boot_completion_mode,
};

#[derive(Serialize)]
pub struct SessionGateStatus {
    generated_at: String,
    source: &'static str,
    unlocked: bool,
    mode: Option<String>,
    first_boot_mode: Option<String>,
    lock: SessionLock,
}

#[derive(Serialize)]
pub struct SessionLock {
    state: SessionLockState,
    reason: String,
    openai_account_required: bool,
    local_mode_available: bool,
    state_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionLockState {
    Unlocked,
    WaitingForFirstBoot,
    RequiresOpenAIAccount,
    LocalOnlyAvailable,
}

#[derive(Deserialize)]
pub struct SessionUnlockRequest {
    mode: String,
}

#[derive(Serialize)]
struct SessionUnlockResponse {
    ok: bool,
    mode: &'static str,
    state_path: String,
}

#[derive(Serialize, Deserialize)]
struct StoredSessionGate {
    mode: String,
    unlocked_at: String,
}

pub async fn session_gate_status() -> Json<SessionGateStatus> {
    Json(build_session_gate_status())
}

pub async fn unlock_session(Json(request): Json<SessionUnlockRequest>) -> Response {
    let mode = match validate_session_unlock_mode(&request.mode, openai_account_authenticated()) {
        Ok(mode) => mode,
        Err(status) => {
            return error_response(
                status,
                "Goblins OS cannot unlock the desktop with the requested identity mode.",
            );
        }
    };

    if persist_session_gate(mode).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Goblins OS could not persist the OS session unlock state.",
        );
    }

    Json(SessionUnlockResponse {
        ok: true,
        mode,
        state_path: session_gate_state_path().display().to_string(),
    })
    .into_response()
}

fn build_session_gate_status() -> SessionGateStatus {
    let first_boot_mode = first_boot_completion_mode();
    let stored_gate = read_session_gate();
    let openai_ready = openai_account_authenticated();
    let mode = stored_gate
        .as_ref()
        .map(|gate| gate.mode.clone())
        .filter(|mode| mode == "cloud-openai" || mode == "local-gpt-oss");
    let unlocked = match mode.as_deref() {
        Some("cloud-openai") => openai_ready,
        Some("local-gpt-oss") => true,
        _ => false,
    };

    let lock = if unlocked {
        SessionLock {
            state: SessionLockState::Unlocked,
            reason: "Goblins OS session is unlocked by the local OS identity gate.".to_string(),
            openai_account_required: matches!(mode.as_deref(), Some("cloud-openai")),
            local_mode_available: true,
            state_path: session_gate_state_path().display().to_string(),
        }
    } else if first_boot_mode.is_none() {
        SessionLock {
            state: SessionLockState::WaitingForFirstBoot,
            reason: "First boot has not completed; the installer owns initial OpenAI or local-only selection.".to_string(),
            openai_account_required: true,
            local_mode_available: true,
            state_path: session_gate_state_path().display().to_string(),
        }
    } else if first_boot_mode.as_deref() == Some("cloud-openai") {
        SessionLock {
            state: SessionLockState::RequiresOpenAIAccount,
            reason: if openai_auth_provider_configured() {
                "This Goblins OS profile requires the real OpenAI account session before the desktop unlocks.".to_string()
            } else {
                "This Goblins OS profile requires OpenAI account login, but the provider is not configured.".to_string()
            },
            openai_account_required: true,
            local_mode_available: true,
            state_path: session_gate_state_path().display().to_string(),
        }
    } else {
        SessionLock {
            state: SessionLockState::LocalOnlyAvailable,
            reason: "Local-only profile selected; unlock records a local gpt-oss desktop session without cloud credentials.".to_string(),
            openai_account_required: false,
            local_mode_available: true,
            state_path: session_gate_state_path().display().to_string(),
        }
    };

    SessionGateStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        unlocked,
        mode,
        first_boot_mode,
        lock,
    }
}

fn validate_session_unlock_mode(
    requested_mode: &str,
    openai_ready: bool,
) -> Result<&'static str, StatusCode> {
    match requested_mode {
        "cloud-openai" => {
            if openai_ready {
                Ok("cloud-openai")
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
        "local-gpt-oss" => Ok("local-gpt-oss"),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

pub(crate) fn persist_session_gate(mode: &'static str) -> std::io::Result<()> {
    let path = session_gate_state_path();
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other(
            "session gate state path has no parent",
        ));
    };

    create_state_dir(parent)?;
    let body = serde_json::to_vec(&StoredSessionGate {
        mode: mode.to_string(),
        unlocked_at: format!("{:?}", SystemTime::now()),
    })?;
    // Write to a sibling temp file then rename onto the final path, so a crash or
    // concurrent overwrite can never leave a truncated session.json that parses to
    // None and silently fails the gate open.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body)?;
    fs::rename(tmp, path)
}

fn read_session_gate() -> Option<StoredSessionGate> {
    let bytes = fs::read(session_gate_state_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn session_gate_state_path() -> PathBuf {
    env::var("GOBLINS_OS_SESSION_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/var/lib/goblins-os/session").to_path_buf())
        .join("gate.json")
}

fn create_state_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o750))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{session_gate_state_path, validate_session_unlock_mode};
    use axum::http::StatusCode;

    #[test]
    fn session_gate_state_uses_os_owned_default_path() {
        if std::env::var("GOBLINS_OS_SESSION_STATE").is_ok() {
            return;
        }

        assert_eq!(
            session_gate_state_path().to_string_lossy(),
            "/var/lib/goblins-os/session/gate.json"
        );
    }

    #[test]
    fn cloud_unlock_requires_openai_account_session() {
        assert_eq!(
            validate_session_unlock_mode("cloud-openai", false),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            validate_session_unlock_mode("cloud-openai", true),
            Ok("cloud-openai")
        );
    }

    #[test]
    fn local_unlock_does_not_require_cloud_session() {
        assert_eq!(
            validate_session_unlock_mode("local-gpt-oss", false),
            Ok("local-gpt-oss")
        );
    }
}
