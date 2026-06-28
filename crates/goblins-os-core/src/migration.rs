//! Migration Assistant substrate (source capability + additive copy planning).
//!
//! The macOS "Migration Assistant" altitude: bring a previous home over on first
//! boot. This module ships the host-testable foundation — which source filesystems
//! Goblins OS can actually read (so a drive it can't read is shown disabled, never
//! silently skipped), the category model, and the allowlisted preference keys the
//! import is permitted to write. The copy-plan route builds the exact rsync argv
//! and ledger paths without mounting or copying; udisks execution, process
//! streaming, and the installer page are the deliberate CI/qemu follow-up.

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use crate::install_targets::{scan_migration_source_partitions, MigrationSourcePartitionProbe};
use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

/// A migratable data category and the source-relative directories it covers.
const CATEGORIES: &[MigrationCategorySpec] = &[
    MigrationCategorySpec {
        id: "documents-desktop",
        name: "Documents & Desktop",
        sources: &["Documents", "Desktop"],
    },
    MigrationCategorySpec {
        id: "pictures",
        name: "Pictures",
        sources: &["Pictures"],
    },
    MigrationCategorySpec {
        id: "music",
        name: "Music",
        sources: &["Music"],
    },
    MigrationCategorySpec {
        id: "videos",
        name: "Videos",
        sources: &["Videos"],
    },
    MigrationCategorySpec {
        id: "downloads",
        name: "Downloads",
        sources: &["Downloads"],
    },
    MigrationCategorySpec {
        id: "app-config",
        name: "App configuration",
        sources: &[".config", ".local/share"],
    },
];

/// The ONLY desktop preferences the import may write, through the existing
/// appearance/accessibility bridges — never a blind foreign dconf load.
const ALLOWLISTED_PREFERENCES: &[&str] = &[
    "color-scheme",
    "text-scaling-factor",
    "enable-animations",
    "background-picture-uri",
];

const MIGRATION_LEDGER_DIR: &str = ".local/share/goblins-os/migration";
const MAX_ESTIMATE_ENTRIES: u64 = 250_000;

#[derive(Clone, Copy)]
struct MigrationCategorySpec {
    id: &'static str,
    name: &'static str,
    sources: &'static [&'static str],
}

#[derive(Serialize)]
pub struct FilesystemSupport {
    family: &'static str,
    readable: bool,
    driver: &'static str,
    note: &'static str,
}

#[derive(Serialize)]
pub struct MigrationCategory {
    id: &'static str,
    name: &'static str,
    sources: Vec<&'static str>,
}

#[derive(Serialize)]
pub struct MigrationCapabilities {
    source: &'static str,
    filesystems: Vec<FilesystemSupport>,
    categories: Vec<MigrationCategory>,
    allowlisted_preferences: Vec<&'static str>,
    detail: String,
}

#[derive(Serialize)]
pub struct MigrationSources {
    source: &'static str,
    sources: Vec<MigrationSource>,
    partial: bool,
    scan_errors: Vec<String>,
    executes_live_mount: bool,
    executes_live_copy: bool,
    detail: String,
}

#[derive(Serialize)]
pub struct MigrationSource {
    id: String,
    device: String,
    disk_id: String,
    disk: String,
    disk_model: String,
    disk_size_gb: u64,
    removable: bool,
    rotational: bool,
    filesystem: String,
    label: Option<String>,
    mounted_at: Option<String>,
    readable: bool,
    disabled_reason: Option<String>,
    read_only_mount_plan: Option<MigrationReadOnlyMountPlan>,
}

#[derive(Serialize)]
pub struct MigrationReadOnlyMountPlan {
    device: String,
    read_only: bool,
    executes_live_mount: bool,
    argv: Vec<String>,
    note: String,
}

#[derive(Deserialize)]
pub struct MigrationCopyPlanRequest {
    source_root: String,
    destination_home: String,
    categories: Vec<String>,
}

#[derive(Deserialize)]
pub struct MigrationEstimateRequest {
    source_root: String,
    categories: Vec<String>,
}

#[derive(Serialize)]
pub struct MigrationCopyPlanOutcome {
    ok: bool,
    text: String,
    plan: Option<MigrationCopyPlan>,
}

#[derive(Serialize)]
pub struct MigrationEstimateOutcome {
    ok: bool,
    text: String,
    estimate: Option<MigrationEstimate>,
}

