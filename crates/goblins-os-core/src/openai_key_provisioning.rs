//! Per-user OpenAI API-key provisioning behind a dedicated trusted broker.
//!
//! Ordinary desktop clients can request an add, rotate, or remove operation,
//! but never send or receive key material. The fixed broker encrypts transient
//! plaintext in its own process with `systemd-creds`; this module accepts only
//! that encrypted envelope, binds it to the broker's kernel-authenticated UID,
//! validates it by decrypting inside the core, and persists ciphertext only.

use std::{
    collections::BTreeMap,
    ffi::{CString, OsStr},
    fmt, fs, io,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt as _,
            fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
            process::CommandExt as _,
        },
    },
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use axum::{
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use cap_fs_ext::{DirExt as _, FollowSymlinks, OpenOptionsFollowExt as _};
use cap_std::fs::{
    Dir, DirBuilder, DirBuilderExt as _, Metadata, MetadataExt as _, OpenOptions,
    OpenOptionsExt as _,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::http_error::error_response;

const USER_CREDENTIAL_ROOT: &str = "/var/lib/goblins-os/secrets/openai/users";
const CREDENTIAL_FILE_NAME: &str = "openai-api-key.cred";
const CREDENTIAL_NAME: &str = "goblins.openai-api-key.v1";
const SYSTEMD_CREDS: &str = "/usr/bin/systemd-creds";
const OPERATION_TTL: Duration = Duration::from_secs(300);
const IN_FLIGHT_OPERATION_TTL: Duration = Duration::from_secs(30);
const CREDENTIAL_COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(20);
const COMMAND_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_ENCRYPTED_CREDENTIAL_BYTES: usize = 128 * 1024;
const MAX_KEY_BYTES: usize = 512;
const MAX_COMMAND_STDOUT_BYTES: usize = MAX_KEY_BYTES + 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProvisionAction {
    Add,
    Rotate,
    Remove,
}

impl ProvisionAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Rotate => "rotate",
            Self::Remove => "remove",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperationState {
    Pending,
    Presented,
    Committing,
    Removing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BeginOperationError {
    Unavailable,
    WrongAction,
}

#[derive(Clone, Debug)]
struct PendingOperation {
    intended_user_id: u32,
    action: ProvisionAction,
    configured_at_request: bool,
    state: OperationState,
    expires_at: Instant,
}

#[derive(Default)]
struct OperationStore {
    entries: Mutex<BTreeMap<String, PendingOperation>>,
}

fn operation_store() -> &'static OperationStore {
    static STORE: OnceLock<OperationStore> = OnceLock::new();
    STORE.get_or_init(OperationStore::default)
}

#[derive(Default)]
struct UserMutationLocks {
    entries: Mutex<BTreeMap<u32, Arc<Mutex<()>>>>,
}

fn user_mutation_locks() -> &'static UserMutationLocks {
    static LOCKS: OnceLock<UserMutationLocks> = OnceLock::new();
    LOCKS.get_or_init(UserMutationLocks::default)
}

/// Serialize credential and engine-selection mutations for one installed user.
/// The map stores only numeric UIDs and lock objects, never key material.
pub(crate) fn with_user_key_engine_mutation<T>(user_id: u32, mutation: impl FnOnce() -> T) -> T {
    let lock = {
        let mut locks = user_mutation_locks()
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(
            locks
                .entry(user_id)
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    };
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    mutation()
}

/// Secret bytes exist only in the core and deliberately render as redacted.
/// The backing allocation is wiped when the route resolution is dropped.
pub(crate) struct UserApiKey(Zeroizing<Vec<u8>>);

impl UserApiKey {
    pub(crate) fn as_str(&self) -> &str {
        // Construction validates ASCII, which is always UTF-8.
        std::str::from_utf8(&self.0).expect("validated API key bytes are UTF-8")
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(value: &str) -> Self {
        Self(Zeroizing::new(value.as_bytes().to_vec()))
    }
}

impl fmt::Debug for UserApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UserApiKey([REDACTED])")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManageRequest {
    action: ProvisionAction,
}

#[derive(Serialize)]
struct ManageResponse {
    ok: bool,
    state: &'static str,
    text: &'static str,
}

pub(crate) async fn request_management(
    Extension(client): Extension<crate::control_plane::RequestClient>,
    Json(request): Json<ManageRequest>,
) -> Response {
    let user_id = client.user_id();
    let configured = credential_is_stored(user_id);
    let state_is_valid = match request.action {
        ProvisionAction::Add => !configured,
        ProvisionAction::Rotate | ProvisionAction::Remove => configured,
    };
    if !state_is_valid {
        return error_response(
            StatusCode::CONFLICT,
            match request.action {
                ProvisionAction::Add => {
                    "An OpenAI API key is already stored. Choose Replace to change it."
                }
                ProvisionAction::Rotate => "There is no stored OpenAI API key to replace.",
                ProvisionAction::Remove => "There is no stored OpenAI API key to remove.",
            },
        );
    }

    let Some(lease_id) =
        insert_pending_operation(user_id, request.action, configured, Instant::now())
    else {
        return error_response(
            StatusCode::CONFLICT,
            "Finish or cancel the current OpenAI API key change first.",
        );
    };

    if !matches!(
        crate::session_bridge::launch_openai_key_broker(),
        crate::session_bridge::SessionBridgeResult::Success(ref marker) if marker == "launched"
    ) {
        remove_operation(&lease_id);
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "The protected key window could not be opened. Nothing was stored or changed.",
        );
    }

    Json(ManageResponse {
        ok: true,
        state: "requested",
        text: "Continue in the protected OpenAI API key window.",
    })
    .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrokerClaimRequest {}

#[derive(Serialize)]
pub(crate) struct BrokerClaimResponse {
    ok: bool,
    lease_id: Option<String>,
    action: Option<&'static str>,
    configured: bool,
}

pub(crate) async fn broker_claim(
    Extension(client): Extension<crate::control_plane::RequestClient>,
    Json(BrokerClaimRequest {}): Json<BrokerClaimRequest>,
) -> (StatusCode, Json<BrokerClaimResponse>) {
    let Some((lease_id, operation)) = claim_operation(client.user_id(), Instant::now()) else {
        return (
            StatusCode::GONE,
            Json(BrokerClaimResponse {
                ok: false,
                lease_id: None,
                action: None,
                configured: credential_is_stored(client.user_id()),
            }),
        );
    };
    (
        StatusCode::OK,
        Json(BrokerClaimResponse {
            ok: true,
            lease_id: Some(lease_id),
            action: Some(operation.action.as_str()),
            configured: operation.configured_at_request,
        }),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrokerCommitRequest {
    lease_id: String,
    encrypted_credential: String,
}

#[derive(Serialize)]
pub(crate) struct BrokerMutationResponse {
    ok: bool,
}

pub(crate) async fn broker_commit(
    Extension(client): Extension<crate::control_plane::RequestClient>,
    Json(request): Json<BrokerCommitRequest>,
) -> (StatusCode, Json<BrokerMutationResponse>) {
    if !valid_lease_id(&request.lease_id)
        || !valid_encrypted_credential(request.encrypted_credential.as_bytes())
    {
        return mutation_response(StatusCode::BAD_REQUEST, false);
    }
    let operation =
        match begin_commit_operation(&request.lease_id, client.user_id(), Instant::now()) {
            Ok(operation) => operation,
            Err(BeginOperationError::WrongAction) => {
                return mutation_response(StatusCode::BAD_REQUEST, false);
            }
            Err(BeginOperationError::Unavailable) => {
                return mutation_response(StatusCode::GONE, false);
            }
        };

    let user_id = client.user_id();
    let lease_id = request.lease_id;
    let expected_existing = operation.action == ProvisionAction::Rotate;
    // The encrypted envelope is not plaintext, but it is still authorization
    // material. Move its allocation directly into a zeroizing blocking task;
    // neither it nor the decrypted key crosses the task result or a Debug type.
    let encrypted_credential = Zeroizing::new(request.encrypted_credential.into_bytes());
    let stored = tokio::task::spawn_blocking(move || {
        let Some(plaintext) = decrypt_encrypted_credential(user_id, &encrypted_credential) else {
            return BrokerCommitOutcome::InvalidCredential;
        };
        if !valid_user_key_bytes(&plaintext) {
            return BrokerCommitOutcome::InvalidCredential;
        }
        with_user_key_engine_mutation(user_id, || {
            store_credential_at(
                Path::new(USER_CREDENTIAL_ROOT),
                user_id,
                &encrypted_credential,
                expected_existing,
            )
        })
        .map_or(BrokerCommitOutcome::Conflict, |()| {
            BrokerCommitOutcome::Stored
        })
    })
    .await;
    complete_operation(&lease_id, user_id, OperationState::Committing);
    match stored {
        Ok(BrokerCommitOutcome::Stored) => {}
        Ok(BrokerCommitOutcome::InvalidCredential) => {
            return mutation_response(StatusCode::UNPROCESSABLE_ENTITY, false);
        }
        Ok(BrokerCommitOutcome::Conflict) => {
            return mutation_response(StatusCode::CONFLICT, false);
        }
        Err(_) => {
            return mutation_response(StatusCode::INTERNAL_SERVER_ERROR, false);
        }
    }

    crate::resident::bump_hosted_authority_generation();
    mutation_response(StatusCode::OK, true)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BrokerCommitOutcome {
    Stored,
    InvalidCredential,
    Conflict,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrokerDecisionRequest {
    lease_id: String,
    decision: String,
}

pub(crate) async fn broker_decision(
    Extension(client): Extension<crate::control_plane::RequestClient>,
    Json(request): Json<BrokerDecisionRequest>,
) -> (StatusCode, Json<BrokerMutationResponse>) {
    if !valid_lease_id(&request.lease_id) {
        return mutation_response(StatusCode::BAD_REQUEST, false);
    }
    match request.decision.as_str() {
        "cancel" | "abort" => {
            match consume_presented_operation(&request.lease_id, client.user_id(), Instant::now()) {
                Some(_) => mutation_response(StatusCode::OK, true),
                None => mutation_response(StatusCode::GONE, false),
            }
        }
        "remove" => {
            let operation =
                match begin_remove_operation(&request.lease_id, client.user_id(), Instant::now()) {
                    Ok(operation) => operation,
                    Err(BeginOperationError::WrongAction) => {
                        return mutation_response(StatusCode::BAD_REQUEST, false);
                    }
                    Err(BeginOperationError::Unavailable) => {
                        return mutation_response(StatusCode::GONE, false);
                    }
                };
            debug_assert_eq!(operation.action, ProvisionAction::Remove);
            // Persist the safe local route before unlinking authorization. A
            // crash can therefore leave an unused ciphertext, never a selected
            // hosted engine with missing authorization. The UID mutation lock
            // spans both writes, so engine selection cannot re-enable this
            // route between the fail-safe write and credential deletion.
            let removed = remove_credential_transaction_at(
                Path::new(USER_CREDENTIAL_ROOT),
                client.user_id(),
                || crate::openai_key::fail_safe_user_api_to_local(client.user_id()),
            );
            complete_operation(
                &request.lease_id,
                client.user_id(),
                OperationState::Removing,
            );
            if removed.is_err() {
                return mutation_response(StatusCode::INTERNAL_SERVER_ERROR, false);
            }
            crate::resident::bump_hosted_authority_generation();
            mutation_response(StatusCode::OK, true)
        }
        _ => mutation_response(StatusCode::BAD_REQUEST, false),
    }
}

fn mutation_response(status: StatusCode, ok: bool) -> (StatusCode, Json<BrokerMutationResponse>) {
    (status, Json(BrokerMutationResponse { ok }))
}

pub(crate) fn credential_is_stored(user_id: u32) -> bool {
    credential_is_stored_at(Path::new(USER_CREDENTIAL_ROOT), user_id)
}

/// Return only whether this authenticated user's key-management operation is
/// pending or in flight. Action, lease, timestamps, and other users are never
/// exposed through status responses.
pub(crate) fn management_pending(user_id: u32) -> bool {
    let mut entries = operation_store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune_operations(&mut entries, Instant::now());
    entries
        .values()
        .any(|operation| operation.intended_user_id == user_id)
}

pub(crate) fn decrypt_user_credential(user_id: u32) -> Option<UserApiKey> {
    with_user_key_engine_mutation(user_id, || {
        let ciphertext = read_credential_at(Path::new(USER_CREDENTIAL_ROOT), user_id)?;
        let plaintext = decrypt_encrypted_credential(user_id, &ciphertext)?;
        valid_user_key_bytes(&plaintext).then_some(UserApiKey(plaintext))
    })
}

fn decrypt_encrypted_credential(
    user_id: u32,
    encrypted_credential: &[u8],
) -> Option<Zeroizing<Vec<u8>>> {
    let uid_argument = format!("--uid={user_id}");
    let name_argument = format!("--name={CREDENTIAL_NAME}");
    sensitive_command_output(
        SYSTEMD_CREDS,
        &[
            "--no-ask-password",
            "--quiet",
            "--user",
            &uid_argument,
            &name_argument,
            "--refuse-null",
            "decrypt",
            "-",
            "-",
        ],
        encrypted_credential,
        CREDENTIAL_COMMAND_TIMEOUT,
    )
}

fn sensitive_command_output(
    binary: &str,
    args: &[&str],
    input: &[u8],
    timeout: Duration,
) -> Option<Zeroizing<Vec<u8>>> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // Isolate the utility and anything it forks. Even a successful direct
        // child may leave a descendant holding stdout open, so every terminal
        // path kills this dedicated process group before accepting output.
        .process_group(0);
    let mut child = command.spawn().ok()?;
    let process_group = child.id();
    let mut stdin = Some(child.stdin.take()?);
    let mut stdout = child.stdout.take()?;
    if set_nonblocking(stdin.as_ref()?.as_raw_fd()).is_err()
        || set_nonblocking(stdout.as_raw_fd()).is_err()
    {
        terminate_process_group(&mut child, process_group);
        return None;
    }

    let input = Zeroizing::new(input.to_vec());
    let mut input_offset = 0;
    let mut output = Zeroizing::new(Vec::with_capacity(MAX_COMMAND_STDOUT_BYTES));
    let started = Instant::now();
    loop {
        if pump_stdin(&mut stdin, &input, &mut input_offset).is_err()
            || pump_stdout(&mut stdout, &mut output, MAX_COMMAND_STDOUT_BYTES).is_err()
        {
            drop(stdin);
            terminate_process_group(&mut child, process_group);
            return None;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                drop(stdin);
                terminate_process_group(&mut child, process_group);
                if !status.success() || input_offset != input.len() {
                    return None;
                }
                return drain_stdout_after_exit(
                    stdout,
                    output,
                    MAX_COMMAND_STDOUT_BYTES,
                    COMMAND_CLEANUP_TIMEOUT,
                );
            }
            Ok(None) if started.elapsed() < timeout => thread::sleep(COMMAND_POLL_INTERVAL),
            Ok(None) | Err(_) => {
                drop(stdin);
                terminate_process_group(&mut child, process_group);
                return None;
            }
        }
    }
}

fn set_nonblocking(descriptor: libc::c_int) -> io::Result<()> {
    // SAFETY: fcntl operates on an owned live pipe fd, and neither operation
    // dereferences a pointer.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn pump_stdin(stdin: &mut Option<ChildStdin>, input: &[u8], offset: &mut usize) -> io::Result<()> {
    while *offset < input.len() {
        let pipe = stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "credential pipe closed"))?;
        match pipe.write(&input[*offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "credential pipe closed",
                ))
            }
            Ok(written) => *offset += written,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error),
        }
    }
    if *offset == input.len() {
        stdin.take();
    }
    Ok(())
}

