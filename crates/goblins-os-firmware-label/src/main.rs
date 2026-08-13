// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const EFIBOOTMGR: &str = "/usr/sbin/efibootmgr";
const EFIVAR: &str = "/usr/bin/efivar";
const FINDMNT: &str = "/usr/bin/findmnt";
const LSBLK: &str = "/usr/bin/lsblk";
const EFI_GLOBAL_GUID: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";
const EFIVARS: &str = "/sys/firmware/efi/efivars";
const EXPECTED_LOADER: &str = "\\EFI\\fedora\\shimaa64.efi";
const OLD_LABEL: &str = "Fedora";
const NEW_LABEL: &str = "Goblins OS";
const STATE_DIR: &str = "/var/lib/goblins-os-firmware-label";
const COMPLETE_MARKER: &str = "/var/lib/goblins-os-firmware-label/migrated-v1";
const MAX_COMMAND_OUTPUT: usize = 1024 * 1024;
const MAX_VARIABLE_SIZE: usize = 1024 * 1024;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct BootSnapshot {
    current_id: String,
    label: String,
    device_path: String,
    boot_order: Option<String>,
    boot_next: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EspIdentity {
    physical_disk: PathBuf,
    part_uuid: String,
    part_number: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Outcome {
    AlreadyComplete,
    AlreadyBranded,
    Migrated,
    NotApplicable,
}

fn main() {
    match run() {
        Ok(Outcome::AlreadyComplete) => {
            println!("goblins-os-firmware-label: migration was already completed");
        }
        Ok(Outcome::AlreadyBranded) => {
            println!("goblins-os-firmware-label: the active firmware entry is already branded");
        }
        Ok(Outcome::Migrated) => {
            println!(
                "goblins-os-firmware-label: relabeled the exact active Goblins OS firmware entry"
            );
        }
        Ok(Outcome::NotApplicable) => {
            println!("goblins-os-firmware-label: no legacy Goblins OS firmware entry is active");
        }
        Err(error) => {
            eprintln!("goblins-os-firmware-label: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<Outcome> {
    if Path::new(COMPLETE_MARKER).exists() || Path::new(COMPLETE_MARKER).is_symlink() {
        verify_complete_marker()?;
        return Ok(Outcome::AlreadyComplete);
    }
    if std::env::consts::ARCH != "aarch64" || !Path::new(EFIVARS).is_dir() {
        return Ok(Outcome::NotApplicable);
    }
    verify_product_identity()?;
    verify_runtime_tools()?;
    verify_bootupd_is_not_running()?;

    let initial = read_boot_snapshot()?;
    if initial.label != OLD_LABEL && initial.label != NEW_LABEL {
        return Ok(Outcome::NotApplicable);
    }

    let loader = find_exact_loader()?;
    let esp = inspect_esp(&loader)?;
    let root_disk = inspect_root_disk()?;
    if esp.physical_disk != root_disk {
        return Err(format!(
            "the mounted ESP ({}) and /sysroot ({}) do not resolve to the same single physical disk",
            esp.physical_disk.display(),
            root_disk.display()
        ));
    }
    validate_device_path(&initial.device_path, &esp.part_uuid, &esp.part_number)?;

    let variable_path = variable_path(&initial.current_id);
    let initial_bytes = read_variable(&variable_path)?;
    let initial_label = variable_label(&initial_bytes)?;
    if initial_label != initial.label {
        return Err("BootCurrent's efivarfs label does not match efibootmgr output".to_string());
    }

    if initial.label == NEW_LABEL {
        write_complete_marker()?;
        return Ok(Outcome::AlreadyBranded);
    }

    let rewritten = rewrite_variable_label(&initial_bytes, OLD_LABEL, NEW_LABEL)?;

    // Re-read both views immediately before the only firmware mutation. Any
    // concurrent boot-variable or storage change makes this run fail closed.
    let pre_write = read_boot_snapshot()?;
    if pre_write != initial {
        return Err("the active firmware entry changed while it was being verified".to_string());
    }
    let pre_write_bytes = read_variable(&variable_path)?;
    if pre_write_bytes != initial_bytes {
        return Err("the active firmware variable changed while it was being verified".to_string());
    }

    write_variable(&initial.current_id, &rewritten)?;

    let written_bytes = read_variable(&variable_path)?;
    if written_bytes != rewritten {
        return Err(
            "the firmware variable did not preserve the verified payload byte-for-byte".to_string(),
        );
    }
    let after = read_boot_snapshot()?;
    verify_only_label_changed(&initial, &after)?;
    write_complete_marker()?;
    Ok(Outcome::Migrated)
}

fn verify_product_identity() -> Result<()> {
    let release = fs::read_to_string("/usr/lib/os-release")
        .map_err(|error| format!("could not read /usr/lib/os-release: {error}"))?;
    if !release.lines().any(|line| line == "NAME=\"Goblins OS\"") {
        return Err("the running image does not have the Goblins OS product identity".to_string());
    }
    Ok(())
}

fn verify_runtime_tools() -> Result<()> {
    for path in [EFIBOOTMGR, EFIVAR, FINDMNT, LSBLK] {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("required tool {path} is unavailable: {error}"))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!(
                "required tool {path} is not an executable regular file"
            ));
        }
    }
    Ok(())
}

fn verify_bootupd_is_not_running() -> Result<()> {
    let processes = fs::read_dir("/proc").map_err(|error| {
        format!("could not inspect /proc for bootloader update activity: {error}")
    })?;
    for process in processes {
        let process =
            process.map_err(|error| format!("could not inspect a /proc entry: {error}"))?;
        let name = process.file_name();
        if !name
            .to_str()
            .is_some_and(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()))
        {
            continue;
        }
        let comm_path = process.path().join("comm");
        match fs::read_to_string(&comm_path) {
            Ok(comm)
                if matches!(
                    comm.trim(),
                    "bootupd" | "bootupctl" | "efibootmgr" | "efivar"
                ) =>
            {
                return Err(format!(
                    "another firmware updater ({}) is running; retrying without modifying firmware",
                    comm.trim()
                ));
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::NotFound | ErrorKind::PermissionDenied
                ) => {}
            Err(error) => {
                return Err(format!(
                    "could not inspect {}: {error}",
                    comm_path.display()
                ));
            }
        }
    }
    Ok(())
}

fn run_text(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not execute {program}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{program} exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    if output.stdout.len() > MAX_COMMAND_OUTPUT {
        return Err(format!(
            "{program} returned more than {MAX_COMMAND_OUTPUT} bytes"
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| format!("{program} returned output that was not valid UTF-8"))
}

fn read_boot_snapshot() -> Result<BootSnapshot> {
    let output = run_text(EFIBOOTMGR, &["--verbose"])?;
    parse_boot_snapshot(&output)
}

fn parse_boot_snapshot(output: &str) -> Result<BootSnapshot> {
    let current_id = unique_field(output, "BootCurrent: ")?
        .ok_or_else(|| "efibootmgr did not report BootCurrent".to_string())?;
    if current_id.len() != 4 || !current_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("efibootmgr reported an invalid BootCurrent identifier".to_string());
    }
    let current_id = current_id.to_ascii_uppercase();
    let prefix = format!("Boot{current_id}");
    let entries: Vec<&str> = output
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| {
            line.strip_prefix(&prefix)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|next| next == '*' || next == ' ')
        })
        .collect();
    if entries.len() != 1 {
        return Err(format!(
            "efibootmgr reported {} entries for BootCurrent {current_id}",
            entries.len()
        ));
    }
    let mut entry = entries[0]
        .strip_prefix(&prefix)
        .ok_or_else(|| "could not parse the BootCurrent entry".to_string())?;
    if let Some(rest) = entry.strip_prefix('*') {
        entry = rest;
    }
    entry = entry.trim_start();
    let (label, device_path) = entry.split_once('\t').ok_or_else(|| {
        "the BootCurrent entry did not separate its label and device path".to_string()
    })?;
    let label = label.trim_end();
    let device_path = device_path.trim();
    if label.is_empty() || device_path.is_empty() {
        return Err("the BootCurrent entry had an empty label or device path".to_string());
    }
    Ok(BootSnapshot {
        current_id,
        label: label.to_string(),
        device_path: device_path.to_string(),
        boot_order: unique_field(output, "BootOrder: ")?,
        boot_next: unique_field(output, "BootNext: ")?,
    })
}

