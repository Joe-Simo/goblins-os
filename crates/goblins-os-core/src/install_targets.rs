use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::SystemTime,
};

const DEFAULT_SYS_BLOCK: &str = "/sys/block";
const DEFAULT_EFI_DIR: &str = "/sys/firmware/efi";
const SECURE_BOOT_VAR: &str =
    "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";
const DEFAULT_BOOTC_INSTALL_CONFIG: &str = "/usr/lib/bootc/install/00-goblins-os.toml";
const DEFAULT_FILESYSTEM: &str = "xfs";
const MIN_INSTALL_DISK_GB: u64 = 32;

/// Live state of a running `bootc install`. The phase is always the latest real
/// line bootc printed to its own stdout/stderr — never a fabricated percentage or
/// timer. A whole-disk install gives no honest progress fraction, so Goblins OS
/// reports the truth: which state it is in, and the most recent thing bootc said.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallProgressState {
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Serialize)]
pub struct InstallProgress {
    state: InstallProgressState,
    phase: String,
}

impl InstallProgress {
    const fn idle() -> Self {
        Self {
            state: InstallProgressState::Idle,
            phase: String::new(),
        }
    }
}

/// Process-global progress of the one install this machine can run at a time. The
/// const initializer keeps it lock-free at startup; the reader threads spawned by
/// `spawn_bootc_install` are the only writers.
static INSTALL_PROGRESS: Mutex<InstallProgress> = Mutex::new(InstallProgress::idle());

fn set_install_state(state: InstallProgressState) {
    if let Ok(mut progress) = INSTALL_PROGRESS.lock() {
        progress.state = state;
    }
}

/// Atomically claim the single install slot under one lock: flip Idle (or a prior
/// terminal state — a retry after a failure is allowed) to Running and return
/// true, or return false if an install is already Running so the caller refuses a
/// second concurrent execute. This is the one place that transitions into Running.
fn begin_install_if_idle() -> bool {
    let Ok(mut progress) = INSTALL_PROGRESS.lock() else {
        return false;
    };
    if progress.state == InstallProgressState::Running {
        return false;
    }
    progress.state = InstallProgressState::Running;
    progress.phase = "Preparing the disk…".to_string();
    true
}

fn set_install_phase(line: &str) {
    if let Ok(mut progress) = INSTALL_PROGRESS.lock() {
        progress.phase = line.to_string();
    }
}

/// Read-only view of the running install for the native installer's progress
/// screen. The installer polls this; it shows the dots breathing until a terminal
/// state arrives, and only ever prints the real phase text reported here.
pub async fn install_progress_status() -> Json<InstallProgress> {
    let progress = INSTALL_PROGRESS
        .lock()
        .map(|progress| progress.clone())
        .unwrap_or_else(|_| InstallProgress::idle());
    Json(progress)
}

#[derive(Serialize)]
pub struct InstallTargetStatus {
    generated_at: String,
    source: &'static str,
    environment: InstallEnvironment,
    boot_entries: BootEntryStatus,
    bootc: BootcInstallStatus,
    policy: InstallPolicy,
    targets: Vec<InstallTarget>,
}

#[derive(Serialize)]
pub struct InstallEnvironment {
    architecture: String,
    supported_architectures: Vec<&'static str>,
    native_supported: bool,
    boot_mode: &'static str,
    efi_available: bool,
    secure_boot: SecureBootStatus,
    architecture_guidance: String,
    boot_guidance: String,
}

#[derive(Serialize)]
pub struct SecureBootStatus {
    state: &'static str,
    detail: String,
}

#[derive(Serialize)]
pub struct BootEntryStatus {
    available: bool,
    entries: Vec<BootEntry>,
    detail: String,
    guidance: String,
}

#[derive(Serialize)]
pub struct BootEntry {
    id: String,
    label: String,
    target: String,
    active: bool,
}

#[derive(Serialize)]
pub struct BootcInstallStatus {
    available: bool,
    privileged: bool,
    image: String,
    install_config_path: String,
    default_filesystem: &'static str,
    command_model: &'static str,
}

#[derive(Serialize)]
pub struct InstallPolicy {
    destructive_acknowledgement: &'static str,
    execute_env_gate: &'static str,
    storage_layout: &'static str,
    simple_install_scope: &'static str,
    formatting_guidance: &'static str,
    bootloader: &'static str,
    bootloader_recovery: &'static str,
    advanced_storage_guidance: &'static str,
    install_path_options: Vec<InstallPathOption>,
    pre_install_safety: Vec<InstallPlanItem>,
    pre_write_install_plan: Vec<InstallPlanItem>,
    dual_boot_preflight: &'static str,
    dual_boot_guidance: &'static str,
    dual_boot_preservation: &'static str,
    dual_boot_handoff: &'static str,
    dual_boot_safe_route: DualBootSafeRoute,
    full_storage_installer: FullStorageInstallerHandoff,
    dual_boot_quick_start: Vec<InstallPlanItem>,
    dual_boot_readiness: Vec<DualBootReadinessItem>,
    dual_boot_choices: Vec<DualBootChoice>,
    dual_boot_guide: Vec<DualBootGuideStep>,
    dual_boot_decision_map: Vec<DualBootDecision>,
    storage_review_checklist: Vec<StorageReviewItem>,
    post_install_verification: Vec<InstallPlanItem>,
    local_model_weights: &'static str,
}

#[derive(Serialize)]
pub struct InstallPathOption {
    title: &'static str,
    summary: &'static str,
    action: &'static str,
    safety: &'static str,
}

#[derive(Serialize)]
pub struct InstallPlanItem {
    title: &'static str,
    detail: &'static str,
}

#[derive(Serialize)]
pub struct DualBootSafeRoute {
    title: &'static str,
    summary: &'static str,
    primary_action: &'static str,
    first_screen: &'static str,
    target_rule: &'static str,
    preserve_rule: &'static str,
    final_review: &'static str,
    after_install: &'static str,
}

#[derive(Serialize)]
pub struct FullStorageInstallerHandoff {
    title: &'static str,
    summary: &'static str,
    action_label: &'static str,
    command: &'static str,
    desktop_id: &'static str,
    storage_entry: &'static str,
    safest_for: &'static str,
    final_check: &'static str,
}

#[derive(Serialize)]
pub struct DualBootReadinessItem {
    title: &'static str,
    before_install: &'static str,
    installer_choice: &'static str,
    final_check: &'static str,
}

#[derive(Serialize)]
pub struct DualBootChoice {
    title: &'static str,
    preparation: &'static str,
    install_target: &'static str,
    preserve: &'static str,
    finish: &'static str,
}

#[derive(Serialize)]
pub struct DualBootGuideStep {
    title: &'static str,
    detail: &'static str,
}

#[derive(Serialize)]
pub struct DualBootDecision {
    title: &'static str,
    best_for: &'static str,
    prepare_space: &'static str,
    install_target: &'static str,
    preserve: &'static str,
    boot_picker: &'static str,
}

#[derive(Serialize)]
pub struct StorageReviewItem {
    title: &'static str,
    detail: &'static str,
}

#[derive(Serialize, Clone)]
pub struct ExistingSystem {
    kind: String,
    partition: String,
    detail: String,
    preservation: String,
}

#[derive(Serialize, Clone)]
pub struct InstallRecommendation {
    title: String,
    action: String,
    install_target: String,
    preserve: String,
    finish: String,
}

#[derive(Serialize, Clone)]
pub struct DualBootPlan {
    status: String,
    title: String,
    summary: String,
    primary_action: String,
    storage_target: String,
    preserve: String,
    bootloader: String,
    finish: String,
    steps: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct InstallTarget {
    id: String,
    path: String,
    model: String,
    size_gb: u64,
    removable: bool,
    rotational: bool,
    mounted: bool,
    partitions: Vec<String>,
    existing_systems: Vec<ExistingSystem>,
    recommendation: InstallRecommendation,
    dual_boot_plan: DualBootPlan,
    eligible: bool,
    reasons: Vec<String>,
}

#[derive(Deserialize)]
pub struct PrepareInstallRequest {
    target_path: String,
    filesystem: Option<String>,
    block_setup: Option<String>,
    wipe: Option<bool>,
    execute: Option<bool>,
    acknowledgement: Option<String>,
}

#[derive(Serialize)]
pub struct PrepareInstallResponse {
    state: InstallPrepareState,
    command: Vec<String>,
    target: Option<InstallTarget>,
    detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallPrepareState {
    Prepared,
    Started,
    Blocked,
}

pub async fn install_target_status() -> Json<InstallTargetStatus> {
    Json(build_install_target_status())
}

pub async fn prepare_install(
    Json(request): Json<PrepareInstallRequest>,
) -> (StatusCode, Json<PrepareInstallResponse>) {
    let status = build_install_target_status();
    let target = status
        .targets
        .iter()
        .find(|target| target.path == request.target_path)
        .cloned();

    let Some(target) = target else {
        return install_response(
            StatusCode::BAD_REQUEST,
            InstallPrepareState::Blocked,
            Vec::new(),
            None,
            "The selected install target is not visible in the current Linux block-device scan.",
        );
    };

    if !status.environment.native_supported {
        return install_response(
            StatusCode::CONFLICT,
            InstallPrepareState::Blocked,
            Vec::new(),
            Some(target),
            format!(
                "Goblins OS installs are native x86_64 or aarch64 only; this runtime is {}.",
                status.environment.architecture
            ),
        );
    }

    let filesystem = request.filesystem.as_deref().unwrap_or(DEFAULT_FILESYSTEM);
    if let Err(detail) = simple_install_filesystem(filesystem) {
        return install_response(
            StatusCode::BAD_REQUEST,
            InstallPrepareState::Blocked,
            Vec::new(),
            Some(target),
            detail,
        );
    }

    let block_setup = request.block_setup.as_deref().unwrap_or("direct");
    if let Err(detail) = simple_install_block_setup(block_setup) {
        return install_response(
            StatusCode::BAD_REQUEST,
            InstallPrepareState::Blocked,
            Vec::new(),
            Some(target),
            detail,
        );
    }

    if let Err(detail) =
        simple_install_wipe_for_execute(request.execute.unwrap_or(false), request.wipe)
    {
        return install_response(
            StatusCode::BAD_REQUEST,
            InstallPrepareState::Blocked,
            Vec::new(),
            Some(target),
            detail,
        );
    }

    let mut command = vec![
        "bootc".to_string(),
        "install".to_string(),
        "to-disk".to_string(),
        "--filesystem".to_string(),
        filesystem.to_string(),
        "--wipe".to_string(),
    ];
    command.push(target.path.clone());

    if !target.eligible {
        let detail = ineligible_install_detail(&target);
        return install_response(
            StatusCode::CONFLICT,
            InstallPrepareState::Blocked,
            command,
            Some(target),
            detail,
        );
    }

    if !request.execute.unwrap_or(false) {
        return install_response(
            StatusCode::OK,
            InstallPrepareState::Prepared,
            command,
            Some(target),
            "Install plan prepared. No disk has been changed; execution stays blocked until the destructive install gate is explicitly enabled.",
        );
    }

    // The acknowledgement must match exactly; trimming only forgives stray
    // surrounding whitespace (e.g. a pasted phrase with a trailing newline), never
    // the device path or words inside the phrase.
    let expected_ack = destructive_acknowledgement(&target.path);
    let provided_ack = request
        .acknowledgement
        .as_deref()
        .unwrap_or_default()
        .trim();
    if provided_ack != expected_ack {
        return install_response(
            StatusCode::FORBIDDEN,
            InstallPrepareState::Blocked,
            command,
            Some(target),
            "Exact destructive acknowledgement is required before Goblins OS can wipe a disk.",
        );
    }

    if env::var("GOBLINS_OS_ENABLE_DESTRUCTIVE_INSTALL").as_deref() != Ok("1") {
        return install_response(
            StatusCode::FORBIDDEN,
            InstallPrepareState::Blocked,
            command,
            Some(target),
            "Destructive disk writes are disabled for this session. Run from the approved installer environment before executing.",
        );
    }

    // Claim the single install slot atomically. A whole-disk install must never run
    // twice in parallel, so a second execute that races in (a double-click, a retry,
    // or a stray API client) while one is already running is refused rather than
    // spawning a second bootc install or stomping the live run's progress.
    if !begin_install_if_idle() {
        return install_response(
            StatusCode::CONFLICT,
            InstallPrepareState::Blocked,
            command,
            Some(target),
            "An install is already running on this computer.",
        );
    }

    if spawn_bootc_install(&command).is_err() {
        // The slot was claimed but nothing started; release it so a retry is clean.
        set_install_state(InstallProgressState::Idle);
        return install_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            InstallPrepareState::Blocked,
            command,
            Some(target),
            "Goblins OS could not start the disk installer. No disk was changed.",
        );
    }