fn pump_stdout(
    stdout: &mut ChildStdout,
    output: &mut Zeroizing<Vec<u8>>,
    limit: usize,
) -> io::Result<bool> {
    let mut chunk = [0_u8; 4096];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => return Ok(true),
            Ok(read) => {
                let remaining = limit.saturating_sub(output.len());
                if read > remaining {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "credential command output exceeded its private bound",
                    ));
                }
                output.extend_from_slice(&chunk[..read]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error),
        }
    }
}

fn drain_stdout_after_exit(
    mut stdout: ChildStdout,
    mut output: Zeroizing<Vec<u8>>,
    limit: usize,
    timeout: Duration,
) -> Option<Zeroizing<Vec<u8>>> {
    let started = Instant::now();
    loop {
        match pump_stdout(&mut stdout, &mut output, limit) {
            Ok(true) => return Some(output),
            Ok(false) if started.elapsed() < timeout => thread::sleep(COMMAND_POLL_INTERVAL),
            Ok(false) | Err(_) => return None,
        }
    }
}

fn terminate_process_group(child: &mut Child, process_group: u32) {
    if let Ok(process_group) = libc::pid_t::try_from(process_group) {
        // SAFETY: a negative pid targets only the dedicated child process group.
        // SIGKILL is required because descendants may retain secret-bearing pipes.
        unsafe {
            libc::kill(-process_group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if started.elapsed() < COMMAND_CLEANUP_TIMEOUT => {
                thread::sleep(COMMAND_POLL_INTERVAL);
            }
            Ok(None) | Err(_) => return,
        }
    }
}

fn valid_user_key_bytes(key: &[u8]) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_BYTES
        && key
            .iter()
            .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
}

