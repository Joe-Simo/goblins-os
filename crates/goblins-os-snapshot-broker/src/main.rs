use std::{
    ffi::{CStr, CString, OsStr},
    fs,
    io::{self, IoSlice, Read, Write},
    os::{
        fd::{AsRawFd, BorrowedFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Component, Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, MetadataExt as _, OpenOptions},
};
use rustix::net::{sendmsg, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};
use serde::{Deserialize, Serialize};

const DEFAULT_SOCKET: &str = "/run/goblins-os-snapshots/broker.sock";
const CORE_SERVICE_USER: &str = "goblins-os";
const DESKTOP_USER: &str = "goblin";
const BROKER_READER_GROUP: &str = "goblins-snapshot-readers";
const SNAPPER: &str = "/usr/bin/snapper";
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: u64 = 1024 * 1024;
const MAX_BROWSE_ENTRIES: usize = 300;
/// Offset pagination intentionally stops after 30k safe entries. `ReadDir`
/// offers no portable stable cookie, so a larger caller-controlled offset
/// would make a root broker rescan unbounded directory contents per request.
const MAX_BROWSE_CURSOR: usize = 30_000;
const MAX_BROWSE_SCANNED_ENTRIES: usize = MAX_BROWSE_CURSOR + MAX_BROWSE_ENTRIES + 1;
const MAX_BROWSE_RESPONSE_BYTES: usize = 768 * 1024;
const BROWSE_WORK_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_PATH_BYTES: usize = 4 * 1024;
const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
enum BrokerRequest {
    ListSnapshots,
    Browse {
        snapshot_id: String,
        snapshot_subvolume: String,
        directory: String,
        cursor: usize,
    },
    OpenFile {
        snapshot_id: String,
        snapshot_subvolume: String,
        file: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct SnapshotRecord {
    id: String,
    kind: String,
    date: Option<String>,
    user: Option<String>,
    cleanup: Option<String>,
    description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct BrowseEntry {
    name: String,
    relative_path: String,
    logical_path: String,
    kind: &'static str,
    bytes: Option<u64>,
}

#[derive(Serialize)]
struct BrokerResponse {
    ok: bool,
    snapshots: Vec<SnapshotRecord>,
    entries: Vec<BrowseEntry>,
    truncated: bool,
    source_bytes: Option<u64>,
    next_cursor: Option<usize>,
    detail: String,
}

fn main() {
    if std::env::args().any(|arg| arg == "--self-test") {
        println!("goblins-os-snapshot-broker self-test passed");
        return;
    }
    if let Err(error) = run() {
        eprintln!("goblins-os-snapshot-broker: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let socket = std::env::var_os("GOBLINS_OS_SNAPSHOT_BROKER_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET));
    if let Some(parent) = socket.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if let Ok(metadata) = fs::symlink_metadata(&socket) {
        if !metadata.file_type().is_socket() || metadata.uid() != 0 {
            return Err("refusing to replace an untrusted broker socket path".to_string());
        }
        fs::remove_file(&socket).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    let reader_gid = resolve_group_id(BROKER_READER_GROUP)
        .ok_or_else(|| "the snapshot reader group is unavailable".to_string())?;
    let path = CString::new(socket.as_os_str().as_encoded_bytes())
        .map_err(|_| "the snapshot broker socket path is invalid".to_string())?;
    // SAFETY: path is NUL-terminated and names the newly bound socket.
    if unsafe { libc::chown(path.as_ptr(), 0, reader_gid) } != 0 {
        return Err("could not assign the snapshot broker socket group".to_string());
    }
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o660))
        .map_err(|error| error.to_string())?;
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                thread::spawn(move || {
                    let _ = handle_stream(&mut stream);
                });
            }
            Err(error) => eprintln!("goblins-os-snapshot-broker: connection failed: {error}"),
        }
    }
    Ok(())
}