fn unique_field(output: &str, prefix: &str) -> Result<Option<String>> {
    let values: Vec<String> = output
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter_map(|line| line.strip_prefix(prefix))
        .map(str::trim)
        .map(str::to_string)
        .collect();
    match values.as_slice() {
        [] => Ok(None),
        [value] if !value.is_empty() => Ok(Some(value.clone())),
        _ => Err(format!(
            "efibootmgr did not report exactly one {prefix} field"
        )),
    }
}

fn find_exact_loader() -> Result<PathBuf> {
    let candidates = [
        "/boot/efi/EFI/fedora/shimaa64.efi",
        "/efi/EFI/fedora/shimaa64.efi",
        "/boot/EFI/fedora/shimaa64.efi",
    ];
    let mut found = Vec::new();
    for candidate in candidates {
        match fs::symlink_metadata(candidate) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                found.push(PathBuf::from(candidate));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "could not inspect expected EFI loader {candidate}: {error}"
                ));
            }
        }
    }
    match found.as_slice() {
        [loader] => Ok(loader.clone()),
        _ => Err(format!(
            "expected exactly one mounted {EXPECTED_LOADER} loader, found {}",
            found.len()
        )),
    }
}

fn inspect_esp(loader: &Path) -> Result<EspIdentity> {
    let loader = loader
        .to_str()
        .ok_or_else(|| "the EFI loader path was not valid UTF-8".to_string())?;
    let mount = run_text(
        FINDMNT,
        &[
            "--noheadings",
            "--raw",
            "--output",
            "SOURCE,FSTYPE",
            "--target",
            loader,
        ],
    )?;
    let fields = single_nonempty_line(&mount, "findmnt ESP result")?;
    let fields: Vec<&str> = fields.split_whitespace().collect();
    if fields.len() != 2 || fields[1] != "vfat" {
        return Err("the exact EFI loader is not on one unambiguous vfat mount".to_string());
    }
    let source = canonical_device_source(fields[0])?;
    let partition = run_text(
        LSBLK,
        &[
            "--nodeps",
            "--noheadings",
            "--raw",
            "--output",
            "PARTUUID,PARTN",
            source
                .to_str()
                .ok_or_else(|| "the ESP source path was not valid UTF-8".to_string())?,
        ],
    )?;
    let partition = single_nonempty_line(&partition, "lsblk ESP identity")?;
    let fields: Vec<&str> = partition.split_whitespace().collect();
    if fields.len() != 2
        || fields[0].is_empty()
        || fields[1].is_empty()
        || !fields[1].bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("the ESP did not have one GPT partition UUID and partition number".to_string());
    }
    Ok(EspIdentity {
        physical_disk: physical_disk(&source)?,
        part_uuid: fields[0].to_ascii_lowercase(),
        part_number: fields[1].to_string(),
    })
}

