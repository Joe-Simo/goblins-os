//! Migration Assistant substrate (read-only source capability + category model).
//!
//! The macOS "Migration Assistant" altitude: bring a previous home over on first
//! boot. This module ships the host-testable foundation — which source filesystems
//! Goblins OS can actually read (so a drive it can't read is shown disabled, never
//! silently skipped), the category model, and the allowlisted preference keys the
//! import is permitted to write. The udisks/rsync
//! copy job and the installer page are the deliberate CI/qemu follow-up; nothing
//! here mounts, copies, or writes — it is pure capability reporting.

use axum::Json;
use serde::Serialize;

/// A migratable data category and the source-relative directories it covers.
const CATEGORIES: &[(&str, &[&str])] = &[
    ("Documents & Desktop", &["Documents", "Desktop"]),
    ("Pictures", &["Pictures"]),
    ("Music", &["Music"]),
    ("Videos", &["Videos"]),
    ("Downloads", &["Downloads"]),
    ("App configuration", &[".config", ".local/share"]),
];

/// The ONLY desktop preferences the import may write, through the existing
/// appearance/accessibility bridges — never a blind foreign dconf load.
const ALLOWLISTED_PREFERENCES: &[&str] = &[
    "color-scheme",
    "text-scaling-factor",
    "enable-animations",
    "background-picture-uri",
];

#[derive(Serialize)]
pub struct FilesystemSupport {
    family: &'static str,
    readable: bool,
    driver: &'static str,
    note: &'static str,
}

#[derive(Serialize)]
pub struct MigrationCategory {
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

pub async fn migration_capabilities() -> Json<MigrationCapabilities> {
    Json(build_migration_capabilities())
}

fn build_migration_capabilities() -> MigrationCapabilities {
    let ntfs = driver_present("ntfs-3g") || driver_present("mount.ntfs-3g");
    let exfat = driver_present("exfatprogs")
        || driver_present("mount.exfat")
        || driver_present("exfatfsck");
    let filesystems = filesystem_table(ntfs, exfat);

    let categories = CATEGORIES
        .iter()
        .map(|(name, sources)| MigrationCategory {
            name,
            sources: sources.to_vec(),
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
    use super::filesystem_table;

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
}
