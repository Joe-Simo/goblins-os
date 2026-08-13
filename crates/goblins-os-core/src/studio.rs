//! The Build Studio: a minimal, multi-turn agent build session over whichever
//! engine is active — local GPT-OSS, the user's OpenAI account via Codex, or a
//! bring-your-own key. The user describes what to build; the engine answers and
//! produces files in the app's workspace; the conversation and the files are
//! persisted so the Studio is a real, resumable surface. It is the single place
//! Goblins OS turns intent into software, and it looks the same whichever brain
//! runs — Codex builds files directly, while a chat model builds through a small
//! file-emit step. No pre-installed apps; everything here was built on request.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs,
    io::{self, Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Extension, Query},
    http::StatusCode,
    Json,
};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ai::{audit_ai_action, AiActionOutcome};
use crate::app_builder;

const MAX_MESSAGE_CHARS: usize = 4000;
const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_WORKSPACE_DEPTH: usize = 32;
const MAX_WORKSPACE_FILES: usize = 2_000;
const MAX_CHECKPOINT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CHECKPOINT_FILE_BYTES: usize = 96 * 1024 * 1024;
const MAX_DIFF_BYTES: usize = 96 * 1024;
const MAX_EXPORT_ARCHIVE_BYTES: usize = 20 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    text: String,
    at: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredSession {
    id: String,
    name: String,
    engine: String,
    created_at: String,
    updated_at: String,
    thread: Vec<Message>,
}

#[derive(Serialize)]
pub struct StudioSession {
    id: String,
    name: String,
    engine: String,
    created_at: String,
    updated_at: String,
    thread: Vec<Message>,
    files: Vec<String>,
    undo_available: bool,
}

#[derive(Serialize)]
pub struct SessionSummary {
    id: String,
    name: String,
    engine: String,
    updated_at: String,
    turns: usize,
}

#[derive(Serialize)]
pub struct SessionList {
    count: usize,
    sessions: Vec<SessionSummary>,
}

#[derive(Deserialize)]
pub struct TurnRequest {
    #[serde(default)]
    app_id: Option<String>,
    message: String,
}

#[derive(Serialize)]
pub struct TurnOutcome {
    ok: bool,
    text: String,
    session: Option<StudioSession>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct StudioWorkspaceFile {
    path: String,
    size_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct StudioFileChange {
    path: String,
    size_bytes: u64,
    change: String,
}

#[derive(Deserialize)]
pub struct UndoTurnRequest {
    app_id: String,
}

#[derive(Serialize, Deserialize)]
struct WorkspaceCheckpoint {
    had_workspace: bool,
    previous_session: Option<StoredSession>,
    directories: Vec<String>,
    files: Vec<CheckpointFile>,
}

#[derive(Serialize, Deserialize)]
struct CheckpointFile {
    path: String,
    content: Vec<u8>,
}

struct WorkspaceReview {
    files: Vec<StudioWorkspaceFile>,
    total_bytes: u64,
    content_digest: [u8; 32],
}

struct StagedWorkspace {
    root: Dir,
    directory: Dir,
    name: OsString,
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct StudioTransactionJournal {
    version: u8,
    app_id: String,
    workspace_stage: Option<String>,
    workspace_backup: String,
    workspace_had_current: bool,
    session_stage: Option<String>,
    session_backup: String,
    session_had_current: bool,
}

impl Drop for StagedWorkspace {
    fn drop(&mut self) {
        let _ = self.root.remove_dir_all(&self.name);
    }
}

#[derive(Deserialize)]
pub struct SessionQuery {
    app_id: String,
}

#[derive(Deserialize)]
pub struct FileQuery {
    app_id: String,
    path: String,
}

#[derive(Serialize)]
pub struct FileView {
    path: String,
    content: String,
    truncated: bool,
    binary: bool,
    change: String,
    diff: String,
    diff_truncated: bool,
    previous_available: bool,
}

#[derive(Serialize)]
pub struct StudioCapability {
    available: bool,
    detail: String,
}

#[derive(Serialize)]
pub struct StudioRuntimeCapability {
    available: bool,
    kind: String,
    entrypoint: Option<String>,
    detail: String,
}

#[derive(Serialize)]
pub struct StudioProjectReview {
    ok: bool,
    id: String,
    name: String,
    files: Vec<StudioFileChange>,
    file_count: usize,
    total_bytes: u64,
    workspace_sha256: String,
    runtime: StudioRuntimeCapability,
    export: StudioCapability,
    containerization: StudioCapability,
    last_run: Option<StudioRunRecord>,
}

#[derive(Deserialize)]
pub struct StudioActionRequest {
    app_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct StudioRunRecord {
    state: String,
    entrypoint: String,
    started_at: String,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    logs_truncated: bool,
    detail: String,
    #[serde(default)]
    workspace_sha256: String,
    #[serde(default)]
    workspace_current: bool,
}

#[derive(Serialize)]
pub struct StudioRunOutcome {
    ok: bool,
    text: String,
    run: Option<StudioRunRecord>,
}

#[derive(Serialize)]
pub struct StudioExportOutcome {
    ok: bool,
    text: String,
    path: Option<String>,
    sha256: Option<String>,
}

#[derive(Serialize)]
pub struct StudioContainerOutcome {
    ok: bool,
    text: String,
    path: Option<String>,
    sha256: Option<String>,
    image_ref: Option<String>,
}

fn studio_app_lock(id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<BTreeMap<String, Weak<Mutex<()>>>>> = OnceLock::new();
    let locks = LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(id).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(id.to_string(), Arc::downgrade(&lock));
    lock
}

fn session_snapshot_digest(session: Option<&StoredSession>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"goblins-studio-session-v1\0");
    match session {
        Some(session) => {
            digest.update([1]);
            if let Ok(encoded) = serde_json::to_vec(session) {
                digest.update((encoded.len() as u64).to_le_bytes());
                digest.update(encoded);
            }
        }
        None => digest.update([0]),
    }
    digest.finalize().into()
}

pub(crate) fn recover_all_studio_transactions() -> Result<(), String> {
    let apps_root = app_builder::apps_dir();
    recover_all_studio_transactions_at(&apps_root)
}

fn recover_all_studio_transactions_at(apps_root: &Path) -> Result<(), String> {
    let transactions = match open_transactions_dir_at(apps_root, false) {
        Ok(transactions) => transactions,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err("The Studio transaction store could not be opened safely.".to_string())
        }
    };
    let mut ids = Vec::new();
    for entry in transactions
        .entries()
        .map_err(|_| "The Studio transaction store could not be inspected.".to_string())?
    {
        let entry =
            entry.map_err(|_| "A Studio transaction entry could not be inspected.".to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err("A Studio transaction name was not valid text.".to_string());
        };
        let file_type = entry
            .file_type()
            .map_err(|_| "A Studio transaction type could not be inspected.".to_string())?;
        if name.starts_with(".goblins-studio-") && name.ends_with(".tmp") {
            if !file_type.is_file() {
                return Err("A stale Studio journal temporary was not a regular file.".to_string());
            }
            transactions.remove_file(name).map_err(|_| {
                "A stale Studio journal temporary could not be removed.".to_string()
            })?;
            continue;
        }
        if !file_type.is_file() || !name.ends_with(".json") {
            return Err("The Studio transaction store contains an unsafe entry.".to_string());
        }
        let id = name.trim_end_matches(".json");
        if !canonical_workspace_id(id).is_ok_and(|canonical| canonical == id) {
            return Err("A Studio transaction app identifier was not valid.".to_string());
        }
        ids.push(id.to_string());
    }
    for id in ids {
        let app_lock = studio_app_lock(&id);
        let _app_guard = app_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recover_interrupted_studio_transaction(apps_root, &id)?;
    }
    sync_directory(&transactions)
        .map_err(|_| "The Studio transaction cleanup could not be made durable.".to_string())
}

pub async fn studio_sessions() -> Json<SessionList> {
    let sessions = list_summaries();
    Json(SessionList {
        count: sessions.len(),
        sessions,
    })
}

pub async fn studio_session(Query(query): Query<SessionQuery>) -> (StatusCode, Json<TurnOutcome>) {
    let id = match canonical_workspace_id(&query.app_id) {
        Ok(id) => id,
        Err(_) => {
            return turn_error(
                StatusCode::BAD_REQUEST,
                "That Build Studio app identifier is not valid.",
            )
        }
    };
    let app_lock = studio_app_lock(&id);
    let _app_guard = app_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if recover_interrupted_studio_transaction(&app_builder::apps_dir(), &id).is_err() {
        return turn_error(
            StatusCode::CONFLICT,
            "An interrupted Studio transaction could not be recovered safely.",
        );
    }
    match load_session(&id) {
        Some(stored) => (
            StatusCode::OK,
            Json(TurnOutcome {
                ok: true,
                text: "Session loaded.".to_string(),
                session: Some(hydrate(stored)),
            }),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(TurnOutcome {
                ok: false,
                text: "No Build Studio session for that app.".to_string(),
                session: None,
            }),
        ),
    }
}

pub async fn studio_file(Query(query): Query<FileQuery>) -> (StatusCode, Json<FileView>) {
    let empty = || FileView {
        path: query.path.clone(),
        content: String::new(),
        truncated: false,
        binary: false,
        change: "unavailable".to_string(),
        diff: String::new(),
        diff_truncated: false,
        previous_available: false,
    };
    if canonical_workspace_id(&query.app_id).is_err() || relative_file_path(&query.path).is_err() {
        return (StatusCode::BAD_REQUEST, Json(empty()));
    }
    let id = canonical_workspace_id(&query.app_id).expect("Studio app id was validated");
    let app_lock = studio_app_lock(&id);
    let _app_guard = app_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if recover_interrupted_studio_transaction(&app_builder::apps_dir(), &id).is_err() {
        return (StatusCode::CONFLICT, Json(empty()));
    }
    match workspace_file_view_at(&app_builder::apps_dir(), &id, &query.path) {
        Ok(view) => (StatusCode::OK, Json(view)),
        Err(_) => (StatusCode::NOT_FOUND, Json(empty())),
    }
}

pub async fn studio_project(
    Query(query): Query<SessionQuery>,
) -> (StatusCode, Json<StudioProjectReview>) {
    let id = match canonical_workspace_id(&query.app_id) {
        Ok(id) => id,
        Err(_) => return project_error(StatusCode::BAD_REQUEST, query.app_id),
    };
    let app_lock = studio_app_lock(&id);
    let _app_guard = app_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if recover_interrupted_studio_transaction(&app_builder::apps_dir(), &id).is_err() {
        return project_error(StatusCode::CONFLICT, id);
    }
    let Some(session) = load_session(&id) else {
        return project_error(StatusCode::NOT_FOUND, id);
    };
    match project_review_at(&app_builder::apps_dir(), &session) {
        Ok(review) => (StatusCode::OK, Json(review)),
        Err(_) => project_error(StatusCode::CONFLICT, id),
    }
}

/// Run the one fixed, detected Python entrypoint in the networkless Studio
/// sandbox. This is a bounded local execution, never a model turn or a shell
/// command supplied by the project.
pub async fn studio_run(
    Json(request): Json<StudioActionRequest>,
) -> (StatusCode, Json<StudioRunOutcome>) {
    crate::bounded::run_blocking(move || studio_run_blocking(request))
        .await
        .unwrap_or_else(|_| {
            run_error(
                StatusCode::TOO_MANY_REQUESTS,
                crate::bounded::LONG_OPERATION_BUSY_MESSAGE,
            )
        })
}

fn studio_run_blocking(request: StudioActionRequest) -> (StatusCode, Json<StudioRunOutcome>) {
    let id = match canonical_workspace_id(&request.app_id) {
        Ok(id) => id,
        Err(_) => {
            return run_error(
                StatusCode::BAD_REQUEST,
                "That Build Studio app identifier is not valid.",
            )
        }
    };
    let app_lock = studio_app_lock(&id);
    let _app_guard = app_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let apps_root = app_builder::apps_dir();
    if recover_interrupted_studio_transaction(&apps_root, &id).is_err() {
        return run_error(
            StatusCode::CONFLICT,
            "An interrupted Studio transaction could not be recovered safely.",
        );
    }
    if load_session(&id).is_none() {
        return run_error(
            StatusCode::NOT_FOUND,
            "No Build Studio session exists for that project.",
        );
    }
    let manifest = match workspace_manifest_at(&apps_root, &id) {
        Ok(manifest) => manifest,
        Err(_) => {
            return run_error(
                StatusCode::CONFLICT,
                "The Studio workspace could not be reviewed safely before running.",
            )
        }
    };
    let files = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let runtime = crate::studio_runtime::runtime_status(&files);
    if !runtime.available {
        return run_error(StatusCode::CONFLICT, &runtime.detail);
    }
    let entrypoint = runtime.entrypoint.unwrap_or_default();
    let result = if runtime.kind == "static-web" {
        match workspace_snapshot_at(&apps_root, &id, &manifest) {
            Ok(snapshot) => crate::studio_runtime::open_web_preview(&id, snapshot, &entrypoint),
            Err(_) => {
                return run_error(
                    StatusCode::CONFLICT,
                    "The static preview snapshot could not be read safely.",
                )
            }
        }
    } else {
        let workspace = match open_workspace_at(&apps_root, &id, false) {
            Ok(workspace) => workspace.into_std_file(),
            Err(_) => {
                return run_error(
                    StatusCode::CONFLICT,
                    "The Studio workspace could not be opened safely.",
                )
            }
        };
        crate::studio_runtime::run_python(workspace, &files)
    };
    let run = StudioRunRecord {
        state: result.state.to_string(),
        entrypoint,
        started_at: now_secs(),
        exit_code: result.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
        logs_truncated: result.logs_truncated,
        detail: result.detail,
        workspace_sha256: hex_digest(manifest.content_digest),
        workspace_current: true,
    };
    if write_run_record_at(&apps_root, &id, &run).is_err() {
        return run_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The local run finished, but its logs could not be saved safely.",
        );
    }
    (
        StatusCode::OK,
        Json(StudioRunOutcome {
            ok: matches!(run.state.as_str(), "completed" | "preview-opened"),
            text: run.detail.clone(),
            run: Some(run),
        }),
    )
}

/// Create byte-for-byte deterministic project tar data, then ask the trusted
/// desktop-session bridge to save it in the signed-in user's Downloads folder.
pub async fn studio_export(
    Json(request): Json<StudioActionRequest>,
) -> (StatusCode, Json<StudioExportOutcome>) {
    crate::bounded::run_blocking(move || studio_export_blocking(request))
        .await
        .unwrap_or_else(|_| {
            export_error(
                StatusCode::TOO_MANY_REQUESTS,
                crate::bounded::LONG_OPERATION_BUSY_MESSAGE,
            )
        })
}

fn studio_export_blocking(request: StudioActionRequest) -> (StatusCode, Json<StudioExportOutcome>) {
    let id = match canonical_workspace_id(&request.app_id) {
        Ok(id) => id,
        Err(_) => {
            return export_error(
                StatusCode::BAD_REQUEST,
                "That Build Studio app identifier is not valid.",
            )
        }
    };
    let app_lock = studio_app_lock(&id);
    let _app_guard = app_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let apps_root = app_builder::apps_dir();
    if recover_interrupted_studio_transaction(&apps_root, &id).is_err() {
        return export_error(
            StatusCode::CONFLICT,
            "An interrupted Studio transaction could not be recovered safely.",
        );
    }
    if load_session(&id).is_none() {
        return export_error(
            StatusCode::NOT_FOUND,
            "No Build Studio session exists for that project.",
        );
    }
    let archive = match deterministic_export_at(&apps_root, &id) {
        Ok(archive) => archive,
        Err(detail) => return export_error(StatusCode::CONFLICT, &detail),
    };
    let digest = hex_digest(Sha256::digest(&archive).into());
    let suggested_name = format!("{id}.tar");
    match crate::session_bridge::save_studio_export(&suggested_name, &archive, &digest) {
        crate::session_bridge::SessionBridgeResult::Success(path) => (
            StatusCode::OK,
            Json(StudioExportOutcome {
                ok: true,
                text: format!("Exported a deterministic project archive to {path}."),
                path: Some(path),
                sha256: Some(digest),
            }),
        ),
        crate::session_bridge::SessionBridgeResult::Failed(detail) => export_error(
            StatusCode::BAD_GATEWAY,
            &format!("The desktop could not save the Studio export: {detail}"),
        ),
        crate::session_bridge::SessionBridgeResult::Unavailable => export_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "The desktop export service is not available in this session.",
        ),
    }
}

