use axum::{extract::Extension, http::Uri, Json};
use serde::Serialize;

use crate::{
    credentials::{openai_credential, openai_credential_with_compat},
    policy::{policy_state_for_control, PolicyControlState},
};

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceStatus {
    External,
    ServerGated,
    Local,
    PermissionGated,
    PolicyBlocked,
    NotConfigured,
}

#[derive(Serialize)]
pub struct ServiceCatalogEntry {
    id: &'static str,
    name: &'static str,
    role: &'static str,
    launch: &'static str,
    policy_control: &'static str,
    status: ServiceStatus,
    api_surface: &'static str,
    sdk: &'static str,
    os_boundary: &'static str,
    secret_boundary: &'static str,
    readiness: String,
}

#[derive(Serialize)]
pub struct ServiceCatalog {
    services: Vec<ServiceCatalogEntry>,
}

pub async fn service_catalog(
    Extension(client): Extension<crate::control_plane::RequestClient>,
) -> Json<ServiceCatalog> {
    Json(ServiceCatalog {
        services: build_services(Some(client.user_id())),
    })
}

fn build_services(user_id: Option<u32>) -> Vec<ServiceCatalogEntry> {
    vec![
        ServiceCatalogEntry {
            id: "chatgpt",
            name: "ChatGPT",
            role: "Primary conversation space",
            launch: "https://chatgpt.com",
            policy_control: "cloud-openai",
            status: service_status("cloud-openai", ServiceStatus::External),
            api_surface: "official-web-app",
            sdk: "not-applicable",
            os_boundary: "Goblins OS opens the fixed official ChatGPT web surface through its policy-gated launcher.",
            secret_boundary: "The launcher starts the fixed web-handler command from an empty, reviewed desktop environment and passes no provider or account credential to that command. A pre-existing browser owns its own environment and account session.",
            readiness: "Opens the official ChatGPT web surface when cloud OpenAI policy allows it."
                .to_string(),
        },
        ServiceCatalogEntry {
            id: "codex",
            name: "Codex",
            role: "Codex workspace via compatible app or official web",
            launch: "local://goblins-os/openai/codex",
            policy_control: "cloud-openai",
            status: service_status("cloud-openai", ServiceStatus::External),
            api_surface: "compatible-linux-app-codex-deep-link",
            sdk: "Compatible installed Codex app",
            os_boundary: "The Goblins launcher opens a compatible installed Codex app; Build Studio remains a separate OS-owned workspace and engine experience. Ordinary desktop Codex links are routed through the Goblins wrapper, while a manually invoked third-party executable remains outside this launcher boundary.",
            secret_boundary: "The launcher starts the compatible app from an empty, reviewed desktop environment. Any installed app owns its own per-user account session; Goblins OS does not inject one.",
            readiness: "Opens a compatible installed Codex app when available, or the official Codex web surface otherwise, when cloud OpenAI policy allows it."
                .to_string(),
        },
        ServiceCatalogEntry {
            id: "build-studio",
            name: "Build Studio",
            role: "The OS app model: create applications from intent instead of installing them",
            launch: "local://goblins-os/apps/build",
            policy_control: "app-builder",
            status: build_studio_status(user_id),
            api_surface: "resident-generate",
            sdk: "Codex when the account engine is selected; Responses API when your own OpenAI key is selected; the managed OpenAI service when explicitly selected; local GPT-OSS otherwise",
            os_boundary: "Rust Build Studio owns policy and storage and always uses the explicitly selected Goblins AI engine.",
            secret_boundary: "Build Studio never receives raw API keys, account tokens, or tool credentials.",
            readiness: build_studio_readiness(user_id),
        },
        ServiceCatalogEntry {
            id: "platform",
            name: "OpenAI Platform",
            role: "Models, projects, usage, and developer controls",
            launch: "https://platform.openai.com",
            policy_control: "cloud-openai",
            status: service_status("cloud-openai", ServiceStatus::External),
            api_surface: "official-web-app",
            sdk: "not-applicable",
            os_boundary: "external browser surface opened by OS launcher",
            secret_boundary: "No Platform credential is stored by this local service catalog.",
            readiness: "Opens the official OpenAI Platform surface when cloud policy allows it."
                .to_string(),
        },
        ServiceCatalogEntry {
            id: "responses-api",
            name: "Responses API",
            role: "Unified intelligence layer for OS services",
            launch: "https://platform.openai.com/docs/api-reference/responses",
            policy_control: "cloud-openai",
            status: configured_service_status(
                "cloud-openai",
                responses_api_configured(user_id),
                ServiceStatus::ServerGated,
            ),
            api_surface: "/v1/responses",
            sdk: "Official OpenAI API surface called from Rust over the server-side core",
            os_boundary: "Goblins OS makes hosted requests inside its protected system service; apps use only approved Goblins OS actions.",
            secret_boundary: "Your OpenAI key stays in protected system storage or a managed organization service.",
            readiness: if responses_api_configured(user_id) {
                "Configured for server-side Responses API calls.".to_string()
            } else {
                "Add your own OpenAI API key in Settings, or ask a device administrator to configure the managed organization service.".to_string()
            },
        },
        ServiceCatalogEntry {
            id: "voice-realtime",
            name: "Voice and Realtime",
            role: "Speech and low-latency multimodal interface",
            launch: "https://platform.openai.com/docs/guides/realtime",
            policy_control: "cloud-openai",
            status: configured_service_status(
                "cloud-openai",
                relay_configured("GOBLINS_OS_REALTIME_RELAY_URL", "OPENAI_OS_REALTIME_RELAY_URL"),
                ServiceStatus::ServerGated,
            ),
            api_surface: "Realtime API",
            sdk: "Official OpenAI Realtime/WebRTC support inside protected system services",
            os_boundary: "Goblins voice keeps wake, transcription, and speech on-device unless OpenAI Realtime is explicitly enabled.",
            secret_boundary: "Realtime session secrets stay in protected system services and never enter desktop apps.",
            readiness: relay_readiness(
                "GOBLINS_OS_REALTIME_RELAY_URL",
                "OPENAI_OS_REALTIME_RELAY_URL",
                "OpenAI Realtime",
            ),
        },
        ServiceCatalogEntry {
            id: "images",
            name: "Images",
            role: "Image generation and editing workspace",
            launch: "https://platform.openai.com/docs/guides/images",
            policy_control: "cloud-openai",
            status: configured_service_status(
                "cloud-openai",
                relay_configured("GOBLINS_OS_IMAGES_RELAY_URL", "OPENAI_OS_IMAGES_RELAY_URL"),
                ServiceStatus::ServerGated,
            ),
            api_surface: "Images API",
            sdk: "Official OpenAI Images API inside protected system services",
            os_boundary: "Image requests route through local OS services before any hosted call.",
            secret_boundary: "Image credentials stay in protected system services and never enter desktop apps.",
            readiness: relay_readiness(
                "GOBLINS_OS_IMAGES_RELAY_URL",
                "OPENAI_OS_IMAGES_RELAY_URL",
                "OpenAI Images",
            ),
        },
        ServiceCatalogEntry {
            id: "agents",
            name: "Agents SDK",
            role: "Tool-using automations and managed workflows",
            launch: "https://platform.openai.com/docs/guides/agents",
            policy_control: "agents",
            status: configured_service_status(
                "agents",
                relay_configured(
                    "GOBLINS_OS_AGENTS_SDK_RELAY_URL",
                    "OPENAI_OS_AGENTS_SDK_RELAY_URL",
                ),
                ServiceStatus::ServerGated,
            ),
            api_surface: "Agents SDK",
            sdk: "Official OpenAI Agents SDK for Python/TypeScript inside protected system services",
            os_boundary: "Rust policy and permission gates stay in Goblins OS; the SDK runner owns tools, handoffs, guardrails, tracing, and sandbox execution server-side.",
            secret_boundary: "Agent API keys and tool credentials stay in protected system services and never enter desktop apps.",
            readiness: relay_readiness(
                "GOBLINS_OS_AGENTS_SDK_RELAY_URL",
                "OPENAI_OS_AGENTS_SDK_RELAY_URL",
                "OpenAI Agents SDK",
            ),
        },
        ServiceCatalogEntry {
            id: "chatkit",
            name: "ChatKit",
            role: "Embeddable OpenAI chat UI for app surfaces",
            launch: "https://platform.openai.com/docs/guides/chatkit",
            policy_control: "cloud-openai",
            status: configured_service_status(
                "cloud-openai",
                relay_configured("GOBLINS_OS_CHATKIT_RELAY_URL", "OPENAI_OS_CHATKIT_RELAY_URL"),
                ServiceStatus::ServerGated,
            ),
            api_surface: "ChatKit",
            sdk: "Official OpenAI ChatKit through protected system services",
            os_boundary: "Native Goblins OS surfaces remain GTK/Rust; web ChatKit appears only when an administrator explicitly enables it.",
            secret_boundary: "ChatKit client sessions are brokered by OS services, not embedded with static secrets.",
            readiness: relay_readiness(
                "GOBLINS_OS_CHATKIT_RELAY_URL",
                "OPENAI_OS_CHATKIT_RELAY_URL",
                "OpenAI ChatKit",
            ),
        },
        ServiceCatalogEntry {
            id: "files-context",
            name: "Files and Context",
            role: "Local-first workspace context for OpenAI services",
            launch: "local://goblins-os/context",
            policy_control: "local-models",
            status: service_status("local-models", ServiceStatus::Local),
            api_surface: "local-context",
            sdk: "Rust OS context APIs",
            os_boundary: "Context is bounded and summarized by the Rust core before model access.",
            secret_boundary: "Files, prompts, and hidden content are not sent unless an explicit action includes bounded context.",
            readiness: "Ready when local model policy allows context actions.".to_string(),
        },
        ServiceCatalogEntry {
            id: "computer-use",
            name: "Computer Use",
            role: "Controlled OS automation through explicit permissions",
            launch: "local://goblins-os/computer-use",
            policy_control: "computer-use",
            status: service_status("computer-use", ServiceStatus::ServerGated),
            api_surface: "computer-use tools",
            sdk: "OpenAI tool-use contract behind Goblins OS permission gates",
            os_boundary: "Rust policy controls require explicit local permission before OS actions.",
            secret_boundary: "Tool credentials stay in OS services or server-side tool runners.",
            readiness: "Requires explicit local permission before any automated action.".to_string(),
        },
        ServiceCatalogEntry {
            id: "enterprise-controls",
            name: "Enterprise Controls",
            role: "Native OpenAI policy, data boundary, and admin controls",
            launch: "local://goblins-os/policy",
            policy_control: "enterprise-controls",
            status: service_status("enterprise-controls", ServiceStatus::Local),
            api_surface: "local-policy",
            sdk: "Rust policy APIs",
            os_boundary: "Goblins OS owns policy evaluation locally.",
            secret_boundary: "Policies expose state, never secrets.",
            readiness: "Ready through local policy controls.".to_string(),
        },
        ServiceCatalogEntry {
            id: "settings",
            name: "Settings",
            role: "Native Goblins OS controls for OpenAI identity, local models, and services",
            launch: "local://goblins-os/settings",
            policy_control: "local-models",
            status: ServiceStatus::Local,
            api_surface: "local-settings",
            sdk: "Rust/GTK Settings over Goblins OS core APIs",
            os_boundary: "Settings reads local status and writes only explicit user choices.",
            secret_boundary: "Settings receives readiness booleans and plain-language status, never secret paths, API keys, or account tokens.",
            readiness: "Ready as a native Goblins OS control surface.".to_string(),
        },
        ServiceCatalogEntry {
            id: "recovery",
            name: "Recovery",
            role:
                "Native recovery checks for boot image, services, resident state, and model storage",
            launch: "local://goblins-os/recovery",
            policy_control: "local-models",
            status: ServiceStatus::Local,
            api_surface: "local-recovery",
            sdk: "Rust recovery APIs",
            os_boundary: "Recovery checks local bootc, systemd, resident, and model state.",
            secret_boundary: "Recovery reports status only and never exports secrets.",
            readiness: "Ready through local recovery checks.".to_string(),
        },
    ]
}