#[derive(Serialize)]
pub struct MigrationCopyPlan {
    source_root: String,
    destination_home: String,
    jobs: Vec<MigrationCopyJob>,
    rsync_argv: Vec<String>,
    copied_ledger: String,
    skipped_ledger: String,
    allowlisted_preferences: Vec<&'static str>,
    executes_live_copy: bool,
}

#[derive(Serialize)]
pub struct MigrationEstimate {
    source_root: String,
    categories: Vec<MigrationCategoryEstimate>,
    total_bytes: u64,
    total_files: u64,
    missing_paths: Vec<String>,
    skipped_paths: Vec<String>,
    truncated: bool,
    executes_live_copy: bool,
}

#[derive(Serialize)]
pub struct MigrationCopyJob {
    category_id: &'static str,
    category_name: &'static str,
    source_paths: Vec<String>,
    destination: String,
}

#[derive(Serialize)]
pub struct MigrationCategoryEstimate {
    category_id: &'static str,
    category_name: &'static str,
    source_paths: Vec<MigrationPathEstimate>,
    total_bytes: u64,
    total_files: u64,
    missing_paths: Vec<String>,
    skipped_paths: Vec<String>,
    truncated: bool,
}

#[derive(Serialize)]
pub struct MigrationPathEstimate {
    path: String,
    exists: bool,
    bytes: u64,
    files: u64,
    truncated: bool,
}

pub async fn migration_capabilities() -> Json<MigrationCapabilities> {
    Json(build_migration_capabilities())
}

pub async fn migration_sources() -> Json<MigrationSources> {
    Json(build_migration_sources())
}

pub async fn migration_copy_plan(
    Json(request): Json<MigrationCopyPlanRequest>,
) -> (StatusCode, Json<MigrationCopyPlanOutcome>) {
    match build_migration_copy_plan(request) {
        Ok(plan) => (
            StatusCode::OK,
            Json(MigrationCopyPlanOutcome {
                ok: true,
                text: "Migration copy plan is ready. No files were copied by this planning step."
                    .to_string(),
                plan: Some(plan),
            }),
        ),
        Err(text) => (
            StatusCode::BAD_REQUEST,
            Json(MigrationCopyPlanOutcome {
                ok: false,
                text,
                plan: None,
            }),
        ),
    }
}

pub async fn migration_estimate(
    Json(request): Json<MigrationEstimateRequest>,
) -> (StatusCode, Json<MigrationEstimateOutcome>) {
    match build_migration_estimate(request) {
        Ok(estimate) => (
            StatusCode::OK,
            Json(MigrationEstimateOutcome {
                ok: true,
                text: if estimate.truncated {
                    "Migration estimate reached the source scan limit. The live copy plan remains unchanged and no files were copied.".to_string()
                } else {
                    "Migration estimate is ready. No files were mounted or copied by this sizing step.".to_string()
                },
                estimate: Some(estimate),
            }),
        ),
        Err(text) => (
            StatusCode::BAD_REQUEST,
            Json(MigrationEstimateOutcome {
                ok: false,
                text,
                estimate: None,
            }),
        ),
    }
}

fn build_migration_sources() -> MigrationSources {
    let ntfs = driver_present("ntfs-3g") || driver_present("mount.ntfs-3g");
    let exfat = driver_present("exfatprogs")
        || driver_present("mount.exfat")
        || driver_present("exfatfsck")
        || driver_present("fsck.exfat");
    let scan = scan_migration_source_partitions();
    let scan_errors = scan.scan_errors;
    let partial = !scan_errors.is_empty();
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let mounted_partitions = partition_mounts_from_mountinfo(&mountinfo);
    let sources =
        build_migration_sources_from_probes(scan.probes, &mounted_partitions, ntfs, exfat);
    let detail = if partial {
        "Migration source scan is ready but partial. No disks were mounted and no files were copied by this source scan. Review scan_errors before selecting a source.".to_string()
    } else if sources.is_empty() {
        "Migration source scan is ready. No disks were mounted and no files were copied by this source scan. No migration source partitions were found.".to_string()
    } else {
        "Migration source scan is ready. No disks were mounted and no files were copied by this source scan. Readable unmounted sources include a read-only mount plan for the CI/qemu first-boot job.".to_string()
    };

    MigrationSources {
        source: "goblins-os-core",
        sources,
        partial,
        scan_errors,
        executes_live_mount: false,
        executes_live_copy: false,
        detail,
    }
}

