//! Read-only local snapshot reporting for the recovery surface.
//!
//! Snapshots are only truthful when the installed home storage is btrfs and the
//! Snapper `home` config exists. Existing xfs installs therefore report an
//! honest off-state instead of inventing a timeline.

use std::{env, fs, path::Path, time::Duration};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::{bounded_command_output, probe_timeout, BoundedCommandError};

const DEFAULT_MOUNTINFO: &str = "/proc/self/mountinfo";
const DEFAULT_SNAPPER_CONFIG: &str = "/etc/snapper/configs/home";
const SNAPPER_CONFIG_NAME: &str = "home";
/// Bound for `snapper list`. The snapper CLI D-Bus-activates `snapperd` on
/// first use and that activation alone can take several seconds, so the list
/// call needs a wider bound than the default read probe.
const SNAPPER_LIST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize)]
pub struct SnapshotsStatus {
    source: &'static str,
    available: bool,
    snapper_available: bool,
    btrfs_tools_available: bool,
    config_available: bool,
    config_path: String,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SnapshotRecord {
    id: String,
    kind: String,
    date: Option<String>,
    user: Option<String>,
    cleanup: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
pub struct RestoreSnapshotRequest {
    snapshot_id: String,
    path: String,
}

#[derive(Serialize)]
pub struct RestoreSnapshotOutcome {
    ok: bool,
    executes_restore: bool,
    snapshot_id: String,
    path: String,
    text: String,
}

#[derive(Debug)]
enum SnapperListError {
    Missing,
    TimedOut,
    Failed(String),
    Unreadable,
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

pub async fn restore_snapshot(
    Json(request): Json<RestoreSnapshotRequest>,
) -> (StatusCode, Json<RestoreSnapshotOutcome>) {
    let snapshot_id = request.snapshot_id.trim();
    let path = request.path.trim();

    if snapshot_id.is_empty() || !snapshot_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return restore_response(
            StatusCode::BAD_REQUEST,
            snapshot_id,
            path,
            "Choose a real Snapper snapshot id before restore.",
        );
    }
    if !path.starts_with("/home/") && !path.starts_with("/var/home/") {
        return restore_response(
            StatusCode::BAD_REQUEST,
            snapshot_id,
            path,
            "Snapshot restore is limited to paths under /home or /var/home.",
        );
    }

    restore_response(
        StatusCode::PRECONDITION_REQUIRED,
        snapshot_id,
        path,
        "Snapshot restore is CI/qemu-gated and read-only in this substrate; no files were changed.",
    )
}

fn restore_response(
    status: StatusCode,
    snapshot_id: &str,
    path: &str,
    text: &str,
) -> (StatusCode, Json<RestoreSnapshotOutcome>) {
    (
        status,
        Json(RestoreSnapshotOutcome {
            ok: false,
            executes_restore: false,
            snapshot_id: snapshot_id.to_string(),
            path: path.to_string(),
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
    let home_mount = read_home_mount().map(|entry| SnapshotMount {
        mount_point: entry.mount_point,
        filesystem: entry.filesystem,
        source: entry.source,
    });

    let mut blockers = Vec::new();
    if !btrfs_tools_available {
        blockers.push("btrfs tooling is not installed".to_string());
    }
    if !snapper_available {
        blockers.push("snapper is not installed".to_string());
    }
    if !config_available {
        blockers.push(format!("{config_path} is missing"));
    }
    match home_mount.as_ref() {
        Some(mount) if mount.filesystem == "btrfs" => {}
        Some(mount) => blockers.push(format!(
            "Local snapshots need a btrfs /home; this system reports {} on {}.",
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
            home_mount,
            snapshots: Vec::new(),
            restore_ready: false,
            executes_restore: false,
            detail: blockers.join(" "),
        };
    }

    match run_snapper_list() {
        Ok(output) => {
            let snapshots = parse_snapper_machine_readable(&output);
            let detail = if snapshots.is_empty() {
                "Snapper is configured for btrfs home storage, but it reported no local snapshots yet."
                    .to_string()
            } else {
                format!(
                    "Snapper reported {} local home snapshot(s). Restore remains qemu-gated.",
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
                home_mount,
                snapshots,
                restore_ready: false,
                executes_restore: false,
                detail,
            }
        }
        Err(SnapperListError::Missing) => degraded_snapshots_status(
            snapper_available,
            btrfs_tools_available,
            config_available,
            config_path,
            home_mount,
            "snapper is not installed.",
        ),
        Err(SnapperListError::TimedOut) => degraded_snapshots_status(
            snapper_available,
            btrfs_tools_available,
            config_available,
            config_path,
            home_mount,
            "Snapper did not answer before the snapshot timeout.",
        ),
        Err(SnapperListError::Failed(detail)) => degraded_snapshots_status(
            snapper_available,
            btrfs_tools_available,
            config_available,
            config_path,
            home_mount,
            &format!("snapper could not list home snapshots: {detail}"),
        ),
        Err(SnapperListError::Unreadable) => degraded_snapshots_status(
            snapper_available,
            btrfs_tools_available,
            config_available,
            config_path,
            home_mount,
            "snapper returned unreadable snapshot output.",
        ),
    }
}

fn degraded_snapshots_status(
    snapper_available: bool,
    btrfs_tools_available: bool,
    config_available: bool,
    config_path: String,
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
        home_mount,
        snapshots: Vec::new(),
        restore_ready: false,
        executes_restore: false,
        detail: detail.to_string(),
    }
}

fn run_snapper_list() -> Result<String, SnapperListError> {
    let output = match bounded_command_output(
        "snapper",
        &["-c", SNAPPER_CONFIG_NAME, "list", "--machine-readable"],
        SNAPPER_LIST_TIMEOUT,
    ) {
        Ok(output) => output,
        // A timed-out list means snapper IS present but did not answer in
        // time; reporting it as "not installed" would be dishonest.
        Err(BoundedCommandError::TimedOut) => return Err(SnapperListError::TimedOut),
        Err(_) => return Err(SnapperListError::Missing),
    };

    if !output.status.success() {
        return Err(SnapperListError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    String::from_utf8(output.stdout).map_err(|_| SnapperListError::Unreadable)
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

fn parse_snapper_machine_readable(output: &str) -> Vec<SnapshotRecord> {
    output
        .lines()
        .filter_map(parse_snapper_row)
        .collect::<Vec<_>>()
}

fn parse_snapper_row(line: &str) -> Option<SnapshotRecord> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let fields = split_machine_row(trimmed);
    if fields.len() < 2 {
        return None;
    }
    let id = fields[0].trim().to_string();
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
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

fn split_machine_row(line: &str) -> Vec<String> {
    let semicolons = line.matches(';').count();
    let tabs = line.matches('\t').count();
    let delimiter = if tabs > 0 {
        '\t'
    } else if semicolons >= line.matches(',').count() {
        ';'
    } else {
        ','
    };

    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                current.push('"');
                chars.next();
            } else {
                in_quotes = !in_quotes;
            }
        } else if ch == delimiter && !in_quotes {
            fields.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    fields.push(current.trim().to_string());
    fields
}

fn clean_optional_field(value: Option<&String>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() || trimmed == "-" {
        None
    } else {
        Some(trimmed.to_string())
    }
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
    fn mountinfo_falls_back_to_root_for_existing_xfs_installs() {
        let entries = parse_mountinfo("29 1 8:1 / / rw,relatime - xfs /dev/vda3 rw\n");

        let mount = mount_for_path(&entries, "/var/home").unwrap();
        assert_eq!(mount.mount_point, "/");
        assert_eq!(mount.filesystem, "xfs");
    }

    #[test]
    fn snapper_parser_reads_machine_rows_without_headers() {
        let rows = parse_snapper_machine_readable(
            "Type,Pre #,Date,User,Cleanup,Description,Userdata\n\
             12,single,,2026-07-01 10:30:00,root,timeline,\"Before update, keep\",important=yes\n\
             13,post,12,2026-07-01 10:35:00,root,number,After update,\n",
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "12");
        assert_eq!(rows[0].kind, "single");
        assert_eq!(rows[0].description.as_deref(), Some("Before update, keep"));
        assert_eq!(rows[1].id, "13");
        assert_eq!(rows[1].cleanup.as_deref(), Some("number"));
    }

    #[test]
    fn restore_response_never_claims_file_changes() {
        let (_, body) = restore_response(
            StatusCode::PRECONDITION_REQUIRED,
            "7",
            "/var/home/alex/file.txt",
            "blocked",
        );
        assert!(!body.ok);
        assert!(!body.executes_restore);
        assert_eq!(body.snapshot_id, "7");
    }
}