/// Package a reviewed static web workspace as a normalized OCI image-layout
/// archive. This is an offline data transformation: it neither invokes a
/// container engine nor executes project code or contacts a registry.
pub async fn studio_containerize(
    Json(request): Json<StudioActionRequest>,
) -> (StatusCode, Json<StudioContainerOutcome>) {
    crate::bounded::run_blocking(move || studio_containerize_blocking(request))
        .await
        .unwrap_or_else(|_| {
            container_error(
                StatusCode::TOO_MANY_REQUESTS,
                crate::bounded::LONG_OPERATION_BUSY_MESSAGE,
            )
        })
}

fn studio_containerize_blocking(
    request: StudioActionRequest,
) -> (StatusCode, Json<StudioContainerOutcome>) {
    let id = match canonical_workspace_id(&request.app_id) {
        Ok(id) => id,
        Err(_) => {
            return container_error(
                StatusCode::BAD_REQUEST,
                "That Build Studio app identifier is not valid.",
            )
        }
    };
    let app_lock = studio_app_lock(&id);
    let _app_guard = app_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let apps_root = app_builder::apps_dir();
    if recover_interrupted_studio_transaction(&apps_root, &id).is_err() {
        return container_error(
            StatusCode::CONFLICT,
            "An interrupted Studio transaction could not be recovered safely.",
        );
    }
    if load_session(&id).is_none() {
        return container_error(
            StatusCode::NOT_FOUND,
            "No Build Studio session exists for that project.",
        );
    }
    let manifest = match workspace_manifest_at(&apps_root, &id) {
        Ok(manifest) => manifest,
        Err(_) => {
            return container_error(
                StatusCode::CONFLICT,
                "The Studio workspace could not be reviewed safely for container packaging.",
            )
        }
    };
    let paths = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let capability = crate::studio_container::container_status(&paths);
    if !capability.available {
        return container_error(StatusCode::CONFLICT, &capability.detail);
    }
    let snapshot =
        match workspace_snapshot_at(&apps_root, &id, &manifest) {
            Ok(snapshot) => snapshot,
            Err(_) => return container_error(
                StatusCode::CONFLICT,
                "The reviewed project snapshot could not be read safely for container packaging.",
            ),
        };
    let workspace_sha256 = hex_digest(manifest.content_digest);
    let container = match crate::studio_container::build_container(&id, &workspace_sha256, snapshot)
    {
        Ok(container) => container,
        Err(detail) => return container_error(StatusCode::CONFLICT, &detail),
    };
    let suggested_name = format!("{id}-container.oci.tar");
    match crate::session_bridge::save_studio_export(
        &suggested_name,
        &container.bytes,
        &container.sha256,
    ) {
        crate::session_bridge::SessionBridgeResult::Success(path) => (
            StatusCode::OK,
            Json(StudioContainerOutcome {
                ok: true,
                text: format!(
                    "Saved deterministic OCI image {} to {path}. Import it with Podman or another OCI-compatible tool.",
                    container.image_ref
                ),
                path: Some(path),
                sha256: Some(container.sha256),
                image_ref: Some(container.image_ref),
            }),
        ),
        crate::session_bridge::SessionBridgeResult::Failed(detail) => container_error(
            StatusCode::BAD_GATEWAY,
            &format!("The desktop could not save the Studio container: {detail}"),
        ),
        crate::session_bridge::SessionBridgeResult::Unavailable => container_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "The desktop export service is not available in this session.",
        ),
    }
}

/// A Studio turn is a real agent run — `codex exec` under its 600s bound or a
/// model turn through the resident relay (120s+ read timeout) — so the body
/// runs on the blocking pool instead of pinning an async runtime worker.
pub async fn studio_turn(
    client: Option<Extension<crate::control_plane::RequestClient>>,
    Json(request): Json<TurnRequest>,
) -> (StatusCode, Json<TurnOutcome>) {
    let client = client.map(|Extension(client)| client);
    crate::bounded::run_blocking(move || studio_turn_blocking(request, client))
        .await
        .unwrap_or_else(|_| {
            turn_error(
                StatusCode::TOO_MANY_REQUESTS,
                crate::bounded::LONG_OPERATION_BUSY_MESSAGE,
            )
        })
}

/// Restore the automatic checkpoint created immediately before the last
/// successful Studio turn. This is local-only and consumes the checkpoint, so
/// the button represents one honest undo rather than an unbounded history claim.
pub async fn undo_studio_turn(
    Json(request): Json<UndoTurnRequest>,
) -> (StatusCode, Json<TurnOutcome>) {
    crate::bounded::run_blocking(move || undo_studio_turn_blocking(request))
        .await
        .unwrap_or_else(|_| {
            turn_error(
                StatusCode::TOO_MANY_REQUESTS,
                crate::bounded::LONG_OPERATION_BUSY_MESSAGE,
            )
        })
}

fn undo_studio_turn_blocking(request: UndoTurnRequest) -> (StatusCode, Json<TurnOutcome>) {
    let id = match canonical_workspace_id(&request.app_id) {
        Ok(id) => id,
        Err(_) => {
            return turn_error(
                StatusCode::BAD_REQUEST,
                "That Build Studio app identifier is not valid.",
            )
        }
    };
    // Undo is a local recovery action, not a new AI execution. It must remain
    // available even when Private mode or app-building policy now blocks turns.
    let app_lock = studio_app_lock(&id);
    let _app_guard = app_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if recover_interrupted_studio_transaction(&app_builder::apps_dir(), &id).is_err() {
        return turn_error(
            StatusCode::CONFLICT,
            "An interrupted Studio recovery could not be completed safely.",
        );
    }
    let restored = match restore_workspace_checkpoint(&app_builder::apps_dir(), &id) {
        Ok(restored) => restored,
        Err(_) => {
            return turn_error(
                StatusCode::CONFLICT,
                "No safe Studio checkpoint is available for that build.",
            )
        }
    };
    audit_ai_action("build-app", Some("studio"), AiActionOutcome::Succeeded);
    (
        StatusCode::OK,
        Json(TurnOutcome {
            ok: true,
            text: "Undid the last Studio turn and restored its previous files.".to_string(),
            session: restored.map(hydrate),
        }),
    )
}

fn studio_turn_blocking(
    request: TurnRequest,
    client: Option<crate::control_plane::RequestClient>,
) -> (StatusCode, Json<TurnOutcome>) {
    studio_turn_blocking_with_policy(
        request,
        crate::policy::policy_state_for_control("app-builder"),
        client,
    )
}