    install_response(
        StatusCode::ACCEPTED,
        InstallPrepareState::Started,
        command,
        Some(target),
        "Disk install started. Keep this device connected to power until the installer finishes.",
    )
}

pub(crate) fn build_install_target_status() -> InstallTargetStatus {
    let bootc = BootcInstallStatus {
        available: executable_exists("bootc"),
        privileged: running_as_root(),
        image: env::var("GOBLINS_OS_BOOTC_IMAGE").unwrap_or_else(|_| "unconfigured".to_string()),
        install_config_path: env::var("GOBLINS_OS_BOOTC_INSTALL_CONFIG")
            .unwrap_or_else(|_| DEFAULT_BOOTC_INSTALL_CONFIG.to_string()),
        default_filesystem: DEFAULT_FILESYSTEM,
        command_model: "Goblins OS disk install --filesystem xfs --wipe <device>",
    };
    let targets = scan_install_targets(&bootc);

    InstallTargetStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        environment: build_install_environment(),
        boot_entries: scan_boot_entries(),
        bootc,
        policy: InstallPolicy {
            destructive_acknowledgement: "WIPE <device> AND INSTALL GOBLINS OS",
            execute_env_gate: "GOBLINS_OS_ENABLE_DESTRUCTIVE_INSTALL=1",
            storage_layout:
                "The Goblins OS disk installer creates a GPT layout with platform boot partitions, an EFI System Partition on UEFI systems, and an xfs root filesystem by default.",
            simple_install_scope:
                "Simple install is for one blank internal disk only after the installer can read the partition scan and reports no existing partitions. It erases the selected disk, creates the boot and root layout, formats the new Goblins OS root filesystem, and installs the bootloader for that disk.",
            formatting_guidance:
                "Simple install formats a scan-verified blank disk with a fresh GPT layout and xfs root. If the scan cannot read partition data, or if you need ext4, btrfs, separate /home, resized free space, encryption, LUKS/LVM, or any custom partitioning, use advanced storage.",
            bootloader:
                "Bootloader and EFI setup are owned by the installer during install. The simple path is not a preservation proof; verify the final storage summary before writing changes.",
            bootloader_recovery:
                "After install, pick Goblins OS or the existing OS from firmware boot options. If an entry is missing, adjust boot order or the selected EFI entry; do not format preserved partitions as a repair step.",
            advanced_storage_guidance:
                "Advanced storage shows Installation Destination with Custom/manual storage or Reclaim Space, formatting, mount points, bootloader/EFI target, TPM2 LUKS or LUKS/LVM choices, unreadable/unknown partition cases, and every partition that will be preserved or changed before writing.",
            install_path_options: vec![
                InstallPathOption {
                    title: "Keep my current OS",
                    summary: "Dual boot with Windows, macOS, Linux, or another OS.",
                    action: "Open advanced storage, choose Installation Destination, then Custom/manual storage or Reclaim Space. Choose unallocated free space or a separate disk for Goblins OS.",
                    safety: "Back up first. Leave Windows, macOS/APFS, Linux, other OS, recovery, EFI, and data partitions unformatted.",
                },
                InstallPathOption {
                    title: "Replace one blank disk",
                    summary: "Use the native simple flow only when the disk has no partitions you want to keep.",
                    action: "Select only a blank internal disk whose readable scan reports no partitions, review the erase scope, then type the device-specific wipe phrase.",
                    safety: "This erases the selected disk, creates the Goblins OS boot/root layout, and is not the dual-boot, custom-storage, or scan-unknown path.",
                },
                InstallPathOption {
                    title: "Advanced storage",
                    summary: "Use this for encryption, ext4, btrfs, separate /home, resized partitions, or mixed disks.",
                    action: "Use advanced storage and verify formatting, mount points, bootloader, and EFI target in the final summary.",
                    safety: "Continue only when every preserve/format row matches what you intend to keep or replace.",
                },
            ],
            pre_install_safety: vec![
                InstallPlanItem {
                    title: "Back up first",
                    detail: "Make a current backup of every Windows, macOS, Linux, other OS, recovery, EFI, and data partition you want to keep before resizing or installing.",
                },
                InstallPlanItem {
                    title: "Save recovery keys",
                    detail: "Record BitLocker, FileVault, LUKS, account, and firmware recovery information before changing storage or boot order.",
                },
                InstallPlanItem {
                    title: "Make space from the OS you keep",
                    detail: "Shrink Windows from Disk Management, create macOS/APFS free space from Disk Utility, and resize Linux/LUKS/LVM with the distro or trusted live media that understands it.",
                },
                InstallPlanItem {
                    title: "Use the matching native ISO",
                    detail: "Use goblins-os-x86_64.iso on x86_64 computers and goblins-os-aarch64.iso on aarch64 computers. Do not assume one installer image covers both.",
                },
                InstallPlanItem {
                    title: "Keep power connected",
                    detail: "Keep the computer plugged in and do not disconnect target disks while storage changes or the bootloader install are running.",
                },
                InstallPlanItem {
                    title: "Review before writing",
                    detail: "Continue only when the final summary shows the Goblins OS target, bootloader/EFI target, formatted filesystems, and every preserved partition exactly as intended.",
                },
            ],
            pre_write_install_plan: vec![
                InstallPlanItem {
                    title: "Disk choice",
                    detail: "Simple install continues only after one eligible blank internal disk with a readable partition scan is selected. Any disk with existing Windows, macOS/APFS, Linux, other OS, recovery, EFI, data partitions, or an unreadable scan is routed to manual storage.",
                },
                InstallPlanItem {
                    title: "Partition table",
                    detail: "The selected blank disk is replaced with a fresh GPT layout owned by the Goblins OS install.",
                },
                InstallPlanItem {
                    title: "Bootloader and EFI",
                    detail: "The installer creates or uses the installer-approved boot partitions and EFI System Partition for the selected disk; the final summary must name the bootloader/EFI target before writing.",
                },
                InstallPlanItem {
                    title: "Root filesystem",
                    detail: "The simple path formats a new xfs root filesystem for the immutable system image.",
                },
                InstallPlanItem {
                    title: "Custom formatting",
                    detail: "ext4, btrfs, separate /home, resized free space, TPM2 LUKS, LUKS/LVM, scan-unknown disks, and mixed-disk layouts stay in advanced storage so every format and preserve row is visible.",
                },
                InstallPlanItem {
                    title: "Startup choice",
                    detail: "After install, choose Goblins OS or preserved systems from the firmware boot picker or boot order; never format preserved partitions as a recovery step.",
                },
            ],
            dual_boot_preflight:
                "Before dual boot, make free space from the OS you are keeping when possible: shrink Windows from Windows Disk Management, manage macOS/APFS space from Disk Utility, and resize Linux/LUKS volumes with your existing distro tools. Back up first.",
            dual_boot_guidance:
                "To keep Windows, macOS, Linux, or another OS, do not use this whole-disk erase flow. Open advanced storage, choose Installation Destination with manual storage, choose free space or a dedicated disk, and preserve existing system, recovery, and EFI partitions.",
            dual_boot_preservation:
                "Dual boot path: choose Installation Destination, then Custom/manual storage or Reclaim Space. Install Goblins OS into unallocated free space or a dedicated disk; leave Windows, macOS/APFS, Linux, other OS, recovery, and EFI partitions untouched unless you intentionally mean to replace that OS.",
            dual_boot_handoff:
                "Keep your current OS: open advanced storage, choose Installation Destination with Custom/manual storage or Reclaim Space, select only unallocated free space or a separate disk for Goblins OS, and confirm the final summary shows existing Windows, macOS/APFS, Linux, other OS, recovery, EFI, and data partitions preserved.",
            dual_boot_safe_route: DualBootSafeRoute {
                title: "Install beside an existing OS",
                summary: "The safest dual-boot path starts with the OS or data you are keeping, not with a disk erase. Use it for Windows, macOS, Linux, another OS, shared data, recovery, vendor, or EFI partitions.",
                primary_action: "Open advanced storage",
                first_screen: "Choose Keep my current OS or the desktop entry named Install Goblins OS Beside Another OS.",
                target_rule: "Choose only unallocated free space or a separate dedicated Goblins OS disk. If you need to resize, make space from Windows Disk Management, macOS Disk Utility, the Linux distro you are keeping, or trusted live media first.",
                preserve_rule: "Leave Windows, macOS/APFS, Linux, other OS, recovery, vendor, EFI, and shared data partitions unformatted unless you intentionally mean to replace that system.",
                final_review: "Before writing, the final storage summary must name the Goblins OS target, every filesystem that will be formatted, every preserved partition, and the bootloader/EFI target.",
                after_install: "After install, use the firmware startup menu or boot picker and confirm Goblins OS plus every preserved system starts before changing default boot order.",
            },
            full_storage_installer: FullStorageInstallerHandoff {
                title: "Advanced storage",
                summary: "Use this path to keep Windows, macOS, Linux, another OS, or shared data while adding Goblins OS.",
                action_label: "Open advanced storage",
                command: "/usr/libexec/goblins-os/goblins-os-full-installer",
                desktop_id: "org.goblins.OS.FullInstaller.desktop",
                storage_entry: "Installation Destination with Custom/manual storage or Reclaim Space",
                safest_for: "Dual boot, resized free space, a dedicated non-blank disk, encryption, separate /home, LUKS/LVM, ext4, btrfs, or any layout where another OS or data must be preserved.",
                final_check: "Before writing, the final storage summary must show the Goblins OS target, bootloader/EFI target, formatted filesystems, and every preserved Windows, macOS/APFS, Linux, other OS, recovery, EFI, vendor, and data partition.",
            },
            dual_boot_quick_start: vec![
                InstallPlanItem {
                    title: "1. Back up and unlock recovery",
                    detail: "Back up the systems and data you are keeping, then save BitLocker, FileVault, LUKS, account, and firmware recovery keys before changing partitions.",
                },
                InstallPlanItem {
                    title: "2. Make room from the OS you keep",
                    detail: "Shrink Windows with Disk Management, create macOS/APFS free space with Disk Utility, resize Linux/LUKS/LVM with the distro or trusted live media, or choose a separate dedicated disk.",
                },
                InstallPlanItem {
                    title: "3. Install beside another OS",
                    detail: "Open advanced storage, choose Installation Destination with Custom/manual storage or Reclaim Space, then assign Goblins OS only to unallocated free space or a dedicated disk.",
                },
                InstallPlanItem {
                    title: "4. Confirm preserve, format, and bootloader",
                    detail: "Before writing, the final summary must list the Goblins OS target, bootloader/EFI target, every filesystem that will be formatted, and every Windows, macOS/APFS, Linux, other OS, recovery, EFI, vendor, and data partition that will be preserved.",
                },
                InstallPlanItem {
                    title: "5. Test every boot path",
                    detail: "After install, use the firmware startup menu or boot picker to start Goblins OS and every preserved system; change default boot order only after those checks pass.",
                },
            ],
            dual_boot_readiness: vec![
                DualBootReadinessItem {
                    title: "Windows readiness",
                    before_install: "Back up, suspend BitLocker if enabled, shrink Windows with Disk Management, and leave Microsoft Reserved, recovery, EFI, and data partitions intact.",
                    installer_choice: "Choose Keep my current OS, then Installation Destination with Custom/manual storage or Reclaim Space, and select only the new unallocated space or a dedicated disk for Goblins OS.",
                    final_check: "Reboot with the firmware boot picker and confirm both Goblins OS and Windows start before changing boot order.",
                },
                DualBootReadinessItem {
                    title: "macOS readiness",
                    before_install: "Back up, confirm the Mac can boot the native Goblins OS architecture, and create free space with Disk Utility or use a separate disk.",
                    installer_choice: "Choose Keep my current OS, then install only into free space or the dedicated Goblins OS disk; keep APFS containers, macOS volumes, recovery, EFI, and data partitions unformatted.",
                    final_check: "Use the startup boot picker and confirm both Goblins OS and macOS start before changing startup disk behavior.",
                },
                DualBootReadinessItem {
                    title: "Linux readiness",
                    before_install: "Back up and resize ext4, xfs, btrfs, LUKS, LVM, swap, or /home layouts from the distro being kept or trusted live media.",
                    installer_choice: "Choose Keep my current OS, then Custom/manual storage or Reclaim Space, and select only confirmed free space or a separate disk for Goblins OS.",
                    final_check: "Use the firmware boot picker or boot order and confirm both Goblins OS and the existing Linux install start.",
                },
                DualBootReadinessItem {
                    title: "Other OS or data readiness",
                    before_install: "Back up and identify every partition you care about; unknown, vendor, recovery, EFI, and shared data partitions default to preserve.",
                    installer_choice: "Choose Keep my current OS or Advanced storage, then install only into confirmed free space or a dedicated Goblins OS disk.",
                    final_check: "After install, confirm the existing OS or data disk is still available before making any further storage changes.",
                },
                DualBootReadinessItem {
                    title: "Dedicated disk readiness",
                    before_install: "Prefer a separate internal disk for Goblins OS when available, and verify which disks contain systems or data you are keeping.",
                    installer_choice: "Use the simple Goblins OS flow only when that dedicated disk is blank; otherwise use Custom/manual storage so every preserved or formatted row is visible.",
                    final_check: "Use the firmware boot picker or boot order to choose between Goblins OS and preserved systems.",
                },
            ],
            dual_boot_choices: vec![
                DualBootChoice {
                    title: "Keep Windows",
                    preparation: "Back up first, suspend BitLocker if it is enabled, then shrink Windows from Windows Disk Management so the free space is created by Windows.",
                    install_target: "Open advanced storage, choose Installation Destination with Custom/manual storage or Reclaim Space, then choose only the unallocated free space or a separate disk for Goblins OS.",
                    preserve: "Leave Windows, Microsoft Reserved, recovery, EFI, and data partitions unformatted.",
                    finish: "After install, use the firmware boot picker to choose Goblins OS or Windows; confirm Windows still boots before changing boot order.",
                },
                DualBootChoice {
                    title: "Keep macOS",
                    preparation: "Back up first, use Disk Utility to create free space or choose a separate disk, and confirm this Mac can boot the Goblins OS ISO for its native architecture.",
                    install_target: "Open advanced storage, choose Installation Destination with Custom/manual storage or Reclaim Space, then choose only the free space or dedicated disk for Goblins OS.",
                    preserve: "Leave APFS containers, macOS volumes, recovery, EFI, and data partitions unformatted.",
                    finish: "After install, use the startup boot picker to choose Goblins OS or macOS; confirm macOS still boots before changing boot order.",
                },
                DualBootChoice {
                    title: "Keep Linux",
                    preparation: "Back up first, then use the Linux distribution you are keeping or trusted live media to resize ext4, xfs, btrfs, LUKS, LVM, or swap layouts where supported.",
                    install_target: "Open advanced storage, choose Installation Destination with Custom/manual storage or Reclaim Space, then choose only confirmed free space or a separate disk for Goblins OS.",
                    preserve: "Leave existing Linux root, /home, LUKS, LVM, swap, recovery, EFI, and data partitions unformatted unless replacing that install.",
                    finish: "After install, use the firmware boot picker or boot order to choose Goblins OS or the existing Linux install; confirm both boot.",
                },
                DualBootChoice {
                    title: "Keep another OS or data",
                    preparation: "Back up first and identify every partition you care about. If you do not recognize a partition, treat it as data to preserve.",
                    install_target: "Open advanced storage, choose Installation Destination with Custom/manual storage or Reclaim Space, then install Goblins OS only into confirmed free space or a separate disk.",
                    preserve: "Leave unknown OS, recovery, EFI, vendor, and shared data partitions unformatted.",
                    finish: "After install, use the firmware boot picker and confirm the existing OS or data disk is still available before changing storage again.",
                },
                DualBootChoice {
                    title: "Use a dedicated disk",
                    preparation: "Choose a separate internal disk for Goblins OS when possible. Disconnect or leave untouched disks that contain operating systems you are keeping.",
                    install_target: "Use the simple Goblins OS flow only if the dedicated disk is blank; otherwise use Custom/manual storage so the final summary shows exactly what changes.",
                    preserve: "Leave all Windows, macOS/APFS, Linux, other OS, recovery, EFI, and data partitions on other disks untouched.",
                    finish: "After install, use the firmware boot picker or boot order to choose between Goblins OS and the existing systems.",
                },
            ],
            dual_boot_guide: vec![
                DualBootGuideStep {
                    title: "Windows",
                    detail: "From Windows, back up first, suspend BitLocker if it is enabled, shrink with Disk Management, then install Goblins OS only into the new unallocated space or a separate disk. Leave Windows, Microsoft Reserved, recovery, and EFI partitions untouched.",
                },
                DualBootGuideStep {
                    title: "macOS/APFS",
                    detail: "From macOS, back up first and use Disk Utility to create free space or choose a separate disk. Install Goblins OS only into that space or disk; leave APFS containers, recovery, and EFI partitions untouched when keeping macOS.",
                },
                DualBootGuideStep {
                    title: "Linux",
                    detail: "Use the distro you are keeping, or trusted live media, to create unallocated free space where resizing is supported. Preserve existing ext4, xfs, btrfs, LUKS, LVM, swap, /home, recovery, and EFI partitions unless you chose to replace them.",
                },
                DualBootGuideStep {
                    title: "Other OS or data",
                    detail: "Treat unknown or data partitions as keepers. Choose Custom/manual storage or Reclaim Space, install only into confirmed free space or a dedicated disk, and never format a partition you do not recognize.",
                },
                DualBootGuideStep {
                    title: "Bootloader and EFI",
                    detail: "Review advanced storage final storage summary before writing changes. Use the selected EFI System Partition only as shown there, and do not format it unless replacing every OS on that disk.",
                },
                DualBootGuideStep {
                    title: "Startup menu",
                    detail: "After install, choose Goblins OS or the existing OS from the firmware startup menu or boot picker. If an OS is not listed, adjust firmware boot order or the selected EFI entry; do not format preserved partitions as a repair step.",
                },
                DualBootGuideStep {
                    title: "Final storage review",
                    detail: "The safe dual-boot finish is a summary that shows Goblins OS using free space or a dedicated disk, existing OS partitions preserved, and the bootloader/EFI target clearly listed before changes are written.",
                },
            ],
            dual_boot_decision_map: vec![
                DualBootDecision {
                    title: "Windows beside Goblins OS",
                    best_for: "You want to keep Windows and add Goblins OS on the same computer.",
                    prepare_space: "Back up, suspend BitLocker if enabled, then shrink Windows from Windows Disk Management so Windows creates the free space.",
                    install_target: "Choose Keep my current OS, open advanced storage, then use Installation Destination with Custom/manual storage or Reclaim Space and select only the new unallocated space or a separate disk.",
                    preserve: "Do not format Windows, Microsoft Reserved, recovery, EFI, vendor, or data partitions.",
                    boot_picker: "After install, use the firmware boot picker to start both Goblins OS and Windows before changing boot order.",
                },
                DualBootDecision {
                    title: "macOS beside Goblins OS",
                    best_for: "You want to keep macOS and add Goblins OS on compatible Apple hardware.",
                    prepare_space: "Back up, confirm the hardware can boot the native Goblins OS architecture, then create free space with Disk Utility or use a separate disk.",
                    install_target: "Choose Keep my current OS, open advanced storage, then use Installation Destination with Custom/manual storage or Reclaim Space and select only free space or a dedicated Goblins OS disk.",
                    preserve: "Do not format APFS containers, macOS volumes, recovery, EFI, vendor, or data partitions.",
                    boot_picker: "After install, use the startup boot picker to start both Goblins OS and macOS before changing startup disk behavior.",
                },
                DualBootDecision {
                    title: "Linux beside Goblins OS",
                    best_for: "You want to keep an existing Linux install and add Goblins OS.",
                    prepare_space: "Back up and resize ext4, xfs, btrfs, LUKS, LVM, swap, or /home from the distro being kept or trusted live media.",
                    install_target: "Choose Keep my current OS, open advanced storage, then use Installation Destination with Custom/manual storage or Reclaim Space and select only confirmed free space or a separate disk.",
                    preserve: "Do not format existing Linux root, /home, boot, LUKS, LVM, swap, recovery, EFI, or data partitions unless replacing that install.",
                    boot_picker: "After install, use the firmware boot picker or boot order to start both Goblins OS and the existing Linux install.",
                },
                DualBootDecision {
                    title: "Another OS or shared data",
                    best_for: "You want to keep an OS, vendor partition, or shared data disk Goblins OS cannot identify with certainty.",
                    prepare_space: "Back up and identify every partition you care about; unknown partitions are treated as preserved by default.",
                    install_target: "Choose Keep my current OS or Advanced storage, then install only into confirmed free space or a dedicated Goblins OS disk.",
                    preserve: "Do not format unknown OS, recovery, EFI, vendor, shared data, or archive partitions.",
                    boot_picker: "After install, confirm the existing OS or data disk is still available before changing storage again.",
                },
                DualBootDecision {
                    title: "Separate disk",
                    best_for: "You have a dedicated internal disk for Goblins OS and want the cleanest multi-boot setup.",
                    prepare_space: "Back up and verify which disk contains each operating system or data set before selecting a target.",
                    install_target: "Use the simple Goblins OS flow only when the dedicated disk is blank; otherwise use Custom/manual storage so every preserved or formatted row is visible.",
                    preserve: "Leave all Windows, macOS/APFS, Linux, other OS, recovery, EFI, and data partitions on other disks untouched.",
                    boot_picker: "After install, use the firmware boot picker or boot order to choose between Goblins OS and preserved systems.",
                },
            ],
            storage_review_checklist: vec![
                StorageReviewItem {
                    title: "Install target",
                    detail: "Blank-disk path selects one whole internal disk. Dual boot selects only unallocated free space or a dedicated disk in advanced storage.",
                },
                StorageReviewItem {
                    title: "Formatting",
                    detail: "Simple install creates a fresh GPT layout and xfs root. Manual storage is required for ext4, btrfs, separate /home, resized partitions, LUKS/LVM, or TPM2 LUKS choices.",
                },
                StorageReviewItem {
                    title: "Bootloader and EFI",
                    detail: "The final summary must name the bootloader target and EFI System Partition. Keep existing EFI partitions unformatted when keeping another OS.",
                },
                StorageReviewItem {
                    title: "Preserved systems",
                    detail: "Windows, macOS/APFS, Linux, other OS, recovery, EFI, and data partitions must be listed as preserved unless the user is replacing that OS.",
                },
                StorageReviewItem {
                    title: "Before writing",
                    detail: "The user gets one last storage summary before disk writes. If anything does not match the intended install, go back instead of continuing.",
                },
                StorageReviewItem {
                    title: "Required final summary",
                    detail: "Do not proceed unless the final summary names the target disk or free-space assignment, every filesystem that will be formatted, every Windows, macOS/APFS, Linux, other OS, recovery, EFI, vendor, and data partition that will be preserved, and the bootloader/EFI target.",
                },
                StorageReviewItem {
                    title: "After reboot",
                    detail: "Use the firmware startup menu or boot picker to choose Goblins OS or the existing OS, then adjust boot order only after preserved systems boot successfully.",
                },
            ],
            post_install_verification: vec![
                InstallPlanItem {
                    title: "Remove installer media",
                    detail: "Remove the USB drive or mounted installer image before the first reboot so the computer starts from internal storage.",
                },
                InstallPlanItem {
                    title: "Start Goblins OS",
                    detail: "Use the firmware startup menu or boot picker to choose Goblins OS and complete first-boot setup.",
                },
                InstallPlanItem {
                    title: "Check preserved systems",
                    detail: "If you kept Windows, macOS, Linux, another OS, or a data disk, restart again and confirm each preserved system or disk is still available before changing boot order.",
                },
                InstallPlanItem {
                    title: "Set boot order last",
                    detail: "Change default boot order only after Goblins OS and every preserved system boot successfully.",
                },
                InstallPlanItem {
                    title: "Recover missing entries",
                    detail: "If an entry is missing, use firmware boot options or the selected EFI entry. Do not format preserved partitions as a repair step.",
                },
            ],
            local_model_weights:
                "OpenAI open-weight model files remain outside the immutable OS image and are downloaded after consent.",
        },
        targets,
    }
}