fn valid_encrypted_credential(value: &[u8]) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ENCRYPTED_CREDENTIAL_BYTES
        && value.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(*byte, b'+' | b'/' | b'=' | b'\n' | b'\r')
        })
}

fn insert_pending_operation(
    user_id: u32,
    action: ProvisionAction,
    configured: bool,
    now: Instant,
) -> Option<String> {
    let mut entries = operation_store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune_operations(&mut entries, now);
    if entries
        .values()
        .any(|entry| entry.intended_user_id == user_id)
    {
        return None;
    }
    loop {
        let mut random = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut random);
        let lease_id: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        if entries.contains_key(&lease_id) {
            continue;
        }
        entries.insert(
            lease_id.clone(),
            PendingOperation {
                intended_user_id: user_id,
                action,
                configured_at_request: configured,
                state: OperationState::Pending,
                expires_at: now + OPERATION_TTL,
            },
        );
        return Some(lease_id);
    }
}

fn claim_operation(user_id: u32, now: Instant) -> Option<(String, PendingOperation)> {
    let mut entries = operation_store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune_operations(&mut entries, now);
    let mut matches = entries
        .iter()
        .filter(|(_, entry)| {
            entry.intended_user_id == user_id && entry.state == OperationState::Pending
        })
        .map(|(lease_id, _)| lease_id.clone());
    let lease_id = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let entry = entries.get_mut(&lease_id)?;
    entry.state = OperationState::Presented;
    Some((lease_id, entry.clone()))
}