fn studio_turn_blocking_with_policy(
    request: TurnRequest,
    policy: crate::policy::PolicyControlState,
    client: Option<crate::control_plane::RequestClient>,
) -> (StatusCode, Json<TurnOutcome>) {
    let message = request.message.trim();
    if message.is_empty() || message.chars().count() > MAX_MESSAGE_CHARS {
        return turn_error(
            StatusCode::BAD_REQUEST,
            "Describe what you want to build in 1 to 4000 characters.",
        );
    }

    // Studio is an app-building surface, so it shares the exact same policy
    // guard as the one-shot builder. This must remain before session creation,
    // engine execution, and workspace writes.
    if let Err(detail) = app_builder::authorize_app_builder_for_state(policy, "studio") {
        return turn_error(StatusCode::FORBIDDEN, detail);
    }
    audit_ai_action("build-app", Some("studio"), AiActionOutcome::Started);

    // Continuing a build always requires its explicit saved id. A request without
    // one comes from the intentional New Build state and therefore receives a
    // fresh identity even when its description matches an existing project.
    let requested_id = match &request.app_id {
        Some(existing) if !existing.trim().is_empty() => existing.trim().to_string(),
        _ => fresh_studio_session_id(message),
    };
    let id = match canonical_workspace_id(&requested_id) {
        Ok(id) => id,
        Err(_) => {
            return turn_error(
                StatusCode::BAD_REQUEST,
                "That Build Studio app identifier is not valid.",
            )
        }
    };

    // Hosted review, file mutations, session save, and Undo all serialize on
    // the same app identity. The core retains this exact snapshot while the
    // trusted broker is visible, so concurrent requests cannot change what an
    // approval authorizes.
    let app_lock = studio_app_lock(&id);
    let _app_guard = app_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if recover_interrupted_studio_transaction(&app_builder::apps_dir(), &id).is_err() {
        audit_ai_action("build-app", Some("studio"), AiActionOutcome::Failed);
        return turn_error(
            StatusCode::CONFLICT,
            "An interrupted Studio transaction could not be recovered safely.",
        );
    }

    let previous_session = load_session(&id);
    let mut session = previous_session.clone().unwrap_or_else(|| StoredSession {
        id: id.clone(),
        name: app_builder::derive_app_name(message),
        engine: String::new(),
        created_at: now_secs(),
        updated_at: now_secs(),
        thread: Vec::new(),
    });

    let workspace_review = workspace_manifest_at(&app_builder::apps_dir(), &id);
    let reviewed_workspace = workspace_review.as_ref().ok();
    let empty_workspace_review = WorkspaceReview {
        files: Vec::new(),
        total_bytes: 0,
        content_digest: Sha256::digest(b"goblins-studio-workspace-v1\0").into(),
    };
    let workspace_for_chat = reviewed_workspace.unwrap_or(&empty_workspace_review);
    let session_digest = session_snapshot_digest(previous_session.as_ref());

    session.thread.push(Message {
        role: "you".to_string(),
        text: message.to_string(),
        at: now_secs(),
    });
    let apps_root = app_builder::apps_dir();
    let resident_context_disclosure = studio_chat_context_disclosure(message, &session);
    let codex_context_disclosure =
        studio_codex_context_disclosure(message, &session, workspace_for_chat);
    let resident_prompt = model_prompt(&session, message);
    let codex_request = codex_prompt(&session, message);
    let codex_review_content =
        studio_codex_review_content(&codex_request, &session, workspace_for_chat, session_digest);
    let request_binding = serde_json::to_vec(&(
        message,
        id.as_str(),
        session_digest,
        workspace_for_chat.content_digest,
    ))
    .unwrap_or_default();
    let mut codex_checkpoint_active = false;
    let mut codex_staged_workspace = None;
    let execution = crate::resident::resident_execute_studio_context(
        crate::resident::StudioContextRequest {
            client,
            action_id: "studio.turn",
            request_binding: &request_binding,
            resident_prompt: &resident_prompt,
            codex_review_content: &codex_review_content,
            resident_context_disclosure: &resident_context_disclosure,
            codex_context_disclosure: &codex_context_disclosure,
        },
        || {
            if workspace_review.is_err() {
                return Err("The existing Studio workspace cannot be reviewed safely for Codex. Remove links, special files, or oversized files before continuing.".to_string());
            }
            let checkpoint =
                create_workspace_checkpoint(&apps_root, &id, previous_session.as_ref())?;
            codex_checkpoint_active = true;
            let staged = stage_workspace_from_checkpoint(&apps_root, &checkpoint)?;
            let result =
                crate::codex::run_codex_in(&staged.directory, &staged.path, &codex_request);
            if result.is_ok() {
                codex_staged_workspace = Some(staged);
            }
            result
        },
    );

    let (raw, engine) = match execution {
        Ok(result) => result,
        Err(crate::resident::StudioContextExecutionError::Cancelled) => {
            audit_ai_action("build-app", Some("studio"), AiActionOutcome::Denied);
            return turn_error(
                StatusCode::FORBIDDEN,
                "Nothing was shared. The hosted Studio turn was cancelled.",
            );
        }
        Err(crate::resident::StudioContextExecutionError::TimedOut) => {
            audit_ai_action("build-app", Some("studio"), AiActionOutcome::Blocked);
            return turn_error(
                StatusCode::REQUEST_TIMEOUT,
                "Nothing was shared because the hosted Studio review expired.",
            );
        }
        Err(crate::resident::StudioContextExecutionError::Unavailable(detail)) => {
            audit_ai_action("build-app", Some("studio"), AiActionOutcome::Blocked);
            let recovery = if codex_checkpoint_active {
                " The automatic checkpoint is still available through Undo Last Turn."
            } else {
                ""
            };
            return turn_error(
                StatusCode::SERVICE_UNAVAILABLE,
                &format!("The selected engine could not build that: {detail}.{recovery}"),
            );
        }
    };
    session.engine = engine.to_string();

    let (agent_text, staged_workspace) = if engine == "codex" {
        let Some(staged) = codex_staged_workspace else {
            audit_ai_action("build-app", Some("studio"), AiActionOutcome::Failed);
            return turn_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Codex completed without a safely confined Studio staging workspace.",
            );
        };
        (raw, staged)
    } else {
        let checkpoint =
            match create_workspace_checkpoint(&apps_root, &id, previous_session.as_ref()) {
                Ok(checkpoint) => checkpoint,
                Err(detail) => {
                    audit_ai_action("build-app", Some("studio"), AiActionOutcome::Failed);
                    return turn_error(StatusCode::CONFLICT, &detail);
                }
            };
        let staged = match stage_workspace_from_checkpoint(&apps_root, &checkpoint) {
            Ok(staged) => staged,
            Err(detail) => {
                audit_ai_action("build-app", Some("studio"), AiActionOutcome::Failed);
                return turn_error(StatusCode::CONFLICT, &detail);
            }
        };
        let (prose, files) = parse_emitted_files(&raw);
        if write_emitted_files(&staged.directory, &files).is_err() {
            audit_ai_action("build-app", Some("studio"), AiActionOutcome::Failed);
            return turn_error(
                StatusCode::CONFLICT,
                "The engine returned an unsafe workspace change. The live app was left unchanged and its automatic checkpoint remains available.",
            );
        }
        let agent_text = if prose.is_empty() {
            format!(
                "Updated {} file{}.",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            )
        } else {
            prose
        };
        (agent_text, staged)
    };

    session.thread.push(Message {
        role: "agent".to_string(),
        text: agent_text,
        at: now_secs(),
    });
    session.updated_at = now_secs();

    if let Err(detail) =
        commit_staged_state(&apps_root, &id, Some(&staged_workspace), Some(&session))
    {
        audit_ai_action("build-app", Some("studio"), AiActionOutcome::Failed);
        return turn_error(StatusCode::INTERNAL_SERVER_ERROR, &detail);
    }

    audit_ai_action("build-app", Some("studio"), AiActionOutcome::Succeeded);

    (
        StatusCode::OK,
        Json(TurnOutcome {
            ok: true,
            text: "Built.".to_string(),
            session: Some(hydrate(session)),
        }),
    )
}

fn codex_prompt(session: &StoredSession, message: &str) -> String {
    format!(
        "You are building an app inside Goblins OS for the user. Work in the current directory. \
         The app is \"{}\". Create or edit the files needed and keep them small and focused. \
         When done, reply with a short summary of what you built or changed.\n\nRequest: {message}",
        session.name
    )
}

fn model_prompt(session: &StoredSession, message: &str) -> String {
    format!(
        "You are the Build Studio engine for Goblins OS. Build a small, focused app named \"{}\" \
         for the user's request. Output the project files, each in a block exactly like:\n\
         ===FILE: relative/path.ext===\n<file contents>\n===END===\n\
         Use relative paths only (no leading slash, no ..). After the file blocks, add one short \
         line summarizing what you built. Keep files minimal and runnable.\n\nRequest: {message}",
        session.name
    )
}

fn studio_chat_context_disclosure(message: &str, session: &StoredSession) -> String {
    format!(
        "the exact Studio request ({} characters) and app name '{}'; existing workspace contents, the session id, and prior conversation messages are not added automatically, but any sensitive text in the reviewed request itself is included",
        message.chars().count(), session.name
    )
}

fn studio_codex_context_disclosure(
    message: &str,
    session: &StoredSession,
    workspace: &WorkspaceReview,
) -> String {
    format!(
        "the exact Studio request ({} characters), app name '{}', session id '{}', and the contents of all {} reviewed workspace files ({} bytes); prior conversation messages are not added automatically, other user files and OS private data or credential stores are unavailable, and only this reviewed workspace plus non-private system runtime files needed to run Codex are mounted; sensitive content inside the reviewed request or workspace is included",
        message.chars().count(),
        session.name,
        session.id,
        workspace.files.len(),
        workspace.total_bytes
    )
}