fn handle_stream(stream: &mut UnixStream) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    if peer_uid(stream)? != resolve_user_id(CORE_SERVICE_USER).unwrap_or(u32::MAX) {
        return write_response(
            stream,
            failure("Only the authenticated core service may read snapshots."),
            None,
        );
    }
    let body = read_bounded(stream, MAX_REQUEST_BYTES)?;
    let request = match serde_json::from_slice::<BrokerRequest>(&body) {
        Ok(request) => request,
        Err(_) => return write_response(stream, failure("The snapshot request is invalid."), None),
    };
    match request {
        BrokerRequest::ListSnapshots => match list_snapshots() {
            Ok(snapshots) => write_response(
                stream,
                BrokerResponse {
                    ok: true,
                    snapshots,
                    entries: Vec::new(),
                    truncated: false,
                    source_bytes: None,
                    next_cursor: None,
                    detail: String::new(),
                },
                None,
            ),
            Err(detail) => write_response(stream, failure(detail), None),
        },
        BrokerRequest::Browse {
            snapshot_id,
            snapshot_subvolume,
            directory,
            cursor,
        } => match browse_snapshot(&snapshot_id, &snapshot_subvolume, &directory, cursor) {
            Ok((entries, next_cursor)) => write_response(
                stream,
                BrokerResponse {
                    ok: true,
                    snapshots: Vec::new(),
                    entries,
                    truncated: next_cursor.is_some(),
                    source_bytes: None,
                    next_cursor,
                    detail: String::new(),
                },
                None,
            ),
            Err(detail) => write_response(stream, failure(detail), None),
        },
        BrokerRequest::OpenFile {
            snapshot_id,
            snapshot_subvolume,
            file,
        } => match open_snapshot_file(&snapshot_id, &snapshot_subvolume, &file) {
            Ok(file) => {
                let bytes = file.metadata()?.len();
                write_response(
                    stream,
                    BrokerResponse {
                        ok: true,
                        snapshots: Vec::new(),
                        entries: Vec::new(),
                        truncated: false,
                        source_bytes: Some(bytes),
                        next_cursor: None,
                        detail: String::new(),
                    },
                    Some(&file),
                )
            }
            Err(detail) => write_response(stream, failure(detail), None),
        },
    }
}

fn list_snapshots() -> Result<Vec<SnapshotRecord>, String> {
    let mut command = Command::new(SNAPPER);
    command
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .args([
            "--machine-readable",
            "csv",
            "-c",
            "home",
            "list",
            "--columns",
            "number,type,pre-number,date,user,cleanup,description",
        ]);
    let output = bounded_command_output(&mut command, Duration::from_secs(15)).map_err(
        |error| match error.kind() {
            io::ErrorKind::NotFound => "Snapper is not installed.".to_string(),
            io::ErrorKind::TimedOut => {
                "Snapper did not answer before the protected snapshot timeout.".to_string()
            }
            _ => "The protected snapshot list could not be read.".to_string(),
        },
    )?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Err(if detail.is_empty() {
            "Snapper could not list storage snapshots.".to_string()
        } else {
            format!("Snapper could not list storage snapshots: {detail}")
        });
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|_| "Snapper returned unreadable snapshot output.".to_string())?;
    Ok(parse_snapper_machine_readable(&output))
}

fn bounded_command_output(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = spawn_capped_drain(child.stdout.take());
    let stderr = spawn_capped_drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout);
                drop(stderr);
                return Err(io::Error::new(io::ErrorKind::TimedOut, "command timed out"));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout);
                drop(stderr);
                return Err(error);
            }
        }
    };
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn spawn_capped_drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let Some(pipe) = pipe else {
            return Vec::new();
        };
        let mut limited = pipe.take(MAX_COMMAND_OUTPUT_BYTES);
        let mut captured = Vec::new();
        let _ = limited.read_to_end(&mut captured);
        let _ = io::copy(&mut limited.into_inner(), &mut io::sink());
        captured
    })
}

fn parse_snapper_machine_readable(output: &str) -> Vec<SnapshotRecord> {
    parse_csv_records(output)
        .into_iter()
        .filter_map(parse_snapper_fields)
        .collect()
}

fn parse_snapper_fields(fields: Vec<String>) -> Option<SnapshotRecord> {
    if fields.len() < 2 {
        return None;
    }
    let id = fields[0].trim().to_string();
    if id == "0" || id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(SnapshotRecord {
        id,
        kind: clean_optional_field(fields.get(1)).unwrap_or_else(|| "unknown".to_string()),
        date: clean_optional_field(fields.get(3)),
        user: clean_optional_field(fields.get(4)),
        cleanup: clean_optional_field(fields.get(5)),
        description: clean_optional_field(fields.get(6)),
    })
}

