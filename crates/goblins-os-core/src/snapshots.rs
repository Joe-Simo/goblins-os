//! Local snapshot reporting and additive, no-overwrite file recovery.
//!
//! Snapshots are only truthful when the storage containing the installed home
//! is Btrfs and the Snapper `home` config exists. On the default layout Snapper
//! snapshots the Btrfs root; Recovery deliberately exposes only copies of files
//! inside the logged-in home. Existing XFS installs therefore report an honest
//! off-state instead of inventing a timeline.

use std::{
    env,
    ffi::CString,
    fs,
    io::{self, IoSliceMut, Read, Write},
    net::Shutdown,
    os::{
        fd::{FromRawFd, IntoRawFd, OwnedFd},
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::UnixStream,
        },
    },
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

use axum::{http::StatusCode, Json};
use rustix::net::{recvmsg, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags};
use serde::{Deserialize, Serialize};
use socket2::{Domain, SockAddr, Socket, Type};

use crate::bounded::{bounded_command_output, probe_timeout};
use crate::session_bridge::{recover_snapshot_file, SnapshotRecoveryBridgeResult};

const DEFAULT_MOUNTINFO: &str = "/proc/self/mountinfo";
const DEFAULT_SNAPPER_CONFIG: &str = "/etc/snapper/configs/home";
const RESTORE_CONFIRMATION: &str = "RECOVER FILE COPY";
const MAX_RECOVERY_PATH_BYTES: usize = 4 * 1024;
const SNAPSHOT_BROKER_SOCKET: &str = "/run/goblins-os-snapshots/broker.sock";
const SNAPSHOT_BROKER_READER_GROUP: &str = "goblins-snapshot-readers";
const SNAPSHOT_BROKER_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BROWSE_CURSOR: usize = 30_000;
const MAX_BROKER_REQUEST_BYTES: usize = 16 * 1024;
const MAX_BROKER_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_RECOVERY_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Serialize)]
pub struct SnapshotsStatus {
    source: &'static str,
    available: bool,
    snapper_available: bool,
    btrfs_tools_available: bool,
    config_available: bool,
    config_path: String,
    snapshot_subvolume: Option<String>,
    home_mount: Option<SnapshotMount>,
    snapshots: Vec<SnapshotRecord>,
    restore_ready: bool,
    executes_restore: bool,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SnapshotMount {
    mount_point: String,
    filesystem: String,
    source: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct SnapshotRecord {
    id: String,
    kind: String,
    date: Option<String>,
    user: Option<String>,
    cleanup: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreSnapshotRequest {
    snapshot_id: String,
    path: String,
    destination_directory: String,
    confirmation: String,
}

#[derive(Serialize)]
pub struct RestoreSnapshotOutcome {
    ok: bool,
    executes_restore: bool,
    snapshot_id: String,
    path: String,
    destination_path: Option<String>,
    bytes_copied: u64,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseSnapshotRequest {
    snapshot_id: String,
    #[serde(default)]
    directory: String,
    #[serde(default)]
    cursor: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotBrowseEntry {
    name: String,
    relative_path: String,
    logical_path: String,
    kind: String,
    bytes: Option<u64>,
}

#[derive(Serialize)]
pub struct BrowseSnapshotOutcome {
    ok: bool,
    snapshot_id: String,
    directory: String,
    entries: Vec<SnapshotBrowseEntry>,
    truncated: bool,
    next_cursor: Option<usize>,
    detail: String,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum SnapshotBrokerRequest<'a> {
    ListSnapshots,
    Browse {
        snapshot_id: &'a str,
        snapshot_subvolume: &'a str,
        directory: &'a str,
        cursor: usize,
    },
    OpenFile {
        snapshot_id: &'a str,
        snapshot_subvolume: &'a str,
        file: &'a str,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotBrokerResponse {
    ok: bool,
    snapshots: Vec<SnapshotRecord>,
    entries: Vec<SnapshotBrowseEntry>,
    truncated: bool,
    source_bytes: Option<u64>,
    next_cursor: Option<usize>,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MountInfoEntry {
    mount_point: String,
    filesystem: String,
    source: String,
}

pub async fn snapshots_status() -> Json<SnapshotsStatus> {
    Json(build_snapshots_status())
}

pub async fn browse_snapshot(
    Json(request): Json<BrowseSnapshotRequest>,
) -> (StatusCode, Json<BrowseSnapshotOutcome>) {
    let snapshot_id = request.snapshot_id.trim().to_string();
    let directory = request.directory.trim_matches('/').to_string();
    let cursor = request.cursor;
    if !valid_snapshot_id(&snapshot_id)
        || !valid_relative_snapshot_path(&directory, true)
        || cursor > MAX_BROWSE_CURSOR
    {
        return browse_response(
            StatusCode::BAD_REQUEST,
            &snapshot_id,
            &directory,
            Vec::new(),
            false,
            None,
            "Choose a reported snapshot and a safe folder inside your snapshot home.",
        );
    }
    let status = build_snapshots_status();
    if !status.restore_ready || !status.executes_restore {
        return browse_response(
            StatusCode::PRECONDITION_FAILED,
            &snapshot_id,
            &directory,
            Vec::new(),
            false,
            None,
            "Local snapshot browsing is not ready on this device.",
        );
    }
    if !status
        .snapshots
        .iter()
        .any(|snapshot| snapshot.id == snapshot_id)
    {
        return browse_response(
            StatusCode::NOT_FOUND,
            &snapshot_id,
            &directory,
            Vec::new(),
            false,
            None,
            "The selected snapshot is no longer available. Refresh Recovery.",
        );
    }
    let Some(snapshot_subvolume) = status.snapshot_subvolume else {
        return browse_response(
            StatusCode::PRECONDITION_FAILED,
            &snapshot_id,
            &directory,
            Vec::new(),
            false,
            None,
            "The configured snapshot source could not be verified.",
        );
    };
    let request = SnapshotBrokerRequest::Browse {
        snapshot_id: &snapshot_id,
        snapshot_subvolume: &snapshot_subvolume,
        directory: &directory,
        cursor,
    };
    match snapshot_broker_call(&request, false) {
        Ok((response, None)) if response.ok => browse_response(
            StatusCode::OK,
            &snapshot_id,
            &directory,
            response.entries,
            response.truncated,
            response.next_cursor,
            if response.truncated {
                "Showing one safe page from this snapshot folder. More entries are available."
            } else {
                "Choose a file from this read-only snapshot folder."
            },
        ),
        Ok((response, _)) => browse_response(
            StatusCode::BAD_GATEWAY,
            &snapshot_id,
            &directory,
            Vec::new(),
            false,
            None,
            &response.detail,
        ),
        Err(detail) => browse_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &snapshot_id,
            &directory,
            Vec::new(),
            false,
            None,
            &detail,
        ),
    }
}

fn browse_response(
    status: StatusCode,
    snapshot_id: &str,
    directory: &str,
    entries: Vec<SnapshotBrowseEntry>,
    truncated: bool,
    next_cursor: Option<usize>,
    detail: &str,
) -> (StatusCode, Json<BrowseSnapshotOutcome>) {
    (
        status,
        Json(BrowseSnapshotOutcome {
            ok: status.is_success(),
            snapshot_id: snapshot_id.to_string(),
            directory: directory.to_string(),
            entries,
            truncated,
            next_cursor,
            detail: detail.to_string(),
        }),
    )
}

pub async fn restore_snapshot(
    Json(request): Json<RestoreSnapshotRequest>,
) -> (StatusCode, Json<RestoreSnapshotOutcome>) {
    let snapshot_id = request.snapshot_id.trim().to_string();
    let path = request.path.trim().to_string();
    let destination_directory = request.destination_directory.trim().to_string();

    if snapshot_id.is_empty() || !snapshot_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return restore_response(
            StatusCode::BAD_REQUEST,
            &snapshot_id,
            &path,
            None,
            0,
            "Choose a real Snapper snapshot id before restore.",
        );
    }
    if snapshot_id.len() > 20
        || !valid_recovery_path(&path)
        || !valid_recovery_path(&destination_directory)
    {
        return restore_response(
            StatusCode::BAD_REQUEST,
            &snapshot_id,
            &path,
            None,
            0,
            "Choose one absolute home file path and one absolute destination folder.",
        );
    }
    if request.confirmation.trim() != RESTORE_CONFIRMATION {
        return restore_response(
            StatusCode::BAD_REQUEST,
            &snapshot_id,
            &path,
            None,
            0,
            "File recovery needs the exact confirmation: RECOVER FILE COPY.",
        );
    }
    let current = build_snapshots_status();
    if !current.restore_ready || !current.executes_restore {
        return restore_response(
            StatusCode::PRECONDITION_FAILED,
            &snapshot_id,
            &path,
            None,
            0,
            "Local snapshot recovery is not ready on this device.",
        );
    }
    if !current
        .snapshots
        .iter()
        .any(|snapshot| snapshot.id == snapshot_id)
    {
        return restore_response(
            StatusCode::NOT_FOUND,
            &snapshot_id,
            &path,
            None,
            0,
            "The selected snapshot is no longer available. Refresh Recovery and choose another snapshot.",
        );
    }
    let Some(snapshot_subvolume) = current.snapshot_subvolume else {
        return restore_response(
            StatusCode::PRECONDITION_FAILED,
            &snapshot_id,
            &path,
            None,
            0,
            "Goblins OS could not verify the configured snapshot source.",
        );
    };

    let outcome = tokio::task::spawn_blocking(move || {
        let broker_request = SnapshotBrokerRequest::OpenFile {
            snapshot_id: &snapshot_id,
            snapshot_subvolume: &snapshot_subvolume,
            file: &path,
        };
        let result = match snapshot_broker_call(&broker_request, true) {
            Ok((response, Some(source))) if response.ok => {
                let source_bytes = response.source_bytes.unwrap_or(u64::MAX);
                if source_bytes > MAX_RECOVERY_FILE_BYTES {
                    SnapshotRecoveryBridgeResult::Failed(
                        "The selected snapshot file exceeds the 8 GiB recovery limit.".to_string(),
                    )
                } else {
                    recover_snapshot_file(
                        &snapshot_id,
                        &snapshot_subvolume,
                        &path,
                        &destination_directory,
                        &source,
                        source_bytes,
                    )
                }
            }
            Ok((response, _)) => {
                SnapshotRecoveryBridgeResult::Failed(if response.detail.is_empty() {
                    "The snapshot broker did not provide a verified source file.".to_string()
                } else {
                    response.detail
                })
            }
            Err(detail) => SnapshotRecoveryBridgeResult::Failed(detail),
        };
        (snapshot_id, path, result)
    })
    .await;

    match outcome {
        Ok((snapshot_id, path, SnapshotRecoveryBridgeResult::Success(copy))) => restore_response(
            StatusCode::CREATED,
            &snapshot_id,
            &path,
            Some(copy.destination_path),
            copy.bytes_copied,
            "Recovered a new file copy from the selected snapshot. The current file and every existing destination entry were left unchanged.",
        ),
        Ok((snapshot_id, path, SnapshotRecoveryBridgeResult::Unavailable)) => restore_response(
            StatusCode::SERVICE_UNAVAILABLE,
            &snapshot_id,
            &path,
            None,
            0,
            "The logged-in desktop recovery service is unavailable. Nothing was copied.",
        ),
        Ok((snapshot_id, path, SnapshotRecoveryBridgeResult::Failed(detail))) => restore_response(
            StatusCode::CONFLICT,
            &snapshot_id,
            &path,
            None,
            0,
            &detail,
        ),
        Ok((snapshot_id, path, SnapshotRecoveryBridgeResult::InvalidResponse)) => restore_response(
            StatusCode::BAD_GATEWAY,
            &snapshot_id,
            &path,
            None,
            0,
            "The desktop recovery service returned an invalid result. No recovered copy is reported.",
        ),
        Err(_) => restore_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "",
            "",
            None,
            0,
            "The file-recovery worker stopped unexpectedly. No recovered copy is reported.",
        ),
    }
}

fn valid_recovery_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_RECOVERY_PATH_BYTES
        && !path.as_bytes().contains(&0)
        && Path::new(path).is_absolute()
}

fn valid_snapshot_id(snapshot_id: &str) -> bool {
    !snapshot_id.is_empty()
        && snapshot_id.len() <= 20
        && snapshot_id.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_relative_snapshot_path(path: &str, empty_allowed: bool) -> bool {
    path.len() <= MAX_RECOVERY_PATH_BYTES
        && !path.as_bytes().contains(&0)
        && (empty_allowed || !path.is_empty())
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn snapshot_broker_call(
    request: &SnapshotBrokerRequest<'_>,
    expect_fd: bool,
) -> Result<(SnapshotBrokerResponse, Option<std::fs::File>), String> {
    let socket = snapshot_broker_socket();
    validate_snapshot_broker_socket(&socket)?;
    let request = serde_json::to_vec(request)
        .map_err(|_| "The snapshot broker request could not be encoded.".to_string())?;
    if request.len() > MAX_BROKER_REQUEST_BYTES {
        return Err("The snapshot broker request exceeds its safety limit.".to_string());
    }
    let deadline = Instant::now() + SNAPSHOT_BROKER_TIMEOUT;
    let mut stream = connect_unix_before(&socket, deadline)
        .map_err(|_| "The protected snapshot broker is unavailable.".to_string())?;
    stream
        .set_write_timeout(Some(
            remaining_before(deadline)
                .map_err(|_| "The snapshot broker deadline elapsed.".to_string())?,
        ))
        .map_err(|_| "The snapshot broker transport could not be bounded.".to_string())?;
    stream
        .write_all(&request)
        .map_err(|_| "The snapshot broker request could not be sent.".to_string())?;
    let _ = stream.shutdown(Shutdown::Write);
    let (body, file) = read_broker_response_with_optional_fd(&mut stream, deadline)
        .map_err(|_| "The protected snapshot broker returned an invalid response.".to_string())?;
    let response = serde_json::from_slice::<SnapshotBrokerResponse>(&body)
        .map_err(|_| "The protected snapshot broker returned invalid JSON.".to_string())?;
    if expect_fd != file.is_some() && response.ok {
        return Err(
            "The protected snapshot broker returned the wrong descriptor shape.".to_string(),
        );
    }
    Ok((response, file))
}

fn snapshot_broker_socket() -> PathBuf {
    env::var_os("GOBLINS_OS_SNAPSHOT_BROKER_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(SNAPSHOT_BROKER_SOCKET))
}

fn validate_snapshot_broker_socket(socket: &Path) -> Result<(), String> {
    let expected_gid = resolve_group_id(SNAPSHOT_BROKER_READER_GROUP)
        .ok_or_else(|| "The installed snapshot reader group could not be resolved.".to_string())?;
    let parent = socket
        .parent()
        .ok_or_else(|| "The snapshot broker socket has no parent directory.".to_string())?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|_| "The snapshot broker directory is unavailable.".to_string())?;
    let socket_metadata = fs::symlink_metadata(socket)
        .map_err(|_| "The snapshot broker socket is unavailable.".to_string())?;
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != 0
        || parent_metadata.gid() != expected_gid
        || parent_metadata.permissions().mode() & 0o7777 != 0o750
        || !socket_metadata.file_type().is_socket()
        || socket_metadata.uid() != 0
        || socket_metadata.gid() != expected_gid
        || socket_metadata.permissions().mode() & 0o7777 != 0o660
    {
        return Err("The snapshot broker path failed ownership or mode validation.".to_string());
    }
    Ok(())
}

fn resolve_group_id(name: &str) -> Option<u32> {
    let name = CString::new(name).ok()?;
    let mut record = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0_u8; 16 * 1024];
    let status = unsafe {
        libc::getgrnam_r(
            name.as_ptr(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        None
    } else {
        Some(unsafe { record.assume_init() }.gr_gid)
    }
}

fn connect_unix_before(path: &Path, deadline: Instant) -> io::Result<UnixStream> {
    let address = SockAddr::unix(path)?;
    #[cfg(target_os = "linux")]
    let socket_type = Type::STREAM.cloexec();
    #[cfg(not(target_os = "linux"))]
    let socket_type = Type::STREAM;
    let socket = Socket::new(Domain::UNIX, socket_type, None)?;
    #[cfg(not(target_os = "linux"))]
    socket.set_cloexec(true)?;
    socket.connect_timeout(&address, remaining_before(deadline)?)?;
    socket.set_nonblocking(false)?;
    Ok(socket.into())
}

fn read_broker_response_with_optional_fd(
    stream: &mut UnixStream,
    deadline: Instant,
) -> io::Result<(Vec<u8>, Option<std::fs::File>)> {
    stream.set_read_timeout(Some(remaining_before(deadline)?))?;
    let mut first = vec![0_u8; 16 * 1024];
    let mut iov = [IoSliceMut::new(&mut first)];
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    #[cfg(target_os = "linux")]
    let recv_flags = RecvFlags::CMSG_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let recv_flags = RecvFlags::empty();
    let message = recvmsg(&*stream, &mut iov, &mut control, recv_flags)?;
    let mut received = Vec::<OwnedFd>::new();
    for ancillary in control.drain() {
        if let RecvAncillaryMessage::ScmRights(fds) = ancillary {
            received.extend(fds);
        }
    }
    if received.len() > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "too many broker descriptors",
        ));
    }
    let file = received.pop().map(|fd| {
        // SAFETY: ownership moves from OwnedFd into File exactly once.
        unsafe { std::fs::File::from_raw_fd(fd.into_raw_fd()) }
    });
    first.truncate(message.bytes);
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        stream.set_read_timeout(Some(remaining_before(deadline)?))?;
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if first.len().saturating_add(read) > MAX_BROKER_RESPONSE_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "broker response too large",
                    ));
                }
                first.extend_from_slice(&buffer[..read]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok((first, file))
}

fn remaining_before(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "deadline elapsed"))
}