fn studio_codex_review_content(
    codex_request: &str,
    session: &StoredSession,
    workspace: &WorkspaceReview,
    session_digest: [u8; 32],
) -> String {
    let file_manifest = if workspace.files.is_empty() {
        "No existing workspace files.".to_string()
    } else {
        workspace
            .files
            .iter()
            .map(|file| format!("- {} ({} bytes)", file.path, file.size_bytes))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let workspace_digest = hex_digest(workspace.content_digest);
    let session_digest = hex_digest(session_digest);
    format!(
        "App: {}\nSession: {}\nSession snapshot SHA-256: {session_digest}\nWorkspace snapshot SHA-256: {workspace_digest}\nWorkspace bytes: {}\nReviewed workspace access (Codex can read the full contents of every listed file after approval):\n{file_manifest}\n\nExact Codex instruction:\n{codex_request}",
        session.name, session.id, workspace.total_bytes
    )
}

fn hex_digest(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Parse the file-emit format a chat model returns into (summary prose, files).
/// Anything outside a `===FILE:` / `===END===` block is treated as the summary.
fn parse_emitted_files(raw: &str) -> (String, Vec<(String, String)>) {
    let mut files = Vec::new();
    let mut prose = String::new();
    let mut lines = raw.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(path) = line.trim().strip_prefix("===FILE:") {
            let path = path.trim().trim_end_matches('=').trim().to_string();
            let mut content = String::new();
            for body in lines.by_ref() {
                if body.trim() == "===END===" {
                    break;
                }
                content.push_str(body);
                content.push('\n');
            }
            if !path.is_empty() {
                files.push((path, content));
            }
        } else {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    (prose.trim().to_string(), files)
}

fn write_emitted_files(workspace: &Dir, files: &[(String, String)]) -> io::Result<()> {
    for (path, content) in files {
        let relative = relative_file_path(path)?;
        let (parent, name) = open_relative_parent(workspace, &relative, true)?;
        let bytes = content.as_bytes();
        write_workspace_file(&parent, &name, &bytes[..bytes.len().min(MAX_FILE_BYTES)])?;
    }
    Ok(())
}

fn hydrate(session: StoredSession) -> StudioSession {
    let apps_root = app_builder::apps_dir();
    let files = list_workspace_files_at(&apps_root, &session.id);
    let undo_available = workspace_checkpoint_exists(&apps_root, &session.id);
    StudioSession {
        id: session.id,
        name: session.name,
        engine: session.engine,
        created_at: session.created_at,
        updated_at: session.updated_at,
        thread: session.thread,
        files,
        undo_available,
    }
}

fn list_summaries() -> Vec<SessionSummary> {
    let dir = sessions_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<SessionSummary> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<StoredSession>(&bytes).ok())
        .map(|session| SessionSummary {
            id: session.id,
            name: session.name,
            engine: session.engine,
            updated_at: session.updated_at,
            turns: session.thread.len(),
        })
        .collect();
    // Most recently updated first.
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

fn load_session(id: &str) -> Option<StoredSession> {
    let bytes = fs::read(session_path(id)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
fn write_session_at(apps_root: &Path, session: &StoredSession) -> std::io::Result<()> {
    let dir = sessions_dir_at(apps_root);
    fs::create_dir_all(&dir)?;
    let path = session_path_at(apps_root, &session.id);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(session)?)?;
    fs::rename(tmp, path)
}

fn workspace_manifest_at(apps_root: &Path, id: &str) -> io::Result<WorkspaceReview> {
    let workspace = match open_workspace_at(apps_root, id, false) {
        Ok(workspace) => workspace,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(WorkspaceReview {
                files: Vec::new(),
                total_bytes: 0,
                content_digest: Sha256::digest(b"goblins-studio-workspace-v1\0").into(),
            })
        }
        Err(error) => return Err(error),
    };
    let mut review = WorkspaceReview {
        files: Vec::new(),
        total_bytes: 0,
        content_digest: [0; 32],
    };
    collect_workspace_manifest(&workspace, Path::new(""), 0, &mut review)?;
    let mut digest = Sha256::new();
    digest.update(b"goblins-studio-workspace-v1\0");
    for file in &review.files {
        let relative = relative_file_path(&file.path)?;
        let content = read_workspace_file_from(&workspace, &relative)?;
        if content.len() as u64 != file.size_bytes {
            return Err(invalid_workspace_path());
        }
        digest.update((file.path.len() as u64).to_le_bytes());
        digest.update(file.path.as_bytes());
        digest.update(file.size_bytes.to_le_bytes());
        digest.update(content);
    }
    review.content_digest = digest.finalize().into();
    Ok(review)
}

fn collect_workspace_manifest(
    dir: &Dir,
    prefix: &Path,
    depth: usize,
    review: &mut WorkspaceReview,
) -> io::Result<()> {
    if depth > MAX_WORKSPACE_DEPTH {
        return Err(invalid_workspace_path());
    }
    for entry in dir.entries()? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let relative = prefix.join(&name);
        if relative.to_str().is_none() || file_type.is_symlink() {
            return Err(invalid_workspace_path());
        }
        if file_type.is_dir() {
            let child = dir.open_dir_nofollow(&name)?;
            collect_workspace_manifest(&child, &relative, depth + 1, review)?;
        } else if file_type.is_file() {
            let metadata = entry.metadata()?;
            if metadata.nlink() != 1
                || metadata.len() > MAX_FILE_BYTES as u64
                || review.files.len() >= MAX_WORKSPACE_FILES
            {
                return Err(invalid_workspace_path());
            }
            review.total_bytes = review
                .total_bytes
                .checked_add(metadata.len())
                .filter(|total| *total <= MAX_CHECKPOINT_BYTES as u64)
                .ok_or_else(invalid_workspace_path)?;
            review.files.push(StudioWorkspaceFile {
                path: relative
                    .to_str()
                    .expect("workspace path was validated as UTF-8")
                    .to_string(),
                size_bytes: metadata.len(),
            });
        } else {
            return Err(invalid_workspace_path());
        }
    }
    review
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(())
}

fn project_review_at(apps_root: &Path, session: &StoredSession) -> io::Result<StudioProjectReview> {
    let manifest = workspace_manifest_at(apps_root, &session.id)?;
    let file_paths = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let runtime = crate::studio_runtime::runtime_status(&file_paths);
    let containerization = crate::studio_container::container_status(&file_paths);
    let changes = workspace_changes_at(apps_root, &session.id)?;
    let workspace_sha256 = hex_digest(manifest.content_digest);
    let last_run = load_run_record_at(apps_root, &session.id)
        .ok()
        .flatten()
        .map(|mut run| {
            run.workspace_current = !run.workspace_sha256.is_empty()
                && run.workspace_sha256 == workspace_sha256;
            if !run.workspace_current {
                run.detail = format!(
                    "Saved logs from an earlier project version. Run this version again for current output. {}",
                    run.detail
                );
            }
            run
        });
    Ok(StudioProjectReview {
        ok: true,
        id: session.id.clone(),
        name: session.name.clone(),
        file_count: manifest.files.len(),
        total_bytes: manifest.total_bytes,
        workspace_sha256,
        files: changes,
        runtime: StudioRuntimeCapability {
            available: runtime.available,
            kind: runtime.kind.to_string(),
            entrypoint: runtime.entrypoint,
            detail: runtime.detail,
        },
        export: StudioCapability {
            available: !manifest.files.is_empty(),
            detail: if manifest.files.is_empty() {
                "Add at least one project file before exporting.".to_string()
            } else {
                "Saves a deterministic tar archive to Downloads with its SHA-256 digest."
                    .to_string()
            },
        },
        containerization: StudioCapability {
            available: containerization.available,
            detail: containerization.detail,
        },
        last_run,
    })
}

fn project_error(status: StatusCode, id: String) -> (StatusCode, Json<StudioProjectReview>) {
    (
        status,
        Json(StudioProjectReview {
            ok: false,
            id,
            name: String::new(),
            files: Vec::new(),
            file_count: 0,
            total_bytes: 0,
            workspace_sha256: String::new(),
            runtime: StudioRuntimeCapability {
                available: false,
                kind: "unavailable".to_string(),
                entrypoint: None,
                detail: "The Studio project could not be reviewed safely.".to_string(),
            },
            export: StudioCapability {
                available: false,
                detail: "Export is unavailable until the project can be reviewed safely."
                    .to_string(),
            },
            containerization: StudioCapability {
                available: false,
                detail: "Container packaging is unavailable in this image.".to_string(),
            },
            last_run: None,
        }),
    )
}

fn run_error(status: StatusCode, text: &str) -> (StatusCode, Json<StudioRunOutcome>) {
    (
        status,
        Json(StudioRunOutcome {
            ok: false,
            text: text.to_string(),
            run: None,
        }),
    )
}

fn export_error(status: StatusCode, text: &str) -> (StatusCode, Json<StudioExportOutcome>) {
    (
        status,
        Json(StudioExportOutcome {
            ok: false,
            text: text.to_string(),
            path: None,
            sha256: None,
        }),
    )
}

fn container_error(status: StatusCode, text: &str) -> (StatusCode, Json<StudioContainerOutcome>) {
    (
        status,
        Json(StudioContainerOutcome {
            ok: false,
            text: text.to_string(),
            path: None,
            sha256: None,
            image_ref: None,
        }),
    )
}

fn workspace_changes_at(apps_root: &Path, id: &str) -> io::Result<Vec<StudioFileChange>> {
    let workspace = open_workspace_at(apps_root, id, false).ok();
    let manifest = workspace_manifest_at(apps_root, id)?;
    let checkpoint = load_workspace_checkpoint_at(apps_root, id)?;
    let mut current = BTreeMap::new();
    for file in &manifest.files {
        current.insert(file.path.clone(), file.size_bytes);
    }
    let Some(checkpoint) = checkpoint else {
        return Ok(manifest
            .files
            .into_iter()
            .map(|file| StudioFileChange {
                path: file.path,
                size_bytes: file.size_bytes,
                change: "current".to_string(),
            })
            .collect());
    };
    let previous = checkpoint
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.content.as_slice()))
        .collect::<BTreeMap<_, _>>();
    let mut paths = current.keys().cloned().collect::<BTreeSet<_>>();
    paths.extend(checkpoint.files.iter().map(|file| file.path.clone()));
    let mut changes = Vec::new();
    for path in paths {
        let current_bytes = match (&workspace, current.get(&path)) {
            (Some(workspace), Some(_)) => Some(read_workspace_file_from(
                workspace,
                &relative_file_path(&path)?,
            )?),
            _ => None,
        };
        let previous_bytes = previous.get(path.as_str()).copied();
        let change = match (current_bytes.as_deref(), previous_bytes) {
            (Some(current), Some(previous)) if current == previous => "unchanged",
            (Some(_), Some(_)) => "modified",
            (Some(_), None) => "added",
            (None, Some(_)) => "deleted",
            (None, None) => continue,
        };
        changes.push(StudioFileChange {
            path: path.clone(),
            size_bytes: current.get(&path).copied().unwrap_or(0),
            change: change.to_string(),
        });
    }
    Ok(changes)
}

fn workspace_file_view_at(apps_root: &Path, id: &str, path: &str) -> io::Result<FileView> {
    let current = match read_workspace_file_at(apps_root, id, path) {
        Ok((bytes, truncated)) => Some((bytes, truncated)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let checkpoint = load_workspace_checkpoint_at(apps_root, id)?;
    let previous = checkpoint.as_ref().and_then(|checkpoint| {
        checkpoint
            .files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.content.as_slice())
    });
    if current.is_none() && previous.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Studio file is missing",
        ));
    }
    let current_bytes = current.as_ref().map(|(bytes, _)| bytes.as_slice());
    let change = match (current_bytes, previous) {
        (Some(current), Some(previous)) if current == previous => "unchanged",
        (Some(_), Some(_)) => "modified",
        (Some(_), None) if checkpoint.is_some() => "added",
        (Some(_), None) => "current",
        (None, Some(_)) => "deleted",
        (None, None) => "unavailable",
    };
    let binary = current_bytes
        .or(previous)
        .is_some_and(|bytes| std::str::from_utf8(bytes).is_err());
    let content = if binary {
        String::new()
    } else {
        current_bytes
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or("")
            .to_string()
    };
    let (diff, diff_truncated) = if binary || checkpoint.is_none() || change == "unchanged" {
        (String::new(), false)
    } else {
        bounded_file_diff(
            path,
            previous.unwrap_or_default(),
            current_bytes.unwrap_or_default(),
        )
    };
    Ok(FileView {
        path: path.to_string(),
        content,
        truncated: current.as_ref().is_some_and(|(_, truncated)| *truncated),
        binary,
        change: change.to_string(),
        diff,
        diff_truncated,
        previous_available: previous.is_some(),
    })
}

fn bounded_file_diff(path: &str, previous: &[u8], current: &[u8]) -> (String, bool) {
    let previous = String::from_utf8_lossy(previous);
    let current = String::from_utf8_lossy(current);
    let old_lines = previous.lines().collect::<Vec<_>>();
    let new_lines = current.lines().collect::<Vec<_>>();
    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old_lines.len().saturating_sub(prefix)
        && suffix < new_lines.len().saturating_sub(prefix)
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let context_start = prefix.saturating_sub(3);
    let old_end = old_lines.len().saturating_sub(suffix);
    let new_end = new_lines.len().saturating_sub(suffix);
    let context_suffix = suffix.min(3);
    let mut diff = format!("--- a/{path}\n+++ b/{path}\n");
    for line in &old_lines[context_start..prefix] {
        push_diff_line(&mut diff, ' ', line);
    }
    for line in &old_lines[prefix..old_end] {
        push_diff_line(&mut diff, '-', line);
        if diff.len() > MAX_DIFF_BYTES {
            truncate_utf8(&mut diff, MAX_DIFF_BYTES);
            return (diff, true);
        }
    }
    for line in &new_lines[prefix..new_end] {
        push_diff_line(&mut diff, '+', line);
        if diff.len() > MAX_DIFF_BYTES {
            truncate_utf8(&mut diff, MAX_DIFF_BYTES);
            return (diff, true);
        }
    }
    for line in &new_lines[new_end..new_end.saturating_add(context_suffix)] {
        push_diff_line(&mut diff, ' ', line);
    }
    if diff.len() > MAX_DIFF_BYTES {
        truncate_utf8(&mut diff, MAX_DIFF_BYTES);
        (diff, true)
    } else {
        (diff, false)
    }
}

fn truncate_utf8(text: &mut String, max_bytes: usize) {
    let mut boundary = max_bytes.min(text.len());
    while !text.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    text.truncate(boundary);
}

fn push_diff_line(diff: &mut String, marker: char, line: &str) {
    diff.push(marker);
    diff.push_str(line);
    diff.push('\n');
}

fn deterministic_export_at(apps_root: &Path, id: &str) -> Result<Vec<u8>, String> {
    let manifest = workspace_manifest_at(apps_root, id)
        .map_err(|_| "The Studio workspace could not be reviewed safely for export.".to_string())?;
    if manifest.files.is_empty() {
        return Err("Add at least one project file before exporting.".to_string());
    }
    let workspace = open_workspace_at(apps_root, id, false)
        .map_err(|_| "The Studio workspace could not be opened safely for export.".to_string())?;
    let output = Vec::new();
    let mut archive = tar::Builder::new(output);
    archive.mode(tar::HeaderMode::Deterministic);
    for file in &manifest.files {
        let content = read_workspace_file_from(
            &workspace,
            &relative_file_path(&file.path)
                .map_err(|_| "The export contains an unsafe path.".to_string())?,
        )
        .map_err(|_| "A Studio project file could not be read safely for export.".to_string())?;
        let archive_path = format!("{id}/{}", file.path);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        archive
            .append_data(&mut header, archive_path, Cursor::new(content))
            .map_err(|_| "The deterministic Studio archive could not be assembled.".to_string())?;
    }
    archive
        .finish()
        .map_err(|_| "The deterministic Studio archive could not be finalized.".to_string())?;
    let output = archive
        .into_inner()
        .map_err(|_| "The deterministic Studio archive could not be finalized.".to_string())?;
    if output.len() > MAX_EXPORT_ARCHIVE_BYTES {
        return Err(
            "The Studio project is too large for the bounded desktop export path.".to_string(),
        );
    }
    Ok(output)
}

fn workspace_snapshot_at(
    apps_root: &Path,
    id: &str,
    manifest: &WorkspaceReview,
) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let workspace = open_workspace_at(apps_root, id, false)?;
    let mut snapshot = BTreeMap::new();
    for file in &manifest.files {
        let path = relative_file_path(&file.path)?;
        let content = read_workspace_file_from(&workspace, &path)?;
        if content.len() as u64 != file.size_bytes {
            return Err(invalid_workspace_path());
        }
        snapshot.insert(file.path.clone(), content);
    }
    Ok(snapshot)
}

