//! Local, on-device voice for Goblins OS.
//!
//! Voice is assembled from local, offline-capable parts so it works the same
//! whether the engine is GPT-OSS or a bring-your-own key, and stays fully private
//! in offline mode: speech-to-text with a local Whisper runtime, the resident
//! model for the reply, text-to-speech with a local Piper voice, captured and
//! played through the OS audio stack (ALSA over PipeWire). The Whisper and Piper
//! models are weights, so — like GPT-OSS — they are never bundled in the image;
//! the OS reports what is present and what to add, and greys voice out until then.

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
    path::{Component, Path, PathBuf},
    process::Stdio,
    thread,
    time::{Duration, Instant},
};

use axum::{http::StatusCode, Json};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, Metadata, MetadataExt, OpenOptions, OpenOptionsExt},
};
use serde::Serialize;

use crate::bounded::{bounded_output_of, isolated_command, BoundedCommandError};

const DEFAULT_VOICE_DIR: &str = "/var/lib/goblins-os/voice";
const VOICE_WAKE_WORD: &str = "Goblin";
const VOICE_WAKE_PHRASES: &[&str] = &["Goblin", "Hey Goblin"];
const VOICE_WAKE_LISTENER_DETAIL: &str = "Press the voice button, then say Goblin. Background wake listening is not ready until the local wake-word listener is available.";
const REQUIRED_VOICE_WORK_MODE: u32 = 0o700;
const REQUIRED_VOICE_PROBE_MODE: u32 = 0o600;
const MAX_TRANSCRIPT_BYTES: usize = 64 * 1024;
const VOICE_STORAGE_PROOF_BYTES: &[u8] = b"goblins-os-voice-storage-proof\n";

#[derive(Serialize)]
pub struct VoiceStatus {
    source: &'static str,
    available: bool,
    offline_safe: bool,
    wake_word: &'static str,
    wake_phrases: &'static [&'static str],
    wake_listening: Capability,
    speech_to_text: Capability,
    text_to_speech: Capability,
    capture: Capability,
    playback: Capability,
    detail: String,
}

#[derive(Serialize)]
struct Capability {
    ready: bool,
    component: String,
    detail: String,
}

#[derive(Serialize)]
pub struct ConverseOutcome {
    ok: bool,
    transcript: String,
    reply: String,
    text: String,
}

/// The result of a dictation pass — the recognized text to type into the focused
/// field, or an honest reason it could not run.
#[derive(Serialize)]
pub struct DictateOutcome {
    ok: bool,
    transcript: String,
    text: String,
}

#[derive(Serialize)]
pub struct VoiceStorageProof {
    ok: bool,
    storage: &'static str,
    create_new: bool,
    write: bool,
    fsync: bool,
    unlink: bool,
    detail: &'static str,
}

pub async fn voice_status() -> Json<VoiceStatus> {
    Json(build_status())
}

/// Server-only release proof that the real production core identity can mutate
/// the exact shipped voice work root. No caller-controlled path or content is
/// accepted, and the create-new probe is fsynced and removed before success.
pub async fn voice_storage_release_proof() -> (StatusCode, Json<VoiceStorageProof>) {
    match probe_voice_storage_at(&Path::new(DEFAULT_VOICE_DIR).join("work")) {
        Ok(()) => (
            StatusCode::OK,
            Json(VoiceStorageProof {
                ok: true,
                storage: "voice-work",
                create_new: true,
                write: true,
                fsync: true,
                unlink: true,
                detail: "Production core voice storage write verified.",
            }),
        ),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(VoiceStorageProof {
                ok: false,
                storage: "voice-work",
                create_new: false,
                write: false,
                fsync: false,
                unlink: false,
                detail: "Production core voice storage write could not be verified.",
            }),
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StorageObjectIdentity {
    device: u64,
    inode: u64,
}

fn invalid_storage_proof(detail: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, detail)
}

/// Resolve an absolute directory one component at a time from an already-open
/// root descriptor. `open_dir_nofollow` prevents every intermediate component
/// from redirecting the proof through a symlink, while each returned descriptor
/// remains stable if an ancestor is renamed concurrently.
fn open_absolute_directory_nofollow(path: &Path) -> io::Result<Dir> {
    if !path.is_absolute() {
        return Err(invalid_storage_proof(
            "voice proof storage path must be absolute",
        ));
    }
    let mut directory = Dir::open_ambient_dir(Path::new("/"), ambient_authority())?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => directory = directory.open_dir_nofollow(name)?,
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(invalid_storage_proof(
                    "voice proof storage path contains an unsafe component",
                ));
            }
        }
    }
    Ok(directory)
}

