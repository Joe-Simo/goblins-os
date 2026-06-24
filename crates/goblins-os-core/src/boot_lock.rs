use axum::Json;
use serde::Serialize;
use std::{env, time::SystemTime};

use crate::auth::{openai_account_authenticated, openai_auth_provider_configured};

#[derive(Serialize)]
pub struct BootLockStatus {
    generated_at: String,
    source: &'static str,
    locked: bool,
    active_mode: &'static str,
    cloud: ModeGate,
    local: ModeGate,
    session: SessionGate,
    privacy_note: &'static str,
}

#[derive(Serialize)]
pub struct ModeGate {
    label: &'static str,
    state: GateState,
    detail: String,
    action_label: &'static str,
    requirements: Vec<String>,
}

#[derive(Serialize)]
pub struct SessionGate {
    shell_configured: bool,
    gui_platform_configured: bool,
    native_shell_configured: bool,
    secret_isolation: bool,
    pre_auth_lock: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateState {
    Ready,
    Local,
    Waiting,
    Blocked,
}

pub async fn boot_lock_status() -> Json<BootLockStatus> {
    Json(build_boot_lock_status())
}

pub fn build_boot_lock_status() -> BootLockStatus {
    let openai_provider_ready = openai_auth_provider_configured();
    let openai_account_ready = openai_provider_ready && openai_account_authenticated();
    let shell_configured = env::var("GOBLINS_OS_SESSION").is_ok();
    let gui_platform_configured =
        env::var("GOBLINS_OS_GUI_PLATFORM").is_ok_and(|value| value == "gnome-session");
    let native_shell_configured =
        env::var("GOBLINS_OS_SHELL_MODE").is_ok_and(|value| value == "native-desktop");
    let secret_isolation = !public_secret_leak();
    let pre_auth_lock = !openai_account_ready;

    let mut cloud_requirements = Vec::new();
    if !openai_provider_ready {
        cloud_requirements.push(
            "Configure the server-side OpenAI account identity provider before cloud login."
                .to_string(),
        );
    } else if !openai_account_ready {
        cloud_requirements
            .push("Complete OpenAI account login before unlocking cloud services.".to_string());
    }
    if !secret_isolation {
        cloud_requirements.push("Remove sensitive-looking public environment keys.".to_string());
    }

    let mut local_requirements = Vec::new();
    if !native_shell_configured {
        local_requirements.push(
            "Configure the Rust native desktop shell before exposing local model setup."
                .to_string(),
        );
    }
    local_requirements.push(
        "Run installer hardware, storage, driver, and inference runtime checks before model downloads."
            .to_string(),
    );

    BootLockStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        locked: pre_auth_lock,
        active_mode: "cloud-openai",
        cloud: ModeGate {
            label: "Cloud OpenAI",
            state: if !secret_isolation {
                GateState::Blocked
            } else if openai_account_ready {
                GateState::Ready
            } else {
                GateState::Waiting
            },
            detail: if openai_account_ready {
                "OpenAI sign-in is ready for this desktop.".to_string()
            } else if openai_provider_ready {
                "OpenAI sign-in is available; waiting for account login.".to_string()
            } else {
                "OpenAI sign-in is not set up yet; local-only desktop access remains available."
                    .to_string()
            },
            action_label: if openai_account_ready {
                "Continue with OpenAI"
            } else if openai_provider_ready {
                "Sign in with OpenAI"
            } else {
                "OpenAI sign-in not set up"
            },
            requirements: cloud_requirements,
        },
        local: ModeGate {
            label: "Local gpt-oss",
            state: if native_shell_configured && secret_isolation {
                GateState::Local
            } else {
                GateState::Blocked
            },
            detail:
                "Local mode does not require ChatGPT or Codex cloud login, and model weights are downloaded only after installer consent."
                    .to_string(),
            action_label: "Continue local setup",
            requirements: local_requirements,
        },
        session: SessionGate {
            shell_configured,
            gui_platform_configured,
            native_shell_configured,
            secret_isolation,
            pre_auth_lock,
        },
        privacy_note:
            "Goblins OS does not fake OpenAI login and does not expose client-side API keys.",
    }
}

fn public_secret_leak() -> bool {
    env::vars().any(|(key, _)| {
        key.starts_with("NEXT_PUBLIC_")
            && [
                "OPENAI",
                "AI_GATEWAY",
                "KEY",
                "TOKEN",
                "SECRET",
                "CLIENT_SECRET",
            ]
            .iter()
            .any(|marker| key.contains(marker))
    })
}

#[cfg(test)]
mod tests {
    use super::public_secret_leak;

    #[test]
    fn public_secret_scan_is_available() {
        let _ = public_secret_leak();
    }
}