fn inspect_root_disk() -> Result<PathBuf> {
    let output = run_text(
        FINDMNT,
        &[
            "--noheadings",
            "--raw",
            "--output",
            "SOURCE",
            "--target",
            "/sysroot",
        ],
    )?;
    let source = single_nonempty_line(&output, "findmnt /sysroot result")?;
    let source = canonical_device_source(source)?;
    physical_disk(&source)
}

fn canonical_device_source(source: &str) -> Result<PathBuf> {
    let source = strip_findmnt_subvolume(source);
    if !source.starts_with("/dev/") {
        return Err(format!(
            "block source {source} is not an absolute /dev path"
        ));
    }
    fs::canonicalize(source)
        .map_err(|error| format!("could not canonicalize block source {source}: {error}"))
}

fn strip_findmnt_subvolume(source: &str) -> &str {
    source
        .strip_suffix(']')
        .and_then(|source| source.rsplit_once('[').map(|(path, _)| path))
        .unwrap_or(source)
}

fn physical_disk(source: &Path) -> Result<PathBuf> {
    let source = source
        .to_str()
        .ok_or_else(|| "a block-device path was not valid UTF-8".to_string())?;
    let output = run_text(
        LSBLK,
        &[
            "--inverse",
            "--noheadings",
            "--raw",
            "--paths",
            "--output",
            "NAME,TYPE",
            source,
        ],
    )?;
    let disk = physical_disk_from_lsblk(&output)?;
    fs::canonicalize(&disk).map_err(|error| {
        format!(
            "could not canonicalize physical disk {}: {error}",
            disk.display()
        )
    })
}

