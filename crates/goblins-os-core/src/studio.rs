//! The Build Studio: a minimal, multi-turn agent build session over whichever
//! engine is active — local GPT-OSS, the user's OpenAI account via Codex, or a
//! bring-your-own key. The user describes what to build; the engine answers and
//! produces files in the app's workspace; the conversation and the files are
//! persisted so the Studio is a real, resumable surface. It is the single place
//! Goblins OS turns intent into software, and it looks the same whichever brain
//! runs — Codex builds files directly, while a chat model builds through a small
//! file-emit step. No pre-installed apps; everything here was built on request.

use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{extract::Query, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::app_builder;

const MAX_MESSAGE_CHARS: usize = 4000;
const MAX_FILE_BYTES: usize = 256 * 1024;

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
}

pub async fn studio_sessions() -> Json<SessionList> {
    let sessions = list_summaries();
    Json(SessionList {
        count: sessions.len(),
        sessions,
    })
}

pub async fn studio_session(Query(query): Query<SessionQuery>) -> (StatusCode, Json<TurnOutcome>) {
    match load_session(&query.app_id) {
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
    };
    let Some(safe) = safe_workspace_path(&query.app_id, &query.path) else {
        return (StatusCode::BAD_REQUEST, Json(empty()));
    };
    match fs::read(&safe) {
        Ok(bytes) => {
            let truncated = bytes.len() > MAX_FILE_BYTES;
            let slice = &bytes[..bytes.len().min(MAX_FILE_BYTES)];
            (
                StatusCode::OK,
                Json(FileView {
                    path: query.path,
                    content: String::from_utf8_lossy(slice).into_owned(),
                    truncated,
                }),
            )
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(empty())),
    }
}

/// A Studio turn is a real agent run — `codex exec` under its 600s bound or a
/// model turn through the resident relay (120s+ read timeout) — so the body
/// runs on the blocking pool instead of pinning an async runtime worker.
pub async fn studio_turn(Json(request): Json<TurnRequest>) -> (StatusCode, Json<TurnOutcome>) {
    crate::bounded::run_blocking(move || studio_turn_blocking(request)).await
}

fn studio_turn_blocking(request: TurnRequest) -> (StatusCode, Json<TurnOutcome>) {
    let message = request.message.trim();
    if message.is_empty() || message.chars().count() > MAX_MESSAGE_CHARS {
        return turn_error(
            StatusCode::BAD_REQUEST,
            "Describe what you want to build in 1 to 4000 characters.",
        );
    }

    // A new session is keyed by its first message (stable id, so re-describing the
    // same thing continues the same build rather than forking a new one).
    let id = match &request.app_id {
        Some(existing) if !existing.trim().is_empty() => existing.trim().to_string(),
        _ => app_builder::app_id(message),
    };

    let mut session = load_session(&id).unwrap_or_else(|| StoredSession {
        id: id.clone(),
        name: app_builder::derive_app_name(message),
        engine: String::new(),
        created_at: now_secs(),
        updated_at: now_secs(),
        thread: Vec::new(),
    });

    session.thread.push(Message {
        role: "you".to_string(),
        text: message.to_string(),
        at: now_secs(),
    });

    let engine = crate::resident::active_engine_label();
    session.engine = engine.to_string();
    let workspace = workspace_dir(&id);

    let agent_text = match build_turn(engine, &workspace, &session, message) {
        Ok(text) => text,
        Err(detail) => {
            return turn_error(StatusCode::SERVICE_UNAVAILABLE, &detail);
        }
    };

    session.thread.push(Message {
        role: "agent".to_string(),
        text: agent_text,
        at: now_secs(),
    });
    session.updated_at = now_secs();

    if write_session(&session).is_err() {
        return turn_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The Studio session could not be saved.",
        );
    }

    (
        StatusCode::OK,
        Json(TurnOutcome {
            ok: true,
            text: "Built.".to_string(),
            session: Some(hydrate(session)),
        }),
    )
}

/// Run one build turn on the active engine. Codex is a real agent and writes the
/// files itself; a chat model writes files through the OS's file-emit step.
fn build_turn(
    engine: &str,
    workspace: &Path,
    session: &StoredSession,
    message: &str,
) -> Result<String, String> {
    if engine == "codex" {
        return crate::codex::run_codex_in(workspace, &codex_prompt(session, message));
    }
    if engine == "none" {
        return Err(
            "No engine is active. Choose GPT-OSS, sign in to Codex, or add an API key.".to_string(),
        );
    }

    let raw = crate::resident::resident_generate(&model_prompt(session, message))
        .map_err(|detail| format!("The engine could not build that: {detail}."))?;
    let (prose, files) = parse_emitted_files(&raw);
    write_emitted_files(workspace, &files);
    if prose.is_empty() {
        Ok(format!(
            "Updated {} file{}.",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ))
    } else {
        Ok(prose)
    }
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

fn write_emitted_files(workspace: &Path, files: &[(String, String)]) {
    for (path, content) in files {
        let Some(target) = join_within(workspace, path) else {
            continue;
        };
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let bytes = content.as_bytes();
        let _ = fs::write(&target, &bytes[..bytes.len().min(MAX_FILE_BYTES)]);
    }
}

fn hydrate(session: StoredSession) -> StudioSession {
    let files = list_workspace_files(&workspace_dir(&session.id));
    StudioSession {
        id: session.id,
        name: session.name,
        engine: session.engine,
        created_at: session.created_at,
        updated_at: session.updated_at,
        thread: session.thread,
        files,
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

fn write_session(session: &StoredSession) -> std::io::Result<()> {
    let dir = sessions_dir();
    fs::create_dir_all(&dir)?;
    let path = session_path(&session.id);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(session)?)?;
    fs::rename(tmp, path)
}

fn list_workspace_files(workspace: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_files(workspace, workspace, &mut files);
    files.sort();
    files
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_string_lossy().into_owned());
        }
    }
}

fn sessions_dir() -> PathBuf {
    app_builder::apps_dir().join("sessions")
}

fn session_path(id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.json", sanitize_id(id)))
}

fn workspace_dir(id: &str) -> PathBuf {
    app_builder::apps_dir()
        .join("workspace")
        .join(sanitize_id(id))
}

/// Resolve a workspace-relative request path to an absolute path, refusing any
/// path that escapes the app's workspace (so `..` and absolute paths cannot read
/// outside the sandbox).
fn safe_workspace_path(id: &str, path: &str) -> Option<PathBuf> {
    join_within(&workspace_dir(id), path)
}

fn join_within(root: &Path, path: &str) -> Option<PathBuf> {
    let candidate = Path::new(path);
    for component in candidate.components() {
        match component {
            Component::Normal(_) => {}
            // Reject absolute, parent (..), root, and prefix components.
            _ => return Some(root.to_path_buf()).filter(|_| false),
        }
    }
    Some(root.join(candidate))
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .take(96)
        .collect()
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
    use super::{join_within, parse_emitted_files, sanitize_id};
    use std::path::Path;

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
    fn join_within_blocks_escapes() {
        let root = Path::new("/var/lib/goblins-os/apps/workspace/app");
        assert!(join_within(root, "src/main.py").is_some());
        // Traversal and absolute paths are refused.
        assert!(join_within(root, "../../etc/passwd").is_none());
        assert!(join_within(root, "/etc/passwd").is_none());
    }

    #[test]
    fn sanitize_id_strips_path_characters() {
        assert_eq!(sanitize_id("a-notes-app-1a2b3c4d"), "a-notes-app-1a2b3c4d");
        assert_eq!(sanitize_id("../../evil/id"), "evilid");
    }
}
