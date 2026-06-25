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

use crate::auth::{openai_account_authenticated, openai_auth_provider_configured};
use crate::http_error::error_response;
use crate::model_manager::build_local_model_catalog;
use crate::session_gate::persist_session_gate;

#[derive(Serialize)]
pub struct InstallerReadiness {
    generated_at: String,
    source: &'static str,
    first_boot: FirstBootState,
    profile: InstallerProfile,
    stages: Vec<InstallerStage>,
    privacy_note: &'static str,
    storage_note: String,
}

#[derive(Serialize)]
pub struct InstallerProfile {
    default_mode: &'static str,
    cloud_requires_openai_account: bool,
    local_requires_cloud_login: bool,
}

#[derive(Serialize)]
pub struct FirstBootState {
    completed: bool,
    state_path: String,
}

#[derive(Serialize)]
pub struct InstallerStage {
    id: &'static str,
    index: &'static str,
    label: &'static str,
    state: InstallerStageState,
    detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallerStageState {
    Ready,
    Local,
    Waiting,
    Blocked,
}

pub async fn installer_readiness() -> Json<InstallerReadiness> {
    Json(build_installer_readiness())
}

pub async fn complete_installer(Json(request): Json<InstallerCompleteRequest>) -> Response {
    let mode = match request.mode.as_str() {
        "cloud-openai" => {
            if !openai_account_authenticated() {
                return error_response(
                    StatusCode::FORBIDDEN,
                    "Cloud OpenAI first boot cannot complete until the OpenAI account session is stored by Goblins OS.",
                );
            }

            "cloud-openai"
        }
        "local-gpt-oss" => "local-gpt-oss",
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Installer completion mode is not supported by Goblins OS.",
            );
        }
    };

    if persist_first_boot_completion(mode).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Goblins OS could not persist first-boot installer state.",
        );
    }
    if persist_session_gate(mode).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Goblins OS could not unlock the OS session after first boot.",
        );
    }

    Json(InstallerCompleteResponse {
        ok: true,
        mode,
        state_path: first_boot_state_path().display().to_string(),
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct InstallerCompleteRequest {
    mode: String,
}

#[derive(Serialize)]
struct InstallerCompleteResponse {
    ok: bool,
    mode: &'static str,
    state_path: String,
}

#[derive(Serialize, Deserialize)]
struct StoredInstallerCompletion {
    mode: String,
    completed_at: String,
}

fn build_installer_readiness() -> InstallerReadiness {
    let local_models = build_local_model_catalog();
    let hardware = local_models.hardware();
    let openai_provider_ready = openai_auth_provider_configured();
    let openai_account_ready = openai_provider_ready && openai_account_authenticated();
    let build_sandbox_ready = env::var("GOBLINS_OS_BUILD_SANDBOX").is_ok();
    let shell_session_ready = env::var("GOBLINS_OS_SESSION").is_ok();
    let model_storage_ready = hardware.model_dir_available_gb().is_some();
    let runtime_ready = hardware.runtime_ready();
    let installable_count = local_models.installable_model_count();
    let blocked_count = local_models.blocked_model_count();
    let model_gate_state = if installable_count > 0 {
        InstallerStageState::Ready
    } else if blocked_count > 0 {
        InstallerStageState::Blocked
    } else {
        InstallerStageState::Waiting
    };
    let storage_note = match hardware.model_dir_available_gb() {
        Some(available) => format!(
            "{available}GB available for local AI models; downloads are checked before they start."
        ),
        None => "Storage for local AI models could not be measured yet.".to_string(),
    };

    InstallerReadiness {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        first_boot: FirstBootState {
            completed: first_boot_state_path().is_file(),
            state_path: first_boot_state_path().display().to_string(),
        },
        profile: InstallerProfile {
            default_mode: "cloud-openai",
            cloud_requires_openai_account: true,
            local_requires_cloud_login: false,
        },
        privacy_note:
            "Local AI prompts stay on this device unless the user chooses hosted models or explicitly shares data.",
        storage_note: storage_note.clone(),
        stages: vec![
            InstallerStage {
                id: "pre-auth-lock",
                index: "01",
                label: "OpenAI account",
                state: if openai_account_ready {
                    InstallerStageState::Ready
                } else {
                    InstallerStageState::Waiting
                },
                detail: if openai_account_ready {
                    "OpenAI sign-in is ready for this desktop.".to_string()
                } else if openai_provider_ready {
                    "OpenAI sign-in is available; waiting for account login."
                        .to_string()
                } else {
                    "OpenAI sign-in is not set up yet; local-only setup remains available."
                        .to_string()
                },
            },
            InstallerStage {
                id: "hardware-scan",
                index: "02",
                label: "Hardware scan",
                state: InstallerStageState::Ready,
                detail: format!(
                    "{}GB RAM detected; GPU VRAM is {}.",
                    hardware.ram_gb(),
                    hardware
                        .gpu_vram_gb()
                        .map(|vram| format!("{vram}GB"))
                        .unwrap_or_else(|| "not detected".to_string())
                ),
            },
            InstallerStage {
                id: "inference-runtime",
                index: "03",
                label: "Local AI",
                state: if runtime_ready {
                    InstallerStageState::Ready
                } else {
                    InstallerStageState::Waiting
                },
                detail: if runtime_ready {
                    "A local AI engine is available for offline prompts.".to_string()
                } else {
                    "Local AI will become available after setup finishes or a model engine is selected."
                        .to_string()
                },
            },
            InstallerStage {
                id: "model-download-gate",
                index: "04",
                label: "Model downloads",
                state: model_gate_state,
                detail: if installable_count > 0 {
                    format!("{installable_count} local model option can be offered during install.")
                } else if blocked_count > 0 {
                    format!(
                        "{blocked_count} model option is disabled until hardware requirements pass."
                    )
                } else {
                    "Models are not bundled; downloads wait for storage checks and user approval."
                        .to_string()
                },
            },
            InstallerStage {
                id: "model-storage",
                index: "05",
                label: "Model storage",
                state: if model_storage_ready {
                    InstallerStageState::Ready
                } else {
                    InstallerStageState::Waiting
                },
                // The headline GB figure lives in the summary storage note; this
                // step describes the mechanics so the two never read as twins.
                detail: "Local AI model downloads are verified before they are installed."
                    .to_string(),
            },
            InstallerStage {
                id: "build-studio",
                index: "06",
                label: "Build Studio",
                state: if build_sandbox_ready {
                    InstallerStageState::Ready
                } else {
                    InstallerStageState::Waiting
                },
                detail: if build_sandbox_ready {
                    "Build Studio is ready to create apps on this device."
                        .to_string()
                } else {
                    "App building will be available after setup checks finish."
                        .to_string()
                },
            },
            InstallerStage {
                id: "boot-session",
                index: "07",
                label: "Boot session",
                state: if shell_session_ready {
                    InstallerStageState::Ready
                } else {
                    InstallerStageState::Waiting
                },
                detail: if shell_session_ready {
                    "The Goblins OS desktop is ready to open.".to_string()
                } else {
                    "The desktop will open after first-run setup completes."
                        .to_string()
                },
            },
            InstallerStage {
                id: "local-mode-policy",
                index: "08",
                label: "Local privacy",
                state: InstallerStageState::Local,
                detail:
                    "Local mode does not require cloud login; downloads are explicit and resumable."
                        .to_string(),
            },
        ],
    }
}

fn persist_first_boot_completion(mode: &'static str) -> std::io::Result<()> {
    let path = first_boot_state_path();
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("installer state path has no parent"));
    };

    create_state_dir(parent)?;
    let body = serde_json::to_vec(&StoredInstallerCompletion {
        mode: mode.to_string(),
        completed_at: format!("{:?}", SystemTime::now()),
    })?;
    fs::write(path, body)
}

pub(crate) fn first_boot_completion_mode() -> Option<String> {
    let bytes = fs::read(first_boot_state_path()).ok()?;
    serde_json::from_slice::<StoredInstallerCompletion>(&bytes)
        .ok()
        .map(|completion| completion.mode)
}

fn first_boot_state_path() -> PathBuf {
    env::var("GOBLINS_OS_INSTALLER_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/var/lib/goblins-os/installer").to_path_buf())
        .join("first-boot.json")
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
    use super::first_boot_state_path;

    #[test]
    fn first_boot_state_uses_os_owned_default_path() {
        if std::env::var("GOBLINS_OS_INSTALLER_STATE").is_ok() {
            return;
        }

        assert_eq!(
            first_boot_state_path().to_string_lossy(),
            "/var/lib/goblins-os/installer/first-boot.json"
        );
    }
}