fn build_install_environment() -> InstallEnvironment {
    let architecture = current_install_architecture();
    let native_supported = native_supported_architecture(&architecture);
    let efi_dir = env::var("GOBLINS_OS_EFI_DIR").unwrap_or_else(|_| DEFAULT_EFI_DIR.to_string());
    let efi_available = Path::new(&efi_dir).is_dir();
    let boot_mode = if efi_available {
        "uefi"
    } else {
        "legacy-or-unknown"
    };
    let secure_boot = secure_boot_status(efi_available);
    let architecture_guidance = if native_supported {
        format!(
            "This installer is running as native {architecture}. Use goblins-os-x86_64.iso on x86_64 computers and goblins-os-aarch64.iso on aarch64 computers; do not treat one ISO as universal."
        )
    } else {
        format!(
            "This installer is running on unsupported architecture {architecture}. Goblins OS release installs are native x86_64 or aarch64 only."
        )
    };
    let boot_guidance = if efi_available {
        "UEFI firmware is visible. Review the final installer summary for the EFI System Partition and bootloader target before writing changes."
            .to_string()
    } else {
        "UEFI firmware is not visible from this runtime. Do not proceed until the installer boot mode and bootloader target are understood for this computer."
            .to_string()
    };

    InstallEnvironment {
        architecture,
        supported_architectures: vec!["x86_64", "aarch64"],
        native_supported,
        boot_mode,
        efi_available,
        secure_boot,
        architecture_guidance,
        boot_guidance,
    }
}