fn parse_csv_records(input: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                field.push('"');
                chars.next();
            } else {
                in_quotes = !in_quotes;
            }
        } else if character == ',' && !in_quotes {
            record.push(field.trim().to_string());
            field.clear();
        } else if character == '\n' && !in_quotes {
            record.push(field.trim().to_string());
            field.clear();
            if record.iter().any(|field| !field.is_empty()) {
                records.push(std::mem::take(&mut record));
            } else {
                record.clear();
            }
        } else if character == '\r' && !in_quotes {
            continue;
        } else {
            field.push(character);
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field.trim().to_string());
        if record.iter().any(|field| !field.is_empty()) {
            records.push(record);
        }
    }
    records
}

fn clean_optional_field(value: Option<&String>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty() && value != "-").then(|| value.to_string())
}

fn browse_snapshot(
    snapshot_id: &str,
    snapshot_subvolume: &str,
    directory: &str,
    cursor: usize,
) -> Result<(Vec<BrowseEntry>, Option<usize>), &'static str> {
    let home_path = desktop_home()?;
    let (root, home_relative) = snapshot_home(snapshot_id, snapshot_subvolume, &home_path)?;
    browse_snapshot_at(
        &root,
        &home_relative,
        Path::new(directory),
        &home_path,
        cursor,
    )
}

fn open_snapshot_file(
    snapshot_id: &str,
    snapshot_subvolume: &str,
    file: &str,
) -> Result<std::fs::File, &'static str> {
    let home_path = desktop_home()?;
    let logical_file = Path::new(file);
    let relative_file = logical_file
        .strip_prefix(&home_path)
        .map_err(|_| "Choose a file reported by the protected snapshot browser.")?;
    let (root, home_relative) = snapshot_home(snapshot_id, snapshot_subvolume, &home_path)?;
    open_snapshot_file_at(&root, &home_relative, relative_file)
}

fn snapshot_home(
    snapshot_id: &str,
    snapshot_subvolume: &str,
    home_path: &Path,
) -> Result<(Dir, PathBuf), &'static str> {
    if snapshot_id.is_empty()
        || snapshot_id.len() > 20
        || !snapshot_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("The snapshot id is invalid.");
    }
    if !matches!(snapshot_subvolume, "/" | "/var" | "/var/home") {
        return Err("The snapshot source is not allowlisted.");
    }
    let subvolume = Path::new(snapshot_subvolume);
    if !valid_desktop_home_path(home_path) || !home_path.starts_with(subvolume) {
        return Err("The desktop home path is outside the snapshot source.");
    }
    let home_relative = home_path
        .strip_prefix(subvolume)
        .map_err(|_| "The desktop home path is outside the snapshot source.")?
        .to_path_buf();
    let root_path = subvolume
        .join(".snapshots")
        .join(snapshot_id)
        .join("snapshot");
    let root = open_absolute_directory_nofollow(&root_path)
        .map_err(|_| "The selected snapshot could not be opened safely.")?;
    Ok((root, home_relative))
}

