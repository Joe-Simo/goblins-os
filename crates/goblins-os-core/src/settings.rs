use axum::Json;
use serde::Serialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::auth::{openai_account_authenticated, openai_auth_provider_configured};
use crate::hardware::{recovery_facility_checks, FacilityState};
use crate::system::system_service_recovery_checks;
use crate::system_image::system_image_summary;

#[derive(Serialize)]
pub struct SettingsSystemStatus {
    generated_at: String,
    source: &'static str,
    session: SessionSettings,
    identity: IdentitySettings,
    local_account: LocalAccountSettings,
    storage: StorageSettings,
    services: ServiceSettings,
}

#[derive(Serialize)]
pub struct RecoveryStatus {
    generated_at: String,
    source: &'static str,
    checks: Vec<RecoveryCheck>,
}

#[derive(Serialize)]
pub struct SessionSettings {
    desktop: String,
    gui_platform: String,
    shell_mode: String,
}

#[derive(Serialize)]
pub struct IdentitySettings {
    provider_configured: bool,
    account_authenticated: bool,
    session_path: String,
}

#[derive(Serialize)]
pub struct LocalAccountSettings {
    username: String,
    display_name: String,
    uid: Option<u32>,
    gid: Option<u32>,
    home: String,
    shell: String,
    hostname: String,
    account_type: String,
    admin_groups: Vec<String>,
}

#[derive(Serialize)]
pub struct StorageSettings {
    model_dir: String,
    installer_state_dir: String,
    session_state_dir: String,
    policy_state_dir: String,
    resident_state_dir: String,
    secrets_dir: String,
}

#[derive(Serialize)]
pub struct ServiceSettings {
    bootc_image: String,
    bootc_available: bool,
    systemctl_available: bool,
    network_manager_available: bool,
}

#[derive(Serialize)]
pub struct RecoveryCheck {
    id: &'static str,
    label: &'static str,
    state: RecoveryState,
    detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryState {
    Ready,
    Waiting,
}

pub async fn settings_system() -> Json<SettingsSystemStatus> {
    Json(build_settings_system_status())
}

pub async fn recovery_status() -> Json<RecoveryStatus> {
    Json(build_recovery_status())
}

fn build_settings_system_status() -> SettingsSystemStatus {
    SettingsSystemStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        session: SessionSettings {
            desktop: env_string("GOBLINS_OS_SESSION", "unconfigured"),
            gui_platform: env_string("GOBLINS_OS_GUI_PLATFORM", "unconfigured"),
            shell_mode: env_string("GOBLINS_OS_SHELL_MODE", "unconfigured"),
        },
        identity: IdentitySettings {
            provider_configured: openai_auth_provider_configured(),
            account_authenticated: openai_account_authenticated(),
            session_path: env_string(
                "OPENAI_ACCOUNT_SESSION_PATH",
                "/var/lib/goblins-os/secrets/openai/session.json",
            ),
        },
        local_account: current_local_account_settings(),
        storage: StorageSettings {
            model_dir: env_string("GOBLINS_OS_MODEL_DIR", "/var/lib/goblins-os/models"),
            installer_state_dir: env_string(
                "GOBLINS_OS_INSTALLER_STATE",
                "/var/lib/goblins-os/installer",
            ),
            session_state_dir: env_string(
                "GOBLINS_OS_SESSION_STATE",
                "/var/lib/goblins-os/session",
            ),
            policy_state_dir: env_string("GOBLINS_OS_POLICY_STATE", "/var/lib/goblins-os/policy"),
            resident_state_dir: env_string(
                "GOBLINS_OS_RESIDENT_STATE",
                "/var/lib/goblins-os/resident",
            ),
            secrets_dir: "/var/lib/goblins-os/secrets/openai".to_string(),
        },
        services: ServiceSettings {
            bootc_image: env_string("GOBLINS_OS_BOOTC_IMAGE", "unconfigured"),
            bootc_available: executable_exists("bootc"),
            systemctl_available: executable_exists("systemctl"),
            network_manager_available: executable_exists("NetworkManager")
                || executable_exists("nmcli"),
        },
    }
}