fn create_workspace_checkpoint(
    apps_root: &Path,
    id: &str,
    previous_session: Option<&StoredSession>,
) -> Result<WorkspaceCheckpoint, String> {
    let mut checkpoint = WorkspaceCheckpoint {
        had_workspace: false,
        previous_session: previous_session.cloned(),
        directories: Vec::new(),
        files: Vec::new(),
    };
    match open_workspace_at(apps_root, id, false) {
        Ok(workspace) => {
            checkpoint.had_workspace = true;
            let mut total_bytes = 0_u64;
            collect_checkpoint_files(
                &workspace,
                Path::new(""),
                0,
                &mut checkpoint,
                &mut total_bytes,
            )
            .map_err(|_| {
                "The Studio workspace could not be checkpointed safely. Remove links, special files, or oversized files before continuing."
                    .to_string()
            })?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err("The Studio workspace could not be checkpointed safely.".to_string()),
    }

    let encoded = serde_json::to_vec(&checkpoint)
        .map_err(|_| "The Studio checkpoint could not be encoded.".to_string())?;
    if encoded.len() > MAX_CHECKPOINT_FILE_BYTES {
        return Err(
            "The Studio workspace is too large for a safe one-turn checkpoint.".to_string(),
        );
    }
    let undo = open_undo_dir_at(apps_root, true)
        .map_err(|_| "The Studio checkpoint store is not ready.".to_string())?;
    write_workspace_file(&undo, &checkpoint_name(id), &encoded)
        .map_err(|_| "The Studio checkpoint could not be saved.".to_string())?;
    Ok(checkpoint)
}

fn collect_checkpoint_files(
    dir: &Dir,
    prefix: &Path,
    depth: usize,
    checkpoint: &mut WorkspaceCheckpoint,
    total_bytes: &mut u64,
) -> io::Result<()> {
    if depth > MAX_WORKSPACE_DEPTH {
        return Err(invalid_workspace_path());
    }
    for entry in dir.entries()? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let relative = prefix.join(&name);
        let relative_text = relative.to_str().ok_or_else(invalid_workspace_path)?;
        if file_type.is_symlink() {
            return Err(invalid_workspace_path());
        }
        if file_type.is_dir() {
            checkpoint.directories.push(relative_text.to_string());
            let child = dir.open_dir_nofollow(&name)?;
            collect_checkpoint_files(&child, &relative, depth + 1, checkpoint, total_bytes)?;
        } else if file_type.is_file() {
            if checkpoint.files.len() >= MAX_WORKSPACE_FILES {
                return Err(invalid_workspace_path());
            }
            let content = read_checkpoint_file(dir, &name)?;
            *total_bytes = total_bytes
                .checked_add(content.len() as u64)
                .filter(|total| *total <= MAX_CHECKPOINT_BYTES as u64)
                .ok_or_else(invalid_workspace_path)?;
            checkpoint.files.push(CheckpointFile {
                path: relative_text.to_string(),
                content,
            });
        } else {
            return Err(invalid_workspace_path());
        }
    }
    Ok(())
}

fn read_checkpoint_file(parent: &Dir, name: &OsStr) -> io::Result<Vec<u8>> {
    let entry_metadata = parent.symlink_metadata(name)?;
    if !entry_metadata.is_file()
        || entry_metadata.nlink() != 1
        || entry_metadata.len() > MAX_FILE_BYTES as u64
    {
        return Err(invalid_workspace_path());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent.open_with(name, &options)?;
    let opened_metadata = file.metadata()?;
    if !opened_metadata.is_file()
        || opened_metadata.nlink() != 1
        || opened_metadata.len() != entry_metadata.len()
    {
        return Err(invalid_workspace_path());
    }
    let mut content = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut content)?;
    if content.len() as u64 != opened_metadata.len() {
        return Err(invalid_workspace_path());
    }
    Ok(content)
}

fn stage_workspace_from_checkpoint(
    apps_root: &Path,
    checkpoint: &WorkspaceCheckpoint,
) -> Result<StagedWorkspace, String> {
    let root = open_workspace_root_at(apps_root, true)
        .map_err(|_| "The Studio staging root could not be opened safely.".to_string())?;
    let name = create_unique_directory(&root, ".goblins-studio-stage")
        .map_err(|_| "A private Studio staging workspace could not be created.".to_string())?;
    let directory = root
        .open_dir_nofollow(&name)
        .map_err(|_| "The Studio staging workspace could not be opened safely.".to_string())?;
    let mut staged = StagedWorkspace {
        path: apps_root.join("workspace").join(&name),
        root,
        directory,
        name,
    };
    if checkpoint.had_workspace {
        materialize_checkpoint(&staged.directory, checkpoint).map_err(|_| {
            "The Studio workspace could not be copied into private staging.".to_string()
        })?;
    }
    // Re-open after all initial files are materialized so the handle passed to
    // Codex is the exact directory that the sandbox path validator checks.
    staged.directory = staged
        .root
        .open_dir_nofollow(&staged.name)
        .map_err(|_| "The Studio staging workspace changed unexpectedly.".to_string())?;
    sync_workspace_tree(&staged.directory)
        .and_then(|()| sync_directory(&staged.root))
        .map_err(|_| "The Studio staging workspace could not be made durable.".to_string())?;
    Ok(staged)
}

fn materialize_checkpoint(workspace: &Dir, checkpoint: &WorkspaceCheckpoint) -> io::Result<()> {
    for directory in &checkpoint.directories {
        open_relative_dir(workspace, Path::new(directory), true)?;
    }
    for file in &checkpoint.files {
        let relative = relative_file_path(&file.path)?;
        let (parent, name) = open_relative_parent(workspace, &relative, true)?;
        write_workspace_file(&parent, &name, &file.content)?;
    }
    Ok(())
}

