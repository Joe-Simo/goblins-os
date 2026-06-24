use axum::Json;
use serde::Serialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};
use sysinfo::{Disks, System};

use crate::{accelerators::DetectedGpu, model_manager::RuntimeReport};

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Serialize)]
pub struct HardwareStatus {
    generated_at: String,
    source: &'static str,
    platform: PlatformStatus,
    memory: MemoryStatus,
    accelerators: Vec<DetectedGpu>,
    storage: Vec<StorageVolume>,
    runtimes: RuntimeReport,
    facilities: Vec<SystemFacility>,
}

#[derive(Serialize)]
pub struct StorageVolume {
    id: &'static str,
    mount_point: String,
    total_gb: u64,
    available_gb: u64,
}

#[derive(Serialize)]
pub struct PlatformStatus {
    os: String,
    desktop: String,
    session_type: String,
    current_desktop: String,
}

#[derive(Serialize)]
pub struct MemoryStatus {
    total_gb: u64,
    available_gb: u64,
}

#[derive(Serialize)]
pub struct SystemFacility {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) state: FacilityState,
    pub(crate) detail: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FacilityState {
    Ready,
    Waiting,
}

pub async fn hardware_status() -> Json<HardwareStatus> {
    Json(build_hardware_status())
}

pub fn build_hardware_status() -> HardwareStatus {
    let mut system = System::new();
    system.refresh_memory();

    HardwareStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        platform: PlatformStatus {
            os: env::consts::OS.to_string(),
            desktop: env_string("DESKTOP_SESSION", "unconfigured"),
            session_type: env_string("XDG_SESSION_TYPE", "unconfigured"),
            current_desktop: env_string("XDG_CURRENT_DESKTOP", "unconfigured"),
        },
        memory: MemoryStatus {
            total_gb: bytes_to_gib(system.total_memory()),
            available_gb: bytes_to_gib(system.available_memory()),
        },
        accelerators: crate::accelerators::detect_gpus(),
        storage: storage_volumes(),
        runtimes: crate::model_manager::detect_runtimes(),
        facilities: {
            let mut facilities = vec![
                display_facility(),
                input_facility(),
                network_facility(),
                bluetooth_facility(),
                audio_facility(),
                accessibility_facility(),
                display_manager_facility(),
                boot_image_facility(),
            ];
            facilities.extend(privacy_facility_checks());
            facilities
        },
    }
}

pub fn recovery_facility_checks() -> Vec<SystemFacility> {
    build_hardware_status().facilities
}

pub(crate) fn privacy_facility_checks() -> Vec<SystemFacility> {
    vec![portal_facility(), keyring_facility(), policy_facility()]
}

fn display_facility() -> SystemFacility {
    let has_wayland = env::var("WAYLAND_DISPLAY").is_ok();
    let has_dri = Path::new("/dev/dri").exists();
    facility(
        "display-compositor",
        "Display and compositor",
        has_wayland || has_dri || executable_exists("gnome-shell"),
        "Goblins OS expects a native desktop session with compositor, input, portal, and display-device support.",
        vec![
            evidence_env("WAYLAND_DISPLAY"),
            evidence_path("/dev/dri"),
            evidence_binary("gnome-shell"),
        ],
    )
}

fn input_facility() -> SystemFacility {
    facility(
        "input-devices",
        "Keyboard, pointer, and input",
        Path::new("/dev/input").exists() || Path::new("/proc/bus/input/devices").exists(),
        "Keyboard, pointer, tablet, and other input devices available to this session.",
        vec![
            evidence_path("/dev/input"),
            evidence_path("/proc/bus/input/devices"),
        ],
    )
}

fn network_facility() -> SystemFacility {
    let nm_active = systemctl_is_active("NetworkManager.service");
    facility(
        "networking",
        "Networking",
        nm_active || executable_exists("NetworkManager") || executable_exists("nmcli"),
        "Goblins OS manages network connectivity for Wi-Fi, wired, VPN, and DNS.",
        vec![
            evidence_binary("NetworkManager"),
            evidence_binary("nmcli"),
            evidence_systemd("NetworkManager.service", nm_active),
        ],
    )
}