fn physical_disk_from_lsblk(output: &str) -> Result<PathBuf> {
    let mut disks = BTreeSet::new();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 2 {
            return Err("lsblk returned an ambiguous block-device ancestry".to_string());
        }
        if fields[1] == "disk" {
            disks.insert(PathBuf::from(fields[0]));
        }
    }
    match disks.into_iter().collect::<Vec<_>>().as_slice() {
        [disk] => Ok(disk.clone()),
        disks => Err(format!(
            "block-device ancestry resolved to {} physical disks instead of exactly one",
            disks.len()
        )),
    }
}

fn single_nonempty_line<'a>(output: &'a str, description: &str) -> Result<&'a str> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    match lines.as_slice() {
        [line] => Ok(line),
        _ => Err(format!(
            "{description} contained {} nonempty lines",
            lines.len()
        )),
    }
}

fn validate_device_path(device_path: &str, part_uuid: &str, part_number: &str) -> Result<()> {
    let path = device_path.to_ascii_lowercase();
    if !path.starts_with("hd(") {
        return Err("the active firmware entry does not start with an HD device path".to_string());
    }
    let close = path
        .find(")/")
        .ok_or_else(|| "the active firmware entry has no complete HD device path".to_string())?;
    let fields: Vec<&str> = path[3..close].split(',').map(str::trim).collect();
    if fields.len() != 5
        || fields[0] != part_number
        || fields[1] != "gpt"
        || fields[2] != part_uuid.to_ascii_lowercase()
    {
        return Err(
            "the active firmware entry does not identify the mounted ESP partition".to_string(),
        );
    }
    let suffix = &path[close + 1..];
    let expected = EXPECTED_LOADER.to_ascii_lowercase();
    let direct = format!("/{expected}");
    let wrapped = format!("/file({expected})");
    if suffix != direct && suffix != wrapped {
        return Err(format!(
            "the active firmware entry does not point exactly to {EXPECTED_LOADER}"
        ));
    }
    Ok(())
}

fn variable_path(id: &str) -> PathBuf {
    Path::new(EFIVARS).join(format!("Boot{id}-{EFI_GLOBAL_GUID}"))
}

fn read_variable(path: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "could not read firmware variable {}: {error}",
            path.display()
        )
    })?;
    if bytes.len() > MAX_VARIABLE_SIZE {
        return Err(format!(
            "firmware variable {} exceeded {MAX_VARIABLE_SIZE} bytes",
            path.display()
        ));
    }
    Ok(bytes)
}