fn create_unique_directory(parent: &Dir, prefix: &str) -> io::Result<OsString> {
    for _ in 0..8 {
        let name = OsString::from(format!(
            "{prefix}-{:016x}{:016x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ));
        match parent.create_dir(&name) {
            Ok(()) => return Ok(name),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a private Studio directory",
    ))
}

fn unique_absent_name(parent: &Dir, prefix: &str, suffix: &str) -> io::Result<OsString> {
    for _ in 0..8 {
        let name = OsString::from(format!(
            "{prefix}-{:016x}{:016x}{suffix}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ));
        match parent.symlink_metadata(&name) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(name),
            Ok(_) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a private Studio transaction name",
    ))
}

fn remove_entry(parent: &Dir, name: &OsStr) -> io::Result<()> {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.is_dir() => parent.remove_dir_all(name),
        Ok(_) => parent.remove_file(name),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn entry_exists(parent: &Dir, name: &OsStr) -> io::Result<bool> {
    match parent.symlink_metadata(name) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn commit_staged_state(
    apps_root: &Path,
    id: &str,
    staged_workspace: Option<&StagedWorkspace>,
    desired_session: Option<&StoredSession>,
) -> Result<(), String> {
    recover_interrupted_studio_transaction(apps_root, id)?;
    let workspace_root = open_workspace_root_at(apps_root, true)
        .map_err(|_| "The Studio workspace root could not be opened safely.".to_string())?;
    let sessions = open_sessions_dir_at(apps_root, true)
        .map_err(|_| "The Studio session store could not be opened safely.".to_string())?;
    let workspace_backup = unique_absent_name(&workspace_root, ".goblins-workspace-backup", "")
        .map_err(|_| "The Studio workspace transaction could not start.".to_string())?;
    let session_backup = unique_absent_name(&sessions, ".goblins-session-backup", ".json")
        .map_err(|_| "The Studio session transaction could not start.".to_string())?;
    let session_stage = if let Some(session) = desired_session {
        let name = unique_absent_name(&sessions, ".goblins-session-stage", ".json")
            .map_err(|_| "The Studio session could not be staged.".to_string())?;
        let encoded = serde_json::to_vec_pretty(session)
            .map_err(|_| "The Studio session could not be encoded.".to_string())?;
        write_workspace_file(&sessions, &name, &encoded)
            .map_err(|_| "The Studio session could not be staged.".to_string())?;
        sync_directory(&sessions)
            .map_err(|_| "The Studio session stage could not be made durable.".to_string())?;
        Some(name)
    } else {
        None
    };
    let session_name = checkpoint_name(id);
    let workspace_had_current = entry_exists(&workspace_root, OsStr::new(id))
        .map_err(|_| "The current Studio workspace could not be inspected.".to_string())?;
    let session_had_current = entry_exists(&sessions, &session_name)
        .map_err(|_| "The current Studio session could not be inspected.".to_string())?;
    let journal = StudioTransactionJournal {
        version: 1,
        app_id: id.to_string(),
        workspace_stage: staged_workspace
            .and_then(|staged| staged.name.to_str())
            .map(str::to_string),
        workspace_backup: workspace_backup
            .to_str()
            .ok_or_else(|| "The Studio transaction name was not valid text.".to_string())?
            .to_string(),
        workspace_had_current,
        session_stage: session_stage
            .as_ref()
            .and_then(|name| name.to_str())
            .map(str::to_string),
        session_backup: session_backup
            .to_str()
            .ok_or_else(|| "The Studio transaction name was not valid text.".to_string())?
            .to_string(),
        session_had_current,
    };
    if let Err(detail) = write_studio_transaction_journal(apps_root, &journal) {
        if let Some(staged) = session_stage.as_ref() {
            let _ = remove_entry(&sessions, staged);
            let _ = sync_directory(&sessions);
        }
        return Err(detail);
    }

    let commit = (|| -> io::Result<()> {
        match workspace_root.symlink_metadata(id) {
            Ok(_) => {
                workspace_root.rename(id, &workspace_root, &workspace_backup)?;
                sync_directory(&workspace_root)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if let Some(staged) = staged_workspace {
            staged
                .root
                .rename(&staged.name, &workspace_root, OsStr::new(id))?;
            sync_directory(&workspace_root)?;
        }

        match sessions.symlink_metadata(&session_name) {
            Ok(_) => {
                sessions.rename(&session_name, &sessions, &session_backup)?;
                sync_directory(&sessions)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if let Some(staged) = session_stage.as_ref() {
            sessions.rename(staged, &sessions, &session_name)?;
            sync_directory(&sessions)?;
        }
        Ok(())
    })();

    if commit.is_err() {
        let _ = recover_interrupted_studio_transaction(apps_root, id);
        return Err(
            "The Studio transaction could not be committed; the previous app was preserved."
                .to_string(),
        );
    }

    remove_studio_transaction_journal(apps_root, id)?;
    if workspace_had_current {
        let _ = remove_entry(&workspace_root, &workspace_backup);
    }
    if session_had_current {
        let _ = remove_entry(&sessions, &session_backup);
    }
    let _ = sync_directory(&workspace_root);
    let _ = sync_directory(&sessions);
    Ok(())
}

fn recover_interrupted_studio_transaction(apps_root: &Path, id: &str) -> Result<(), String> {
    let transactions = match open_transactions_dir_at(apps_root, false) {
        Ok(transactions) => transactions,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("The Studio transaction store could not be opened.".to_string()),
    };
    let journal_name = checkpoint_name(id);
    let encoded = match read_bounded_regular_file(&transactions, &journal_name, 16 * 1024) {
        Ok(encoded) => encoded,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("The interrupted Studio transaction was not safe.".to_string()),
    };
    let journal: StudioTransactionJournal = serde_json::from_slice(&encoded)
        .map_err(|_| "The interrupted Studio transaction was not valid.".to_string())?;
    validate_studio_transaction_journal(id, &journal)
        .map_err(|_| "The interrupted Studio transaction failed validation.".to_string())?;

    let workspace_root = open_workspace_root_at(apps_root, true)
        .map_err(|_| "The Studio workspace root could not be recovered.".to_string())?;
    let sessions = open_sessions_dir_at(apps_root, true)
        .map_err(|_| "The Studio session store could not be recovered.".to_string())?;
    let workspace_backup = OsStr::new(&journal.workspace_backup);
    let session_backup = OsStr::new(&journal.session_backup);
    let session_name = checkpoint_name(id);

    if journal.workspace_had_current {
        if entry_exists(&workspace_root, workspace_backup)
            .map_err(|_| "The Studio workspace backup could not be inspected.".to_string())?
        {
            remove_entry(&workspace_root, OsStr::new(id)).map_err(|_| {
                "The interrupted Studio workspace could not be rolled back.".to_string()
            })?;
            workspace_root
                .rename(workspace_backup, &workspace_root, OsStr::new(id))
                .map_err(|_| "The prior Studio workspace could not be restored.".to_string())?;
        }
    } else {
        remove_entry(&workspace_root, OsStr::new(id)).map_err(|_| {
            "The interrupted new Studio workspace could not be removed.".to_string()
        })?;
    }
    if let Some(stage) = journal.workspace_stage.as_deref() {
        remove_entry(&workspace_root, OsStr::new(stage)).map_err(|_| {
            "The interrupted Studio staging workspace could not be removed.".to_string()
        })?;
    }

    if journal.session_had_current {
        if entry_exists(&sessions, session_backup)
            .map_err(|_| "The Studio session backup could not be inspected.".to_string())?
        {
            remove_entry(&sessions, &session_name).map_err(|_| {
                "The interrupted Studio session could not be rolled back.".to_string()
            })?;
            sessions
                .rename(session_backup, &sessions, &session_name)
                .map_err(|_| "The prior Studio session could not be restored.".to_string())?;
        }
    } else {
        remove_entry(&sessions, &session_name)
            .map_err(|_| "The interrupted new Studio session could not be removed.".to_string())?;
    }
    if let Some(stage) = journal.session_stage.as_deref() {
        remove_entry(&sessions, OsStr::new(stage)).map_err(|_| {
            "The interrupted Studio session stage could not be removed.".to_string()
        })?;
    }
    sync_directory(&workspace_root)
        .and_then(|()| sync_directory(&sessions))
        .map_err(|_| "The recovered Studio state could not be made durable.".to_string())?;
    remove_studio_transaction_journal(apps_root, id)
}

fn validate_studio_transaction_journal(
    id: &str,
    journal: &StudioTransactionJournal,
) -> io::Result<()> {
    if journal.version != 1 || journal.app_id != id {
        return Err(invalid_workspace_path());
    }
    let valid_name = |name: &str, prefix: &str, suffix: &str| {
        !name.is_empty()
            && name.starts_with(prefix)
            && name.ends_with(suffix)
            && Path::new(name).components().count() == 1
            && matches!(
                Path::new(name).components().next(),
                Some(Component::Normal(_))
            )
    };
    if !valid_name(&journal.workspace_backup, ".goblins-workspace-backup-", "")
        || !valid_name(&journal.session_backup, ".goblins-session-backup-", ".json")
        || journal
            .workspace_stage
            .as_deref()
            .is_some_and(|name| !valid_name(name, ".goblins-studio-stage-", ""))
        || journal
            .session_stage
            .as_deref()
            .is_some_and(|name| !valid_name(name, ".goblins-session-stage-", ".json"))
    {
        return Err(invalid_workspace_path());
    }
    Ok(())
}

fn write_studio_transaction_journal(
    apps_root: &Path,
    journal: &StudioTransactionJournal,
) -> Result<(), String> {
    let transactions = open_transactions_dir_at(apps_root, true)
        .map_err(|_| "The Studio transaction journal could not be opened.".to_string())?;
    let encoded = serde_json::to_vec(journal)
        .map_err(|_| "The Studio transaction journal could not be encoded.".to_string())?;
    write_workspace_file(&transactions, &checkpoint_name(&journal.app_id), &encoded)
        .and_then(|()| sync_directory(&transactions))
        .map_err(|_| "The Studio transaction journal could not be made durable.".to_string())
}

fn remove_studio_transaction_journal(apps_root: &Path, id: &str) -> Result<(), String> {
    let transactions = open_transactions_dir_at(apps_root, false)
        .map_err(|_| "The Studio transaction journal could not be reopened.".to_string())?;
    match transactions.remove_file(checkpoint_name(id)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err("The Studio transaction journal could not be closed.".to_string()),
    }
    sync_directory(&transactions)
        .map_err(|_| "The Studio transaction journal close was not durable.".to_string())
}

fn sync_directory(directory: &Dir) -> io::Result<()> {
    // On Linux, cap-std deliberately represents `Dir` with an `O_PATH`
    // descriptor. Duplicating that descriptor and calling fsync fails with
    // EBADF, even though the held capability is valid. Re-open `.` through the
    // capability as an ordinary read-only directory descriptor so the
    // directory-entry transaction can be durably committed without falling
    // back to an ambient path.
    directory.open(Path::new("."))?.into_std().sync_all()
}

fn sync_workspace_tree(directory: &Dir) -> io::Result<()> {
    for entry in directory.entries()? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(invalid_workspace_path());
        }
        if file_type.is_dir() {
            let child = directory.open_dir_nofollow(entry.file_name())?;
            sync_workspace_tree(&child)?;
        } else if !file_type.is_file() {
            return Err(invalid_workspace_path());
        }
    }
    sync_directory(directory)
}

fn restore_workspace_checkpoint(
    apps_root: &Path,
    id: &str,
) -> Result<Option<StoredSession>, String> {
    let undo = open_undo_dir_at(apps_root, false)
        .map_err(|_| "The Studio checkpoint store is not available.".to_string())?;
    let name = checkpoint_name(id);
    let checkpoint = load_workspace_checkpoint_at(apps_root, id)
        .map_err(|_| "The Studio checkpoint could not be verified.".to_string())?
        .ok_or_else(|| "No safe Studio checkpoint is available for that build.".to_string())?;

    let staged = if checkpoint.had_workspace {
        Some(stage_workspace_from_checkpoint(apps_root, &checkpoint)?)
    } else {
        None
    };
    commit_staged_state(
        apps_root,
        id,
        staged.as_ref(),
        checkpoint.previous_session.as_ref(),
    )?;
    undo.remove_file(&name)
        .map_err(|_| "The used Studio checkpoint could not be closed.".to_string())?;
    Ok(checkpoint.previous_session)
}

fn load_workspace_checkpoint_at(
    apps_root: &Path,
    id: &str,
) -> io::Result<Option<WorkspaceCheckpoint>> {
    let undo = match open_undo_dir_at(apps_root, false) {
        Ok(undo) => undo,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let encoded =
        match read_bounded_regular_file(&undo, &checkpoint_name(id), MAX_CHECKPOINT_FILE_BYTES) {
            Ok(encoded) => encoded,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
    let checkpoint: WorkspaceCheckpoint =
        serde_json::from_slice(&encoded).map_err(|_| invalid_workspace_path())?;
    validate_workspace_checkpoint(id, &checkpoint)?;
    Ok(Some(checkpoint))
}

fn validate_workspace_checkpoint(id: &str, checkpoint: &WorkspaceCheckpoint) -> io::Result<()> {
    if !checkpoint.had_workspace
        && (!checkpoint.directories.is_empty() || !checkpoint.files.is_empty())
    {
        return Err(invalid_workspace_path());
    }
    if checkpoint
        .previous_session
        .as_ref()
        .is_some_and(|session| session.id != id)
        || checkpoint.files.len() > MAX_WORKSPACE_FILES
        || checkpoint.directories.len() > MAX_WORKSPACE_FILES
    {
        return Err(invalid_workspace_path());
    }

    let mut directories = BTreeSet::new();
    for directory in &checkpoint.directories {
        let path = relative_file_path(directory)?;
        if !directories.insert(path) {
            return Err(invalid_workspace_path());
        }
    }
    let mut files = BTreeSet::new();
    let mut total_bytes = 0_usize;
    for file in &checkpoint.files {
        let path = relative_file_path(&file.path)?;
        if file.content.len() > MAX_FILE_BYTES
            || !files.insert(path.clone())
            || directories.contains(&path)
        {
            return Err(invalid_workspace_path());
        }
        total_bytes = total_bytes
            .checked_add(file.content.len())
            .filter(|total| *total <= MAX_CHECKPOINT_BYTES)
            .ok_or_else(invalid_workspace_path)?;
        let mut ancestor = path.parent();
        while let Some(parent) = ancestor {
            if parent.as_os_str().is_empty() {
                break;
            }
            if files.contains(parent) {
                return Err(invalid_workspace_path());
            }
            ancestor = parent.parent();
        }
    }
    for directory in &directories {
        if files.contains(directory) {
            return Err(invalid_workspace_path());
        }
        let mut ancestor = directory.parent();
        while let Some(parent) = ancestor {
            if parent.as_os_str().is_empty() {
                break;
            }
            if files.contains(parent) {
                return Err(invalid_workspace_path());
            }
            ancestor = parent.parent();
        }
    }
    Ok(())
}

fn write_run_record_at(apps_root: &Path, id: &str, run: &StudioRunRecord) -> io::Result<()> {
    canonical_workspace_id(id)?;
    let runs = open_studio_runs_dir_at(apps_root, true)?;
    let bytes = serde_json::to_vec(run).map_err(io::Error::other)?;
    if bytes.len() > 192 * 1024 {
        return Err(invalid_workspace_path());
    }
    write_workspace_file(&runs, &checkpoint_name(id), &bytes)?;
    sync_directory(&runs)
}

fn load_run_record_at(apps_root: &Path, id: &str) -> io::Result<Option<StudioRunRecord>> {
    canonical_workspace_id(id)?;
    let runs = match open_studio_runs_dir_at(apps_root, false) {
        Ok(runs) => runs,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let bytes = match read_bounded_regular_file(&runs, &checkpoint_name(id), 192 * 1024) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let run = serde_json::from_slice(&bytes).map_err(|_| invalid_workspace_path())?;
    Ok(Some(run))
}

fn read_bounded_regular_file(parent: &Dir, name: &OsStr, max_bytes: usize) -> io::Result<Vec<u8>> {
    let metadata = parent.symlink_metadata(name)?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > max_bytes as u64 {
        return Err(invalid_workspace_path());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent.open_with(name, &options)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.nlink() != 1 || opened.len() != metadata.len() {
        return Err(invalid_workspace_path());
    }
    let mut bytes = Vec::new();
    file.take((max_bytes + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(invalid_workspace_path());
    }
    Ok(bytes)
}

fn workspace_checkpoint_exists(apps_root: &Path, id: &str) -> bool {
    open_undo_dir_at(apps_root, false).is_ok_and(|undo| {
        undo.symlink_metadata(checkpoint_name(id))
            .is_ok_and(|metadata| {
                metadata.is_file()
                    && metadata.nlink() == 1
                    && metadata.len() <= MAX_CHECKPOINT_FILE_BYTES as u64
            })
    })
}

fn checkpoint_name(id: &str) -> OsString {
    OsString::from(format!("{id}.json"))
}

fn list_workspace_files_at(apps_root: &Path, id: &str) -> Vec<String> {
    let mut files = Vec::new();
    let Ok(workspace) = open_workspace_at(apps_root, id, false) else {
        return files;
    };
    collect_files(&workspace, Path::new(""), 0, &mut files);
    files.sort();
    files
}

fn collect_files(dir: &Dir, prefix: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_WORKSPACE_DEPTH || out.len() >= MAX_WORKSPACE_FILES {
        return;
    }
    let Ok(entries) = dir.entries() else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_WORKSPACE_FILES {
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Model output is untrusted. Never follow symlinks or enumerate special
        // files: a Codex-created link must not turn the core into a credential
        // reader outside the workspace capability.
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let relative = prefix.join(&name);
        if file_type.is_dir() {
            if let Ok(child) = dir.open_dir_nofollow(&name) {
                collect_files(&child, &relative, depth + 1, out);
            }
        } else if file_type.is_file()
            && entry.metadata().is_ok_and(|metadata| metadata.nlink() == 1)
        {
            out.push(relative.to_string_lossy().into_owned());
        }
    }
}

fn sessions_dir() -> PathBuf {
    sessions_dir_at(&app_builder::apps_dir())
}

fn sessions_dir_at(apps_root: &Path) -> PathBuf {
    apps_root.join("sessions")
}

fn session_path(id: &str) -> PathBuf {
    session_path_at(&app_builder::apps_dir(), id)
}

fn session_path_at(apps_root: &Path, id: &str) -> PathBuf {
    sessions_dir_at(apps_root).join(format!("{}.json", sanitize_id(id)))
}

fn invalid_workspace_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid Build Studio workspace path",
    )
}

fn canonical_workspace_id(id: &str) -> io::Result<String> {
    let trimmed = id.trim();
    let canonical = sanitize_id(trimmed);
    if canonical.is_empty() || canonical != trimmed {
        return Err(invalid_workspace_path());
    }
    Ok(canonical)
}

fn relative_file_path(path: &str) -> io::Result<PathBuf> {
    let candidate = Path::new(path);
    let mut components = 0;
    for component in candidate.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(invalid_workspace_path());
        }
        components += 1;
        if components > MAX_WORKSPACE_DEPTH + 1 {
            return Err(invalid_workspace_path());
        }
    }
    if components == 0 {
        return Err(invalid_workspace_path());
    }
    Ok(candidate.to_path_buf())
}

fn open_apps_root_at(apps_root: &Path, create: bool) -> io::Result<Dir> {
    if create {
        fs::create_dir_all(apps_root)?;
    }
    Dir::open_ambient_dir(apps_root, ambient_authority())
}

fn open_workspace_root_at(apps_root: &Path, create: bool) -> io::Result<Dir> {
    let apps = open_apps_root_at(apps_root, create)?;
    open_or_create_dir_nofollow(&apps, OsStr::new("workspace"), create)
}

fn open_undo_dir_at(apps_root: &Path, create: bool) -> io::Result<Dir> {
    let apps = open_apps_root_at(apps_root, create)?;
    open_or_create_dir_nofollow(&apps, OsStr::new("studio-undo"), create)
}

fn open_sessions_dir_at(apps_root: &Path, create: bool) -> io::Result<Dir> {
    let apps = open_apps_root_at(apps_root, create)?;
    open_or_create_dir_nofollow(&apps, OsStr::new("sessions"), create)
}

fn open_transactions_dir_at(apps_root: &Path, create: bool) -> io::Result<Dir> {
    let apps = open_apps_root_at(apps_root, create)?;
    open_or_create_dir_nofollow(&apps, OsStr::new("studio-transactions"), create)
}

fn open_studio_runs_dir_at(apps_root: &Path, create: bool) -> io::Result<Dir> {
    let apps = open_apps_root_at(apps_root, create)?;
    open_or_create_dir_nofollow(&apps, OsStr::new("studio-runs"), create)
}

fn open_or_create_dir_nofollow(parent: &Dir, name: &OsStr, create: bool) -> io::Result<Dir> {
    match parent.open_dir_nofollow(name) {
        Ok(dir) => Ok(dir),
        Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            parent.open_dir_nofollow(name)
        }
        Err(error) => Err(error),
    }
}

/// Open an app workspace through directory capabilities. Every untrusted path
/// component is opened without following symlinks, so the resulting handle
/// cannot be redirected outside `apps_root/workspace`.
fn open_workspace_at(apps_root: &Path, id: &str, create: bool) -> io::Result<Dir> {
    let id = canonical_workspace_id(id)?;
    let workspace_root = open_workspace_root_at(apps_root, create)?;
    open_or_create_dir_nofollow(&workspace_root, OsStr::new(&id), create)
}

fn open_relative_dir(workspace: &Dir, relative: &Path, create: bool) -> io::Result<Dir> {
    let mut dir = workspace.try_clone()?;
    let mut components = 0;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid_workspace_path());
        };
        components += 1;
        if components > MAX_WORKSPACE_DEPTH {
            return Err(invalid_workspace_path());
        }
        dir = open_or_create_dir_nofollow(&dir, name, create)?;
    }
    if components == 0 {
        return Err(invalid_workspace_path());
    }
    Ok(dir)
}

fn open_relative_parent(
    workspace: &Dir,
    relative: &Path,
    create: bool,
) -> io::Result<(Dir, OsString)> {
    let components = relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => Ok(name.to_os_string()),
            _ => Err(invalid_workspace_path()),
        })
        .collect::<io::Result<Vec<_>>>()?;
    if components.is_empty() || components.len() > MAX_WORKSPACE_DEPTH + 1 {
        return Err(invalid_workspace_path());
    }

    let (name, parents) = components.split_last().ok_or_else(invalid_workspace_path)?;
    let mut parent = workspace.try_clone()?;
    for component in parents {
        parent = open_or_create_dir_nofollow(&parent, component, create)?;
    }
    Ok((parent, name.clone()))
}

fn read_workspace_file_at(apps_root: &Path, id: &str, path: &str) -> io::Result<(Vec<u8>, bool)> {
    let relative = relative_file_path(path)?;
    let workspace = open_workspace_at(apps_root, id, false)?;
    let (parent, name) = open_relative_parent(&workspace, &relative, false)?;
    let metadata = parent.symlink_metadata(&name)?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Studio can only read regular, non-linked workspace files",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent.open_with(&name, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Studio can only read regular, non-linked workspace files",
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > MAX_FILE_BYTES;
    bytes.truncate(MAX_FILE_BYTES);
    Ok((bytes, truncated))
}

fn read_workspace_file_from(workspace: &Dir, relative: &Path) -> io::Result<Vec<u8>> {
    let (parent, name) = open_relative_parent(workspace, relative, false)?;
    let metadata = parent.symlink_metadata(&name)?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Studio can only read regular, non-linked workspace files",
        ));
    }

    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent.open_with(&name, &options)?;
    let opened = file.metadata()?;
    if !opened.is_file()
        || opened.nlink() != 1
        || opened.len() != metadata.len()
        || opened.len() > MAX_FILE_BYTES as u64
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Studio can only read regular, non-linked workspace files",
        ));
    }

    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES || bytes.len() as u64 != opened.len() {
        return Err(invalid_workspace_path());
    }
    Ok(bytes)
}