fn restore_response(
    status: StatusCode,
    snapshot_id: &str,
    path: &str,
    destination_path: Option<String>,
    bytes_copied: u64,
    text: &str,
) -> (StatusCode, Json<RestoreSnapshotOutcome>) {
    (
        status,
        Json(RestoreSnapshotOutcome {
            ok: status.is_success(),
            executes_restore: status.is_success(),
            snapshot_id: snapshot_id.to_string(),
            path: path.to_string(),
            destination_path,
            bytes_copied,
            text: text.to_string(),
        }),
    )
}

fn build_snapshots_status() -> SnapshotsStatus {
    let snapper_available = executable_exists("snapper");
    let btrfs_tools_available = executable_exists("btrfs");
    let config_path = env::var("GOBLINS_OS_SNAPPER_HOME_CONFIG")
        .unwrap_or_else(|_| DEFAULT_SNAPPER_CONFIG.into());
    let config_available = Path::new(&config_path).is_file();
    let configured_snapshot_subvolume = read_snapper_subvolume(Path::new(&config_path));
    let home_mount = read_home_mount().map(|entry| SnapshotMount {
        mount_point: entry.mount_point,
        filesystem: entry.filesystem,
        source: entry.source,
    });
    let snapshot_subvolume = configured_snapshot_subvolume
        .as_deref()
        .filter(|subvolume| {
            let home_uses_btrfs = home_mount
                .as_ref()
                .is_some_and(|mount| mount.filesystem == "btrfs");
            home_uses_btrfs
                && btrfs_tools_available
                && snapshot_subvolume_contract(
                    subvolume,
                    home_mount.as_ref().map(|mount| mount.mount_point.as_str()),
                    btrfs_subvolume_is_verified(subvolume),
                )
        })
        .map(str::to_string);

    let mut blockers = Vec::new();
    if !btrfs_tools_available {
        blockers.push("btrfs tooling is not installed".to_string());
    }
    if !snapper_available {
        blockers.push("snapper is not installed".to_string());
    }
    if !config_available {
        blockers.push(format!("{config_path} is missing"));
    } else if configured_snapshot_subvolume.is_none() {
        blockers.push(format!(
            "{config_path} does not name a supported /, /var, or /var/home Btrfs snapshot source"
        ));
    } else if snapshot_subvolume.is_none() {
        blockers.push(format!(
            "{config_path} does not match a verified Btrfs subvolume at the current home mount target. Goblins OS will not infer snapshot safety from the filesystem type alone"
        ));
    }
    match home_mount.as_ref() {
        Some(mount) if mount.filesystem == "btrfs" => {}
        Some(mount) if mount.filesystem == "xfs" => blockers.push(
            "This existing XFS install remains supported and will never be converted in place. To add local Snapshot Recovery, back up your files and reinstall onto the current Btrfs storage layout, then restore your files into the new install."
                .to_string(),
        ),
        Some(mount) => blockers.push(format!(
            "Local snapshots need Btrfs storage containing your home folder; this system reports {} on {}. Goblins OS will not convert an existing filesystem in place.",
            mount.filesystem, mount.mount_point
        )),
        None => blockers.push(
            "Local snapshots need a readable mountinfo entry for /var/home or /home.".to_string(),
        ),
    }

    if !blockers.is_empty() {
        return SnapshotsStatus {
            source: "goblins-os-core",
            available: false,
            snapper_available,
            btrfs_tools_available,
            config_available,
            config_path,
            snapshot_subvolume,
            home_mount,
            snapshots: Vec::new(),
            restore_ready: false,
            executes_restore: false,
            detail: blockers.join(" "),
        };
    }

    match snapshot_broker_call(&SnapshotBrokerRequest::ListSnapshots, false) {
        Ok((response, None)) if response.ok => {
            let snapshots = response.snapshots;
            let restore_ready = !snapshots.is_empty();
            let detail = if snapshots.is_empty() {
                "Snapper is configured for the Btrfs storage containing your home folder, but it reported no local snapshots yet. On the default Btrfs-root layout the storage snapshot also contains system data; Recovery exposes only safe copies of files inside your home."
                    .to_string()
            } else {
                format!(
                    "Snapper reported {} local storage snapshot(s). Recovery exposes only home files and creates a new copy in a destination you choose.",
                    snapshots.len()
                )
            };
            SnapshotsStatus {
                source: "goblins-os-core",
                available: true,
                snapper_available,
                btrfs_tools_available,
                config_available,
                config_path,
                snapshot_subvolume,
                home_mount,
                snapshots,
                restore_ready,
                executes_restore: restore_ready,
                detail,
            }
        }
        Ok((response, _)) => degraded_snapshots_status(
            snapper_available,
            btrfs_tools_available,
            config_available,
            config_path,
            snapshot_subvolume,
            home_mount,
            if response.detail.is_empty() {
                "The protected snapshot broker could not list storage snapshots."
            } else {
                &response.detail
            },
        ),
        Err(detail) => degraded_snapshots_status(
            snapper_available,
            btrfs_tools_available,
            config_available,
            config_path,
            snapshot_subvolume,
            home_mount,
            &detail,
        ),
    }
}