fn bluetooth_facility() -> SystemFacility {
    let bluetooth_active = systemctl_is_active("bluetooth.service");
    let bluetooth_daemon_available = executable_exists("bluetoothd");
    facility(
        "bluetooth",
        "Bluetooth",
        bluetooth_ready(bluetooth_active, bluetooth_daemon_available),
        "Bluetooth pairing and device connections are available when supported hardware is present.",
        vec![
            evidence_binary("bluetoothctl"),
            evidence_binary("bluetoothd"),
            evidence_systemd("bluetooth.service", bluetooth_active),
        ],
    )
}

fn bluetooth_ready(service_active: bool, daemon_available: bool) -> bool {
    service_active || daemon_available
}

fn audio_facility() -> SystemFacility {
    let pipewire_active = systemctl_user_is_active("pipewire.service");
    let wireplumber_active = systemctl_user_is_active("wireplumber.service");
    facility(
        "audio",
        "Audio",
        pipewire_active
            || wireplumber_active
            || executable_exists("pipewire")
            || executable_exists("wireplumber"),
        "Audio is expected through PipeWire and WirePlumber in the Goblins OS session.",
        vec![
            evidence_binary("pipewire"),
            evidence_binary("wireplumber"),
            evidence_systemd_user("pipewire.service", pipewire_active),
            evidence_systemd_user("wireplumber.service", wireplumber_active),
        ],
    )
}

fn portal_facility() -> SystemFacility {
    let portal_active = systemctl_user_is_active("xdg-desktop-portal.service");
    facility(
        "desktop-portals",
        "Desktop portals",
        portal_active
            || executable_exists("xdg-desktop-portal")
            || executable_exists("xdg-desktop-portal-gnome"),
        "Portals provide native app integration for permissions, files, and desktop handoff.",
        vec![
            evidence_binary("xdg-desktop-portal"),
            evidence_binary("xdg-desktop-portal-gnome"),
            evidence_systemd_user("xdg-desktop-portal.service", portal_active),
        ],
    )
}

fn accessibility_facility() -> SystemFacility {
    facility(
        "accessibility",
        "Accessibility services",
        executable_exists("at-spi-bus-launcher")
            || Path::new("/usr/libexec/at-spi-bus-launcher").exists(),
        "Assistive technologies, such as screen readers, for native desktop apps.",
        vec![
            evidence_binary("at-spi-bus-launcher"),
            evidence_path("/usr/libexec/at-spi-bus-launcher"),
        ],
    )
}

fn keyring_facility() -> SystemFacility {
    facility(
        "keyring",
        "Keyring",
        executable_exists("gnome-keyring-daemon"),
        "The desktop keyring is expected for credential integration; OpenAI secrets still remain OS-service owned.",
        vec![evidence_binary("gnome-keyring-daemon")],
    )
}

fn policy_facility() -> SystemFacility {
    let polkit_active = systemctl_is_active("polkit.service");
    facility(
        "policy",
        "Policy and privilege prompts",
        polkit_active || executable_exists("polkitd") || executable_exists("pkaction"),
        "PolicyKit availability is required for native privileged OS actions and prompts.",
        vec![
            evidence_binary("polkitd"),
            evidence_binary("pkaction"),
            evidence_systemd("polkit.service", polkit_active),
        ],
    )
}

fn display_manager_facility() -> SystemFacility {
    let gdm_active = systemctl_is_active("gdm.service");
    facility(
        "display-manager",
        "Display manager",
        gdm_active || executable_exists("gdm"),
        "The Goblins OS sign-in window is configured for this device.",
        vec![
            evidence_binary("gdm"),
            evidence_systemd("gdm.service", gdm_active),
        ],
    )
}

fn boot_image_facility() -> SystemFacility {
    // Presence of the `bootc` binary is evidence of image tooling, not proof
    // that status/rollback work in this session. The detail says only what the
    // probe can honestly back up; live deployment state comes from
    // `GET /v1/system/image`, which actually runs `bootc status`.
    facility(
        "boot-image",
        "Boot image management",
        executable_exists("bootc"),
        "System image tooling is present; live image status and rollback are reported by the system image service.",
        vec![evidence_binary("bootc")],
    )
}

fn facility(
    id: &'static str,
    label: &'static str,
    ready: bool,
    detail: &'static str,
    evidence: Vec<String>,
) -> SystemFacility {
    SystemFacility {
        id,
        label,
        state: if ready {
            FacilityState::Ready
        } else {
            FacilityState::Waiting
        },
        detail: detail.to_string(),
        evidence,
    }
}

