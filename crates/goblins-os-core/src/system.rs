use axum::Json;
use serde::Serialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

const DEFAULT_SYSTEMD_UNIT_DIR: &str = "/usr/lib/systemd/system";
const DEFAULT_LIBEXEC_DIR: &str = "/usr/libexec/goblins-os";

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
pub struct SystemServicesStatus {
    generated_at: String,
    source: &'static str,
    manager_available: bool,
    unit_dir: String,
    libexec_dir: String,
    services: Vec<OsServiceStatus>,
}

#[derive(Serialize)]
pub struct OsServiceStatus {
    id: &'static str,
    label: &'static str,
    unit: &'static str,
    binary: Option<&'static str>,
    expected_state: &'static str,
    state: SystemServiceState,
    unit_file: String,
    unit_file_present: bool,
    binary_path: Option<String>,
    binary_present: Option<bool>,
    detail: String,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SystemServiceState {
    Active,
    Inactive,
    Waiting,
}

pub(crate) struct SystemServiceRecoveryCheck {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) ready: bool,
    pub(crate) detail: String,
}

struct ServiceDefinition {
    id: &'static str,
    label: &'static str,
    unit: &'static str,
    binary: Option<&'static str>,
    expected_state: &'static str,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "goblins-os-core",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub async fn system_services() -> Json<SystemServicesStatus> {
    Json(build_system_services_status())
}

pub(crate) fn system_service_recovery_checks() -> Vec<SystemServiceRecoveryCheck> {
    build_system_services_status()
        .services
        .into_iter()
        .map(|service| SystemServiceRecoveryCheck {
            id: service.id,
            label: service.label,
            ready: service.recovery_ready(),
            detail: service.detail,
        })
        .collect()
}

