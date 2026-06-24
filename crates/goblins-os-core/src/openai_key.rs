//! Optional bring-your-own OpenAI API key.
//!
//! Goblins OS is centered on the local GPT-OSS model; this module lets a user
//! who *also* wants OpenAI's hosted models supply their own API key. The key is
//! the user's own (billed to their own API account), is written only to OS-owned
//! secret storage (0600), is never returned to any client, and is used only
//! server-side by the core. This is the sanctioned alternative to GPT-OSS — not
//! a baked-in secret and not a fake login.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

const DEFAULT_KEY_PATH: &str = "/var/lib/goblins-os/secrets/openai/api-key";
const DEFAULT_ENGINE_PATH: &str = "/var/lib/goblins-os/resident/engine";
const DEFAULT_OPENAI_MODEL: &str = "gpt-5.5";

/// The default engine: the on-device GPT-OSS heart of Goblins OS.
const ENGINE_LOCAL: &str = "local-gpt-oss";
/// The user's OpenAI account, via the Codex CLI (Sign in with ChatGPT).
const ENGINE_CODEX: &str = "codex";
/// The optional engine: OpenAI's hosted models via the user's own API key.
const ENGINE_OPENAI: &str = "openai-api";

#[derive(Deserialize)]
pub struct SetOpenAiKeyRequest {
    api_key: String,
    consent: bool,
}

#[derive(Deserialize)]
pub struct SetEngineRequest {
    engine: String,
}

#[derive(Serialize)]
pub struct OpenAiKeyStatus {
    configured: bool,
    model: String,
    /// True when the active engine is hosted OpenAI rather than local GPT-OSS.
    engine_selected: bool,
    /// The effective engine label: `local-gpt-oss` (default) or `openai-api`.
    engine: String,
    storage: String,
}

pub async fn openai_key_status() -> Json<OpenAiKeyStatus> {
    Json(build_status())
}

pub async fn set_openai_key(Json(request): Json<SetOpenAiKeyRequest>) -> Response {
    if !request.consent {
        return error_response(
            StatusCode::PRECONDITION_REQUIRED,
            "Storing an OpenAI API key requires explicit consent.",
        );
    }
    let key = request.api_key.trim();
    if !is_plausible_key(key) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "That does not look like an OpenAI API key (expected an 'sk-' secret).",
        );
    }
    if write_key_to(&key_path(), key).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "OpenAI API key could not be written to OS-owned secret storage.",
        );
    }
    Json(build_status()).into_response()
}

/// Select which engine powers the resident (and the app builder): the on-device
/// GPT-OSS heart, or OpenAI's hosted models. Hosted models can only be selected
/// once a personal key is stored — Goblins OS never offers a switch it cannot
/// honor. The choice is persisted in OS-owned state and read by the relay.
pub async fn set_resident_engine(Json(request): Json<SetEngineRequest>) -> Response {
    let engine = request.engine.trim();
    if engine != ENGINE_LOCAL && engine != ENGINE_CODEX && engine != ENGINE_OPENAI {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Unknown engine. Choose GPT-OSS, your OpenAI account (Codex), or the hosted OpenAI API.",
        );
    }
    // Both hosted paths reach the internet, so neither is selectable while offline.
    if (engine == ENGINE_OPENAI || engine == ENGINE_CODEX) && crate::privacy::offline_enabled() {
        return error_response(
            StatusCode::PRECONDITION_REQUIRED,
            "That engine needs the internet. Turn off Private mode first to use it.",
        );
    }
    if engine == ENGINE_CODEX && !crate::codex::codex_available() {
        return error_response(
            StatusCode::PRECONDITION_REQUIRED,
            "Sign in to Codex with your OpenAI account before selecting it.",
        );
    }
    if engine == ENGINE_OPENAI && stored_api_key().is_none() {
        return error_response(
            StatusCode::PRECONDITION_REQUIRED,
            "Add an OpenAI API key before selecting OpenAI's hosted models.",
        );
    }
    if write_engine_to(&engine_path(), engine).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The engine selection could not be saved to OS-owned state.",
        );
    }
    Json(build_status()).into_response()
}

/// The stored key, for server-side use by the core only. The relay refuses to use
/// it while offline, so a stored key never causes egress in private mode.
pub fn stored_api_key() -> Option<String> {
    read_key_from(&key_path())
}