fn systemctl_is_active(service: &str) -> bool {
    command_success("systemctl", &["is-active", "--quiet", service])
}

fn systemctl_user_is_active(service: &str) -> bool {
    command_success("systemctl", &["--user", "is-active", "--quiet", service])
}

fn command_success(binary: &str, args: &[&str]) -> bool {
    Command::new(binary)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn evidence_binary(binary: &str) -> String {
    format!(
        "{binary}:{}",
        if executable_exists(binary) {
            "present"
        } else {
            "missing"
        }
    )
}

fn evidence_path(path: &str) -> String {
    format!(
        "{path}:{}",
        if Path::new(path).exists() {
            "present"
        } else {
            "missing"
        }
    )
}

fn evidence_env(key: &str) -> String {
    format!(
        "{key}:{}",
        env::var(key).unwrap_or_else(|_| "missing".to_string())
    )
}

fn evidence_systemd(service: &str, active: bool) -> String {
    format!(
        "{service}:{}",
        if active {
            "active"
        } else {
            "inactive-or-unavailable"
        }
    )
}

fn evidence_systemd_user(service: &str, active: bool) -> String {
    format!(
        "user/{service}:{}",
        if active {
            "active"
        } else {
            "inactive-or-unavailable"
        }
    )
}

fn storage_volumes() -> Vec<StorageVolume> {
    let disks = Disks::new_with_refreshed_list();
    let mut volumes: Vec<StorageVolume> = Vec::new();

    for (id, path) in [
        ("system-root", PathBuf::from("/")),
        ("model-store", crate::model_manager::model_dir()),
    ] {
        let Some(disk) = disks
            .iter()
            .filter(|disk| path.starts_with(disk.mount_point()))
            .max_by_key(|disk| disk.mount_point().to_string_lossy().len())
        else {
            continue;
        };

        let mount_point = disk.mount_point().to_string_lossy().to_string();
        if volumes
            .iter()
            .any(|volume| volume.mount_point == mount_point)
        {
            continue;
        }

        volumes.push(StorageVolume {
            id,
            mount_point,
            total_gb: bytes_to_gib(disk.total_space()),
            available_gb: bytes_to_gib(disk.available_space()),
        });
    }

    volumes
}

fn bytes_to_gib(bytes: u64) -> u64 {
    bytes.div_ceil(GIB)
}

fn env_string(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn executable_exists(binary: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| {
        let candidate: PathBuf = path.join(binary);
        fs::metadata(candidate)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::{bluetooth_ready, build_hardware_status, bytes_to_gib, recovery_facility_checks};

    #[test]
    fn hardware_status_reports_core_os_facilities() {
        let status = build_hardware_status();

        assert_eq!(status.source, "goblins-os-core");
        assert!(status
            .facilities
            .iter()
            .any(|facility| facility.id == "networking"));
        assert!(status
            .facilities
            .iter()
            .any(|facility| facility.id == "audio"));
        assert!(status
            .facilities
            .iter()
            .any(|facility| facility.id == "desktop-portals"));
    }

    #[test]
    fn bluetooth_readiness_requires_service_or_daemon() {
        assert!(!bluetooth_ready(false, false));
        assert!(bluetooth_ready(true, false));
        assert!(bluetooth_ready(false, true));
    }

    #[test]
    fn recovery_facilities_are_available_for_recovery_view() {
        let checks = recovery_facility_checks();

        assert!(checks.iter().any(|facility| facility.id == "boot-image"));
    }

    #[test]
    fn bytes_to_gib_rounds_up() {
        assert_eq!(bytes_to_gib(1), 1);
        assert_eq!(bytes_to_gib(1024 * 1024 * 1024), 1);
    }

    #[test]
    fn hardware_status_scans_accelerators_storage_and_runtimes() {
        let status = build_hardware_status();
        let json = serde_json::to_value(&status).unwrap();

        assert!(json
            .get("accelerators")
            .is_some_and(|value| value.is_array()));
        assert!(json.get("storage").is_some_and(|value| value.is_array()));
        let runtimes = json.get("runtimes").expect("runtimes section present");
        assert!(runtimes.get("ollama").is_some());
        assert!(runtimes.get("vllm").is_some());
        assert!(runtimes.get("lm_studio").is_some());
    }
}
