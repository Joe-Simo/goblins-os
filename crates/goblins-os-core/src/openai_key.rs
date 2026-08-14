//! Per-user OpenAI engine selection and non-secret key status.
//!
//! Goblins OS ships with no provider key. A user may ask a dedicated trusted
//! broker to store their own key after installation; ordinary desktop clients
//! can only read readiness metadata and select an engine. Plaintext key access
//! remains confined to the protected core execution path.

use axum::{
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use crate::http_error::error_response;
use crate::policy::{policy_state_for_control, PolicyControlState};

const DEFAULT_ENGINE_ROOT: &str = "/var/lib/goblins-os/ai/users";
const DEFAULT_OPENAI_MODEL: &str = "gpt-5.6";
const PRIVATE_STORAGE_LABEL: &str = "protected on-device credential";

/// The default engine: the on-device GPT-OSS heart of Goblins OS.
const ENGINE_LOCAL: &str = "local-gpt-oss";
/// The user's OpenAI account, via the Codex CLI (Sign in with ChatGPT).
const ENGINE_CODEX: &str = "codex";
/// The optional engine: OpenAI's hosted models via the user's own API key.
const ENGINE_OPENAI: &str = "openai-api";
/// An operator-managed HTTPS relay. This is a cloud engine and is never a
/// fallback for the on-device selection; it must be selected explicitly.
const ENGINE_MANAGED_CLOUD: &str = "cloud-openai";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EngineSelection {
    LocalGptOss,
    Codex,
    OpenAiApi,
    ManagedCloud,
}

impl EngineSelection {
    pub(crate) fn from_id(value: &str) -> Option<Self> {
        match value.trim() {
            ENGINE_LOCAL => Some(Self::LocalGptOss),
            ENGINE_CODEX => Some(Self::Codex),
            ENGINE_OPENAI => Some(Self::OpenAiApi),
            ENGINE_MANAGED_CLOUD => Some(Self::ManagedCloud),
            _ => None,
        }
    }

    pub(crate) const fn as_id(self) -> &'static str {
        match self {
            Self::LocalGptOss => ENGINE_LOCAL,
            Self::Codex => ENGINE_CODEX,
            Self::OpenAiApi => ENGINE_OPENAI,
            Self::ManagedCloud => ENGINE_MANAGED_CLOUD,
        }
    }

    pub(crate) const fn is_hosted(self) -> bool {
        !matches!(self, Self::LocalGptOss)
    }
}

#[derive(Deserialize)]
pub struct SetEngineRequest {
    engine: String,
}

#[derive(Serialize)]
pub struct OpenAiKeyStatus {
    configured: bool,
    /// Non-secret, requester-scoped progress state for the protected broker.
    key_change_pending: bool,
    model: String,
    /// True only when the user's BYO OpenAI API engine is selected.
    engine_selected: bool,
    /// The explicit effective engine label.
    engine: String,
    /// Compatibility field for existing clients. This is deliberately a label,
    /// never the raw path of the OS-owned secret.
    storage: &'static str,
}

pub async fn openai_key_status(
    Extension(client): Extension<crate::control_plane::RequestClient>,
) -> Json<OpenAiKeyStatus> {
    Json(build_status(client.user_id()))
}

/// Preserve a valid resident route before Codex removes its credentials. This
/// is kept inside the engine-state module so the logout path does not duplicate
/// the authoritative persistence location or engine identifiers.
pub(crate) fn fail_safe_from_codex_to_local() -> std::io::Result<()> {
    let root = engine_root();
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut changed = false;
    for entry in entries {
        let entry = entry?;
        let Some(user_id) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        changed |= crate::openai_key_provisioning::with_user_key_engine_mutation(user_id, || {
            let path = engine_path_for(user_id);
            let current = selected_engine_for(user_id);
            fail_safe_selection_to_local(current, EngineSelection::Codex, &path)?;
            Ok::<bool, std::io::Error>(current == EngineSelection::Codex)
        })?;
    }
    if changed {
        crate::resident::bump_hosted_authority_generation();
    }
    Ok(())
}

pub(crate) fn fail_safe_user_api_to_local(user_id: u32) -> std::io::Result<()> {
    let current = selected_engine_for(user_id);
    fail_safe_selection_to_local(
        current,
        EngineSelection::OpenAiApi,
        &engine_path_for(user_id),
    )
}

fn fail_safe_selection_to_local(
    current: EngineSelection,
    removed: EngineSelection,
    path: &Path,
) -> std::io::Result<()> {
    if current == removed {
        write_engine_to(path, ENGINE_LOCAL)
    } else {
        Ok(())
    }
}

