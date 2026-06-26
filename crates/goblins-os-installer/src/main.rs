use std::{
    env,
    error::Error,
    fmt,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    thread,
    time::{Duration, Instant},
};

// Rc/RefCell back the install-flow widgets, which only exist in the native desktop build.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use std::{cell::RefCell, rc::Rc};

use serde::Deserialize;

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use goblins_os_ui::status_pill;

const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";
const DEFAULT_CORE_WAIT_SECS: u64 = 60;
const MAX_CORE_BODY_BYTES: usize = 1024 * 1024;

type InstallerResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone)]
struct InstallerConfig {
    core_url: String,
    core_wait: Duration,
}

#[derive(Clone, Copy)]
struct BootState {
    core_ready: bool,
}

#[derive(Clone)]
struct InstallerState {
    boot: BootState,
    auth: Option<AuthStatus>,
    network: Option<NetworkStatus>,
    readiness: Option<InstallerReadiness>,
    install_targets: Option<InstallTargetStatus>,
    local_models: Option<LocalModelCatalog>,
    services: Vec<ServiceCatalogEntry>,
}

#[derive(Clone, Deserialize)]
struct AuthStatus {
    configured: bool,
    authenticated: bool,
    provider: String,
    session_storage: String,
    message: String,
}

/// Connectivity as reported by the OS core (NetworkManager-backed). The installer
/// mirrors status only; Wi-Fi scans and joins go straight back to the core.
#[derive(Clone, Deserialize)]
struct NetworkStatus {
    manager_available: bool,
    online: bool,
    connectivity: String,
    detail: String,
    active: Option<ActiveConnection>,
}

#[derive(Clone, Deserialize)]
struct ActiveConnection {
    name: String,
    kind: String,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Deserialize)]
struct WifiScan {
    manager_available: bool,
    networks: Vec<WifiNetwork>,
    detail: String,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Deserialize)]
struct WifiNetwork {
    ssid: String,
    signal: u8,
    security: String,
    #[serde(default)]
    in_use: bool,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Deserialize)]
struct WifiConnectOutcome {
    text: String,
}

#[derive(Clone, Deserialize)]
struct InstallerReadiness {
    source: String,
    first_boot: FirstBootState,
    profile: InstallerProfile,
    stages: Vec<InstallerStage>,
    privacy_note: String,
    storage_note: String,
}

#[derive(Clone, Deserialize)]
struct FirstBootState {
    completed: bool,
    state_path: String,
}

#[derive(Clone, Deserialize)]
struct InstallerProfile {
    default_mode: String,
    cloud_requires_openai_account: bool,
    local_requires_cloud_login: bool,
}

#[derive(Clone, Deserialize)]
struct InstallerStage {
    id: String,
    index: String,
    label: String,
    state: String,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct ServiceCatalog {
    services: Vec<ServiceCatalogEntry>,
}

#[derive(Clone, Deserialize)]
struct ServiceCatalogEntry {
    id: String,
    name: String,
    role: String,
    launch: String,
    status: String,
}

#[derive(Clone, Deserialize)]
struct LocalModelCatalog {
    install_policy: String,
    hardware: HardwareReport,
    models: Vec<LocalModelOption>,
}

#[derive(Clone, Deserialize)]
struct HardwareReport {
    ram_gb: u64,
    gpu_vram_gb: Option<u64>,
    model_dir: String,
    model_dir_available_gb: Option<u64>,
    runtime: RuntimeReport,
}

#[derive(Clone, Deserialize)]
struct RuntimeReport {
    selected: Option<String>,
    ollama: bool,
    vllm: bool,
    lm_studio: bool,
}

#[derive(Clone, Deserialize)]
struct LocalModelOption {
    id: String,
    name: String,
    role: String,
    source: String,
    weights_in_os_image: bool,
    download_required: bool,
    minimum_ram_gb: u64,
    minimum_gpu_vram_gb: Option<u64>,
    disk_requirement: String,
    state: String,
    reasons: Vec<String>,
    install: LocalModelInstall,
}

#[derive(Clone, Deserialize)]
struct LocalModelInstall {
    state: String,
    consent_required: bool,
    consent_recorded: bool,
    manifest_required: bool,
    verification_required: bool,
    resumable: bool,
    state_path: String,
    target_dir: String,
    manifest_path: String,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct InstallTargetStatus {
    #[serde(default)]
    environment: InstallEnvironment,
    #[serde(default)]
    boot_entries: BootEntryStatus,
    bootc: BootcInstallStatus,
    policy: InstallPolicy,
    targets: Vec<InstallTarget>,
}

#[derive(Clone, Deserialize, Default)]
struct InstallEnvironment {
    architecture: String,
    supported_architectures: Vec<String>,
    native_supported: bool,
    boot_mode: String,
    efi_available: bool,
    secure_boot: SecureBootStatus,
    architecture_guidance: String,
    boot_guidance: String,
}

#[derive(Clone, Deserialize, Default)]
struct SecureBootStatus {
    state: String,
    detail: String,
}

#[derive(Clone, Deserialize, Default)]
struct BootEntryStatus {
    available: bool,
    entries: Vec<BootEntry>,
    detail: String,
    guidance: String,
}

#[derive(Clone, Deserialize)]
struct BootEntry {
    id: String,
    label: String,
    #[serde(default)]
    #[cfg_attr(
        not(all(target_os = "linux", feature = "native-desktop")),
        allow(dead_code)
    )]
    target: String,
    active: bool,
}

#[derive(Clone, Deserialize)]
struct BootcInstallStatus {
    available: bool,
    privileged: bool,
    image: String,
    install_config_path: String,
    default_filesystem: String,
    command_model: String,
}

#[derive(Clone, Deserialize)]
struct InstallPolicy {
    destructive_acknowledgement: String,
    execute_env_gate: String,
    storage_layout: String,
    #[serde(default)]
    simple_install_scope: String,
    #[serde(default)]
    formatting_guidance: String,
    bootloader: String,
    #[serde(default)]
    bootloader_recovery: String,
    #[serde(default)]
    advanced_storage_guidance: String,
    #[serde(default)]
    install_path_options: Vec<InstallPathOption>,
    #[serde(default)]
    pre_install_safety: Vec<InstallPlanItem>,
    #[serde(default)]
    pre_write_install_plan: Vec<InstallPlanItem>,
    #[serde(default)]
    dual_boot_preflight: String,
    dual_boot_guidance: String,
    #[serde(default)]
    dual_boot_preservation: String,
    #[serde(default)]
    dual_boot_handoff: String,
    #[serde(default)]
    dual_boot_safe_route: DualBootSafeRoute,
    #[serde(default)]
    full_storage_installer: FullStorageInstallerHandoff,
    #[serde(default)]
    dual_boot_quick_start: Vec<InstallPlanItem>,
    #[serde(default)]
    dual_boot_readiness: Vec<DualBootReadinessItem>,
    #[serde(default)]
    dual_boot_choices: Vec<DualBootChoice>,
    #[serde(default)]
    dual_boot_guide: Vec<DualBootGuideStep>,
    #[serde(default)]
    dual_boot_decision_map: Vec<DualBootDecision>,
    #[serde(default)]
    storage_review_checklist: Vec<StorageReviewItem>,
    #[serde(default)]
    post_install_verification: Vec<InstallPlanItem>,
    local_model_weights: String,
}

#[derive(Clone, Deserialize)]
struct InstallPathOption {
    title: String,
    summary: String,
    action: String,
    safety: String,
}

#[derive(Clone, Deserialize)]
struct InstallPlanItem {
    title: String,
    detail: String,
}

#[derive(Clone, Deserialize, Default)]
struct DualBootSafeRoute {
    title: String,
    summary: String,
    primary_action: String,
    first_screen: String,
    target_rule: String,
    preserve_rule: String,
    final_review: String,
    after_install: String,
}

#[derive(Clone, Deserialize, Default)]
struct FullStorageInstallerHandoff {
    title: String,
    summary: String,
    action_label: String,
    command: String,
    desktop_id: String,
    storage_entry: String,
    safest_for: String,
    final_check: String,
}

#[derive(Clone, Deserialize)]
struct DualBootReadinessItem {
    title: String,
    before_install: String,
    installer_choice: String,
    final_check: String,
}

#[derive(Clone, Deserialize)]
struct DualBootChoice {
    title: String,
    preparation: String,
    install_target: String,
    preserve: String,
    finish: String,
}

#[derive(Clone, Deserialize)]
struct DualBootGuideStep {
    title: String,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct DualBootDecision {
    title: String,
    best_for: String,
    prepare_space: String,
    install_target: String,
    preserve: String,
    boot_picker: String,
}

#[derive(Clone, Deserialize)]
struct StorageReviewItem {
    title: String,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct ExistingSystem {
    kind: String,
    partition: String,
    detail: String,
    preservation: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize, Default)]
struct InstallRecommendation {
    title: String,
    action: String,
    install_target: String,
    preserve: String,
    finish: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize, Default)]
struct DualBootPlan {
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

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct InstallTarget {
    path: String,
    model: String,
    size_gb: u64,
    removable: bool,
    rotational: bool,
    mounted: bool,
    partitions: Vec<String>,
    #[serde(default)]
    existing_systems: Vec<ExistingSystem>,
    #[serde(default)]
    recommendation: InstallRecommendation,
    #[serde(default)]
    dual_boot_plan: DualBootPlan,
    eligible: bool,
    reasons: Vec<String>,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct PrepareInstallResponse {
    state: String,
    command: Vec<String>,
    detail: String,
}

/// Live progress of a running `bootc install`, mirrored from the OS core. `state`
/// is one of idle/running/succeeded/failed; `phase` is the latest real line bootc
/// printed — the installer only ever displays this verbatim, never a fabricated
/// percentage. Only the native install flow consumes it.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Deserialize)]
struct InstallProgress {
    state: String,
    phase: String,
}

#[derive(Debug, PartialEq, Eq)]
struct HttpEndpoint {
    host: String,
    port: u16,
}

#[derive(Debug, PartialEq, Eq)]
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
enum CoreFetchError {
    Status(u16),
    Malformed,
    Transport,
    Decode,
}

impl fmt::Display for CoreFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) => write!(formatter, "core returned HTTP {status}"),
            Self::Malformed => formatter.write_str("core response was malformed"),
            Self::Transport => formatter.write_str("core connection failed"),
            Self::Decode => formatter.write_str("core response JSON did not match the OS contract"),
        }
    }
}

fn main() -> InstallerResult<()> {
    let config = InstallerConfig::from_env();
    let boot = inspect_boot_state(&config);
    let state = load_installer_state(&config, boot);

    println!("Goblins OS native first-boot installer started");
    println!("core={}", config.core_url);
    println!("installer_mode=native-desktop");
    println!("{}", installer_state_summary(&state));

    if state
        .readiness
        .as_ref()
        .is_some_and(|readiness| readiness.first_boot.completed)
    {
        println!("first_boot_installer=complete");
        return Ok(());
    }

    run_native_installer(config, state)
}

impl InstallerConfig {
    fn from_env() -> Self {
        Self {
            core_url: env::var("GOBLINS_OS_CORE_URL")
                .or_else(|_| env::var("OPENAI_OS_CORE_URL"))
                .unwrap_or_else(|_| DEFAULT_CORE_URL.into()),
            core_wait: Duration::from_secs(env_u64(
                "GOBLINS_OS_INSTALLER_CORE_WAIT_SECS",
                DEFAULT_CORE_WAIT_SECS,
            )),
        }
    }
}

fn inspect_boot_state(config: &InstallerConfig) -> BootState {
    BootState {
        core_ready: wait_for_core(&config.core_url, config.core_wait),
    }
}

fn load_installer_state(config: &InstallerConfig, boot: BootState) -> InstallerState {
    if !boot.core_ready {
        return InstallerState {
            boot,
            auth: None,
            network: None,
            readiness: None,
            install_targets: None,
            local_models: None,
            services: Vec::new(),
        };
    }

    let auth = get_core_json::<AuthStatus>(&config.core_url, "/v1/auth/openai/status").ok();
    let network = get_core_json::<NetworkStatus>(&config.core_url, "/v1/network/status").ok();
    let readiness =
        get_core_json::<InstallerReadiness>(&config.core_url, "/v1/installer/readiness").ok();
    let install_targets =
        get_core_json::<InstallTargetStatus>(&config.core_url, "/v1/installer/install-targets")
            .ok();
    let local_models =
        get_core_json::<LocalModelCatalog>(&config.core_url, "/v1/local-models").ok();
    let services = get_core_json::<ServiceCatalog>(&config.core_url, "/v1/services")
        .map(|catalog| catalog.services)
        .unwrap_or_default();

    InstallerState {
        boot,
        auth,
        network,
        readiness,
        install_targets,
        local_models,
        services,
    }
}