fn service_status(control_id: &str, allowed_status: ServiceStatus) -> ServiceStatus {
    match policy_state_for_control(control_id) {
        PolicyControlState::Allowed => allowed_status,
        PolicyControlState::Denied => ServiceStatus::PolicyBlocked,
        PolicyControlState::PermissionGated => ServiceStatus::PermissionGated,
    }
}

fn configured_service_status(
    control_id: &str,
    configured: bool,
    allowed_status: ServiceStatus,
) -> ServiceStatus {
    match policy_state_for_control(control_id) {
        PolicyControlState::Allowed if configured => allowed_status,
        PolicyControlState::Allowed => ServiceStatus::NotConfigured,
        PolicyControlState::Denied => ServiceStatus::PolicyBlocked,
        PolicyControlState::PermissionGated => ServiceStatus::PermissionGated,
    }
}

fn build_studio_status(user_id: Option<u32>) -> ServiceStatus {
    match policy_state_for_control("app-builder") {
        PolicyControlState::Denied => ServiceStatus::PolicyBlocked,
        PolicyControlState::PermissionGated => ServiceStatus::PermissionGated,
        PolicyControlState::Allowed => match crate::resident::active_engine_locality(user_id) {
            Some(crate::resident::EngineLocality::OnDevice) => ServiceStatus::Local,
            Some(crate::resident::EngineLocality::Cloud) => ServiceStatus::ServerGated,
            None => ServiceStatus::NotConfigured,
        },
    }
}