pub(crate) fn build_system_services_status() -> SystemServicesStatus {
    let unit_dir = env::var("GOBLINS_OS_SYSTEMD_UNIT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SYSTEMD_UNIT_DIR));
    let libexec_dir = env::var("GOBLINS_OS_LIBEXEC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_LIBEXEC_DIR));
    let manager_available = executable_exists("systemctl");

    build_system_services_status_from(&unit_dir, &libexec_dir, manager_available)
}

fn build_system_services_status_from(
    unit_dir: &Path,
    libexec_dir: &Path,
    manager_available: bool,
) -> SystemServicesStatus {
    SystemServicesStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        manager_available,
        unit_dir: unit_dir.display().to_string(),
        libexec_dir: libexec_dir.display().to_string(),
        services: service_definitions()
            .into_iter()
            .map(|definition| service_status(definition, unit_dir, libexec_dir, manager_available))
            .collect(),
    }
}

fn service_status(
    definition: ServiceDefinition,
    unit_dir: &Path,
    libexec_dir: &Path,
    manager_available: bool,
) -> OsServiceStatus {
    let unit_file = unit_dir.join(definition.unit);
    let unit_file_present = regular_file_exists(&unit_file);
    let binary_path = definition.binary.map(|binary| libexec_dir.join(binary));
    let binary_present = binary_path
        .as_ref()
        .map(|path| regular_file_exists(path.as_path()));
    let state = if !manager_available {
        SystemServiceState::Waiting
    } else if systemctl_is_active(definition.unit) {
        SystemServiceState::Active
    } else {
        SystemServiceState::Inactive
    };

    OsServiceStatus {
        id: definition.id,
        label: definition.label,
        unit: definition.unit,
        binary: definition.binary,
        expected_state: definition.expected_state,
        state,
        unit_file: unit_file.display().to_string(),
        unit_file_present,
        binary_path: binary_path.map(|path| path.display().to_string()),
        binary_present,
        detail: service_detail(
            &definition,
            state,
            manager_available,
            unit_file_present,
            binary_present,
        ),
    }
}

fn service_detail(
    definition: &ServiceDefinition,
    state: SystemServiceState,
    manager_available: bool,
    unit_file_present: bool,
    binary_present: Option<bool>,
) -> String {
    let support_label = if unit_file_present && binary_present.unwrap_or(true) {
        "Required support is installed."
    } else {
        "Required support is incomplete."
    };

    match state {
        SystemServiceState::Active => format!(
            "{} is running as expected. {support_label}",
            definition.label
        ),
        SystemServiceState::Inactive if manager_available => format!(
            "{} is installed but not running. {support_label}",
            definition.label
        ),
        SystemServiceState::Waiting => format!(
            "{} status will be available after system services are ready. {support_label}",
            definition.label
        ),
        SystemServiceState::Inactive => {
            format!("{} is not running. {support_label}", definition.label)
        }
    }
}

impl OsServiceStatus {
    fn recovery_ready(&self) -> bool {
        self.unit_file_present
            && self.binary_present.unwrap_or(true)
            && matches!(self.state, SystemServiceState::Active)
    }
}

fn service_definitions() -> Vec<ServiceDefinition> {
    vec![
        ServiceDefinition {
            id: "os-core-service",
            label: "Goblins OS core",
            unit: "goblins-os-core.service",
            binary: Some("goblins-os-core"),
            expected_state: "active",
        },
        ServiceDefinition {
            id: "resident-service",
            label: "Goblins AI runtime",
            unit: "goblins-os-resident.service",
            binary: Some("goblins-os-resident"),
            expected_state: "active",
        },
        ServiceDefinition {
            id: "model-cache-service",
            label: "Local model cache preparation",
            unit: "goblins-os-model-cache.service",
            binary: None,
            expected_state: "completed",
        },
        ServiceDefinition {
            id: "display-manager-service",
            label: "Sign-in window",
            unit: "gdm.service",
            binary: None,
            expected_state: "active",
        },
        ServiceDefinition {
            id: "network-manager-service",
            label: "Network",
            unit: "NetworkManager.service",
            binary: None,
            expected_state: "active",
        },
    ]
}

fn systemctl_is_active(service: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", service])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn regular_file_exists(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn executable_exists(binary: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| regular_file_exists(&path.join(binary)))
}

#[cfg(test)]
mod tests {
    use super::{build_system_services_status_from, service_definitions, SystemServiceState};
    use std::{fs, path::PathBuf, process};

    #[test]
    fn service_status_tracks_core_units_and_binaries() {
        let root =
            std::env::temp_dir().join(format!("goblins-os-system-services-test-{}", process::id()));
        let unit_dir = root.join("units");
        let libexec_dir = root.join("libexec");
        fs::create_dir_all(&unit_dir).unwrap();
        fs::create_dir_all(&libexec_dir).unwrap();
        fs::write(unit_dir.join("goblins-os-core.service"), "").unwrap();
        fs::write(libexec_dir.join("goblins-os-core"), "").unwrap();

        let status = build_system_services_status_from(&unit_dir, &libexec_dir, false);
        let core = status
            .services
            .iter()
            .find(|service| service.id == "os-core-service")
            .unwrap();

        assert_eq!(status.source, "goblins-os-core");
        assert_eq!(core.state, SystemServiceState::Waiting);
        assert!(core.unit_file_present);
        assert_eq!(core.binary_present, Some(true));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn service_definitions_include_the_os_services() {
        let definitions = service_definitions();
        let ids = definitions
            .iter()
            .map(|definition| definition.id)
            .collect::<Vec<_>>();

        assert!(ids.contains(&"os-core-service"));
        assert!(ids.contains(&"resident-service"));
        assert!(ids.contains(&"model-cache-service"));
        assert!(definitions
            .iter()
            .any(|definition| definition.id == "resident-service"
                && definition.label == "Goblins AI runtime"));
        let old_runtime_label = ["Codex", "resident"].join(" ");
        assert!(!definitions
            .iter()
            .any(|definition| definition.label.contains(&old_runtime_label)));
    }

    #[test]
    fn missing_files_are_reported_as_not_ready() {
        let unit_dir = PathBuf::from("/tmp/goblins-os-missing-units");
        let libexec_dir = PathBuf::from("/tmp/goblins-os-missing-libexec");
        let status = build_system_services_status_from(&unit_dir, &libexec_dir, false);
        let resident = status
            .services
            .iter()
            .find(|service| service.id == "resident-service")
            .unwrap();

        assert!(!resident.unit_file_present);
        assert_eq!(resident.binary_present, Some(false));
    }
}
