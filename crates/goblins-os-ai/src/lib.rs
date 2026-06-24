//! Shared Goblins AI action contract.
//!
//! This crate is intentionally small and data-only: the core daemon owns policy,
//! model relays, and action execution; resident and native UI surfaces can all
//! read the same registry without inventing separate labels or permission names.

use serde::{Deserialize, Serialize};

pub const REGISTRY_VERSION: &str = "2026-06-22.ai-native-os-actions.v2";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiActionKind {
    Answer,
    OpenSettings,
    ChangeSetting,
    LaunchItem,
    Write,
    Summarize,
    Troubleshoot,
    BuildApp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiContextKind {
    Global,
    SelectedText,
    ActiveWindow,
    Screenshot,
    FileSystem,
    Settings,
    Notifications,
    SystemStatus,
    AppBuilder,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiPermission {
    ResidentAssistant,
    ScreenContext,
    FileContext,
    SettingsControl,
    NotificationContext,
    SystemTroubleshooting,
    AppBuilder,
    ComputerUse,
}

impl AiPermission {
    pub const fn control_id(self) -> &'static str {
        match self {
            Self::ResidentAssistant => "resident-assistant",
            Self::ScreenContext => "screen-context",
            Self::FileContext => "file-context",
            Self::SettingsControl => "settings-control",
            Self::NotificationContext => "notification-context",
            Self::SystemTroubleshooting => "system-troubleshooting",
            Self::AppBuilder => "app-builder",
            Self::ComputerUse => "computer-use",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiConfirmation {
    None,
    ExplicitConfirmation,
    PermissionAndConfirmation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiEntrypoint {
    KeyboardShortcut,
    Launcher,
    ControlCenter,
    Settings,
    SelectedText,
    Screenshot,
    Files,
    Notifications,
    Troubleshooting,
    AppBuilder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AiAction {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub kind: AiActionKind,
    pub contexts: &'static [AiContextKind],
    pub permission: AiPermission,
    pub confirmation: AiConfirmation,
    pub entrypoints: &'static [AiEntrypoint],
    pub route_hint: &'static str,
    pub enabled_without_engine: bool,
}

const GLOBAL_CONTEXTS: &[AiContextKind] = &[AiContextKind::Global, AiContextKind::SystemStatus];
const SETTINGS_CONTEXTS: &[AiContextKind] = &[AiContextKind::Settings, AiContextKind::SystemStatus];
const SELECTED_TEXT_CONTEXTS: &[AiContextKind] =
    &[AiContextKind::SelectedText, AiContextKind::ActiveWindow];
const SCREEN_CONTEXTS: &[AiContextKind] = &[AiContextKind::Screenshot, AiContextKind::ActiveWindow];
const FILE_CONTEXTS: &[AiContextKind] = &[AiContextKind::FileSystem];
const NOTIFICATION_CONTEXTS: &[AiContextKind] =
    &[AiContextKind::Notifications, AiContextKind::SystemStatus];
const APP_BUILDER_CONTEXTS: &[AiContextKind] = &[AiContextKind::AppBuilder, AiContextKind::Global];

const GLOBAL_ENTRYPOINTS: &[AiEntrypoint] = &[
    AiEntrypoint::KeyboardShortcut,
    AiEntrypoint::Launcher,
    AiEntrypoint::ControlCenter,
    AiEntrypoint::Settings,
];
const TROUBLESHOOT_ENTRYPOINTS: &[AiEntrypoint] = &[
    AiEntrypoint::Settings,
    AiEntrypoint::Notifications,
    AiEntrypoint::Troubleshooting,
];

pub const ACTIONS: &[AiAction] = &[
    AiAction {
        id: "ask-goblins",
        title: "Ask Goblin",
        detail: "Answer a question using the active model while keeping prompts and keys inside OS-owned model access.",
        kind: AiActionKind::Answer,
        contexts: GLOBAL_CONTEXTS,
        permission: AiPermission::ResidentAssistant,
        confirmation: AiConfirmation::None,
        entrypoints: GLOBAL_ENTRYPOINTS,
        route_hint: "resident.chat",
        enabled_without_engine: false,
    },
    AiAction {
        id: "open-settings-panel",
        title: "Open the right Settings panel",
        detail: "Route a natural-language request to the exact Goblins OS Settings panel or native device panel.",
        kind: AiActionKind::OpenSettings,
        contexts: SETTINGS_CONTEXTS,
        permission: AiPermission::ResidentAssistant,
        confirmation: AiConfirmation::None,
        entrypoints: GLOBAL_ENTRYPOINTS,
        route_hint: "settings.open-panel",
        enabled_without_engine: true,
    },
    AiAction {
        id: "change-safe-setting",
        title: "Change a safe setting",
        detail: "Prepare a supported setting change, explain the effect, and apply it only after explicit confirmation.",
        kind: AiActionKind::ChangeSetting,
        contexts: SETTINGS_CONTEXTS,
        permission: AiPermission::SettingsControl,
        confirmation: AiConfirmation::PermissionAndConfirmation,
        entrypoints: &[AiEntrypoint::Settings, AiEntrypoint::ControlCenter],
        route_hint: "settings.confirmed-change",
        enabled_without_engine: false,
    },
    AiAction {
        id: "ask-selected-text",
        title: "Ask about selected text",
        detail: "Use the current selection as private context after the user invokes Ask Goblin.",
        kind: AiActionKind::Answer,
        contexts: SELECTED_TEXT_CONTEXTS,
        permission: AiPermission::ScreenContext,
        confirmation: AiConfirmation::ExplicitConfirmation,
        entrypoints: &[AiEntrypoint::SelectedText, AiEntrypoint::KeyboardShortcut],
        route_hint: "context.selected-text",
        enabled_without_engine: false,
    },
    AiAction {
        id: "write-with-goblins",
        title: "Write with Goblin",
        detail: "Rewrite, proofread, summarize, or change tone for selected text only after the user invokes writing help.",
        kind: AiActionKind::Write,
        contexts: SELECTED_TEXT_CONTEXTS,
        permission: AiPermission::ScreenContext,
        confirmation: AiConfirmation::ExplicitConfirmation,
        entrypoints: &[
            AiEntrypoint::SelectedText,
            AiEntrypoint::KeyboardShortcut,
            AiEntrypoint::Launcher,
            AiEntrypoint::ControlCenter,
        ],
        route_hint: "context.selected-text.write",
        enabled_without_engine: false,
    },
    AiAction {
        id: "summarize-screen",
        title: "Summarize what is on screen",
        detail: "Use screenshot or current-window context only after consent, with no silent capture.",
        kind: AiActionKind::Summarize,
        contexts: SCREEN_CONTEXTS,
        permission: AiPermission::ScreenContext,
        confirmation: AiConfirmation::PermissionAndConfirmation,
        entrypoints: &[AiEntrypoint::Screenshot, AiEntrypoint::KeyboardShortcut],
        route_hint: "context.screenshot",
        enabled_without_engine: false,
    },
    AiAction {
        id: "ask-file-or-folder",
        title: "Ask about a file or folder",
        detail: "Read only the chosen file or folder path and keep the action audit-visible.",
        kind: AiActionKind::Summarize,
        contexts: FILE_CONTEXTS,
        permission: AiPermission::FileContext,
        confirmation: AiConfirmation::ExplicitConfirmation,
        entrypoints: &[AiEntrypoint::Files, AiEntrypoint::Launcher],
        route_hint: "context.file",
        enabled_without_engine: false,
    },
    AiAction {
        id: "explain-system-status",
        title: "Explain system status",
        detail: "Explain Goblins AI, model, network, storage, display, audio, battery, and service state from OS status.",
        kind: AiActionKind::Troubleshoot,
        contexts: GLOBAL_CONTEXTS,
        permission: AiPermission::SystemTroubleshooting,
        confirmation: AiConfirmation::None,
        entrypoints: TROUBLESHOOT_ENTRYPOINTS,
        route_hint: "system.status",
        enabled_without_engine: false,
    },
    AiAction {
        id: "troubleshoot-network-audio-display-storage",
        title: "Troubleshoot network, audio, display, or storage",
        detail: "Inspect real OS status and propose reversible fixes; changes require confirmation.",
        kind: AiActionKind::Troubleshoot,
        contexts: SETTINGS_CONTEXTS,
        permission: AiPermission::SystemTroubleshooting,
        confirmation: AiConfirmation::PermissionAndConfirmation,
        entrypoints: TROUBLESHOOT_ENTRYPOINTS,
        route_hint: "system.troubleshoot",
        enabled_without_engine: false,
    },
    AiAction {
        id: "answer-notification",
        title: "Act on a notification",
        detail: "Use only the invoked notification and ask before launching an app or changing state.",
        kind: AiActionKind::LaunchItem,
        contexts: NOTIFICATION_CONTEXTS,
        permission: AiPermission::NotificationContext,
        confirmation: AiConfirmation::ExplicitConfirmation,
        entrypoints: &[AiEntrypoint::Notifications],
        route_hint: "notification.context-action",
        enabled_without_engine: false,
    },
    AiAction {
        id: "build-app",
        title: "Build an app",
        detail: "Send the request to the OS app-builder path under sandbox and policy controls.",
        kind: AiActionKind::BuildApp,
        contexts: APP_BUILDER_CONTEXTS,
        permission: AiPermission::AppBuilder,
        confirmation: AiConfirmation::PermissionAndConfirmation,
        entrypoints: &[AiEntrypoint::Launcher, AiEntrypoint::AppBuilder, AiEntrypoint::Files],
        route_hint: "apps.builds",
        enabled_without_engine: false,
    },
];

pub fn action_registry() -> &'static [AiAction] {
    ACTIONS
}

pub fn action_by_id(id: &str) -> Option<&'static AiAction> {
    ACTIONS.iter().find(|action| action.id == id)
}

#[cfg(test)]
mod tests {
    use super::{action_by_id, action_registry, AiConfirmation, AiPermission};
    use std::collections::HashSet;

    #[test]
    fn registry_ids_are_unique_and_cover_system_entrypoints() {
        let mut ids = HashSet::new();
        for action in action_registry() {
            assert!(ids.insert(action.id), "duplicate action id {}", action.id);
            assert!(!action.entrypoints.is_empty());
            assert!(!action.contexts.is_empty());
            assert!(!action.route_hint.is_empty());
        }
        assert!(action_by_id("ask-goblins").is_some());
        assert!(action_by_id("build-app").is_some());
        assert!(action_by_id("write-with-goblins").is_some());
        assert!(action_by_id("summarize-screen").is_some());
    }

    #[test]
    fn sensitive_actions_require_confirmation() {
        for id in [
            "change-safe-setting",
            "write-with-goblins",
            "summarize-screen",
            "ask-file-or-folder",
            "answer-notification",
            "build-app",
        ] {
            let action = action_by_id(id).unwrap();
            assert_ne!(action.confirmation, AiConfirmation::None);
        }
    }

    #[test]
    fn permission_names_match_policy_control_ids() {
        assert_eq!(
            AiPermission::ResidentAssistant.control_id(),
            "resident-assistant"
        );
        assert_eq!(AiPermission::ScreenContext.control_id(), "screen-context");
        assert_eq!(
            AiPermission::SettingsControl.control_id(),
            "settings-control"
        );
        assert_eq!(AiPermission::AppBuilder.control_id(), "app-builder");
    }
}