fn build_studio_readiness(user_id: Option<u32>) -> String {
    match policy_state_for_control("app-builder") {
        PolicyControlState::Denied => {
            return "Build Studio is blocked by the active Goblins OS policy.".to_string();
        }
        PolicyControlState::PermissionGated => {
            return "Build Studio requires an explicit Goblins OS permission review.".to_string();
        }
        PolicyControlState::Allowed => {}
    }

    match crate::resident::active_engine_locality(user_id) {
        Some(crate::resident::EngineLocality::OnDevice) => {
            "Ready through the selected on-device Goblins AI engine.".to_string()
        }
        Some(crate::resident::EngineLocality::Cloud) => {
            "Ready through the explicitly selected cloud Goblins AI engine.".to_string()
        }
        None => "The selected Goblins AI engine is not ready.".to_string(),
    }
}

fn responses_api_configured(user_id: Option<u32>) -> bool {
    (user_id.is_some_and(crate::openai_key_provisioning::credential_is_stored)
        && crate::resident::openai_api_base_is_valid())
        || crate::resident::managed_cloud_route_configured()
}

fn relay_configured(primary_url_var: &str, legacy_url_var: &str) -> bool {
    let url = openai_credential_with_compat(primary_url_var, legacy_url_var);
    url.as_deref().is_some_and(server_https_url)
        && openai_credential("AI_GATEWAY_API_KEY").is_some_and(|key| !key.trim().is_empty())
}