pub(crate) fn configured_model() -> String {
    env::var("GOBLINS_OS_OPENAI_MODEL").unwrap_or_else(|_| DEFAULT_OPENAI_MODEL.to_string())
}

/// True when the active engine is the hosted-OpenAI (BYO key) engine rather than
/// the default local GPT-OSS heart. A persisted OS-owned preference (set from the
/// Settings GUI) is authoritative; absent one, an ops env override is honored.
pub(crate) fn openai_engine_selected() -> bool {
    selected_engine() == ENGINE_OPENAI
}

/// True when the active engine is the user's OpenAI account via Codex CLI.
pub(crate) fn codex_engine_selected() -> bool {
    selected_engine() == ENGINE_CODEX
}

/// The effective engine label, resolving the persisted preference first and
/// falling back to the env override, then to the local GPT-OSS default.
fn selected_engine() -> String {
    if let Some(preference) = engine_preference() {
        return preference;
    }
    match env::var("GOBLINS_OS_RESIDENT_ENGINE").ok().as_deref() {
        Some(ENGINE_OPENAI) => ENGINE_OPENAI.to_string(),
        Some(ENGINE_CODEX) => ENGINE_CODEX.to_string(),
        _ => ENGINE_LOCAL.to_string(),
    }
}

fn build_status() -> OpenAiKeyStatus {
    let engine = selected_engine();
    OpenAiKeyStatus {
        configured: stored_api_key().is_some(),
        model: configured_model(),
        engine_selected: engine == ENGINE_OPENAI,
        engine,
        storage: key_path().display().to_string(),
    }
}

fn is_plausible_key(key: &str) -> bool {
    key.starts_with("sk-") && key.len() >= 20 && key.chars().all(|ch| !ch.is_whitespace())
}

fn key_path() -> PathBuf {
    env::var("GOBLINS_OS_OPENAI_KEY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_KEY_PATH).to_path_buf())
}

fn engine_path() -> PathBuf {
    env::var("GOBLINS_OS_RESIDENT_ENGINE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_ENGINE_PATH).to_path_buf())
}

/// The persisted engine preference, if the user has made an explicit choice.
/// Unlike the API key this is not a secret; it records only which engine is
/// active (`local-gpt-oss` or `openai-api`).
fn engine_preference() -> Option<String> {
    read_engine_from(&engine_path())
}

fn read_engine_from(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn write_engine_to(path: &Path, engine: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, engine.as_bytes())?;
    fs::rename(tmp, path)
}

fn read_key_from(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn write_key_to(path: &Path, key: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(key.as_bytes())?;
        file.sync_all()
    }

    #[cfg(not(unix))]
    {
        fs::write(path, key)
    }
}

fn error_response(status: StatusCode, text: &'static str) -> Response {
    (status, Json(serde_json::json!({ "text": text }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        is_plausible_key, read_engine_from, read_key_from, write_engine_to, write_key_to,
        ENGINE_LOCAL, ENGINE_OPENAI,
    };
    use std::path::PathBuf;

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
    fn key_round_trips_through_os_owned_storage_and_is_never_in_status_shape() {
        let dir = unique_tmp("goblins-os-key");
        let path = dir.join("api-key");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(read_key_from(&path), None);
        write_key_to(&path, "sk-proj-secretvalue1234567890").expect("write key");
        assert_eq!(
            read_key_from(&path).as_deref(),
            Some("sk-proj-secretvalue1234567890")
        );

        // The status struct exposes only booleans/model/path — never the key field.
        let status_json = serde_json::to_string(&super::OpenAiKeyStatus {
            configured: true,
            model: "gpt-5.5".to_string(),
            engine_selected: false,
            engine: ENGINE_LOCAL.to_string(),
            storage: path.display().to_string(),
        })
        .unwrap();
        assert!(!status_json.contains("sk-proj-secretvalue"));

        let _ = std::fs::remove_dir_all(&dir);
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

        // Re-selecting overwrites in place — the OS holds a single active engine.
        write_engine_to(&path, ENGINE_LOCAL).expect("re-select engine");
        assert_eq!(read_engine_from(&path).as_deref(), Some(ENGINE_LOCAL));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn stored_key_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = unique_tmp("goblins-os-key-mode");
        let path = dir.join("api-key");
        let _ = std::fs::remove_dir_all(&dir);
        write_key_to(&path, "sk-proj-secretvalue1234567890").expect("write key");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "the API key must be readable only by its owner"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