fn build_migration_sources_from_probes(
    probes: Vec<MigrationSourcePartitionProbe>,
    mounted_partitions: &BTreeMap<String, String>,
    ntfs: bool,
    exfat: bool,
) -> Vec<MigrationSource> {
    probes
        .into_iter()
        .map(|probe| migration_source_from_probe(probe, mounted_partitions, ntfs, exfat))
        .collect()
}

fn migration_source_from_probe(
    probe: MigrationSourcePartitionProbe,
    mounted_partitions: &BTreeMap<String, String>,
    ntfs: bool,
    exfat: bool,
) -> MigrationSource {
    let filesystem = migration_probe_filesystem(&probe);
    let label = migration_probe_label(&probe);
    let mounted_at = mounted_partitions.get(&probe.partition_path).cloned();
    let readability = migration_filesystem_readability(&filesystem, ntfs, exfat);
    let read_only_mount_plan = if readability.readable && mounted_at.is_none() {
        Some(read_only_mount_plan(&probe.partition_path))
    } else {
        None
    };

    MigrationSource {
        id: probe.partition_path.trim_start_matches("/dev/").to_string(),
        device: probe.partition_path,
        disk_id: probe.disk_id,
        disk: probe.disk_path,
        disk_model: probe.model,
        disk_size_gb: probe.size_gb,
        removable: probe.removable,
        rotational: probe.rotational,
        filesystem,
        label,
        mounted_at,
        readable: readability.readable,
        disabled_reason: readability.disabled_reason,
        read_only_mount_plan,
    }
}

struct MigrationFilesystemReadability {
    readable: bool,
    disabled_reason: Option<String>,
}

fn migration_filesystem_readability(
    filesystem: &str,
    ntfs: bool,
    exfat: bool,
) -> MigrationFilesystemReadability {
    let normalized = filesystem.to_ascii_lowercase();
    match normalized.as_str() {
        "ext2" | "ext3" | "ext4" | "btrfs" | "xfs" | "vfat" | "fat" | "fat16" | "fat32"
        | "msdos" => MigrationFilesystemReadability {
            readable: true,
            disabled_reason: None,
        },
        "ntfs" | "bitlocker" => {
            if ntfs {
                MigrationFilesystemReadability {
                    readable: true,
                    disabled_reason: None,
                }
            } else {
                MigrationFilesystemReadability {
                    readable: false,
                    disabled_reason: Some(
                        "Goblins can't read this Windows disk until ntfs-3g is installed."
                            .to_string(),
                    ),
                }
            }
        }
        "exfat" => {
            if exfat {
                MigrationFilesystemReadability {
                    readable: true,
                    disabled_reason: None,
                }
            } else {
                MigrationFilesystemReadability {
                    readable: false,
                    disabled_reason: Some(
                        "Goblins can't read this exFAT disk until exfatprogs is installed."
                            .to_string(),
                    ),
                }
            }
        }
        "apfs" | "hfs" | "hfsplus" | "hfs+" => MigrationFilesystemReadability {
            readable: false,
            disabled_reason: Some("Goblins can't read this disk's format (APFS).".to_string()),
        },
        "" | "unknown" => MigrationFilesystemReadability {
            readable: false,
            disabled_reason: Some(
                "Goblins can't read this disk until its filesystem is identified.".to_string(),
            ),
        },
        other => MigrationFilesystemReadability {
            readable: false,
            disabled_reason: Some(format!(
                "Goblins can't read this disk's filesystem ({other}) yet."
            )),
        },
    }
}

fn migration_probe_filesystem(probe: &MigrationSourcePartitionProbe) -> String {
    metadata_value(&probe.metadata, &["TYPE", "ID_FS_TYPE"])
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "unknown".to_string())
}

fn migration_probe_label(probe: &MigrationSourcePartitionProbe) -> Option<String> {
    metadata_value(
        &probe.metadata,
        &["LABEL", "ID_FS_LABEL", "PARTLABEL", "PARTNAME"],
    )
    .map(ToString::to_string)
}