fn server_https_url(value: &str) -> bool {
    let Ok(uri) = value.parse::<Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    uri.scheme_str() == Some("https")
        && !authority.host().is_empty()
        && !authority.as_str().contains('@')
}

fn relay_readiness(primary_url_var: &str, legacy_url_var: &str, label: &str) -> String {
    if relay_configured(primary_url_var, legacy_url_var) {
        format!("{label} is ready through protected system credentials.")
    } else {
        format!("{label} is not ready yet. Ask a device administrator to configure the protected service before turning it on.")
    }
}

#[cfg(test)]
mod tests {
    use super::build_services;
    use std::collections::HashSet;

    #[test]
    fn every_service_opens_a_real_openai_surface_or_an_os_owned_action() {
        for service in build_services(None) {
            if let Some(rest) = service.launch.strip_prefix("https://") {
                // Web tiles are only allowed to open genuine OpenAI surfaces.
                assert!(
                    rest.starts_with("chatgpt.com")
                        || rest.starts_with("platform.openai.com")
                        || rest.starts_with("openai.com"),
                    "service `{}` must open a real OpenAI surface, got {}",
                    service.id,
                    service.launch
                );
            } else {
                // Everything else must be an OS-owned local action, never some other web app.
                assert!(
                    service.launch.starts_with("local://goblins-os/"),
                    "service `{}` must be an OS-owned local action, got {}",
                    service.id,
                    service.launch
                );
            }
        }
    }