fn current_install_architecture() -> String {
    env::var("GOBLINS_OS_INSTALL_ARCH").unwrap_or_else(|_| env::consts::ARCH.to_string())
}

fn native_supported_architecture(architecture: &str) -> bool {
    matches!(architecture, "x86_64" | "aarch64")
}

fn secure_boot_status(efi_available: bool) -> SecureBootStatus {
    if !efi_available {
        return SecureBootStatus {
            state: "not-uefi",
            detail: "Secure Boot status is not available because UEFI firmware is not visible."
                .to_string(),
        };
    }

    if let Ok(state) = env::var("GOBLINS_OS_SECURE_BOOT_STATE") {
        return match state.as_str() {
            "enabled" => SecureBootStatus {
                state: "enabled",
                detail: "Secure Boot is reported enabled by the installer environment.".to_string(),
            },
            "disabled" => SecureBootStatus {
                state: "disabled",
                detail: "Secure Boot is reported disabled by the installer environment."
                    .to_string(),
            },
            _ => SecureBootStatus {
                state: "unknown",
                detail: format!("Secure Boot state override is not recognized: {state}."),
            },
        };
    }

    let secure_boot_path =
        env::var("GOBLINS_OS_SECURE_BOOT_VAR").unwrap_or_else(|_| SECURE_BOOT_VAR.to_string());
    match fs::read(&secure_boot_path) {
        Ok(bytes) if bytes.get(4).copied() == Some(1) => SecureBootStatus {
            state: "enabled",
            detail:
                "Secure Boot is enabled. Use the signed Fedora/Goblins OS boot path provided by the installer image."
                    .to_string(),
        },
        Ok(bytes) if bytes.get(4).copied() == Some(0) => SecureBootStatus {
            state: "disabled",
            detail:
                "Secure Boot is disabled. The installer can still use the firmware boot picker and EFI entry flow."
                    .to_string(),
        },
        Ok(_) => SecureBootStatus {
            state: "unknown",
            detail:
                "Secure Boot variable was present but did not contain the expected firmware value."
                    .to_string(),
        },
        Err(_) => SecureBootStatus {
            state: "unknown",
            detail:
                "Secure Boot status could not be read from firmware; review firmware settings if boot fails."
                    .to_string(),
        },
    }
}

fn scan_boot_entries() -> BootEntryStatus {
    if let Ok(text) = env::var("GOBLINS_OS_EFIBOOTMGR_OUTPUT") {
        let entries = parse_boot_entries(&text);
        return boot_entry_status(entries, "firmware entries reported by test override");
    }

    let output = Command::new("efibootmgr").output();
    let Ok(output) = output else {
        return BootEntryStatus {
            available: false,
            entries: Vec::new(),
            detail: "Firmware boot entries are not visible because efibootmgr is not available in this runtime.".to_string(),
            guidance: "Use the firmware startup menu or boot picker after install. If a preserved OS is missing, check firmware boot order or the selected EFI entry before changing partitions.".to_string(),
        };
    };

    if !output.status.success() {
        return BootEntryStatus {
            available: false,
            entries: Vec::new(),
            detail: "Firmware boot entries could not be read from this runtime.".to_string(),
            guidance: "Use the firmware startup menu or boot picker after install. If a preserved OS is missing, check firmware boot order or the selected EFI entry before changing partitions.".to_string(),
        };
    }

    let text = String::from_utf8_lossy(&output.stdout);
    boot_entry_status(
        parse_boot_entries(&text),
        "firmware entries reported by efibootmgr",
    )
}

fn boot_entry_status(entries: Vec<BootEntry>, source: &str) -> BootEntryStatus {
    if entries.is_empty() {
        return BootEntryStatus {
            available: true,
            entries,
            detail: format!("{source}; no Boot#### entries were found."),
            guidance: "Use the firmware startup menu or boot picker after install and verify Goblins OS plus every preserved system appears.".to_string(),
        };
    }

    let labels = entries
        .iter()
        .map(|entry| {
            if entry.target.is_empty() {
                entry.label.clone()
            } else {
                format!("{} -> {}", entry.label, entry.target)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    BootEntryStatus {
        available: true,
        entries,
        detail: format!("{source}; visible entries: {labels}."),
        guidance: "After install, choose Goblins OS or preserved systems from the firmware startup menu or boot picker. Change boot order only after each preserved entry still starts.".to_string(),
    }
}

fn parse_boot_entries(text: &str) -> Vec<BootEntry> {
    text.lines()
        .filter_map(parse_boot_entry_line)
        .take(12)
        .collect()
}

fn parse_boot_entry_line(line: &str) -> Option<BootEntry> {
    let line = line.trim();
    let rest = line.strip_prefix("Boot")?;
    let id = rest.get(0..4)?;
    if !id.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let active = rest.get(4..5) == Some("*");
    let body = rest.get(if active { 5.. } else { 4.. })?.trim();
    let mut parts = body.splitn(2, '\t');
    let label = parts.next().unwrap_or_default().trim().to_string();
    let target = parts.next().unwrap_or_default().trim().to_string();
    if label.is_empty() {
        return None;
    }

    Some(BootEntry {
        id: id.to_string(),
        label,
        target,
        active,
    })
}

fn scan_install_targets(bootc: &BootcInstallStatus) -> Vec<InstallTarget> {
    let sys_block =
        env::var("GOBLINS_OS_SYS_BLOCK_DIR").unwrap_or_else(|_| DEFAULT_SYS_BLOCK.into());
    scan_install_targets_in(Path::new(&sys_block), bootc)
}

fn scan_install_targets_in(sys_block: &Path, bootc: &BootcInstallStatus) -> Vec<InstallTarget> {
    let Ok(entries) = fs::read_dir(sys_block) else {
        return Vec::new();
    };

    let mut targets: Vec<InstallTarget> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| build_install_target(&entry.path(), bootc))
        .collect();
    targets.sort_by(|a, b| a.path.cmp(&b.path));
    targets
}

fn build_install_target(sys_path: &Path, bootc: &BootcInstallStatus) -> Option<InstallTarget> {
    let name = sys_path.file_name()?.to_string_lossy().to_string();
    if skip_block_device(&name) {
        return None;
    }

    let size_gb = sectors_to_gib(read_u64(sys_path.join("size")).unwrap_or(0));
    let removable = read_u64(sys_path.join("removable")).unwrap_or(0) == 1;
    let rotational = read_u64(sys_path.join("queue/rotational")).unwrap_or(0) == 1;
    let model = read_trimmed(sys_path.join("device/model"))
        .or_else(|| read_trimmed(sys_path.join("device/name")))
        .unwrap_or_else(|| "Unknown block device".to_string());
    let path = format!("/dev/{name}");
    let (partitions, partition_scan_error) = match block_partitions(sys_path, &name) {
        Ok(partitions) => (partitions, None),
        Err(detail) => (Vec::new(), Some(detail)),
    };
    let existing_systems = if partition_scan_error.is_some() {
        Vec::new()
    } else {
        existing_systems_for(sys_path, &partitions)
    };
    let mounted = target_is_mounted(&path, &partitions);
    let architecture = current_install_architecture();
    let mut reasons = Vec::new();

    if !native_supported_architecture(&architecture) {
        reasons.push(format!(
            "Goblins OS installs are native x86_64 or aarch64 only; this runtime is {architecture}"
        ));
    }
    if !bootc.available {
        reasons.push(
            "The Goblins OS disk installer is not available in this live environment".to_string(),
        );
    }
    if !bootc.privileged {
        reasons.push("Administrator privileges are required to install".to_string());
    }
    if let Some(detail) = partition_scan_error {
        reasons.push(detail);
    }
    if size_gb < MIN_INSTALL_DISK_GB {
        reasons.push(format!(
            "Minimum install size is {MIN_INSTALL_DISK_GB} GB; this disk is {size_gb} GB"
        ));
    }
    if mounted {
        reasons.push("This disk or its partitions are currently mounted".to_string());
    }
    if removable {
        reasons.push("Default installs do not target removable media.".to_string());
    }
    if !existing_systems.is_empty() {
        reasons.push(format!(
            "Existing {} detected. To keep Windows, macOS, Linux, or another OS, use Installation Destination with Custom/manual storage or Reclaim Space; the simple flow only installs to a blank disk.",
            existing_systems_summary(&existing_systems)
        ));
    }
    if reasons.is_empty() {
        reasons.push("Ready for guarded disk install preparation.".to_string());
    }

    let eligible = reasons.len() == 1 && reasons[0].starts_with("Ready");
    let recommendation = install_recommendation(&path, &partitions, &existing_systems, eligible);
    let dual_boot_plan = dual_boot_plan(&path, &partitions, &existing_systems, eligible);

    Some(InstallTarget {
        id: name,
        path,
        model,
        size_gb,
        removable,
        rotational,
        mounted,
        partitions,
        existing_systems,
        recommendation,
        dual_boot_plan,
        eligible,
        reasons,
    })
}

fn skip_block_device(name: &str) -> bool {
    name.starts_with("loop")
        || name.starts_with("ram")
        || name.starts_with("zram")
        || name.starts_with("dm-")
        || name.starts_with("md")
}

fn block_partitions(sys_path: &Path, disk_name: &str) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(sys_path).map_err(|error| {
        partition_scan_blocked_detail(disk_name, format!("could not open device entries: {error}"))
    })?;

    let mut partitions = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            partition_scan_blocked_detail(
                disk_name,
                format!("could not read a device entry: {error}"),
            )
        })?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(disk_name) && entry.path().join("partition").is_file() {
            partitions.push(format!("/dev/{name}"));
        }
    }
    partitions.sort();
    Ok(partitions)
}