fn label_range(variable: &[u8]) -> Result<(std::ops::Range<usize>, String)> {
    // efivarfs prefixes the UEFI variable payload with a little-endian u32
    // variable-attribute field. EFI_LOAD_OPTION then contains u32 attributes,
    // u16 FilePathListLength, and a NUL-terminated UTF-16 description.
    if variable.len() < 12 {
        return Err(
            "the active firmware variable is too short to be an EFI load option".to_string(),
        );
    }
    let outer_attributes = u32::from_le_bytes(variable[0..4].try_into().expect("fixed slice"));
    if outer_attributes != 7 {
        return Err(format!(
            "the active firmware variable has unsupported attributes {outer_attributes:#x}"
        ));
    }
    let file_path_len =
        u16::from_le_bytes(variable[8..10].try_into().expect("fixed slice")) as usize;
    if file_path_len == 0 {
        return Err("the active firmware variable has an empty EFI device path".to_string());
    }
    let mut units = Vec::new();
    let mut cursor = 10;
    loop {
        if cursor + 2 > variable.len() {
            return Err(
                "the active firmware variable has an unterminated UTF-16 label".to_string(),
            );
        }
        let unit = u16::from_le_bytes([variable[cursor], variable[cursor + 1]]);
        cursor += 2;
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    if cursor + file_path_len > variable.len() {
        return Err(
            "the active firmware variable's device-path length exceeds its payload".to_string(),
        );
    }
    let label = String::from_utf16(&units)
        .map_err(|_| "the active firmware variable label is not valid UTF-16".to_string())?;
    Ok((10..cursor, label))
}

fn variable_label(variable: &[u8]) -> Result<String> {
    label_range(variable).map(|(_, label)| label)
}

fn rewrite_variable_label(variable: &[u8], expected: &str, replacement: &str) -> Result<Vec<u8>> {
    let (range, label) = label_range(variable)?;
    if label != expected {
        return Err(format!(
            "the active firmware variable label was {label:?}, not the exact legacy label {expected:?}"
        ));
    }
    let mut encoded = Vec::new();
    for unit in replacement.encode_utf16().chain(std::iter::once(0)) {
        encoded.extend_from_slice(&unit.to_le_bytes());
    }
    let mut rewritten = Vec::with_capacity(variable.len() - range.len() + encoded.len());
    rewritten.extend_from_slice(&variable[..range.start]);
    rewritten.extend_from_slice(&encoded);
    rewritten.extend_from_slice(&variable[range.end..]);
    Ok(rewritten)
}

fn write_variable(id: &str, rewritten: &[u8]) -> Result<()> {
    if rewritten.len() < 5 {
        return Err("refusing to write an incomplete firmware variable".to_string());
    }
    verify_state_dir()?;
    let attributes = u32::from_le_bytes(rewritten[0..4].try_into().expect("fixed slice"));
    let mut data_file = tempfile::NamedTempFile::new_in(STATE_DIR)
        .map_err(|error| format!("could not create the private firmware data file: {error}"))?;
    data_file
        .write_all(&rewritten[4..])
        .and_then(|()| data_file.as_file().sync_all())
        .map_err(|error| format!("could not persist the private firmware data file: {error}"))?;
    let name = format!("{EFI_GLOBAL_GUID}-Boot{id}");
    let attributes = attributes.to_string();
    let data_path = data_file
        .path()
        .to_str()
        .ok_or_else(|| "the private firmware data path was not valid UTF-8".to_string())?;
    let _ = run_text(
        EFIVAR,
        &[
            "--name",
            &name,
            "--attributes",
            &attributes,
            "--write",
            "--datafile",
            data_path,
        ],
    )?;
    Ok(())
}

fn verify_only_label_changed(before: &BootSnapshot, after: &BootSnapshot) -> Result<()> {
    if after.current_id != before.current_id
        || after.label != NEW_LABEL
        || after.device_path != before.device_path
        || after.boot_order != before.boot_order
        || after.boot_next != before.boot_next
    {
        return Err(
            "firmware verification found a change beyond the exact display label".to_string(),
        );
    }
    Ok(())
}

fn verify_state_dir() -> Result<()> {
    let metadata = fs::symlink_metadata(STATE_DIR)
        .map_err(|error| format!("could not inspect the systemd state directory: {error}"))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(
            "the firmware-label state directory failed its root-owned 0700 contract".to_string(),
        );
    }
    Ok(())
}