fn effective_group_id() -> u32 {
    // SAFETY: getegid has no preconditions and does not dereference pointers.
    unsafe { libc::getegid() }
}

fn effective_user_id() -> u32 {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    unsafe { libc::geteuid() }
}

fn validate_voice_work_metadata(metadata: &Metadata) -> io::Result<StorageObjectIdentity> {
    if !metadata.is_dir()
        || metadata.uid() != effective_user_id()
        || metadata.gid() != effective_group_id()
        || metadata.mode() & 0o7777 != REQUIRED_VOICE_WORK_MODE
    {
        return Err(invalid_storage_proof(
            "voice work storage must be a real core-owned directory with mode 0700",
        ));
    }
    Ok(StorageObjectIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn validate_voice_probe_metadata(metadata: &Metadata) -> io::Result<StorageObjectIdentity> {
    if !metadata.is_file()
        || metadata.uid() != effective_user_id()
        || metadata.gid() != effective_group_id()
        || metadata.mode() & 0o7777 != REQUIRED_VOICE_PROBE_MODE
        || metadata.nlink() != 1
    {
        return Err(invalid_storage_proof(
            "voice storage probe must be one private core-owned regular file",
        ));
    }
    Ok(StorageObjectIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn create_voice_storage_probe(work: &Dir) -> io::Result<(OsString, cap_std::fs::File)> {
    for _ in 0..16 {
        let name = OsString::from(format!(
            ".release-proof-{:016x}{:016x}.probe",
            rand::random::<u64>(),
            rand::random::<u64>()
        ));
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(REQUIRED_VOICE_PROBE_MODE)
            .follow(FollowSymlinks::No);
        match work.open_with(&name, &options) {
            Ok(file) => return Ok((name, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique voice storage proof file",
    ))
}

fn sync_voice_work_directory(work: &Dir) -> io::Result<()> {
    // cap-std intentionally uses O_PATH for directory capabilities on Linux;
    // open the already-held directory itself to obtain a syncable descriptor
    // without resolving the ambient production path again.
    // SAFETY: work owns a live directory descriptor, the path is a static
    // NUL-terminated ".", and successful openat returns a newly owned fd.
    let descriptor = unsafe {
        libc::openat(
            work.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful openat call transferred ownership of this new
    // descriptor to the caller; File closes it exactly once on drop.
    unsafe { fs::File::from_raw_fd(descriptor) }.sync_all()
}

fn write_sync_unlink_voice_probe(
    work: &Dir,
    name: &OsStr,
    mut file: cap_std::fs::File,
) -> io::Result<()> {
    let opened_identity = validate_voice_probe_metadata(&file.metadata()?)?;
    if file.metadata()?.len() != 0 {
        return Err(invalid_storage_proof(
            "new voice storage probe was not empty",
        ));
    }
    file.write_all(VOICE_STORAGE_PROOF_BYTES)?;
    file.sync_all()?;

    let written_metadata = file.metadata()?;
    if validate_voice_probe_metadata(&written_metadata)? != opened_identity
        || written_metadata.len() != VOICE_STORAGE_PROOF_BYTES.len() as u64
    {
        return Err(invalid_storage_proof(
            "voice storage probe changed identity, ownership, mode, or size",
        ));
    }
    drop(file);

    let entry_metadata = work.symlink_metadata(name)?;
    if validate_voice_probe_metadata(&entry_metadata)? != opened_identity {
        return Err(invalid_storage_proof(
            "voice storage probe directory entry was replaced",
        ));
    }
    work.remove_file(name)?;
    match work.symlink_metadata(name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => {
            return Err(invalid_storage_proof(
                "voice storage probe still exists after unlink",
            ));
        }
        Err(error) => return Err(error),
    }
    sync_voice_work_directory(work)
}

fn probe_voice_storage_at(work_path: &Path) -> io::Result<()> {
    let work = open_absolute_directory_nofollow(work_path)?;
    let original_directory = validate_voice_work_metadata(&work.dir_metadata()?)?;
    let (name, file) = create_voice_storage_probe(&work)?;
    let result = write_sync_unlink_voice_probe(&work, &name, file);
    if result.is_err() {
        let _ = work.remove_file(&name);
    }
    result?;

    // Re-resolve the fixed production path without following any links and
    // require that it still names the same directory descriptor used above.
    let resolved_again = open_absolute_directory_nofollow(work_path)?;
    if validate_voice_work_metadata(&resolved_again.dir_metadata()?)? != original_directory {
        return Err(invalid_storage_proof(
            "voice work storage changed identity during the release proof",
        ));
    }
    Ok(())
}

/// Delete any voice artifacts left by an unclean shutdown before the core
/// starts accepting requests. The held directory capability never follows a
/// child symlink, and the dedicated work root contains transient voice data
/// only, so unknown stale entries are removed rather than preserved silently.
pub(crate) fn purge_stale_voice_workspaces() -> io::Result<()> {
    purge_stale_voice_workspaces_at(&work_dir())
}

fn purge_stale_voice_workspaces_at(work_path: &Path) -> io::Result<()> {
    let work = open_absolute_directory_nofollow(work_path)?;
    let original_directory = validate_voice_work_metadata(&work.dir_metadata()?)?;
    let entries = work
        .entries()?
        .map(|entry| entry.map(|entry| (entry.file_name(), entry.file_type())))
        .collect::<io::Result<Vec<_>>>()?;
    for (name, file_type) in entries {
        if file_type?.is_dir() {
            work.remove_dir_all(&name)?;
        } else {
            work.remove_file(&name)?;
        }
    }
    sync_voice_work_directory(&work)?;
    let resolved_again = open_absolute_directory_nofollow(work_path)?;
    if validate_voice_work_metadata(&resolved_again.dir_metadata()?)? != original_directory {
        return Err(invalid_storage_proof(
            "voice work storage changed identity during stale cleanup",
        ));
    }
    Ok(())
}

/// A converse turn is minutes of blocking work (mic capture, Whisper, a model
/// turn, Piper, playback), so the body runs on the blocking pool instead of
/// pinning an async runtime worker.
pub async fn voice_converse() -> (StatusCode, Json<ConverseOutcome>) {
    crate::bounded::run_voice_blocking(voice_converse_blocking)
        .await
        .unwrap_or_else(|_| {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(ConverseOutcome {
                    ok: false,
                    transcript: String::new(),
                    reply: String::new(),
                    text: crate::bounded::VOICE_OPERATION_BUSY_MESSAGE.to_string(),
                }),
            )
        })
}

fn voice_converse_blocking() -> (StatusCode, Json<ConverseOutcome>) {
    match run_converse() {
        Ok((transcript, reply)) => (
            StatusCode::OK,
            Json(ConverseOutcome {
                ok: true,
                text: "Heard you, replied out loud.".to_string(),
                transcript,
                reply,
            }),
        ),
        Err(detail) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ConverseOutcome {
                ok: false,
                transcript: String::new(),
                reply: String::new(),
                text: detail,
            }),
        ),
    }
}

/// Dictation: capture the mic and transcribe to text — the STT half of the loop,
/// for "speak into any text field." It needs only Whisper + capture (no model
/// answer, no speech-out), and the desktop helper types the returned transcript
/// into the focused field via the Wayland synthetic-input path (wtype).
/// Dictation blocks on the fixed capture window plus a Whisper pass, so the
/// body runs on the blocking pool instead of pinning an async runtime worker.
pub async fn voice_dictate() -> (StatusCode, Json<DictateOutcome>) {
    crate::bounded::run_voice_blocking(voice_dictate_blocking)
        .await
        .unwrap_or_else(|_| {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(DictateOutcome {
                    ok: false,
                    transcript: String::new(),
                    text: crate::bounded::VOICE_OPERATION_BUSY_MESSAGE.to_string(),
                }),
            )
        })
}

fn voice_dictate_blocking() -> (StatusCode, Json<DictateOutcome>) {
    match run_dictate() {
        Ok(transcript) => (
            StatusCode::OK,
            Json(DictateOutcome {
                ok: true,
                text: "Transcribed.".to_string(),
                transcript,
            }),
        ),
        Err(detail) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(DictateOutcome {
                ok: false,
                transcript: String::new(),
                text: detail,
            }),
        ),
    }
}

fn run_dictate() -> Result<String, String> {
    let stt = stt_capability();
    if !stt.ready {
        return Err(stt.detail);
    }
    with_private_voice_workspace(|workspace| {
        let mut input = private_voice_audio_file(workspace, "microphone")?;
        record_audio(&mut input)?;
        let transcript = transcribe(input.path())?;
        if transcript.is_empty() {
            return Err("Goblins OS didn’t catch that — try again.".to_string());
        }
        Ok(transcript)
    })
}

pub(crate) fn capture_voice_command_transcript() -> Result<String, String> {
    run_dictate()
}

fn build_status() -> VoiceStatus {
    let stt = stt_capability();
    let tts = tts_capability();
    let (capture, playback) = desktop_audio_capabilities();
    let available = stt.ready && tts.ready && capture.ready && playback.ready;

    VoiceStatus {
        source: "goblins-os-core",
        available,
        // Every part is local, so voice never needs the network — it is safe in
        // offline / private mode.
        offline_safe: true,
        wake_word: VOICE_WAKE_WORD,
        wake_phrases: VOICE_WAKE_PHRASES,
        wake_listening: wake_listening_capability(),
        detail: if available {
            "Goblin voice is ready. Press the voice button, say Goblin, and it answers out loud on this device. Background wake listening is not ready until the local wake-word listener is available."
                .to_string()
        } else {
            "Goblin voice runs on local Whisper and Piper models. Add the missing voice components; background wake listening stays off until the local wake-word listener is available.".to_string()
        },
        speech_to_text: stt,
        text_to_speech: tts,
        capture,
        playback,
    }
}

fn wake_listening_capability() -> Capability {
    Capability {
        ready: false,
        component: "local wake-word listener".to_string(),
        detail: VOICE_WAKE_LISTENER_DETAIL.to_string(),
    }
}

fn stt_capability() -> Capability {
    let binary = whisper_bin();
    if !binary_present(&binary) {
        return Capability {
            ready: false,
            component: binary,
            detail: "Local Whisper runtime not found.".to_string(),
        };
    }
    match first_model(&stt_dir(), &["bin", "gguf", "ggml"]) {
        Some(model) => Capability {
            ready: true,
            component: binary,
            detail: format!("Speech-to-text ready with {}.", model.display()),
        },
        None => Capability {
            ready: false,
            component: binary,
            detail: format!(
                "No Whisper model in {} — add one to enable speech-to-text.",
                stt_dir().display()
            ),
        },
    }
}

fn tts_capability() -> Capability {
    let binary = piper_bin();
    if !binary_present(&binary) {
        return Capability {
            ready: false,
            component: binary,
            detail: "Local Piper runtime not found.".to_string(),
        };
    }
    match first_model(&tts_dir(), &["onnx"]) {
        Some(model) => Capability {
            ready: true,
            component: binary,
            detail: format!("Text-to-speech ready with {}.", model.display()),
        },
        None => Capability {
            ready: false,
            component: binary,
            detail: format!(
                "No Piper voice in {} — add one to enable text-to-speech.",
                tts_dir().display()
            ),
        },
    }
}

fn desktop_audio_capabilities() -> (Capability, Capability) {
    match crate::session_bridge::voice_audio_status() {
        crate::session_bridge::VoiceBridgeResult::Success(status) => (
            Capability {
                ready: status.capture_ready,
                component: "desktop-session microphone".to_string(),
                detail: status.capture_detail,
            },
            Capability {
                ready: status.playback_ready,
                component: "desktop-session speaker".to_string(),
                detail: status.playback_detail,
            },
        ),
        crate::session_bridge::VoiceBridgeResult::Unavailable => {
            unavailable_audio_capabilities("The desktop-session audio bridge is not ready.")
        }
        crate::session_bridge::VoiceBridgeResult::Failed(detail) => {
            unavailable_audio_capabilities(&detail)
        }
        crate::session_bridge::VoiceBridgeResult::InvalidResponse => {
            unavailable_audio_capabilities(
                "The desktop-session audio bridge returned an invalid response.",
            )
        }
    }
}

fn unavailable_audio_capabilities(detail: &str) -> (Capability, Capability) {
    (
        Capability {
            ready: false,
            component: "desktop-session microphone".to_string(),
            detail: detail.to_string(),
        },
        Capability {
            ready: false,
            component: "desktop-session speaker".to_string(),
            detail: detail.to_string(),
        },
    )
}

/// The full local voice loop: capture the microphone, transcribe with Whisper,
/// answer with the Goblins AI runtime (GPT-OSS or the user's key), synthesize the
/// reply with Piper, and play it. Every step degrades to a clear message.
fn run_converse() -> Result<(String, String), String> {
    let stt = stt_capability();
    let tts = tts_capability();
    if !stt.ready {
        return Err(stt.detail);
    }
    if !tts.ready {
        return Err(tts.detail);
    }
    with_private_voice_workspace(|workspace| {
        let mut input = private_voice_audio_file(workspace, "microphone")?;
        record_audio(&mut input)?;
        let transcript = transcribe(input.path())?;
        if transcript.is_empty() {
            return Err("Goblins OS didn’t catch that — try again.".to_string());
        }
        drop(input);

        let reply = crate::resident::resident_generate(&transcript)
            .map_err(|detail| format!("The on-device model could not answer: {detail}."))?;

        let reply_wav = private_voice_audio_file(workspace, "reply")?;
        synthesize(&reply, reply_wav.path())?;
        play_audio(reply_wav.path())?;

        Ok((transcript, reply))
    })
}

fn record_audio(output: &mut tempfile::NamedTempFile) -> Result<(), String> {
    let wav = match crate::session_bridge::voice_capture() {
        crate::session_bridge::VoiceBridgeResult::Success(wav) => wav,
        crate::session_bridge::VoiceBridgeResult::Unavailable => {
            return Err("The desktop-session microphone bridge is not ready.".to_string())
        }
        crate::session_bridge::VoiceBridgeResult::Failed(detail) => return Err(detail),
        crate::session_bridge::VoiceBridgeResult::InvalidResponse => {
            return Err("The desktop-session microphone returned invalid audio.".to_string())
        }
    };
    output
        .as_file_mut()
        .set_len(0)
        .and_then(|()| output.as_file_mut().write_all(&wav))
        .and_then(|()| output.as_file_mut().flush())
        .map_err(|_| "Captured voice audio could not be stored privately.".to_string())
}

fn transcribe(input: &Path) -> Result<String, String> {
    let model = first_model(&stt_dir(), &["bin", "gguf", "ggml"])
        .ok_or_else(|| "No Whisper model is installed.".to_string())?;
    let prefix = input.with_extension("");
    // Recognition is genuinely heavy compute — the bound has to cover model
    // load plus inference for whatever model the user installed — so it gets a
    // much wider bound than the status probes.
    let mut command = isolated_command(&whisper_bin());
    command
        .args(["-m"])
        .arg(&model)
        .args(["-f"])
        .arg(input)
        .args(["-otxt", "-nt", "-of"])
        .arg(&prefix);
    let output =
        bounded_output_of(&mut command, Duration::from_secs(120)).map_err(|error| match error {
            BoundedCommandError::TimedOut => "Speech-to-text did not finish in time.".to_string(),
            _ => "The Whisper runtime could not start.".to_string(),
        })?;
    if !output.status.success() {
        return Err("Speech-to-text failed.".to_string());
    }
    let transcript_path = prefix.with_extension("txt");
    let text = read_private_transcript(&transcript_path)?;
    fs::remove_file(&transcript_path)
        .map_err(|_| "The private voice transcript could not be cleaned up.".to_string())?;
    Ok(text.trim().to_string())
}

fn read_private_transcript(path: &Path) -> Result<String, String> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "Speech-to-text did not produce a private transcript.".to_string())?;
    let before = file
        .metadata()
        .map_err(|_| "The private voice transcript could not be inspected.".to_string())?;
    if !before.is_file()
        || before.uid() != effective_user_id()
        || before.gid() != effective_group_id()
        || before.mode() & 0o7777 != 0o600
        || before.nlink() != 1
        || before.len() > MAX_TRANSCRIPT_BYTES as u64
    {
        return Err("The private voice transcript failed safety validation.".to_string());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    Read::by_ref(&mut file)
        .take((MAX_TRANSCRIPT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "The private voice transcript could not be read.".to_string())?;
    let after = file
        .metadata()
        .map_err(|_| "The private voice transcript could not be revalidated.".to_string())?;
    if bytes.len() > MAX_TRANSCRIPT_BYTES
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || after.len() != bytes.len() as u64
        || after.mode() & 0o7777 != 0o600
        || after.nlink() != 1
    {
        return Err("The private voice transcript changed while it was read.".to_string());
    }
    String::from_utf8(bytes)
        .map_err(|_| "Speech-to-text produced an invalid transcript encoding.".to_string())
}

fn synthesize(text: &str, output: &Path) -> Result<(), String> {
    use std::io::Write;

    let voice = first_model(&tts_dir(), &["onnx"])
        .ok_or_else(|| "No Piper voice is installed.".to_string())?;
    let mut command = isolated_command(&piper_bin());
    let mut child = command
        .args(["-q", "-m"])
        .arg(&voice)
        .args(["-f"])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "The Piper runtime could not start.".to_string())?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|_| "Text-to-speech input failed.".to_string())?;
    }
    // Close stdin so Piper sees end-of-input: unlike `wait`, `try_wait` does
    // not drop the pipe for us, and Piper reads text until EOF.
    drop(child.stdin.take());
    // Piper writes the audio straight to the output file (stdout/stderr are
    // null), so no pipe draining is needed — a bounded poll mirroring
    // `bounded_output_of` keeps a hung synthesis from wedging a runtime
    // worker forever.
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= Duration::from_secs(60) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Speech synthesis did not finish in time.".to_string());
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Text-to-speech failed.".to_string());
            }
        }
    };
    if status.success() {
        Ok(())
    } else {
        Err("Text-to-speech failed.".to_string())
    }
}