/// Select which engine powers the resident (and the app builder): the on-device
/// GPT-OSS heart, or OpenAI's hosted models. Hosted models can only be selected
/// once the protected service credential is ready — Goblins OS never offers a
/// switch it cannot honor. The choice is persisted in OS-owned state and read
/// by the relay.
pub async fn set_resident_engine(
    Extension(client): Extension<crate::control_plane::RequestClient>,
    Json(request): Json<SetEngineRequest>,
) -> Response {
    let user_id = client.user_id();
    let Some(selection) = EngineSelection::from_id(&request.engine) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Unknown engine. Choose GPT-OSS, your OpenAI account (Codex), your OpenAI API key, or the managed OpenAI service.",
        );
    };

    // Every hosted route is gated here before the preference is persisted, and
    // again by the authoritative resolver immediately before execution.
    if selection.is_hosted() && crate::privacy::offline_enabled() {
        return error_response(
            StatusCode::PRECONDITION_REQUIRED,
            "That engine needs the internet. Turn off Private mode first to use it.",
        );
    }
    if selection.is_hosted()
        && policy_state_for_control("cloud-openai") != PolicyControlState::Allowed
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "OpenAI cloud services are blocked by the active Goblins OS policy.",
        );
    }
    crate::openai_key_provisioning::with_user_key_engine_mutation(user_id, || {
        if selection == EngineSelection::Codex && !crate::codex::codex_available() {
            return error_response(
                StatusCode::PRECONDITION_REQUIRED,
                "Sign in to Codex with your OpenAI account before selecting it.",
            );
        }
        if selection == EngineSelection::OpenAiApi
            && !crate::openai_key_provisioning::credential_is_stored(user_id)
        {
            return error_response(
                StatusCode::PRECONDITION_REQUIRED,
                "Add your OpenAI API key in the protected key window before selecting this engine.",
            );
        }
        if selection == EngineSelection::OpenAiApi && !crate::resident::openai_api_base_is_valid() {
            return error_response(
                StatusCode::PRECONDITION_REQUIRED,
                "The configured OpenAI service address must use HTTPS before this engine can be selected.",
            );
        }
        if selection == EngineSelection::ManagedCloud
            && !crate::resident::managed_cloud_route_configured()
        {
            return error_response(
                StatusCode::PRECONDITION_REQUIRED,
                "The managed OpenAI service is not configured with a valid HTTPS route.",
            );
        }
        if write_engine_to(&engine_path_for(user_id), selection.as_id()).is_err() {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The engine selection could not be saved to OS-owned state.",
            );
        }
        crate::resident::bump_hosted_authority_generation();
        Json(build_status(user_id)).into_response()
    })
}

pub(crate) fn configured_model() -> String {
    env::var("GOBLINS_OS_OPENAI_MODEL").unwrap_or_else(|_| DEFAULT_OPENAI_MODEL.to_string())
}

/// The effective engine label, resolving the persisted preference first and
/// falling back to the env override, then to the local GPT-OSS default.
pub(crate) fn selected_engine_for(user_id: u32) -> EngineSelection {
    if let Some(preference) = engine_preference(user_id) {
        if let Some(selection) = EngineSelection::from_id(&preference) {
            return selection;
        }
    }
    env::var("GOBLINS_OS_RESIDENT_ENGINE")
        .ok()
        .and_then(|value| EngineSelection::from_id(&value))
        .unwrap_or(EngineSelection::LocalGptOss)
}

fn build_status(user_id: u32) -> OpenAiKeyStatus {
    let engine = selected_engine_for(user_id);
    OpenAiKeyStatus {
        configured: crate::openai_key_provisioning::credential_is_stored(user_id),
        key_change_pending: crate::openai_key_provisioning::management_pending(user_id),
        model: configured_model(),
        engine_selected: engine == EngineSelection::OpenAiApi,
        engine: engine.as_id().to_string(),
        storage: PRIVATE_STORAGE_LABEL,
    }
}

fn engine_root() -> PathBuf {
    env::var_os("GOBLINS_OS_AI_STATE")
        .map(|directory| PathBuf::from(directory).join("users"))
        .unwrap_or_else(|| Path::new(DEFAULT_ENGINE_ROOT).to_path_buf())
}

fn engine_path_for(user_id: u32) -> PathBuf {
    engine_root().join(user_id.to_string()).join("engine")
}

/// The persisted engine preference, if the user has made an explicit choice.
/// Unlike the API key this is not a secret; it records only which engine is
/// active (`local-gpt-oss` or `openai-api`).
fn engine_preference(user_id: u32) -> Option<String> {
    read_engine_from(&engine_path_for(user_id))
}