fn metadata_value<'a>(metadata: &'a BTreeMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| metadata.get(*key).map(String::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn read_only_mount_plan(device: &str) -> MigrationReadOnlyMountPlan {
    MigrationReadOnlyMountPlan {
        device: device.to_string(),
        read_only: true,
        executes_live_mount: false,
        argv: vec![
            "udisksctl".to_string(),
            "mount".to_string(),
            "-b".to_string(),
            device.to_string(),
            "-o".to_string(),
            "ro".to_string(),
        ],
        note: "Plan only. The live first-boot job must execute and verify the read-only mount in CI/qemu.".to_string(),
    }
}

fn partition_mounts_from_mountinfo(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(parse_mountinfo_partition)
        .collect::<BTreeMap<_, _>>()
}

fn parse_mountinfo_partition(line: &str) -> Option<(String, String)> {
    let (before_separator, after_separator) = line.split_once(" - ")?;
    let mount_point = before_separator.split_whitespace().nth(4)?;
    let source = after_separator.split_whitespace().nth(1)?;
    if !source.starts_with("/dev/") {
        return None;
    }
    Some((
        source.to_string(),
        decode_mountinfo_path(mount_point).unwrap_or_else(|| mount_point.to_string()),
    ))
}

fn decode_mountinfo_path(value: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let code = [chars.next()?, chars.next()?, chars.next()?];
        let octal = code.iter().collect::<String>();
        let byte = u8::from_str_radix(&octal, 8).ok()?;
        out.push(byte as char);
    }
    Some(out)
}

fn build_migration_capabilities() -> MigrationCapabilities {
    let ntfs = driver_present("ntfs-3g") || driver_present("mount.ntfs-3g");
    let exfat = driver_present("exfatprogs")
        || driver_present("mount.exfat")
        || driver_present("exfatfsck")
        || driver_present("fsck.exfat");
    let filesystems = filesystem_table(ntfs, exfat);

    let categories = CATEGORIES
        .iter()
        .map(|category| MigrationCategory {
            id: category.id,
            name: category.name,
            sources: category.sources.to_vec(),
        })
        .collect();

    MigrationCapabilities {
        source: "goblins-os-core",
        filesystems,
        categories,
        allowlisted_preferences: ALLOWLISTED_PREFERENCES.to_vec(),
        detail: "Goblins OS can bring data over from the source filesystems marked readable. The copy step is read-only on the source and additive into your new home.".to_string(),
    }
}

fn build_migration_copy_plan(
    request: MigrationCopyPlanRequest,
) -> Result<MigrationCopyPlan, String> {
    let source_root = normalize_absolute_dir(&request.source_root, "source root")?;
    let destination_home = normalize_absolute_dir(&request.destination_home, "destination home")?;
    if source_root == destination_home {
        return Err("Migration source and destination must be different directories.".to_string());
    }
    if destination_home.starts_with(&format!("{source_root}/")) {
        return Err("Migration destination cannot be inside the selected source.".to_string());
    }

    let categories = selected_categories(&request.categories)?;
    let jobs = migration_copy_jobs(&source_root, &destination_home, &categories);
    let copied_ledger = join_absolute(&destination_home, MIGRATION_LEDGER_DIR, "copied.tsv");
    let skipped_ledger = join_absolute(&destination_home, MIGRATION_LEDGER_DIR, "skipped.tsv");
    let rsync_argv = migration_rsync_argv(&jobs, &destination_home, &copied_ledger);

    Ok(MigrationCopyPlan {
        source_root,
        destination_home,
        jobs,
        rsync_argv,
        copied_ledger,
        skipped_ledger,
        allowlisted_preferences: ALLOWLISTED_PREFERENCES.to_vec(),
        executes_live_copy: false,
    })
}

fn build_migration_estimate(
    request: MigrationEstimateRequest,
) -> Result<MigrationEstimate, String> {
    let source_root = normalize_absolute_dir(&request.source_root, "source root")?;
    let categories = selected_categories(&request.categories)?;
    let mut scan = MigrationSizeScan::default();
    let estimates = categories
        .iter()
        .map(|category| estimate_category(&source_root, category, &mut scan))
        .collect::<Vec<_>>();
    let total_bytes = estimates.iter().map(|estimate| estimate.total_bytes).sum();
    let total_files = estimates.iter().map(|estimate| estimate.total_files).sum();
    let missing_paths = estimates
        .iter()
        .flat_map(|estimate| estimate.missing_paths.iter().cloned())
        .collect();
    let skipped_paths = estimates
        .iter()
        .flat_map(|estimate| estimate.skipped_paths.iter().cloned())
        .collect();
    let truncated = estimates.iter().any(|estimate| estimate.truncated);

    Ok(MigrationEstimate {
        source_root,
        categories: estimates,
        total_bytes,
        total_files,
        missing_paths,
        skipped_paths,
        truncated,
        executes_live_copy: false,
    })
}

