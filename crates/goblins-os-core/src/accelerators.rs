//! Cross-vendor accelerator detection used by both the system hardware view and
//! the local model manager.
//!
//! Detection reads the Linux DRM sysfs tree so that AMD, Intel, and NVIDIA GPUs
//! are all recognized without depending on a single vendor's tooling. NVIDIA's
//! proprietary driver does not publish VRAM through sysfs, so an `nvidia-smi`
//! probe fills that gap when such a card is present. An explicit operator
//! override remains authoritative for headless, virtualized, or passthrough
//! installs that the on-host probes cannot see.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;

const GIB: u64 = 1024 * 1024 * 1024;
const DRM_CLASS_DIR: &str = "/sys/class/drm";

#[derive(Serialize, Clone)]
pub struct DetectedGpu {
    pub vendor: String,
    pub vendor_id: Option<String>,
    pub model_id: Option<String>,
    pub driver: Option<String>,
    pub vram_gb: Option<u64>,
    pub source: &'static str,
}

/// Detect every GPU exposed through the DRM subsystem, enriching NVIDIA cards
/// with VRAM reported by `nvidia-smi` when sysfs is silent about it.
pub fn detect_gpus() -> Vec<DetectedGpu> {
    let mut gpus = detect_drm_gpus();

    let needs_nvidia_vram = gpus
        .iter()
        .any(|gpu| vendor_is_nvidia(&gpu.vendor) && gpu.vram_gb.is_none());
    if needs_nvidia_vram {
        if let Some(vram_gb) = detect_nvidia_vram_gb() {
            for gpu in gpus
                .iter_mut()
                .filter(|gpu| vendor_is_nvidia(&gpu.vendor) && gpu.vram_gb.is_none())
            {
                gpu.vram_gb = Some(vram_gb);
                gpu.source = "drm-sysfs+nvidia-smi";
            }
        }
    }

    gpus
}

/// The VRAM figure model eligibility should trust: an operator override wins,
/// otherwise the largest VRAM across detected accelerators.
pub fn best_vram_gb() -> Option<u64> {
    if let Some(configured) = configured_vram_gb() {
        return Some(configured);
    }

    detect_gpus()
        .into_iter()
        .filter_map(|gpu| gpu.vram_gb)
        .max()
}

fn detect_drm_gpus() -> Vec<DetectedGpu> {
    let mut cards: Vec<String> = match fs::read_dir(DRM_CLASS_DIR) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_card_name(name))
            .collect(),
        Err(_) => return Vec::new(),
    };
    cards.sort();

    cards
        .into_iter()
        .map(|card| {
            let device = Path::new(DRM_CLASS_DIR).join(&card).join("device");
            let vendor_id = read_trimmed(device.join("vendor"));
            let model_id = read_trimmed(device.join("device"));
            let driver = driver_name(&device);
            let vram_gb = read_trimmed(device.join("mem_info_vram_total"))
                .as_deref()
                .and_then(parse_decimal_bytes)
                .map(bytes_to_gib);

            DetectedGpu {
                vendor: vendor_label(vendor_id.as_deref()),
                vendor_id,
                model_id,
                driver,
                vram_gb,
                source: "drm-sysfs",
            }
        })
        .collect()
}

fn driver_name(device: &Path) -> Option<String> {
    if let Ok(target) = fs::read_link(device.join("driver")) {
        if let Some(name) = target.file_name().and_then(|name| name.to_str()) {
            return Some(name.to_string());
        }
    }

    fs::read_to_string(device.join("uevent"))
        .ok()
        .as_deref()
        .and_then(parse_driver_from_uevent)
}

fn detect_nvidia_vram_gb() -> Option<u64> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_nvidia_smi_vram_mb_max(&String::from_utf8_lossy(&output.stdout))
        .map(|mb| bytes_to_gib(mb * 1024 * 1024))
}