fn play_audio(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|_| "Synthesized voice audio could not be inspected.".to_string())?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
        return Err("Synthesized voice audio exceeds the playback size limit.".to_string());
    }
    let wav = fs::read(path)
        .map_err(|_| "Synthesized voice audio could not be read privately.".to_string())?;
    match crate::session_bridge::voice_playback(&wav) {
        crate::session_bridge::VoiceBridgeResult::Success(()) => Ok(()),
        crate::session_bridge::VoiceBridgeResult::Unavailable => {
            Err("The desktop-session speaker bridge is not ready.".to_string())
        }
        crate::session_bridge::VoiceBridgeResult::Failed(detail) => Err(detail),
        crate::session_bridge::VoiceBridgeResult::InvalidResponse => {
            Err("Synthesized voice audio was rejected before playback.".to_string())
        }
    }
}

fn binary_present(binary: &str) -> bool {
    if binary.contains('/') {
        return Path::new(binary).exists();
    }
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

/// The first model file in `dir` whose extension is one of `extensions`, chosen
/// deterministically (sorted) so the same model is used across calls.
fn first_model(dir: &Path, extensions: &[&str]) -> Option<PathBuf> {
    let mut matches: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
        })
        .collect();
    matches.sort();
    matches.into_iter().next()
}

fn voice_dir() -> PathBuf {
    env::var("GOBLINS_OS_VOICE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_VOICE_DIR).to_path_buf())
}

fn stt_dir() -> PathBuf {
    voice_dir().join("stt")
}

fn tts_dir() -> PathBuf {
    voice_dir().join("tts")
}

fn work_dir() -> PathBuf {
    voice_dir().join("work")
}

fn with_private_voice_workspace<T>(
    operation: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let root = work_dir();
    let root_directory = open_absolute_directory_nofollow(&root)
        .map_err(|_| "The private voice work directory is not ready.".to_string())?;
    let root_metadata = root_directory
        .dir_metadata()
        .map_err(|_| "The private voice work directory could not be inspected.".to_string())?;
    let original_root = validate_voice_work_metadata(&root_metadata)
        .map_err(|_| "The private voice work directory is not secured.".to_string())?;
    drop(root_directory);

    let workspace = tempfile::Builder::new()
        .prefix("voice-call-")
        .tempdir_in(&root)
        .map_err(|_| "A private voice workspace could not be created.".to_string())?;
    let result = (|| {
        let metadata = fs::symlink_metadata(workspace.path())
            .map_err(|_| "The private voice workspace could not be inspected.".to_string())?;
        if !metadata.is_dir()
            || metadata.uid() != effective_user_id()
            || metadata.gid() != effective_group_id()
            || metadata.mode() & 0o7777 != 0o700
        {
            return Err("The per-call voice workspace is not owner-private.".to_string());
        }
        let resolved_root = open_absolute_directory_nofollow(&root)
            .and_then(|directory| {
                let metadata = directory.dir_metadata()?;
                validate_voice_work_metadata(&metadata)
            })
            .map_err(|_| "The private voice work directory changed unexpectedly.".to_string())?;
        if resolved_root != original_root {
            return Err("The private voice work directory changed unexpectedly.".to_string());
        }
        operation(workspace.path())
    })();
    let workspace_path = workspace.path().to_path_buf();
    let cleanup = workspace
        .close()
        .or_else(|_| fs::remove_dir_all(&workspace_path));
    finish_private_voice_operation(result, cleanup)
}

fn finish_private_voice_operation<T>(
    operation: Result<T, String>,
    cleanup: io::Result<()>,
) -> Result<T, String> {
    if cleanup.is_err() {
        return Err("The private voice workspace could not be cleaned up.".to_string());
    }
    operation
}

fn private_voice_audio_file(
    workspace: &Path,
    purpose: &str,
) -> Result<tempfile::NamedTempFile, String> {
    let file = tempfile::Builder::new()
        .prefix(&format!("{purpose}-"))
        .suffix(".wav")
        .tempfile_in(workspace)
        .map_err(|_| "A private voice audio file could not be created.".to_string())?;
    let metadata = file
        .as_file()
        .metadata()
        .map_err(|_| "The private voice audio file could not be inspected.".to_string())?;
    if !metadata.is_file()
        || metadata.uid() != effective_user_id()
        || metadata.gid() != effective_group_id()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err("Voice audio must be one owner-only regular file.".to_string());
    }
    Ok(file)
}