fn current_local_account_settings() -> LocalAccountSettings {
    let username = env::var("USER")
        .ok()
        .or_else(|| env::var("LOGNAME").ok())
        .map(|user| user.trim().to_string())
        .filter(|user| !user.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let home = env::var("HOME").ok();
    let hostname = env::var("HOSTNAME")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            fs::read_to_string("/etc/hostname")
                .ok()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| "Unknown computer".to_string());
    let passwd = fs::read_to_string("/etc/passwd").unwrap_or_default();
    let groups = fs::read_to_string("/etc/group").unwrap_or_default();

    local_account_settings_from_records(&username, &passwd, &groups, home.as_deref(), &hostname)
}

fn local_account_settings_from_records(
    username: &str,
    passwd: &str,
    groups: &str,
    home_override: Option<&str>,
    hostname: &str,
) -> LocalAccountSettings {
    let username = username.trim();
    let username = if username.is_empty() {
        "unknown"
    } else {
        username
    };
    let passwd_account = passwd_account_for_user(passwd, username);
    let uid = passwd_account.as_ref().map(|account| account.uid);
    let gid = passwd_account.as_ref().map(|account| account.gid);
    let admin_groups = admin_groups_for_user(groups, username, gid);
    let display_name = passwd_account
        .as_ref()
        .map(|account| display_name_from_gecos(&account.gecos, username))
        .unwrap_or_else(|| username.to_string());
    let home = passwd_account
        .as_ref()
        .map(|account| account.home.clone())
        .or_else(|| home_override.map(str::to_string))
        .filter(|home| !home.trim().is_empty())
        .unwrap_or_else(|| "Unknown home folder".to_string());
    let shell = passwd_account
        .as_ref()
        .map(|account| account.shell.clone())
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or_else(|| "Unknown login shell".to_string());
    let hostname = if hostname.trim().is_empty() {
        "Unknown computer".to_string()
    } else {
        hostname.trim().to_string()
    };
    let account_type = local_account_type(uid, &admin_groups).to_string();

    LocalAccountSettings {
        username: username.to_string(),
        display_name,
        uid,
        gid,
        home,
        shell,
        hostname,
        account_type,
        admin_groups,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PasswdAccount {
    uid: u32,
    gid: u32,
    gecos: String,
    home: String,
    shell: String,
}

fn passwd_account_for_user(passwd: &str, username: &str) -> Option<PasswdAccount> {
    let username = username.trim();
    if username.is_empty() {
        return None;
    }

    passwd.lines().find_map(|line| {
        let mut parts = line.splitn(7, ':');
        let account_name = parts.next()?.trim();
        if account_name != username {
            return None;
        }
        let _password_placeholder = parts.next()?;
        let uid = parts.next()?.parse::<u32>().ok()?;
        let gid = parts.next()?.parse::<u32>().ok()?;
        let gecos = parts.next()?.to_string();
        let home = parts.next()?.to_string();
        let shell = parts.next()?.to_string();
        Some(PasswdAccount {
            uid,
            gid,
            gecos,
            home,
            shell,
        })
    })
}

fn admin_groups_for_user(groups: &str, username: &str, primary_gid: Option<u32>) -> Vec<String> {
    const ADMIN_GROUPS: &[&str] = &["wheel", "sudo", "admin"];

    ADMIN_GROUPS
        .iter()
        .filter_map(|expected_group| {
            let member = groups.lines().any(|line| {
                let mut parts = line.splitn(4, ':');
                let group_name = parts.next().unwrap_or("").trim();
                let _password_placeholder = parts.next();
                let gid = parts.next().and_then(|value| value.parse::<u32>().ok());
                let members = parts.next().unwrap_or("");
                group_name == *expected_group
                    && (primary_gid.is_some_and(|primary| Some(primary) == gid)
                        || members.split(',').any(|member| member.trim() == username))
            });
            member.then(|| (*expected_group).to_string())
        })
        .collect()
}

fn display_name_from_gecos(gecos: &str, username: &str) -> String {
    let display_name = gecos.split(',').next().unwrap_or("").trim();
    if display_name.is_empty() {
        username.to_string()
    } else {
        display_name.to_string()
    }
}

fn local_account_type(uid: Option<u32>, admin_groups: &[String]) -> &'static str {
    if uid == Some(0) || !admin_groups.is_empty() {
        "Administrator"
    } else if uid.is_some() {
        "Standard"
    } else {
        "Unknown"
    }
}

fn build_recovery_status() -> RecoveryStatus {
    let settings = build_settings_system_status();
    let image = system_image_summary();
    let model_dir = PathBuf::from(&settings.storage.model_dir);
    let installer_state = PathBuf::from(&settings.storage.installer_state_dir);
    let session_state = PathBuf::from(&settings.storage.session_state_dir);
    let policy_state = PathBuf::from(&settings.storage.policy_state_dir);
    let resident_state = PathBuf::from(&settings.storage.resident_state_dir);
    let secrets_dir = PathBuf::from(&settings.storage.secrets_dir);
    let verifier = PathBuf::from("/usr/libexec/goblins-os/goblins-os-verify");

    RecoveryStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        checks: vec![
            RecoveryCheck {
                id: "bootc-tooling",
                label: "System image",
                // Honest readiness: green only when `bootc status` actually
                // reported a booted deployment, not merely because a `bootc`
                // binary exists on PATH.
                state: if image.reportable {
                    RecoveryState::Ready
                } else {
                    RecoveryState::Waiting
                },
                detail: if image.reportable {
                    if image.rollback_available {
                        "The system image service reports the running system image and a rollback image for this device."
                            .to_string()
                    } else {
                        "The system image service reports the running system image for this device; no rollback image is recorded yet."
                            .to_string()
                    }
                } else if settings.services.bootc_available {
                    "System image tooling is present, but the system image service could not report deployment status in this session, so image status and rollback are not confirmed here."
                        .to_string()
                } else {
                    "System image checks will be available from the installed OS environment.".to_string()
                },
            },
            RecoveryCheck {
                id: "system-services",
                label: "Recovery services",
                state: if settings.services.systemctl_available {
                    RecoveryState::Ready
                } else {
                    RecoveryState::Waiting
                },
                detail: if settings.services.systemctl_available {
                    "Recovery services are ready for Goblins OS health checks.".to_string()
                } else {
                    "Recovery service checks will be available from the installed OS environment.".to_string()
                },
            },
            path_check(
                "model-cache",
                "Model cache",
                &model_dir,
                "Local gpt-oss model downloads stay outside the immutable OS image.",
            ),
            path_check(
                "installer-state",
                "First-boot state",
                &installer_state,
                "First-boot completion state is stored privately on this device.",
            ),
            path_check(
                "session-gate",
                "Session gate state",
                &session_state,
                "Desktop unlock state is owned by the native Goblins OS login gate.",
            ),
            path_check(
                "policy-state",
                "Policy state",
                &policy_state,
                "Consumer, business, enterprise, and local-only policy profiles are stored privately on this device.",
            ),
            path_check(
                "resident-state",
                "Goblins AI runtime state",
                &resident_state,
                "Persistent Goblins AI runtime state is stored outside the immutable OS image.",
            ),
            path_check(
                "secret-storage",
                "OpenAI secret storage",
                &secrets_dir,
                "OpenAI account and service tokens stay in OS-owned secret storage.",
            ),
            path_check(
                "os-packaging-verifier",
                "OS packaging verifier",
                &verifier,
                "The installed system includes a verifier for image, session, recovery, and desktop packaging health.",
            ),
        ]
        .into_iter()
        .chain(
            system_service_recovery_checks()
                .into_iter()
                .map(|service| RecoveryCheck {
                    id: service.id,
                    label: service.label,
                    state: if service.ready {
                        RecoveryState::Ready
                    } else {
                        RecoveryState::Waiting
                    },
                    detail: service.detail,
                }),
        )
        .chain(recovery_facility_checks().into_iter().map(|facility| RecoveryCheck {
            id: facility.id,
            label: facility.label,
            state: match facility.state {
                FacilityState::Ready => RecoveryState::Ready,
                FacilityState::Waiting => RecoveryState::Waiting,
            },
            detail: facility.detail,
        }))
        .collect(),
    }
}

