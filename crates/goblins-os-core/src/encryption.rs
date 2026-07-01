//! Read-only disk-encryption posture for Settings.
//!
//! The install-time encryption flow is boot-critical and remains qemu-gated.
//! This module only reports live posture from stable OS interfaces. It never
//! enrolls TPM keys, creates recovery keys, edits crypttab, or starts installs.

use std::{
    env, fs,
    path::Path,
    process::{Command, Stdio},
};

use axum::Json;
use serde::Serialize;

const DEFAULT_MOUNTINFO: &str = "/proc/self/mountinfo";
const DEFAULT_CRYPTTAB: &str = "/etc/crypttab";
const SECURE_BOOT_VAR: &str =
    "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";

#[derive(Serialize)]
pub struct EncryptionStatus {
    source: &'static str,
    available: bool,
    encrypted: bool,
    cryptsetup_available: bool,
    systemd_cryptenroll_available: bool,
    crypttab_available: bool,
    tpm_device_present: bool,
    secure_boot: SecureBootEncryptionStatus,
    root: RootEncryptionStatus,
    tpm2_enrolled: bool,
    crypttab_tpm2_configured: bool,
    recovery_key_enrolled: bool,
    auto_unlock_ready: bool,
    recovery_key_required: bool,
    executes_enrollment: bool,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RootEncryptionStatus {
    mount_source: Option<String>,
    mapper_name: Option<String>,
    backing_device: Option<String>,
    luks_type: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecureBootEncryptionStatus {
    Enabled,
    Disabled,
    Unavailable,
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MountInfoEntry {
    mount_point: String,
    source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CrypttabEntry {
    name: String,
    source: String,
    options: String,
}

#[derive(Clone, Copy)]
struct EncryptionDetailInput {
    encrypted: bool,
    cryptsetup_available: bool,
    systemd_cryptenroll_available: bool,
    tpm2_enrolled: bool,
    crypttab_tpm2_configured: bool,
    recovery_key_enrolled: bool,
    tpm_device_present: bool,
    secure_boot: SecureBootEncryptionStatus,
}

pub async fn encryption_status() -> Json<EncryptionStatus> {
    Json(build_encryption_status())
}

fn build_encryption_status() -> EncryptionStatus {
    let cryptsetup_available = executable_exists("cryptsetup");
    let systemd_cryptenroll_available = executable_exists("systemd-cryptenroll");
    let tpm_device_present = path_exists("/dev/tpmrm0") || path_exists("/dev/tpm0");
    let secure_boot = read_secure_boot_status();
    let crypttab_path = env::var("GOBLINS_OS_CRYPTTAB").unwrap_or_else(|_| DEFAULT_CRYPTTAB.into());
    let crypttab_entries = read_crypttab_entries(&crypttab_path);
    let crypttab_available = crypttab_entries.is_some();
    let crypttab_entries = crypttab_entries.unwrap_or_default();
    let root_mount = read_root_mount();
    let mount_source = root_mount.as_ref().map(|entry| entry.source.clone());
    let mapper_name = mount_source.as_deref().and_then(mapper_name_from_source);
    let root_crypttab = mapper_name
        .as_deref()
        .and_then(|name| crypttab_entries.iter().find(|entry| entry.name == name))
        .or_else(|| {
            mount_source
                .as_deref()
                .and_then(|source| crypttab_entries.iter().find(|entry| entry.source == source))
        });
    let backing_device = root_crypttab.map(|entry| entry.source.clone());
    let crypttab_tpm2_configured =
        root_crypttab.is_some_and(|entry| entry.options.contains("tpm2-device"));
    let cryptsetup_status = mapper_name
        .as_deref()
        .filter(|_| cryptsetup_available)
        .and_then(run_cryptsetup_status);
    let luks_type = cryptsetup_status
        .as_deref()
        .and_then(parse_cryptsetup_luks_type);
    let encrypted = luks_type.is_some()
        || mapper_name
            .as_ref()
            .is_some_and(|name| crypttab_entries.iter().any(|entry| entry.name == *name));

    let enroll_target = backing_device
        .as_deref()
        .or(mount_source.as_deref())
        .filter(|_| encrypted && systemd_cryptenroll_available);
    let enrollment = enroll_target
        .and_then(run_systemd_cryptenroll_list)
        .map(|output| parse_systemd_cryptenroll_list(&output));
    let tpm2_enrolled = enrollment.as_ref().is_some_and(|enrollment| enrollment.0);
    let recovery_key_enrolled = enrollment.as_ref().is_some_and(|enrollment| enrollment.1);
    let auto_unlock_ready = encrypted
        && tpm2_enrolled
        && tpm_device_present
        && matches!(
            secure_boot,
            SecureBootEncryptionStatus::Enabled | SecureBootEncryptionStatus::Disabled
        );
    let recovery_key_required = encrypted && !recovery_key_enrolled;
    let detail = encryption_detail(EncryptionDetailInput {
        encrypted,
        cryptsetup_available,
        systemd_cryptenroll_available,
        tpm2_enrolled,
        crypttab_tpm2_configured,
        recovery_key_enrolled,
        tpm_device_present,
        secure_boot,
    });

    EncryptionStatus {
        source: "goblins-os-core",
        available: cryptsetup_available || mount_source.is_some(),
        encrypted,
        cryptsetup_available,
        systemd_cryptenroll_available,
        crypttab_available,
        tpm_device_present,
        secure_boot,
        root: RootEncryptionStatus {
            mount_source,
            mapper_name,
            backing_device,
            luks_type,
        },
        tpm2_enrolled,
        crypttab_tpm2_configured,
        recovery_key_enrolled,
        auto_unlock_ready,
        recovery_key_required,
        executes_enrollment: false,
        detail,
    }
}

fn encryption_detail(input: EncryptionDetailInput) -> String {
    let EncryptionDetailInput {
        encrypted,
        cryptsetup_available,
        systemd_cryptenroll_available,
        tpm2_enrolled,
        crypttab_tpm2_configured,
        recovery_key_enrolled,
        tpm_device_present,
        secure_boot,
    } = input;

    if !cryptsetup_available {
        return "Encryption posture is read-only here because cryptsetup is not installed."
            .to_string();
    }
    if !encrypted {
        return "Root encryption is not reported for this booted system. Install-time encryption remains qemu-gated."
            .to_string();
    }

    let mut parts = vec!["Root is reported as LUKS-encrypted.".to_string()];
    if !systemd_cryptenroll_available {
        parts.push("systemd-cryptenroll is unavailable, so TPM and recovery-key enrollment cannot be read.".to_string());
    } else if tpm2_enrolled {
        if tpm_device_present {
            parts.push("A TPM2 token is enrolled for unlock.".to_string());
        } else {
            parts.push(
                "A TPM2 token is enrolled, but no TPM device is visible in this session."
                    .to_string(),
            );
        }
    } else if crypttab_tpm2_configured {
        parts.push(
            "crypttab requests TPM2 unlock, but no enrolled TPM2 token is reported.".to_string(),
        );
    } else {
        parts.push("No TPM2 unlock token is reported.".to_string());
    }

    if recovery_key_enrolled {
        parts.push("A recovery-key token is reported.".to_string());
    } else {
        parts.push(
            "No recovery-key token is reported; Goblins OS must not enable TPM-only install without escrow."
                .to_string(),
        );
    }

    match secure_boot {
        SecureBootEncryptionStatus::Enabled => {
            parts.push("Secure Boot is enabled for the TPM posture check.".to_string())
        }
        SecureBootEncryptionStatus::Disabled => {
            parts.push("Secure Boot is disabled; TPM auto-unlock posture is weaker.".to_string())
        }
        SecureBootEncryptionStatus::Unavailable => {
            parts.push("Secure Boot state is unavailable in this session.".to_string())
        }
        SecureBootEncryptionStatus::Unreadable => {
            parts.push("Secure Boot state could not be read in this session.".to_string())
        }
    }

    parts.join(" ")
}

fn read_root_mount() -> Option<MountInfoEntry> {
    let mountinfo_path =
        env::var("GOBLINS_OS_MOUNTINFO").unwrap_or_else(|_| DEFAULT_MOUNTINFO.to_string());
    let text = fs::read_to_string(mountinfo_path).ok()?;
    parse_mountinfo(&text)
        .into_iter()
        .find(|entry| entry.mount_point == "/")
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
        source: post_fields[1].to_string(),
    })
}

fn mapper_name_from_source(source: &str) -> Option<String> {
    source
        .strip_prefix("/dev/mapper/")
        .or_else(|| source.strip_prefix("/dev/disk/by-id/dm-name-"))
        .map(|name| name.to_string())
}

fn unescape_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn read_crypttab_entries(path: &str) -> Option<Vec<CrypttabEntry>> {
    let text = fs::read_to_string(path).ok()?;
    Some(parse_crypttab(&text))
}

fn parse_crypttab(text: &str) -> Vec<CrypttabEntry> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let fields = trimmed.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 2 {
                return None;
            }
            Some(CrypttabEntry {
                name: fields[0].to_string(),
                source: fields[1].to_string(),
                options: fields.get(3).copied().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

fn parse_cryptsetup_luks_type(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        let value = trimmed.strip_prefix("type:")?.trim();
        if value.to_ascii_lowercase().starts_with("luks") {
            Some(value.to_string())
        } else {
            None
        }
    })
}

fn parse_systemd_cryptenroll_list(output: &str) -> (bool, bool) {
    let mut tpm2 = false;
    let mut recovery = false;
    for line in output.lines() {
        let lower = line.to_ascii_lowercase();
        tpm2 |= lower.contains("tpm2");
        recovery |= lower.contains("recovery");
    }
    (tpm2, recovery)
}

fn read_secure_boot_status() -> SecureBootEncryptionStatus {
    let path = env::var("GOBLINS_OS_SECURE_BOOT_VAR").unwrap_or_else(|_| SECURE_BOOT_VAR.into());
    let path = Path::new(&path);
    if !path.exists() {
        return SecureBootEncryptionStatus::Unavailable;
    }
    match fs::read(path) {
        Ok(bytes) if bytes.get(4) == Some(&1) => SecureBootEncryptionStatus::Enabled,
        Ok(bytes) if bytes.get(4) == Some(&0) => SecureBootEncryptionStatus::Disabled,
        Ok(_) | Err(_) => SecureBootEncryptionStatus::Unreadable,
    }
}

fn run_cryptsetup_status(name: &str) -> Option<String> {
    command_output("cryptsetup", &["status", name])
}

fn run_systemd_cryptenroll_list(device: &str) -> Option<String> {
    command_output("systemd-cryptenroll", &["--list", device])
}

fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn executable_exists(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountinfo_extracts_root_mapper_source() {
        let entries = parse_mountinfo(
            "29 1 0:28 / / rw,relatime - overlay overlay rw\n\
             31 1 253:0 / /sysroot rw,relatime - xfs /dev/mapper/luks-root rw\n",
        );

        assert_eq!(entries[0].mount_point, "/");
        assert_eq!(entries[0].source, "overlay");
        assert_eq!(
            mapper_name_from_source("/dev/mapper/luks-root").as_deref(),
            Some("luks-root")
        );
    }

    #[test]
    fn crypttab_parser_keeps_name_source_and_options() {
        let entries = parse_crypttab(
            "# comment\n\
             luks-root UUID=1111-2222 none tpm2-device=auto,tpm2-pcrs=7,luks\n",
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "luks-root");
        assert_eq!(entries[0].source, "UUID=1111-2222");
        assert!(entries[0].options.contains("tpm2-pcrs=7"));
    }

    #[test]
    fn cryptsetup_status_reports_luks_type_only() {
        let output = "/dev/mapper/luks-root is active.\n  type:    LUKS2\n";
        assert_eq!(parse_cryptsetup_luks_type(output).as_deref(), Some("LUKS2"));
        assert_eq!(parse_cryptsetup_luks_type("type: plain"), None);
    }

    #[test]
    fn cryptenroll_parser_detects_tpm2_and_recovery_tokens() {
        let output = "SLOT TYPE\n0 password\n1 recovery\n2 tpm2\n";
        assert_eq!(parse_systemd_cryptenroll_list(output), (true, true));
        assert_eq!(
            parse_systemd_cryptenroll_list("0 password\n"),
            (false, false)
        );
    }

    #[test]
    fn detail_requires_recovery_key_for_encrypted_tpm_only_posture() {
        let detail = encryption_detail(EncryptionDetailInput {
            encrypted: true,
            cryptsetup_available: true,
            systemd_cryptenroll_available: true,
            tpm2_enrolled: true,
            crypttab_tpm2_configured: false,
            recovery_key_enrolled: false,
            tpm_device_present: true,
            secure_boot: SecureBootEncryptionStatus::Enabled,
        });

        assert!(detail.contains("Root is reported as LUKS-encrypted."));
        assert!(detail.contains("must not enable TPM-only install without escrow"));
    }
}
