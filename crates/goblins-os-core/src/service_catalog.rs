use axum::Json;
use serde::Serialize;
use std::env;

use crate::policy::{policy_state_for_control, PolicyControlState};

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

pub async fn service_catalog() -> Json<ServiceCatalog> {
    Json(ServiceCatalog {
        services: build_services(),
    })
}

fn build_services() -> Vec<ServiceCatalogEntry> {
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
            os_boundary: "external browser surface opened by OS launcher",
            secret_boundary: "No API key or token is passed to Goblins OS clients.",
            readiness: "Opens the official ChatGPT surface when cloud OpenAI policy allows it."
                .to_string(),
        },
        ServiceCatalogEntry {
            id: "codex",
            name: "Codex",
            role: "Coding agent, app builder, and Build Studio engine",
            launch: "https://chatgpt.com/codex",
            policy_control: "cloud-openai",
            status: codex_service_status(),
            api_surface: "codex",
            sdk: "OpenAI Codex CLI / Codex SDK account-owned path",
            os_boundary: "Goblins OS drives Codex in an OS-owned workspace; Codex owns account credentials.",
            secret_boundary: "Codex credentials stay under CODEX_HOME and are never returned by the core API.",
            readiness: codex_readiness(),
        },
        ServiceCatalogEntry {
            id: "build-studio",
            name: "Build Studio",
            role: "The OS app model: create applications from intent instead of installing them",
            launch: "local://goblins-os/apps/build",
            policy_control: "app-builder",
            status: configured_service_status(
                "app-builder",
                crate::resident::resident_engine_ready() || agents_sdk_relay_configured(),
                ServiceStatus::ServerGated,
            ),
            api_surface: "resident-generate",
            sdk: "Official OpenAI Agents SDK relay when configured; Codex when the account engine is selected; Responses API when BYO OpenAI is selected; local GPT-OSS otherwise",
            os_boundary: "Rust Build Studio owns policy, storage, and approvals; configured Agents SDK relays own tools, handoffs, guardrails, tracing, and sandbox execution server-side.",
            secret_boundary: "Build Studio never receives raw API keys, account tokens, SDK relay credentials, or tool credentials.",
            readiness: if agents_sdk_relay_configured() {
                "Ready through the server-side official OpenAI Agents SDK relay.".to_string()
            } else if crate::resident::resident_engine_ready() {
                "Ready through the active Goblins AI engine.".to_string()
            } else {
                "Waiting for GPT-OSS, Codex sign-in, BYO OpenAI key, or an OS-owned SDK relay."
                    .to_string()
            },
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
                responses_api_configured(),
                ServiceStatus::ServerGated,
            ),
            api_surface: "/v1/responses",
            sdk: "Official OpenAI API surface called from Rust over the server-side core",
            os_boundary: "The Rust core owns HTTPS calls; clients call only Goblins OS localhost routes.",
            secret_boundary: "BYO API keys stay in OS-owned 0600 storage or behind a server-side relay.",
            readiness: if responses_api_configured() {
                "Configured for server-side Responses API calls.".to_string()
            } else {
                "Add a BYO OpenAI key or configure a server-side OpenAI relay.".to_string()
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
            sdk: "Official OpenAI Realtime/WebRTC or relay SDK path, server-side only",
            os_boundary: "Goblins voice keeps local wake/STT/TTS available and uses OpenAI Realtime only through a configured OS relay.",
            secret_boundary: "Realtime session secrets are minted by OS services or a server relay, never by desktop clients.",
            readiness: relay_readiness(
                "GOBLINS_OS_REALTIME_RELAY_URL",
                "OPENAI_OS_REALTIME_RELAY_URL",
                "Realtime relay",
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
            sdk: "Official OpenAI Images API via OS-owned relay",
            os_boundary: "Image requests route through local OS services before any hosted call.",
            secret_boundary: "Image API credentials stay in OS services or a server-side relay.",
            readiness: relay_readiness(
                "GOBLINS_OS_IMAGES_RELAY_URL",
                "OPENAI_OS_IMAGES_RELAY_URL",
                "Images relay",
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
            sdk: "Official OpenAI Agents SDK for Python/TypeScript behind an OS-owned server relay",
            os_boundary: "Rust policy and permission gates stay in Goblins OS; the SDK runner owns tools, handoffs, guardrails, tracing, and sandbox execution server-side.",
            secret_boundary: "Agent API keys and tool credentials stay in the SDK relay, never in GUI clients.",
            readiness: relay_readiness(
                "GOBLINS_OS_AGENTS_SDK_RELAY_URL",
                "OPENAI_OS_AGENTS_SDK_RELAY_URL",
                "Agents SDK relay",
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
            sdk: "Official OpenAI ChatKit through an OS-owned relay",
            os_boundary: "Native Goblins OS surfaces remain GTK/Rust; web ChatKit is available only through an explicit relay-backed surface.",
            secret_boundary: "ChatKit client sessions are brokered by OS services, not embedded with static secrets.",
            readiness: relay_readiness(
                "GOBLINS_OS_CHATKIT_RELAY_URL",
                "OPENAI_OS_CHATKIT_RELAY_URL",
                "ChatKit relay",
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
            secret_boundary: "Settings receives booleans and paths, never API keys or account tokens.",
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

fn codex_service_status() -> ServiceStatus {
    match policy_state_for_control("cloud-openai") {
        PolicyControlState::Allowed if crate::codex::codex_available() => {
            ServiceStatus::ServerGated
        }
        PolicyControlState::Allowed => ServiceStatus::NotConfigured,
        PolicyControlState::Denied => ServiceStatus::PolicyBlocked,
        PolicyControlState::PermissionGated => ServiceStatus::PermissionGated,
    }
}

fn codex_readiness() -> String {
    if crate::codex::codex_available() {
        "Codex is installed and signed in with the user's OpenAI account.".to_string()
    } else if crate::codex::codex_installed() {
        "Codex is installed; sign in with the user's OpenAI account before selecting it."
            .to_string()
    } else {
        "Start from the full Goblins OS image with Codex included before using the account-owned builder."
            .to_string()
    }
}

fn responses_api_configured() -> bool {
    crate::openai_key::stored_api_key().is_some() || cloud_relay_configured()
}

fn cloud_relay_configured() -> bool {
    relay_configured(
        "GOBLINS_OS_RESIDENT_RELAY_URL",
        "OPENAI_OS_RESIDENT_RELAY_URL",
    )
}

fn agents_sdk_relay_configured() -> bool {
    relay_configured(
        "GOBLINS_OS_AGENTS_SDK_RELAY_URL",
        "OPENAI_OS_AGENTS_SDK_RELAY_URL",
    )
}

fn relay_configured(primary_url_var: &str, legacy_url_var: &str) -> bool {
    (env::var_os(primary_url_var).is_some() || env::var_os(legacy_url_var).is_some())
        && env::var_os("AI_GATEWAY_API_KEY").is_some()
}

fn relay_readiness(primary_url_var: &str, legacy_url_var: &str, label: &str) -> String {
    if relay_configured(primary_url_var, legacy_url_var) {
        format!("{label} is configured through OS-owned secret storage.")
    } else {
        format!("{label} is not configured; add a server-side relay before enabling it.")
    }
}

#[cfg(test)]
mod tests {
    use super::build_services;
    use std::collections::HashSet;

    #[test]
    fn every_service_opens_a_real_openai_surface_or_an_os_owned_action() {
        for service in build_services() {
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
        let launches: Vec<&str> = build_services()
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
        let services = build_services();
        let responses = services
            .iter()
            .find(|service| service.id == "responses-api")
            .expect("responses service");
        assert_eq!(responses.api_surface, "/v1/responses");
        assert!(responses.sdk.contains("Rust"));
        assert!(responses.secret_boundary.contains("0600"));

        let agents = services
            .iter()
            .find(|service| service.id == "agents")
            .expect("agents service");
        assert!(agents.sdk.contains("Official OpenAI Agents SDK"));
        assert!(agents.os_boundary.contains("Rust policy"));
        assert!(agents.secret_boundary.contains("never in GUI clients"));

        let build_studio = services
            .iter()
            .find(|service| service.id == "build-studio")
            .expect("build studio service");
        assert!(build_studio.sdk.contains("Official OpenAI Agents SDK"));
        assert!(build_studio.sdk.contains("Codex"));
        assert!(build_studio.sdk.contains("Responses API"));
        assert!(build_studio.os_boundary.contains("approvals"));
        assert!(build_studio.os_boundary.contains("sandbox execution"));

        assert!(
            services.iter().any(|service| service.id == "chatkit"),
            "ChatKit must be a first-class configured-or-disabled surface"
        );
    }
}
