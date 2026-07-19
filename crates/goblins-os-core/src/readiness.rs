use axum::Json;
use serde::Serialize;
use std::{env, time::SystemTime};

use crate::auth::{openai_account_authenticated, openai_auth_provider_configured};

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadinessState {
    Ready,
    Local,
    Waiting,
    Blocked,
}

#[derive(Serialize)]
pub struct ReadinessCheck {
    id: &'static str,
    label: &'static str,
    state: ReadinessState,
    detail: String,
}

#[derive(Serialize)]
pub struct ReadinessResponse {
    generated_at: String,
    checks: Vec<ReadinessCheck>,
}

pub async fn readiness() -> Json<ReadinessResponse> {
    Json(build_readiness())
}

pub fn build_readiness() -> ReadinessResponse {
    let gateway_ready = crate::resident::managed_cloud_route_configured()
        || crate::resident::local_model_route_configured();
    let build_sandbox_ready = env::var("GOBLINS_OS_BUILD_SANDBOX").is_ok();
    let bootc_image_ready = env::var("GOBLINS_OS_BOOTC_IMAGE").is_ok();
    let model_dir_ready = env::var("GOBLINS_OS_MODEL_DIR").is_ok();
    let shell_session_ready = env::var("GOBLINS_OS_SESSION").is_ok();
    let native_shell_ready =
        env::var("GOBLINS_OS_SHELL_MODE").is_ok_and(|value| value == "native-desktop");
    let gui_platform_ready =
        env::var("GOBLINS_OS_GUI_PLATFORM").is_ok_and(|value| value == "gnome-session");
    let openai_provider_ready = openai_auth_provider_configured();
    let openai_account_ready = openai_provider_ready && openai_account_authenticated();
    let public_secret_leak = env::vars().any(|(key, _)| {
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
    });

    ReadinessResponse {
        generated_at: format!("{:?}", SystemTime::now()),
        checks: vec![
            ReadinessCheck {
                id: "rust-core",
                label: "Goblins OS services",
                state: ReadinessState::Ready,
                detail: "Goblins OS services are running.".to_string(),
            },
            ReadinessCheck {
                id: "bootc-image",
                label: "System image",
                state: if bootc_image_ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Waiting
                },
                detail: if bootc_image_ready {
                    "System image delivery is configured for this device.".to_string()
                } else {
                    "Waiting for the signed system image for the final OS build.".to_string()
                },
            },
            ReadinessCheck {
                id: "shell-session",
                label: "Goblins OS desktop",
                state: if shell_session_ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Waiting
                },
                detail: if shell_session_ready {
                    "Goblins OS native shell session is configured as the boot experience."
                        .to_string()
                } else {
                    "Goblins OS native desktop session is not active in this runtime.".to_string()
                },
            },
            ReadinessCheck {
                id: "gnome-session-platform",
                label: "Desktop session",
                state: if gui_platform_ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Waiting
                },
                detail: if gui_platform_ready {
                    "Desktop session integration is configured for compositor, input, portal, and app controls.".to_string()
                } else {
                    "Waiting for the OS image to declare desktop session integration.".to_string()
                },
            },
            ReadinessCheck {
                id: "native-rust-shell",
                label: "Goblins OS shell",
                state: if native_shell_ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Waiting
                },
                detail: if native_shell_ready {
                    "The Goblins OS shell is configured as the primary desktop experience."
                        .to_string()
                } else {
                    "Waiting for the Goblins OS shell to become the primary desktop experience."
                        .to_string()
                },
            },
            ReadinessCheck {
                id: "local-services",
                label: "Local OS services",
                state: ReadinessState::Local,
                detail: "Local OS services run on this device and do not require external hosting."
                    .to_string(),
            },
            ReadinessCheck {
                id: "openai-account",
                label: "OpenAI account handoff",
                state: if openai_account_ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Waiting
                },
                detail: if openai_account_ready {
                    "Server-owned OpenAI account session is present in OS secret storage."
                        .to_string()
                } else if openai_provider_ready {
                    "OpenAI account provider is configured; waiting for the account login flow."
                        .to_string()
                } else {
                    "No supported OpenAI account identity provider is configured yet.".to_string()
                },
            },
            ReadinessCheck {
                id: "goblins-ai-runtime",
                label: "Goblins AI runtime",
                state: if gateway_ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Waiting
                },
                detail: if gateway_ready {
                    "Goblins AI has OS-owned model access configured.".to_string()
                } else {
                    "Waiting for cloud or local model access before Goblins AI can answer."
                        .to_string()
                },
            },
            ReadinessCheck {
                id: "local-model-manager",
                label: "Local model manager",
                state: if model_dir_ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Waiting
                },
                detail: if model_dir_ready {
                    "gpt-oss downloads are routed to an OS-owned model directory; weights are not bundled in the image.".to_string()
                } else {
                    "Waiting for an OS-owned model directory before optional gpt-oss downloads are enabled.".to_string()
                },
            },
            ReadinessCheck {
                id: "build-studio",
                label: "Build Studio",
                state: if build_sandbox_ready && gateway_ready {
                    ReadinessState::Ready
                } else {
                    ReadinessState::Waiting
                },
                detail: if build_sandbox_ready && gateway_ready {
                    "Local app building and OS-owned model access are configured.".to_string()
                } else {
                    "Waiting for local app building and model access before app creation is ready."
                        .to_string()
                },
            },
            ReadinessCheck {
                id: "secret-isolation",
                label: "Secret isolation",
                state: if public_secret_leak {
                    ReadinessState::Blocked
                } else {
                    ReadinessState::Ready
                },
                detail: if public_secret_leak {
                    "Sensitive configuration names were detected in the desktop session."
                        .to_string()
                } else {
                    "No sensitive key names are exposed to desktop applications.".to_string()
                },
            },
        ],
    }
}