fn selected_categories(ids: &[String]) -> Result<Vec<MigrationCategorySpec>, String> {
    if ids.is_empty() {
        return Err("Choose at least one migration category.".to_string());
    }
    let mut selected = Vec::with_capacity(ids.len());
    for id in ids {
        let id = id.trim();
        let Some(category) = CATEGORIES.iter().find(|category| category.id == id) else {
            return Err(format!(
                "Migration category '{id}' is not supported by Goblins OS."
            ));
        };
        if selected
            .iter()
            .any(|candidate: &MigrationCategorySpec| candidate.id == category.id)
        {
            return Err("Migration categories cannot contain duplicates.".to_string());
        }
        selected.push(*category);
    }
    Ok(selected)
}

fn estimate_category(
    source_root: &str,
    category: &MigrationCategorySpec,
    scan: &mut MigrationSizeScan,
) -> MigrationCategoryEstimate {
    let mut source_paths = Vec::new();
    let mut missing_paths = Vec::new();
    let mut skipped_paths = Vec::new();
    let mut total_bytes = 0;
    let mut total_files = 0;
    let mut truncated = false;

    for source in category.sources {
        let path = join_absolute(source_root, source, "");
        let mut path_estimate = MigrationPathEstimate {
            path: path.clone(),
            exists: false,
            bytes: 0,
            files: 0,
            truncated: false,
        };
        let path_buf = PathBuf::from(&path);
        if !path_buf.exists() {
            missing_paths.push(path);
            source_paths.push(path_estimate);
            continue;
        }

        path_estimate.exists = true;
        let mut local_skipped = Vec::new();
        let estimate = estimate_path_size(&path_buf, scan, &mut local_skipped);
        path_estimate.bytes = estimate.bytes;
        path_estimate.files = estimate.files;
        path_estimate.truncated = estimate.truncated;
        total_bytes += estimate.bytes;
        total_files += estimate.files;
        truncated |= estimate.truncated;
        skipped_paths.extend(local_skipped);
        source_paths.push(path_estimate);
    }

    MigrationCategoryEstimate {
        category_id: category.id,
        category_name: category.name,
        source_paths,
        total_bytes,
        total_files,
        missing_paths,
        skipped_paths,
        truncated,
    }
}

fn migration_copy_jobs(
    source_root: &str,
    destination_home: &str,
    categories: &[MigrationCategorySpec],
) -> Vec<MigrationCopyJob> {
    categories
        .iter()
        .map(|category| MigrationCopyJob {
            category_id: category.id,
            category_name: category.name,
            source_paths: category
                .sources
                .iter()
                .map(|source| join_absolute(source_root, source, ""))
                .collect(),
            destination: ensure_trailing_slash(destination_home),
        })
        .collect()
}

fn migration_rsync_argv(
    jobs: &[MigrationCopyJob],
    destination_home: &str,
    copied_ledger: &str,
) -> Vec<String> {
    let mut argv = vec![
        "rsync".to_string(),
        "-aHAX".to_string(),
        "--no-owner".to_string(),
        "--no-group".to_string(),
        "--ignore-existing".to_string(),
        "--ignore-missing-args".to_string(),
        "--safe-links".to_string(),
        "--protect-args".to_string(),
        "--human-readable".to_string(),
        "--info=progress2".to_string(),
        "--itemize-changes".to_string(),
        "--out-format=%i\t%n%L".to_string(),
        format!("--log-file={copied_ledger}"),
        "--exclude=.cache/".to_string(),
        "--exclude=.local/share/Trash/".to_string(),
    ];
    for job in jobs {
        argv.extend(
            job.source_paths
                .iter()
                .map(|path| ensure_trailing_slash(path)),
        );
    }
    argv.push(ensure_trailing_slash(destination_home));
    argv
}

#[derive(Default)]
struct MigrationSizeScan {
    entries: u64,
}

#[derive(Default)]
struct MigrationSizeEstimate {
    bytes: u64,
    files: u64,
    truncated: bool,
}