fn begin_commit_operation(
    lease_id: &str,
    user_id: u32,
    now: Instant,
) -> Result<PendingOperation, BeginOperationError> {
    begin_presented_operation(
        lease_id,
        user_id,
        now,
        OperationState::Committing,
        |action| matches!(action, ProvisionAction::Add | ProvisionAction::Rotate),
    )
}

fn begin_remove_operation(
    lease_id: &str,
    user_id: u32,
    now: Instant,
) -> Result<PendingOperation, BeginOperationError> {
    begin_presented_operation(lease_id, user_id, now, OperationState::Removing, |action| {
        action == ProvisionAction::Remove
    })
}

fn begin_presented_operation(
    lease_id: &str,
    user_id: u32,
    now: Instant,
    target: OperationState,
    action_allowed: impl FnOnce(ProvisionAction) -> bool,
) -> Result<PendingOperation, BeginOperationError> {
    let mut entries = operation_store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune_operations(&mut entries, now);
    let entry = entries
        .get_mut(lease_id)
        .filter(|entry| {
            entry.intended_user_id == user_id
                && entry.state == OperationState::Presented
                && entry.expires_at > now
        })
        .ok_or(BeginOperationError::Unavailable)?;
    if !action_allowed(entry.action) {
        return Err(BeginOperationError::WrongAction);
    }
    entry.state = target;
    entry.expires_at = now + IN_FLIGHT_OPERATION_TTL;
    Ok(entry.clone())
}

fn consume_presented_operation(
    lease_id: &str,
    user_id: u32,
    now: Instant,
) -> Option<PendingOperation> {
    let mut entries = operation_store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune_operations(&mut entries, now);
    let operation = entries
        .get(lease_id)
        .filter(|entry| {
            entry.intended_user_id == user_id
                && entry.state == OperationState::Presented
                && entry.expires_at > now
        })?
        .clone();
    entries.remove(lease_id);
    Some(operation)
}

fn complete_operation(lease_id: &str, user_id: u32, state: OperationState) {
    let mut entries = operation_store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if entries
        .get(lease_id)
        .is_some_and(|operation| operation.intended_user_id == user_id && operation.state == state)
    {
        entries.remove(lease_id);
    }
}