fn configured_vram_gb() -> Option<u64> {
    env::var("GOBLINS_OS_GPU_VRAM_GB")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn read_trimmed(path: PathBuf) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_card_name(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

fn vendor_label(vendor_id: Option<&str>) -> String {
    match vendor_id.map(normalize_pci_id).as_deref() {
        Some("0x10de") => "NVIDIA".to_string(),
        Some("0x1002") => "AMD".to_string(),
        Some("0x8086") => "Intel".to_string(),
        Some("0x1af4") => "Virtio".to_string(),
        Some("0x15ad") => "VMware".to_string(),
        Some("0x1234") => "QEMU".to_string(),
        Some(other) => format!("PCI {other}"),
        None => "Unknown".to_string(),
    }
}

fn vendor_is_nvidia(vendor: &str) -> bool {
    vendor.eq_ignore_ascii_case("NVIDIA")
}

fn normalize_pci_id(id: &str) -> String {
    let trimmed = id.trim();
    if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
        trimmed.to_ascii_lowercase()
    } else {
        format!("0x{}", trimmed.to_ascii_lowercase())
    }
}

fn parse_decimal_bytes(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok().filter(|bytes| *bytes > 0)
}

fn parse_driver_from_uevent(uevent: &str) -> Option<String> {
    uevent
        .lines()
        .find_map(|line| line.trim().strip_prefix("DRIVER="))
        .map(|driver| driver.trim().to_string())
        .filter(|driver| !driver.is_empty())
}

fn parse_nvidia_smi_vram_mb_max(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .filter(|mb| *mb > 0)
        .max()
}

fn bytes_to_gib(bytes: u64) -> u64 {
    bytes.div_ceil(GIB)
}

#[cfg(test)]
mod tests {
    use super::{
        is_card_name, parse_decimal_bytes, parse_driver_from_uevent, parse_nvidia_smi_vram_mb_max,
        vendor_is_nvidia, vendor_label,
    };

    #[test]
    fn only_real_drm_cards_are_matched() {
        assert!(is_card_name("card0"));
        assert!(is_card_name("card12"));
        assert!(!is_card_name("card0-DP-1"));
        assert!(!is_card_name("card"));
        assert!(!is_card_name("renderD128"));
        assert!(!is_card_name("controlD64"));
    }

    #[test]
    fn vendor_ids_map_across_gpu_vendors() {
        assert_eq!(vendor_label(Some("0x10de")), "NVIDIA");
        assert_eq!(vendor_label(Some("0x1002")), "AMD");
        assert_eq!(vendor_label(Some("0x8086")), "Intel");
        assert_eq!(vendor_label(Some("10DE")), "NVIDIA");
        assert_eq!(vendor_label(Some("0x9999")), "PCI 0x9999");
        assert_eq!(vendor_label(None), "Unknown");
        assert!(vendor_is_nvidia(&vendor_label(Some("0x10de"))));
        assert!(!vendor_is_nvidia(&vendor_label(Some("0x1002"))));
    }

    #[test]
    fn vram_bytes_parse_only_when_positive() {
        assert_eq!(parse_decimal_bytes("17163091968\n"), Some(17163091968));
        assert_eq!(parse_decimal_bytes("0"), None);
        assert_eq!(parse_decimal_bytes("not-a-number"), None);
    }

    #[test]
    fn driver_is_read_from_uevent_when_symlink_is_absent() {
        let uevent = "DRIVER=amdgpu\nPCI_CLASS=30000\nPCI_ID=1002:73FF\n";
        assert_eq!(parse_driver_from_uevent(uevent), Some("amdgpu".to_string()));
        assert_eq!(parse_driver_from_uevent("PCI_CLASS=30000\n"), None);
    }

    #[test]
    fn nvidia_smi_reports_the_largest_card() {
        assert_eq!(parse_nvidia_smi_vram_mb_max("24576\n81920\n"), Some(81920));
        assert_eq!(parse_nvidia_smi_vram_mb_max(""), None);
        assert_eq!(parse_nvidia_smi_vram_mb_max("0\n0\n"), None);
    }
}