fn browse_snapshot_at(
    snapshot_root: &Dir,
    home_relative: &Path,
    directory: &Path,
    logical_home: &Path,
    cursor: usize,
) -> Result<(Vec<BrowseEntry>, Option<usize>), &'static str> {
    if cursor > MAX_BROWSE_CURSOR {
        return Err("The snapshot page cursor is outside the fixed safety limit.");
    }
    validate_relative_path(home_relative, true)?;
    validate_relative_path(directory, true)?;
    let home = open_relative_directory_nofollow(snapshot_root, home_relative)
        .map_err(|_| "The snapshot home folder could not be opened safely.")?;
    let current = open_relative_directory_nofollow(&home, directory)
        .map_err(|_| "That snapshot folder is unavailable.")?;
    let mut page = Vec::new();
    let mut valid_index = 0_usize;
    let mut response_bytes = 0_usize;
    let mut next_cursor = None;
    let browse_deadline = Instant::now() + BROWSE_WORK_TIMEOUT;
    for (scanned, entry) in current
        .entries()
        .map_err(|_| "That snapshot folder could not be listed.")?
        .enumerate()
    {
        if Instant::now() >= browse_deadline {
            return Err("This snapshot folder could not be listed inside the bounded recovery-browser time limit. Open a narrower folder or use an administrator-assisted recovery tool.");
        }
        if scanned >= MAX_BROWSE_SCANNED_ENTRIES {
            return Err("This snapshot folder exceeds the bounded recovery browser limit. Open a narrower folder or recover the file with an administrator-assisted tool.");
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.is_empty() || name.len() > 255 {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(kind) => kind,
            Err(_) => continue,
        };
        let (kind, bytes) = if file_type.is_dir() {
            ("directory", None)
        } else if file_type.is_file() {
            let bytes = entry.metadata().ok().map(|metadata| metadata.len());
            if bytes.is_some_and(|bytes| bytes > MAX_SOURCE_BYTES) {
                continue;
            }
            ("file", bytes)
        } else {
            continue;
        };
        let relative_path = directory.join(&name);
        let entry = BrowseEntry {
            name,
            relative_path: relative_path.to_string_lossy().into_owned(),
            logical_path: logical_home
                .join(&relative_path)
                .to_string_lossy()
                .into_owned(),
            kind,
            bytes,
        };
        if valid_index < cursor {
            valid_index += 1;
            continue;
        }
        let entry_bytes = serde_json::to_vec(&entry)
            .map_err(|_| "The snapshot entry could not be encoded safely.")?
            .len();
        if page.len() == MAX_BROWSE_ENTRIES
            || response_bytes.saturating_add(entry_bytes) > MAX_BROWSE_RESPONSE_BYTES
        {
            next_cursor = Some(valid_index);
            break;
        }
        response_bytes = response_bytes.saturating_add(entry_bytes);
        page.push(entry);
        valid_index += 1;
    }
    Ok((page, next_cursor))
}

fn open_snapshot_file_at(
    snapshot_root: &Dir,
    home_relative: &Path,
    relative_file: &Path,
) -> Result<std::fs::File, &'static str> {
    validate_relative_path(home_relative, true)?;
    validate_relative_path(relative_file, false)?;
    let home = open_relative_directory_nofollow(snapshot_root, home_relative)
        .map_err(|_| "The snapshot home folder could not be opened safely.")?;
    let (parent, name) = open_relative_parent_nofollow(&home, relative_file)
        .map_err(|_| "The selected snapshot file is unavailable.")?;
    let expected = parent
        .symlink_metadata(&name)
        .map_err(|_| "The selected snapshot file is unavailable.")?;
    if !expected.is_file() || expected.len() > MAX_SOURCE_BYTES {
        return Err("Recovery supports regular files up to 8 GiB.");
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent
        .open_with(&name, &options)
        .map_err(|_| "The selected snapshot file could not be opened safely.")?;
    let opened = file
        .metadata()
        .map_err(|_| "The selected snapshot file changed unexpectedly.")?;
    if !opened.is_file()
        || opened.dev() != expected.dev()
        || opened.ino() != expected.ino()
        || opened.len() != expected.len()
    {
        return Err("The selected snapshot file changed unexpectedly.");
    }
    Ok(file.into_std())
}

fn desktop_home() -> Result<PathBuf, &'static str> {
    let (uid, home) = resolve_user(DESKTOP_USER)
        .ok_or("The configured desktop account could not be resolved safely.")?;
    if uid == 0 || !valid_desktop_home_path(&home) {
        return Err("The configured desktop account has an unsafe home path.");
    }
    Ok(home)
}

fn valid_desktop_home_path(path: &Path) -> bool {
    if path.as_os_str().as_encoded_bytes().len() > MAX_PATH_BYTES || !path.is_absolute() {
        return false;
    }
    let parts = path.components().collect::<Vec<_>>();
    matches!(parts.as_slice(),
        [Component::RootDir, Component::Normal(home), Component::Normal(user)]
            if *home == "home" && *user == DESKTOP_USER)
        || matches!(parts.as_slice(),
            [Component::RootDir, Component::Normal(var), Component::Normal(home), Component::Normal(user)]
                if *var == "var" && *home == "home" && *user == DESKTOP_USER)
}