fn verify_complete_marker() -> Result<()> {
    verify_state_dir()?;
    let metadata = fs::symlink_metadata(COMPLETE_MARKER)
        .map_err(|error| format!("could not inspect the migration marker: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.mode() & 0o777 != 0o400
        || metadata.len() != b"firmware-label-v1\n".len() as u64
    {
        return Err(
            "the firmware-label migration marker failed its ownership contract".to_string(),
        );
    }
    let contents = fs::read(COMPLETE_MARKER)
        .map_err(|error| format!("could not read the migration marker: {error}"))?;
    if contents != b"firmware-label-v1\n" {
        return Err("the firmware-label migration marker had unexpected contents".to_string());
    }
    Ok(())
}

fn write_complete_marker() -> Result<()> {
    verify_state_dir()?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o400);
    match options.open(COMPLETE_MARKER) {
        Ok(mut marker) => {
            marker
                .write_all(b"firmware-label-v1\n")
                .and_then(|()| marker.sync_all())
                .map_err(|error| format!("could not persist the migration marker: {error}"))?;
            File::open(STATE_DIR)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| {
                    format!("could not persist the migration state directory: {error}")
                })?;
            verify_complete_marker()
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => verify_complete_marker(),
        Err(error) => Err(format!("could not create the migration marker: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "94ff4025-5276-4bec-adea-e98da271b64c";

    fn boot_output(label: &str, path: &str) -> String {
        format!(
            "BootCurrent: 0003\nBootNext: 0004\nBootOrder: 0003,0001,0004\nBoot0001* Other\tHD(1,GPT,11111111-1111-1111-1111-111111111111,0x800,0x1000)/File(\\\\EFI\\\\other.efi)\nBoot0003* {label}\t{path}\n"
        )
    }

    fn variable(label: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&7_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        for unit in label.encode_utf16().chain(std::iter::once(0)) {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        bytes
    }

    #[test]
    fn parses_the_exact_active_entry_and_boot_state() {
        let path = format!("HD(2,GPT,{UUID},0x1000,0x3f800)/File(\\EFI\\fedora\\shimaa64.efi)");
        let snapshot = parse_boot_snapshot(&boot_output(OLD_LABEL, &path)).expect("snapshot");
        assert_eq!(snapshot.current_id, "0003");
        assert_eq!(snapshot.label, OLD_LABEL);
        assert_eq!(snapshot.device_path, path);
        assert_eq!(snapshot.boot_order.as_deref(), Some("0003,0001,0004"));
        assert_eq!(snapshot.boot_next.as_deref(), Some("0004"));
    }

    #[test]
    fn validates_both_efibootmgr_loader_renderings() {
        let direct = format!("HD(2,GPT,{UUID},0x1000,0x3f800)/\\EFI\\fedora\\shimaa64.efi");
        let wrapped = format!("HD(2,GPT,{UUID},0x1000,0x3f800)/File(\\EFI\\fedora\\shimaa64.efi)");
        validate_device_path(&direct, UUID, "2").expect("direct loader");
        validate_device_path(&wrapped, UUID, "2").expect("wrapped loader");
    }

    #[test]
    fn rejects_another_partition_or_loader() {
        let wrong_partition =
            format!("HD(3,GPT,{UUID},0x1000,0x3f800)/\\EFI\\fedora\\shimaa64.efi");
        assert!(validate_device_path(&wrong_partition, UUID, "2").is_err());
        let wrong_loader = format!("HD(2,GPT,{UUID},0x1000,0x3f800)/\\EFI\\fedora\\grubaa64.efi");
        assert!(validate_device_path(&wrong_loader, UUID, "2").is_err());
    }

    #[test]
    fn rewrites_only_the_utf16_description() {
        let original = variable(OLD_LABEL);
        let rewritten = rewrite_variable_label(&original, OLD_LABEL, NEW_LABEL).expect("rewrite");
        assert_eq!(variable_label(&rewritten).as_deref(), Ok(NEW_LABEL));
        assert_eq!(&rewritten[..10], &original[..10]);
        let old_range = label_range(&original).expect("old label").0;
        let new_range = label_range(&rewritten).expect("new label").0;
        assert_eq!(&rewritten[new_range.end..], &original[old_range.end..]);
    }

    #[test]
    fn refuses_to_rewrite_a_nonlegacy_label() {
        assert!(rewrite_variable_label(&variable("Fedora Linux"), OLD_LABEL, NEW_LABEL).is_err());
    }

    #[test]
    fn rejects_invalid_variable_attributes_and_lengths() {
        let mut wrong_attributes = variable(OLD_LABEL);
        wrong_attributes[..4].copy_from_slice(&3_u32.to_le_bytes());
        assert!(variable_label(&wrong_attributes).is_err());
        let mut wrong_length = variable(OLD_LABEL);
        wrong_length[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(variable_label(&wrong_length).is_err());
    }

    #[test]
    fn requires_one_physical_parent() {
        let disk =
            physical_disk_from_lsblk("/dev/dm-0 crypt\n/dev/nvme0n1p3 part\n/dev/nvme0n1 disk\n")
                .expect("one disk");
        assert_eq!(disk, PathBuf::from("/dev/nvme0n1"));
        assert!(
            physical_disk_from_lsblk("/dev/md0 raid1\n/dev/nvme0n1 disk\n/dev/nvme1n1 disk\n")
                .is_err()
        );
    }

    #[test]
    fn post_write_contract_allows_only_the_label() {
        let path = format!("HD(2,GPT,{UUID},0x1000,0x3f800)/\\EFI\\fedora\\shimaa64.efi");
        let before = parse_boot_snapshot(&boot_output(OLD_LABEL, &path)).expect("before");
        let after = parse_boot_snapshot(&boot_output(NEW_LABEL, &path)).expect("after");
        verify_only_label_changed(&before, &after).expect("label only");
        let mut changed_order = after;
        changed_order.boot_order = Some("0001,0003,0004".to_string());
        assert!(verify_only_label_changed(&before, &changed_order).is_err());
    }

    #[test]
    fn strips_only_a_complete_findmnt_subvolume_suffix() {
        assert_eq!(
            strip_findmnt_subvolume("/dev/disk/by-id/example[/ostree/deploy]"),
            "/dev/disk/by-id/example"
        );
        assert_eq!(
            strip_findmnt_subvolume("/dev/disk/by-id/name[with-bracket"),
            "/dev/disk/by-id/name[with-bracket"
        );
    }
}