fn prune_operations(entries: &mut BTreeMap<String, PendingOperation>, now: Instant) {
    entries.retain(|_, entry| entry.expires_at > now);
}

fn remove_operation(lease_id: &str) {
    operation_store()
        .entries
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(lease_id);
}

fn valid_lease_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn credential_is_stored_at(root: &Path, user_id: u32) -> bool {
    open_credential_root_at(root, false)
        .and_then(|root| open_user_credential_directory(&root, user_id, false))
        .and_then(|user| credential_entry_state(&user))
        .unwrap_or(false)
}

fn read_credential_at(root: &Path, user_id: u32) -> Option<Vec<u8>> {
    let root = open_credential_root_at(root, false).ok()?;
    let user = open_user_credential_directory(&root, user_id, false).ok()?;
    let mut file = open_valid_credential_file(&user).ok()?;
    let opened = credential_file_identity(&file.metadata().ok()?).ok()?;
    let mut bytes = Vec::with_capacity(opened.length as usize);
    Read::by_ref(&mut file)
        .take((MAX_ENCRYPTED_CREDENTIAL_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    let after = credential_file_identity(&file.metadata().ok()?).ok()?;
    (opened == after && bytes.len() as u64 == opened.length && valid_encrypted_credential(&bytes))
        .then_some(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CredentialFileIdentity {
    device: u64,
    inode: u64,
    length: u64,
}

fn credential_file_identity(metadata: &Metadata) -> io::Result<CredentialFileIdentity> {
    credential_file_identity_allowing_empty(metadata, false)
}

fn credential_file_identity_allowing_empty(
    metadata: &Metadata,
    allow_empty: bool,
) -> io::Result<CredentialFileIdentity> {
    // A single-link requirement prevents a protected pathname from being used
    // as a handle to a file with another mutable name elsewhere.
    if !metadata.is_file()
        || metadata.uid() != effective_user_id()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
        || (!allow_empty && metadata.len() == 0)
        || metadata.len() > MAX_ENCRYPTED_CREDENTIAL_BYTES as u64
    {
        return Err(private_storage_error(
            "credential must be one core-owned mode-0600 regular file",
        ));
    }
    Ok(CredentialFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
    })
}

fn open_valid_credential_file(user: &Dir) -> io::Result<cap_std::fs::File> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = user.open_with(CREDENTIAL_FILE_NAME, &options)?;
    credential_file_identity(&file.metadata()?)?;
    Ok(file)
}

fn credential_entry_state(user: &Dir) -> io::Result<bool> {
    match open_valid_credential_file(user) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn open_credential_root_at(path: &Path, create: bool) -> io::Result<Dir> {
    match open_private_ambient_directory(path) {
        Ok(directory) => return Ok(directory),
        Err(error) if create && error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent_path = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "protected credential parent directory is unavailable",
        )
    })?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "invalid credential root name")
    })?;
    let parent = open_private_ambient_directory(parent_path)?;
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    match parent.create_dir_with(name, &builder) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let directory = parent.open_dir_nofollow(name)?;
    validate_private_directory(&directory.dir_metadata()?)?;
    Ok(directory)
}

fn open_private_ambient_directory(path: &Path) -> io::Result<Dir> {
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let directory = options.open(path)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir()
        || metadata.uid() != effective_user_id()
        || metadata.permissions().mode() & 0o7777 != 0o700
    {
        return Err(private_storage_error(
            "credential root must be a core-owned mode-0700 directory",
        ));
    }
    Ok(Dir::from_std_file(directory))
}

fn open_user_credential_directory(root: &Dir, user_id: u32, create: bool) -> io::Result<Dir> {
    let name = user_id.to_string();
    let directory = match root.open_dir_nofollow(&name) {
        Ok(directory) => directory,
        Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
            let mut builder = DirBuilder::new();
            builder.mode(0o700);
            match root.create_dir_with(&name, &builder) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            root.open_dir_nofollow(&name)?
        }
        Err(error) => return Err(error),
    };
    validate_private_directory(&directory.dir_metadata()?)?;
    Ok(directory)
}

fn validate_private_directory(metadata: &Metadata) -> io::Result<()> {
    if !metadata.is_dir()
        || metadata.uid() != effective_user_id()
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(private_storage_error(
            "credential directory must be core-owned with mode 0700",
        ));
    }
    Ok(())
}

fn effective_user_id() -> u32 {
    // SAFETY: geteuid takes no arguments and has no preconditions.
    unsafe { libc::geteuid() }
}

fn private_storage_error(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn store_credential_at(
    root: &Path,
    user_id: u32,
    encrypted_credential: &[u8],
    expected_existing: bool,
) -> std::io::Result<()> {
    if !valid_encrypted_credential(encrypted_credential) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid encrypted credential envelope",
        ));
    }
    let root = open_credential_root_at(root, true)?;
    let user = open_user_credential_directory(&root, user_id, true)?;
    if credential_entry_state(&user)? != expected_existing {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "credential state changed during provisioning",
        ));
    }
    store_credential_in_open_directory(&user, encrypted_credential, expected_existing)
}