/// Commit a model-emitted file through an adjacent, freshly created temporary
/// file. Renaming replaces a malicious final symlink or hard link as a directory
/// entry; it never follows the link and therefore cannot overwrite its target.
fn write_workspace_file(parent: &Dir, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
    let mut temporary = None;
    let mut file = None;
    for _ in 0..8 {
        let candidate = OsString::from(format!(
            ".goblins-studio-{:016x}.tmp",
            rand::random::<u64>()
        ));
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        match parent.open_with(&candidate, &options) {
            Ok(created) => {
                temporary = Some(candidate);
                file = Some(created);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    let temporary = temporary.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a safe Studio output file",
        )
    })?;
    let mut file = file.expect("a temporary name is stored with its open file");
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = parent.remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if let Err(error) = parent.rename(&temporary, parent, name) {
        let _ = parent.remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .take(96)
        .collect()
}

fn fresh_studio_session_id(message: &str) -> String {
    studio_session_id_with_nonce(message, rand::random(), rand::random())
}

fn studio_session_id_with_nonce(message: &str, nonce_a: u64, nonce_b: u64) -> String {
    app_builder::app_id(&format!(
        "{message}\n[goblins-studio-session:{nonce_a:016x}{nonce_b:016x}]"
    ))
}

fn now_secs() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs().to_string())
        .unwrap_or_default()
}