fn installer_state_summary(state: &InstallerState) -> String {
    let auth = state
        .auth
        .as_ref()
        .map(|auth| {
            format!(
                "{}:{}:{}:{}",
                auth.provider,
                if auth.authenticated {
                    "authenticated"
                } else if auth.configured {
                    "provider-ready"
                } else {
                    "locked"
                },
                auth.session_storage,
                auth.message
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    let readiness = state
        .readiness
        .as_ref()
        .map(|readiness| {
            let first_stage = readiness
                .stages
                .first()
                .map(|stage| {
                    format!(
                        "{}:{}:{}:{}:{}",
                        stage.index, stage.id, stage.label, stage.state, stage.detail
                    )
                })
                .unwrap_or_else(|| "none".to_string());

            format!(
                "{} complete={} state={} profile={}:cloud-required={}:local-login={} first-stage={} privacy={} storage={}",
                readiness.source,
                readiness.first_boot.completed,
                readiness.first_boot.state_path,
                readiness.profile.default_mode,
                readiness.profile.cloud_requires_openai_account,
                readiness.profile.local_requires_cloud_login,
                first_stage,
                readiness.privacy_note,
                readiness.storage_note
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    let local_models = state
        .local_models
        .as_ref()
        .map(|catalog| {
            let first_model = catalog
                .models
                .first()
                .map(local_model_summary)
                .unwrap_or_else(|| "none".to_string());

            format!(
                "{} hardware={}GB vram={} storage={} model-dir={} runtime={} first-model={}",
                catalog.install_policy,
                catalog.hardware.ram_gb,
                catalog
                    .hardware
                    .gpu_vram_gb
                    .map(|vram| format!("{vram}GB"))
                    .unwrap_or_else(|| "not-detected".to_string()),
                catalog
                    .hardware
                    .model_dir_available_gb
                    .map(|gb| format!("{gb}GB"))
                    .unwrap_or_else(|| "unknown".to_string()),
                catalog.hardware.model_dir,
                runtime_label(&catalog.hardware.runtime),
                first_model
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let install_targets = state
        .install_targets
        .as_ref()
        .map(|status| {
            let first = status
                .targets
                .first()
                .map(install_target_summary)
                .unwrap_or_else(|| "none".to_string());
            format!(
                "environment=[{}] boot_entries=[{}] bootc={} privileged={} image={} config={} fs={} command={} policy={} simple_scope={} formatting={} bootloader={} bootloader_recovery={} advanced_storage={} install_paths={} pre_install_safety={} pre_write_plan={} dual_boot_preflight={} dual_boot={} preserve_existing_os={} dual_boot_handoff={} dual_boot_safe_route={} full_storage_installer={} dual_boot_quick_start={} dual_boot_readiness={} dual_boot_choices={} dual_boot_guide={} dual_boot_decision_map={} storage_review={} post_install_verification={} destructive_ack={} gate={} weights={} targets={} first=[{}]",
                install_environment_summary(&status.environment),
                boot_entry_status_summary(&status.boot_entries),
                status.bootc.available,
                status.bootc.privileged,
                status.bootc.image,
                status.bootc.install_config_path,
                status.bootc.default_filesystem,
                status.bootc.command_model,
                status.policy.storage_layout,
                status.policy.simple_install_scope,
                status.policy.formatting_guidance,
                status.policy.bootloader,
                status.policy.bootloader_recovery,
                status.policy.advanced_storage_guidance,
                install_path_options_summary(&status.policy),
                pre_install_safety_summary(&status.policy),
                pre_write_install_plan_summary(&status.policy),
                status.policy.dual_boot_preflight,
                status.policy.dual_boot_guidance,
                status.policy.dual_boot_preservation,
                status.policy.dual_boot_handoff,
                dual_boot_safe_route_summary(&status.policy),
                full_storage_installer_summary(&status.policy),
                dual_boot_quick_start_summary(&status.policy),
                dual_boot_readiness_summary(&status.policy),
                dual_boot_choices_summary(&status.policy),
                dual_boot_guide_summary(&status.policy),
                dual_boot_decision_map_summary(&status.policy),
                storage_review_checklist_summary(&status.policy),
                post_install_verification_summary(&status.policy),
                status.policy.destructive_acknowledgement,
                status.policy.execute_env_gate,
                status.policy.local_model_weights,
                status.targets.len(),
                first
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    let first_service = state
        .services
        .first()
        .map(|service| {
            format!(
                "{}:{}:{}:{}:{}",
                service.id, service.name, service.role, service.launch, service.status
            )
        })
        .unwrap_or_else(|| "none".to_string());

    let network = state
        .network
        .as_ref()
        .map(|network| {
            format!(
                "manager={} online={} connectivity={} active={} detail={}",
                network.manager_available,
                network.online,
                network.connectivity,
                network
                    .active
                    .as_ref()
                    .map(|active| format!("{}:{}", active.name, active.kind))
                    .unwrap_or_else(|| "none".to_string()),
                network.detail
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    format!(
        "installer_state=core:{} auth=[{}] network=[{}] readiness=[{}] install_targets=[{}] local_models=[{}] services={} first_service=[{}]",
        if state.boot.core_ready {
            "ready"
        } else {
            "waiting"
        },
        auth,
        network,
        readiness,
        install_targets,
        local_models,
        state.services.len(),
        first_service
    )
}

fn pre_write_install_plan_summary(policy: &InstallPolicy) -> String {
    if policy.pre_write_install_plan.is_empty() {
        return "none".to_string();
    }

    policy
        .pre_write_install_plan
        .iter()
        .map(|item| format!("{}:{}", item.title, item.detail))
        .collect::<Vec<_>>()
        .join("|")
}

fn dual_boot_safe_route_summary(policy: &InstallPolicy) -> String {
    if policy.dual_boot_safe_route.title.is_empty() {
        return "none".to_string();
    }

    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        policy.dual_boot_safe_route.title,
        policy.dual_boot_safe_route.summary,
        policy.dual_boot_safe_route.primary_action,
        policy.dual_boot_safe_route.first_screen,
        policy.dual_boot_safe_route.target_rule,
        policy.dual_boot_safe_route.preserve_rule,
        policy.dual_boot_safe_route.final_review,
        policy.dual_boot_safe_route.after_install
    )
}

fn full_storage_installer_summary(policy: &InstallPolicy) -> String {
    if policy.full_storage_installer.title.is_empty() {
        return "none".to_string();
    }

    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        policy.full_storage_installer.title,
        policy.full_storage_installer.summary,
        policy.full_storage_installer.action_label,
        policy.full_storage_installer.command,
        policy.full_storage_installer.desktop_id,
        policy.full_storage_installer.storage_entry,
        policy.full_storage_installer.safest_for,
        policy.full_storage_installer.final_check
    )
}

fn dual_boot_quick_start_summary(policy: &InstallPolicy) -> String {
    if policy.dual_boot_quick_start.is_empty() {
        return "none".to_string();
    }

    policy
        .dual_boot_quick_start
        .iter()
        .map(|item| format!("{}:{}", item.title, item.detail))
        .collect::<Vec<_>>()
        .join("|")
}

fn dual_boot_readiness_summary(policy: &InstallPolicy) -> String {
    if policy.dual_boot_readiness.is_empty() {
        return "none".to_string();
    }

    policy
        .dual_boot_readiness
        .iter()
        .map(|item| {
            format!(
                "{}:{}:{}:{}",
                item.title, item.before_install, item.installer_choice, item.final_check
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn dual_boot_choices_summary(policy: &InstallPolicy) -> String {
    if policy.dual_boot_choices.is_empty() {
        return "none".to_string();
    }

    policy
        .dual_boot_choices
        .iter()
        .map(|choice| {
            format!(
                "{}:{}:{}:{}:{}",
                choice.title,
                choice.preparation,
                choice.install_target,
                choice.preserve,
                choice.finish
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn dual_boot_guide_summary(policy: &InstallPolicy) -> String {
    if policy.dual_boot_guide.is_empty() {
        return "none".to_string();
    }

    policy
        .dual_boot_guide
        .iter()
        .map(|step| format!("{}:{}", step.title, step.detail))
        .collect::<Vec<_>>()
        .join("|")
}

fn dual_boot_decision_map_summary(policy: &InstallPolicy) -> String {
    if policy.dual_boot_decision_map.is_empty() {
        return "none".to_string();
    }

    policy
        .dual_boot_decision_map
        .iter()
        .map(|decision| {
            format!(
                "{}:{}:{}:{}:{}:{}",
                decision.title,
                decision.best_for,
                decision.prepare_space,
                decision.install_target,
                decision.preserve,
                decision.boot_picker
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn install_path_options_summary(policy: &InstallPolicy) -> String {
    if policy.install_path_options.is_empty() {
        return "none".to_string();
    }

    policy
        .install_path_options
        .iter()
        .map(|option| {
            format!(
                "{}:{}:{}:{}",
                option.title, option.summary, option.action, option.safety
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn pre_install_safety_summary(policy: &InstallPolicy) -> String {
    if policy.pre_install_safety.is_empty() {
        return "none".to_string();
    }

    policy
        .pre_install_safety
        .iter()
        .map(|item| format!("{}:{}", item.title, item.detail))
        .collect::<Vec<_>>()
        .join("|")
}

fn storage_review_checklist_summary(policy: &InstallPolicy) -> String {
    if policy.storage_review_checklist.is_empty() {
        return "none".to_string();
    }

    policy
        .storage_review_checklist
        .iter()
        .map(|item| format!("{}:{}", item.title, item.detail))
        .collect::<Vec<_>>()
        .join("|")
}

fn post_install_verification_summary(policy: &InstallPolicy) -> String {
    if policy.post_install_verification.is_empty() {
        return "none".to_string();
    }

    policy
        .post_install_verification
        .iter()
        .map(|item| format!("{}:{}", item.title, item.detail))
        .collect::<Vec<_>>()
        .join("|")
}

fn install_environment_summary(environment: &InstallEnvironment) -> String {
    let supported = if environment.supported_architectures.is_empty() {
        "x86_64,aarch64".to_string()
    } else {
        environment.supported_architectures.join(",")
    };
    let architecture = if environment.architecture.is_empty() {
        "unknown"
    } else {
        environment.architecture.as_str()
    };
    let boot_mode = if environment.boot_mode.is_empty() {
        "unknown"
    } else {
        environment.boot_mode.as_str()
    };
    let secure_boot = if environment.secure_boot.state.is_empty() {
        "unknown"
    } else {
        environment.secure_boot.state.as_str()
    };

    format!(
        "arch={} native_supported={} supported={} boot_mode={} efi_available={} secure_boot={} architecture_guidance={} boot_guidance={} secure_boot_detail={}",
        architecture,
        environment.native_supported,
        supported,
        boot_mode,
        environment.efi_available,
        secure_boot,
        environment.architecture_guidance,
        environment.boot_guidance,
        environment.secure_boot.detail
    )
}

fn boot_entry_status_summary(status: &BootEntryStatus) -> String {
    let entries = if status.entries.is_empty() {
        "none".to_string()
    } else {
        status
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "{}{}:{}",
                    entry.id,
                    if entry.active { "*" } else { "" },
                    entry.label
                )
            })
            .collect::<Vec<_>>()
            .join("|")
    };

    format!(
        "available={} entries={} detail={} guidance={}",
        status.available, entries, status.detail, status.guidance
    )
}

fn install_target_summary(target: &InstallTarget) -> String {
    format!(
        "{}:{}:{}GB removable={} rotational={} mounted={} partitions={} existing_systems={} dual_boot_plan={} eligible={} reason={}",
        target.path,
        target.model,
        target.size_gb,
        target.removable,
        target.rotational,
        target.mounted,
        target.partitions.join("|"),
        install_target_existing_systems_summary(target),
        install_target_dual_boot_plan_summary(target),
        target.eligible,
        target
            .reasons
            .first()
            .map(String::as_str)
            .unwrap_or("ready")
    )
}

fn install_target_existing_systems_summary(target: &InstallTarget) -> String {
    if target.existing_systems.is_empty() {
        return "none".to_string();
    }

    target
        .existing_systems
        .iter()
        .map(|system| {
            format!(
                "{}:{}:{}:{}",
                system.kind, system.partition, system.detail, system.preservation
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn install_target_dual_boot_plan_summary(target: &InstallTarget) -> String {
    if target.dual_boot_plan.title.is_empty() {
        return "none".to_string();
    }

    format!(
        "{}:{}:{}:{}:{}:{}:{}",
        target.dual_boot_plan.status,
        target.dual_boot_plan.title,
        target.dual_boot_plan.summary,
        target.dual_boot_plan.primary_action,
        target.dual_boot_plan.storage_target,
        target.dual_boot_plan.preserve,
        target.dual_boot_plan.finish
    )
}

fn local_model_summary(model: &LocalModelOption) -> String {
    format!(
        "{}:{}:{}:{} weights-in-image={} download={} min-ram={} min-vram={} disk={} install={} consent-required={} consent={} manifest-required={} verified={} resumable={} state-path={} target={} manifest={} install-detail={} reason={}",
        model.id,
        model.name,
        model.source,
        model.state,
        model.weights_in_os_image,
        model.download_required,
        model.minimum_ram_gb,
        model
            .minimum_gpu_vram_gb
            .map(|vram| format!("{vram}GB"))
            .unwrap_or_else(|| "none".to_string()),
        model.disk_requirement,
        model.install.state,
        model.install.consent_required,
        model.install.consent_recorded,
        model.install.manifest_required,
        model.install.verification_required,
        model.install.resumable,
        model.install.state_path,
        model.install.target_dir,
        model.install.manifest_path,
        model.install.detail,
        model
            .reasons
            .first()
            .map(String::as_str)
            .unwrap_or(model.role.as_str())
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_native_installer(config: InstallerConfig, state: InstallerState) -> InstallerResult<()> {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.Installer")
        .build();

    application.connect_activate(move |app| {
        goblins_os_ui::init_theming(GOBLINS_OS_INSTALLER_CSS);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Goblins OS Setup")
            .decorated(false)
            .default_width(1280)
            .default_height(820)
            .build();

        window.set_child(Some(&build_installer(app, &config, &state)));
        window.fullscreen();
        window.present();
    });

    application.run();
    Ok(())
}

/// The five "Install to this computer" page containers. They are added to the
/// Stack once; the disk page is built from the loaded scan at startup, and the
/// later pages are rebuilt on navigation from the chosen disk. `gtk4::Box` is a
/// GObject handle, so cloning shares the same widget across the page closures.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone)]
struct InstallPages {
    disk: gtk4::Box,
    review: gtk4::Box,
    confirm: gtk4::Box,
    progress: gtk4::Box,
    done: gtk4::Box,
}

/// Cross-screen install state. The GTK main loop is single-threaded, so an
/// `Rc<RefCell<…>>` is the right shared-mutable handle: the disk page writes the
/// chosen target, the confirm page writes the failure detail, and the done page
/// reads both. The app handle lets the done page restart or close.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone)]
struct InstallFlow {
    selected: Rc<RefCell<Option<InstallTarget>>>,
    last_error: Rc<RefCell<Option<String>>>,
    state: Rc<InstallerState>,
    app: gtk4::Application,
}

/// A calm, centered first-boot welcome — GPT-OSS first, account optional. This is
/// the onboarding most users ever see; "Advanced setup" reveals the details.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_installer(
    app: &gtk4::Application,
    config: &InstallerConfig,
    state: &InstallerState,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(220);
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    let flow = InstallFlow {
        selected: Rc::new(RefCell::new(None)),
        last_error: Rc::new(RefCell::new(None)),
        state: Rc::new(state.clone()),
        app: app.clone(),
    };
    let pages = InstallPages {
        disk: gtk::Box::new(gtk::Orientation::Vertical, 0),
        review: gtk::Box::new(gtk::Orientation::Vertical, 0),
        confirm: gtk::Box::new(gtk::Orientation::Vertical, 0),
        progress: gtk::Box::new(gtk::Orientation::Vertical, 0),
        done: gtk::Box::new(gtk::Orientation::Vertical, 0),
    };

    let welcome = build_welcome_page(app, config, state, &stack);
    let appearance = build_appearance_page(&stack);
    let accessibility = build_accessibility_page(&stack);
    let first_app = build_first_app_page(app, config, &stack);
    let network = build_network_page(config, state, &stack);
    let details = build_details_page(app, config, state, &stack);
    stack.add_named(&welcome, Some("welcome"));
    stack.add_named(&appearance, Some("appearance"));
    stack.add_named(&accessibility, Some("accessibility"));
    stack.add_named(&first_app, Some("first-app"));
    stack.add_named(&network, Some("network"));
    stack.add_named(&details, Some("details"));
    stack.add_named(&pages.disk, Some("install-disk"));
    stack.add_named(&pages.review, Some("install-review"));
    stack.add_named(&pages.confirm, Some("install-confirm"));
    stack.add_named(&pages.progress, Some("install-progress"));
    stack.add_named(&pages.done, Some("install-done"));

    let core_url = config.core_url.clone();
    // The disk page is built from the loaded scan (held in `flow.state`); the
    // later install pages are rebuilt on navigation from the chosen disk.
    populate_install_disk(&pages, &stack, &flow, &core_url);

    // Onboarding opens on Welcome; an explicit page may be requested (used by the
    // packaging-time render harness to prove each first-boot screen).
    let initial = match env::var("GOBLINS_OS_INSTALLER_PAGE").ok().as_deref() {
        Some("network") => "network",
        Some("appearance") => "appearance",
        Some("accessibility") => "accessibility",
        Some("first-app") => "first-app",
        Some("details") => "details",
        Some("install-disk") => "install-disk",
        Some("install-review") => "install-review",
        Some("install-confirm") => "install-confirm",
        Some("install-progress") => "install-progress",
        Some("install-done") => "install-done",
        _ => "welcome",
    };

    // A harness-forced capture of a page that needs a chosen disk preselects the
    // first eligible disk and builds that page, so the screenshot is truthful
    // (a real device path) rather than the cold placeholder.
    if matches!(
        initial,
        "install-review" | "install-confirm" | "install-progress" | "install-done"
    ) {
        if flow.selected.borrow().is_none() {
            if let Some(target) = flow.state.install_targets.as_ref().and_then(|status| {
                status
                    .targets
                    .iter()
                    .find(|target| target.eligible)
                    .cloned()
            }) {
                *flow.selected.borrow_mut() = Some(target);
            }
        }
        match initial {
            "install-review" => populate_install_review(&pages, &stack, &flow, &core_url),
            "install-confirm" => populate_install_confirm(&pages, &stack, &flow, &core_url),
            "install-progress" => populate_install_progress(&pages, &stack, &flow, &core_url),
            "install-done" => populate_install_done(&pages, &stack, &flow, &core_url),
            _ => {}
        }
    }

    stack.set_visible_child_name(initial);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&stack);
    root
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn centered_label(text: &str, class: &str, wrap: bool) -> gtk4::Label {
    use gtk4::prelude::*;

    let label = gtk4::Label::new(Some(text));
    label.set_justify(gtk4::Justification::Center);
    label.set_halign(gtk4::Align::Center);
    label.set_wrap(wrap);
    if wrap {
        label.set_max_width_chars(46);
    }
    label.add_css_class(class);
    label
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_welcome_page(
    app: &gtk4::Application,
    config: &InstallerConfig,
    state: &InstallerState,
    stack: &gtk4::Stack,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let auth_configured = state.auth.as_ref().is_some_and(|auth| auth.configured);
    let auth_authenticated = state.auth.as_ref().is_some_and(|auth| auth.authenticated);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-onboarding-root");

    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_valign(gtk::Align::Center);
    center.set_halign(gtk::Align::Center);
    center.set_vexpand(true);
    center.set_hexpand(true);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.add_css_class("gos-onboarding");
    column.set_halign(gtk::Align::Center);
    column.set_size_request(580, -1);

    let mark = goblins_os_ui::themed_brand_mark(76);
    mark.set_margin_bottom(24);
    column.append(&mark);

    column.append(&centered_label(
        "Goblins OS",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label(
        "Welcome to Goblins OS",
        "gos-onboarding-title",
        false,
    ));
    column.append(&centered_label(
        "Build what you need — on-device with GPT-OSS, or with your OpenAI account through Codex. No apps, no store: you build it.",
        "gos-onboarding-subtitle",
        true,
    ));

    let primary = button("Start guided setup", &["gos-onboarding-primary"]);
    primary.set_halign(gtk::Align::Center);
    {
        let stack = stack.clone();
        primary.connect_clicked(move |_| stack.set_visible_child_name("appearance"));
    }
    column.append(&primary);

    // Privacy-first path: enter the OS in offline / private mode. GPT-OSS runs the
    // same on-device, but the AI is held to this machine — no internet egress.
    let private = button(
        "Private — keep this computer offline",
        &["gos-onboarding-secondary"],
    );
    private.set_halign(gtk::Align::Center);
    {
        let app_handle = app.clone();
        let core_url = config.core_url.clone();
        private.connect_clicked(move |_| match set_privacy_mode(&core_url, true) {
            Ok(()) => match complete_and_unlock_first_boot(&core_url, "local-gpt-oss") {
                Ok(()) => app_handle.quit(),
                Err(error) => eprintln!("installer_complete_private_error={error}"),
            },
            Err(error) => eprintln!("installer_privacy_mode_error={error}"),
        });
    }
    column.append(&private);

    // Install Goblins OS permanently to this computer. Shown only when the OS can
    // actually carry it out — bootc present, running privileged, and at least one
    // eligible disk — so we never render an install affordance we can’t honor.
    let can_install = state.install_targets.as_ref().is_some_and(|status| {
        status.bootc.available
            && status.bootc.privileged
            && status.targets.iter().any(|target| target.eligible)
    });
    if can_install {
        let install = button(
            "Install Goblins OS to this computer",
            &["gos-onboarding-secondary"],
        );
        install.set_halign(gtk::Align::Center);
        {
            let stack = stack.clone();
            install.connect_clicked(move |_| stack.set_visible_child_name("install-disk"));
        }
        column.append(&install);
        column.append(&centered_label(
            "Run from this live image without changing your disk, or install Goblins OS permanently.",
            "gos-onboarding-footnote",
            true,
        ));
    }

    // Only offer "sign in with your OpenAI account" when a real OpenAI account
    // provider is actually provisioned (enterprise/MDM, or a future "Sign in with
    // ChatGPT" client). Otherwise we never show an account button we can’t honor —
    // the honest way to use OpenAI's hosted models is your own API key (below).
    if auth_configured {
        let openai = button(
            if auth_authenticated {
                "Enter with your OpenAI account"
            } else {
                "Sign in with your OpenAI account"
            },
            &["gos-onboarding-secondary"],
        );
        openai.set_halign(gtk::Align::Center);
        {
            let app_handle = app.clone();
            let core_url = config.core_url.clone();
            openai.connect_clicked(move |_| {
                if auth_authenticated {
                    if complete_and_unlock_first_boot(&core_url, "cloud-openai").is_ok() {
                        app_handle.quit();
                    }
                } else if let Ok(destination) = openai_login_destination(&core_url) {
                    let _ = gtk::gio::AppInfo::launch_default_for_uri(
                        &destination,
                        None::<&gtk::gio::AppLaunchContext>,
                    );
                }
            });
        }
        column.append(&openai);
    }

    let customize = button("Advanced setup", &["gos-onboarding-quiet"]);
    customize.set_halign(gtk::Align::Center);
    {
        let stack = stack.clone();
        customize.connect_clicked(move |_| {
            stack.set_visible_child_name("details");
        });
    }
    column.append(&customize);

    // A quiet, optional route to connect — connecting downloads GPT-OSS and lets
    // the AI build/fetch, but it never gates "Continue with GPT-OSS" above.
    let online = state.network.as_ref().is_some_and(|network| network.online);
    if online {
        let connected = state
            .network
            .as_ref()
            .and_then(|network| network.active.as_ref())
            .map(|active| {
                let name = active.name.trim();
                // Never surface a raw kernel interface name (enp0s1, eth0, wlp…) to a
                // first-boot person — fall back to a calm, human line instead.
                if name.is_empty() || is_kernel_ifname(name) {
                    "Connected to the internet.".to_string()
                } else {
                    format!("Connected to {name}.")
                }
            })
            .unwrap_or_else(|| "You’re online.".to_string());
        column.append(&centered_label(
            &connected,
            "gos-onboarding-footnote",
            false,
        ));
    } else {
        let connect = button("Connect to Wi-Fi", &["gos-onboarding-quiet"]);
        connect.set_halign(gtk::Align::Center);
        {
            let stack = stack.clone();
            connect.connect_clicked(move |_| stack.set_visible_child_name("network"));
        }
        column.append(&connect);
    }

    column.append(&centered_label(
        "Want OpenAI's hosted models? Add your own OpenAI API key in Settings.",
        "gos-onboarding-footnote",
        true,
    ));

    center.append(&column);
    root.append(&center);
    root
}

/// Shared top offset for the three guided setup steps (Appearance, Accessibility,
/// First App). The steps carry bodies of different heights (different subtitle wrap
/// counts and card sizes), so vertically centering each column made the header
/// (back-link + mark + eyebrow + title) land at a different Y on every step — a
/// visible baseline jump as the user advanced. Anchoring each column to the top with
/// this one constant pins the whole header block to the same origin on all three.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const ONBOARDING_STEP_HEADER_TOP: i32 = 96;

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_appearance_page(stack: &gtk4::Stack) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-onboarding-root");

    // Top-anchored (not centered) so the header baseline is identical across the
    // three guided steps regardless of how tall each body is. See
    // ONBOARDING_STEP_HEADER_TOP.
    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_valign(gtk::Align::Start);
    center.set_halign(gtk::Align::Center);
    center.set_vexpand(true);
    center.set_hexpand(true);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_size_request(620, -1);
    column.set_halign(gtk::Align::Center);
    column.set_margin_top(ONBOARDING_STEP_HEADER_TOP);

    let back = button("← Welcome", &["gos-onboarding-quiet"]);
    back.set_halign(gtk::Align::Start);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("welcome"));
    }
    column.append(&back);

    let mark = goblins_os_ui::themed_brand_mark(60);
    mark.set_margin_top(6);
    mark.set_margin_bottom(18);
    column.append(&mark);

    column.append(&centered_label(
        "Step 1 · Appearance",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label(
        "Choose the desktop tone",
        "gos-onboarding-title",
        false,
    ));
    column.append(&centered_label(
        "Pick the first look for the menu bar, launcher, control center, Files, and every native Goblins OS surface. You can change it later in Settings.",
        "gos-onboarding-subtitle",
        true,
    ));

    // Same containment as the Accessibility and First App steps: the choice cards
    // live inside the shared onboarding card (gos-net-panel) rather than floating
    // bare on the canvas, so the layout does not visibly jump between guided steps.
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("gos-net-panel");
    panel.set_size_request(620, -1);

    let grid = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    grid.set_homogeneous(true);

    let light = setup_choice("Light", "Paper surfaces, graphite ink, high clarity.");
    let dark = setup_choice("Dark", "Graphite glass, quiet contrast, night-ready.");
    let tone_group = Rc::new([light.clone(), dark.clone()]);
    {
        let tone_group = tone_group.clone();
        let light = light.clone();
        light.connect_clicked(move |chosen| {
            let _ = goblins_os_ui::set_color_scheme("default");
            select_one(chosen, tone_group.as_slice());
        });
    }
    {
        let tone_group = tone_group.clone();
        let dark = dark.clone();
        dark.connect_clicked(move |chosen| {
            let _ = goblins_os_ui::set_color_scheme("prefer-dark");
            select_one(chosen, tone_group.as_slice());
        });
    }
    // Open already showing the live tone: dark when the desktop prefers dark,
    // light otherwise (matching what set_color_scheme writes above).
    if goblins_os_ui::system_color_scheme() == "prefer-dark" {
        select_one(&dark, tone_group.as_slice());
    } else {
        select_one(&light, tone_group.as_slice());
    }
    grid.append(&light);
    grid.append(&dark);
    panel.append(&grid);
    column.append(&panel);

    let cont = button("Continue", &["gos-onboarding-primary"]);
    cont.set_halign(gtk::Align::Center);
    cont.set_margin_top(24);
    {
        let stack = stack.clone();
        cont.connect_clicked(move |_| stack.set_visible_child_name("accessibility"));
    }
    column.append(&cont);

    center.append(&column);
    root.append(&center);
    root
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_accessibility_page(stack: &gtk4::Stack) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-onboarding-root");

    // Top-anchored so this step's header shares the same baseline as the other two
    // guided steps. See ONBOARDING_STEP_HEADER_TOP.
    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_valign(gtk::Align::Start);
    center.set_halign(gtk::Align::Center);
    center.set_vexpand(true);
    center.set_hexpand(true);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_size_request(620, -1);
    column.set_halign(gtk::Align::Center);
    column.set_margin_top(ONBOARDING_STEP_HEADER_TOP);

    let back = button("← Appearance", &["gos-onboarding-quiet"]);
    back.set_halign(gtk::Align::Start);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("appearance"));
    }
    column.append(&back);

    let mark = goblins_os_ui::themed_brand_mark(60);
    mark.set_margin_top(6);
    mark.set_margin_bottom(18);
    column.append(&mark);

    column.append(&centered_label(
        "Step 2 · Accessibility",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label(
        "Set the motion and type",
        "gos-onboarding-title",
        false,
    ));
    column.append(&centered_label(
        "Goblins OS uses short spring motion and Inter by default. These toggles write the desktop accessibility settings, so system utilities follow the same preference.",
        "gos-onboarding-subtitle",
        true,
    ));

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("gos-net-panel");
    panel.set_size_request(620, -1);

    let motion = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    motion.set_homogeneous(true);
    let standard_motion = setup_choice("Standard motion", "Fluid window and panel transitions.");
    let reduce_motion = setup_choice(
        "Reduce motion",
        "Cut transitions and keep state changes direct.",
    );
    let motion_group = Rc::new([standard_motion.clone(), reduce_motion.clone()]);
    {
        let motion_group = motion_group.clone();
        standard_motion.connect_clicked(move |chosen| {
            let _ = set_interface_bool("enable-animations", true);
            select_one(chosen, motion_group.as_slice());
        });
    }
    {
        let motion_group = motion_group.clone();
        reduce_motion.connect_clicked(move |chosen| {
            let _ = set_interface_bool("enable-animations", false);
            select_one(chosen, motion_group.as_slice());
        });
    }
    // Open showing the live motion preference (animations on => Standard motion).
    if interface_bool("enable-animations", true) {
        select_one(&standard_motion, motion_group.as_slice());
    } else {
        select_one(&reduce_motion, motion_group.as_slice());
    }
    motion.append(&standard_motion);
    motion.append(&reduce_motion);
    panel.append(&motion);

    let type_size = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    type_size.set_homogeneous(true);
    let regular_text = setup_choice("Regular text", "The default Inter scale.");
    let larger_text = setup_choice("Larger text", "Increase the desktop text scale.");
    let text_group = Rc::new([regular_text.clone(), larger_text.clone()]);
    {
        let text_group = text_group.clone();
        regular_text.connect_clicked(move |chosen| {
            let _ = set_interface_double("text-scaling-factor", 1.0);
            select_one(chosen, text_group.as_slice());
        });
    }
    {
        let text_group = text_group.clone();
        larger_text.connect_clicked(move |chosen| {
            let _ = set_interface_double("text-scaling-factor", 1.16);
            select_one(chosen, text_group.as_slice());
        });
    }
    // Open showing the live text scale: Regular when ~1.0, Larger when scaled up.
    if (interface_double("text-scaling-factor", 1.0) - 1.0).abs() < 0.01 {
        select_one(&regular_text, text_group.as_slice());
    } else {
        select_one(&larger_text, text_group.as_slice());
    }
    type_size.append(&regular_text);
    type_size.append(&larger_text);
    panel.append(&type_size);
    column.append(&panel);

    let cont = button("Continue", &["gos-onboarding-primary"]);
    cont.set_halign(gtk::Align::Center);
    cont.set_margin_top(24);
    {
        let stack = stack.clone();
        cont.connect_clicked(move |_| stack.set_visible_child_name("first-app"));
    }
    column.append(&cont);

    center.append(&column);
    root.append(&center);
    root
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_first_app_page(
    app: &gtk4::Application,
    config: &InstallerConfig,
    stack: &gtk4::Stack,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-onboarding-root");

    // Top-anchored so this step's header shares the same baseline as the other two
    // guided steps. See ONBOARDING_STEP_HEADER_TOP.
    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_valign(gtk::Align::Start);
    center.set_halign(gtk::Align::Center);
    center.set_vexpand(true);
    center.set_hexpand(true);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_size_request(620, -1);
    column.set_halign(gtk::Align::Center);
    column.set_margin_top(ONBOARDING_STEP_HEADER_TOP);

    let back = button("← Accessibility", &["gos-onboarding-quiet"]);
    back.set_halign(gtk::Align::Start);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("accessibility"));
    }
    column.append(&back);

    let mark = goblins_os_ui::themed_brand_mark(60);
    mark.set_margin_top(6);
    mark.set_margin_bottom(18);
    column.append(&mark);

    column.append(&centered_label(
        "Step 3 · First App",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label(
        "Build the first thing you need",
        "gos-onboarding-title",
        false,
    ));
    column.append(&centered_label(
        first_app_onboarding_subtitle(),
        "gos-onboarding-subtitle",
        true,
    ));

    // Same 10px inner rhythm as the Appearance and Accessibility step cards, so the
    // three onboarding cards share one container grammar rather than each using a
    // slightly different internal spacing.
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("gos-net-panel");
    panel.set_size_request(620, -1);

    let entry = gtk::Entry::new();
    entry.add_css_class("gos-setup-first-app-entry");
    entry.set_placeholder_text(Some("A focus timer that logs writing sessions"));
    panel.append(&entry);

    let feedback = label(
        "You can skip this and build from the launcher at any time.",
        &["gos-net-helper"],
    );
    feedback.set_xalign(0.0);
    panel.append(&feedback);
    column.append(&panel);

    // Same CTA grammar as Steps 1 and 2: the advancing action lives OUTSIDE and
    // below the card, with the same 24px top margin the Continue button uses, so the
    // three steps read as one machine advancing — never a card with its buttons
    // nested on one step and floating beneath on the next. The primary "Build first
    // app" leads; the "Enter Goblins OS" skip is the quiet secondary directly below
    // it, so the primary always out-ranks the skip.
    let actions = gtk::Box::new(gtk::Orientation::Vertical, 10);
    actions.set_halign(gtk::Align::Center);
    actions.set_margin_top(24);
    let build = button("Build first app", &["gos-onboarding-primary"]);
    build.set_halign(gtk::Align::Center);
    // The primary reads as a live CTA only once there is a real intent to build.
    // While the field is empty it stays disabled (and GTK dims it through :disabled),
    // so a saturated solid-blue primary never sits over an empty field promising an
    // action it has nothing to act on — the always-available "Enter Goblins OS" skip
    // below carries the empty-field path instead. The entry's `changed` signal flips
    // this the moment any non-whitespace text is typed.
    build.set_sensitive(!entry.text().trim().is_empty());
    let skip = button("Enter Goblins OS", &["gos-onboarding-quiet"]);
    skip.set_halign(gtk::Align::Center);
    actions.append(&build);
    actions.append(&skip);
    column.append(&actions);

    {
        let build = build.clone();
        entry.connect_changed(move |entry| {
            build.set_sensitive(!entry.text().trim().is_empty());
        });
    }

    {
        let app_handle = app.clone();
        let core_url = config.core_url.clone();
        let entry = entry.clone();
        let feedback = feedback.clone();
        build.connect_clicked(move |_| {
            let intent = entry.text().trim().to_string();
            // The button is only sensitive when the field has content, so this is a
            // safety net rather than a routine path — never a silent placeholder build.
            let intent = if intent.is_empty() {
                "A focus timer that logs writing sessions".to_string()
            } else {
                intent
            };
            match submit_setup_build(&core_url, &intent)
                .and_then(|_| complete_and_unlock_first_boot(&core_url, "local-gpt-oss"))
            {
                Ok(()) => app_handle.quit(),
                Err(error) => {
                    feedback.set_text(first_app_build_failure_copy());
                    eprintln!("installer_first_app_error={error}");
                }
            }
        });
    }
    {
        let app_handle = app.clone();
        let core_url = config.core_url.clone();
        skip.connect_clicked(move |_| {
            match complete_and_unlock_first_boot(&core_url, "local-gpt-oss") {
                Ok(()) => app_handle.quit(),
                Err(error) => eprintln!("installer_complete_guided_skip_error={error}"),
            }
        });
    }

    center.append(&column);
    root.append(&center);
    root
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn setup_choice(title: &str, detail: &str) -> gtk4::Button {
    use gtk4::prelude::*;

    let button = gtk4::Button::new();
    button.add_css_class("gos-setup-choice");

    // Title/copy on the left, a single-select checkmark pinned to the trailing
    // edge. macOS Setup Assistant marks the active option with a check, not only
    // a fill — so the chosen tone/motion/type-size card is unmistakable even
    // when two accent-tinted cards sit side by side. select_one toggles its
    // visibility; it starts hidden and inherits the card's ink, so it stays
    // legible over the accent tint in both Light and Dark.
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let body = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    body.set_hexpand(true);
    body.append(&label(title, &["gos-row-title"]));
    body.append(&label(detail, &["gos-row-copy"]));
    row.append(&body);

    let check = label("✓", &["gos-setup-choice-check"]);
    check.set_valign(gtk4::Align::Center);
    check.set_halign(gtk4::Align::End);
    check.set_visible(false);
    row.append(&check);

    // select_one finds the checkmark by walking the body's sibling, so keep the
    // check as the row's last child.
    button.set_child(Some(&row));
    button
}

/// The trailing checkmark a `setup_choice` card carries, or `None` for a card
/// built some other way. Lets `select_one` flip the check's visibility without
/// each caller having to thread the widget through.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn setup_choice_check(card: &gtk4::Button) -> Option<gtk4::Label> {
    use gtk4::prelude::*;

    let row = card.child()?;
    let mut child = row.first_child();
    while let Some(widget) = child {
        if let Some(check) = widget
            .downcast_ref::<gtk4::Label>()
            .filter(|lbl| lbl.has_css_class("gos-setup-choice-check"))
        {
            return Some(check.clone());
        }
        child = widget.next_sibling();
    }
    None
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_interface_bool(key: &str, value: bool) -> bool {
    use gtk4::prelude::SettingsExt;

    if gtk4::gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.desktop.interface", true))
        .is_none()
    {
        return false;
    }
    gtk4::gio::Settings::new("org.gnome.desktop.interface")
        .set_boolean(key, value)
        .is_ok()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_interface_double(key: &str, value: f64) -> bool {
    use gtk4::prelude::SettingsExt;

    if gtk4::gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.desktop.interface", true))
        .is_none()
    {
        return false;
    }
    gtk4::gio::Settings::new("org.gnome.desktop.interface")
        .set_double(key, value)
        .is_ok()
}

/// Read a desktop-interface boolean (e.g. `enable-animations`), falling back to
/// `default` when the schema is absent — so a minimal container pre-selects the
/// honest default instead of aborting. Mirrors `set_interface_bool`.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn interface_bool(key: &str, default: bool) -> bool {
    use gtk4::prelude::SettingsExt;

    if gtk4::gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.desktop.interface", true))
        .is_none()
    {
        return default;
    }
    gtk4::gio::Settings::new("org.gnome.desktop.interface").boolean(key)
}

/// Read a desktop-interface double (e.g. `text-scaling-factor`), falling back to
/// `default` when the schema is absent. Mirrors `set_interface_double`.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn interface_double(key: &str, default: f64) -> f64 {
    use gtk4::prelude::SettingsExt;

    if gtk4::gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.desktop.interface", true))
        .is_none()
    {
        return default;
    }
    gtk4::gio::Settings::new("org.gnome.desktop.interface").double(key)
}

/// Mark `chosen` as the active card in a homogeneous setup group and clear the
/// selected state from its siblings, so exactly one card in the group ever reads
/// as selected. macOS Setup Assistant always shows the current tone/option this
/// way; the Appearance and Accessibility steps reuse this for every group.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn select_one(chosen: &gtk4::Button, group: &[gtk4::Button]) {
    use gtk4::prelude::WidgetExt;

    for card in group {
        let is_chosen = card == chosen;
        if is_chosen {
            card.add_css_class("gos-setup-choice-selected");
        } else {
            card.remove_css_class("gos-setup-choice-selected");
        }
        // Show the trailing check only on the active card, so exactly one card
        // in the group ever reads as picked — the fill-tint and the checkmark
        // agree.
        if let Some(check) = setup_choice_check(card) {
            check.set_visible(is_chosen);
        }
    }
}

/// True when a NetworkManager connection name is really a raw kernel interface name
/// (enp0s1, eth0, wlp2s0, eno1, lo…) rather than a human network/SSID — those should
/// never reach first-boot copy. Heuristic: a known NIC prefix, no spaces, has a digit.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn is_kernel_ifname(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() || n.contains(' ') {
        return false;
    }
    let prefixed = [
        "enp", "eno", "ens", "eth", "en", "wlp", "wlan", "wl", "usb", "lo",
    ]
    .iter()
    .any(|p| n.starts_with(p));
    prefixed && n.bytes().any(|b| b.is_ascii_digit())
}

/// The first-boot network step: get connected so the OS can download GPT-OSS and
/// let the on-device model build apps and fetch packages. Prominent but never
/// blocking — an offline user can always continue (local GPT-OSS runs offline).
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_network_page(
    config: &InstallerConfig,
    state: &InstallerState,
    stack: &gtk4::Stack,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let manager = state
        .network
        .as_ref()
        .map(|network| network.manager_available)
        .unwrap_or(false);
    let online = state.network.as_ref().is_some_and(|network| network.online);
    let active_name = state
        .network
        .as_ref()
        .and_then(|network| network.active.as_ref())
        .and_then(|active| {
            let name = active.name.trim();
            if name.is_empty() || is_kernel_ifname(name) {
                None
            } else {
                Some(format!("{} ({})", name, active.kind))
            }
        });

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-onboarding-root");

    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_valign(gtk::Align::Center);
    center.set_halign(gtk::Align::Center);
    center.set_vexpand(true);
    center.set_hexpand(true);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_size_request(580, -1);
    column.set_halign(gtk::Align::Center);

    let back = button("← Welcome", &["gos-onboarding-quiet"]);
    back.set_halign(gtk::Align::Start);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("welcome"));
    }
    column.append(&back);

    let mark = goblins_os_ui::themed_brand_mark(60);
    mark.set_margin_top(6);
    mark.set_margin_bottom(18);
    column.append(&mark);

    column.append(&centered_label(
        "Step · Network",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label("Get connected", "gos-net-title", false));
    column.append(&centered_label(
        "Goblins OS downloads your GPT-OSS model and lets the on-device AI build apps and fetch packages. A connection now makes the first run faster — but it’s optional.",
        "gos-onboarding-subtitle",
        true,
    ));

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    panel.add_css_class("gos-net-panel");
    panel.set_size_request(580, -1);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("gos-net-dot");
    dot.set_size_request(10, 10);
    dot.set_valign(gtk::Align::Center);
    if online {
        dot.add_css_class("is-online");
    } else if !manager {
        dot.add_css_class("is-blocked");
    }
    let status = label(net_status_label(manager, online), &["gos-net-status-label"]);
    let rescan = button("Rescan", &["gos-net-rescan"]);
    // "Rescan" only makes sense once a scan exists — before that the body offers
    // the single "Scan for Wi-Fi" action.
    rescan.set_visible(manager && online);
    header.append(&dot);
    header.append(&status);
    header.append(&spacer());
    header.append(&rescan);
    panel.append(&header);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.append(&body);
    column.append(&panel);

    let cont = button("Continue", &["gos-onboarding-primary"]);
    cont.set_halign(gtk::Align::Center);
    cont.set_margin_top(22);
    cont.set_label(net_continue_label(manager, online));
    {
        let stack = stack.clone();
        cont.connect_clicked(move |_| stack.set_visible_child_name("welcome"));
    }
    column.append(&cont);

    column.append(&centered_label(
        "Goblins OS works offline once your model is on device. You can connect anytime later from Settings.",
        "gos-onboarding-footnote",
        true,
    ));

    // Initial body content reflects the loaded connectivity; from here the body is
    // rebuilt in place by the scan / select / join handlers.
    if !manager {
        net_row(
            &body,
            "The network service is not responding on this device. You can still continue offline; Goblins OS runs offline, and you can connect later from Settings > Network.",
            true,
        );
    } else if online {
        net_row(
            &body,
            &match active_name {
                Some(name) => format!("Connected to {name} · the internet is reachable."),
                None => "Connected · the internet is reachable.".to_string(),
            },
            false,
        );
    } else {
        let scan = button("Scan for Wi-Fi", &["gos-local-action"]);
        let core_url = config.core_url.clone();
        let body_c = body.clone();
        let status_c = status.clone();
        let dot_c = dot.clone();
        let cont_c = cont.clone();
        let rescan_c = rescan.clone();
        scan.connect_clicked(move |_| {
            net_render_list(&core_url, &body_c, &status_c, &dot_c, &cont_c);
            rescan_c.set_visible(true);
        });
        body.append(&scan);
    }

    // Rescan re-runs the Wi-Fi scan and repaints the list.
    {
        let core_url = config.core_url.clone();
        let body_c = body.clone();
        let status_c = status.clone();
        let dot_c = dot.clone();
        let cont_c = cont.clone();
        rescan.connect_clicked(move |_| {
            net_render_list(&core_url, &body_c, &status_c, &dot_c, &cont_c);
        });
    }

    center.append(&column);
    root.append(&center);
    root
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn net_status_label(manager: bool, online: bool) -> &'static str {
    if !manager {
        "Networking not ready"
    } else if online {
        "Connected"
    } else {
        "Not connected"
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn net_continue_label(manager: bool, online: bool) -> &'static str {
    if !manager {
        "Continue offline"
    } else if online {
        "Continue"
    } else {
        "Continue without Wi-Fi"
    }
}

/// Remove every child of a body box so it can be repainted for the next state.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn net_clear(body: &gtk4::Box) {
    use gtk4::prelude::*;
    while let Some(child) = body.first_child() {
        body.remove(&child);
    }
}

/// Append a single calm status row to the network card body.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn net_row(body: &gtk4::Box, text: &str, blocked: bool) {
    use gtk4::prelude::*;
    let row = label(text, &["gos-net-row"]);
    row.set_wrap(true);
    row.set_xalign(0.0);
    if blocked {
        row.add_css_class("gos-blocked-soft");
    }
    body.append(&row);
}

/// Scan for Wi-Fi and paint the network list into the card body.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn net_render_list(
    core_url: &str,
    body: &gtk4::Box,
    status: &gtk4::Label,
    dot: &gtk4::Box,
    cont: &gtk4::Button,
) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    net_clear(body);
    status.set_text("Choose a network");

    let scan = fetch_wifi_scan(core_url);
    let Some(scan) = scan else {
        net_row(
            body,
            "Goblins OS could not reach the network service.",
            true,
        );
        return;
    };
    if !scan.manager_available {
        net_row(
            body,
            "The network service is not responding on this device.",
            true,
        );
        return;
    }
    if scan.networks.is_empty() {
        net_row(
            body,
            if scan.detail.is_empty() {
                "No networks found nearby. Try Rescan."
            } else {
                scan.detail.as_str()
            },
            false,
        );
        return;
    }

    for network in scan.networks.iter().take(6) {
        let row = gtk::Button::new();
        row.add_css_class("gos-net-ssid");
        row.set_hexpand(true);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        line.append(&label(&network.ssid, &["gos-net-ssid-name"]));
        line.append(&spacer());
        line.append(&label(&net_meta(network), &["gos-net-meta"]));
        row.set_child(Some(&line));

        let core_url = core_url.to_string();
        let ssid = network.ssid.clone();
        let secured = !network.security.trim().is_empty();
        let body_c = body.clone();
        let status_c = status.clone();
        let dot_c = dot.clone();
        let cont_c = cont.clone();
        row.connect_clicked(move |_| {
            if secured {
                net_render_join(&core_url, &ssid, None, &body_c, &status_c, &dot_c, &cont_c);
            } else {
                net_try_connect(&core_url, &ssid, "", &body_c, &status_c, &dot_c, &cont_c);
            }
        });
        body.append(&row);
    }
}

/// A calm right-aligned descriptor for a network: security + signal strength.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn net_meta(network: &WifiNetwork) -> String {
    let lock = if network.security.trim().is_empty() {
        "Open"
    } else {
        "Secured"
    };
    let strength = if network.signal >= 67 {
        "Strong"
    } else if network.signal >= 34 {
        "Fair"
    } else {
        "Weak"
    };
    if network.in_use {
        format!("{lock} · Connected")
    } else {
        format!("{lock} · {strength}")
    }
}

/// Paint the focused "join this network" view: password field + Join.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn net_render_join(
    core_url: &str,
    ssid: &str,
    error: Option<&str>,
    body: &gtk4::Box,
    status: &gtk4::Label,
    dot: &gtk4::Box,
    cont: &gtk4::Button,
) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    net_clear(body);
    status.set_text("Enter the password");

    body.append(&label(ssid, &["gos-net-ssid-name"]));

    let entry = gtk::PasswordEntry::new();
    entry.add_css_class("gos-net-passfield");
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some("Network password"));
    body.append(&entry);

    let helper = label(
        "Stored on this device by the OS. Goblins OS never sends your password to OpenAI.",
        &["gos-net-helper"],
    );
    helper.set_wrap(true);
    helper.set_xalign(0.0);
    body.append(&helper);

    if let Some(error) = error {
        let line = label(error, &["gos-net-helper", "gos-blocked-soft"]);
        line.set_wrap(true);
        line.set_xalign(0.0);
        body.append(&line);
    }

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let join = button("Join", &["gos-net-join"]);
    let cancel = button("← Networks", &["gos-onboarding-quiet"]);
    actions.append(&join);
    actions.append(&cancel);
    body.append(&actions);

    let connect = {
        let core_url = core_url.to_string();
        let ssid = ssid.to_string();
        let entry = entry.clone();
        let body_c = body.clone();
        let status_c = status.clone();
        let dot_c = dot.clone();
        let cont_c = cont.clone();
        move || {
            let password = entry.text().to_string();
            net_try_connect(
                &core_url,
                &ssid,
                password.trim(),
                &body_c,
                &status_c,
                &dot_c,
                &cont_c,
            );
        }
    };
    {
        let connect = connect.clone();
        join.connect_clicked(move |_| connect());
    }
    {
        let connect = connect.clone();
        entry.connect_activate(move |_| connect());
    }
    {
        let core_url = core_url.to_string();
        let body_c = body.clone();
        let status_c = status.clone();
        let dot_c = dot.clone();
        let cont_c = cont.clone();
        cancel.connect_clicked(move |_| {
            net_render_list(&core_url, &body_c, &status_c, &dot_c, &cont_c);
        });
    }
}

/// Attempt to join a network through the OS core; repaint the card with the
/// confirmed connection on success, or the password view + error on failure.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn net_try_connect(
    core_url: &str,
    ssid: &str,
    password: &str,
    body: &gtk4::Box,
    status: &gtk4::Label,
    dot: &gtk4::Box,
    cont: &gtk4::Button,
) {
    use gtk4::prelude::*;

    net_clear(body);
    status.set_text(&format!("Connecting to {ssid}…"));
    dot.remove_css_class("is-blocked");
    dot.add_css_class("is-connecting");

    match connect_wifi(core_url, ssid, password) {
        Ok(message) => {
            status.set_text("Connected");
            dot.remove_css_class("is-connecting");
            dot.add_css_class("is-online");
            cont.set_label("Continue");
            net_row(body, &message, false);
        }
        Err(detail) => {
            status.set_text("Couldn't connect");
            dot.remove_css_class("is-connecting");
            net_render_join(core_url, ssid, Some(&detail), body, status, dot, cont);
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn fetch_wifi_scan(core_url: &str) -> Option<WifiScan> {
    get_core_json::<WifiScan>(core_url, "/v1/network/wifi/scan").ok()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn connect_wifi(core_url: &str, ssid: &str, password: &str) -> Result<String, String> {
    let body = serde_json::json!({ "ssid": ssid, "password": password }).to_string();
    let response = http_request(
        core_url,
        "POST",
        "/v1/network/wifi/connect",
        Some(body.as_bytes()),
    )
    .map_err(|_| "Goblins OS could not reach the network service.".to_string())?;
    let outcome: WifiConnectOutcome = serde_json::from_slice(&response.body)
        .map_err(|_| "Goblins OS could not read the connection result.".to_string())?;
    if (200..=299).contains(&response.status) {
        Ok(outcome.text)
    } else {
        Err(outcome.text)
    }
}

/// The advanced first-boot surface: account, install target, and local models.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_details_page(
    app: &gtk4::Application,
    config: &InstallerConfig,
    state: &InstallerState,
    stack: &gtk4::Stack,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let auth_configured = state.auth.as_ref().is_some_and(|auth| auth.configured);
    let auth_authenticated = state.auth.as_ref().is_some_and(|auth| auth.authenticated);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-installer-root");

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    top.add_css_class("gos-installer-top");
    // The top bar is a LIGHT surface, so the back control uses the light-surface
    // button style (dark ink on a hairline pill); gos-secondary-action is the
    // white-on-graphite night style and would read washed-out here.
    let back = button("← Welcome", &["gos-local-action"]);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| {
            stack.set_visible_child_name("welcome");
        });
    }
    top.append(&back);
    top.append(&goblins_os_ui::themed_brand_mark(22));
    // The wordmark must never wrap — in this crowded top bar the shared label
    // helper's wrap=true would break "Goblins OS" onto two lines.
    let brand = label("Goblins OS", &["gos-brand"]);
    brand.set_wrap(false);
    top.append(&brand);
    // Like the wordmark, the page name must never wrap in the crowded top bar.
    let page_name = label("Advanced setup", &["gos-muted"]);
    page_name.set_wrap(false);
    top.append(&page_name);
    top.append(&spacer());
    // The three readiness chips are descriptive labels, not a tri-color health
    // signal — there is no user-legible reason two would be green and one amber. Per
    // the project's centralized "descriptive status rests in a neutral pill" rule,
    // they all render in one calm neutral chip (.gos-readiness-chip overrides the
    // ready/waiting hue), so the top bar stays as monochrome as the mark beside it.
    for chip in [
        status_pill("OS", state.boot.core_ready),
        status_pill("OpenAI", auth_authenticated),
        status_pill("Local", state.local_models.is_some()),
    ] {
        chip.add_css_class("gos-readiness-chip");
        top.append(&chip);
    }
    root.append(&top);

    // One calm, centered single column — the same shell every other onboarding
    // and install screen uses — instead of a horizontal two-panel dashboard, so
    // welcome → details → install paces as one consistent flow. The panels stack
    // vertically inside the column; the column is centered and width-capped so the
    // page does not sprawl edge-to-edge.
    let body = gtk::Box::new(gtk::Orientation::Vertical, 14);
    body.add_css_class("gos-installer-body");
    body.add_css_class("gos-onboarding");
    body.set_halign(gtk::Align::Center);
    body.set_valign(gtk::Align::Start);
    body.set_size_request(620, -1);
    body.set_margin_top(28);
    body.set_margin_bottom(40);

    let hero = gtk::Box::new(gtk::Orientation::Vertical, 18);
    hero.set_valign(gtk::Align::Start);
    hero.add_css_class("gos-hero-panel");
    hero.set_hexpand(true);
    hero.append(&label("Goblins-native desktop", &["gos-kicker"]));
    hero.append(&label("Set up Goblins OS", &["gos-hero-title"]));
    hero.append(&label(
        state
            .auth
            .as_ref()
            .map(|auth| auth.message.as_str())
            .unwrap_or("Waiting for local OS services."),
        &["gos-hero-copy"],
    ));

    // A state ("OpenAI sign-in is not set up yet" / "OpenAI account ready") is a status line,
    // never button chrome — only a real action gets a button.
    if auth_configured && !auth_authenticated {
        let sign_in = button("Sign in with OpenAI", &["gos-primary-action"]);
        let core_url = config.core_url.clone();
        sign_in.connect_clicked(move |_| match openai_login_destination(&core_url) {
            Ok(destination) => {
                if let Err(error) = gtk::gio::AppInfo::launch_default_for_uri(
                    &destination,
                    None::<&gtk::gio::AppLaunchContext>,
                ) {
                    eprintln!("installer_openai_login_launch_error={error}");
                }
            }
            Err(error) => eprintln!("installer_openai_login_start_error={error}"),
        });
        hero.append(&sign_in);
    } else {
        hero.append(&label(
            if auth_authenticated {
                "OpenAI account ready."
            } else {
                "OpenAI sign-in is not set up yet. You can continue locally and set it up later in Settings."
            },
            &["gos-hero-copy"],
        ));
    }

    // Exactly one filled lead action on the hero, so the next step is unmistakable.
    // When the OpenAI account is ready, entering the desktop with it is the headline
    // path, so it takes the filled lead and local setup recedes to the secondary
    // ghost. Until then the cloud button is a true disabled silhouette (the
    // hero-scoped .gos-disabled-action: dimmed label, transparent fill, half-strength
    // hairline — clearly inert, never an empty input) and the live local path takes
    // the filled lead instead. No two near-identical fills competing for the eye.
    let enter_cloud = button(
        "Enter Goblins OS desktop",
        if auth_authenticated {
            &["gos-primary-action"]
        } else {
            &["gos-disabled-action"]
        },
    );
    if auth_authenticated {
        let app_handle = app.clone();
        let core_url = config.core_url.clone();
        enter_cloud.connect_clicked(move |_| {
            match complete_and_unlock_first_boot(&core_url, "cloud-openai") {
                Ok(()) => app_handle.quit(),
                Err(error) => eprintln!("installer_complete_cloud_error={error}"),
            }
        });
    }
    hero.append(&enter_cloud);

    let enter_local = button(
        "Continue local setup",
        if auth_authenticated {
            &["gos-secondary-action"]
        } else {
            &["gos-primary-action"]
        },
    );
    {
        let app_handle = app.clone();
        let core_url = config.core_url.clone();
        enter_local.connect_clicked(move |_| {
            match complete_and_unlock_first_boot(&core_url, "local-gpt-oss") {
                Ok(()) => app_handle.quit(),
                Err(error) => eprintln!("installer_complete_local_error={error}"),
            }
        });
    }
    hero.append(&enter_local);

    body.append(&hero);

    let checks = gtk::Box::new(gtk::Orientation::Vertical, 10);
    checks.add_css_class("gos-checks-panel");
    checks.set_hexpand(true);
    checks.append(&label("Installer readiness", &["gos-kicker"]));
    match &state.readiness {
        Some(readiness) => {
            checks.append(&system_row("Privacy", &readiness.privacy_note));
            checks.append(&system_row("Storage", &readiness.storage_note));
            for stage in readiness.stages.iter().take(8) {
                checks.append(&system_row(
                    &format!("{} {} · {}", stage.index, stage.label, stage.state),
                    &stage.detail,
                ));
            }
        }
        None => checks.append(&system_row(
            "Installer checks",
            "Waiting for installer readiness.",
        )),
    }
    body.append(&checks);

    let install = gtk::Box::new(gtk::Orientation::Vertical, 10);
    install.add_css_class("gos-install-panel");
    install.set_hexpand(true);
    install.append(&label("Install target", &["gos-kicker"]));
    match &state.install_targets {
        Some(status) => {
            install.append(&system_row(
                "Install readiness",
                &format!(
                    "{} · default disk format {} · whole-disk installs require confirming the exact device.",
                    if status.bootc.available && status.bootc.privileged {
                        "Ready to review disk safety"
                    } else if !status.bootc.available {
                        "Start from Goblins OS install media"
                    } else {
                        "Administrator approval required"
                    },
                    status.bootc.default_filesystem,
                ),
            ));
            install.append(&system_row(
                "Storage layout",
                &format!(
                    "{} · {}",
                    status.policy.storage_layout, status.policy.local_model_weights
                ),
            ));
            if !status.policy.simple_install_scope.is_empty() {
                install.append(&system_row(
                    "Simple install scope",
                    &status.policy.simple_install_scope,
                ));
            }
            if !status.policy.formatting_guidance.is_empty() {
                install.append(&system_row(
                    "Formatting",
                    &status.policy.formatting_guidance,
                ));
            }
            if !status.policy.bootloader_recovery.is_empty() {
                install.append(&system_row(
                    "Startup recovery",
                    &status.policy.bootloader_recovery,
                ));
            }
            append_dual_boot_safe_route(&install, &status.policy, false);
            append_dual_boot_quick_start(&install, &status.policy);
            append_pre_write_install_plan(&install, &status.policy);
            append_storage_review_checklist(&install, &status.policy);
            append_dual_boot_readiness(&install, &status.policy);
            append_dual_boot_choices(&install, &status.policy);
            append_dual_boot_decision_map(&install, &status.policy);
            append_full_storage_installer_handoff(&install, &status.policy, false);
            append_dual_boot_guide(&install, &status.policy);
            install.append(&system_row(
                "Before anything is erased",
                &format!(
                    "{} No disk changes happen until you type the required confirmation.",
                    status.policy.destructive_acknowledgement
                ),
            ));
            if status.targets.is_empty() {
                install.append(&system_row(
                    "No install disks",
                    "No eligible install disks are visible right now.",
                ));
            }
            let install_feedback = label(
                "Select an eligible disk to prepare a final review. No disk is changed at this step.",
                &["gos-row-copy"],
            );
            for target in status.targets.iter().take(4) {
                install.append(&model_row(
                    &format!(
                        "{} · {}GB · {}",
                        target.path,
                        target.size_gb,
                        if target.eligible { "ready" } else { "blocked" }
                    ),
                    &format!(
                        "{} · mounted {} · removable {} · {}",
                        target.model,
                        target.mounted,
                        target.removable,
                        target
                            .reasons
                            .first()
                            .map(String::as_str)
                            .unwrap_or("Ready for final install review.")
                    ),
                    target.eligible,
                ));
                if target.eligible {
                    let prepare = button(
                        &format!("Prepare install for {}", target.path),
                        &["gos-local-action"],
                    );
                    let core_url = config.core_url.clone();
                    let target_path = target.path.clone();
                    let feedback = install_feedback.clone();
                    prepare.connect_clicked(move |_| {
                        match prepare_install_command(&core_url, &target_path) {
                            Ok(detail) => feedback.set_text(&detail),
                            Err(error) => {
                                feedback.set_text("Goblins OS could not prepare the install plan. No disk was changed.");
                                eprintln!("installer_prepare_install_error={error}");
                            }
                        }
                    });
                    install.append(&prepare);
                }
            }
            install.append(&install_feedback);
        }
        None => install.append(&system_row(
            "Install readiness",
            "Waiting for the install media storage scan.",
        )),
    }
    body.append(&install);

    let models = gtk::Box::new(gtk::Orientation::Vertical, 10);
    models.add_css_class("gos-model-panel");
    models.set_hexpand(true);
    models.append(&label("Local models", &["gos-kicker"]));
    match &state.local_models {
        Some(catalog) => {
            models.append(&system_row(
                "Hardware",
                &format!(
                    "{}GB RAM · VRAM {} · runtime {}",
                    catalog.hardware.ram_gb,
                    catalog
                        .hardware
                        .gpu_vram_gb
                        .map(|vram| format!("{vram}GB"))
                        .unwrap_or_else(|| "not detected".to_string()),
                    runtime_label(&catalog.hardware.runtime)
                ),
            ));
            let download_feedback = label(
                "Local model downloads require explicit consent.",
                &["gos-row-copy"],
            );
            for model in catalog.models.iter() {
                let detail = model
                    .reasons
                    .first()
                    .map(String::as_str)
                    .unwrap_or(model.disk_requirement.as_str());
                models.append(&model_row(
                    &format!("{} · {}", model.name, model.state),
                    &format!(
                        "{} · {} · install {} · {}",
                        model.source, detail, model.install.state, model.install.detail
                    ),
                    model.state == "installable",
                ));
                if model.state == "installable" && model.install.state != "installed" {
                    let action = button(
                        &format!("Download {} with consent", model.name),
                        &["gos-local-action"],
                    );
                    let core_url = config.core_url.clone();
                    let model_id = model.id.clone();
                    let feedback = download_feedback.clone();
                    action.connect_clicked(move |_| {
                        match request_model_install(&core_url, &model_id) {
                            Ok(detail) => feedback.set_text(&detail),
                            Err(error) => {
                                feedback.set_text(
                                    "Goblins OS rejected the local model download request.",
                                );
                                eprintln!("installer_model_install_error={error}");
                            }
                        }
                    });
                    models.append(&action);
                }
            }
            models.append(&download_feedback);
        }
        None => models.append(&system_row(
            "gpt-oss",
            "Waiting for model compatibility scan.",
        )),
    }
    body.append(&models);

    let details_scroller = gtk::ScrolledWindow::new();
    details_scroller.set_hexpand(true);
    details_scroller.set_vexpand(true);
    details_scroller.set_has_frame(false);
    details_scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    details_scroller.set_child(Some(&body));
    root.append(&details_scroller);
    root
}

/// Clear a page box and lay out the shared onboarding scaffold (full-bleed root,
/// scroll-safe, centered 580px column), returning the column to fill. Every
/// install screen reuses this so the flow paces like the rest of the first-boot
/// onboarding.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_scaffold(page: &gtk4::Box) -> gtk4::Box {
    use gtk4::prelude::*;

    net_clear(page);
    page.add_css_class("gos-onboarding-root");

    let scroll_document = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    scroll_document.set_hexpand(true);
    scroll_document.set_vexpand(true);

    let top_spacer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    top_spacer.set_vexpand(true);

    let bottom_spacer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    bottom_spacer.set_vexpand(true);

    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    column.add_css_class("gos-onboarding");
    column.set_halign(gtk4::Align::Center);
    column.set_valign(gtk4::Align::Start);
    column.set_margin_bottom(40);
    column.set_size_request(580, -1);

    scroll_document.append(&top_spacer);
    scroll_document.append(&column);
    scroll_document.append(&bottom_spacer);
    // Overflow must always be reachable: a long page (review rows + the consent
    // footnote on a short display) scrolls instead of clipping the CTA. The
    // spacers center short pages, then shrink away before content can overflow.
    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_has_frame(false);
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_child(Some(&scroll_document));
    page.append(&scroller);
    column
}

/// A short, honest device kind from the signals the scan actually has: removable
/// media reads "Removable" (a USB stick is not an SSD), otherwise the rotational
/// flag distinguishes a spinning HDD from a solid-state drive.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn disk_kind(target: &InstallTarget) -> &'static str {
    if target.removable {
        "Removable"
    } else if target.rotational {
        "HDD"
    } else {
        "SSD"
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn partition_summary(target: &InstallTarget) -> String {
    if target_partition_scan_blocked(target) {
        return "Partition scan was not readable; simple install is disabled.".to_string();
    }

    match target.partitions.len() {
        0 => "Readable scan reported no partitions".to_string(),
        1 => format!("1 partition: {}", target.partitions[0]),
        count => format!("{count} partitions: {}", target.partitions.join(", ")),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn existing_system_summary(target: &InstallTarget) -> String {
    if target_partition_scan_blocked(target) {
        return "Goblins OS could not verify existing systems because the partition scan was not readable. Use advanced storage.".to_string();
    }

    if target.existing_systems.is_empty() {
        return "No existing OS, recovery, or EFI partitions detected by the readable installer scan."
            .to_string();
    }

    let names = existing_system_kind_names(&target.existing_systems);
    let partitions = existing_system_partition_list(&target.existing_systems);
    format!("{names} detected on {partitions}. Use Custom/manual storage or Reclaim Space to preserve them.")
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn target_partition_scan_blocked(target: &InstallTarget) -> bool {
    target
        .reasons
        .iter()
        .any(|reason| reason.contains("Partition scan"))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn target_uses_preservation_handoff(target: &InstallTarget) -> bool {
    !target.existing_systems.is_empty() || target_partition_scan_blocked(target)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn preservation_handoff_prompt(target: &InstallTarget) -> String {
    if target_partition_scan_blocked(target) {
        return format!(
            "Open advanced storage because Goblins OS could not verify partitions on {}.",
            target.path
        );
    }

    // The card states the INTENT — which systems are kept — not the raw partition
    // device nodes. A literal "/dev/nvme0n1p1, /dev/nvme0n1p2, …" list dumped into
    // body copy made the row unscannable; the exact node list still lives one hover
    // away in the row tooltip (disk_card_detail_tooltip -> existing_system_summary,
    // built from existing_system_partition_list), so nothing is lost. The leading
    // "Open advanced storage from detected disk" verb phrase is pinned by the verify
    // crate and the shipping gate, so it stays verbatim — only the node list is cut.
    format!(
        "Open advanced storage from detected disk to keep {} and other partitions on this disk.",
        existing_system_kind_names(&target.existing_systems)
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn detected_system_preservation_checklist(target: &InstallTarget) -> Option<String> {
    if target_partition_scan_blocked(target) {
        return Some(
            "The simple installer cannot prove this disk is blank. Open advanced storage, verify the target, every preserve/format row, and the bootloader/EFI target in the final summary, then write only if it matches your intent."
                .to_string(),
        );
    }

    if target.existing_systems.is_empty() {
        return None;
    }

    Some(format!(
        "Back up and save recovery keys. {}. In advanced storage, choose only unallocated free space or a dedicated Goblins OS disk. Before writing, confirm {} stay preserved and the bootloader/EFI target is named. After install, test every preserved system from the firmware boot picker before changing boot order.",
        detected_system_preparation_hint(target),
        existing_system_partition_list(&target.existing_systems)
    ))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn detected_system_preparation_hint(target: &InstallTarget) -> String {
    let mut steps = Vec::new();
    if target
        .existing_systems
        .iter()
        .any(|system| system.kind == "Windows")
    {
        steps.push("Windows: suspend BitLocker if enabled and create space with Disk Management");
    }
    if target
        .existing_systems
        .iter()
        .any(|system| system.kind == "macOS/APFS")
    {
        steps.push("macOS: create space with Disk Utility and leave APFS plus recovery intact");
    }
    if target
        .existing_systems
        .iter()
        .any(|system| system.kind == "Linux")
    {
        steps.push("Linux: resize with the distro or trusted live media that understands LUKS, LVM, and the filesystem");
    }
    if target
        .existing_systems
        .iter()
        .any(|system| system.kind == "Other OS/data")
    {
        steps.push(
            "Other OS/data: identify unknown partitions and preserve them unless replacing that data",
        );
    }
    if steps.is_empty() {
        steps.push("Treat EFI, recovery, vendor, and unknown partitions as preserve-by-default");
    }
    steps.join(". ")
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn existing_system_kind_names(systems: &[ExistingSystem]) -> String {
    let mut kinds = Vec::new();
    for system in systems {
        if !kinds.contains(&system.kind.as_str()) {
            kinds.push(system.kind.as_str());
        }
    }
    match kinds.as_slice() {
        [] => "No existing systems".to_string(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn existing_system_partition_list(systems: &[ExistingSystem]) -> String {
    systems
        .iter()
        .map(|system| system.partition.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn existing_system_details(target: &InstallTarget) -> String {
    if target_partition_scan_blocked(target) {
        return "The partition scan was not readable, so Goblins OS did not classify this disk as blank. Manual storage must show the preserve/format and bootloader/EFI rows before any write."
            .to_string();
    }

    if target.existing_systems.is_empty() {
        return "No preservation signals were reported for this disk.".to_string();
    }

    target
        .existing_systems
        .iter()
        .map(|system| {
            format!(
                "{}: {}. {}",
                system.kind, system.detail, system.preservation
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_recommendation_summary(target: &InstallTarget) -> String {
    if target.recommendation.title.is_empty() {
        if target.eligible {
            return "Replace this blank disk. Continue only if this whole disk should belong to Goblins OS."
                .to_string();
        }
        return install_blocked_detail(target);
    }

    format!(
        "{}. {} Target: {} Preserve: {} Finish: {}",
        target.recommendation.title,
        target.recommendation.action,
        target.recommendation.install_target,
        target.recommendation.preserve,
        target.recommendation.finish
    )
}

/// Human label for the internal dual-boot plan status slug (never shown raw).
/// The raw slug stays the internal key for CSS-class/logic comparisons.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn dual_boot_status_label(status: &str) -> &str {
    match status {
        "manual-preserve-required" => "manual storage required to preserve the existing OS",
        "blank-dedicated-disk-ready" => "ready — a blank, dedicated disk",
        "blocked-until-reviewed" => "blocked until you review storage",
        other => other,
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn dual_boot_plan_summary(target: &InstallTarget) -> String {
    if target.dual_boot_plan.title.is_empty() {
        if target.existing_systems.is_empty() {
            return "No existing operating system was detected on this disk by the installer scan."
                .to_string();
        }
        return dual_boot_disk_guidance(target).unwrap_or_else(|| {
            "Open advanced storage, then use Installation Destination with Custom/manual storage or Reclaim Space to keep another OS."
                .to_string()
        });
    }

    let mut detail = format!(
        "{}. {} Action: {} Target: {} Preserve: {} Boot: {} Finish: {}",
        target.dual_boot_plan.title,
        target.dual_boot_plan.summary,
        target.dual_boot_plan.primary_action,
        target.dual_boot_plan.storage_target,
        target.dual_boot_plan.preserve,
        target.dual_boot_plan.bootloader,
        target.dual_boot_plan.finish
    );
    if !target.dual_boot_plan.status.is_empty() {
        detail.push_str(&format!(
            " Status: {}.",
            dual_boot_status_label(&target.dual_boot_plan.status)
        ));
    }
    if !target.dual_boot_plan.steps.is_empty() {
        detail.push_str(" Steps: ");
        detail.push_str(&target.dual_boot_plan.steps.join(" "));
    }
    detail
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn dual_boot_disk_guidance(target: &InstallTarget) -> Option<String> {
    if target.existing_systems.is_empty() {
        return None;
    }

    Some(format!(
        "Preserve path: keep {} on {}. Open advanced storage, then use Installation Destination with Custom/manual storage or Reclaim Space and select only unallocated free space or a separate disk for Goblins OS.",
        existing_system_kind_names(&target.existing_systems),
        existing_system_partition_list(&target.existing_systems)
    ))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_blocked_detail(target: &InstallTarget) -> String {
    let mut details = target.reasons.clone();
    if let Some(guidance) = dual_boot_disk_guidance(target) {
        details.push(guidance);
    }
    details.join("\n")
}

/// The severity tone a disk-card status line reads in. The whole installer voice is
/// calm, honest status, so the color ladder is deliberately three steps — never an
/// undifferentiated red wash on advisory copy. Each tone maps to one CSS class so the
/// hue is token-driven and themes Light/Dark with the rest of the OS.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Copy)]
enum DiskStatusTone {
    /// Affirmative — the disk is ready to replace as-is (green).
    Ready,
    /// Advisory but actionable — not an error; the row still does something useful
    /// (e.g. opens advanced storage to preserve another OS). Reads neutral/muted.
    Advisory,
    /// Caution — selecting this row is destructive (a whole-disk replace). Amber.
    Caution,
    /// Hard block — the disk genuinely can't be used for installation. Red, reserved.
    Blocked,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
impl DiskStatusTone {
    /// The optional state class layered onto `.gos-install-disk-state`. The default
    /// (no extra class) is the affirmative green, so `Ready` adds nothing.
    fn state_class(self) -> Option<&'static str> {
        match self {
            DiskStatusTone::Ready => None,
            DiskStatusTone::Advisory => Some("is-advisory"),
            DiskStatusTone::Caution => Some("is-caution"),
            DiskStatusTone::Blocked => Some("is-blocked"),
        }
    }
}

/// The single status sentence a disk card shows beneath its identity line, plus the
/// severity tone it reads in. One line — not the old seven-fact stack — so the card
/// scans at a glance; the full per-disk detail is still reachable through
/// [`disk_card_detail_tooltip`] and the page-level "Storage & boot details"
/// disclosure. Returns `(text, tone)`.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn disk_card_status_line(target: &InstallTarget) -> (String, DiskStatusTone) {
    if target.eligible {
        if target.partitions.is_empty() {
            // A truly blank disk needs no caution — the calm one-liner is the story.
            (
                "Blank disk · ready to replace".to_string(),
                DiskStatusTone::Ready,
            )
        } else {
            // A disk with partitions is selectable but selecting it erases the whole
            // disk — a genuine caution, not a hard block. The verbose preservation
            // prose lives in the page-level disclosure.
            (
                "Has partitions · whole-disk replace only — keep another OS through advanced storage".to_string(),
                DiskStatusTone::Caution,
            )
        }
    } else if target_uses_preservation_handoff(target) {
        // A detected existing system is an action, not a dead end: the card states
        // the one thing the user must do, and the row click opens advanced storage.
        // This is advisory-but-actionable — it must not share the alarm-red of a
        // hard block, so it rests in the neutral/muted tone.
        (
            "Existing systems detected · opens advanced storage to preserve them".to_string(),
            DiskStatusTone::Advisory,
        )
    } else if target.reasons.is_empty() {
        (
            "This disk can't be used for installation.".to_string(),
            DiskStatusTone::Blocked,
        )
    } else {
        // A genuinely blocked disk (under-min size, removable USB, unreadable scan)
        // keeps its full disqualifying reason — that is the decisive fact, not
        // advisory prose, so it stays on the card in the reserved alarm-red tone.
        (install_blocked_detail(target), DiskStatusTone::Blocked)
    }
}

/// The full per-disk detail, demoted one hover away from the now-single-line card:
/// partition scan, detected systems, recommended path, dual-boot plan, and (when
/// present) the preservation checklist. Nothing the old fact stack carried is lost.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn disk_card_detail_tooltip(target: &InstallTarget) -> String {
    let mut lines = vec![
        partition_summary(target),
        existing_system_summary(target),
        format!(
            "Recommended path: {}",
            install_recommendation_summary(target)
        ),
        format!("Dual-boot plan: {}", dual_boot_plan_summary(target)),
    ];
    if let Some(checklist) = detected_system_preservation_checklist(target) {
        lines.push(format!("Preservation checklist: {checklist}"));
    }
    lines.join("\n\n")
}

/// A calm key/value row for the Review and Done summaries (an uppercase label over
/// a value), on the shared `gos-row` surface.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn review_row(title: &str, detail: &str) -> gtk4::Box {
    use gtk4::prelude::*;

    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    row.add_css_class("gos-row");
    row.append(&label(title, &["gos-install-row-title"]));

    let lines = review_detail_lines(detail);
    if lines.len() <= 1 {
        let value = label(
            lines.first().map(String::as_str).unwrap_or(detail),
            &["gos-install-row-value"],
        );
        value.set_wrap(true);
        value.set_xalign(0.0);
        row.append(&value);
    } else {
        row.add_css_class("gos-install-row-long");
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 5);
        list.add_css_class("gos-install-row-lines");
        for line in lines {
            let value = label(
                &format!("- {line}"),
                &["gos-install-row-value", "gos-install-row-line"],
            );
            value.set_wrap(true);
            value.set_xalign(0.0);
            list.append(&value);
        }
        row.append(&list);
    }
    row
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn review_detail_lines(detail: &str) -> Vec<String> {
    const MAX_LINE_CHARS: usize = 150;
    const MARKERS: [&str; 16] = [
        " Action:",
        " Target:",
        " Preserve:",
        " Boot:",
        " Finish:",
        " Status:",
        " Steps:",
        " Before:",
        " Installer:",
        " Final check:",
        " Prepare:",
        " Keep:",
        " Opens:",
        " Best for:",
        " Start:",
        " After install:",
    ];

    let mut lines = Vec::new();
    for raw_line in detail.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut fragments = split_at_markers(trimmed, &MARKERS);
        if fragments.len() == 1 && trimmed.chars().count() > MAX_LINE_CHARS {
            fragments = split_sentences(trimmed);
        }

        for fragment in fragments {
            let fragment = fragment.trim();
            if fragment.is_empty() {
                continue;
            }
            if fragment.chars().count() > MAX_LINE_CHARS {
                lines.extend(split_sentences(fragment));
            } else {
                lines.push(fragment.to_string());
            }
        }
    }

    if lines.is_empty() {
        vec![detail.trim().to_string()]
    } else {
        lines
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn split_at_markers(text: &str, markers: &[&str]) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut rest = text.trim();

    loop {
        let next = markers
            .iter()
            .filter_map(|marker| rest.find(marker).map(|index| (index, *marker)))
            .filter(|(index, _)| *index > 0)
            .min_by_key(|(index, _)| *index);

        let Some((index, _marker)) = next else {
            break;
        };
        let head = rest[..index].trim();
        if !head.is_empty() {
            fragments.push(head.to_string());
        }
        rest = rest[(index + 1)..].trim();
    }

    if !rest.is_empty() {
        fragments.push(rest.to_string());
    }

    fragments
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn split_sentences(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;

    for (index, window) in text.as_bytes().windows(2).enumerate() {
        if window == b". " || window == b"; " {
            let end = index + 1;
            let fragment = text[start..end].trim();
            if !fragment.is_empty() {
                lines.push(fragment.to_string());
            }
            start = index + 2;
        }
    }

    let tail = text[start..].trim();
    if !tail.is_empty() {
        lines.push(tail.to_string());
    }

    if lines.is_empty() {
        vec![text.trim().to_string()]
    } else {
        lines
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_dual_boot_guide(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.dual_boot_guide.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Dual-boot guide",
        "Choose the row for the OS you are keeping. The safe target is unallocated free space or a separate disk, verified in the final storage summary before anything is written.",
    ));
    for step in &policy.dual_boot_guide {
        panel.append(&review_row(&step.title, &step.detail));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_target_dual_boot_plan(panel: &gtk4::Box, target: &InstallTarget) {
    use gtk4::prelude::*;

    if target.dual_boot_plan.title.is_empty()
        && target.dual_boot_plan.summary.is_empty()
        && target.dual_boot_plan.steps.is_empty()
    {
        return;
    }

    let heading = if target.dual_boot_plan.status.is_empty() {
        "Dual-boot plan".to_string()
    } else {
        format!(
            "Dual-boot plan · {}",
            dual_boot_status_label(&target.dual_boot_plan.status)
        )
    };
    // macOS Disk Utility / Installer presents storage facts as labeled rows, not a
    // run-on sentence. A real structured plan is broken into Action / Target /
    // Preserve / Bootloader / Finish rows; the no-structured-plan fallback keeps the
    // single guidance line.
    if target.dual_boot_plan.title.is_empty() {
        panel.append(&review_row(&heading, &dual_boot_plan_summary(target)));
    } else {
        let plan = &target.dual_boot_plan;
        panel.append(&review_row(&heading, &plan.summary));
        for (field, value) in [
            ("Action", plan.primary_action.as_str()),
            ("Target", plan.storage_target.as_str()),
            ("Preserve", plan.preserve.as_str()),
            ("Bootloader", plan.bootloader.as_str()),
            ("Finish", plan.finish.as_str()),
        ] {
            if !value.is_empty() {
                panel.append(&review_row(field, value));
            }
        }
    }
    for (index, step) in target.dual_boot_plan.steps.iter().enumerate() {
        panel.append(&review_row(&format!("Step {}", index + 1), step));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_dual_boot_decision_map(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.dual_boot_decision_map.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Dual-boot decision map",
        "Choose the row that matches what you are keeping. The target is always confirmed free space or a separate disk, with existing systems preserved in the final storage summary.",
    ));
    for decision in &policy.dual_boot_decision_map {
        panel.append(&review_row(
            &decision.title,
            &format!(
                "Best for: {} Prepare: {} Target: {} Preserve: {} Boot: {}",
                decision.best_for,
                decision.prepare_space,
                decision.install_target,
                decision.preserve,
                decision.boot_picker
            ),
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_pre_write_install_plan(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.pre_write_install_plan.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Before writing to disk",
        "Review exactly what the installer will change. The simple path is a blank-disk replacement path; dual boot and custom formatting stay in advanced storage.",
    ));
    for item in &policy.pre_write_install_plan {
        panel.append(&review_row(&item.title, &item.detail));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_dual_boot_quick_start(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.dual_boot_quick_start.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Dual-boot quick start",
        "Use this path when keeping Windows, macOS, Linux, another OS, or data. It stays in advanced storage until the final preserve, format, and bootloader summary is correct.",
    ));
    for item in &policy.dual_boot_quick_start {
        panel.append(&review_row(&item.title, &item.detail));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_dual_boot_readiness(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.dual_boot_readiness.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Dual-boot readiness",
        "Use this checklist before writing storage changes. Pick the OS you are keeping, create or choose the Goblins OS target, then confirm both systems boot after install.",
    ));
    for item in &policy.dual_boot_readiness {
        panel.append(&review_row(
            &item.title,
            &format!(
                "Before: {} Installer: {} Finish: {}",
                item.before_install, item.installer_choice, item.final_check
            ),
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_dual_boot_choices(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.dual_boot_choices.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Dual-boot assistant",
        "Pick the operating system you are keeping. Each path starts with a backup, creates free space before install when possible, and finishes by confirming both systems still boot.",
    ));
    for choice in &policy.dual_boot_choices {
        panel.append(&review_row(
            &choice.title,
            &format!(
                "Before: {} Target: {} Keep: {} Finish: {}",
                choice.preparation, choice.install_target, choice.preserve, choice.finish
            ),
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_dual_boot_safe_route(panel: &gtk4::Box, policy: &InstallPolicy, interactive: bool) {
    use gtk4::prelude::*;

    let route = &policy.dual_boot_safe_route;
    if route.title.is_empty() {
        return;
    }

    const DUAL_BOOT_SAFE_ROUTE_TITLE: &str = "Install beside an existing OS";
    let title = if route.title == DUAL_BOOT_SAFE_ROUTE_TITLE {
        DUAL_BOOT_SAFE_ROUTE_TITLE
    } else {
        route.title.as_str()
    };

    panel.append(&review_row(title, &route.summary));
    panel.append(&review_row("Start here", &route.first_screen));

    if interactive && !policy.full_storage_installer.command.is_empty() {
        let action_label = if route.primary_action.is_empty() {
            "Open advanced storage"
        } else {
            route.primary_action.as_str()
        };
        let open = button(action_label, &["gos-onboarding-secondary"]);
        open.set_halign(gtk4::Align::Start);
        let feedback = label("", &["gos-onboarding-footnote"]);
        feedback.set_visible(false);
        feedback.set_wrap(true);
        feedback.set_xalign(0.0);

        let command = policy.full_storage_installer.command.clone();
        let feedback_c = feedback.clone();
        open.connect_clicked(move |_| match launch_full_storage_installer(&command) {
            Ok(()) => {
                feedback_c.set_text(
                    "Advanced storage is opening. Choose free space or a separate Goblins OS disk, then verify preserve, format, and bootloader/EFI rows before writing.",
                );
                feedback_c.set_visible(true);
            }
            Err(error) => {
                feedback_c.set_text(&error);
                feedback_c.set_visible(true);
                eprintln!("installer_dual_boot_safe_route_launch_error={error}");
            }
        });

        panel.append(&open);
        panel.append(&feedback);
    }

    panel.append(&review_row("Choose target", &route.target_rule));
    panel.append(&review_row("Preserve", &route.preserve_rule));
    panel.append(&review_row("Final review", &route.final_review));
    panel.append(&review_row("After install", &route.after_install));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_full_storage_installer_handoff(
    panel: &gtk4::Box,
    policy: &InstallPolicy,
    interactive: bool,
) {
    use gtk4::prelude::*;

    let handoff = &policy.full_storage_installer;
    if handoff.title.is_empty() {
        return;
    }

    panel.append(&review_row(
        &handoff.title,
        &format!(
            "{} Opens: {} Best for: {} Final check: {}",
            handoff.summary, handoff.storage_entry, handoff.safest_for, handoff.final_check
        ),
    ));

    if !interactive {
        return;
    }

    let action_label = if handoff.action_label.is_empty() {
        "Open advanced storage"
    } else {
        handoff.action_label.as_str()
    };
    let open = button(action_label, &["gos-onboarding-secondary"]);
    open.set_halign(gtk4::Align::Start);
    let feedback = label("", &["gos-onboarding-footnote"]);
    feedback.set_visible(false);
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);

    let command = handoff.command.clone();
    let feedback_c = feedback.clone();
    open.connect_clicked(move |_| match launch_full_storage_installer(&command) {
        Ok(()) => {
            feedback_c.set_text(
                "Advanced storage is opening. Verify the final preserve, format, and bootloader/EFI rows before writing.",
            );
            feedback_c.set_visible(true);
        }
        Err(error) => {
            feedback_c.set_text(&error);
            feedback_c.set_visible(true);
            eprintln!("installer_full_storage_launch_error={error}");
        }
    });

    panel.append(&open);
    panel.append(&feedback);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_dual_boot_launcher(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    let handoff = &policy.full_storage_installer;
    if handoff.command.is_empty() && policy.dual_boot_choices.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Install beside another OS",
        "Choose this when you want Goblins OS alongside Windows, macOS, Linux, another OS, or a data disk. It opens advanced storage so you can choose free space or a dedicated disk before anything is written.",
    ));

    let command = handoff.command.clone();
    let primary_label = if handoff.action_label.is_empty() {
        "Open guided storage installer"
    } else {
        handoff.action_label.as_str()
    };
    let open = button(primary_label, &["gos-onboarding-secondary"]);
    open.set_halign(gtk4::Align::Start);
    let feedback = label("", &["gos-onboarding-footnote"]);
    feedback.set_visible(false);
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);
    {
        let command = command.clone();
        let feedback = feedback.clone();
        open.connect_clicked(move |_| match launch_full_storage_installer(&command) {
            Ok(()) => {
                feedback.set_text(
                    "Advanced storage is opening. Choose Installation Destination, then free space or a dedicated disk for Goblins OS.",
                );
                feedback.set_visible(true);
            }
            Err(error) => {
                feedback.set_text(&error);
                feedback.set_visible(true);
                eprintln!("installer_dual_boot_launcher_error={error}");
            }
        });
    }
    panel.append(&open);

    if !policy.dual_boot_choices.is_empty() {
        panel.append(&review_row(
            "What are you keeping?",
            "Pick Windows, macOS, Linux, another OS/data, or a dedicated disk. Each choice opens advanced storage before disk writes and keeps the erase-only flow out of the way.",
        ));
        for choice in &policy.dual_boot_choices {
            let row = gtk4::Button::new();
            row.add_css_class("gos-dual-boot-choice");
            row.set_hexpand(true);
            row.set_tooltip_text(Some(&format!(
                "{}. {}",
                choice.title, choice.install_target
            )));

            let inner = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
            let title = label(&choice.title, &["gos-dual-boot-choice-title"]);
            title.set_xalign(0.0);
            inner.append(&title);
            let detail = label(
                &format!(
                    "Prepare: {} Target: {} Keep: {}",
                    choice.preparation, choice.install_target, choice.preserve
                ),
                &["gos-dual-boot-choice-detail"],
            );
            detail.set_wrap(true);
            detail.set_xalign(0.0);
            inner.append(&detail);
            row.set_child(Some(&inner));

            let command = command.clone();
            let choice_title = choice.title.clone();
            let feedback = feedback.clone();
            row.connect_clicked(move |_| match launch_full_storage_installer(&command) {
                Ok(()) => {
                    feedback.set_text(&format!(
                        "Opening advanced storage for {choice_title}. Select only unallocated free space or a dedicated Goblins OS disk, then verify preserve, format, and bootloader/EFI rows before writing changes."
                    ));
                    feedback.set_visible(true);
                }
                Err(error) => {
                    feedback.set_text(&error);
                    feedback.set_visible(true);
                    eprintln!("installer_dual_boot_choice_launch_error={error}");
                }
            });
            panel.append(&row);
        }
    }

    panel.append(&feedback);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_install_path_options(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.install_path_options.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Recommended install paths",
        "Choose the path that matches what you want to keep. The installer blocks simple erase on disks with existing systems and points dual boot back to manual storage.",
    ));
    for option in &policy.install_path_options {
        panel.append(&review_row(
            &option.title,
            &format!("{} {} {}", option.summary, option.action, option.safety),
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_pre_install_safety(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.pre_install_safety.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Before installing",
        "Complete these checks before resizing storage, choosing a bootloader target, or erasing a disk.",
    ));
    for item in &policy.pre_install_safety {
        panel.append(&review_row(&item.title, &item.detail));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_install_environment(panel: &gtk4::Box, environment: &InstallEnvironment) {
    use gtk4::prelude::*;

    if environment.architecture.is_empty()
        && environment.boot_mode.is_empty()
        && environment.architecture_guidance.is_empty()
        && environment.boot_guidance.is_empty()
    {
        return;
    }

    let supported = if environment.supported_architectures.is_empty() {
        "x86_64 and aarch64".to_string()
    } else {
        environment.supported_architectures.join(" and ")
    };
    let architecture = if environment.architecture.is_empty() {
        "unknown architecture"
    } else {
        environment.architecture.as_str()
    };
    let support_state = if environment.native_supported {
        "native release architecture"
    } else {
        "not a supported release architecture"
    };
    let architecture_detail = if environment.architecture_guidance.is_empty() {
        format!(
            "This installer is running on {architecture}, {support_state}. Goblins OS release ISOs are architecture-specific: {supported}."
        )
    } else {
        format!(
            "This installer is running on {architecture}, {support_state}. {}",
            environment.architecture_guidance
        )
    };

    let boot_mode = if environment.boot_mode.is_empty() {
        "unknown"
    } else {
        match environment.boot_mode.as_str() {
            "uefi" => "UEFI",
            "legacy-or-unknown" => "Legacy BIOS or unknown",
            other => other,
        }
    };
    let secure_boot = if environment.secure_boot.state.is_empty() {
        "unknown"
    } else {
        match environment.secure_boot.state.as_str() {
            "not-uefi" => "not applicable (no UEFI)",
            other => other,
        }
    };
    let secure_detail = if environment.secure_boot.detail.is_empty() {
        "Secure Boot status has not been reported by the installer environment."
    } else {
        environment.secure_boot.detail.as_str()
    };
    // Consolidate the firmware line. When no EFI system is visible the machine is
    // booting in legacy/BIOS mode (or an EFI dir simply is not mounted): collapse
    // the five overlapping "not visible / not available / do not proceed"
    // negations into one neutral, honest line. The stern "do not proceed" hedge
    // is reserved for genuinely ambiguous environments (unsupported architecture),
    // not the ordinary legacy-BIOS-with-eligible-disk case. The calm UEFI branch
    // keeps the existing per-source guidance and Secure Boot detail.
    let boot_detail = if environment.efi_available {
        if environment.boot_guidance.is_empty() {
            format!(
                "Boot mode: {boot_mode}. EFI firmware is visible. Secure Boot: {secure_boot}. {secure_detail}"
            )
        } else {
            format!(
                "Boot mode: {boot_mode}. EFI firmware is visible. Secure Boot: {secure_boot}. {} {}",
                environment.boot_guidance, secure_detail
            )
        }
    } else if environment.native_supported {
        "Boot mode: Legacy BIOS or unmounted EFI. No EFI System Partition was detected from this runtime; this image installs a BIOS/GRUB bootloader and no EFI System Partition is required. Secure Boot: not applicable (Legacy/BIOS boot)."
            .to_string()
    } else {
        // Genuinely ambiguous: unsupported architecture with no visible firmware.
        // Keep the honest hedge to stop before the boot target is understood.
        "Boot mode: Legacy BIOS or unmounted EFI. No EFI System Partition was detected from this runtime. Do not proceed until the installer boot mode and bootloader target are understood for this computer. Secure Boot: not applicable (Legacy/BIOS boot)."
            .to_string()
    };

    panel.append(&review_row(
        "Installer environment",
        "Confirm the ISO architecture, firmware mode, and bootloader target before choosing storage.",
    ));
    panel.append(&review_row("Native architecture", &architecture_detail));
    panel.append(&review_row("Firmware and bootloader", &boot_detail));
}

/// Whether the boot-entry status has anything to render — so a caller can decide
/// whether to draw a surrounding "Boot details" group heading at all, rather than
/// leaving a dangling eyebrow when `append_boot_entries` early-returns on empty.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn boot_entries_have_content(status: &BootEntryStatus) -> bool {
    !(status.detail.is_empty() && status.guidance.is_empty() && status.entries.is_empty())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_boot_entries(panel: &gtk4::Box, status: &BootEntryStatus) {
    use gtk4::prelude::*;

    if !boot_entries_have_content(status) {
        return;
    }

    let detail = if status.detail.is_empty() {
        if status.available {
            "Firmware boot entries are visible, but no entries were reported.".to_string()
        } else {
            "Firmware boot entries are not visible from this runtime.".to_string()
        }
    } else {
        status.detail.clone()
    };

    panel.append(&review_row("Firmware boot entries", &detail));
    for entry in &status.entries {
        let active = if entry.active { "active" } else { "inactive" };
        let target = if entry.target.is_empty() {
            "firmware target not reported".to_string()
        } else {
            format!("target {}", entry.target)
        };
        panel.append(&review_row(
            &format!("Boot{}", entry.id),
            &format!("{} · {active} · {target}", entry.label),
        ));
    }
    if !status.guidance.is_empty() {
        panel.append(&review_row("Boot picker check", &status.guidance));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_storage_review_checklist(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.storage_review_checklist.is_empty() {
        return;
    }

    panel.append(&review_row(
        "Storage review checklist",
        "Check these items in the final summary before any disk write.",
    ));
    for item in &policy.storage_review_checklist {
        panel.append(&review_row(&item.title, &item.detail));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_post_install_verification(panel: &gtk4::Box, policy: &InstallPolicy) {
    use gtk4::prelude::*;

    if policy.post_install_verification.is_empty() {
        return;
    }

    panel.append(&review_row(
        "After install",
        "Before changing boot order, confirm Goblins OS and every system you kept can still start.",
    ));
    for item in &policy.post_install_verification {
        panel.append(&review_row(&item.title, &item.detail));
    }
}

/// Screen 1 — choose the disk. Reuses the network-step card; lists every detected
/// disk; eligible disks are selectable, ineligible disks are inert with the
/// core's own reason. `Continue` enables only once an eligible disk is chosen.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn populate_install_disk(
    pages: &InstallPages,
    stack: &gtk4::Stack,
    flow: &InstallFlow,
    core_url: &str,
) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let column = install_scaffold(&pages.disk);

    let back = button("← Welcome", &["gos-onboarding-quiet"]);
    back.set_halign(gtk::Align::Start);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("welcome"));
    }
    column.append(&back);

    let mark = goblins_os_ui::themed_brand_mark(60);
    mark.set_margin_top(6);
    mark.set_margin_bottom(18);
    column.append(&mark);

    column.append(&centered_label(
        "Step 1 of 3 · Choose disk",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label(
        "Choose your disk",
        "gos-onboarding-title",
        false,
    ));
    column.append(&centered_label(
        "Keep another OS with advanced storage. Use a disk row only when replacing one blank disk after typed confirmation.",
        "gos-onboarding-subtitle",
        true,
    ));

    let targets = flow.state.install_targets.as_ref();
    let bootc_available = targets
        .map(|status| status.bootc.available)
        .unwrap_or(false);
    let bootc_privileged = targets
        .map(|status| status.bootc.privileged)
        .unwrap_or(false);
    let any_eligible = targets
        .map(|status| status.targets.iter().any(|target| target.eligible))
        .unwrap_or(false);

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("gos-net-panel");
    panel.set_size_request(580, -1);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("gos-net-dot");
    dot.set_size_request(10, 10);
    dot.set_valign(gtk::Align::Center);
    if any_eligible {
        dot.add_css_class("is-online");
    } else {
        dot.add_css_class("is-blocked");
    }
    let status_text = if any_eligible {
        // The page header summarizes the scan (rows carry their own per-disk
        // "Ready to install" status) — and a mixed list is not all-ready, so
        // this reads as a neutral scan result, not a "safe to erase now" claim.
        "Eligible disk found — review the path below"
    } else if !bootc_available {
        "Installation runs from the Goblins OS installer image"
    } else if !bootc_privileged {
        "Installation requires administrator privileges"
    } else {
        "No eligible disks"
    };
    header.append(&dot);
    header.append(&label(status_text, &["gos-net-status-label"]));
    header.append(&spacer());
    panel.append(&header);

    // macOS Recovery surfaces the selectable target first. Lead with the disk
    // list so the one action this page is named for is reachable without
    // scrolling; all advisory prose is demoted beneath the selection.
    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    panel.append(&list);

    // The card carries ONLY the scan header and the selectable disk list, so the
    // one action this page is named for leads the first viewport. Every advisory
    // sentence — the dual-boot rule, the safe route, environment, boot entries,
    // formatting, and the pre/post checklists — is collected into one collapsed
    // "Storage & boot details" disclosure that renders below the Continue action.
    let disclosure = if let Some(status) = targets {
        let details = gtk::Box::new(gtk::Orientation::Vertical, 10);

        // State the keep-another-OS rule exactly once, at the top of the details,
        // then keep the interactive safe-route launcher right beneath it. The
        // verbose review rows below never repeat the OS list again.
        details.append(&review_row(
            "Keeping another OS or data?",
            "If you are keeping Windows, macOS, Linux, another OS, recovery, EFI, vendor partitions, or shared data, start with advanced storage. Disk rows replace one blank disk only after typed confirmation.",
        ));
        append_dual_boot_safe_route(&details, &status.policy, true);

        if !status.policy.simple_install_scope.is_empty() {
            details.append(&review_row(
                "Replace one blank disk",
                &status.policy.simple_install_scope,
            ));
        }
        // The canonical preservation rule, with the full OS list stated here once —
        // this is the single place the verbatim clause lives, instead of the five-plus
        // repetitions the old wall carried.
        details.append(&review_row(
            "Keep an existing OS",
            "Windows, macOS, Linux, another OS, recovery, and EFI partitions stay untouched only when you use advanced storage.",
        ));
        details.append(&review_row(
            "Choose install path",
            "To dual boot, choose the OS you are keeping first. The advanced storage opens before any disk writes.",
        ));
        details.append(&review_row(
            "Detected systems are actions",
            "Rows that show an existing OS, recovery, EFI, or data open advanced storage instead of erase confirmation.",
        ));
        details.append(&review_row(
            "Unsure? Keep your current OS",
            "Open advanced storage and confirm the final summary shows what is preserved and what is formatted.",
        ));
        details.append(&review_row(
            "Use this screen when",
            "You want Goblins OS to replace one blank internal disk. Disks with existing OS, recovery, EFI, or data partitions are routed to manual storage.",
        ));
        append_install_environment(&details, &status.environment);
        append_boot_entries(&details, &status.boot_entries);
        append_dual_boot_launcher(&details, &status.policy);
        append_full_storage_installer_handoff(&details, &status.policy, false);
        append_install_path_options(&details, &status.policy);
        append_pre_install_safety(&details, &status.policy);
        if !status.policy.dual_boot_preflight.is_empty() {
            details.append(&review_row(
                "Before dual boot",
                &status.policy.dual_boot_preflight,
            ));
        }
        details.append(&review_row(
            "Best dual-boot path",
            &status.policy.dual_boot_preservation,
        ));
        if !status.policy.dual_boot_handoff.is_empty() {
            details.append(&review_row(
                "Keep your current OS",
                &status.policy.dual_boot_handoff,
            ));
        }
        append_pre_write_install_plan(&details, &status.policy);
        details.append(&review_row(
            "Boot and formatting",
            &status.policy.bootloader,
        ));
        if !status.policy.formatting_guidance.is_empty() {
            details.append(&review_row(
                "Formatting",
                &status.policy.formatting_guidance,
            ));
        }
        if !status.policy.advanced_storage_guidance.is_empty() {
            details.append(&review_row(
                "Advanced storage",
                &status.policy.advanced_storage_guidance,
            ));
        }
        if !status.policy.bootloader_recovery.is_empty() {
            details.append(&review_row(
                "After reboot",
                &status.policy.bootloader_recovery,
            ));
        }
        append_storage_review_checklist(&details, &status.policy);
        append_post_install_verification(&details, &status.policy);

        let disclosure = gtk::Expander::new(Some("Storage & boot details"));
        disclosure.set_expanded(false);
        disclosure.set_child(Some(&details));
        Some(disclosure)
    } else {
        None
    };

    column.append(&panel);

    let cont = button("Continue", &["gos-onboarding-primary"]);
    cont.set_halign(gtk::Align::Center);
    cont.set_margin_top(22);
    cont.set_sensitive(flow.selected.borrow().is_some());

    let mut shown = 0;
    if let Some(status) = targets {
        for target in status.targets.iter() {
            shown += 1;
            let preservation_handoff = target_uses_preservation_handoff(target);
            let row = gtk::Button::new();
            row.add_css_class("gos-install-disk");
            row.add_css_class(if target.eligible {
                "is-eligible"
            } else {
                "is-blocked"
            });
            row.set_sensitive(target.eligible || preservation_handoff);
            row.set_hexpand(true);

            // The card is two stacked blocks: a tight identity block (name +
            // device path) on top, then an evenly-spaced facts block beneath.
            // Splitting them — instead of one flat 4px stack of seven labels —
            // gives the scan an anchor and a rhythm so the facts no longer read
            // as a wall of same-size text.
            let inner = gtk::Box::new(gtk::Orientation::Vertical, 10);
            let identity = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let titleline = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            titleline.append(&label(
                &format!("{} · {} GB", target.model, target.size_gb),
                &["gos-install-disk-model"],
            ));
            titleline.append(&spacer());
            titleline.append(&label(disk_kind(target), &["gos-install-disk-kind"]));
            identity.append(&titleline);
            identity.append(&label(&target.path, &["gos-install-disk-path"]));
            inner.append(&identity);

            // The card shows ONE status line, not a seven-line fact stack: a calm
            // identity-plus-status reading the eye can scan in a glance. The verbose
            // per-disk facts (partition scan, detected systems, recommended path,
            // dual-boot plan) are real and preserved — they move into the row's
            // tooltip and the page-level "Storage & boot details" disclosure — but
            // the card itself carries only the single sentence that decides the row.
            let facts = gtk::Box::new(gtk::Orientation::Vertical, 7);
            let (status_text, status_tone) = disk_card_status_line(target);
            let status_line = label(&status_text, &["gos-install-disk-state"]);
            // Wrap to a second line inside the card instead of clipping the load-
            // bearing tail of the sentence at the right inset. The label is given
            // horizontal expansion and a word-char wrap bound so it fills the card
            // width and breaks cleanly rather than measuring its full single-line
            // natural width (which a GtkButton child otherwise does, hard-clipping
            // the verb phrase "preserve them" against the right edge).
            status_line.set_wrap(true);
            status_line.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            status_line.set_hexpand(true);
            status_line.set_max_width_chars(40);
            status_line.set_xalign(0.0);
            if let Some(tone_class) = status_tone.state_class() {
                status_line.add_css_class(tone_class);
            }
            facts.append(&status_line);
            inner.append(&facts);
            let preservation_feedback = if preservation_handoff {
                let prompt = preservation_handoff_prompt(target);
                // The handoff prompt rides the same advisory (neutral) tone as the
                // status line above it — preserving another OS is an action, not an
                // error, so it never wears the reserved alarm-red.
                let prompt = label(&prompt, &["gos-install-disk-state", "is-advisory"]);
                prompt.set_wrap(true);
                prompt.set_xalign(0.0);
                inner.append(&prompt);
                Some(prompt)
            } else {
                None
            };
            // The full per-disk detail still lives one hover away — partition scan,
            // detected systems, recommended path, dual-boot plan, and (when present)
            // the preservation checklist — so nothing the old fact stack carried is
            // lost; it is demoted, not deleted.
            row.set_tooltip_text(Some(&disk_card_detail_tooltip(target)));
            row.set_child(Some(&inner));

            if target.eligible {
                let list_c = list.clone();
                let cont_c = cont.clone();
                let flow_c = flow.clone();
                let target_c = target.clone();
                row.connect_clicked(move |this| {
                    let mut child = list_c.first_child();
                    while let Some(widget) = child {
                        widget.remove_css_class("is-selected");
                        child = widget.next_sibling();
                    }
                    this.add_css_class("is-selected");
                    *flow_c.selected.borrow_mut() = Some(target_c.clone());
                    cont_c.set_sensitive(true);
                });
            } else if preservation_handoff {
                let command = status.policy.full_storage_installer.command.clone();
                let feedback = preservation_feedback.expect("preservation feedback label");
                row.connect_clicked(move |_| match launch_full_storage_installer(&command) {
                    Ok(()) => {
                        feedback.set_text(
                            "Advanced storage is opening for this disk. Verify preserve, format, and bootloader/EFI rows before writing.",
                        );
                    }
                    Err(error) => {
                        feedback.set_text(&error);
                        eprintln!("installer_detected_disk_full_storage_launch_error={error}");
                    }
                });
            }
            list.append(&row);
        }
    }

    if shown == 0 {
        let empty = label(
            "No eligible blank disks were found. For dual boot, open advanced storage, choose Installation Destination with Custom/manual storage or Reclaim Space, then choose unallocated free space or a separate disk.",
            &["gos-net-row"],
        );
        empty.set_wrap(true);
        empty.set_xalign(0.0);
        list.append(&empty);
    }

    {
        let pages_c = pages.clone();
        let stack_c = stack.clone();
        let flow_c = flow.clone();
        let core_url_c = core_url.to_string();
        cont.connect_clicked(move |_| {
            populate_install_review(&pages_c, &stack_c, &flow_c, &core_url_c);
            stack_c.set_visible_child_name("install-review");
        });
    }
    column.append(&cont);

    // Keeping another OS? One short pointer (not a sixth restatement of the rule)
    // directs to the safe route, which now lives in full inside the disclosure.
    column.append(&centered_label(
        "Keeping another OS? Open Storage & boot details below for the advanced-storage route.",
        "gos-onboarding-footnote",
        true,
    ));

    // The advisory disclosure renders last — below Continue — so the disk list and
    // the action it advances always lead the first viewport.
    if let Some(disclosure) = disclosure {
        disclosure.set_margin_top(18);
        disclosure.set_halign(gtk::Align::Center);
        disclosure.set_size_request(580, -1);
        column.append(&disclosure);
    }
}

/// Screen 2 — review exactly what will happen, with the chosen device as the
/// focal value. No core call; advances to the typed confirmation.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn populate_install_review(
    pages: &InstallPages,
    stack: &gtk4::Stack,
    flow: &InstallFlow,
    core_url: &str,
) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let column = install_scaffold(&pages.review);

    let back = button("← Change disk", &["gos-onboarding-quiet"]);
    back.set_halign(gtk::Align::Start);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("install-disk"));
    }
    column.append(&back);

    let mark = goblins_os_ui::themed_brand_mark(60);
    mark.set_margin_top(6);
    mark.set_margin_bottom(18);
    column.append(&mark);

    column.append(&centered_label(
        "Step 2 of 3 · Review",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label(
        "Review the install",
        "gos-onboarding-title",
        false,
    ));
    column.append(&centered_label(
        "Here is exactly what Goblins OS will do.",
        "gos-onboarding-subtitle",
        true,
    ));

    let selected = flow.selected.borrow().clone();
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("gos-net-panel");
    // Width-matched to the confirm step's card so the wizard keeps one rhythm.
    panel.set_size_request(820, -1);
    match &selected {
        Some(target) => {
            // Lead with the device being erased — Disk/Drive/partitions/action.
            // The installer environment (ISO architecture, firmware mode) is
            // demoted into the firmware/boot policy group below so this card
            // reads as a confirmation, not as documentation.
            panel.append(&review_row("Disk", &target.path));
            panel.append(&review_row(
                "Drive",
                &format!(
                    "{} · {} GB · {}",
                    target.model,
                    target.size_gb,
                    disk_kind(target)
                ),
            ));
            panel.append(&review_row(
                "Existing partitions",
                &partition_summary(target),
            ));
            panel.append(&review_row(
                "Detected systems",
                &existing_system_summary(target),
            ));
            panel.append(&review_row(
                "Recommended path",
                &install_recommendation_summary(target),
            ));
            append_target_dual_boot_plan(&panel, target);
            panel.append(&review_row(
                "Action",
                "Erase the entire disk and write a fresh GPT layout. This does not preserve Windows, macOS, Linux, another OS, recovery, or EFI partitions on this disk.",
            ));
            panel.append(&review_row(
                "Root filesystem",
                "xfs · immutable system image",
            ));
            if let Some(status) = flow.state.install_targets.as_ref() {
                // Firmware/boot context follows the device identity and action,
                // grouped with the bootloader/dual-boot policy rows.
                append_install_environment(&panel, &status.environment);
                append_boot_entries(&panel, &status.boot_entries);
                append_dual_boot_safe_route(&panel, &status.policy, false);
                append_install_path_options(&panel, &status.policy);
                append_dual_boot_quick_start(&panel, &status.policy);
                append_pre_install_safety(&panel, &status.policy);
                append_dual_boot_readiness(&panel, &status.policy);
                append_dual_boot_choices(&panel, &status.policy);
                append_dual_boot_decision_map(&panel, &status.policy);
                append_pre_write_install_plan(&panel, &status.policy);
                panel.append(&review_row("Bootloader", &status.policy.bootloader));
                if !status.policy.dual_boot_preflight.is_empty() {
                    panel.append(&review_row(
                        "Dual-boot preflight",
                        &status.policy.dual_boot_preflight,
                    ));
                }
                if !status.policy.formatting_guidance.is_empty() {
                    panel.append(&review_row(
                        "Formatting",
                        &status.policy.formatting_guidance,
                    ));
                }
                if !status.policy.simple_install_scope.is_empty() {
                    panel.append(&review_row(
                        "Erase scope",
                        &status.policy.simple_install_scope,
                    ));
                }
                if !status.policy.advanced_storage_guidance.is_empty() {
                    panel.append(&review_row(
                        "Advanced storage",
                        &status.policy.advanced_storage_guidance,
                    ));
                }
                if !status.policy.bootloader_recovery.is_empty() {
                    panel.append(&review_row(
                        "After reboot",
                        &status.policy.bootloader_recovery,
                    ));
                }
                append_storage_review_checklist(&panel, &status.policy);
                append_post_install_verification(&panel, &status.policy);
                panel.append(&review_row(
                    "Keep another OS",
                    &status.policy.dual_boot_preservation,
                ));
                if !status.policy.dual_boot_handoff.is_empty() {
                    panel.append(&review_row(
                        "Dual-boot handoff",
                        &status.policy.dual_boot_handoff,
                    ));
                }
                append_full_storage_installer_handoff(&panel, &status.policy, false);
                append_dual_boot_guide(&panel, &status.policy);
            }
            if !target.existing_systems.is_empty() {
                if let Some(checklist) = detected_system_preservation_checklist(target) {
                    panel.append(&review_row(
                        "Detected OS preservation checklist",
                        &checklist,
                    ));
                }
                panel.append(&review_row(
                    "Preservation details",
                    &existing_system_details(target),
                ));
            }
            panel.append(&review_row(
                "Local models",
                "OpenAI open-weight files download later, after you consent — not part of the image",
            ));
        }
        None => panel.append(&review_row("Disk", "the selected disk")),
    }
    column.append(&panel);

    let cont = button("Continue to confirmation", &["gos-onboarding-primary"]);
    cont.set_halign(gtk::Align::Center);
    cont.set_margin_top(22);
    cont.set_sensitive(selected.is_some());
    {
        let pages_c = pages.clone();
        let stack_c = stack.clone();
        let flow_c = flow.clone();
        let core_url_c = core_url.to_string();
        cont.connect_clicked(move |_| {
            populate_install_confirm(&pages_c, &stack_c, &flow_c, &core_url_c);
            stack_c.set_visible_child_name("install-confirm");
        });
    }
    column.append(&cont);

    column.append(&centered_label(
        "This path replaces the selected disk. To keep Windows, macOS, Linux, or another OS, go back and use advanced storage instead.",
        "gos-onboarding-footnote",
        true,
    ));
}

/// Screen 3 — the destructive confirmation. The user types the exact, device-
/// anchored phrase; the install button enables only on an exact match, and the
/// core independently re-verifies the phrase, eligibility, root, and the env gate.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn populate_install_confirm(
    pages: &InstallPages,
    stack: &gtk4::Stack,
    flow: &InstallFlow,
    core_url: &str,
) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let column = install_scaffold(&pages.confirm);

    let back = button("← Back", &["gos-onboarding-quiet"]);
    back.set_halign(gtk::Align::Start);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("install-review"));
    }
    column.append(&back);

    // Confirm is the tallest single-screen step (hero + phrase card + entry + helper
    // + CTA + closing line), so its vertical rhythm is tightened a touch versus the
    // other steps to keep the whole erase confirmation — including the closing
    // reassurance line — within the default viewport rather than clipped at the fold.
    let mark = goblins_os_ui::themed_brand_mark(56);
    mark.set_margin_top(2);
    mark.set_margin_bottom(12);
    column.append(&mark);

    let selected = flow.selected.borrow().clone();
    let (device, drive_desc) = match &selected {
        Some(target) => (
            target.path.clone(),
            format!("{}, {} GB", target.model, target.size_gb),
        ),
        None => (
            "the selected disk".to_string(),
            "the selected disk".to_string(),
        ),
    };
    let expected = format!("WIPE {device} AND INSTALL GOBLINS OS");

    column.append(&centered_label(
        "Step 3 of 3 · Confirm",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label(
        "This erases the disk",
        "gos-onboarding-title",
        false,
    ));
    column.append(&centered_label(
        &format!(
            "To erase {device} ({drive_desc}), including any Windows, macOS, Linux, other OS, recovery, and EFI partitions on that disk, type this phrase exactly."
        ),
        "gos-onboarding-subtitle",
        true,
    ));

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    panel.add_css_class("gos-net-panel");
    panel.set_size_request(580, -1);
    panel.append(&label("Required Confirmation", &["gos-onboarding-kicker"]));

    let phrase = label(&expected, &["gos-install-ack-phrase"]);
    phrase.set_selectable(true);
    phrase.set_wrap(true);
    phrase.set_xalign(0.0);
    panel.append(&phrase);

    let entry = gtk::Entry::new();
    entry.add_css_class("gos-install-ack-entry");
    entry.set_placeholder_text(Some("Type the phrase above"));
    panel.append(&entry);

    let helper = label(
        "The phrase includes your disk path so you cannot erase the wrong device by accident. To keep another OS, stop here and open advanced storage with Custom/manual storage or Reclaim Space.",
        &["gos-net-helper"],
    );
    helper.set_wrap(true);
    helper.set_xalign(0.0);
    panel.append(&helper);
    // The destructive action and its phrase field must stay in view — macOS keeps
    // erase confirmations on a single screen. The dual-boot plan + verbose storage/
    // boot facts go behind a collapsed "Storage & boot details" disclosure (the same
    // pattern the review screen uses), so "Erase disk and install" is never pushed
    // below the fold. The critical erase scope is already stated in the hero subtitle.
    let details = gtk::Box::new(gtk::Orientation::Vertical, 10);
    if let Some(target) = selected.as_ref() {
        append_target_dual_boot_plan(&details, target);
    }
    if let Some(status) = flow.state.install_targets.as_ref() {
        append_install_environment(&details, &status.environment);
        append_boot_entries(&details, &status.boot_entries);
        append_pre_install_safety(&details, &status.policy);
        if !status.policy.simple_install_scope.is_empty() {
            details.append(&review_row(
                "Erase scope",
                &status.policy.simple_install_scope,
            ));
        }
        if !status.policy.formatting_guidance.is_empty() {
            details.append(&review_row(
                "Formatting",
                &status.policy.formatting_guidance,
            ));
        }
        append_pre_write_install_plan(&details, &status.policy);
        if !status.policy.bootloader_recovery.is_empty() {
            details.append(&review_row(
                "After reboot",
                &status.policy.bootloader_recovery,
            ));
        }
        append_post_install_verification(&details, &status.policy);
    }
    let disclosure = gtk::Expander::new(Some("Storage & boot details"));
    disclosure.set_expanded(false);
    disclosure.set_margin_top(8);
    disclosure.set_child(Some(&details));
    panel.append(&disclosure);
    column.append(&panel);

    let install = button(
        "Erase disk and install",
        &["gos-onboarding-primary", "gos-onboarding-destructive"],
    );
    install.set_halign(gtk::Align::Center);
    install.set_margin_top(16);
    install.set_sensitive(false);

    let feedback = label("", &["gos-net-row", "gos-blocked-soft"]);
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);
    feedback.set_visible(false);

    // Live, prefix-aware feedback: neutral while the typed text is still a correct
    // prefix, a calm ready tint on an exact match (which enables the button), and a
    // gentle blocked tint only when a wrong character is typed.
    {
        let expected_c = expected.clone();
        let install_c = install.clone();
        entry.connect_changed(move |entry| {
            let typed = entry.text();
            // Forgive stray surrounding whitespace (a pasted phrase with a trailing
            // space or newline) — never the path or words inside the phrase.
            let typed = typed.trim();
            entry.remove_css_class("is-matching");
            entry.remove_css_class("is-diverged");
            if typed == expected_c {
                entry.add_css_class("is-matching");
                install_c.set_sensitive(true);
            } else {
                install_c.set_sensitive(false);
                if !typed.is_empty() && !expected_c.starts_with(typed) {
                    entry.add_css_class("is-diverged");
                }
            }
        });
    }

    {
        let pages_c = pages.clone();
        let stack_c = stack.clone();
        let flow_c = flow.clone();
        let core_url_c = core_url.to_string();
        let device_c = device.clone();
        let entry_c = entry.clone();
        let feedback_c = feedback.clone();
        install.connect_clicked(move |_| {
            start_install(
                &pages_c,
                &stack_c,
                &flow_c,
                &core_url_c,
                &device_c,
                entry_c.text().trim(),
                &feedback_c,
            );
        });
    }
    {
        let pages_c = pages.clone();
        let stack_c = stack.clone();
        let flow_c = flow.clone();
        let core_url_c = core_url.to_string();
        let device_c = device.clone();
        let feedback_c = feedback.clone();
        let install_c = install.clone();
        entry.connect_activate(move |entry| {
            if install_c.is_sensitive() {
                start_install(
                    &pages_c,
                    &stack_c,
                    &flow_c,
                    &core_url_c,
                    &device_c,
                    entry.text().trim(),
                    &feedback_c,
                );
            }
        });
    }
    column.append(&install);
    column.append(&feedback);
    // The closing reassurance line carries its own bottom gutter so it always clears
    // the window chrome instead of bleeding against the fold on the confirm step.
    let closing = centered_label(
        "Once you confirm, the install runs to completion.",
        "gos-onboarding-footnote",
        true,
    );
    closing.set_margin_bottom(12);
    column.append(&closing);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn launch_full_storage_installer(configured_command: &str) -> Result<(), String> {
    let mut candidates = Vec::new();
    if let Some(command) = StorageInstallerCommand::parse(configured_command) {
        candidates.push(command);
    }
    candidates.extend([
        StorageInstallerCommand::new("/usr/libexec/goblins-os/goblins-os-full-installer"),
        StorageInstallerCommand::new("goblins-os-full-installer"),
        StorageInstallerCommand::new("liveinst"),
        StorageInstallerCommand::new("/usr/bin/liveinst"),
        StorageInstallerCommand::with_args("anaconda", &["--liveinst"]),
        StorageInstallerCommand::with_args("/usr/bin/anaconda", &["--liveinst"]),
    ]);

    let mut failures = Vec::new();
    for candidate in candidates {
        if candidate.program.starts_with('/') && !std::path::Path::new(&candidate.program).exists()
        {
            continue;
        }

        match std::process::Command::new(&candidate.program)
            .args(&candidate.args)
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failures.push(format!("{}: {error}", candidate.display())),
        }
    }

    if failures.is_empty() {
        Err("Advanced storage is not available in this live image. Simple install remains disabled for dual boot, custom partitions, scan-unknown disks, encryption, or a non-blank dedicated disk; reboot from Goblins OS install media and choose Install Goblins OS Beside Another OS.".to_string())
    } else {
        Err("Advanced storage could not start in this live image. No disk was changed; simple install remains disabled for dual boot, custom storage, and scan-unknown disks. Reboot from Goblins OS install media and choose Install Goblins OS Beside Another OS.".to_string())
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone)]
struct StorageInstallerCommand {
    program: String,
    args: Vec<String>,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
impl StorageInstallerCommand {
    fn new(program: &str) -> Self {
        Self {
            program: program.to_string(),
            args: Vec::new(),
        }
    }

    fn with_args(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
        }
    }

    fn parse(command: &str) -> Option<Self> {
        let mut parts = command.split_whitespace();
        let program = parts.next()?.to_string();
        let args = parts.map(str::to_string).collect();
        Some(Self { program, args })
    }

    fn display(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

/// Send the guarded execute request and route on the core's response. The button
/// is already gated on an exact typed phrase; the core re-checks every guard, so
/// this can never bypass a safety gate.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn start_install(
    pages: &InstallPages,
    stack: &gtk4::Stack,
    flow: &InstallFlow,
    core_url: &str,
    device: &str,
    acknowledgement: &str,
    feedback: &gtk4::Label,
) {
    use gtk4::prelude::*;

    // Defense in depth: never POST execute without a real, eligible selection.
    if flow.selected.borrow().is_none() {
        return;
    }

    match execute_install(core_url, device, acknowledgement) {
        // Only show "installing" when the core actually reports it started — a
        // 202 whose body says `started`. Any other 2xx (e.g. a 200 dry-run state)
        // is NOT a running install and must not show the progress screen.
        Ok((202, state, _)) if state == "started" => {
            populate_install_progress(pages, stack, flow, core_url);
            stack.set_visible_child_name("install-progress");
        }
        Ok((500, _, detail)) => {
            // The install service failed before disk writes; keep the user on a
            // no-disk-changed path and show the service detail.
            *flow.last_error.borrow_mut() = Some(detail);
            populate_install_done(pages, stack, flow, core_url);
            stack.set_visible_child_name("install-done");
        }
        Ok((_, _, detail)) => {
            // 400/403/409 (or an unexpected non-started 2xx) — nothing was wiped;
            // show the core's own reason and stay on the confirm screen.
            feedback.set_text(&detail);
            feedback.set_visible(true);
        }
        Err(error) => {
            feedback.set_text("Goblins OS could not reach local OS services to start the install.");
            feedback.set_visible(true);
            eprintln!("installer_execute_error={error}");
        }
    }
}

/// Screen 4 — installing. Three breathing dots and one honest phase line; never a
/// fabricated percentage or timer. The phase text is only ever the latest real
/// line bootc printed, polled from the core; a terminal state advances to Done.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn populate_install_progress(
    pages: &InstallPages,
    stack: &gtk4::Stack,
    flow: &InstallFlow,
    core_url: &str,
) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let column = install_scaffold(&pages.progress);

    // The same mark recipe as every sibling install step.
    let mark = goblins_os_ui::themed_brand_mark(60);
    mark.set_margin_top(6);
    mark.set_margin_bottom(18);
    column.append(&mark);

    // Every wizard step carries its step kicker — this one included.
    column.append(&centered_label(
        "Step · Installing",
        "gos-onboarding-kicker",
        false,
    ));
    column.append(&centered_label(
        "Installing Goblins OS",
        "gos-onboarding-title",
        false,
    ));
    column.append(&centered_label(
        "Keep this computer plugged in. It will be ready in a few minutes.",
        "gos-onboarding-subtitle",
        true,
    ));

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 16);
    panel.add_css_class("gos-net-panel");
    panel.set_size_request(580, -1);

    // The OS-wide "thinking" pulse: three dots breathing on the frame clock, phase-
    // staggered into a calm left-to-right wave (opacity only; the resting shape is
    // the shared gos-thinking-dot rule).
    let thinking = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    thinking.add_css_class("gos-thinking");
    thinking.set_halign(gtk::Align::Center);
    for index in 0..3 {
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("gos-thinking-dot");
        dot.set_size_request(9, 9);
        let offset = f64::from(index) * (1.1 / 3.0);
        let _animation = dot.add_tick_callback(move |dot, clock| {
            let seconds = clock.frame_time() as f64 / 1_000_000.0;
            let omega = std::f64::consts::TAU / 1.1;
            let phase = (((seconds + offset) * omega).sin() * 0.5) + 0.5;
            dot.set_opacity(0.28 + 0.72 * phase);
            gtk::glib::ControlFlow::Continue
        });
        thinking.append(&dot);
    }
    panel.append(&thinking);

    let phase = centered_label("Starting the installer…", "gos-install-phase", true);
    panel.append(&phase);
    column.append(&panel);

    column.append(&centered_label(
        "Don’t power off or disconnect the disk while Goblins OS installs.",
        "gos-install-warning",
        true,
    ));

    // Poll the honest progress endpoint. Show only real phase text; advance on a
    // terminal state. If the endpoint is absent (404) stop quietly — the dots keep
    // breathing and the user restarts when the machine is ready; a transient error
    // keeps polling without ever inventing a phase.
    //
    // The fetch is blocking HTTP, so it runs on a worker thread (only a cloned
    // `core_url` String and the mpsc Sender cross the boundary — both Send; every
    // Rc/RefCell/GTK handle stays on this main thread). The main-thread tick drains
    // the channel and re-arms the next fetch only after the prior result lands, so
    // blocking calls never overlap.
    {
        use std::sync::mpsc;

        let (tx, rx) = mpsc::channel::<Result<InstallProgress, CoreFetchError>>();
        let core_url_owned = core_url.to_string();
        let spawn_fetch = move |tx: mpsc::Sender<Result<InstallProgress, CoreFetchError>>| {
            let core_url_c = core_url_owned.clone();
            thread::spawn(move || {
                let result = get_core_json::<InstallProgress>(
                    &core_url_c,
                    "/v1/installer/install-targets/progress",
                );
                // Receiver gone means the page advanced; nothing to do.
                let _ = tx.send(result);
            });
        };

        spawn_fetch(tx.clone());

        let core_url_done = core_url.to_string();
        let phase_c = phase.clone();
        let pages_c = pages.clone();
        let stack_c = stack.clone();
        let flow_c = flow.clone();
        let _poll = gtk::glib::timeout_add_local(Duration::from_millis(800), move || {
            // Stop the moment this page is no longer showing — once we advance to
            // Done (or anywhere), the poll must not keep firing against widgets that
            // have been rebuilt out from under it.
            if stack_c.visible_child_name().as_deref() != Some("install-progress") {
                return gtk::glib::ControlFlow::Break;
            }
            // Drain whatever the worker reported; if nothing has landed yet, keep
            // breathing and check again on the next tick (no new fetch is armed
            // while one is still in flight).
            match rx.try_recv() {
                Ok(Ok(progress)) => match progress.state.as_str() {
                    "succeeded" => {
                        *flow_c.last_error.borrow_mut() = None;
                        populate_install_done(&pages_c, &stack_c, &flow_c, &core_url_done);
                        stack_c.set_visible_child_name("install-done");
                        gtk::glib::ControlFlow::Break
                    }
                    "failed" => {
                        let detail = if progress.phase.is_empty() {
                            "The disk install reported a failure.".to_string()
                        } else {
                            progress.phase.clone()
                        };
                        *flow_c.last_error.borrow_mut() = Some(detail);
                        populate_install_done(&pages_c, &stack_c, &flow_c, &core_url_done);
                        stack_c.set_visible_child_name("install-done");
                        gtk::glib::ControlFlow::Break
                    }
                    "running" => {
                        if !progress.phase.is_empty() {
                            phase_c.set_text(&progress.phase);
                        }
                        spawn_fetch(tx.clone());
                        gtk::glib::ControlFlow::Continue
                    }
                    _ => {
                        spawn_fetch(tx.clone());
                        gtk::glib::ControlFlow::Continue
                    }
                },
                Ok(Err(CoreFetchError::Status(404))) => gtk::glib::ControlFlow::Break,
                Ok(Err(_)) => {
                    spawn_fetch(tx.clone());
                    gtk::glib::ControlFlow::Continue
                }
                // Still fetching — let it land on a later tick.
                Err(mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                // Worker thread vanished without sending; re-arm to recover.
                Err(mpsc::TryRecvError::Disconnected) => {
                    spawn_fetch(tx.clone());
                    gtk::glib::ControlFlow::Continue
                }
            }
        });
    }
}

/// Screen 5 — done. Success shows numbered next steps and a restart; failure shows
/// the core's verbatim reason and a path back to choose another disk. Both calm.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn populate_install_done(
    pages: &InstallPages,
    stack: &gtk4::Stack,
    flow: &InstallFlow,
    core_url: &str,
) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let column = install_scaffold(&pages.done);

    let selected = flow.selected.borrow().clone();
    let device = selected
        .as_ref()
        .map(|target| target.path.clone())
        .unwrap_or_else(|| "this computer".to_string());
    let model = selected
        .as_ref()
        .map(|target| target.model.clone())
        .unwrap_or_default();
    let failure = flow.last_error.borrow().clone();

    if let Some(detail) = failure {
        let mark = goblins_os_ui::themed_brand_mark(60);
        mark.set_margin_top(6);
        mark.set_margin_bottom(18);
        column.append(&mark);
        column.append(&centered_label(
            "Step · Install",
            "gos-onboarding-kicker",
            false,
        ));
        column.append(&centered_label(
            "The install didn’t finish",
            "gos-onboarding-title",
            false,
        ));
        column.append(&centered_label(
            &format!("Goblins OS couldn't install to {device}. Your disk was not changed."),
            "gos-onboarding-subtitle",
            true,
        ));

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
        panel.add_css_class("gos-net-panel");
        panel.set_size_request(580, -1);
        panel.append(&label("What Happened", &["gos-onboarding-kicker"]));
        let detail_label = label(&detail, &["gos-install-error-detail"]);
        detail_label.set_wrap(true);
        detail_label.set_xalign(0.0);
        panel.append(&detail_label);
        column.append(&panel);

        let retry = button("Try another disk", &["gos-onboarding-primary"]);
        retry.set_halign(gtk::Align::Center);
        retry.set_margin_top(22);
        {
            let pages_c = pages.clone();
            let stack_c = stack.clone();
            let flow_c = flow.clone();
            let core_url_c = core_url.to_string();
            retry.connect_clicked(move |_| {
                *flow_c.selected.borrow_mut() = None;
                *flow_c.last_error.borrow_mut() = None;
                populate_install_disk(&pages_c, &stack_c, &flow_c, &core_url_c);
                stack_c.set_visible_child_name("install-disk");
            });
        }
        column.append(&retry);

        let close = button("Close setup", &["gos-onboarding-quiet"]);
        close.set_halign(gtk::Align::Center);
        {
            let stack_c = stack.clone();
            close.connect_clicked(move |_| stack_c.set_visible_child_name("welcome"));
        }
        column.append(&close);
        return;
    }

    let check = label("✓", &["gos-install-done-mark"]);
    check.set_halign(gtk::Align::Center);
    column.append(&check);
    column.append(&centered_label(
        "Goblins OS is installed",
        "gos-onboarding-title",
        false,
    ));
    let where_text = if model.is_empty() {
        format!("Goblins OS is installed on {device}.")
    } else {
        format!("Goblins OS is installed on {device} ({model}).")
    };
    column.append(&centered_label(
        &where_text,
        "gos-onboarding-subtitle",
        true,
    ));

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("gos-net-panel");
    panel.set_size_request(580, -1);
    // The erase already committed: do not re-show the pre-write dual-boot plan or
    // the "verify the disk / review the erase scope" verification cards here. A
    // post-install screen tells the user what to do next (restart), not re-litigate
    // a decision they already made.
    //
    // The 1-2-3 ladder leads as a clean, uninterrupted action sequence under its own
    // "Next steps" eyebrow, with "Finish first-boot setup" as its un-numbered closer.
    // The post-install boot-entry facts are context, not steps — so they live below in
    // a separately-labeled "Boot details" group instead of being interleaved with the
    // ladder in identically-styled cards.
    panel.append(&label("Next steps", &["gos-onboarding-kicker"]));
    panel.append(&review_row(
        "1  Remove the installer medium",
        "Eject the USB drive or installer image you booted from.",
    ));
    panel.append(&review_row(
        "2  Restart this computer",
        "Use the firmware startup menu or boot picker to start Goblins OS.",
    ));
    panel.append(&review_row(
        "3  Check anything you kept",
        "If you kept another OS or data disk, confirm it still starts or appears before changing boot order.",
    ));
    panel.append(&review_row(
        "Finish first-boot setup",
        "Sign in or continue with on-device GPT-OSS — ready the moment you are.",
    ));

    // Boot-entry info is post-install factual state, not a numbered step — grouped
    // apart so the ladder above reads as a clean 1-2-3 sequence. The eyebrow + top
    // margin set it visibly aside as context.
    if let Some(status) = flow.state.install_targets.as_ref() {
        if boot_entries_have_content(&status.boot_entries) {
            let boot_context = gtk::Box::new(gtk::Orientation::Vertical, 10);
            boot_context.set_margin_top(8);
            boot_context.append(&label("Boot details", &["gos-onboarding-kicker"]));
            append_boot_entries(&boot_context, &status.boot_entries);
            panel.append(&boot_context);
        }
    }
    column.append(&panel);

    let restart = button("Restart now", &["gos-onboarding-primary"]);
    restart.set_halign(gtk::Align::Center);
    restart.set_margin_top(22);
    restart.connect_clicked(move |_| reboot_system());
    column.append(&restart);

    let later = button("I'll restart later", &["gos-onboarding-quiet"]);
    later.set_halign(gtk::Align::Center);
    {
        let app = flow.app.clone();
        later.connect_clicked(move |_| app.quit());
    }
    column.append(&later);
}

/// Ask systemd to restart into the freshly installed Goblins OS.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn reboot_system() {
    if let Err(error) = std::process::Command::new("systemctl")
        .arg("reboot")
        .spawn()
    {
        eprintln!("installer_reboot_error={error}");
    }
}

/// POST the guarded execute request. Returns the core's HTTP status, its declared
/// `state` (prepared/started/blocked), and its `detail` string, so the caller can
/// route on the exact outcome — and only show "installing" when the core actually
/// reports it started.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn execute_install(
    core_url: &str,
    target_path: &str,
    acknowledgement: &str,
) -> Result<(u16, String, String), CoreFetchError> {
    let body = serde_json::json!({
        "target_path": target_path,
        "filesystem": "xfs",
        "block_setup": "direct",
        "wipe": true,
        "execute": true,
        "acknowledgement": acknowledgement,
    })
    .to_string();
    let response = http_request(
        core_url,
        "POST",
        "/v1/installer/install-targets/prepare",
        Some(body.as_bytes()),
    )?;

    let (state, detail) = serde_json::from_slice::<PrepareInstallResponse>(&response.body)
        .map(|parsed| (parsed.state, parsed.detail))
        .unwrap_or_else(|_| {
            (
                String::new(),
                "Goblins OS could not start the install.".to_string(),
            )
        });
    Ok((response.status, state, detail))
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn first_app_onboarding_subtitle() -> &'static str {
    "Describe a small, useful app. Goblins OS builds it locally with the selected engine, and you can revisit it from the launcher later."
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn first_app_build_failure_copy() -> &'static str {
    "Goblins OS could not build the first app yet. You can enter the desktop now and try again from the launcher."
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn system_row(title: &str, detail: &str) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let row = gtk::Box::new(gtk::Orientation::Vertical, 3);
    row.add_css_class("gos-row");
    row.append(&label(title, &["gos-row-title"]));
    row.append(&label(detail, &["gos-row-copy"]));
    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn model_row(title: &str, detail: &str, available: bool) -> gtk4::Box {
    use gtk4::prelude::*;

    let row = system_row(title, detail);
    row.add_css_class(if available {
        "gos-model-available"
    } else {
        "gos-model-unavailable"
    });
    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn label(text: &str, classes: &[&str]) -> gtk4::Label {
    use gtk4::prelude::*;

    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);

    for class in classes {
        label.add_css_class(class);
    }

    label
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn button(text: &str, classes: &[&str]) -> gtk4::Button {
    use gtk4::prelude::*;

    let button = gtk4::Button::with_label(text);

    for class in classes {
        button.add_css_class(class);
    }

    button
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn spacer() -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    spacer
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_native_installer(_config: InstallerConfig, _state: InstallerState) -> InstallerResult<()> {
    println!("native_installer_state=unavailable");
    println!("native_installer_reason=build_requires_linux_native_desktop_feature");
    Ok(())
}

fn wait_for_core(core_url: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if matches!(http_status(core_url, "/health"), Some(200)) {
            return true;
        }

        thread::sleep(Duration::from_millis(750));
    }

    false
}

fn http_status(base_url: &str, path: &str) -> Option<u16> {
    let endpoint = parse_http_endpoint(base_url)?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .ok()?
        .next()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(600)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(900)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(900)))
        .ok()?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        endpoint.host
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut buffer = [0_u8; 128];
    let read = stream.read(&mut buffer).ok()?;
    let response = std::str::from_utf8(&buffer[..read]).ok()?;
    let status = response.split_whitespace().nth(1)?;

    status.parse().ok()
}

fn get_core_json<T>(base_url: &str, path: &str) -> Result<T, CoreFetchError>
where
    T: for<'de> Deserialize<'de>,
{
    let response = http_request(base_url, "GET", path, None)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    serde_json::from_slice(&response.body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn openai_login_destination(core_url: &str) -> Result<String, CoreFetchError> {
    let response = http_request(core_url, "GET", "/v1/auth/openai/start", None)?;
    openai_login_destination_from_response(&response)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn openai_login_destination_from_response(
    response: &HttpResponse,
) -> Result<String, CoreFetchError> {
    if !(300..=399).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    header_value(&response.headers, "location")
        .map(ToString::to_string)
        .ok_or(CoreFetchError::Malformed)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn complete_and_unlock_first_boot(core_url: &str, mode: &str) -> Result<(), CoreFetchError> {
    complete_first_boot(core_url, mode)?;
    unlock_session(core_url, mode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn submit_setup_build(core_url: &str, intent: &str) -> Result<(), CoreFetchError> {
    #[derive(Deserialize)]
    struct BuildOutcome {
        ok: bool,
        #[serde(default)]
        text: String,
    }

    let body = serde_json::json!({ "intent": intent }).to_string();
    let response = http_request(core_url, "POST", "/v1/apps/builds", Some(body.as_bytes()))?;
    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    let outcome: BuildOutcome =
        serde_json::from_slice(&response.body).map_err(|_| CoreFetchError::Decode)?;
    if outcome.ok {
        Ok(())
    } else if outcome.text.is_empty() {
        Err(CoreFetchError::Malformed)
    } else {
        Err(CoreFetchError::Status(response.status))
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn complete_first_boot(core_url: &str, mode: &str) -> Result<(), CoreFetchError> {
    let body = serde_json::json!({ "mode": mode }).to_string();
    let response = http_request(
        core_url,
        "POST",
        "/v1/installer/complete",
        Some(body.as_bytes()),
    )?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn unlock_session(core_url: &str, mode: &str) -> Result<(), CoreFetchError> {
    let body = serde_json::json!({ "mode": mode }).to_string();
    let response = http_request(
        core_url,
        "POST",
        "/v1/session/unlock",
        Some(body.as_bytes()),
    )?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(())
}

/// Persist the OS-owned offline / private-mode choice. When on, the resident
/// relay refuses every hosted and server path, so the AI never reaches the
/// network — the privacy guarantee is enforced server-side, not in the GUI.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_privacy_mode(core_url: &str, offline: bool) -> Result<(), CoreFetchError> {
    let body = serde_json::json!({ "offline": offline }).to_string();
    let response = http_request(core_url, "POST", "/v1/privacy", Some(body.as_bytes()))?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn request_model_install(core_url: &str, model_id: &str) -> Result<String, CoreFetchError> {
    let body = serde_json::json!({ "model_id": model_id, "consent": true }).to_string();
    let response = http_request(
        core_url,
        "POST",
        "/v1/local-models/install",
        Some(body.as_bytes()),
    )?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    let body = String::from_utf8(response.body).map_err(|_| CoreFetchError::Malformed)?;
    Ok(body)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn prepare_install_command(core_url: &str, target_path: &str) -> Result<String, CoreFetchError> {
    let body = serde_json::json!({
        "target_path": target_path,
        "filesystem": "xfs",
        "block_setup": "direct",
        "wipe": true,
        "execute": false,
    })
    .to_string();
    let response = http_request(
        core_url,
        "POST",
        "/v1/installer/install-targets/prepare",
        Some(body.as_bytes()),
    )?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    install_prepare_summary(&response.body)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn install_prepare_summary(body: &[u8]) -> Result<String, CoreFetchError> {
    let response = serde_json::from_slice::<PrepareInstallResponse>(body)
        .map_err(|_| CoreFetchError::Decode)?;
    let target = install_command_target(&response.command)
        .map(|target| format!(" for selected disk {target}"))
        .unwrap_or_default();
    Ok(format!(
        "{} · Review the install plan{target}; no disk has been changed.",
        response.detail
    ))
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn install_command_target(command: &[String]) -> Option<&str> {
    command.iter().rev().find_map(|part| {
        if part.starts_with("/dev/") {
            Some(part.as_str())
        } else {
            None
        }
    })
}

fn http_request(
    base_url: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<HttpResponse, CoreFetchError> {
    let endpoint = parse_http_endpoint(base_url).ok_or(CoreFetchError::Malformed)?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| CoreFetchError::Transport)?
        .next()
        .ok_or(CoreFetchError::Transport)?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(700))
        .map_err(|_| CoreFetchError::Transport)?;

    stream
        .set_read_timeout(Some(Duration::from_millis(1200)))
        .map_err(|_| CoreFetchError::Transport)?;
    stream
        .set_write_timeout(Some(Duration::from_millis(900)))
        .map_err(|_| CoreFetchError::Transport)?;

    let body = body.unwrap_or_default();
    let content_headers = if body.is_empty() {
        String::new()
    } else {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        )
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\n{}Connection: close\r\n\r\n",
        endpoint.host, content_headers
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|_| CoreFetchError::Transport)?;
    if !body.is_empty() {
        stream
            .write_all(body)
            .map_err(|_| CoreFetchError::Transport)?;
    }

    let mut response = Vec::new();
    stream
        .take(MAX_CORE_BODY_BYTES as u64)
        .read_to_end(&mut response)
        .map_err(|_| CoreFetchError::Transport)?;

    parse_http_response(&response)
}

fn parse_http_response(response: &[u8]) -> Result<HttpResponse, CoreFetchError> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(CoreFetchError::Malformed)?;
    let headers =
        std::str::from_utf8(&response[..header_end]).map_err(|_| CoreFetchError::Malformed)?;
    let mut lines = headers.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or(CoreFetchError::Malformed)?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();

    Ok(HttpResponse {
        status,
        headers,
        body: response[(header_end + 4)..].to_vec(),
    })
}

fn parse_http_endpoint(url: &str) -> Option<HttpEndpoint> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?.trim();

    if authority.is_empty() {
        return None;
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (authority, 80),
    };

    if host.is_empty() {
        return None;
    }

    Some(HttpEndpoint {
        host: host.to_string(),
        port,
    })
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn runtime_label(runtime: &RuntimeReport) -> String {
    let mut names = Vec::new();

    if let Some(selected) = &runtime.selected {
        names.push(selected.clone());
    }

    if runtime.ollama {
        names.push("Ollama".to_string());
    }

    if runtime.vllm {
        names.push("vLLM".to_string());
    }

    if runtime.lm_studio {
        names.push("LM Studio".to_string());
    }

    if names.is_empty() {
        "required".to_string()
    } else {
        names.join(", ")
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
/// Almost every surface in the installer is styled by the shared Goblins OS design
/// system: each class it uses is defined in `goblins_os_design::GOBLINS_NATIVE_CSS`,
/// which `native_css` appends *after* this string, so the shared, token-driven,
/// theme-aware rules win at equal specificity. The only rules that live here are a
/// few installer-only overrides that deliberately out-specify a shared rule (each
/// uses a two-class or descendant selector so it beats the single-class base): the
/// neutral readiness chips on the advanced-setup top bar, and the dark-mode legible
/// selected-fill on the onboarding choice cards. Every value is still a shared design
/// token, so these stay theme-aware in Light and Dark.
const GOBLINS_OS_INSTALLER_CSS: &str = r#"
/* The advanced-setup top-bar readiness chips (OS / OpenAI / Local) are descriptive
   labels, not a tri-color health signal, so they all rest in one calm neutral chip
   instead of two-green/one-amber by happenstance. Two-class selectors out-specify
   the single-class .gos-ready / .gos-waiting hue, and use the same neutral
   secondary-label ink the rest of the system gives a descriptive pill, theme-aware
   in both Light and Dark. */
.gos-status-pill.gos-readiness-chip {
  color: @gos_label_secondary;
  background: @gos_fill_secondary;
  border-color: transparent;
  font-weight: 600;
}

/* The selected Appearance/Accessibility setup card must read from its FILL, not the
   1px accent border alone — in Dark the shared 10% accent tint sat too close to the
   unselected surface. Scoping under .gos-onboarding-root out-specifies the shared
   single-class .gos-setup-choice-selected rule so the chosen card lifts to a clearly
   legible fill in both schemes. The check's optical centering is handled in code
   (valign: Center on the row), so there is no per-layout placement to override. */
.gos-onboarding-root .gos-setup-choice-selected {
  background: alpha(@gos_accent, 0.18);
}

.gos-onboarding-root .gos-setup-choice-selected:hover {
  background: alpha(@gos_accent, 0.22);
}

/* Disk-card status tone ladder — three honest steps, not one undifferentiated red.
   The base .gos-install-disk-state (design crate) is the affirmative green "ready";
   .is-blocked (design crate) is the reserved alarm-red for a genuine hard block.
   Two tones live here so the installer never has to touch the design crate:

     · .is-advisory — advisory-but-actionable copy (e.g. "opens advanced storage to
       preserve them"). It is not an error, so it rests in the calm neutral/muted
       secondary ink the rest of the system gives descriptive status, theme-aware in
       both Light and Dark. Two-class selector out-specifies the base green.
     · .is-caution  — selecting the row is destructive (a whole-disk replace). The
       amber waiting/caution token sits one step below the reserved red. */
.gos-install-disk-state.is-advisory {
  color: @gos_label_secondary;
  font-weight: 600;
}

.gos-install-disk-state.is-caution {
  color: @gos_waiting;
  font-weight: 600;
}

/* Disk rows keep the design crate's single container treatment so all three rows
   ladder: the eligible disk rests on the brighter base surface while blocked disks
   recede to the muted surface (.is-blocked) — the eligible target reads as the
   strongest row without a per-row fill swap. (An earlier installer-local background
   override only repainted the eligible row, leaving blocked rows muted, which made
   the list read as three different container systems and inverted the emphasis.) */

/* One shared badge style for every disk-type chip (SSD / HDD / Removable). The chips
   share a class already, but mixed casing ("SSD" vs "Removable") under the eyebrow's
   wide tracking made the short all-caps strings read as "S S D". Normalizing every
   badge to uppercase with one tracking value gives the column a single tokenized
   eyebrow treatment, so all chips track identically regardless of source string. */
.gos-install-disk-kind {
  text-transform: uppercase;
  letter-spacing: 1.2px;
}
"#;

#[cfg(test)]
mod tests {
    use super::{
        first_app_build_failure_copy, first_app_onboarding_subtitle, install_prepare_summary,
        openai_login_destination_from_response, parse_http_endpoint, parse_http_response,
        review_detail_lines, CoreFetchError, HttpEndpoint, HttpResponse,
    };

    #[test]
    fn parses_local_core_endpoint() {
        assert_eq!(
            parse_http_endpoint("http://127.0.0.1:8787"),
            Some(HttpEndpoint {
                host: "127.0.0.1".to_string(),
                port: 8787,
            })
        );
    }

    #[test]
    fn rejects_non_http_endpoint() {
        assert_eq!(parse_http_endpoint("https://127.0.0.1:8787"), None);
    }

    #[test]
    fn parses_core_json_body() {
        let response = parse_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.body, br#"{"ok":true}"#);
    }

    #[test]
    fn parses_openai_login_redirect() {
        let response = parse_http_response(
            b"HTTP/1.1 302 Found\r\nLocation: https://auth.openai.example/start\r\n\r\n",
        )
        .unwrap();

        assert_eq!(
            openai_login_destination_from_response(&response),
            Ok("https://auth.openai.example/start".to_string())
        );
    }

    #[test]
    fn rejects_login_start_without_location() {
        let response = HttpResponse {
            status: 302,
            headers: Vec::new(),
            body: Vec::new(),
        };

        assert_eq!(
            openai_login_destination_from_response(&response),
            Err(CoreFetchError::Malformed)
        );
    }

    #[test]
    fn summarizes_prepared_install_command() {
        let summary = install_prepare_summary(
            br#"{"state":"prepared","command":["bootc","install","to-disk","--filesystem","xfs","--wipe","/dev/nvme0n1"],"detail":"Install plan prepared. No disk has been changed; execution stays blocked until the destructive install gate is explicitly enabled."}"#,
        )
        .unwrap();

        assert!(summary.contains("Install plan prepared."));
        // The raw internal state slug is never prefixed onto user copy.
        assert!(!summary.starts_with("prepared:"));
        assert!(summary.contains("selected disk /dev/nvme0n1"));
        assert!(summary.contains("no disk has been changed"));
        assert!(!summary.contains("bootc install"));
    }

    #[test]
    fn install_review_details_split_dense_safety_copy_into_scan_lines() {
        let lines = review_detail_lines(
            "Blank disk. Action: Select this disk only if it is the disk you want Goblins OS to own completely. Target: The whole disk becomes the Goblins OS target with a fresh GPT layout. Preserve: Other disks are not selected by this flow.",
        );

        assert_eq!(lines[0], "Blank disk.");
        assert!(lines.iter().any(|line| line.starts_with("Action:")));
        assert!(lines.iter().any(|line| line.starts_with("Target:")));
        assert!(lines.iter().any(|line| line.starts_with("Preserve:")));
        assert!(lines.iter().all(|line| !line.contains(" Action:")));
    }

    #[test]
    fn install_review_details_split_long_sentences_without_losing_warning_terms() {
        let lines = review_detail_lines(
            "To erase /dev/nvme1n1, including any Windows, macOS, Linux, other OS, recovery, and EFI partitions on that disk, type this phrase exactly. To keep another OS, stop here and open advanced storage with Custom/manual storage or Reclaim Space.",
        );

        assert!(lines.len() >= 2);
        assert!(lines.iter().any(|line| line.contains("Windows")));
        assert!(lines.iter().any(|line| line.contains("Reclaim Space")));
    }

    #[test]
    fn installer_review_and_details_keep_bottom_actions_scroll_reachable() {
        let source = include_str!("main.rs");

        assert!(source.contains("let details_scroller = gtk::ScrolledWindow::new();"));
        assert!(source.contains("body.set_margin_bottom(40);"));
        assert!(source.contains("details_scroller.set_child(Some(&body));"));
        assert!(source.contains("let scroll_document = gtk4::Box::new"));
        assert!(source.contains("top_spacer.set_vexpand(true);"));
        assert!(source.contains("bottom_spacer.set_vexpand(true);"));
        assert!(source.contains("column.set_valign(gtk4::Align::Start);"));
        assert!(source.contains("column.set_margin_bottom(40);"));
        assert!(source.contains("scroller.set_child(Some(&scroll_document));"));
    }

    #[test]
    fn first_app_build_failure_copy_is_user_facing() {
        let copy = first_app_build_failure_copy();

        assert!(copy.contains("could not build the first app yet"));
        assert!(copy.contains("try again from the launcher"));
        assert!(!copy.contains('{'));
        assert!(!copy.contains("error"));
        assert!(!copy.contains("127.0.0.1"));
        assert!(!copy.contains("/var/lib"));
        assert!(!copy.contains("state file"));
    }

    #[test]
    fn first_app_onboarding_copy_hides_backend_plumbing() {
        let copy = first_app_onboarding_subtitle();

        assert!(copy.contains("builds it locally"));
        assert!(copy.contains("selected engine"));
        assert!(copy.contains("launcher later"));
        assert!(!copy.contains("daemon"));
        assert!(!copy.contains("loopback"));
        assert!(!copy.contains("127.0.0.1"));
        assert!(!copy.contains("service"));
    }

    #[test]
    fn installer_copy_hides_backend_core_language() {
        let source = include_str!("main.rs");

        assert!(source.contains("status_pill(\"OS\""));
        assert!(source.contains("Waiting for local OS services."));
        assert!(
            source.contains("Goblins OS could not reach local OS services to start the install.")
        );
        assert!(source.contains("Installer checks"));
        assert!(source.contains("The disk install reported a failure."));
        for forbidden in [
            ["status_pill(\"", "Core", "\""].join(""),
            ["Waiting for the local Goblins OS ", "core."].join(""),
            ["could not reach the local ", "core"].join(""),
            ["\"", "Goblins OS ", "core", "\""].join(""),
            ["bootc install reported a ", "failure."].join(""),
        ] {
            assert!(
                !source.contains(&forbidden),
                "installer UI copy must not expose backend wording: {forbidden}"
            );
        }
    }
}