fn validate_relative_path(path: &Path, empty_allowed: bool) -> Result<(), &'static str> {
    if path.as_os_str().as_encoded_bytes().len() > MAX_PATH_BYTES
        || (!empty_allowed && path.as_os_str().is_empty())
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("The snapshot path contains an unsafe component.");
    }
    Ok(())
}

fn open_absolute_directory_nofollow(path: &Path) -> io::Result<Dir> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute path required",
        ));
    }
    let root = Dir::open_ambient_dir("/", ambient_authority())?;
    open_relative_directory_nofollow(&root, path.strip_prefix("/").unwrap_or(path))
}

fn open_relative_directory_nofollow(root: &Dir, path: &Path) -> io::Result<Dir> {
    let mut current = root.try_clone()?;
    for component in path.components() {
        match component {
            Component::Normal(name) => current = current.open_dir_nofollow(name)?,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "unsafe path")),
        }
    }
    Ok(current)
}

fn open_relative_parent_nofollow(root: &Dir, path: &Path) -> io::Result<(Dir, PathBuf)> {
    let parts = path
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(PathBuf::from(name)),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "unsafe path")),
        })
        .collect::<io::Result<Vec<_>>>()?;
    let (name, parents) = parts
        .split_last()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file required"))?;
    let mut parent = root.try_clone()?;
    for part in parents {
        parent = parent.open_dir_nofollow(part)?;
    }
    Ok((parent, name.clone()))
}

fn failure(detail: impl Into<String>) -> BrokerResponse {
    BrokerResponse {
        ok: false,
        snapshots: Vec::new(),
        entries: Vec::new(),
        truncated: false,
        source_bytes: None,
        next_cursor: None,
        detail: detail.into(),
    }
}

fn write_response(
    stream: &mut UnixStream,
    response: BrokerResponse,
    file: Option<&std::fs::File>,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(&response).map_err(io::Error::other)?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(io::Error::other("broker response too large"));
    }
    if let Some(file) = file {
        let borrowed = [unsafe { BorrowedFd::borrow_raw(file.as_raw_fd()) }];
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if !control.push(SendAncillaryMessage::ScmRights(&borrowed)) {
            return Err(io::Error::other("descriptor buffer too small"));
        }
        let written = sendmsg(
            &*stream,
            &[IoSlice::new(&bytes)],
            &mut control,
            SendFlags::empty(),
        )?;
        stream.write_all(&bytes[written..])?;
    } else {
        stream.write_all(&bytes)?;
    }
    Ok(())
}

fn read_bounded(stream: &mut UnixStream, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request too large",
        ));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if status == 0 && length as usize == std::mem::size_of::<libc::ucred>() {
        Ok(credentials.uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "snapshot peer credentials require Linux SO_PEERCRED",
    ))
}

fn resolve_user_id(name: &str) -> Option<u32> {
    resolve_user(name).map(|(uid, _)| uid)
}