fn remove_credential_at(root: &Path, user_id: u32) -> std::io::Result<()> {
    let root = open_credential_root_at(root, false)?;
    let user = match open_user_credential_directory(&root, user_id, false) {
        Ok(user) => user,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !credential_entry_state(&user)? {
        return Ok(());
    }
    user.remove_file(CREDENTIAL_FILE_NAME)?;
    sync_capability_directory(&user)
}

fn remove_credential_transaction_at(
    root: &Path,
    user_id: u32,
    fail_safe_engine: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<()> {
    with_user_key_engine_mutation(user_id, || {
        fail_safe_engine()?;
        remove_credential_at(root, user_id)
    })
}

fn store_credential_in_open_directory(
    user: &Dir,
    encrypted_credential: &[u8],
    expected_existing: bool,
) -> io::Result<()> {
    let (temporary, mut file) = create_temporary_credential(user)?;
    let result = (|| {
        let created = credential_file_identity_allowing_empty(&file.metadata()?, true)?;
        file.write_all(encrypted_credential)?;
        file.sync_all()?;
        let written = credential_file_identity(&file.metadata()?)?;
        if written.device != created.device
            || written.inode != created.inode
            || written.length != encrypted_credential.len() as u64
        {
            return Err(private_storage_error(
                "temporary credential changed while it was written",
            ));
        }
        drop(file);
        if expected_existing {
            user.rename(&temporary, user, CREDENTIAL_FILE_NAME)?;
        } else {
            rename_noreplace(
                user,
                OsStr::new(&temporary),
                OsStr::new(CREDENTIAL_FILE_NAME),
            )?;
        }
        let installed = open_valid_credential_file(user)?;
        if credential_file_identity(&installed.metadata()?)? != written {
            return Err(private_storage_error(
                "installed credential changed during atomic replacement",
            ));
        }
        sync_capability_directory(user)
    })();
    if result.is_err() {
        let _ = user.remove_file(&temporary);
    }
    result
}

fn create_temporary_credential(user: &Dir) -> io::Result<(String, cap_std::fs::File)> {
    for _ in 0..16 {
        let temporary = format!(
            ".{CREDENTIAL_FILE_NAME}.{}.{:016x}.tmp",
            std::process::id(),
            rand::random::<u64>()
        );
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .follow(FollowSymlinks::No);
        match user.open_with(&temporary, &options) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a private credential staging file",
    ))
}

fn rename_noreplace(directory: &Dir, source: &OsStr, destination: &OsStr) -> io::Result<()> {
    let source = CString::new(source.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid credential filename"))?;
    let destination = CString::new(destination.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid credential filename"))?;

    #[cfg(target_os = "linux")]
    // SAFETY: both names are relative NUL-terminated strings, both directory
    // descriptors are the same held capability, and RENAME_NOREPLACE prevents
    // an attacker-controlled late destination from being overwritten.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };

    #[cfg(target_vendor = "apple")]
    // SAFETY: this is the Apple atomic no-replace counterpart to renameat2,
    // using the same held directory capability for both relative names.
    let result = unsafe {
        libc::renameatx_np(
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_EXCL,
        ) as libc::c_long
    };

    #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
    let result = {
        directory.hard_link(
            OsStr::from_bytes(source.as_bytes()),
            directory,
            OsStr::from_bytes(destination.as_bytes()),
        )?;
        directory.remove_file(OsStr::from_bytes(source.as_bytes()))?;
        0
    };

    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn sync_capability_directory(directory: &Dir) -> io::Result<()> {
    // cap-std may hold an O_PATH descriptor on Linux, which cannot be fsynced.
    // Open "." relative to the already validated capability and sync that exact
    // directory without resolving an ambient pathname again.
    // SAFETY: the path is static and NUL terminated; successful openat transfers
    // ownership of one new descriptor to File.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: descriptor is newly owned after successful openat.
    unsafe { fs::File::from_raw_fd(descriptor) }.sync_all()
}

#[cfg(test)]
mod tests {
    use super::{
        begin_commit_operation, begin_remove_operation, claim_operation, complete_operation,
        consume_presented_operation, credential_is_stored_at, insert_pending_operation,
        management_pending, open_credential_root_at, open_user_credential_directory,
        operation_store, read_credential_at, remove_credential_at,
        remove_credential_transaction_at, remove_operation, sensitive_command_output,
        store_credential_at, store_credential_in_open_directory, valid_encrypted_credential,
        valid_user_key_bytes, with_user_key_engine_mutation, OperationState, ProvisionAction,
    };
    use std::{
        os::unix::fs::{symlink, PermissionsExt},
        path::PathBuf,
        sync::{mpsc, Arc, Barrier, Mutex, MutexGuard, OnceLock},
        thread,
        time::{Duration, Instant},
    };

    const USER_A: u32 = 1000;
    const USER_B: u32 = 1001;
    const TEST_ENVELOPE: &[u8] = b"R09CTElOU19PU19URVNUX0VOQ1JZUFRFRF9FTlZFTE9QRV9WMQ==\n";

    fn isolation() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        operation_store()
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        guard
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "goblins-openai-key-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn pending_operation_is_single_use_uid_bound_and_expires() {
        let _guard = isolation();
        let now = Instant::now();
        let lease = insert_pending_operation(USER_A, ProvisionAction::Add, false, now)
            .expect("first operation");
        assert!(insert_pending_operation(USER_A, ProvisionAction::Add, false, now).is_none());
        assert!(claim_operation(USER_B, now).is_none());
        let (_, claim) = claim_operation(USER_A, now).expect("intended user claims");
        assert_eq!(claim.action, ProvisionAction::Add);
        assert_eq!(claim.state, OperationState::Presented);
        assert!(claim_operation(USER_A, now).is_none());
        assert!(management_pending(USER_A));
        assert!(!management_pending(USER_B));
        assert!(begin_commit_operation(&lease, USER_B, now).is_err());
        let commit = begin_commit_operation(&lease, USER_A, now).expect("one commit begins");
        assert_eq!(commit.state, OperationState::Committing);
        assert!(begin_commit_operation(&lease, USER_A, now).is_err());
        assert!(consume_presented_operation(&lease, USER_A, now).is_none());
        complete_operation(&lease, USER_A, OperationState::Committing);
        assert!(!management_pending(USER_A));

        let expired = insert_pending_operation(USER_A, ProvisionAction::Add, false, now)
            .expect("expiring operation");
        claim_operation(USER_A, now).expect("present expiring operation");
        assert!(begin_commit_operation(&expired, USER_A, now + Duration::from_secs(301)).is_err());
        assert!(!management_pending(USER_A));

        let abandoned = insert_pending_operation(USER_A, ProvisionAction::Add, false, now)
            .expect("abandoned in-flight operation");
        claim_operation(USER_A, now).unwrap();
        begin_commit_operation(&abandoned, USER_A, now).unwrap();
        assert!(insert_pending_operation(
            USER_A,
            ProvisionAction::Add,
            false,
            now + Duration::from_secs(31)
        )
        .is_some());
    }

    #[test]
    fn presented_lease_has_exactly_one_terminal_transition_under_race() {
        let _guard = isolation();
        let now = Instant::now();
        let lease = insert_pending_operation(USER_A, ProvisionAction::Add, false, now).unwrap();
        claim_operation(USER_A, now).unwrap();
        let barrier = Arc::new(Barrier::new(9));
        let mut racers = Vec::new();
        for _ in 0..8 {
            let barrier = Arc::clone(&barrier);
            let lease = lease.clone();
            racers.push(thread::spawn(move || {
                barrier.wait();
                begin_commit_operation(&lease, USER_A, now).is_ok()
            }));
        }
        barrier.wait();
        assert_eq!(
            racers
                .into_iter()
                .map(|racer| racer.join().expect("commit racer"))
                .filter(|won| *won)
                .count(),
            1
        );
        assert!(consume_presented_operation(&lease, USER_A, now).is_none());
        complete_operation(&lease, USER_A, OperationState::Committing);

        let lease = insert_pending_operation(USER_A, ProvisionAction::Remove, true, now).unwrap();
        claim_operation(USER_A, now).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let remove = {
            let barrier = Arc::clone(&barrier);
            let lease = lease.clone();
            thread::spawn(move || {
                barrier.wait();
                begin_remove_operation(&lease, USER_A, now).is_ok()
            })
        };
        let cancel = {
            let barrier = Arc::clone(&barrier);
            let lease = lease.clone();
            thread::spawn(move || {
                barrier.wait();
                consume_presented_operation(&lease, USER_A, now).is_some()
            })
        };
        barrier.wait();
        assert_ne!(
            remove.join().expect("remove racer"),
            cancel.join().expect("cancel racer")
        );
        complete_operation(&lease, USER_A, OperationState::Removing);
    }

    #[test]
    fn removal_serializes_engine_reselection_through_credential_deletion() {
        let root = temp_root("remove-selection-race");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        store_credential_at(&root, USER_A, TEST_ENVELOPE, false).unwrap();

        let selected = Arc::new(Mutex::new("openai-api"));
        let release = Arc::new(Barrier::new(2));
        let (fail_safe_entered_tx, fail_safe_entered_rx) = mpsc::channel();
        let remove_root = root.clone();
        let remove_selected = Arc::clone(&selected);
        let remove_release = Arc::clone(&release);
        let remover = thread::spawn(move || {
            remove_credential_transaction_at(&remove_root, USER_A, || {
                *remove_selected
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = "local-gpt-oss";
                fail_safe_entered_tx.send(()).unwrap();
                remove_release.wait();
                Ok(())
            })
            .unwrap();
        });
        fail_safe_entered_rx.recv().unwrap();

        let (selection_attempted_tx, selection_attempted_rx) = mpsc::channel();
        let (selection_finished_tx, selection_finished_rx) = mpsc::channel();
        let select_root = root.clone();
        let select_selected = Arc::clone(&selected);
        let selector = thread::spawn(move || {
            selection_attempted_tx.send(()).unwrap();
            let configured = with_user_key_engine_mutation(USER_A, || {
                let configured = credential_is_stored_at(&select_root, USER_A);
                if configured {
                    *select_selected
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = "openai-api";
                }
                configured
            });
            selection_finished_tx.send(configured).unwrap();
        });
        selection_attempted_rx.recv().unwrap();
        assert!(selection_finished_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        release.wait();
        remover.join().unwrap();
        assert!(!selection_finished_rx.recv().unwrap());
        selector.join().unwrap();
        assert!(!credential_is_stored_at(&root, USER_A));
        assert_eq!(
            *selected
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            "local-gpt-oss"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn credential_store_is_per_uid_private_atomic_and_state_guarded() {
        let root = temp_root("store");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();

        store_credential_at(&root, USER_A, TEST_ENVELOPE, false).expect("add credential");
        assert!(credential_is_stored_at(&root, USER_A));
        assert!(!credential_is_stored_at(&root, USER_B));
        assert_eq!(
            read_credential_at(&root, USER_A).as_deref(),
            Some(TEST_ENVELOPE)
        );
        assert!(store_credential_at(&root, USER_A, b"QUJDRA==\n", false).is_err());
        assert_eq!(
            read_credential_at(&root, USER_A).as_deref(),
            Some(TEST_ENVELOPE)
        );

        store_credential_at(&root, USER_A, b"QUJDRA==\n", true).expect("rotate atomically");
        assert_eq!(
            read_credential_at(&root, USER_A).as_deref(),
            Some(b"QUJDRA==\n".as_slice())
        );
        let metadata = std::fs::metadata(root.join("1000/openai-api-key.cred")).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);

        remove_credential_at(&root, USER_A).expect("remove credential");
        remove_credential_at(&root, USER_A).expect("repeated removal is idempotent");
        assert!(!credential_is_stored_at(&root, USER_A));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn credential_store_rejects_symlink_destination() {
        let root = temp_root("symlink");
        let user = root.join(USER_A.to_string());
        std::fs::create_dir_all(&user).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = root.join("target");
        std::fs::write(&target, TEST_ENVELOPE).unwrap();
        symlink(&target, user.join("openai-api-key.cred")).unwrap();
        assert!(store_credential_at(&root, USER_A, TEST_ENVELOPE, false).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), TEST_ENVELOPE);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn credential_capability_rejects_linked_directories_and_root_replacement() {
        let parent = temp_root("linked-directories");
        let real_root = parent.join("real-root");
        let linked_root = parent.join("linked-root");
        let outside = parent.join("outside");
        std::fs::create_dir_all(&real_root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        for directory in [&parent, &real_root, &outside] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        symlink(&real_root, &linked_root).unwrap();
        assert!(store_credential_at(&linked_root, USER_A, TEST_ENVELOPE, false).is_err());

        symlink(&outside, real_root.join(USER_A.to_string())).unwrap();
        assert!(store_credential_at(&real_root, USER_A, TEST_ENVELOPE, false).is_err());
        assert!(!outside.join("openai-api-key.cred").exists());
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn held_directory_capability_survives_path_replacement_without_escape() {
        let root = temp_root("held-directory");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root_capability = open_credential_root_at(&root, false).unwrap();
        let user = open_user_credential_directory(&root_capability, USER_A, true).unwrap();

        let detached = root.join("detached-user");
        let replacement = root.join("replacement-target");
        std::fs::create_dir(&replacement).unwrap();
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::rename(root.join(USER_A.to_string()), &detached).unwrap();
        symlink(&replacement, root.join(USER_A.to_string())).unwrap();

        store_credential_in_open_directory(&user, TEST_ENVELOPE, false).unwrap();
        assert_eq!(
            std::fs::read(detached.join("openai-api-key.cred")).unwrap(),
            TEST_ENVELOPE
        );
        assert!(!replacement.join("openai-api-key.cred").exists());
        assert!(!credential_is_stored_at(&root, USER_A));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn credential_remove_rejects_symlink_without_touching_its_target() {
        let root = temp_root("remove-symlink");
        let user = root.join(USER_A.to_string());
        std::fs::create_dir_all(&user).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = root.join("target");
        std::fs::write(&target, TEST_ENVELOPE).unwrap();
        symlink(&target, user.join("openai-api-key.cred")).unwrap();
        assert!(remove_credential_at(&root, USER_A).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), TEST_ENVELOPE);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sensitive_subprocess_lifecycle_is_bounded_on_every_terminal_path() {
        let started = Instant::now();
        let success = sensitive_command_output(
            "/bin/sh",
            &[
                "-c",
                "value=$(cat); [ \"$value\" = fixture ] || exit 9; (sleep 30) & printf ok; exit 0",
            ],
            b"fixture",
            Duration::from_secs(2),
        );
        assert_eq!(
            success.as_ref().map(|value| value.as_slice()),
            Some(b"ok".as_slice())
        );
        assert!(started.elapsed() < Duration::from_secs(3));

        for (script, timeout) in [
            ("(sleep 30) & exit 7", Duration::from_secs(2)),
            (
                "i=0; while [ $i -lt 600 ]; do printf A; i=$((i+1)); done; sleep 30",
                Duration::from_secs(2),
            ),
            ("sleep 30", Duration::from_millis(100)),
        ] {
            let started = Instant::now();
            assert!(
                sensitive_command_output("/bin/sh", &["-c", script], b"fixture", timeout).is_none()
            );
            assert!(started.elapsed() < Duration::from_secs(3));
        }
    }

    #[test]
    fn key_and_ciphertext_validators_are_bounded_and_nonsemantic() {
        assert!(valid_user_key_bytes(b"GOBLINS_OS_TEST_CREDENTIAL_V1"));
        assert!(!valid_user_key_bytes(b"contains whitespace"));
        assert!(!valid_user_key_bytes(&vec![b'x'; 513]));
        assert!(valid_encrypted_credential(TEST_ENVELOPE));
        assert!(!valid_encrypted_credential(b"not ciphertext!"));
    }

    #[test]
    fn lease_format_never_crosses_the_ordinary_management_response() {
        let _guard = isolation();
        let lease = insert_pending_operation(USER_A, ProvisionAction::Remove, true, Instant::now())
            .unwrap();
        assert_eq!(lease.len(), 64);
        remove_operation(&lease);
    }
}