fn turn_error(status: StatusCode, text: &str) -> (StatusCode, Json<TurnOutcome>) {
    (
        status,
        Json(TurnOutcome {
            ok: false,
            text: text.to_string(),
            session: None,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_file_diff, canonical_workspace_id, create_workspace_checkpoint,
        deterministic_export_at, list_workspace_files_at, open_sessions_dir_at, open_workspace_at,
        open_workspace_root_at, parse_emitted_files, read_workspace_file_at,
        recover_all_studio_transactions_at, relative_file_path, restore_workspace_checkpoint,
        sanitize_id, session_snapshot_digest, stage_workspace_from_checkpoint,
        studio_chat_context_disclosure, studio_codex_context_disclosure,
        studio_codex_review_content, studio_session_id_with_nonce,
        studio_turn_blocking_with_policy, sync_directory, unique_absent_name, workspace_changes_at,
        workspace_checkpoint_exists, workspace_file_view_at, workspace_manifest_at,
        write_emitted_files, write_session_at, write_studio_transaction_journal,
        write_workspace_file, Message, StoredSession, StudioTransactionJournal, TurnRequest,
    };
    use crate::policy::PolicyControlState;
    use axum::http::StatusCode;
    use std::{ffi::OsStr, fs};

    #[test]
    fn parses_emitted_files_and_summary() {
        let raw = "Here is your app.\n\
                   ===FILE: src/main.py===\n\
                   print(\"hi\")\n\
                   ===END===\n\
                   ===FILE: README.md===\n\
                   # App\n\
                   ===END===\n\
                   Built a tiny app.";
        let (prose, files) = parse_emitted_files(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "src/main.py");
        assert!(files[0].1.contains("print(\"hi\")"));
        assert_eq!(files[1].0, "README.md");
        assert!(prose.contains("Here is your app."));
        assert!(prose.contains("Built a tiny app."));
    }

    #[test]
    fn relative_file_paths_block_escapes() {
        assert!(relative_file_path("src/main.py").is_ok());
        assert!(relative_file_path("../../etc/passwd").is_err());
        assert!(relative_file_path("/etc/passwd").is_err());
        assert!(relative_file_path("./main.py").is_err());
        assert!(relative_file_path("").is_err());
    }

    #[test]
    fn new_build_ids_are_fresh_while_explicit_ids_can_still_resume() {
        let first = studio_session_id_with_nonce("Build a notes app", 1, 2);
        let second = studio_session_id_with_nonce("Build a notes app", 3, 4);
        assert_ne!(first, second);
        assert!(canonical_workspace_id(&first).is_ok());
        assert!(canonical_workspace_id(&second).is_ok());
        assert!(first.starts_with("build-a-notes-app-"));
    }

    #[test]
    fn review_marks_added_modified_and_deleted_files_from_the_real_checkpoint() {
        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let id = "review-changes";
        let workspace = apps.join("workspace").join(id);
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(workspace.join("modified.txt"), b"before\n").expect("modified fixture");
        fs::write(workspace.join("deleted.txt"), b"removed\n").expect("deleted fixture");
        create_workspace_checkpoint(&apps, id, None).expect("checkpoint");
        fs::write(workspace.join("modified.txt"), b"after\n").expect("modify");
        fs::remove_file(workspace.join("deleted.txt")).expect("delete");
        fs::write(workspace.join("added.txt"), b"new\n").expect("add");

        let changes = workspace_changes_at(&apps, id).expect("changes");
        assert_eq!(
            changes
                .iter()
                .map(|change| (change.path.as_str(), change.change.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("added.txt", "added"),
                ("deleted.txt", "deleted"),
                ("modified.txt", "modified"),
            ]
        );
        let modified = workspace_file_view_at(&apps, id, "modified.txt").expect("file view");
        assert!(modified.diff.contains("-before"));
        assert!(modified.diff.contains("+after"));
        let deleted = workspace_file_view_at(&apps, id, "deleted.txt").expect("deleted view");
        assert_eq!(deleted.change, "deleted");
        assert!(deleted.previous_available);
        assert!(deleted.diff.contains("-removed"));
    }

    #[test]
    fn deterministic_export_is_sorted_normalized_and_repeatable() {
        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let id = "export-app";
        let workspace = apps.join("workspace").join(id);
        fs::create_dir_all(workspace.join("src")).expect("workspace");
        fs::write(workspace.join("z.txt"), b"last").expect("z");
        fs::write(workspace.join("src/a.txt"), b"first").expect("a");

        let first = deterministic_export_at(&apps, id).expect("first archive");
        let second = deterministic_export_at(&apps, id).expect("second archive");
        assert_eq!(first, second);

        let mut archive = tar::Archive::new(std::io::Cursor::new(first));
        let entries = archive
            .entries()
            .expect("archive entries")
            .map(|entry| {
                let entry = entry.expect("entry");
                (
                    entry.path().expect("path").to_string_lossy().into_owned(),
                    entry.header().mtime().expect("mtime"),
                    entry.header().uid().expect("uid"),
                    entry.header().mode().expect("mode"),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.0.as_str())
                .collect::<Vec<_>>(),
            vec!["export-app/src/a.txt", "export-app/z.txt"]
        );
        assert!(entries
            .iter()
            .all(|(_, mtime, uid, mode)| *mtime == 0 && *uid == 0 && *mode == 0o644));
    }

    #[test]
    fn bounded_diff_truncates_unicode_on_a_character_boundary() {
        let current = "é".repeat(80_000);
        let (diff, truncated) = bounded_file_diff("unicode.txt", b"", current.as_bytes());
        assert!(truncated);
        assert!(diff.len() <= super::MAX_DIFF_BYTES);
        assert!(std::str::from_utf8(diff.as_bytes()).is_ok());
    }

    #[test]
    fn sanitize_id_strips_path_characters() {
        assert_eq!(sanitize_id("a-notes-app-1a2b3c4d"), "a-notes-app-1a2b3c4d");
        assert_eq!(sanitize_id("../../evil/id"), "evilid");
        assert!(canonical_workspace_id("a-notes-app-1a2b3c4d").is_ok());
        assert!(canonical_workspace_id("../../evil/id").is_err());
        assert!(canonical_workspace_id("").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn workspace_links_cannot_read_list_or_overwrite_external_secrets() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let workspace_path = apps.join("workspace").join("safe-app");
        let outside = root.path().join("outside");
        let secret = outside.join("secret.json");
        fs::create_dir_all(&workspace_path).expect("workspace");
        fs::create_dir_all(&outside).expect("outside directory");
        fs::write(&secret, b"OS-OWNED-SECRET").expect("secret");
        fs::write(workspace_path.join("main.txt"), b"safe").expect("regular workspace file");
        symlink(&secret, workspace_path.join("leak.txt")).expect("final-component symlink");
        symlink(&outside, workspace_path.join("escape")).expect("directory symlink");

        assert!(read_workspace_file_at(&apps, "safe-app", "leak.txt").is_err());
        assert!(read_workspace_file_at(&apps, "safe-app", "escape/secret.json").is_err());
        assert_eq!(
            list_workspace_files_at(&apps, "safe-app"),
            vec!["main.txt".to_string()]
        );

        let workspace = open_workspace_at(&apps, "safe-app", false).expect("workspace handle");
        write_emitted_files(
            &workspace,
            &[("leak.txt".to_string(), "safe replacement".to_string())],
        )
        .expect("a final symlink is safely replaced, not followed");
        assert_eq!(
            fs::read(&secret).expect("secret remains"),
            b"OS-OWNED-SECRET"
        );
        assert_eq!(
            fs::read(workspace_path.join("leak.txt")).expect("replacement remains internal"),
            b"safe replacement"
        );

        assert!(write_emitted_files(
            &workspace,
            &[(
                "escape/secret.json".to_string(),
                "attempted overwrite".to_string()
            )],
        )
        .is_err());
        assert_eq!(
            fs::read(&secret).expect("secret remains"),
            b"OS-OWNED-SECRET"
        );
    }

    #[cfg(unix)]
    #[test]
    fn workspace_root_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let outside = root.path().join("outside");
        fs::create_dir_all(apps.join("workspace")).expect("workspace root");
        fs::create_dir_all(&outside).expect("outside directory");
        symlink(&outside, apps.join("workspace").join("linked-app")).expect("workspace symlink");

        assert!(open_workspace_at(&apps, "linked-app", false).is_err());
    }

    #[test]
    fn studio_checkpoint_restores_previous_files_and_conversation_once() {
        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let id = "checkpoint-app";
        let workspace_path = apps.join("workspace").join(id);
        fs::create_dir_all(workspace_path.join("src")).expect("workspace");
        fs::write(workspace_path.join("src/main.txt"), b"before").expect("original file");
        fs::write(workspace_path.join("keep.txt"), b"keep").expect("kept file");
        let previous = StoredSession {
            id: id.to_string(),
            name: "Checkpoint App".to_string(),
            engine: "local-gpt-oss".to_string(),
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
            thread: vec![Message {
                role: "you".to_string(),
                text: "before".to_string(),
                at: "1".to_string(),
            }],
        };
        write_session_at(&apps, &previous).expect("prior session");
        create_workspace_checkpoint(&apps, id, Some(&previous)).expect("checkpoint");
        assert!(workspace_checkpoint_exists(&apps, id));

        fs::write(workspace_path.join("src/main.txt"), b"after").expect("changed file");
        fs::remove_file(workspace_path.join("keep.txt")).expect("deleted file");
        fs::write(workspace_path.join("new.txt"), b"new").expect("new file");
        let mut changed = previous.clone();
        changed.updated_at = "2".to_string();
        changed.thread.push(Message {
            role: "agent".to_string(),
            text: "after".to_string(),
            at: "2".to_string(),
        });
        write_session_at(&apps, &changed).expect("changed session");

        let restored = restore_workspace_checkpoint(&apps, id)
            .expect("restore")
            .expect("prior session");
        assert_eq!(restored.thread.len(), 1);
        assert_eq!(
            fs::read(workspace_path.join("src/main.txt")).unwrap(),
            b"before"
        );
        assert_eq!(fs::read(workspace_path.join("keep.txt")).unwrap(), b"keep");
        assert!(!workspace_path.join("new.txt").exists());
        assert!(!workspace_checkpoint_exists(&apps, id));
        assert!(restore_workspace_checkpoint(&apps, id).is_err());
    }

    #[test]
    fn directory_sync_reopens_linux_capability_handles() {
        let root = tempfile::tempdir().expect("temporary root");
        let directory =
            cap_std::fs::Dir::open_ambient_dir(root.path(), cap_std::ambient_authority())
                .expect("capability directory");

        sync_directory(&directory).expect("durable directory");
    }

    #[test]
    fn invalid_checkpoint_never_clears_the_live_workspace() {
        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let id = "transaction-app";
        let workspace = apps.join("workspace").join(id);
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(workspace.join("main.txt"), b"before").expect("workspace file");
        create_workspace_checkpoint(&apps, id, None).expect("checkpoint");
        fs::write(workspace.join("main.txt"), b"live-after").expect("live change");

        let checkpoint_path = apps.join("studio-undo").join(format!("{id}.json"));
        let mut checkpoint: serde_json::Value =
            serde_json::from_slice(&fs::read(&checkpoint_path).expect("checkpoint bytes"))
                .expect("checkpoint JSON");
        checkpoint["files"][0]["path"] = serde_json::Value::String("../escape".to_string());
        fs::write(
            &checkpoint_path,
            serde_json::to_vec(&checkpoint).expect("corrupt checkpoint JSON"),
        )
        .expect("corrupt checkpoint");

        assert!(restore_workspace_checkpoint(&apps, id).is_err());
        assert_eq!(
            fs::read(workspace.join("main.txt")).expect("live workspace remains"),
            b"live-after"
        );
    }

    #[test]
    fn durable_journal_recovers_crash_between_workspace_and_session_swap() {
        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let id = "crash-recovery-app";
        let workspace_path = apps.join("workspace").join(id);
        fs::create_dir_all(&workspace_path).expect("workspace");
        fs::write(workspace_path.join("main.txt"), b"before").expect("prior workspace");
        let previous = StoredSession {
            id: id.to_string(),
            name: "Crash Recovery".to_string(),
            engine: "local-gpt-oss".to_string(),
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
            thread: Vec::new(),
        };
        write_session_at(&apps, &previous).expect("prior session");
        let checkpoint =
            create_workspace_checkpoint(&apps, id, Some(&previous)).expect("checkpoint");
        let staged = stage_workspace_from_checkpoint(&apps, &checkpoint).expect("workspace stage");
        write_workspace_file(&staged.directory, OsStr::new("main.txt"), b"after")
            .expect("staged workspace change");

        let workspace_root = open_workspace_root_at(&apps, true).expect("workspace root");
        let sessions = open_sessions_dir_at(&apps, true).expect("sessions root");
        let workspace_backup = unique_absent_name(&workspace_root, ".goblins-workspace-backup", "")
            .expect("workspace backup name");
        let session_backup = unique_absent_name(&sessions, ".goblins-session-backup", ".json")
            .expect("session backup name");
        let session_stage = unique_absent_name(&sessions, ".goblins-session-stage", ".json")
            .expect("session stage name");
        let mut changed = previous.clone();
        changed.updated_at = "2".to_string();
        write_workspace_file(
            &sessions,
            &session_stage,
            &serde_json::to_vec(&changed).expect("changed session"),
        )
        .expect("session stage");
        sync_directory(&sessions).expect("durable session stage");
        let journal = StudioTransactionJournal {
            version: 1,
            app_id: id.to_string(),
            workspace_stage: Some(staged.name.to_str().expect("stage name").to_string()),
            workspace_backup: workspace_backup.to_str().expect("backup name").to_string(),
            workspace_had_current: true,
            session_stage: Some(
                session_stage
                    .to_str()
                    .expect("session stage name")
                    .to_string(),
            ),
            session_backup: session_backup
                .to_str()
                .expect("session backup name")
                .to_string(),
            session_had_current: true,
        };
        write_studio_transaction_journal(&apps, &journal).expect("durable journal");

        workspace_root
            .rename(id, &workspace_root, &workspace_backup)
            .expect("backup workspace");
        sync_directory(&workspace_root).expect("durable workspace backup");
        staged
            .root
            .rename(&staged.name, &workspace_root, OsStr::new(id))
            .expect("install staged workspace");
        sync_directory(&workspace_root).expect("durable staged install");
        assert_eq!(fs::read(workspace_path.join("main.txt")).unwrap(), b"after");

        // Simulate a power loss before the session rename. Startup/read recovery
        // must roll the workspace back instead of exposing mixed generations.
        recover_all_studio_transactions_at(&apps).expect("startup journal recovery");
        assert_eq!(
            fs::read(workspace_path.join("main.txt")).unwrap(),
            b"before"
        );
        let recovered_session: StoredSession = serde_json::from_slice(
            &fs::read(apps.join("sessions").join(format!("{id}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(recovered_session.updated_at, "1");
        assert!(!apps
            .join("studio-transactions")
            .join(format!("{id}.json"))
            .exists());
    }

    #[test]
    fn studio_hosted_review_is_exact_about_codex_workspace_access() {
        let root = tempfile::tempdir().expect("temporary root");
        let apps = root.path().join("apps");
        let id = "review-app";
        let workspace_path = apps.join("workspace").join(id);
        fs::create_dir_all(&workspace_path).expect("workspace");
        fs::write(workspace_path.join("main.txt"), b"private text").expect("file");
        let review = workspace_manifest_at(&apps, id).expect("manifest");
        let session = StoredSession {
            id: id.to_string(),
            name: "Review App".to_string(),
            engine: String::new(),
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
            thread: Vec::new(),
        };
        let chat_disclosure = studio_chat_context_disclosure("Change the title", &session);
        assert!(chat_disclosure.contains("exact Studio request (16 characters)"));
        assert!(chat_disclosure.contains("existing workspace contents"));
        assert!(chat_disclosure.contains("not added automatically"));
        let codex_disclosure =
            studio_codex_context_disclosure("Change the title", &session, &review);
        assert!(codex_disclosure.contains("exact Studio request (16 characters)"));
        assert!(codex_disclosure.contains("1 reviewed workspace files (12 bytes)"));

        let session_digest = session_snapshot_digest(Some(&session));
        let review_content =
            studio_codex_review_content("Exact Codex request", &session, &review, session_digest);
        assert!(review_content.contains("App: Review App"));
        assert!(review_content.contains("Session: review-app"));
        assert!(review_content.contains("main.txt (12 bytes)"));
        assert!(review_content.contains("Workspace snapshot SHA-256:"));
        assert!(review_content.contains("Session snapshot SHA-256:"));
        assert!(review_content.contains(
            "Reviewed workspace access (Codex can read the full contents of every listed file after approval)"
        ));
        assert!(review_content.contains("Exact Codex instruction:\nExact Codex request"));
    }

    #[test]
    fn denied_and_permission_gated_turns_stop_before_a_session_exists() {
        for policy in [
            PolicyControlState::Denied,
            PolicyControlState::PermissionGated,
        ] {
            let (status, outcome) = studio_turn_blocking_with_policy(
                TurnRequest {
                    app_id: Some("must-not-be-created".to_string()),
                    message: "Build a private notes app".to_string(),
                },
                policy,
                None,
            );
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(!outcome.ok);
            assert!(outcome.session.is_none());
        }
    }

    #[test]
    fn undo_is_local_recovery_not_a_new_policy_gated_ai_action() {
        let source = include_str!("studio.rs");
        let undo = source
            .split_once("fn undo_studio_turn_blocking(")
            .expect("undo implementation")
            .1
            .split_once("fn studio_turn_blocking(")
            .expect("undo implementation boundary")
            .0;
        assert!(!undo.contains("authorize_app_builder"));
        assert!(undo.contains("recover_interrupted_studio_transaction"));
        assert!(undo.contains("restore_workspace_checkpoint"));
    }
}