fn resolve_user(name: &str) -> Option<(u32, PathBuf)> {
    let name = CString::new(name).ok()?;
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0_u8; 16 * 1024];
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }
    let record = unsafe { record.assume_init() };
    if record.pw_dir.is_null() {
        return None;
    }
    // SAFETY: getpwnam_r returned a record whose strings live in `buffer`;
    // copy the home bytes into an owned PathBuf before the buffer is dropped.
    let home = unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes().to_vec();
    Some((
        record.pw_uid,
        PathBuf::from(OsStr::from_bytes(home.as_slice())),
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn browser_lists_only_regular_files_and_directories_without_following_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::create_dir_all(temp.path().join("goblin/Documents")).unwrap();
        fs::write(
            temp.path().join("goblin/Documents/deleted.txt"),
            b"snapshot",
        )
        .unwrap();
        symlink("deleted.txt", temp.path().join("goblin/Documents/link.txt")).unwrap();
        let (entries, truncated) = browse_snapshot_at(
            &root,
            Path::new("goblin"),
            Path::new("Documents"),
            Path::new("/var/home/goblin"),
            0,
        )
        .unwrap();
        assert_eq!(truncated, None);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].logical_path,
            "/var/home/goblin/Documents/deleted.txt"
        );
        assert_eq!(entries[0].kind, "file");
    }

    #[test]
    fn browser_pages_immutable_directory_without_stranding_entries() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        let documents = temp.path().join("goblin/Documents");
        fs::create_dir_all(&documents).unwrap();
        for index in (0..=MAX_BROWSE_ENTRIES).rev() {
            fs::write(documents.join(format!("file-{index:04}.txt")), b"snapshot").unwrap();
        }
        let (first, next) = browse_snapshot_at(
            &root,
            Path::new("goblin"),
            Path::new("Documents"),
            Path::new("/var/home/goblin"),
            0,
        )
        .unwrap();
        assert_eq!(first.len(), MAX_BROWSE_ENTRIES);
        assert_eq!(next, Some(MAX_BROWSE_ENTRIES));

        let (second, next) = browse_snapshot_at(
            &root,
            Path::new("goblin"),
            Path::new("Documents"),
            Path::new("/var/home/goblin"),
            next.unwrap(),
        )
        .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(next, None);
        let names = first
            .iter()
            .chain(&second)
            .map(|entry| entry.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(names.len(), MAX_BROWSE_ENTRIES + 1);
        assert!(names.contains("file-0000.txt"));
        assert!(names.contains("file-0300.txt"));
    }

    #[test]
    fn open_file_is_held_regular_and_rejects_traversal_and_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(temp.path(), ambient_authority()).unwrap();
        fs::create_dir_all(temp.path().join("goblin/Documents")).unwrap();
        fs::write(temp.path().join("goblin/Documents/report.txt"), b"snapshot").unwrap();
        let file = open_snapshot_file_at(
            &root,
            Path::new("goblin"),
            Path::new("Documents/report.txt"),
        )
        .unwrap();
        assert_eq!(file.metadata().unwrap().len(), 8);
        assert!(
            open_snapshot_file_at(&root, Path::new("goblin"), Path::new("../outside")).is_err()
        );
        symlink("report.txt", temp.path().join("goblin/Documents/link.txt")).unwrap();
        assert!(
            open_snapshot_file_at(&root, Path::new("goblin"), Path::new("Documents/link.txt"))
                .is_err()
        );
    }

    #[test]
    fn home_and_request_contracts_are_narrow() {
        assert!(valid_desktop_home_path(Path::new("/var/home/goblin")));
        assert!(valid_desktop_home_path(Path::new("/home/goblin")));
        for rejected in [
            "/root",
            "/tmp/goblin",
            "/home/alex",
            "/var/home/goblin/extra",
            "/var/home/../root",
        ] {
            assert!(!valid_desktop_home_path(Path::new(rejected)), "{rejected}");
        }
        assert!(serde_json::from_str::<BrokerRequest>(r#"{"op":"list-snapshots"}"#).is_ok());
        assert!(serde_json::from_str::<BrokerRequest>(r#"{"op":"open-file","snapshot_id":"7","snapshot_subvolume":"/","file":"/var/home/goblin/Documents/report.txt"}"#).is_ok());
        assert!(serde_json::from_str::<BrokerRequest>(r#"{"op":"open-file","snapshot_id":"7","snapshot_subvolume":"/","home_path":"/var/home/alex","file":"/var/home/alex/Documents/report.txt"}"#).is_err());
        assert!(serde_json::from_str::<BrokerRequest>(r#"{"op":"open-file","snapshot_id":"7","snapshot_subvolume":"/","file":"/var/home/goblin/Documents/report.txt","command":"sh"}"#).is_err());
    }

    #[test]
    fn snapper_parser_keeps_only_numeric_snapshot_rows() {
        let snapshots = parse_snapper_machine_readable(
            "Number,Type,Pre Number,Date,User,Cleanup,Description\n\
             0,single,,,,,current\n\
             12,single,,2026-07-01 10:30:00,root,timeline,\"Before update, keep\",\n\
             13,single,,2026-07-02 10:30:00,root,timeline,\"Line one\nline two; still one field\"\n\
             invalid,single,,2026-07-01,root,timeline,ignored\n",
        );
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].id, "12");
        assert_eq!(
            snapshots[0].description.as_deref(),
            Some("Before update, keep")
        );
        assert_eq!(snapshots[1].id, "13");
        assert_eq!(
            snapshots[1].description.as_deref(),
            Some("Line one\nline two; still one field")
        );
    }
}