fn partition_scan_blocked_detail(disk_name: &str, reason: String) -> String {
    format!(
        "Partition scan for /dev/{disk_name} was not readable ({reason}); simple install is disabled. Open advanced storage with Custom/manual storage or Reclaim Space, then verify preserve, format, and bootloader/EFI rows before writing."
    )
}

fn existing_systems_for(sys_path: &Path, partitions: &[String]) -> Vec<ExistingSystem> {
    partitions
        .iter()
        .map(|partition| {
            let partition_name = partition.trim_start_matches("/dev/");
            let sys_partition_path = sys_path.join(partition_name);
            let metadata = partition_metadata(&sys_partition_path, partition);
            classify_existing_system(partition, &metadata)
        })
        .collect()
}

fn partition_metadata(sys_partition_path: &Path, partition: &str) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::from([("DEVICE".to_string(), partition.to_string())]);

    if let Ok(text) = fs::read_to_string(sys_partition_path.join("uevent")) {
        metadata.extend(parse_export_lines(&text));
    }
    if let Some(blkid) = blkid_export(partition) {
        metadata.extend(blkid);
    }

    metadata
}

fn blkid_export(partition: &str) -> Option<BTreeMap<String, String>> {
    let output = Command::new("blkid")
        .arg("-o")
        .arg("export")
        .arg(partition)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(parse_export_lines(&text))
}

fn parse_export_lines(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn classify_existing_system(
    partition: &str,
    metadata: &BTreeMap<String, String>,
) -> ExistingSystem {
    let haystack = metadata
        .values()
        .map(|value| value.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let fs_type = metadata
        .get("TYPE")
        .or_else(|| metadata.get("ID_FS_TYPE"))
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let part_type = metadata
        .get("PART_ENTRY_TYPE")
        .or_else(|| metadata.get("PARTTYPE"))
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();

    let (kind, preservation) = if part_type.contains("c12a7328-f81f-11d2-ba4b-00a0c93ec93b")
        || haystack.contains("efi system partition")
        || (fs_type == "vfat" && (haystack.contains("efi") || haystack.contains("system")))
    {
        (
            "EFI boot",
            "Leave the EFI System Partition intact unless you are intentionally replacing every OS on this disk.",
        )
    } else if part_type.contains("de94bba4-06d1-4d40-a16a-bfd50179d6ac")
        || haystack.contains("windows recovery")
        || haystack.contains("recovery")
    {
        (
            "Recovery",
            "Leave recovery partitions intact when keeping the existing operating system.",
        )
    } else if fs_type == "ntfs"
        || fs_type == "exfat"
        || fs_type == "bitlocker"
        || part_type.contains("ebd0a0a2-b9e5-4433-87c0-68b6b72699c7")
        || part_type.contains("e3c9e316-0b5c-4db8-817d-f92df00215ae")
        || haystack.contains("bitlocker")
        || haystack.contains("microsoft")
        || haystack.contains("windows")
    {
        (
            "Windows",
            "Use Windows Disk Management to make free space first, then install Goblins OS into that free space or another disk.",
        )
    } else if fs_type == "apfs"
        || fs_type == "hfs"
        || fs_type == "hfsplus"
        || part_type.contains("7c3457ef-0000-11aa-aa11-00306543ecac")
        || part_type.contains("48465300-0000-11aa-aa11-00306543ecac")
        || haystack.contains("apple")
        || haystack.contains("apfs")
        || haystack.contains("macos")
    {
        (
            "macOS/APFS",
            "Use Disk Utility to make free space first, then leave APFS and recovery partitions untouched.",
        )
    } else if matches!(
        fs_type.as_str(),
        "ext2"
            | "ext3"
            | "ext4"
            | "xfs"
            | "btrfs"
            | "f2fs"
            | "bcachefs"
            | "swap"
            | "crypto_luks"
            | "lvm2_member"
    ) || part_type.contains("0fc63daf-8483-4772-8e79-3d69d8477de4")
        || part_type.contains("e6d6d379-f507-44c2-a23c-238f2a3df928")
        || part_type.contains("a19d880f-05fc-4d3b-a006-743f0f84911e")
        || haystack.contains("linux")
    {
        (
            "Linux",
            "Resize Linux, LUKS, LVM, or btrfs volumes with the tools from the distribution you are keeping.",
        )
    } else {
        (
            "Other OS/data",
            "Treat this as data to preserve until you intentionally choose otherwise in manual storage.",
        )
    };

    ExistingSystem {
        kind: kind.to_string(),
        partition: partition.to_string(),
        detail: existing_system_detail(partition, kind, metadata),
        preservation: preservation.to_string(),
    }
}

fn existing_system_detail(
    partition: &str,
    kind: &str,
    metadata: &BTreeMap<String, String>,
) -> String {
    let mut evidence = Vec::new();
    for key in ["TYPE", "PART_ENTRY_TYPE", "PARTTYPE", "PARTLABEL", "LABEL"] {
        if let Some(value) = metadata.get(key).filter(|value| !value.is_empty()) {
            evidence.push(format!("{key}={value}"));
        }
    }
    if evidence.is_empty() {
        format!("{kind} signal on {partition}")
    } else {
        format!("{kind} signal on {partition} ({})", evidence.join(", "))
    }
}

fn existing_systems_summary(systems: &[ExistingSystem]) -> String {
    let mut kinds = Vec::new();
    for system in systems {
        if !kinds.contains(&system.kind.as_str()) {
            kinds.push(system.kind.as_str());
        }
    }
    match kinds.as_slice() {
        [] => "no existing systems".to_string(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn install_recommendation(
    path: &str,
    partitions: &[String],
    existing_systems: &[ExistingSystem],
    eligible: bool,
) -> InstallRecommendation {
    if !existing_systems.is_empty() {
        let systems = existing_systems_summary(existing_systems);
        let partition_list = partitions.join(", ");
        return InstallRecommendation {
            title: format!("Keep {systems}"),
            action: format!(
                "Open advanced storage and use Installation Destination with Custom/manual storage or Reclaim Space; do not use the whole-disk erase path for {path}."
            ),
            install_target:
                "Select only unallocated free space or a separate dedicated disk for Goblins OS."
                    .to_string(),
            preserve: format!(
                "Leave {partition_list} unformatted unless you intentionally mean to replace those systems or data."
            ),
            finish:
                "After install, use the firmware startup menu or boot picker and confirm Goblins OS plus every preserved system starts before changing boot order."
                    .to_string(),
        };
    }

    if eligible {
        return InstallRecommendation {
            title: "Replace this blank disk".to_string(),
            action: format!(
                "Continue here only if {path} is the whole disk you want Goblins OS to own."
            ),
            install_target:
                "Goblins OS will write a fresh GPT layout, boot/EFI setup, and xfs root for the immutable system image."
                    .to_string(),
            preserve:
                "The readable installer scan reported no existing OS, recovery, EFI, or data partitions on this disk."
                    .to_string(),
            finish:
                "After install, remove the installer media and boot Goblins OS; use firmware boot options if you keep other systems on other disks."
                    .to_string(),
        };
    }

    InstallRecommendation {
        title: "Resolve install blockers".to_string(),
        action: format!(
            "Fix the listed blocker before using the whole-disk path for {path}, or open advanced storage for any custom layout."
        ),
        install_target:
            "Do not write to this disk until the installer shows the target, formatting, bootloader/EFI, and preserved partitions clearly."
                .to_string(),
        preserve:
            "Treat any partition, recovery volume, EFI entry, unknown filesystem, or shared data as preserved unless you intentionally replace it."
                .to_string(),
        finish:
            "After install, use the firmware startup menu or boot picker to confirm every OS you kept still starts."
                .to_string(),
    }
}

fn dual_boot_plan(
    path: &str,
    partitions: &[String],
    existing_systems: &[ExistingSystem],
    eligible: bool,
) -> DualBootPlan {
    if !existing_systems.is_empty() {
        let systems = existing_systems_summary(existing_systems);
        let partition_list = partitions.join(", ");
        return DualBootPlan {
            status: "manual-preserve-required".to_string(),
            title: format!("Keep {systems} and add Goblins OS"),
            summary: format!(
                "This disk contains {systems}. The safe path is a guided dual-boot install through advanced storage, not the whole-disk erase flow."
            ),
            primary_action:
                "Choose Keep my current OS, open advanced storage, then use Installation Destination with Custom/manual storage or Reclaim Space."
                    .to_string(),
            storage_target:
                "Select only confirmed unallocated free space, or choose a separate dedicated Goblins OS disk."
                    .to_string(),
            preserve: format!(
                "Leave {partition_list} unformatted unless you explicitly intend to replace the OS or data on those partitions."
            ),
            bootloader:
                "Before writing, the installer summary must name the bootloader target and EFI System Partition while listing existing OS, recovery, EFI, and data partitions as preserved."
                    .to_string(),
            finish:
                "After install, use the firmware startup menu or boot picker and verify Goblins OS plus every preserved system starts before changing boot order."
                    .to_string(),
            steps: vec![
                "Back up every system and data partition you are keeping.".to_string(),
                format!(
                    "Create free space from the OS you are keeping, or use a separate disk for Goblins OS; do not resize {path} from a tool that does not understand the existing filesystem."
                ),
                "Open advanced storage, then choose Installation Destination with Custom/manual storage or Reclaim Space.".to_string(),
                "Assign Goblins OS only to unallocated free space or the dedicated disk, then confirm all kept partitions are marked preserve.".to_string(),
                "Install, reboot with the firmware startup menu, and test Goblins OS plus every preserved OS before changing the default boot entry.".to_string(),
            ],
        };
    }

    if eligible {
        return DualBootPlan {
            status: "blank-dedicated-disk-ready".to_string(),
            title: "Use this blank disk for Goblins OS".to_string(),
            summary: format!(
                "{path} has a readable partition scan with no reported partitions and can be used as a dedicated Goblins OS disk while other disks remain untouched."
            ),
            primary_action:
                "Select this disk only if it is the disk you want Goblins OS to own completely."
                    .to_string(),
            storage_target:
                "The whole disk becomes the Goblins OS target with a fresh GPT layout, boot/EFI setup, and xfs root filesystem."
                    .to_string(),
            preserve:
                "Other disks are not selected by this flow; still review firmware boot entries and any external disks before writing."
                    .to_string(),
            bootloader:
                "The installer will create or use the installer-approved boot and EFI layout for this selected disk."
                    .to_string(),
            finish:
                "After install, remove installer media and use the firmware boot picker if this computer also has other operating systems on other disks."
                    .to_string(),
            steps: vec![
                "Verify this is the intended dedicated Goblins OS disk.".to_string(),
                "Review the erase scope and typed confirmation phrase.".to_string(),
                "Install Goblins OS to the selected disk.".to_string(),
                "Reboot and pick Goblins OS or preserved systems from firmware boot options.".to_string(),
            ],
        };
    }

    DualBootPlan {
        status: "blocked-until-reviewed".to_string(),
        title: "Resolve blockers before choosing a boot path".to_string(),
        summary: format!(
            "{path} is not ready for the guarded installer. Do not write storage changes until blockers are resolved and the final summary is clear."
        ),
        primary_action:
            "Review the listed blockers, then choose manual storage for any mixed, mounted, removable, or custom layout."
                .to_string(),
        storage_target:
            "Only unallocated free space, a dedicated disk, or an explicitly reviewed custom layout should become the Goblins OS target."
                .to_string(),
        preserve:
            "Treat unknown partitions, recovery volumes, EFI entries, vendor partitions, and shared data as preserved unless replacing them is intentional."
                .to_string(),
        bootloader:
            "Do not continue unless the installer summary names the bootloader/EFI target and the partitions that will be formatted."
                .to_string(),
        finish:
            "After install, confirm Goblins OS and every kept system or data disk still starts or appears before changing boot order."
                .to_string(),
        steps: vec![
            "Fix the listed blockers or switch to Installation Destination with Custom/manual storage.".to_string(),
            "Confirm the Goblins OS target is free space or a dedicated disk.".to_string(),
            "Confirm every partition you are keeping is marked preserve before writing.".to_string(),
        ],
    }
}

fn target_is_mounted(device_path: &str, partitions: &[String]) -> bool {
    let Ok(mountinfo) = fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };

    mountinfo.lines().any(|line| {
        line.contains(device_path) || partitions.iter().any(|partition| line.contains(partition))
    })
}

fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn sectors_to_gib(sectors: u64) -> u64 {
    let bytes = sectors.saturating_mul(512);
    bytes.div_ceil(1024 * 1024 * 1024)
}

fn simple_install_filesystem(filesystem: &str) -> Result<(), &'static str> {
    match filesystem {
        DEFAULT_FILESYSTEM => Ok(()),
        "ext4" | "btrfs" => Err(
            "The simple Goblins OS flow only writes an xfs root on one scan-verified blank disk. Use advanced storage for ext4, btrfs, separate filesystems, or any custom formatting so the final summary shows every format and preserve row.",
        ),
        _ => Err(
            "The requested root filesystem belongs in advanced storage, where the final summary must show every format row, preserve row, and bootloader/EFI target before writing.",
        ),
    }
}

fn simple_install_block_setup(block_setup: &str) -> Result<(), &'static str> {
    match block_setup {
        "direct" => Ok(()),
        "tpm2-luks" => Err(
            "TPM2 LUKS belongs in advanced storage, where encryption, recovery, formatting, bootloader/EFI target, and preserved partitions are all visible in the final summary before writing.",
        ),
        _ => Err(
            "The requested block setup belongs in advanced storage, where encryption, recovery, formatting, bootloader/EFI target, and preserved partitions are all visible in the final summary before writing.",
        ),
    }
}

fn simple_install_wipe_for_execute(execute: bool, wipe: Option<bool>) -> Result<(), &'static str> {
    simple_install_wipe(wipe.unwrap_or(true))?;
    if execute && wipe != Some(true) {
        return Err(
            "Starting the destructive install requires an explicit wipe=true request in addition to the device-specific confirmation phrase and environment gate.",
        );
    }
    Ok(())
}

fn simple_install_wipe(wipe: bool) -> Result<(), &'static str> {
    if wipe {
        Ok(())
    } else {
        Err("The simple Goblins OS flow always uses the wipe guard for one selected blank disk. Use advanced storage for any custom preserve, reclaim, or non-wipe layout so the final summary shows every format and preserve row.")
    }
}