fn estimate_path_size(
    path: &Path,
    scan: &mut MigrationSizeScan,
    skipped_paths: &mut Vec<String>,
) -> MigrationSizeEstimate {
    if scan.entries >= MAX_ESTIMATE_ENTRIES {
        skipped_paths.push(path.display().to_string());
        return MigrationSizeEstimate {
            truncated: true,
            ..MigrationSizeEstimate::default()
        };
    }
    scan.entries += 1;

    let Ok(metadata) = fs::symlink_metadata(path) else {
        skipped_paths.push(path.display().to_string());
        return MigrationSizeEstimate::default();
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        skipped_paths.push(path.display().to_string());
        return MigrationSizeEstimate::default();
    }
    if metadata.is_file() {
        return MigrationSizeEstimate {
            bytes: metadata.len(),
            files: 1,
            truncated: false,
        };
    }
    if !metadata.is_dir() {
        skipped_paths.push(path.display().to_string());
        return MigrationSizeEstimate::default();
    }

    let Ok(entries) = fs::read_dir(path) else {
        skipped_paths.push(path.display().to_string());
        return MigrationSizeEstimate::default();
    };
    let mut estimate = MigrationSizeEstimate::default();
    for entry in entries {
        let Ok(entry) = entry else {
            skipped_paths.push(path.display().to_string());
            continue;
        };
        let child = estimate_path_size(&entry.path(), scan, skipped_paths);
        estimate.bytes += child.bytes;
        estimate.files += child.files;
        estimate.truncated |= child.truncated;
        if child.truncated {
            break;
        }
    }
    estimate
}

fn normalize_absolute_dir(raw: &str, label: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(format!("Migration {label} is required."));
    }
    if value.contains('\0') || value.contains('\n') {
        return Err(format!(
            "Migration {label} contains unsupported characters."
        ));
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(format!("Migration {label} must be an absolute path."));
    }
    if path.parent().is_none() {
        return Err(format!("Migration {label} cannot be the filesystem root."));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("Migration {label} cannot contain '..' components."));
    }
    Ok(trim_trailing_slashes(value))
}