fn degraded_snapshots_status(
    snapper_available: bool,
    btrfs_tools_available: bool,
    config_available: bool,
    config_path: String,
    snapshot_subvolume: Option<String>,
    home_mount: Option<SnapshotMount>,
    detail: &str,
) -> SnapshotsStatus {
    SnapshotsStatus {
        source: "goblins-os-core",
        available: false,
        snapper_available,
        btrfs_tools_available,
        config_available,
        config_path,
        snapshot_subvolume,
        home_mount,
        snapshots: Vec::new(),
        restore_ready: false,
        executes_restore: false,
        detail: detail.to_string(),
    }
}

fn read_snapper_subvolume(path: &Path) -> Option<String> {
    let config = fs::read_to_string(path).ok()?;
    config.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != "SUBVOLUME" {
            return None;
        }
        let value = value.trim().trim_matches('"');
        matches!(value, "/" | "/var" | "/var/home").then(|| value.to_string())
    })
}

fn snapshot_subvolume_contract(
    configured_subvolume: &str,
    home_mount_target: Option<&str>,
    btrfs_subvolume_verified: bool,
) -> bool {
    matches!(configured_subvolume, "/" | "/var" | "/var/home")
        && btrfs_subvolume_verified
        && home_mount_target.is_some_and(|mount| mount_point_covers(mount, configured_subvolume))
}