fn destructive_acknowledgement(path: &str) -> String {
    format!("WIPE {path} AND INSTALL GOBLINS OS")
}

fn ineligible_install_detail(target: &InstallTarget) -> String {
    if let Some(reason) = target
        .reasons
        .iter()
        .find(|reason| reason.contains("Partition scan"))
    {
        return format!(
            "{reason} Do not use the simple erase flow for {path} until storage is verified.",
            path = target.path
        );
    }

    if !target.existing_systems.is_empty() {
        let systems = existing_systems_summary(&target.existing_systems);
        return format!(
            "The simple erase flow will not install to {path} because it contains {systems}. To keep Windows, macOS, Linux, another OS, or data, open advanced storage, choose Installation Destination with Custom/manual storage or Reclaim Space, and select only unallocated free space or a separate disk for Goblins OS.",
            path = target.path
        );
    }

    format!(
        "The simple erase flow will not install to {path}. Resolve the listed blockers, or open advanced storage for dual boot, encryption, resized partitions, custom filesystems, or a dedicated non-blank disk.",
        path = target.path
    )
}

fn spawn_bootc_install(command: &[String]) -> std::io::Result<()> {
    let Some((program, args)) = command.split_first() else {
        return Err(std::io::Error::other("empty disk install command"));
    };

    // Capture bootc's own output so the installer can report honest phase text.
    // bootc logs its install phases to stderr and progress to stdout; we drain
    // both (so the pipes never fill and stall the install) and surface the most
    // recent real line.
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // The Running state is claimed atomically by `begin_install_if_idle` before we
    // get here, so the 202 is never ahead of the progress view and two installs
    // can’t race; this function only spawns the readers and the waiter.
    //
    // Use `thread::Builder::spawn` (which returns a Result) rather than
    // `thread::spawn` (which PANICS if the OS can’t create a thread): the pipe
    // readers are best-effort — a failure just means no live phase text from that
    // stream — but the waiter is required to report completion, so its spawn
    // failure propagates and the caller releases the install slot.
    if let Some(stdout) = child.stdout.take() {
        let _ = thread::Builder::new()
            .name("goblins-bootc-stdout".to_string())
            .spawn(move || pump_install_output(stdout));
    }
    if let Some(stderr) = child.stderr.take() {
        let _ = thread::Builder::new()
            .name("goblins-bootc-stderr".to_string())
            .spawn(move || pump_install_output(stderr));
    }

    thread::Builder::new()
        .name("goblins-bootc-wait".to_string())
        .spawn(move || {
            let succeeded = child.wait().map(|status| status.success()).unwrap_or(false);
            set_install_state(if succeeded {
                InstallProgressState::Succeeded
            } else {
                InstallProgressState::Failed
            });
        })?;

    Ok(())
}

/// Forward a bootc pipe into the live progress phase, one real line at a time.
/// Empty lines are skipped so the phase text never blanks out mid-install.
fn pump_install_output<R: Read>(reader: R) {
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        let line = line.trim();
        if !line.is_empty() {
            set_install_phase(line);
        }
    }
}

fn install_response(
    status: StatusCode,
    state: InstallPrepareState,
    command: Vec<String>,
    target: Option<InstallTarget>,
    detail: impl Into<String>,
) -> (StatusCode, Json<PrepareInstallResponse>) {
    (
        status,
        Json(PrepareInstallResponse {
            state,
            command,
            target,
            detail: detail.into(),
        }),
    )
}

fn executable_exists(binary: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| {
        fs::metadata(path.join(binary))
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    })
}

fn running_as_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
}

