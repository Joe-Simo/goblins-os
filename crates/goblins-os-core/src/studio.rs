//! The Build Studio: a minimal, multi-turn agent build session over whichever
//! engine is active — local GPT-OSS, the user's OpenAI account via Codex, or a
//! bring-your-own key. The user describes what to build; the engine answers and
//! produces files in the app's workspace; the conversation and the files are
//! persisted so the Studio is a real, resumable surface. It is the single place
//! Goblins OS turns intent into software, and it looks the same whichever brain
//! runs — Codex builds files directly, while a chat model builds through a small
//! file-emit step. No pre-installed apps; everything here was built on request.

use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{extract::Query, http::StatusCode, Json};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, Serialize};

use crate::ai::{audit_ai_action, AiActionOutcome};
use crate::app_builder;

const MAX_MESSAGE_CHARS: usize = 4000;
const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_WORKSPACE_DEPTH: usize = 32;
const MAX_WORKSPACE_FILES: usize = 2_000;

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
    if canonical_workspace_id(&query.app_id).is_err() || relative_file_path(&query.path).is_err() {
        return (StatusCode::BAD_REQUEST, Json(empty()));
    }
    match read_workspace_file_at(&app_builder::apps_dir(), &query.app_id, &query.path) {
        Ok((bytes, truncated)) => (
            StatusCode::OK,
            Json(FileView {
                path: query.path,
                content: String::from_utf8_lossy(&bytes).into_owned(),
                truncated,
            }),
        ),
        Err(_) => (StatusCode::NOT_FOUND, Json(empty())),
    }
}

/// A Studio turn is a real agent run — `codex exec` under its 600s bound or a
/// model turn through the resident relay (120s+ read timeout) — so the body
/// runs on the blocking pool instead of pinning an async runtime worker.
pub async fn studio_turn(Json(request): Json<TurnRequest>) -> (StatusCode, Json<TurnOutcome>) {
    crate::bounded::run_blocking(move || studio_turn_blocking(request))
        .await
        .unwrap_or_else(|_| {
            turn_error(
                StatusCode::TOO_MANY_REQUESTS,
                crate::bounded::LONG_OPERATION_BUSY_MESSAGE,
            )
        })
}

fn studio_turn_blocking(request: TurnRequest) -> (StatusCode, Json<TurnOutcome>) {
    studio_turn_blocking_with_policy(
        request,
        crate::policy::policy_state_for_control("app-builder"),
    )
}

fn studio_turn_blocking_with_policy(
    request: TurnRequest,
    policy: crate::policy::PolicyControlState,
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

    // A new session is keyed by its first message (stable id, so re-describing the
    // same thing continues the same build rather than forking a new one).
    let requested_id = match &request.app_id {
        Some(existing) if !existing.trim().is_empty() => existing.trim().to_string(),
        _ => app_builder::app_id(message),
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
    let workspace_cap = match open_workspace_at(&app_builder::apps_dir(), &id, true) {
        Ok(workspace) => workspace,
        Err(_) => {
            audit_ai_action("build-app", Some("studio"), AiActionOutcome::Failed);
            return turn_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The Studio workspace could not be opened safely.",
            );
        }
    };

    let agent_text = match build_turn(engine, &workspace_cap, &session, message) {
        Ok(text) => text,
        Err(detail) => {
            audit_ai_action("build-app", Some("studio"), AiActionOutcome::Blocked);
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
        audit_ai_action("build-app", Some("studio"), AiActionOutcome::Failed);
        return turn_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The Studio session could not be saved.",
        );
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

/// Run one build turn on the active engine. Codex is a real agent and writes the
/// files itself; a chat model writes files through the OS's file-emit step.
fn build_turn(
    engine: &str,
    workspace: &Dir,
    session: &StoredSession,
    message: &str,
) -> Result<String, String> {
    if engine == "codex" {
        return crate::codex::run_codex_in(workspace, &codex_prompt(session, message));
    }
    if engine == "none" {
        return Err(
            "No engine is active. Choose GPT-OSS, sign in to Codex, or ask a device administrator to install an OpenAI API key.".to_string(),
        );
    }

    let raw = crate::resident::resident_generate(&model_prompt(session, message))
        .map_err(|detail| format!("The engine could not build that: {detail}."))?;
    let (prose, files) = parse_emitted_files(&raw);
    write_emitted_files(workspace, &files).map_err(|_| {
        "The engine returned a workspace path that could not be written safely.".to_string()
    })?;
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
    let files = list_workspace_files_at(&app_builder::apps_dir(), &session.id);
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
    app_builder::apps_dir().join("sessions")
}

fn session_path(id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.json", sanitize_id(id)))
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
    let apps = open_apps_root_at(apps_root, create)?;
    let workspace_root = open_or_create_dir_nofollow(&apps, OsStr::new("workspace"), create)?;
    open_or_create_dir_nofollow(&workspace_root, OsStr::new(&id), create)
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
        canonical_workspace_id, list_workspace_files_at, open_workspace_at, parse_emitted_files,
        read_workspace_file_at, relative_file_path, sanitize_id, studio_turn_blocking_with_policy,
        write_emitted_files, TurnRequest,
    };
    use crate::policy::PolicyControlState;
    use axum::http::StatusCode;
    use std::fs;

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
            );
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(!outcome.ok);
            assert!(outcome.session.is_none());
        }
    }
}