    #[test]
    fn each_openai_service_has_its_own_distinct_surface() {
        // Guards against the regression where several first-class services all
        // collapsed onto one generic placeholder URL.
        let launches: Vec<&str> = build_services(None)
            .iter()
            .map(|service| service.launch)
            .collect();
        let unique: HashSet<&&str> = launches.iter().collect();
        assert_eq!(
            unique.len(),
            launches.len(),
            "each OpenAI service must open its own surface, not a shared placeholder"
        );
    }

    #[test]
    fn catalog_declares_current_openai_surfaces_and_sdk_boundaries() {
        let services = build_services(None);
        let chatgpt = services
            .iter()
            .find(|service| service.id == "chatgpt")
            .expect("ChatGPT service");
        assert_eq!(chatgpt.launch, "https://chatgpt.com");
        assert_eq!(chatgpt.api_surface, "official-web-app");
        assert!(chatgpt.os_boundary.contains("official ChatGPT web"));
        assert!(chatgpt.secret_boundary.contains("web-handler command"));
        assert!(chatgpt.secret_boundary.contains("pre-existing browser"));

        let codex = services
            .iter()
            .find(|service| service.id == "codex")
            .expect("Codex service");
        assert_eq!(codex.launch, "local://goblins-os/openai/codex");
        assert_eq!(
            codex.role,
            "Codex workspace via compatible app or official web"
        );
        assert_eq!(codex.api_surface, "compatible-linux-app-codex-deep-link");
        assert_eq!(codex.sdk, "Compatible installed Codex app");
        assert!(codex
            .os_boundary
            .contains("Build Studio remains a separate"));
        assert!(codex.secret_boundary.contains("empty, reviewed"));
        assert!(codex.os_boundary.contains("manually invoked"));

        let responses = services
            .iter()
            .find(|service| service.id == "responses-api")
            .expect("responses service");
        assert_eq!(responses.api_surface, "/v1/responses");
        assert!(responses.sdk.contains("Rust"));
        assert!(responses
            .secret_boundary
            .contains("protected system storage"));

        let agents = services
            .iter()
            .find(|service| service.id == "agents")
            .expect("agents service");
        assert!(agents.sdk.contains("Official OpenAI Agents SDK"));
        assert!(agents.os_boundary.contains("Rust policy"));
        assert!(agents.secret_boundary.contains("never enter desktop apps"));

        let build_studio = services
            .iter()
            .find(|service| service.id == "build-studio")
            .expect("build studio service");
        assert_eq!(build_studio.launch, "local://goblins-os/apps/build");
        assert!(build_studio.sdk.contains("Codex"));
        assert!(build_studio.sdk.contains("Responses API"));
        assert!(build_studio.sdk.contains("explicitly selected"));
        assert!(build_studio.os_boundary.contains("always uses"));
        assert!(!build_studio.sdk.contains("Agents SDK"));

        assert_ne!(
            agents.launch, build_studio.launch,
            "the Agents SDK remains a separate service and is never an invisible Build Studio route"
        );

        assert!(
            services.iter().any(|service| service.id == "chatkit"),
            "ChatKit must be a first-class configured-or-disabled surface"
        );
    }
}