#[cfg(test)]
mod tests {
    use super::{
        block_partitions, build_install_target, build_install_target_status,
        destructive_acknowledgement, ineligible_install_detail, native_supported_architecture,
        parse_boot_entries, scan_install_targets_in, sectors_to_gib, secure_boot_status,
        simple_install_block_setup, simple_install_filesystem, simple_install_wipe,
        simple_install_wipe_for_execute, BootcInstallStatus,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn install_status_uses_bootc_to_disk_policy() {
        let status = build_install_target_status();

        assert_eq!(status.source, "goblins-os-core");
        assert!(!status.boot_entries.detail.is_empty());
        assert!(status
            .boot_entries
            .guidance
            .contains("firmware startup menu"));
        assert!(status
            .environment
            .supported_architectures
            .contains(&"x86_64"));
        assert!(status
            .environment
            .supported_architectures
            .contains(&"aarch64"));
        assert!(
            status
                .environment
                .architecture_guidance
                .contains("goblins-os-x86_64.iso")
                || status
                    .environment
                    .architecture_guidance
                    .contains("native x86_64 or aarch64 only")
        );
        assert!(status
            .environment
            .boot_guidance
            .contains("bootloader target"));
        assert!(!status.environment.secure_boot.detail.is_empty());
        assert_eq!(status.bootc.default_filesystem, "xfs");
        assert!(status
            .bootc
            .command_model
            .contains("Goblins OS disk install"));
        assert!(!status.bootc.command_model.contains("bootc install"));
        assert!(status.policy.destructive_acknowledgement.contains("WIPE"));
        assert!(status
            .policy
            .storage_layout
            .contains("EFI System Partition"));
        assert!(status
            .policy
            .storage_layout
            .contains("Goblins OS disk installer"));
        assert!(status
            .policy
            .simple_install_scope
            .contains("blank internal disk"));
        assert!(status
            .policy
            .simple_install_scope
            .contains("read the partition scan"));
        assert!(status
            .policy
            .simple_install_scope
            .contains("formats the new Goblins OS root filesystem"));
        assert!(status
            .policy
            .formatting_guidance
            .contains("scan cannot read partition data"));
        assert!(status.policy.formatting_guidance.contains("ext4"));
        assert!(status.policy.formatting_guidance.contains("btrfs"));
        assert!(status
            .policy
            .advanced_storage_guidance
            .contains("TPM2 LUKS"));
        assert_eq!(status.policy.install_path_options.len(), 3);
        assert!(status.policy.install_path_options.iter().any(|option| {
            option.title == "Keep my current OS"
                && option
                    .action
                    .contains("Custom/manual storage or Reclaim Space")
                && option.safety.contains("unformatted")
        }));
        assert!(status.policy.install_path_options.iter().any(|option| {
            option.title == "Replace one blank disk" && option.safety.contains("scan-unknown path")
        }));
        assert!(status.policy.install_path_options.iter().any(|option| {
            option.title == "Advanced storage" && option.action.contains("bootloader")
        }));
        assert_eq!(status.policy.pre_install_safety.len(), 6);
        assert!(status.policy.pre_install_safety.iter().any(|item| {
            item.title == "Back up first" && item.detail.contains("data partition")
        }));
        assert!(status.policy.pre_install_safety.iter().any(|item| {
            item.title == "Save recovery keys"
                && item.detail.contains("BitLocker")
                && item.detail.contains("FileVault")
                && item.detail.contains("LUKS")
        }));
        assert!(status.policy.pre_install_safety.iter().any(|item| {
            item.title == "Use the matching native ISO"
                && item.detail.contains("goblins-os-x86_64.iso")
                && item.detail.contains("goblins-os-aarch64.iso")
        }));
        assert!(status.policy.pre_install_safety.iter().any(|item| {
            item.title == "Review before writing" && item.detail.contains("bootloader/EFI target")
        }));
        assert_eq!(status.policy.pre_write_install_plan.len(), 6);
        assert!(status.policy.pre_write_install_plan.iter().any(|item| {
            item.title == "Disk choice"
                && item.detail.contains("readable partition scan")
                && item.detail.contains("routed to manual storage")
        }));
        assert!(status.policy.pre_write_install_plan.iter().any(|item| {
            item.title == "Partition table" && item.detail.contains("fresh GPT layout")
        }));
        assert!(status.policy.pre_write_install_plan.iter().any(|item| {
            item.title == "Bootloader and EFI" && item.detail.contains("bootloader/EFI target")
        }));
        assert!(status
            .policy
            .pre_write_install_plan
            .iter()
            .any(|item| { item.title == "Root filesystem" && item.detail.contains("xfs root") }));
        assert!(status.policy.pre_write_install_plan.iter().any(|item| {
            item.title == "Custom formatting" && item.detail.contains("TPM2 LUKS")
        }));
        assert!(status.policy.bootloader.contains("Bootloader"));
        assert!(status
            .policy
            .bootloader_recovery
            .contains("firmware boot options"));
        assert!(status.policy.dual_boot_preflight.contains("Back up first"));
        assert!(status.policy.dual_boot_guidance.contains("Windows"));
        assert!(status.policy.dual_boot_guidance.contains("another OS"));
        assert!(status.policy.dual_boot_guidance.contains("manual storage"));
        assert!(status
            .policy
            .dual_boot_preservation
            .contains("Reclaim Space"));
        assert!(status
            .policy
            .dual_boot_preservation
            .contains("unallocated free space"));
        assert!(status.policy.dual_boot_preservation.contains("macOS/APFS"));
        assert!(status
            .policy
            .dual_boot_handoff
            .contains("Keep your current OS"));
        assert!(status
            .policy
            .dual_boot_handoff
            .contains("select only unallocated free space"));
        assert!(status.policy.dual_boot_handoff.contains("data partitions"));
        assert_eq!(
            status.policy.dual_boot_safe_route.title,
            "Install beside an existing OS"
        );
        assert!(status
            .policy
            .dual_boot_safe_route
            .summary
            .contains("Windows, macOS, Linux, another OS"));
        assert!(status
            .policy
            .dual_boot_safe_route
            .first_screen
            .contains("Install Goblins OS Beside Another OS"));
        assert!(status
            .policy
            .dual_boot_safe_route
            .target_rule
            .contains("unallocated free space"));
        assert!(status
            .policy
            .dual_boot_safe_route
            .preserve_rule
            .contains("EFI"));
        assert!(status
            .policy
            .dual_boot_safe_route
            .final_review
            .contains("bootloader/EFI target"));
        assert!(status
            .policy
            .dual_boot_safe_route
            .after_install
            .contains("firmware startup menu"));
        assert_eq!(
            status.policy.full_storage_installer.command,
            "/usr/libexec/goblins-os/goblins-os-full-installer"
        );
        assert_eq!(
            status.policy.full_storage_installer.desktop_id,
            "org.goblins.OS.FullInstaller.desktop"
        );
        assert!(status
            .policy
            .full_storage_installer
            .summary
            .contains("Windows, macOS, Linux, another OS"));
        assert!(status
            .policy
            .full_storage_installer
            .storage_entry
            .contains("Custom/manual storage or Reclaim Space"));
        assert!(status
            .policy
            .full_storage_installer
            .final_check
            .contains("bootloader/EFI target"));
        assert_eq!(status.policy.dual_boot_quick_start.len(), 5);
        assert!(status.policy.dual_boot_quick_start.iter().any(|item| {
            item.title == "1. Back up and unlock recovery"
                && item.detail.contains("BitLocker")
                && item.detail.contains("FileVault")
                && item.detail.contains("LUKS")
        }));
        assert!(status.policy.dual_boot_quick_start.iter().any(|item| {
            item.title == "3. Install beside another OS"
                && item
                    .detail
                    .contains("Custom/manual storage or Reclaim Space")
                && item.detail.contains("unallocated free space")
        }));
        assert!(status.policy.dual_boot_quick_start.iter().any(|item| {
            item.title == "4. Confirm preserve, format, and bootloader"
                && item.detail.contains("bootloader/EFI target")
                && item
                    .detail
                    .contains("every filesystem that will be formatted")
                && item.detail.contains("will be preserved")
        }));
        assert!(status.policy.dual_boot_quick_start.iter().any(|item| {
            item.title == "5. Test every boot path"
                && item.detail.contains("firmware startup menu or boot picker")
        }));
        assert_eq!(status.policy.dual_boot_readiness.len(), 5);
        assert!(status.policy.dual_boot_readiness.iter().any(|item| {
            item.title == "Windows readiness"
                && item.before_install.contains("suspend BitLocker")
                && item.installer_choice.contains("Keep my current OS")
                && item.final_check.contains("Windows start")
        }));
        assert!(status.policy.dual_boot_readiness.iter().any(|item| {
            item.title == "macOS readiness"
                && item
                    .before_install
                    .contains("native Goblins OS architecture")
                && item.installer_choice.contains("APFS containers")
                && item.final_check.contains("macOS start")
        }));
        assert!(status.policy.dual_boot_readiness.iter().any(|item| {
            item.title == "Linux readiness"
                && item.before_install.contains("LUKS")
                && item.installer_choice.contains("confirmed free space")
                && item.final_check.contains("existing Linux install")
        }));
        assert!(status.policy.dual_boot_readiness.iter().any(|item| {
            item.title == "Other OS or data readiness"
                && item.before_install.contains("unknown")
                && item.installer_choice.contains("dedicated Goblins OS disk")
        }));
        assert!(status.policy.dual_boot_readiness.iter().any(|item| {
            item.title == "Dedicated disk readiness"
                && item.before_install.contains("separate internal disk")
                && item.installer_choice.contains("dedicated disk is blank")
        }));
        assert_eq!(status.policy.dual_boot_choices.len(), 5);
        assert!(status.policy.dual_boot_choices.iter().any(|choice| {
            choice.title == "Keep Windows"
                && choice.preparation.contains("suspend BitLocker")
                && choice.preserve.contains("Microsoft Reserved")
        }));
        assert!(status.policy.dual_boot_choices.iter().any(|choice| {
            choice.title == "Keep macOS"
                && choice.preparation.contains("Disk Utility")
                && choice.finish.contains("macOS still boots")
        }));
        assert!(status.policy.dual_boot_choices.iter().any(|choice| {
            choice.title == "Keep Linux"
                && choice.preparation.contains("LUKS")
                && choice.preserve.contains("/home")
        }));
        assert!(status.policy.dual_boot_choices.iter().any(|choice| {
            choice.title == "Keep another OS or data" && choice.preserve.contains("unknown OS")
        }));
        assert!(status.policy.dual_boot_choices.iter().any(|choice| {
            choice.title == "Use a dedicated disk"
                && choice
                    .install_target
                    .contains("simple Goblins OS flow only if")
        }));
        assert_eq!(status.policy.dual_boot_guide.len(), 7);
        assert!(status
            .policy
            .dual_boot_guide
            .iter()
            .any(|step| step.title == "Windows" && step.detail.contains("Disk Management")));
        assert!(status
            .policy
            .dual_boot_guide
            .iter()
            .any(|step| step.title == "macOS/APFS" && step.detail.contains("Disk Utility")));
        assert!(status.policy.dual_boot_guide.iter().any(|step| {
            step.title == "Bootloader and EFI" && step.detail.contains("EFI System Partition")
        }));
        assert!(status.policy.dual_boot_guide.iter().any(|step| {
            step.title == "Startup menu" && step.detail.contains("firmware startup menu")
        }));
        assert!(status
            .policy
            .dual_boot_guide
            .iter()
            .any(|step| step.title == "Final storage review"));
        assert_eq!(status.policy.dual_boot_decision_map.len(), 5);
        assert!(status.policy.dual_boot_decision_map.iter().any(|decision| {
            decision.title == "Windows beside Goblins OS"
                && decision.prepare_space.contains("Disk Management")
                && decision.preserve.contains("Microsoft Reserved")
                && decision.boot_picker.contains("Windows")
        }));
        assert!(status.policy.dual_boot_decision_map.iter().any(|decision| {
            decision.title == "macOS beside Goblins OS"
                && decision.prepare_space.contains("Disk Utility")
                && decision.preserve.contains("APFS containers")
                && decision.boot_picker.contains("macOS")
        }));
        assert!(status.policy.dual_boot_decision_map.iter().any(|decision| {
            decision.title == "Linux beside Goblins OS"
                && decision.prepare_space.contains("LUKS")
                && decision.preserve.contains("/home")
                && decision.boot_picker.contains("existing Linux install")
        }));
        assert!(status.policy.dual_boot_decision_map.iter().any(|decision| {
            decision.title == "Another OS or shared data"
                && decision.best_for.contains("shared data")
                && decision.preserve.contains("unknown OS")
        }));
        assert!(status.policy.dual_boot_decision_map.iter().any(|decision| {
            decision.title == "Separate disk"
                && decision.install_target.contains("dedicated disk is blank")
                && decision.boot_picker.contains("firmware boot picker")
        }));
        assert_eq!(status.policy.storage_review_checklist.len(), 7);
        assert!(status
            .policy
            .storage_review_checklist
            .iter()
            .any(|item| item.title == "Formatting" && item.detail.contains("TPM2 LUKS")));
        assert!(status.policy.storage_review_checklist.iter().any(|item| {
            item.title == "Bootloader and EFI" && item.detail.contains("unformatted")
        }));
        assert!(status.policy.storage_review_checklist.iter().any(|item| {
            item.title == "Preserved systems" && item.detail.contains("data partitions")
        }));
        assert!(status.policy.storage_review_checklist.iter().any(|item| {
            item.title == "Required final summary"
                && item.detail.contains("target disk or free-space assignment")
                && item
                    .detail
                    .contains("every filesystem that will be formatted")
                && item.detail.contains("bootloader/EFI target")
        }));
        assert_eq!(status.policy.post_install_verification.len(), 5);
        assert!(status.policy.post_install_verification.iter().any(|item| {
            item.title == "Start Goblins OS" && item.detail.contains("firmware startup menu")
        }));
        assert!(status.policy.post_install_verification.iter().any(|item| {
            item.title == "Check preserved systems"
                && item.detail.contains("Windows")
                && item.detail.contains("macOS")
                && item.detail.contains("Linux")
        }));
        assert!(status.policy.post_install_verification.iter().any(|item| {
            item.title == "Recover missing entries" && item.detail.contains("Do not format")
        }));
        let policy_copy = serde_json::to_string(&status.policy).unwrap();
        assert!(policy_copy.contains("advanced storage"));
        let raw_installer_name = ["Ana", "conda"].concat();
        let raw_bootc_pairing = ["bootc/", raw_installer_name.as_str()].concat();
        let raw_bootc_installer = ["bootc", " installer"].concat();
        let raw_iso_storage_phrases = [
            ["ISO ", "installer"].concat(),
            ["ISO ", "Installation Destination"].concat(),
            ["ISO ", "manual storage"].concat(),
            ["manual storage from the ", "ISO"].concat(),
            ["Installation Destination/", "manual storage path"].concat(),
        ];
        assert!(!policy_copy.contains(&raw_installer_name));
        assert!(!policy_copy.contains(&raw_bootc_pairing));
        assert!(!policy_copy.contains(&raw_bootc_installer));
        for phrase in raw_iso_storage_phrases {
            assert!(!policy_copy.contains(&phrase));
        }
    }

    #[test]
    fn destructive_acknowledgement_includes_selected_device() {
        assert_eq!(
            destructive_acknowledgement("/dev/nvme0n1"),
            "WIPE /dev/nvme0n1 AND INSTALL GOBLINS OS"
        );
    }

    #[test]
    fn simple_install_api_rejects_custom_storage_overrides() {
        assert!(simple_install_filesystem("xfs").is_ok());
        for filesystem in ["ext4", "btrfs"] {
            let detail = simple_install_filesystem(filesystem).unwrap_err();
            assert!(detail.contains("only writes an xfs root"));
            assert!(detail.contains("advanced storage"));
            assert!(detail.contains("final summary"));
        }
        let detail = simple_install_filesystem("zfs").unwrap_err();
        assert!(detail.contains("advanced storage"));
        assert!(detail.contains("bootloader/EFI target"));

        assert!(simple_install_block_setup("direct").is_ok());
        let detail = simple_install_block_setup("tpm2-luks").unwrap_err();
        assert!(detail.contains("TPM2 LUKS"));
        assert!(detail.contains("advanced storage"));
        assert!(detail.contains("bootloader/EFI target"));
        let detail = simple_install_block_setup("lvm").unwrap_err();
        assert!(detail.contains("advanced storage"));
        assert!(detail.contains("bootloader/EFI target"));

        assert!(simple_install_wipe(true).is_ok());
        let detail = simple_install_wipe(false).unwrap_err();
        assert!(detail.contains("always uses the wipe guard"));
        assert!(detail.contains("advanced storage"));
        assert!(detail.contains("final summary"));
        assert!(simple_install_wipe_for_execute(false, None).is_ok());
        let detail = simple_install_wipe_for_execute(true, None).unwrap_err();
        assert!(detail.contains("explicit wipe=true"));
        assert!(detail.contains("device-specific confirmation phrase"));
        assert!(simple_install_wipe_for_execute(true, Some(true)).is_ok());
    }

    #[test]
    fn secure_boot_status_reports_non_uefi_context() {
        let status = secure_boot_status(false);

        assert_eq!(status.state, "not-uefi");
        assert!(status.detail.contains("UEFI firmware is not visible"));
    }

    #[test]
    fn native_architecture_policy_is_x86_64_and_aarch64_only() {
        assert!(native_supported_architecture("x86_64"));
        assert!(native_supported_architecture("aarch64"));
        assert!(!native_supported_architecture("arm"));
        assert!(!native_supported_architecture("riscv64"));
        assert!(!native_supported_architecture("x86"));
    }

    #[test]
    fn parses_firmware_boot_entries_for_dual_boot_review() {
        let entries = parse_boot_entries(
            "BootCurrent: 0002\n\
             Boot0000* Windows Boot Manager\tHD(1,GPT,...)\n\
             Boot0001  Fedora\tHD(1,GPT,...)\n\
             Boot0002* Goblins OS\tHD(1,GPT,...)\n\
             Boot0003* ubuntu\tHD(1,GPT,...)\n",
        );

        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].id, "0000");
        assert_eq!(entries[0].label, "Windows Boot Manager");
        assert_eq!(entries[0].target, "HD(1,GPT,...)");
        assert!(entries[0].active);
        assert_eq!(entries[1].label, "Fedora");
        assert_eq!(entries[1].target, "HD(1,GPT,...)");
        assert!(!entries[1].active);
        assert_eq!(entries[2].label, "Goblins OS");
        assert_eq!(entries[2].target, "HD(1,GPT,...)");
        assert_eq!(entries[3].label, "ubuntu");
        assert_eq!(entries[3].target, "HD(1,GPT,...)");
    }

    #[test]
    fn sectors_to_gib_rounds_up() {
        assert_eq!(sectors_to_gib(1), 1);
        assert_eq!(sectors_to_gib(67_108_864), 32);
    }

    #[test]
    fn scans_sys_block_and_protects_existing_system_partitions() {
        let root = temp_dir("goblins-os-sys-block");
        let disk = root.join("nvme0n1");
        fs::create_dir_all(disk.join("queue")).unwrap();
        fs::create_dir_all(disk.join("device")).unwrap();
        fs::write(disk.join("size"), "67108864\n").unwrap();
        fs::write(disk.join("removable"), "0\n").unwrap();
        fs::write(disk.join("queue/rotational"), "0\n").unwrap();
        fs::write(disk.join("device/model"), "Goblins Test Disk\n").unwrap();
        let partition = disk.join("nvme0n1p1");
        fs::create_dir_all(&partition).unwrap();
        fs::write(partition.join("partition"), "1\n").unwrap();
        fs::write(
            partition.join("uevent"),
            "DEVNAME=nvme0n1p1\nDEVTYPE=partition\nPARTNAME=EFI System Partition\nPART_ENTRY_TYPE=c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n",
        )
        .unwrap();

        let bootc = test_bootc_status();
        let targets = scan_install_targets_in(&root, &bootc);
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, "/dev/nvme0n1");
        assert_eq!(targets[0].size_gb, 32);
        assert_eq!(targets[0].partitions, vec!["/dev/nvme0n1p1"]);
        assert_eq!(targets[0].existing_systems[0].kind, "EFI boot");
        assert_eq!(targets[0].recommendation.title, "Keep EFI boot");
        assert!(targets[0]
            .recommendation
            .action
            .contains("Custom/manual storage or Reclaim Space"));
        assert!(targets[0]
            .recommendation
            .preserve
            .contains("/dev/nvme0n1p1"));
        assert_eq!(targets[0].dual_boot_plan.status, "manual-preserve-required");
        assert!(targets[0]
            .dual_boot_plan
            .summary
            .contains("guided dual-boot install"));
        assert!(targets[0]
            .dual_boot_plan
            .bootloader
            .contains("EFI System Partition"));
        let blocked_detail = ineligible_install_detail(&targets[0]);
        assert!(blocked_detail.contains("advanced storage"));
        assert!(blocked_detail.contains("Custom/manual storage or Reclaim Space"));
        assert!(blocked_detail.contains("unallocated free space"));
        assert!(!targets[0].eligible);
        assert!(targets[0].reasons[0].contains("Existing EFI boot detected"));
    }

    #[test]
    fn scans_sys_block_and_routes_existing_operating_systems_to_manual_storage() {
        let root = temp_dir("goblins-os-dual-boot-sys-block");
        let disk = root.join("nvme2n1");
        fs::create_dir_all(disk.join("queue")).unwrap();
        fs::create_dir_all(disk.join("device")).unwrap();
        fs::write(disk.join("size"), "268435456\n").unwrap();
        fs::write(disk.join("removable"), "0\n").unwrap();
        fs::write(disk.join("queue/rotational"), "0\n").unwrap();
        fs::write(disk.join("device/model"), "Goblins Dual Boot Disk\n").unwrap();

        for (name, metadata) in [
            (
                "nvme2n1p1",
                "DEVNAME=nvme2n1p1\nDEVTYPE=partition\nTYPE=BitLocker\nID_FS_TYPE=ntfs\nPARTLABEL=Windows\n",
            ),
            (
                "nvme2n1p2",
                "DEVNAME=nvme2n1p2\nDEVTYPE=partition\nTYPE=apfs\nPART_ENTRY_TYPE=48465300-0000-11AA-AA11-00306543ECAC\nPARTLABEL=Macintosh HD\n",
            ),
            (
                "nvme2n1p3",
                "DEVNAME=nvme2n1p3\nDEVTYPE=partition\nTYPE=crypto_LUKS\nID_FS_TYPE=f2fs\nPARTLABEL=Linux encrypted root\n",
            ),
            (
                "nvme2n1p4",
                "DEVNAME=nvme2n1p4\nDEVTYPE=partition\nTYPE=zfs_member\nPARTLABEL=Shared data\n",
            ),
        ] {
            let partition = disk.join(name);
            fs::create_dir_all(&partition).unwrap();
            fs::write(partition.join("partition"), "1\n").unwrap();
            fs::write(partition.join("uevent"), metadata).unwrap();
        }

        let bootc = test_bootc_status();
        let targets = scan_install_targets_in(&root, &bootc);
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, "/dev/nvme2n1");
        assert!(!targets[0].eligible);
        assert_eq!(
            targets[0]
                .existing_systems
                .iter()
                .map(|system| system.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["Windows", "macOS/APFS", "Linux", "Other OS/data"]
        );
        assert!(targets[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("Custom/manual storage or Reclaim Space")));
        assert_eq!(
            targets[0].recommendation.title,
            "Keep Windows, macOS/APFS, Linux and Other OS/data"
        );
        assert!(targets[0]
            .recommendation
            .action
            .contains("do not use the whole-disk erase path"));
        assert!(targets[0]
            .recommendation
            .install_target
            .contains("unallocated free space"));
        assert!(targets[0]
            .recommendation
            .preserve
            .contains("/dev/nvme2n1p4"));
        assert!(targets[0]
            .recommendation
            .finish
            .contains("firmware startup menu"));
        assert_eq!(targets[0].dual_boot_plan.status, "manual-preserve-required");
        assert!(targets[0]
            .dual_boot_plan
            .title
            .contains("Keep Windows, macOS/APFS, Linux and Other OS/data"));
        assert!(targets[0]
            .dual_boot_plan
            .primary_action
            .contains("Custom/manual storage or Reclaim Space"));
        assert!(targets[0]
            .dual_boot_plan
            .storage_target
            .contains("unallocated free space"));
        assert!(targets[0]
            .dual_boot_plan
            .preserve
            .contains("/dev/nvme2n1p4"));
        assert!(targets[0]
            .dual_boot_plan
            .finish
            .contains("every preserved system"));
        assert_eq!(targets[0].dual_boot_plan.steps.len(), 5);
        assert!(targets[0]
            .existing_systems
            .iter()
            .any(|system| system.preservation.contains("Disk Management")));
        assert!(targets[0]
            .existing_systems
            .iter()
            .any(|system| system.preservation.contains("Disk Utility")));
        assert!(targets[0]
            .existing_systems
            .iter()
            .any(|system| system.preservation.contains("Resize Linux")));
        assert!(targets[0].existing_systems.iter().any(|system| system
            .preservation
            .contains("Treat this as data to preserve")));
        let blocked_detail = ineligible_install_detail(&targets[0]);
        assert!(blocked_detail.contains("contains Windows, macOS/APFS, Linux and Other OS/data"));
        assert!(blocked_detail.contains("To keep Windows, macOS, Linux, another OS, or data"));
        assert!(blocked_detail.contains("select only unallocated free space"));
    }

    #[test]
    fn unreadable_partition_scan_disables_simple_install() {
        let root = temp_dir("goblins-os-unreadable-sys-block");
        let disk = root.join("nvme9n1");
        fs::write(&disk, "not a directory").unwrap();

        let bootc = test_bootc_status();
        let target = build_install_target(&disk, &bootc).expect("target should be represented");
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(target.path, "/dev/nvme9n1");
        assert!(target.partitions.is_empty());
        assert!(target.existing_systems.is_empty());
        assert!(!target.eligible);
        assert!(target.reasons.iter().any(|reason| {
            reason.contains("Partition scan for /dev/nvme9n1 was not readable")
                && reason.contains("simple install is disabled")
                && reason.contains("bootloader/EFI rows")
        }));
        let blocked_detail = ineligible_install_detail(&target);
        assert!(blocked_detail.contains("advanced storage"));
        assert!(blocked_detail.contains("Do not use the simple erase flow"));
        assert!(block_partitions(&root.join("missing"), "nvmeMissing").is_err());
    }

    #[test]
    fn blank_internal_disk_stays_eligible_for_whole_disk_install() {
        let root = temp_dir("goblins-os-blank-sys-block");
        let disk = root.join("nvme1n1");
        fs::create_dir_all(disk.join("queue")).unwrap();
        fs::create_dir_all(disk.join("device")).unwrap();
        fs::write(disk.join("size"), "67108864\n").unwrap();
        fs::write(disk.join("removable"), "0\n").unwrap();
        fs::write(disk.join("queue/rotational"), "0\n").unwrap();
        fs::write(disk.join("device/model"), "Goblins Blank Disk\n").unwrap();

        let bootc = test_bootc_status();
        let targets = scan_install_targets_in(&root, &bootc);
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, "/dev/nvme1n1");
        assert!(targets[0].partitions.is_empty());
        assert!(targets[0].existing_systems.is_empty());
        assert_eq!(targets[0].recommendation.title, "Replace this blank disk");
        assert!(targets[0]
            .recommendation
            .install_target
            .contains("fresh GPT layout"));
        assert!(targets[0]
            .recommendation
            .preserve
            .contains("readable installer scan"));
        assert_eq!(
            targets[0].dual_boot_plan.status,
            "blank-dedicated-disk-ready"
        );
        assert!(targets[0]
            .dual_boot_plan
            .summary
            .contains("dedicated Goblins OS disk"));
        assert!(targets[0]
            .dual_boot_plan
            .storage_target
            .contains("fresh GPT layout"));
        assert!(targets[0].eligible);
    }

    fn test_bootc_status() -> BootcInstallStatus {
        BootcInstallStatus {
            available: true,
            privileged: true,
            image: "localhost/goblins-os:test".to_string(),
            install_config_path: "/usr/lib/bootc/install/00-goblins-os.toml".to_string(),
            default_filesystem: "xfs",
            command_model: "Goblins OS disk install --filesystem xfs --wipe <device>",
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        ensure_clean(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn ensure_clean(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).unwrap();
        }
    }
}