fn trim_trailing_slashes(value: &str) -> String {
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn ensure_trailing_slash(value: &str) -> String {
    if value.ends_with('/') {
        value.to_string()
    } else {
        format!("{value}/")
    }
}

fn join_absolute(root: &str, relative: &str, leaf: &str) -> String {
    let mut out = trim_trailing_slashes(root);
    for part in relative.split('/').filter(|part| !part.is_empty()) {
        out.push('/');
        out.push_str(part);
    }
    if !leaf.is_empty() {
        out.push('/');
        out.push_str(leaf);
    }
    out
}

/// Which source filesystem families Goblins OS can read. Kernel filesystems are
/// always readable; NTFS/exFAT depend on their userspace drivers being present;
/// APFS/HFS+ are not shipped. Pure + unit-tested so a drive is never silently
/// treated as readable.
fn filesystem_table(ntfs: bool, exfat: bool) -> Vec<FilesystemSupport> {
    vec![
        FilesystemSupport {
            family: "ext4 / btrfs / xfs",
            readable: true,
            driver: "kernel",
            note: "Linux filesystems are read directly by the kernel.",
        },
        FilesystemSupport {
            family: "FAT32",
            readable: true,
            driver: "kernel",
            note: "Read directly by the kernel.",
        },
        FilesystemSupport {
            family: "NTFS",
            readable: ntfs,
            driver: "ntfs-3g",
            note: if ntfs {
                "Windows drives can be read."
            } else {
                "Install the ntfs-3g driver to read Windows drives."
            },
        },
        FilesystemSupport {
            family: "exFAT",
            readable: exfat,
            driver: "exfatprogs",
            note: if exfat {
                "Cross-platform exFAT drives can be read."
            } else {
                "Install the exfatprogs driver to read exFAT drives."
            },
        },
        FilesystemSupport {
            family: "APFS / HFS+",
            readable: false,
            driver: "none",
            note: "Goblins can't read this disk's format (APFS).",
        },
    ]
}

fn driver_present(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{
        build_migration_capabilities, build_migration_copy_plan, build_migration_estimate,
        build_migration_sources_from_probes, filesystem_table, partition_mounts_from_mountinfo,
        MigrationCopyPlanRequest, MigrationEstimateRequest,
    };
    use crate::install_targets::scan_migration_source_partitions_in;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    fn temp_migration_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "goblins-migration-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn filesystem_table_gates_on_drivers() {
        let table = filesystem_table(false, false);
        let ntfs = table.iter().find(|f| f.family == "NTFS").unwrap();
        let exfat = table.iter().find(|f| f.family == "exFAT").unwrap();
        let apfs = table.iter().find(|f| f.family == "APFS / HFS+").unwrap();
        assert!(!ntfs.readable); // no driver → not readable
        assert!(!exfat.readable);
        assert!(!apfs.readable); // never readable
                                 // Kernel filesystems are always readable.
        assert!(table.iter().find(|f| f.family == "FAT32").unwrap().readable);

        let with_drivers = filesystem_table(true, true);
        assert!(
            with_drivers
                .iter()
                .find(|f| f.family == "NTFS")
                .unwrap()
                .readable
        );
        assert!(
            with_drivers
                .iter()
                .find(|f| f.family == "exFAT")
                .unwrap()
                .readable
        );
        // APFS stays unreadable even with the other drivers present.
        assert!(
            !with_drivers
                .iter()
                .find(|f| f.family == "APFS / HFS+")
                .unwrap()
                .readable
        );
    }

    #[test]
    fn capabilities_include_stable_category_ids_and_allowlisted_preferences() {
        let capabilities = build_migration_capabilities();
        assert!(capabilities
            .categories
            .iter()
            .any(|category| category.id == "documents-desktop"
                && category.sources == vec!["Documents", "Desktop"]));
        assert!(capabilities
            .allowlisted_preferences
            .contains(&"color-scheme"));
        assert!(!capabilities
            .allowlisted_preferences
            .iter()
            .any(|key| key.contains("dconf")));
    }

    #[test]
    fn migration_sources_classify_sysfs_partitions_without_mounting() {
        let root = temp_migration_root("source-scan");
        let disk = root.join("sda");
        fs::create_dir_all(disk.join("queue")).unwrap();
        fs::create_dir_all(disk.join("device")).unwrap();
        fs::write(disk.join("size"), "268435456\n").unwrap();
        fs::write(disk.join("removable"), "1\n").unwrap();
        fs::write(disk.join("queue/rotational"), "1\n").unwrap();
        fs::write(disk.join("device/model"), "Migration Source Disk\n").unwrap();

        for (name, metadata) in [
            (
                "sda1",
                "DEVNAME=sda1\nDEVTYPE=partition\nTYPE=ext4\nLABEL=Old Home\n",
            ),
            (
                "sda2",
                "DEVNAME=sda2\nDEVTYPE=partition\nTYPE=apfs\nPARTLABEL=Macintosh HD\n",
            ),
            (
                "sda3",
                "DEVNAME=sda3\nDEVTYPE=partition\nTYPE=exfat\nLABEL=Camera Archive\n",
            ),
        ] {
            let partition = disk.join(name);
            fs::create_dir_all(&partition).unwrap();
            fs::write(partition.join("partition"), "1\n").unwrap();
            fs::write(partition.join("uevent"), metadata).unwrap();
        }

        let scan = scan_migration_source_partitions_in(&root);
        assert!(scan.scan_errors.is_empty());
        let mounted = partition_mounts_from_mountinfo(
            "31 25 8:1 / /run/media/goblin/Old\\040Home rw,relatime - ext4 /dev/sda1 rw\n",
        );
        let sources = build_migration_sources_from_probes(scan.probes, &mounted, true, true);
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(sources.len(), 3);
        let ext4 = sources
            .iter()
            .find(|source| source.device == "/dev/sda1")
            .unwrap();
        assert!(ext4.readable);
        assert_eq!(
            ext4.mounted_at.as_deref(),
            Some("/run/media/goblin/Old Home")
        );
        assert!(ext4.read_only_mount_plan.is_none());

        let apfs = sources
            .iter()
            .find(|source| source.device == "/dev/sda2")
            .unwrap();
        assert!(!apfs.readable);
        assert_eq!(
            apfs.disabled_reason.as_deref(),
            Some("Goblins can't read this disk's format (APFS).")
        );
        assert!(apfs.read_only_mount_plan.is_none());

        let exfat = sources
            .iter()
            .find(|source| source.device == "/dev/sda3")
            .unwrap();
        let plan = exfat.read_only_mount_plan.as_ref().unwrap();
        assert!(exfat.readable);
        assert!(plan.read_only);
        assert!(!plan.executes_live_mount);
        assert_eq!(
            plan.argv,
            vec!["udisksctl", "mount", "-b", "/dev/sda3", "-o", "ro"]
        );
    }

    #[test]
    fn migration_copy_plan_builds_additive_rsync_argv_and_ledgers() {
        let plan = build_migration_copy_plan(MigrationCopyPlanRequest {
            source_root: "/run/media/goblin/Old Home".to_string(),
            destination_home: "/var/home/goblin".to_string(),
            categories: vec!["documents-desktop".to_string(), "pictures".to_string()],
        })
        .expect("valid migration plan");

        assert!(!plan.executes_live_copy);
        assert_eq!(plan.jobs.len(), 2);
        assert_eq!(
            plan.jobs[0].source_paths,
            vec![
                "/run/media/goblin/Old Home/Documents".to_string(),
                "/run/media/goblin/Old Home/Desktop".to_string(),
            ]
        );
        assert!(plan.rsync_argv.contains(&"--info=progress2".to_string()));
        assert!(plan.rsync_argv.contains(&"--ignore-existing".to_string()));
        assert!(plan.rsync_argv.contains(&"--safe-links".to_string()));
        assert!(plan
            .rsync_argv
            .contains(&"/run/media/goblin/Old Home/Documents/".to_string()));
        assert_eq!(plan.rsync_argv.last().unwrap(), "/var/home/goblin/");
        assert_eq!(
            plan.copied_ledger,
            "/var/home/goblin/.local/share/goblins-os/migration/copied.tsv"
        );
        assert_eq!(
            plan.skipped_ledger,
            "/var/home/goblin/.local/share/goblins-os/migration/skipped.tsv"
        );
    }

    #[test]
    fn migration_copy_plan_rejects_unsafe_or_unknown_inputs() {
        assert!(build_migration_copy_plan(MigrationCopyPlanRequest {
            source_root: "relative".to_string(),
            destination_home: "/var/home/goblin".to_string(),
            categories: vec!["documents-desktop".to_string()],
        })
        .is_err());
        assert!(build_migration_copy_plan(MigrationCopyPlanRequest {
            source_root: "/run/media/goblin/old".to_string(),
            destination_home: "/run/media/goblin/old/nested".to_string(),
            categories: vec!["documents-desktop".to_string()],
        })
        .is_err());
        assert!(build_migration_copy_plan(MigrationCopyPlanRequest {
            source_root: "/run/media/goblin/old".to_string(),
            destination_home: "/var/home/goblin".to_string(),
            categories: vec!["unknown".to_string()],
        })
        .is_err());
        assert!(build_migration_copy_plan(MigrationCopyPlanRequest {
            source_root: "/run/media/goblin/old".to_string(),
            destination_home: "/var/home/goblin".to_string(),
            categories: vec!["pictures".to_string(), "pictures".to_string()],
        })
        .is_err());
    }

    #[test]
    fn migration_estimate_counts_selected_categories_without_copying() {
        let root = temp_migration_root("estimate-counts");
        write_file(&root.join("Documents/report.txt"), b"hello");
        write_file(&root.join("Desktop/note.txt"), b"desktop");
        write_file(&root.join("Pictures/image.raw"), &[1, 2, 3, 4]);
        write_file(&root.join("Downloads/ignored.bin"), &[9, 9, 9]);

        let estimate = build_migration_estimate(MigrationEstimateRequest {
            source_root: root.display().to_string(),
            categories: vec!["documents-desktop".to_string(), "pictures".to_string()],
        })
        .unwrap();

        assert!(!estimate.executes_live_copy);
        assert!(!estimate.truncated);
        assert_eq!(estimate.total_files, 3);
        assert_eq!(estimate.total_bytes, 5 + 7 + 4);
        assert_eq!(estimate.categories.len(), 2);
        assert!(estimate.missing_paths.is_empty());
        assert!(estimate.skipped_paths.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_estimate_reports_missing_and_skipped_paths_honestly() {
        let root = temp_migration_root("estimate-missing");
        write_file(&root.join("Documents/report.txt"), b"hello");
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            root.join("Documents/report.txt"),
            root.join("Documents/report-link.txt"),
        )
        .unwrap();

        let estimate = build_migration_estimate(MigrationEstimateRequest {
            source_root: root.display().to_string(),
            categories: vec!["documents-desktop".to_string(), "music".to_string()],
        })
        .unwrap();

        assert!(!estimate.executes_live_copy);
        assert_eq!(estimate.total_files, 1);
        assert!(estimate
            .missing_paths
            .iter()
            .any(|path| path.ends_with("/Desktop")));
        assert!(estimate
            .missing_paths
            .iter()
            .any(|path| path.ends_with("/Music")));
        #[cfg(unix)]
        assert!(estimate
            .skipped_paths
            .iter()
            .any(|path| path.ends_with("/Documents/report-link.txt")));

        fs::remove_dir_all(root).unwrap();
    }
}