fn read_engine_from(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 {
        return None;
    }
    let value = fs::read_to_string(path).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn write_engine_to(path: &Path, engine: &str) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("engine state path has no parent"));
    };
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let parent_metadata = fs::symlink_metadata(parent)?;
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.permissions().mode() & 0o7777 != 0o700
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "engine state directory is not private",
        ));
    }
    let tmp = parent.join(format!(
        ".engine-{}-{:016x}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(engine.as_bytes())?;
        file.sync_all()?;
        if path.exists() {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.file_type().is_file()
                || metadata.file_type().is_symlink()
                || metadata.permissions().mode() & 0o7777 != 0o600
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "engine state destination is not a protected regular file",
                ));
            }
        }
        fs::rename(&tmp, path)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write_result?;
    fs::File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::{
        fail_safe_selection_to_local, read_engine_from, write_engine_to, EngineSelection,
        ENGINE_LOCAL, ENGINE_MANAGED_CLOUD, ENGINE_OPENAI,
    };
    use std::{
        os::unix::fs::{symlink, PermissionsExt},
        path::PathBuf,
    };

    fn unique_tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    #[test]
    fn key_status_shape_never_has_a_secret_or_private_path() {
        let status_json = serde_json::to_string(&super::OpenAiKeyStatus {
            configured: true,
            key_change_pending: false,
            model: "gpt-5.6".to_string(),
            engine_selected: false,
            engine: ENGINE_LOCAL.to_string(),
            storage: super::PRIVATE_STORAGE_LABEL,
        })
        .unwrap();
        assert!(!status_json.contains("sk-proj-secretvalue"));
        assert!(!status_json.contains("/var/lib/goblins-os"));
        assert!(status_json.contains("protected on-device credential"));
    }

    #[test]
    fn engine_preference_round_trips_through_os_owned_state() {
        let dir = unique_tmp("goblins-os-engine");
        let path = dir.join("engine");
        let _ = std::fs::remove_dir_all(&dir);

        // No file yet means no explicit choice (the caller falls back to default).
        assert_eq!(read_engine_from(&path), None);

        write_engine_to(&path, ENGINE_OPENAI).expect("write engine preference");
        assert_eq!(read_engine_from(&path).as_deref(), Some(ENGINE_OPENAI));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(std::fs::read_dir(&dir).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));

        // Re-selecting overwrites in place — the OS holds a single active engine.
        write_engine_to(&path, ENGINE_LOCAL).expect("re-select engine");
        assert_eq!(read_engine_from(&path).as_deref(), Some(ENGINE_LOCAL));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_preference_reader_rejects_symlinks_and_oversized_state() {
        let dir = unique_tmp("goblins-os-engine-invalid");
        let path = dir.join("engine");
        let target = dir.join("target");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&target, ENGINE_LOCAL).unwrap();
        symlink(&target, &path).unwrap();
        assert_eq!(read_engine_from(&path), None);

        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "x".repeat(65)).unwrap();
        assert_eq!(read_engine_from(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_the_selected_codex_route_fails_safe_to_local_first() {
        let dir = unique_tmp("goblins-os-codex-engine-fail-safe");
        let path = dir.join("engine");
        let _ = std::fs::remove_dir_all(&dir);

        fail_safe_selection_to_local(EngineSelection::Codex, EngineSelection::Codex, &path)
            .expect("switch selected Codex route to local");
        assert_eq!(read_engine_from(&path).as_deref(), Some(ENGINE_LOCAL));

        let _ = std::fs::remove_dir_all(&dir);
        fail_safe_selection_to_local(EngineSelection::LocalGptOss, EngineSelection::Codex, &path)
            .expect("leave an unrelated route unchanged");
        assert_eq!(read_engine_from(&path), None);
    }

    #[test]
    fn engine_selection_is_typed_and_never_accepts_unknown_fallbacks() {
        assert_eq!(
            EngineSelection::from_id(ENGINE_LOCAL),
            Some(EngineSelection::LocalGptOss)
        );
        assert_eq!(
            EngineSelection::from_id(ENGINE_MANAGED_CLOUD),
            Some(EngineSelection::ManagedCloud)
        );
        assert!(EngineSelection::ManagedCloud.is_hosted());
        assert!(!EngineSelection::LocalGptOss.is_hosted());
        assert_eq!(EngineSelection::from_id("automatic"), None);
        assert_eq!(EngineSelection::from_id(""), None);
    }
}