fn btrfs_subvolume_is_verified(subvolume: &str) -> bool {
    bounded_command_output("btrfs", &["subvolume", "show", subvolume], probe_timeout())
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn read_home_mount() -> Option<MountInfoEntry> {
    let mountinfo_path =
        env::var("GOBLINS_OS_MOUNTINFO").unwrap_or_else(|_| DEFAULT_MOUNTINFO.to_string());
    let text = fs::read_to_string(mountinfo_path).ok()?;
    mount_for_path(&parse_mountinfo(&text), "/var/home")
        .or_else(|| mount_for_path(&parse_mountinfo(&text), "/home"))
}

fn parse_mountinfo(text: &str) -> Vec<MountInfoEntry> {
    text.lines().filter_map(parse_mountinfo_line).collect()
}

fn parse_mountinfo_line(line: &str) -> Option<MountInfoEntry> {
    let (pre, post) = line.split_once(" - ")?;
    let pre_fields = pre.split_whitespace().collect::<Vec<_>>();
    let post_fields = post.split_whitespace().collect::<Vec<_>>();
    if pre_fields.len() < 5 || post_fields.len() < 2 {
        return None;
    }
    Some(MountInfoEntry {
        mount_point: unescape_mount_field(pre_fields[4]),
        filesystem: post_fields[0].to_string(),
        source: post_fields[1].to_string(),
    })
}

fn mount_for_path(entries: &[MountInfoEntry], path: &str) -> Option<MountInfoEntry> {
    entries
        .iter()
        .filter(|entry| mount_point_covers(&entry.mount_point, path))
        .max_by_key(|entry| entry.mount_point.len())
        .cloned()
}

fn mount_point_covers(mount_point: &str, path: &str) -> bool {
    mount_point == "/" || path == mount_point || path.starts_with(&format!("{mount_point}/"))
}

fn unescape_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn executable_exists(command: &str) -> bool {
    bounded_command_output(command, &["--version"], probe_timeout())
        .map(|output| output.status)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountinfo_selects_btrfs_home_mount() {
        let entries = parse_mountinfo(
            "29 1 8:1 / / rw,relatime - xfs /dev/vda3 rw\n\
             30 29 8:2 / /var/home rw,relatime - btrfs /dev/vda4 rw\n",
        );

        let mount = mount_for_path(&entries, "/var/home/joseph").unwrap();
        assert_eq!(mount.mount_point, "/var/home");
        assert_eq!(mount.filesystem, "btrfs");
        assert_eq!(mount.source, "/dev/vda4");
    }

    #[test]
    fn mountinfo_accepts_a_verified_var_subvolume_shape_without_calling_it_home_only() {
        let entries = parse_mountinfo(
            "29 1 8:1 / / rw,relatime - btrfs /dev/vda3 rw\n\
             30 29 8:1 /var /var rw,relatime - btrfs /dev/vda3 rw\n",
        );

        let mount = mount_for_path(&entries, "/var/home/goblin").unwrap();
        assert_eq!(mount.mount_point, "/var");
        assert_eq!(mount.filesystem, "btrfs");
        assert!(snapshot_subvolume_contract("/var", Some("/var"), true));
        assert!(!snapshot_subvolume_contract("/", Some("/var"), true));
    }

    #[test]
    fn mountinfo_falls_back_to_root_for_existing_xfs_installs() {
        let entries = parse_mountinfo("29 1 8:1 / / rw,relatime - xfs /dev/vda3 rw\n");

        let mount = mount_for_path(&entries, "/var/home").unwrap();
        assert_eq!(mount.mount_point, "/");
        assert_eq!(mount.filesystem, "xfs");
    }

    #[test]
    fn blocked_restore_response_never_claims_file_changes() {
        let (_, body) = restore_response(
            StatusCode::PRECONDITION_REQUIRED,
            "7",
            "/var/home/alex/file.txt",
            None,
            0,
            "blocked",
        );
        assert!(!body.ok);
        assert!(!body.executes_restore);
        assert_eq!(body.snapshot_id, "7");
    }

    #[test]
    fn recovery_paths_are_absolute_nul_free_and_bounded() {
        assert!(valid_recovery_path("/var/home/goblin/Documents/report.txt"));
        assert!(!valid_recovery_path("Documents/report.txt"));
        assert!(!valid_recovery_path("/var/home/goblin/bad\0name"));
        assert!(!valid_recovery_path(&format!("/{}", "x".repeat(4 * 1024))));
    }

    #[test]
    fn snapper_config_accepts_only_supported_snapshot_sources() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("home");
        fs::write(&config, "FSTYPE=\"btrfs\"\nSUBVOLUME=\"/\"\n").unwrap();
        assert_eq!(read_snapper_subvolume(&config).as_deref(), Some("/"));

        fs::write(&config, "SUBVOLUME=\"/var/home\"\n").unwrap();
        assert_eq!(
            read_snapper_subvolume(&config).as_deref(),
            Some("/var/home")
        );

        fs::write(&config, "SUBVOLUME=\"/var\"\n").unwrap();
        assert_eq!(read_snapper_subvolume(&config).as_deref(), Some("/var"));

        for unsupported in ["../var/home", "/tmp", "/var/home/goblin"] {
            fs::write(&config, format!("SUBVOLUME=\"{unsupported}\"\n")).unwrap();
            assert!(read_snapper_subvolume(&config).is_none());
        }
    }

    #[test]
    fn snapshot_source_requires_the_actual_verified_btrfs_subvolume() {
        assert!(snapshot_subvolume_contract("/", Some("/"), true));
        assert!(snapshot_subvolume_contract("/var", Some("/var"), true));
        assert!(snapshot_subvolume_contract(
            "/var/home",
            Some("/var/home"),
            true
        ));
        assert!(snapshot_subvolume_contract("/var/home", Some("/var"), true));
        assert!(!snapshot_subvolume_contract("/", Some("/var"), true));
        assert!(!snapshot_subvolume_contract("/var", Some("/var"), false));
        assert!(!snapshot_subvolume_contract(
            "/sysroot",
            Some("/sysroot"),
            true
        ));
    }
}
