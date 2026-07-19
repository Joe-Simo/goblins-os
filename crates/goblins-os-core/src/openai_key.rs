//! Optional operator-provisioned OpenAI API key.
//!
//! Goblins OS is centered on the local GPT-OSS model. An administrator can also
//! provision an OpenAI API key through the core service's protected systemd
//! credential. The key is never accepted from, returned to, or stored by a
//! desktop process; only the server-side core can read and use it.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use crate::credentials::openai_credential;
use crate::http_error::error_response;
use crate::policy::{policy_state_for_control, PolicyControlState};

const DEFAULT_ENGINE_PATH: &str = "/var/lib/goblins-os/ai/engine";
const DEFAULT_OPENAI_MODEL: &str = "gpt-5.6";
const PRIVATE_STORAGE_LABEL: &str = "protected system credential";

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
    model: String,
    /// True only when the user's BYO OpenAI API engine is selected.
    engine_selected: bool,
    /// The explicit effective engine label.
    engine: String,
    /// Compatibility field for existing clients. This is deliberately a label,
    /// never the raw path of the OS-owned secret.
    storage: &'static str,
}

pub async fn openai_key_status() -> Json<OpenAiKeyStatus> {
    Json(build_status())
}

/// Preserve a valid resident route before Codex removes its credentials. This
/// is kept inside the engine-state module so the logout path does not duplicate
/// the authoritative persistence location or engine identifiers.
pub(crate) fn fail_safe_from_codex_to_local() -> std::io::Result<()> {
    fail_safe_selection_to_local(selected_engine(), EngineSelection::Codex, &engine_path())
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
pub async fn set_resident_engine(Json(request): Json<SetEngineRequest>) -> Response {
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
    if selection == EngineSelection::Codex && !crate::codex::codex_available() {
        return error_response(
            StatusCode::PRECONDITION_REQUIRED,
            "Sign in to Codex with your OpenAI account before selecting it.",
        );
    }
    if selection == EngineSelection::OpenAiApi && stored_api_key().is_none() {
        return error_response(
            StatusCode::PRECONDITION_REQUIRED,
            "A device administrator must install an OpenAI API key before you can select OpenAI's hosted models.",
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
    if write_engine_to(&engine_path(), selection.as_id()).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The engine selection could not be saved to OS-owned state.",
        );
    }
    Json(build_status()).into_response()
}

/// The provisioned key, for server-side use by the core only. It is loaded on
/// demand from systemd's protected credential directory and never enters the
/// process environment or a desktop process. The relay still refuses to use it
/// while offline, so its presence never causes egress in Private mode.
pub fn stored_api_key() -> Option<String> {
    openai_credential("OPENAI_API_KEY")
        .map(|key| key.trim().to_string())
        .filter(|key| is_plausible_key(key))
}

pub(crate) fn configured_model() -> String {
    env::var("GOBLINS_OS_OPENAI_MODEL").unwrap_or_else(|_| DEFAULT_OPENAI_MODEL.to_string())
}

/// The effective engine label, resolving the persisted preference first and
/// falling back to the env override, then to the local GPT-OSS default.
pub(crate) fn selected_engine() -> EngineSelection {
    if let Some(preference) = engine_preference() {
        if let Some(selection) = EngineSelection::from_id(&preference) {
            return selection;
        }
    }
    env::var("GOBLINS_OS_RESIDENT_ENGINE")
        .ok()
        .and_then(|value| EngineSelection::from_id(&value))
        .unwrap_or(EngineSelection::LocalGptOss)
}

fn build_status() -> OpenAiKeyStatus {
    let engine = selected_engine();
    OpenAiKeyStatus {
        configured: stored_api_key().is_some(),
        model: configured_model(),
        engine_selected: engine == EngineSelection::OpenAiApi,
        engine: engine.as_id().to_string(),
        storage: PRIVATE_STORAGE_LABEL,
    }
}

fn is_plausible_key(key: &str) -> bool {
    key.starts_with("sk-") && key.len() >= 20 && key.chars().all(|ch| !ch.is_whitespace())
}

fn engine_path() -> PathBuf {
    env::var_os("GOBLINS_OS_AI_ENGINE_PATH")
        .or_else(|| env::var_os("GOBLINS_OS_RESIDENT_ENGINE_PATH"))
        .map(PathBuf::from)
        .or_else(|| env::var_os("GOBLINS_OS_AI_STATE").map(|dir| PathBuf::from(dir).join("engine")))
        .unwrap_or_else(|| Path::new(DEFAULT_ENGINE_PATH).to_path_buf())
}

/// The persisted engine preference, if the user has made an explicit choice.
/// Unlike the API key this is not a secret; it records only which engine is
/// active (`local-gpt-oss` or `openai-api`).
fn engine_preference() -> Option<String> {
    read_engine_from(&engine_path())
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
    fs::create_dir_all(parent)?;
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
        fail_safe_selection_to_local, is_plausible_key, read_engine_from, write_engine_to,
        EngineSelection, ENGINE_LOCAL, ENGINE_MANAGED_CLOUD, ENGINE_OPENAI,
    };
    use std::{
        os::unix::fs::{symlink, PermissionsExt},
        path::PathBuf,
    };

    fn unique_tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    #[test]
    fn only_plausible_openai_keys_are_accepted() {
        assert!(is_plausible_key("sk-proj-abcdefghijklmnopqrstuvwxyz"));
        assert!(is_plausible_key("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(!is_plausible_key("hello")); // wrong prefix + too short
        assert!(!is_plausible_key("sk-short")); // too short
        assert!(!is_plausible_key("sk-has spaces in it aaaaaaaaaa")); // whitespace
    }

    #[test]
    fn key_status_shape_never_has_a_secret_or_private_path() {
        let status_json = serde_json::to_string(&super::OpenAiKeyStatus {
            configured: true,
            model: "gpt-5.6".to_string(),
            engine_selected: false,
            engine: ENGINE_LOCAL.to_string(),
            storage: super::PRIVATE_STORAGE_LABEL,
        })
        .unwrap();
        assert!(!status_json.contains("sk-proj-secretvalue"));
        assert!(!status_json.contains("/var/lib/goblins-os"));
        assert!(status_json.contains("protected system credential"));
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