fn path_check(
    id: &'static str,
    label: &'static str,
    path: &Path,
    detail: &'static str,
) -> RecoveryCheck {
    let exists = path.exists();
    RecoveryCheck {
        id,
        label,
        state: if exists {
            RecoveryState::Ready
        } else {
            RecoveryState::Waiting
        },
        detail: if exists {
            format!("{} Path: {}.", detail, path.display())
        } else {
            format!("Waiting for {}. Path: {}.", label, path.display())
        },
    }
}

fn env_string(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn executable_exists(binary: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| {
        let candidate = path.join(binary);
        fs::metadata(candidate)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_recovery_status, build_settings_system_status, local_account_settings_from_records,
    };

    #[test]
    fn settings_status_uses_goblins_os_defaults() {
        let status = build_settings_system_status();

        assert_eq!(status.source, "goblins-os-core");
        assert!(status.storage.model_dir.contains("/var/lib/goblins-os"));
        assert!(status.identity.session_path.contains("/var/lib/goblins-os"));
        assert!(!status.local_account.username.trim().is_empty());
    }

    #[test]
    fn local_account_summary_reports_real_os_identity_without_fake_admin_state() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\njoseph:x:1000:1000:Joseph Simo,,,:/home/joseph:/usr/bin/zsh\n";
        let groups = "wheel:x:10:joseph\nsudo:x:27:\njoseph:x:1000:\n";
        let account = local_account_settings_from_records(
            "joseph",
            passwd,
            groups,
            Some("/fallback-home"),
            "goblins-workstation",
        );

        assert_eq!(account.username, "joseph");
        assert_eq!(account.display_name, "Joseph Simo");
        assert_eq!(account.uid, Some(1000));
        assert_eq!(account.gid, Some(1000));
        assert_eq!(account.home, "/home/joseph");
        assert_eq!(account.shell, "/usr/bin/zsh");
        assert_eq!(account.hostname, "goblins-workstation");
        assert_eq!(account.account_type, "Administrator");
        assert_eq!(account.admin_groups, vec!["wheel"]);
    }

    #[test]
    fn local_account_summary_falls_back_truthfully_when_passwd_entry_is_missing() {
        let account =
            local_account_settings_from_records("", "", "", Some("/home/session-user"), "");

        assert_eq!(account.username, "unknown");
        assert_eq!(account.display_name, "unknown");
        assert_eq!(account.uid, None);
        assert_eq!(account.gid, None);
        assert_eq!(account.home, "/home/session-user");
        assert_eq!(account.shell, "Unknown login shell");
        assert_eq!(account.hostname, "Unknown computer");
        assert_eq!(account.account_type, "Unknown");
        assert!(account.admin_groups.is_empty());
    }

    #[test]
    fn recovery_status_reports_real_checks() {
        let status = build_recovery_status();

        assert_eq!(status.source, "goblins-os-core");
        assert!(status.checks.iter().any(|check| check.id == "model-cache"));
        assert!(status
            .checks
            .iter()
            .any(|check| check.id == "secret-storage"));
        assert!(status
            .checks
            .iter()
            .any(|check| check.id == "os-core-service"));
        assert!(status
            .checks
            .iter()
            .any(|check| check.id == "desktop-portals"));
    }
}