fn whisper_bin() -> String {
    env::var("GOBLINS_OS_WHISPER_BIN").unwrap_or_else(|_| "whisper-cli".to_string())
}

fn piper_bin() -> String {
    env::var("GOBLINS_OS_PIPER_BIN").unwrap_or_else(|_| "piper".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        finish_private_voice_operation, first_model, probe_voice_storage_at,
        purge_stale_voice_workspaces_at, Capability, VoiceStatus, VOICE_WAKE_LISTENER_DETAIL,
        VOICE_WAKE_PHRASES, VOICE_WAKE_WORD,
    };
    use std::{
        os::unix::fs::{symlink, MetadataExt, PermissionsExt},
        path::PathBuf,
    };

    fn unique_tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    #[test]
    fn release_proof_writes_fsyncs_and_removes_fixed_voice_storage_probe() {
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let work = root.join("work");
        std::fs::create_dir(&work).unwrap();
        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).unwrap();
        probe_voice_storage_at(&work).unwrap();
        assert!(work.is_dir());
        assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
    }

    #[test]
    fn release_proof_rejects_final_and_intermediate_directory_symlinks() {
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let real_parent = root.join("real-parent");
        let real_work = real_parent.join("work");
        std::fs::create_dir_all(&real_work).unwrap();
        std::fs::set_permissions(&real_work, std::fs::Permissions::from_mode(0o700)).unwrap();

        let final_link = root.join("work-link");
        symlink(&real_work, &final_link).unwrap();
        assert!(probe_voice_storage_at(&final_link).is_err());

        let intermediate_link = root.join("parent-link");
        symlink(&real_parent, &intermediate_link).unwrap();
        assert!(probe_voice_storage_at(&intermediate_link.join("work")).is_err());
        assert_eq!(std::fs::read_dir(&real_work).unwrap().count(), 0);
    }

    #[test]
    fn release_proof_rejects_a_work_directory_with_the_wrong_mode() {
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let work = root.join("work");
        std::fs::create_dir(&work).unwrap();
        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(probe_voice_storage_at(&work).is_err());
        assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
    }

    #[test]
    fn cleanup_failure_overrides_operation_result_without_leaking_detail() {
        let cleanup_error = || std::io::Error::other("sensitive path detail");
        assert_eq!(
            finish_private_voice_operation::<()>(Ok(()), Err(cleanup_error())).unwrap_err(),
            "The private voice workspace could not be cleaned up."
        );
        assert_eq!(
            finish_private_voice_operation::<()>(
                Err("operation failed".to_string()),
                Err(cleanup_error()),
            )
            .unwrap_err(),
            "The private voice workspace could not be cleaned up."
        );
    }

    #[test]
    fn startup_purge_removes_nested_leftovers_without_following_child_symlinks() {
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let work = root.join("work");
        let outside = root.join("outside-secret");
        std::fs::create_dir(&work).unwrap();
        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&outside, b"must remain").unwrap();
        let before = std::fs::metadata(&work).unwrap();

        let stale = work.join("voice-call-stale");
        std::fs::create_dir(&stale).unwrap();
        std::fs::write(stale.join("microphone.wav"), b"private audio").unwrap();
        symlink(&outside, stale.join("reply.wav")).unwrap();

        purge_stale_voice_workspaces_at(&work).unwrap();

        let after = std::fs::metadata(&work).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_eq!(after.mode() & 0o7777, 0o700);
        assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
        assert_eq!(std::fs::read(&outside).unwrap(), b"must remain");
    }

    #[test]
    fn startup_purge_rejects_symlinked_or_insecure_work_roots() {
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let work = root.join("work");
        std::fs::create_dir(&work).unwrap();
        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(purge_stale_voice_workspaces_at(&work).is_err());

        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).unwrap();
        let linked = root.join("linked-work");
        symlink(&work, &linked).unwrap();
        assert!(purge_stale_voice_workspaces_at(&linked).is_err());
    }

    #[test]
    fn first_model_picks_a_matching_file_deterministically() {
        let dir = unique_tmp("goblins-os-voice-stt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // No models yet => none.
        assert_eq!(first_model(&dir, &["bin", "gguf"]), None);

        std::fs::write(dir.join("readme.txt"), b"x").unwrap();
        std::fs::write(dir.join("b-model.gguf"), b"x").unwrap();
        std::fs::write(dir.join("a-model.bin"), b"x").unwrap();

        // Wrong extension is ignored; the alphabetically-first match is chosen.
        let chosen = first_model(&dir, &["bin", "gguf"]).unwrap();
        assert_eq!(chosen.file_name().unwrap(), "a-model.bin");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_serializes_for_the_native_ui() {
        let status = VoiceStatus {
            source: "goblins-os-core",
            available: false,
            offline_safe: true,
            wake_word: VOICE_WAKE_WORD,
            wake_phrases: VOICE_WAKE_PHRASES,
            wake_listening: Capability {
                ready: false,
                component: "local wake-word listener".to_string(),
                detail: VOICE_WAKE_LISTENER_DETAIL.to_string(),
            },
            speech_to_text: Capability {
                ready: false,
                component: "whisper-cli".to_string(),
                detail: "missing".to_string(),
            },
            text_to_speech: Capability {
                ready: false,
                component: "piper".to_string(),
                detail: "missing".to_string(),
            },
            capture: Capability {
                ready: true,
                component: "arecord".to_string(),
                detail: "ok".to_string(),
            },
            playback: Capability {
                ready: true,
                component: "aplay".to_string(),
                detail: "ok".to_string(),
            },
            detail: "add models".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"available\":false"));
        assert!(json.contains("\"offline_safe\":true"));
        assert!(json.contains("\"wake_word\":\"Goblin\""));
        assert!(json.contains("Hey Goblin"));
        assert!(json.contains("wake_listening"));
        assert!(json.contains("speech_to_text"));
    }

    #[test]
    fn wake_word_copy_is_goblins_native_and_truthful() {
        let source = include_str!("voice.rs");

        assert!(source.contains("VOICE_WAKE_WORD: &str = \"Goblin\""));
        assert!(source.contains("\"Hey Goblin\""));
        assert!(source.contains("Background wake listening is not ready"));
        let apple_assistant = ["si", "ri"].join("");
        let passive_claim = ["always", " listening"].join("");
        let lower = source.to_ascii_lowercase();
        assert!(!lower.contains(&apple_assistant));
        assert!(!lower.contains(&passive_claim));
    }
}
