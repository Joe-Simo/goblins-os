use axum::{http::StatusCode, Json};
use goblins_os_ai::{action_registry, AiAction, AiConfirmation, AiEntrypoint, REGISTRY_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    policy::{policy_state_for_control, PolicyControlState},
    resident::{active_engine_label, resident_engine_ready},
};

const DEFAULT_AI_STATE_DIR: &str = "/var/lib/goblins-os/ai";
const MAX_HISTORY_EVENTS: usize = 80;
const OPEN_SETTINGS_PANEL_ACTION_ID: &str = "open-settings-panel";

#[derive(Serialize)]
pub struct AiActionCatalog {
    generated_at: String,
    source: &'static str,
    registry_version: &'static str,
    engine: AiEngineStatus,
    permission_model: &'static str,
    actions: Vec<AiActionStatus>,
}

#[derive(Serialize)]
pub struct AiEngineStatus {
    selected: &'static str,
    ready: bool,
    detail: String,
}

#[derive(Serialize)]
pub struct AiActionStatus {
    id: &'static str,
    title: &'static str,
    detail: &'static str,
    kind: goblins_os_ai::AiActionKind,
    contexts: &'static [goblins_os_ai::AiContextKind],
    permission: goblins_os_ai::AiPermission,
    permission_control: &'static str,
    confirmation: AiConfirmation,
    entrypoints: &'static [goblins_os_ai::AiEntrypoint],
    route_hint: &'static str,
    state: AiActionReadiness,
    enabled: bool,
    reason: String,
}

#[derive(Serialize)]
pub struct AiActionHistory {
    generated_at: u64,
    source: &'static str,
    state_path: String,
    retention: String,
    events: Vec<AiActionHistoryEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct AiActionHistoryEvent {
    action_id: String,
    title: String,
    outcome: AiActionOutcome,
    entrypoint: String,
    permission_control: String,
    confirmation: AiConfirmation,
    occurred_at: u64,
    detail: String,
}

#[derive(Deserialize)]
pub struct RecordAiActionRequest {
    action_id: String,
    outcome: AiActionOutcome,
    entrypoint: Option<String>,
}

#[derive(Serialize)]
pub struct RecordAiActionResponse {
    ok: bool,
    text: String,
    event: Option<AiActionHistoryEvent>,
}

#[derive(Deserialize)]
pub struct FileContextRequest {
    path: String,
}

#[derive(Deserialize)]
pub struct SettingsContextRequest {
    panel: String,
    topic: Option<String>,
    question: Option<String>,
    status_summary: Option<String>,
}

#[derive(Deserialize)]
pub struct OpenSettingsPanelRequest {
    query: String,
    source_panel: Option<String>,
}

#[derive(Deserialize)]
pub struct SystemStatusContextRequest {
    focus: Option<String>,
    question: Option<String>,
    status_summary: Option<String>,
}

#[derive(Deserialize)]
pub struct SafeSettingChangeRequest {
    setting_id: String,
    value: Value,
    confirmed: Option<bool>,
}

#[derive(Deserialize)]
pub struct SelectedTextContextRequest {
    text: String,
    app: Option<String>,
    window_title: Option<String>,
    question: Option<String>,
}

#[derive(Deserialize)]
pub struct ScreenContextRequest {
    source: Option<String>,
    app: Option<String>,
    window_title: Option<String>,
    visible_text: Option<String>,
    visual_summary: Option<String>,
    question: Option<String>,
}

#[derive(Deserialize)]
pub struct NotificationContextRequest {
    app: Option<String>,
    title: Option<String>,
    body: Option<String>,
    action_label: Option<String>,
    question: Option<String>,
}

#[derive(Serialize)]
pub struct FileContextResponse {
    ok: bool,
    text: String,
    context: Option<FileContextSummary>,
}

#[derive(Serialize)]
pub struct SettingsContextResponse {
    ok: bool,
    text: String,
    context: Option<SettingsContextSummary>,
}

#[derive(Serialize)]
pub struct OpenSettingsPanelResponse {
    ok: bool,
    text: String,
    panel: Option<OpenSettingsPanelSummary>,
}

#[derive(Serialize)]
pub struct SystemStatusContextResponse {
    ok: bool,
    text: String,
    context: Option<SystemStatusContextSummary>,
}

#[derive(Serialize)]
pub struct SafeSettingChangeResponse {
    ok: bool,
    applied: bool,
    text: String,
    change: Option<SafeSettingChangeSummary>,
}

#[derive(Serialize)]
pub struct SelectedTextContextResponse {
    ok: bool,
    text: String,
    context: Option<SelectedTextContextSummary>,
}

#[derive(Serialize)]
pub struct ScreenContextResponse {
    ok: bool,
    text: String,
    context: Option<ScreenContextSummary>,
}

#[derive(Serialize)]
pub struct NotificationContextResponse {
    ok: bool,
    text: String,
    context: Option<NotificationContextSummary>,
}

#[derive(Serialize)]
pub struct FileContextSummary {
    name: String,
    kind: &'static str,
    extension: Option<String>,
    size_bytes: Option<u64>,
}

#[derive(Serialize)]
pub struct SettingsContextSummary {
    panel: String,
    topic: String,
    route_hint: String,
}

#[derive(Clone, Serialize)]
pub struct OpenSettingsPanelSummary {
    panel_id: &'static str,
    title: &'static str,
    route_hint: String,
    launch_argument: String,
    confidence: &'static str,
    reason: String,
}

#[derive(Serialize)]
pub struct SystemStatusContextSummary {
    source: &'static str,
    focus: String,
    route_hint: &'static str,
    action_id: &'static str,
    snapshot_chars: usize,
    included: &'static [&'static str],
}

#[derive(Clone, Serialize)]
pub struct SafeSettingChangeSummary {
    setting_id: &'static str,
    title: &'static str,
    requested_value: String,
    effect: &'static str,
}

#[derive(Serialize)]
pub struct SelectedTextContextSummary {
    app: Option<String>,
    window_title: Option<String>,
    text_chars: usize,
}

#[derive(Serialize)]
pub struct ScreenContextSummary {
    source: String,
    app: Option<String>,
    window_title: Option<String>,
    visible_text_chars: usize,
    visual_summary_chars: usize,
}

#[derive(Serialize)]
pub struct NotificationContextSummary {
    source: &'static str,
    app: Option<String>,
    title_chars: usize,
    body_chars: usize,
    action_label: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AiActionOutcome {
    Started,
    Succeeded,
    Failed,
    Blocked,
    PermissionGated,
    ConfirmationRequired,
    Denied,
}

#[derive(Serialize, Deserialize)]
struct StoredAiActionHistory {
    events: Vec<AiActionHistoryEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiActionReadiness {
    Ready,
    WaitingForEngine,
    PermissionGated,
    ConfirmationRequired,
    Denied,
}

pub async fn ai_action_catalog() -> Json<AiActionCatalog> {
    Json(build_ai_action_catalog())
}

pub async fn ai_action_history() -> Json<AiActionHistory> {
    Json(build_ai_action_history())
}

pub async fn record_ai_action_history(
    Json(request): Json<RecordAiActionRequest>,
) -> (StatusCode, Json<RecordAiActionResponse>) {
    match append_ai_action_history(&request.action_id, request.entrypoint.as_deref(), request.outcome)
    {
        Ok(event) => (
            StatusCode::OK,
            Json(RecordAiActionResponse {
                ok: true,
                text: "Goblins AI action recorded without prompt, response, screen, file, or secret content.".to_string(),
                event: Some(event),
            }),
        ),
        Err(detail) => (
            StatusCode::BAD_REQUEST,
            Json(RecordAiActionResponse {
                ok: false,
                text: detail,
                event: None,
            }),
        ),
    }
}

pub async fn ask_file_context(
    Json(request): Json<FileContextRequest>,
) -> (StatusCode, Json<FileContextResponse>) {
    let selected = request.path.trim();
    if selected.is_empty() || selected.chars().count() > 4096 {
        audit_ai_action(
            "ask-file-or-folder",
            Some("files"),
            AiActionOutcome::Blocked,
        );
        return file_context_outcome(
            StatusCode::BAD_REQUEST,
            "Choose one local file or folder before asking Goblins AI.".to_string(),
            None,
        );
    }

    match policy_state_for_control("file-context") {
        PolicyControlState::Allowed => {}
        PolicyControlState::Denied => {
            audit_ai_action("ask-file-or-folder", Some("files"), AiActionOutcome::Denied);
            return file_context_outcome(
                StatusCode::FORBIDDEN,
                "File and folder context is blocked by the active Goblins OS policy profile."
                    .to_string(),
                None,
            );
        }
        PolicyControlState::PermissionGated => {
            audit_ai_action(
                "ask-file-or-folder",
                Some("files"),
                AiActionOutcome::PermissionGated,
            );
            return file_context_outcome(
                StatusCode::FORBIDDEN,
                "Allow file and folder context in Privacy & Permissions before Goblins AI can inspect a selected item.".to_string(),
                None,
            );
        }
    }

    let path = Path::new(selected);
    let context = summarize_selected_path(path);
    let prompt = file_context_prompt(&context);
    match crate::resident::resident_generate(&prompt) {
        Ok(answer) => {
            audit_ai_action(
                "ask-file-or-folder",
                Some("files"),
                AiActionOutcome::Succeeded,
            );
            file_context_outcome(StatusCode::OK, answer, Some(context))
        }
        Err(detail) => {
            audit_ai_action(
                "ask-file-or-folder",
                Some("files"),
                AiActionOutcome::Blocked,
            );
            file_context_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Goblins AI needs GPT-OSS, Codex sign-in, or a BYO OpenAI key before it can answer about the selected item: {detail}."),
                Some(context),
            )
        }
    }
}

pub async fn ask_settings_context(
    Json(request): Json<SettingsContextRequest>,
) -> (StatusCode, Json<SettingsContextResponse>) {
    let context = summarize_settings_context(&request);
    if context.panel.is_empty() {
        audit_ai_action(
            "explain-system-status",
            Some("settings"),
            AiActionOutcome::Blocked,
        );
        return settings_context_outcome(
            StatusCode::BAD_REQUEST,
            "Choose a Settings panel before asking Goblins AI.".to_string(),
            None,
        );
    }

    match policy_state_for_control("system-troubleshooting") {
        PolicyControlState::Allowed => {}
        PolicyControlState::Denied => {
            audit_ai_action(
                "explain-system-status",
                Some("settings"),
                AiActionOutcome::Denied,
            );
            return settings_context_outcome(
                StatusCode::FORBIDDEN,
                "Settings help is blocked by the active Goblins OS policy profile.".to_string(),
                Some(context),
            );
        }
        PolicyControlState::PermissionGated => {
            audit_ai_action(
                "explain-system-status",
                Some("settings"),
                AiActionOutcome::PermissionGated,
            );
            return settings_context_outcome(
                StatusCode::FORBIDDEN,
                "Allow system troubleshooting in Privacy & Permissions before Goblins AI can inspect Settings context.".to_string(),
                Some(context),
            );
        }
    }

    let action_id = settings_context_action_id(&context, request.question.as_deref());
    let prompt = settings_context_prompt(
        &context,
        request.question.as_deref(),
        request.status_summary.as_deref(),
    );
    match crate::resident::resident_generate(&prompt) {
        Ok(answer) => {
            audit_ai_action(action_id, Some("settings"), AiActionOutcome::Succeeded);
            settings_context_outcome(StatusCode::OK, answer, Some(context))
        }
        Err(detail) => {
            audit_ai_action(action_id, Some("settings"), AiActionOutcome::Blocked);
            settings_context_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Goblins AI needs GPT-OSS, Codex sign-in, or a BYO OpenAI key before it can answer about this Settings panel: {detail}."),
                Some(context),
            )
        }
    }
}

pub async fn open_settings_panel(
    Json(request): Json<OpenSettingsPanelRequest>,
) -> (StatusCode, Json<OpenSettingsPanelResponse>) {
    let query = sanitized_context_value(&request.query, 240);
    if query.is_empty() {
        audit_open_settings_panel(AiActionOutcome::Blocked);
        return open_settings_panel_outcome(
            StatusCode::BAD_REQUEST,
            "Describe the setting or system area you want to open.".to_string(),
            None,
        );
    }

    match policy_state_for_control("resident-assistant") {
        PolicyControlState::Allowed => {}
        PolicyControlState::Denied => {
            audit_open_settings_panel(AiActionOutcome::Denied);
            return open_settings_panel_outcome(
                StatusCode::FORBIDDEN,
                "Opening Settings from Goblins AI is blocked by the active Goblins OS policy profile.".to_string(),
                None,
            );
        }
        PolicyControlState::PermissionGated => {
            audit_open_settings_panel(AiActionOutcome::PermissionGated);
            return open_settings_panel_outcome(
                StatusCode::FORBIDDEN,
                "Allow the Goblins AI assistant in Privacy & Permissions before it can route Settings requests.".to_string(),
                None,
            );
        }
    }

    let panel = resolve_open_settings_panel(&query, request.source_panel.as_deref());
    audit_open_settings_panel(AiActionOutcome::Succeeded);
    open_settings_panel_outcome(
        StatusCode::OK,
        format!(
            "Open Settings > {}. Route: {}.",
            panel.title, panel.launch_argument
        ),
        Some(panel),
    )
}

fn audit_open_settings_panel(outcome: AiActionOutcome) {
    audit_ai_action(OPEN_SETTINGS_PANEL_ACTION_ID, Some("settings"), outcome);
}

pub async fn ask_system_status(
    Json(request): Json<SystemStatusContextRequest>,
) -> (StatusCode, Json<SystemStatusContextResponse>) {
    let focus = request
        .focus
        .as_deref()
        .map(|value| sanitized_context_value(value, 120))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "System status".to_string());
    let question = request
        .question
        .as_deref()
        .map(|value| sanitized_context_value(value, 480))
        .filter(|value| !value.is_empty());
    let caller_summary = request
        .status_summary
        .as_deref()
        .map(|value| sanitized_context_value(value, 1000))
        .filter(|value| !value.is_empty());

    match system_troubleshooting_policy() {
        Ok(()) => {}
        Err((status, text, outcome)) => {
            audit_ai_action("explain-system-status", Some("troubleshooting"), outcome);
            return system_status_context_outcome(status, text, None);
        }
    }

    let snapshot = bounded_system_status_snapshot();
    let action_id = system_status_action_id(&focus, question.as_deref());
    let context = SystemStatusContextSummary {
        source: "goblins-os-core",
        focus: focus.clone(),
        route_hint: if action_id == "troubleshoot-network-audio-display-storage" {
            "system.troubleshoot"
        } else {
            "system.status"
        },
        action_id,
        snapshot_chars: snapshot.chars().count(),
        included: &[
            "readiness",
            "services",
            "hardware",
            "local-models",
            "policy",
        ],
    };
    let prompt = system_status_prompt(
        &context,
        &snapshot,
        question.as_deref(),
        caller_summary.as_deref(),
    );

    match crate::resident::resident_generate(&prompt) {
        Ok(answer) => {
            audit_ai_action(
                action_id,
                Some("troubleshooting"),
                AiActionOutcome::Succeeded,
            );
            system_status_context_outcome(StatusCode::OK, answer, Some(context))
        }
        Err(detail) => {
            audit_ai_action(action_id, Some("troubleshooting"), AiActionOutcome::Blocked);
            system_status_context_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Goblins AI needs GPT-OSS, Codex sign-in, or a BYO OpenAI key before it can summarize system status: {detail}."),
                Some(context),
            )
        }
    }
}

pub async fn change_safe_setting(
    Json(request): Json<SafeSettingChangeRequest>,
) -> (StatusCode, Json<SafeSettingChangeResponse>) {
    let change = match safe_setting_change_summary(&request.setting_id, &request.value) {
        Ok(change) => change,
        Err(text) => {
            audit_ai_action(
                "change-safe-setting",
                Some("settings"),
                AiActionOutcome::Blocked,
            );
            return safe_setting_change_outcome(StatusCode::BAD_REQUEST, false, text, None);
        }
    };

    match settings_control_policy() {
        Ok(()) => {}
        Err((status, text, outcome)) => {
            audit_ai_action("change-safe-setting", Some("settings"), outcome);
            return safe_setting_change_outcome(status, false, text, Some(change));
        }
    }

    if request.confirmed != Some(true) {
        audit_ai_action(
            "change-safe-setting",
            Some("settings"),
            AiActionOutcome::ConfirmationRequired,
        );
        return safe_setting_change_outcome(
            StatusCode::PRECONDITION_REQUIRED,
            false,
            format!(
                "Review and confirm before applying {}. Requested value: {}. Effect: {}.",
                change.title, change.requested_value, change.effect
            ),
            Some(change),
        );
    }

    let (status, text) = apply_safe_setting_change(&change);
    audit_ai_action(
        "change-safe-setting",
        Some("settings"),
        safe_setting_change_audit_outcome(status),
    );
    safe_setting_change_outcome(status, status.is_success(), text, Some(change))
}

pub async fn ask_selected_text_context(
    Json(request): Json<SelectedTextContextRequest>,
) -> (StatusCode, Json<SelectedTextContextResponse>) {
    let selected = sanitized_context_value(&request.text, 6000);
    if selected.is_empty() {
        audit_ai_action(
            "ask-selected-text",
            Some("selected-text"),
            AiActionOutcome::Blocked,
        );
        return selected_text_context_outcome(
            StatusCode::BAD_REQUEST,
            "Select text before asking Goblins AI about it.".to_string(),
            None,
        );
    }

    match screen_context_policy() {
        Ok(()) => {}
        Err((status, text, outcome)) => {
            audit_ai_action("ask-selected-text", Some("selected-text"), outcome);
            return selected_text_context_outcome(status, text, None);
        }
    }

    let context = SelectedTextContextSummary {
        app: request
            .app
            .as_deref()
            .map(|value| sanitized_context_value(value, 80))
            .filter(|value| !value.is_empty()),
        window_title: request
            .window_title
            .as_deref()
            .map(|value| sanitized_context_value(value, 120))
            .filter(|value| !value.is_empty()),
        text_chars: selected.chars().count(),
    };
    let prompt = selected_text_context_prompt(&selected, &context, request.question.as_deref());

    match crate::resident::resident_generate(&prompt) {
        Ok(answer) => {
            audit_ai_action(
                "ask-selected-text",
                Some("selected-text"),
                AiActionOutcome::Succeeded,
            );
            selected_text_context_outcome(StatusCode::OK, answer, Some(context))
        }
        Err(detail) => {
            audit_ai_action(
                "ask-selected-text",
                Some("selected-text"),
                AiActionOutcome::Blocked,
            );
            selected_text_context_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Goblins AI needs GPT-OSS, Codex sign-in, or a BYO OpenAI key before it can answer about selected text: {detail}."),
                Some(context),
            )
        }
    }
}

pub async fn write_selected_text_context(
    Json(request): Json<SelectedTextContextRequest>,
) -> (StatusCode, Json<SelectedTextContextResponse>) {
    let selected = sanitized_context_value(&request.text, 6000);
    if selected.is_empty() {
        audit_ai_action(
            "write-with-goblins",
            Some("selected-text"),
            AiActionOutcome::Blocked,
        );
        return selected_text_context_outcome(
            StatusCode::BAD_REQUEST,
            "Select or paste text before using Write with Goblin.".to_string(),
            None,
        );
    }

    match screen_context_policy() {
        Ok(()) => {}
        Err((status, text, outcome)) => {
            audit_ai_action("write-with-goblins", Some("selected-text"), outcome);
            return selected_text_context_outcome(status, text, None);
        }
    }

    let context = SelectedTextContextSummary {
        app: request
            .app
            .as_deref()
            .map(|value| sanitized_context_value(value, 80))
            .filter(|value| !value.is_empty()),
        window_title: request
            .window_title
            .as_deref()
            .map(|value| sanitized_context_value(value, 120))
            .filter(|value| !value.is_empty()),
        text_chars: selected.chars().count(),
    };
    let prompt = writing_tools_prompt(&selected, &context, request.question.as_deref());

    match crate::resident::resident_generate(&prompt) {
        Ok(answer) => {
            audit_ai_action(
                "write-with-goblins",
                Some("selected-text"),
                AiActionOutcome::Succeeded,
            );
            selected_text_context_outcome(StatusCode::OK, answer, Some(context))
        }
        Err(detail) => {
            audit_ai_action(
                "write-with-goblins",
                Some("selected-text"),
                AiActionOutcome::Blocked,
            );
            selected_text_context_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Goblins AI needs GPT-OSS, Codex sign-in, or a BYO OpenAI key before it can help write selected text: {detail}."),
                Some(context),
            )
        }
    }
}

pub async fn ask_notification_context(
    Json(request): Json<NotificationContextRequest>,
) -> (StatusCode, Json<NotificationContextResponse>) {
    let title = request
        .title
        .as_deref()
        .map(|value| sanitized_context_value(value, 200))
        .unwrap_or_default();
    let body = request
        .body
        .as_deref()
        .map(|value| sanitized_context_value(value, 1600))
        .unwrap_or_default();
    let action_label = request
        .action_label
        .as_deref()
        .map(|value| sanitized_context_value(value, 96))
        .filter(|value| !value.is_empty());

    if title.is_empty() && body.is_empty() && action_label.is_none() {
        audit_ai_action(
            "answer-notification",
            Some("notifications"),
            AiActionOutcome::Blocked,
        );
        return notification_context_outcome(
            StatusCode::BAD_REQUEST,
            "Choose a notification before asking Goblins AI about it.".to_string(),
            None,
        );
    }

    match notification_context_policy() {
        Ok(()) => {}
        Err((status, text, outcome)) => {
            audit_ai_action("answer-notification", Some("notifications"), outcome);
            return notification_context_outcome(status, text, None);
        }
    }

    let context = NotificationContextSummary {
        source: "user-invoked-notification",
        app: request
            .app
            .as_deref()
            .map(|value| sanitized_context_value(value, 80))
            .filter(|value| !value.is_empty()),
        title_chars: title.chars().count(),
        body_chars: body.chars().count(),
        action_label,
    };
    let prompt = notification_context_prompt(&context, &title, &body, request.question.as_deref());

    match crate::resident::resident_generate(&prompt) {
        Ok(answer) => {
            audit_ai_action(
                "answer-notification",
                Some("notifications"),
                AiActionOutcome::Succeeded,
            );
            notification_context_outcome(StatusCode::OK, answer, Some(context))
        }
        Err(detail) => {
            audit_ai_action(
                "answer-notification",
                Some("notifications"),
                AiActionOutcome::Blocked,
            );
            notification_context_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Goblins AI needs GPT-OSS, Codex sign-in, or a BYO OpenAI key before it can answer about this notification: {detail}."),
                Some(context),
            )
        }
    }
}

pub async fn ask_screen_context(
    Json(request): Json<ScreenContextRequest>,
) -> (StatusCode, Json<ScreenContextResponse>) {
    let visible_text = request
        .visible_text
        .as_deref()
        .map(|value| sanitized_context_value(value, 5000))
        .unwrap_or_default();
    let visual_summary = request
        .visual_summary
        .as_deref()
        .map(|value| sanitized_context_value(value, 1800))
        .unwrap_or_default();

    if visible_text.is_empty() && visual_summary.is_empty() {
        audit_ai_action(
            "summarize-screen",
            Some("screenshot"),
            AiActionOutcome::Blocked,
        );
        return screen_context_outcome(
            StatusCode::BAD_REQUEST,
            "Capture or describe the visible window before asking Goblins AI about the screen."
                .to_string(),
            None,
        );
    }

    match screen_context_policy() {
        Ok(()) => {}
        Err((status, text, outcome)) => {
            audit_ai_action("summarize-screen", Some("screenshot"), outcome);
            return screen_context_outcome(status, text, None);
        }
    }

    let context = ScreenContextSummary {
        source: request
            .source
            .as_deref()
            .map(|value| sanitized_context_value(value, 80))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "user-invoked-screen-context".to_string()),
        app: request
            .app
            .as_deref()
            .map(|value| sanitized_context_value(value, 80))
            .filter(|value| !value.is_empty()),
        window_title: request
            .window_title
            .as_deref()
            .map(|value| sanitized_context_value(value, 120))
            .filter(|value| !value.is_empty()),
        visible_text_chars: visible_text.chars().count(),
        visual_summary_chars: visual_summary.chars().count(),
    };
    let prompt = screen_context_prompt(
        &context,
        &visible_text,
        &visual_summary,
        request.question.as_deref(),
    );

    match crate::resident::resident_generate(&prompt) {
        Ok(answer) => {
            audit_ai_action(
                "summarize-screen",
                Some("screenshot"),
                AiActionOutcome::Succeeded,
            );
            screen_context_outcome(StatusCode::OK, answer, Some(context))
        }
        Err(detail) => {
            audit_ai_action(
                "summarize-screen",
                Some("screenshot"),
                AiActionOutcome::Blocked,
            );
            screen_context_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Goblins AI needs GPT-OSS, Codex sign-in, or a BYO OpenAI key before it can summarize the screen: {detail}."),
                Some(context),
            )
        }
    }
}

pub(crate) fn audit_ai_action(action_id: &str, entrypoint: Option<&str>, outcome: AiActionOutcome) {
    let _ = append_ai_action_history(action_id, entrypoint, outcome);
}

pub(crate) fn build_ai_action_catalog() -> AiActionCatalog {
    let engine_ready = resident_engine_ready();
    let engine = active_engine_label();
    AiActionCatalog {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        registry_version: REGISTRY_VERSION,
        engine: AiEngineStatus {
            selected: engine,
            ready: engine_ready,
            detail: engine_detail(engine, engine_ready),
        },
        permission_model:
            "Every Goblins AI action declares a policy control, context boundary, and confirmation requirement before execution.",
        actions: action_registry()
            .iter()
            .map(|action| action_status(action, engine_ready))
            .collect(),
    }
}

fn action_status(action: &'static AiAction, engine_ready: bool) -> AiActionStatus {
    let permission_control = action.permission.control_id();
    let policy = policy_state_for_control(permission_control);
    let state = readiness_for_action(action, engine_ready, policy);
    AiActionStatus {
        id: action.id,
        title: action.title,
        detail: action.detail,
        kind: action.kind,
        contexts: action.contexts,
        permission: action.permission,
        permission_control,
        confirmation: action.confirmation,
        entrypoints: action.entrypoints,
        route_hint: action.route_hint,
        state,
        enabled: state == AiActionReadiness::Ready
            || state == AiActionReadiness::ConfirmationRequired,
        reason: readiness_reason(action, state, policy),
    }
}

fn readiness_for_action(
    action: &AiAction,
    engine_ready: bool,
    policy: PolicyControlState,
) -> AiActionReadiness {
    if !engine_ready && !action.enabled_without_engine {
        return AiActionReadiness::WaitingForEngine;
    }

    match policy {
        PolicyControlState::Denied => AiActionReadiness::Denied,
        PolicyControlState::PermissionGated => AiActionReadiness::PermissionGated,
        PolicyControlState::Allowed => {
            if action.confirmation == AiConfirmation::None {
                AiActionReadiness::Ready
            } else {
                AiActionReadiness::ConfirmationRequired
            }
        }
    }
}

fn readiness_reason(
    action: &AiAction,
    state: AiActionReadiness,
    policy: PolicyControlState,
) -> String {
    match state {
        AiActionReadiness::Ready => "Ready through the OS-owned resident path.".to_string(),
        AiActionReadiness::ConfirmationRequired => {
            "Ready after the user reviews and confirms the exact action.".to_string()
        }
        AiActionReadiness::WaitingForEngine => {
            "Disabled until GPT-OSS, Codex, or a BYO OpenAI relay is configured.".to_string()
        }
        AiActionReadiness::PermissionGated => format!(
            "Requires an explicit Goblins OS permission grant for {}.",
            action.permission.control_id()
        ),
        AiActionReadiness::Denied => {
            format!("Denied by the active Goblins OS policy profile ({policy:?}).")
        }
    }
}

fn engine_detail(engine: &str, ready: bool) -> String {
    if ready {
        return format!("Goblins AI is using {engine} through an OS-owned relay.");
    }
    "Goblins AI is waiting for GPT-OSS, Codex sign-in, or a BYO OpenAI key in OS-owned storage."
        .to_string()
}

fn file_context_outcome(
    status: StatusCode,
    text: String,
    context: Option<FileContextSummary>,
) -> (StatusCode, Json<FileContextResponse>) {
    let ok = status.is_success();
    (status, Json(FileContextResponse { ok, text, context }))
}

fn settings_context_outcome(
    status: StatusCode,
    text: String,
    context: Option<SettingsContextSummary>,
) -> (StatusCode, Json<SettingsContextResponse>) {
    let ok = status.is_success();
    (status, Json(SettingsContextResponse { ok, text, context }))
}

fn open_settings_panel_outcome(
    status: StatusCode,
    text: String,
    panel: Option<OpenSettingsPanelSummary>,
) -> (StatusCode, Json<OpenSettingsPanelResponse>) {
    (
        status,
        Json(OpenSettingsPanelResponse {
            ok: status.is_success(),
            text,
            panel,
        }),
    )
}

fn system_status_context_outcome(
    status: StatusCode,
    text: String,
    context: Option<SystemStatusContextSummary>,
) -> (StatusCode, Json<SystemStatusContextResponse>) {
    (
        status,
        Json(SystemStatusContextResponse {
            ok: status.is_success(),
            text,
            context,
        }),
    )
}

fn safe_setting_change_outcome(
    status: StatusCode,
    applied: bool,
    text: String,
    change: Option<SafeSettingChangeSummary>,
) -> (StatusCode, Json<SafeSettingChangeResponse>) {
    (
        status,
        Json(SafeSettingChangeResponse {
            ok: status.is_success(),
            applied,
            text,
            change,
        }),
    )
}

fn selected_text_context_outcome(
    status: StatusCode,
    text: String,
    context: Option<SelectedTextContextSummary>,
) -> (StatusCode, Json<SelectedTextContextResponse>) {
    let ok = status.is_success();
    (
        status,
        Json(SelectedTextContextResponse { ok, text, context }),
    )
}

fn screen_context_outcome(
    status: StatusCode,
    text: String,
    context: Option<ScreenContextSummary>,
) -> (StatusCode, Json<ScreenContextResponse>) {
    let ok = status.is_success();
    (status, Json(ScreenContextResponse { ok, text, context }))
}

fn notification_context_outcome(
    status: StatusCode,
    text: String,
    context: Option<NotificationContextSummary>,
) -> (StatusCode, Json<NotificationContextResponse>) {
    let ok = status.is_success();
    (
        status,
        Json(NotificationContextResponse { ok, text, context }),
    )
}

fn notification_context_policy() -> Result<(), (StatusCode, String, AiActionOutcome)> {
    match policy_state_for_control("notification-context") {
        PolicyControlState::Allowed => Ok(()),
        PolicyControlState::Denied => Err((
            StatusCode::FORBIDDEN,
            "Notification context is blocked by the active Goblins OS policy profile.".to_string(),
            AiActionOutcome::Denied,
        )),
        PolicyControlState::PermissionGated => Err((
            StatusCode::FORBIDDEN,
            "Allow notification context in Privacy & Permissions before Goblins AI can inspect a selected notification.".to_string(),
            AiActionOutcome::PermissionGated,
        )),
    }
}

fn settings_control_policy() -> Result<(), (StatusCode, String, AiActionOutcome)> {
    match policy_state_for_control("settings-control") {
        PolicyControlState::Allowed => Ok(()),
        PolicyControlState::Denied => Err((
            StatusCode::FORBIDDEN,
            "AI setting changes are blocked by the active Goblins OS policy profile.".to_string(),
            AiActionOutcome::Denied,
        )),
        PolicyControlState::PermissionGated => Err((
            StatusCode::FORBIDDEN,
            "Allow confirmed Settings changes in Privacy & Permissions before Goblins AI can change a setting.".to_string(),
            AiActionOutcome::PermissionGated,
        )),
    }
}

fn system_troubleshooting_policy() -> Result<(), (StatusCode, String, AiActionOutcome)> {
    match policy_state_for_control("system-troubleshooting") {
        PolicyControlState::Allowed => Ok(()),
        PolicyControlState::Denied => Err((
            StatusCode::FORBIDDEN,
            "System status help is blocked by the active Goblins OS policy profile.".to_string(),
            AiActionOutcome::Denied,
        )),
        PolicyControlState::PermissionGated => Err((
            StatusCode::FORBIDDEN,
            "Allow system troubleshooting in Privacy & Permissions before Goblins AI can summarize system status.".to_string(),
            AiActionOutcome::PermissionGated,
        )),
    }
}

fn screen_context_policy() -> Result<(), (StatusCode, String, AiActionOutcome)> {
    match policy_state_for_control("screen-context") {
        PolicyControlState::Allowed => Ok(()),
        PolicyControlState::Denied => Err((
            StatusCode::FORBIDDEN,
            "Screen and selected-text context is blocked by the active Goblins OS policy profile."
                .to_string(),
            AiActionOutcome::Denied,
        )),
        PolicyControlState::PermissionGated => Err((
            StatusCode::FORBIDDEN,
            "Allow screen and selected-text context in Privacy & Permissions before Goblins AI can inspect visible content.".to_string(),
            AiActionOutcome::PermissionGated,
        )),
    }
}

fn summarize_selected_path(path: &Path) -> FileContextSummary {
    let metadata = fs::metadata(path).ok();
    let kind = match metadata.as_ref() {
        Some(metadata) if metadata.is_dir() => "folder",
        Some(metadata) if metadata.is_file() => "file",
        Some(_) => "item",
        None => "item",
    };
    FileContextSummary {
        name: display_path_name(path),
        kind,
        extension: path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase()),
        size_bytes: metadata
            .as_ref()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len()),
    }
}

fn summarize_settings_context(request: &SettingsContextRequest) -> SettingsContextSummary {
    let panel = sanitized_context_value(&request.panel, 96);
    let topic = request
        .topic
        .as_deref()
        .map(|value| sanitized_context_value(value, 120))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Current Settings panel".to_string());
    SettingsContextSummary {
        route_hint: settings_route_hint(&panel),
        panel,
        topic,
    }
}

fn safe_setting_change_summary(
    setting_id: &str,
    value: &Value,
) -> Result<SafeSettingChangeSummary, String> {
    match setting_id.trim() {
        "appearance.color-scheme" => {
            let requested_value = json_string_value(value)
                .ok_or_else(|| "Appearance color scheme expects Light, Dark, or Auto.".to_string())?;
            let normalized = match requested_value.as_str() {
                "light" | "prefer-light" => "prefer-light",
                "dark" | "prefer-dark" => "prefer-dark",
                "auto" | "default" => "default",
                _ => {
                    return Err(
                        "Appearance color scheme expects Light, Dark, or Auto.".to_string()
                    );
                }
            };
            Ok(SafeSettingChangeSummary {
                setting_id: "appearance.color-scheme",
                title: "Appearance color scheme",
                requested_value: normalized.to_string(),
                effect: "Changes the desktop appearance preference through the standard GNOME color-scheme setting.",
            })
        }
        "accessibility.reduce-motion" => {
            let requested_value = json_bool_value(value).ok_or_else(|| {
                "Reduce motion expects a true or false value.".to_string()
            })?;
            Ok(SafeSettingChangeSummary {
                setting_id: "accessibility.reduce-motion",
                title: "Reduce motion",
                requested_value: requested_value.to_string(),
                effect: "Changes the desktop animation preference through the standard interface accessibility setting.",
            })
        }
        "notifications.show-banners" => {
            let requested_value = json_bool_value(value).ok_or_else(|| {
                "Notification banners expects a true or false value.".to_string()
            })?;
            Ok(SafeSettingChangeSummary {
                setting_id: "notifications.show-banners",
                title: "Notification banners",
                requested_value: requested_value.to_string(),
                effect: "Changes whether notification banners can appear using the standard desktop notification preference.",
            })
        }
        _ => Err(
            "Goblins AI can only change allowlisted settings: appearance.color-scheme, accessibility.reduce-motion, or notifications.show-banners.".to_string(),
        ),
    }
}

fn apply_safe_setting_change(change: &SafeSettingChangeSummary) -> (StatusCode, String) {
    match change.setting_id {
        "appearance.color-scheme" => {
            crate::appearance::apply_ai_color_scheme(&change.requested_value)
        }
        "accessibility.reduce-motion" => {
            let value = change.requested_value == "true";
            crate::accessibility::apply_ai_reduce_motion(value)
        }
        "notifications.show-banners" => {
            let value = change.requested_value == "true";
            crate::notifications::apply_ai_notification_banners(value)
        }
        _ => (
            StatusCode::BAD_REQUEST,
            "That setting is not in the Goblins AI allowlist.".to_string(),
        ),
    }
}

fn safe_setting_change_audit_outcome(status: StatusCode) -> AiActionOutcome {
    if status.is_success() {
        AiActionOutcome::Succeeded
    } else if status == StatusCode::SERVICE_UNAVAILABLE {
        AiActionOutcome::Blocked
    } else {
        AiActionOutcome::Failed
    }
}

#[derive(Clone, Copy)]
struct SettingsPanelCandidate {
    id: &'static str,
    title: &'static str,
    terms: &'static [&'static str],
}

const SETTINGS_PANEL_CANDIDATES: &[SettingsPanelCandidate] = &[
    SettingsPanelCandidate {
        id: "overview",
        title: "Overview",
        terms: &[
            "overview", "home", "status", "services", "hardware", "resident",
        ],
    },
    SettingsPanelCandidate {
        id: "appearance",
        title: "Appearance",
        terms: &[
            "appearance",
            "light",
            "dark",
            "theme",
            "color scheme",
            "font",
            "inter",
        ],
    },
    SettingsPanelCandidate {
        id: "applications",
        title: "Applications",
        terms: &[
            "applications",
            "apps",
            "default apps",
            "file handlers",
            "app permissions",
            "sandbox",
        ],
    },
    SettingsPanelCandidate {
        id: "desktop-dock",
        title: "Desktop & Dock",
        terms: &[
            "desktop",
            "dock",
            "launcher",
            "window controls",
            "desktop surfaces",
        ],
    },
    SettingsPanelCandidate {
        id: "menu-bar-control-center",
        title: "Menu Bar & Control Center",
        terms: &[
            "menu bar",
            "control center",
            "top bar",
            "quick settings",
            "status menu",
            "wifi menu",
        ],
    },
    SettingsPanelCandidate {
        id: "network",
        title: "Network",
        terms: &[
            "network",
            "wifi",
            "wi-fi",
            "wireless",
            "ssid",
            "ip address",
            "proxy",
            "internet",
        ],
    },
    SettingsPanelCandidate {
        id: "network-services",
        title: "Wired & VPN",
        terms: &[
            "ethernet",
            "wired",
            "vpn",
            "tunnel",
            "lan",
            "advanced network",
        ],
    },
    SettingsPanelCandidate {
        id: "bluetooth",
        title: "Bluetooth",
        terms: &["bluetooth", "adapter", "radio", "pair", "paired device"],
    },
    SettingsPanelCandidate {
        id: "mobile-broadband",
        title: "Mobile Broadband",
        terms: &["mobile broadband", "wwan", "cellular", "modem", "sim"],
    },
    SettingsPanelCandidate {
        id: "sharing",
        title: "Sharing",
        terms: &["sharing", "remote desktop", "file sharing", "hostname"],
    },
    SettingsPanelCandidate {
        id: "displays",
        title: "Displays",
        terms: &[
            "display",
            "displays",
            "screen",
            "monitor",
            "night light",
            "brightness",
            "resolution",
            "scaling",
        ],
    },
    SettingsPanelCandidate {
        id: "color-management",
        title: "Color",
        terms: &["color", "icc", "calibration", "profile", "display color"],
    },
    SettingsPanelCandidate {
        id: "sound",
        title: "Sound",
        terms: &[
            "sound",
            "audio",
            "speaker",
            "volume",
            "mute",
            "microphone",
            "voice",
        ],
    },
    SettingsPanelCandidate {
        id: "keyboard",
        title: "Keyboard",
        terms: &[
            "keyboard",
            "typing",
            "repeat",
            "shortcut",
            "hotkey",
            "input source",
            "num lock",
        ],
    },
    SettingsPanelCandidate {
        id: "mouse-trackpad",
        title: "Mouse & Trackpad",
        terms: &[
            "mouse",
            "trackpad",
            "touchpad",
            "pointer",
            "scroll",
            "tap to click",
            "gesture",
        ],
    },
    SettingsPanelCandidate {
        id: "drawing-tablet",
        title: "Drawing Tablet",
        terms: &[
            "drawing tablet",
            "wacom",
            "tablet",
            "stylus",
            "pen",
            "calibration",
        ],
    },
    SettingsPanelCandidate {
        id: "accessibility",
        title: "Accessibility",
        terms: &[
            "accessibility",
            "a11y",
            "screen reader",
            "large text",
            "text size",
            "magnifier",
            "zoom",
            "reduce motion",
            "on-screen keyboard",
        ],
    },
    SettingsPanelCandidate {
        id: "desktop-wallpaper",
        title: "Desktop & Wallpaper",
        terms: &[
            "wallpaper",
            "background",
            "desktop picture",
            "placement",
            "fit",
            "fill",
            "span",
            "center",
            "tile",
            "stretch",
        ],
    },
    SettingsPanelCandidate {
        id: "notifications",
        title: "Notifications",
        terms: &[
            "notifications",
            "alerts",
            "banners",
            "do not disturb",
            "notification preview",
            "notification sound",
        ],
    },
    SettingsPanelCandidate {
        id: "lock-screen",
        title: "Lock Screen",
        terms: &[
            "lock screen",
            "screen lock",
            "blank screen",
            "login screen",
            "notification privacy",
        ],
    },
    SettingsPanelCandidate {
        id: "search",
        title: "Search",
        terms: &["search", "indexing", "file search", "results"],
    },
    SettingsPanelCandidate {
        id: "multitasking",
        title: "Multitasking",
        terms: &[
            "multitasking",
            "workspaces",
            "hot corner",
            "window switching",
            "snap",
            "mission control",
        ],
    },
    SettingsPanelCandidate {
        id: "power-battery",
        title: "Power & Battery",
        terms: &[
            "power",
            "battery",
            "energy",
            "sleep",
            "suspend",
            "lid",
            "performance",
        ],
    },
    SettingsPanelCandidate {
        id: "games",
        title: "Games",
        terms: &[
            "games",
            "gaming",
            "vulkan",
            "gamemode",
            "gamescope",
            "mangohud",
            "controller",
            "heroic",
            "lutris",
            "bottles",
            "umu",
            "proton",
        ],
    },
    SettingsPanelCandidate {
        id: "printers-scanners",
        title: "Printers & Scanners",
        terms: &[
            "printer", "printers", "scanner", "scanners", "cups", "print", "scan",
        ],
    },
    SettingsPanelCandidate {
        id: "date-time",
        title: "Date & Time",
        terms: &[
            "date",
            "time",
            "clock",
            "timezone",
            "time zone",
            "ntp",
            "calendar",
        ],
    },
    SettingsPanelCandidate {
        id: "language-region",
        title: "Language & Region",
        terms: &[
            "language",
            "region",
            "locale",
            "formats",
            "display language",
            "input language",
        ],
    },
    SettingsPanelCandidate {
        id: "users-accounts",
        title: "Users & Accounts",
        terms: &[
            "users",
            "accounts",
            "account",
            "identity",
            "local user",
            "administrator",
            "computer name",
        ],
    },
    SettingsPanelCandidate {
        id: "online-accounts",
        title: "Online Accounts",
        terms: &[
            "online accounts",
            "internet accounts",
            "cloud accounts",
            "mail accounts",
            "calendar accounts",
        ],
    },
    SettingsPanelCandidate {
        id: "privacy-permissions",
        title: "Privacy & Permissions",
        terms: &[
            "privacy",
            "permissions",
            "recent files",
            "app usage",
            "camera",
            "microphone",
            "usb",
            "thunderbolt",
        ],
    },
    SettingsPanelCandidate {
        id: "security",
        title: "Security",
        terms: &[
            "security",
            "password",
            "firewall",
            "boot integrity",
            "secure storage",
            "secrets",
            "keyring",
        ],
    },
    SettingsPanelCandidate {
        id: "wellbeing",
        title: "Wellbeing",
        terms: &[
            "wellbeing",
            "screen time",
            "break reminders",
            "digital wellbeing",
            "attention",
        ],
    },
    SettingsPanelCandidate {
        id: "models",
        title: "AI & Models",
        terms: &[
            "ai",
            "assistant",
            "goblins ai",
            "openai",
            "codex",
            "gpt-oss",
            "model",
            "models",
            "api key",
            "selected text",
            "screenshot",
            "action history",
        ],
    },
    SettingsPanelCandidate {
        id: "policy",
        title: "Policy",
        terms: &[
            "policy",
            "data boundary",
            "consumer",
            "business",
            "enterprise",
            "permission grant",
        ],
    },
    SettingsPanelCandidate {
        id: "storage",
        title: "Storage",
        terms: &[
            "storage",
            "disk",
            "drives",
            "capacity",
            "free space",
            "cleanup",
            "cache",
            "disk usage",
            "mount",
            "filesystem",
        ],
    },
    SettingsPanelCandidate {
        id: "updates-about",
        title: "Updates & About",
        terms: &[
            "updates",
            "about",
            "bootc",
            "version",
            "image",
            "upgrade",
            "software update",
        ],
    },
    SettingsPanelCandidate {
        id: "recovery",
        title: "Recovery",
        terms: &[
            "recovery", "health", "repair", "rollback", "restore", "reset", "services",
        ],
    },
    SettingsPanelCandidate {
        id: "developer",
        title: "Developer",
        terms: &[
            "developer",
            "diagnostics",
            "logs",
            "journal",
            "core",
            "system monitor",
            "processes",
        ],
    },
];

fn resolve_open_settings_panel(
    query: &str,
    source_panel: Option<&str>,
) -> OpenSettingsPanelSummary {
    let normalized_query = normalize_settings_query(query);
    let source = source_panel
        .map(|value| sanitized_context_value(value, 80))
        .filter(|value| !value.is_empty());
    let mut best = SETTINGS_PANEL_CANDIDATES[0];
    let mut best_score = 0usize;
    let mut best_reason = "No exact match; start from Overview.".to_string();

    for candidate in SETTINGS_PANEL_CANDIDATES {
        let (score, reason) = settings_panel_score(candidate, &normalized_query);
        if score > best_score {
            best = *candidate;
            best_score = score;
            best_reason = reason;
        }
    }

    let confidence = if best_score >= 1000 {
        "exact"
    } else if best_score >= 20 {
        "high"
    } else if best_score > 0 {
        "medium"
    } else {
        "fallback"
    };
    let reason = match source {
        Some(source) => format!("{best_reason} Source panel: {source}."),
        None => best_reason,
    };

    OpenSettingsPanelSummary {
        panel_id: best.id,
        title: best.title,
        route_hint: format!("settings.open-panel.{}", best.id),
        launch_argument: format!("--panel={}", best.id),
        confidence,
        reason,
    }
}

fn settings_panel_score(
    candidate: &SettingsPanelCandidate,
    normalized_query: &str,
) -> (usize, String) {
    if normalized_query == candidate.id
        || normalized_query == normalize_settings_query(candidate.title)
    {
        return (
            1000 + candidate.id.len(),
            format!(
                "Matched the {} Settings panel name exactly.",
                candidate.title
            ),
        );
    }

    let mut best = 0usize;
    let mut matched = None;
    for term in candidate.terms {
        let normalized_term = normalize_settings_query(term);
        if normalized_query == normalized_term {
            return (
                1000 + normalized_term.len(),
                format!("Matched the Settings term \"{term}\" exactly."),
            );
        }
        if normalized_query.contains(&normalized_term) {
            let score = 20 + normalized_term.len();
            if score > best {
                best = score;
                matched = Some(*term);
            }
        }
    }

    if let Some(term) = matched {
        (
            best,
            format!("Matched the Settings term \"{term}\" in the request."),
        )
    } else {
        (0, String::new())
    }
}

fn normalize_settings_query(value: &str) -> String {
    sanitized_context_value(value, 240)
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
}

fn bounded_system_status_snapshot() -> String {
    let snapshot = serde_json::json!({
        "readiness": crate::readiness::build_readiness(),
        "services": crate::system::build_system_services_status(),
        "hardware": crate::hardware::build_hardware_status(),
        "local_models": crate::model_manager::build_local_model_catalog(),
        "policy": crate::policy::build_policy_status(),
    });
    let serialized = serde_json::to_string(&snapshot)
        .unwrap_or_else(|_| "{\"error\":\"status snapshot serialization failed\"}".to_string());
    sanitized_context_value(&serialized, 6000)
}

fn json_string_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(|value| sanitized_context_value(value, 80).to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn json_bool_value(value: &Value) -> Option<bool> {
    value.as_bool()
}

fn display_path_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "Selected item".to_string())
}

fn file_context_prompt(context: &FileContextSummary) -> String {
    let extension = context.extension.as_deref().unwrap_or("none");
    let size = context
        .size_bytes
        .map(|bytes| bytes.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "You are Goblins AI, the system assistant in Goblins OS. The user invoked a file-manager action on one local {kind}. Use only this metadata; do not claim to have read file contents. Name: {name}. Extension: {extension}. Size bytes: {size}. Answer with useful next steps, likely ways to open or work with it, and whether building a focused app for it would help.",
        kind = context.kind,
        name = context.name,
    )
}

fn settings_context_prompt(
    context: &SettingsContextSummary,
    question: Option<&str>,
    status_summary: Option<&str>,
) -> String {
    let question = question
        .map(|value| sanitized_context_value(value, 480))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "Explain this Settings panel and suggest the next safe step.".to_string()
        });
    let status = status_summary
        .map(|value| sanitized_context_value(value, 900))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "No detailed status summary was provided by Settings.".to_string());
    format!(
        "You are Goblins AI, the system assistant in Goblins OS. The user invoked Settings help from the {panel} panel. Use only this Settings metadata and OS-owned status summary; do not claim to inspect hidden controls, files, windows, screenshots, or secrets. Topic: {topic}. Route hint: {route_hint}. Status summary: {status}. User question: {question}. Answer with a concise explanation, the likely Settings panel or built-in Goblins OS control to open, and safe next steps. If a setting change is needed, require explicit user confirmation before any change.",
        panel = context.panel,
        topic = context.topic,
        route_hint = context.route_hint,
    )
}

fn system_status_prompt(
    context: &SystemStatusContextSummary,
    snapshot: &str,
    question: Option<&str>,
    caller_summary: Option<&str>,
) -> String {
    let question = question
        .map(|value| sanitized_context_value(value, 480))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "Summarize the current system state and suggest the safest next step.".to_string()
        });
    let caller_summary = caller_summary
        .map(|value| sanitized_context_value(value, 1000))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "No caller-provided status summary.".to_string());
    format!(
        "You are Goblins AI, the system assistant in Goblins OS. The user explicitly invoked system status help from {route_hint}. Use only this OS-owned status snapshot and caller summary; do not claim to inspect hidden windows, live pixels, files, notification history, secrets, credentials, prompts, replies, or background app data. Focus: {focus}. Caller summary: {caller_summary}. Snapshot JSON: {snapshot}. User question: {question}. Answer concisely with current readiness, likely issue area, and safe next steps. If a setting or privileged action is needed, require explicit user confirmation before any change.",
        route_hint = context.route_hint,
        focus = context.focus,
    )
}

fn selected_text_context_prompt(
    selected_text: &str,
    context: &SelectedTextContextSummary,
    question: Option<&str>,
) -> String {
    let question = question
        .map(|value| sanitized_context_value(value, 480))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "Explain the selected text and suggest the next useful action.".to_string()
        });
    let app = context.app.as_deref().unwrap_or("unknown app");
    let window = context.window_title.as_deref().unwrap_or("unknown window");
    format!(
        "You are Goblins AI, the system assistant in Goblins OS. The user explicitly invoked selected-text context. Use only the selected text and app/window metadata below; do not claim to inspect the rest of the screen, clipboard history, files, notifications, secrets, or hidden windows. App: {app}. Window: {window}. Selected text: {selected_text}. User question: {question}. Answer concisely and suggest a safe next action. If an OS action is needed, require explicit user confirmation before any change."
    )
}

fn writing_tools_prompt(
    selected_text: &str,
    context: &SelectedTextContextSummary,
    question: Option<&str>,
) -> String {
    let question = question
        .map(|value| sanitized_context_value(value, 480))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "Rewrite, proofread, summarize, or adjust this text. Preserve meaning unless the user clearly asks for a change.".to_string()
        });
    let app = context.app.as_deref().unwrap_or("unknown app");
    let window = context.window_title.as_deref().unwrap_or("unknown window");
    format!(
        "You are Goblins AI, the system writing assistant in Goblins OS. The user explicitly invoked writing assistance. Use only the selected text and app/window metadata below; do not claim to inspect the rest of the screen, clipboard history, files, notifications, secrets, or hidden windows. App: {app}. Window: {window}. Selected text: {selected_text}. User writing request: {question}. Return ready-to-use text first. Preserve meaning unless the user asks for a change; if intent is ambiguous, ask one concise clarification. Do not take OS actions from this writing flow."
    )
}

fn notification_context_prompt(
    context: &NotificationContextSummary,
    title: &str,
    body: &str,
    question: Option<&str>,
) -> String {
    let question = question
        .map(|value| sanitized_context_value(value, 480))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            "Explain this notification and suggest the safest next step.".to_string()
        });
    let app = context.app.as_deref().unwrap_or("unknown app");
    let action = context
        .action_label
        .as_deref()
        .unwrap_or("no invoked action label");
    format!(
        "You are Goblins AI, the notification assistant in Goblins OS. The user explicitly invoked notification context. Use only this invoked notification summary; do not claim to inspect notification history, other notifications, files, screenshots, secrets, hidden windows, or background app data. App: {app}. Notification title: {title}. Notification body: {body}. Invoked action label: {action}. User question: {question}. Answer concisely. If replying, dismissing, opening an app, changing settings, or taking any other action is needed, require explicit user confirmation before the action."
    )
}

fn screen_context_prompt(
    context: &ScreenContextSummary,
    visible_text: &str,
    visual_summary: &str,
    question: Option<&str>,
) -> String {
    let question = question
        .map(|value| sanitized_context_value(value, 480))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Summarize what is visible and suggest a safe next step.".to_string());
    let app = context.app.as_deref().unwrap_or("unknown app");
    let window = context.window_title.as_deref().unwrap_or("unknown window");
    format!(
        "You are Goblins AI, the system assistant in Goblins OS. The user explicitly invoked screen context from {source}. Use only the provided visible text, visual summary, and app/window metadata; do not claim to inspect live pixels, files, notifications, secrets, hidden windows, or content not included here. App: {app}. Window: {window}. Visible text: {visible_text}. Visual summary: {visual_summary}. User question: {question}. Answer concisely. If a system change or app action is needed, require explicit user confirmation before any action.",
        source = context.source
    )
}

fn settings_context_action_id(
    context: &SettingsContextSummary,
    question: Option<&str>,
) -> &'static str {
    let haystack = format!(
        "{} {} {}",
        context.panel,
        context.topic,
        question.unwrap_or_default()
    )
    .to_ascii_lowercase();
    if [
        "fix",
        "troubleshoot",
        "network",
        "audio",
        "sound",
        "display",
        "screen",
        "storage",
        "disk",
    ]
    .iter()
    .any(|term| haystack.contains(term))
    {
        "troubleshoot-network-audio-display-storage"
    } else {
        "explain-system-status"
    }
}

fn system_status_action_id(focus: &str, question: Option<&str>) -> &'static str {
    let haystack = format!("{} {}", focus, question.unwrap_or_default()).to_ascii_lowercase();
    if [
        "fix",
        "troubleshoot",
        "network",
        "wifi",
        "wi-fi",
        "audio",
        "sound",
        "display",
        "screen",
        "storage",
        "disk",
        "full",
    ]
    .iter()
    .any(|term| haystack.contains(term))
    {
        "troubleshoot-network-audio-display-storage"
    } else {
        "explain-system-status"
    }
}

fn settings_route_hint(panel: &str) -> String {
    match panel {
        "network" | "network-services" => "settings.open-panel.network",
        "sound" => "settings.open-panel.sound",
        "displays" => "settings.open-panel.displays",
        "storage" => "settings.open-panel.storage",
        "privacy-permissions" => "settings.open-panel.privacy",
        "models" => "settings.open-panel.models",
        "accessibility" => "settings.open-panel.accessibility",
        "bluetooth" => "settings.open-panel.bluetooth",
        _ => "settings.open-panel.current",
    }
    .to_string()
}

fn sanitized_context_value(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_ai_action_history() -> AiActionHistory {
    let path = ai_history_path();
    let stored = read_ai_action_history(&path);
    AiActionHistory {
        generated_at: unix_now(),
        source: "goblins-os-core",
        state_path: path.display().to_string(),
        retention: format!(
            "Keeps the latest {MAX_HISTORY_EVENTS} action records. Prompts, replies, screen content, file content, notification text, and secrets are not stored."
        ),
        events: stored.events,
    }
}

fn append_ai_action_history(
    action_id: &str,
    entrypoint: Option<&str>,
    outcome: AiActionOutcome,
) -> Result<AiActionHistoryEvent, String> {
    let action = action_registry()
        .iter()
        .find(|action| action.id == action_id.trim())
        .ok_or_else(|| {
            "Unknown Goblins AI action id; history only records registered OS actions.".to_string()
        })?;
    let path = ai_history_path();
    let mut stored = read_ai_action_history(&path);
    let event = action_history_event(action, entrypoint, outcome);
    stored.events.insert(0, event.clone());
    stored.events.truncate(MAX_HISTORY_EVENTS);
    persist_ai_action_history(&path, &stored)
        .map_err(|_| "Goblins OS could not persist the AI action history.".to_string())?;
    Ok(event)
}

fn action_history_event(
    action: &'static AiAction,
    entrypoint: Option<&str>,
    outcome: AiActionOutcome,
) -> AiActionHistoryEvent {
    AiActionHistoryEvent {
        action_id: action.id.to_string(),
        title: action.title.to_string(),
        outcome,
        entrypoint: normalized_entrypoint(action, entrypoint).to_string(),
        permission_control: action.permission.control_id().to_string(),
        confirmation: action.confirmation,
        occurred_at: unix_now(),
        detail: audit_detail(outcome).to_string(),
    }
}

fn normalized_entrypoint(action: &'static AiAction, entrypoint: Option<&str>) -> &'static str {
    let requested = entrypoint.unwrap_or_default().trim();
    action
        .entrypoints
        .iter()
        .copied()
        .find(|candidate| entrypoint_id(*candidate) == requested)
        .map(entrypoint_id)
        .unwrap_or_else(|| {
            action
                .entrypoints
                .first()
                .copied()
                .map(entrypoint_id)
                .unwrap_or("unknown")
        })
}

fn entrypoint_id(entrypoint: AiEntrypoint) -> &'static str {
    match entrypoint {
        AiEntrypoint::KeyboardShortcut => "keyboard-shortcut",
        AiEntrypoint::Launcher => "launcher",
        AiEntrypoint::ControlCenter => "control-center",
        AiEntrypoint::Settings => "settings",
        AiEntrypoint::SelectedText => "selected-text",
        AiEntrypoint::Screenshot => "screenshot",
        AiEntrypoint::Files => "files",
        AiEntrypoint::Notifications => "notifications",
        AiEntrypoint::Troubleshooting => "troubleshooting",
        AiEntrypoint::AppBuilder => "app-builder",
    }
}

fn audit_detail(outcome: AiActionOutcome) -> &'static str {
    match outcome {
        AiActionOutcome::Started => {
            "Started through a registered Goblins AI entry point. User content is not stored in history."
        }
        AiActionOutcome::Succeeded => {
            "Completed through an OS-owned Goblins AI path. Prompt and response content are not stored."
        }
        AiActionOutcome::Failed => {
            "The action failed without writing prompt, response, screen, file, notification, or secret content to history."
        }
        AiActionOutcome::Blocked => {
            "Blocked before model or action execution by setup, engine readiness, or OS state."
        }
        AiActionOutcome::PermissionGated => {
            "Stopped for explicit permission review before protected context was accessed."
        }
        AiActionOutcome::ConfirmationRequired => {
            "Stopped until the user confirms the exact system action."
        }
        AiActionOutcome::Denied => "Denied by the active Goblins OS policy.",
    }
}

fn read_ai_action_history(path: &Path) -> StoredAiActionHistory {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoredAiActionHistory>(&bytes).ok())
        .unwrap_or_else(|| StoredAiActionHistory { events: Vec::new() })
}

fn persist_ai_action_history(path: &Path, stored: &StoredAiActionHistory) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("AI history path has no parent"));
    };
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o750))?;
    }
    fs::write(path, serde_json::to_vec(stored)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o640))?;
    }
    Ok(())
}

fn ai_history_path() -> PathBuf {
    env::var("GOBLINS_OS_AI_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_AI_STATE_DIR).to_path_buf())
        .join("action-history.json")
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        append_ai_action_history, build_ai_action_catalog, build_ai_action_history,
        file_context_prompt, notification_context_prompt, readiness_for_action,
        resolve_open_settings_panel, safe_setting_change_summary, screen_context_prompt,
        selected_text_context_prompt, settings_context_action_id, settings_context_prompt,
        summarize_selected_path, summarize_settings_context, system_status_action_id,
        system_status_prompt, writing_tools_prompt, AiActionOutcome, AiActionReadiness,
        NotificationContextSummary, ScreenContextSummary, SelectedTextContextSummary,
        SettingsContextRequest, SystemStatusContextSummary, MAX_HISTORY_EVENTS,
    };
    use goblins_os_ai::{action_by_id, AiConfirmation};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::policy::PolicyControlState;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn no_engine_disables_contextual_model_actions() {
        let action = action_by_id("ask-goblins").unwrap();
        assert_eq!(
            readiness_for_action(action, false, PolicyControlState::Allowed),
            AiActionReadiness::WaitingForEngine
        );
    }

    #[test]
    fn settings_routing_stays_available_without_engine() {
        let action = action_by_id("open-settings-panel").unwrap();
        assert_eq!(
            readiness_for_action(action, false, PolicyControlState::Allowed),
            AiActionReadiness::Ready
        );
    }

    #[test]
    fn settings_panel_router_maps_exact_and_natural_language_requests() {
        let exact = resolve_open_settings_panel("sound", None);
        assert_eq!(exact.panel_id, "sound");
        assert_eq!(exact.route_hint, "settings.open-panel.sound");
        assert_eq!(exact.launch_argument, "--panel=sound");
        assert_eq!(exact.confidence, "exact");

        let wifi = resolve_open_settings_panel("join a new wi-fi network", Some("overview"));
        assert_eq!(wifi.panel_id, "network");
        assert_eq!(wifi.launch_argument, "--panel=network");
        assert!(wifi.reason.contains("network"));
        assert!(wifi.reason.contains("Source panel: overview"));

        let motion = resolve_open_settings_panel("turn on reduce motion", None);
        assert_eq!(motion.panel_id, "accessibility");

        let api_key = resolve_open_settings_panel("set my OpenAI API key", None);
        assert_eq!(api_key.panel_id, "models");

        let storage = resolve_open_settings_panel("storage is almost full", None);
        assert_eq!(storage.panel_id, "storage");

        let unknown = resolve_open_settings_panel("something vague", None);
        assert_eq!(unknown.panel_id, "overview");
        assert_eq!(unknown.confidence, "fallback");
    }

    #[test]
    fn sensitive_allowed_actions_still_require_confirmation() {
        let action = action_by_id("change-safe-setting").unwrap();
        assert_ne!(action.confirmation, AiConfirmation::None);
        assert_eq!(
            readiness_for_action(action, true, PolicyControlState::Allowed),
            AiActionReadiness::ConfirmationRequired
        );
    }

    #[test]
    fn catalog_exposes_the_full_shared_registry() {
        let catalog = build_ai_action_catalog();
        assert_eq!(catalog.source, "goblins-os-core");
        assert!(catalog
            .actions
            .iter()
            .any(|action| action.id == "ask-goblins"));
        assert!(catalog
            .actions
            .iter()
            .any(|action| action.permission_control == "screen-context"));
        assert!(catalog
            .actions
            .iter()
            .any(|action| action.id == "write-with-goblins"));
    }

    #[test]
    fn action_history_records_registered_actions_without_content() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = unique_state_dir("registered");
        std::env::set_var("GOBLINS_OS_AI_STATE", &dir);

        let event =
            append_ai_action_history("ask-goblins", Some("launcher"), AiActionOutcome::Succeeded)
                .expect("registered action should be recorded");
        assert_eq!(event.action_id, "ask-goblins");
        assert_eq!(event.entrypoint, "launcher");
        assert_eq!(event.outcome, AiActionOutcome::Succeeded);
        assert!(!event.detail.contains("ping"));

        let history = build_ai_action_history();
        assert_eq!(history.events.len(), 1);
        assert_eq!(history.events[0].permission_control, "resident-assistant");

        std::env::remove_var("GOBLINS_OS_AI_STATE");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn action_history_rejects_unknown_action_ids() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = unique_state_dir("unknown");
        std::env::set_var("GOBLINS_OS_AI_STATE", &dir);

        assert!(
            append_ai_action_history("not-a-real-action", None, AiActionOutcome::Failed).is_err()
        );
        assert!(build_ai_action_history().events.is_empty());

        std::env::remove_var("GOBLINS_OS_AI_STATE");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn action_history_is_bounded_to_recent_events() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = unique_state_dir("bounded");
        std::env::set_var("GOBLINS_OS_AI_STATE", &dir);

        for _ in 0..(MAX_HISTORY_EVENTS + 5) {
            append_ai_action_history(
                "open-settings-panel",
                Some("settings"),
                AiActionOutcome::Succeeded,
            )
            .expect("history write should succeed");
        }
        assert_eq!(build_ai_action_history().events.len(), MAX_HISTORY_EVENTS);

        std::env::remove_var("GOBLINS_OS_AI_STATE");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn file_context_prompt_uses_metadata_without_claiming_file_contents() {
        let context =
            summarize_selected_path(std::path::Path::new("/home/goblin/Notes/Budget.csv"));
        assert_eq!(context.name, "Budget.csv");
        assert_eq!(context.extension.as_deref(), Some("csv"));
        let prompt = file_context_prompt(&context);
        assert!(prompt.contains("Budget.csv"));
        assert!(prompt.contains("Use only this metadata"));
        assert!(prompt.contains("do not claim to have read file contents"));
        assert!(!prompt.contains("/home/goblin"));
    }

    #[test]
    fn settings_context_prompt_uses_metadata_and_requires_confirmation() {
        let request = SettingsContextRequest {
            panel: "sound".to_string(),
            topic: Some("Output device".to_string()),
            question: Some("Why is audio quiet?".to_string()),
            status_summary: Some("Sound panel reports output volume at 25 percent.".to_string()),
        };
        let context = summarize_settings_context(&request);
        assert_eq!(
            settings_context_action_id(&context, request.question.as_deref()),
            "troubleshoot-network-audio-display-storage"
        );
        let prompt = settings_context_prompt(
            &context,
            request.question.as_deref(),
            request.status_summary.as_deref(),
        );
        assert!(prompt.contains("Use only this Settings metadata"));
        assert!(prompt.contains("require explicit user confirmation"));
        assert!(prompt.contains("settings.open-panel.sound"));
        assert!(prompt.contains("do not claim to inspect hidden controls"));
    }

    #[test]
    fn system_status_prompt_uses_only_os_owned_snapshot() {
        let context = SystemStatusContextSummary {
            source: "goblins-os-core",
            focus: "storage".to_string(),
            route_hint: "system.troubleshoot",
            action_id: "troubleshoot-network-audio-display-storage",
            snapshot_chars: 42,
            included: &["readiness", "services"],
        };
        assert_eq!(
            system_status_action_id("storage", Some("disk is full")),
            "troubleshoot-network-audio-display-storage"
        );
        assert_eq!(
            system_status_action_id("overall health", Some("what is ready?")),
            "explain-system-status"
        );
        let prompt = system_status_prompt(
            &context,
            "{\"readiness\":[]}",
            Some("What should I fix?"),
            Some("Storage panel reported low free space."),
        );
        assert!(prompt.contains("Use only this OS-owned status snapshot"));
        assert!(prompt.contains("do not claim to inspect hidden windows"));
        assert!(prompt.contains("secrets, credentials"));
        assert!(prompt.contains("require explicit user confirmation"));
        assert!(prompt.contains("Storage panel reported low free space."));
    }

    #[test]
    fn safe_setting_change_allowlist_is_narrow_and_explainable() {
        let color =
            safe_setting_change_summary("appearance.color-scheme", &serde_json::json!("dark"))
                .expect("color scheme should be allowlisted");
        assert_eq!(color.setting_id, "appearance.color-scheme");
        assert_eq!(color.requested_value, "prefer-dark");
        assert!(color.effect.contains("GNOME color-scheme"));

        let motion =
            safe_setting_change_summary("accessibility.reduce-motion", &serde_json::json!(true))
                .expect("reduce motion should be allowlisted");
        assert_eq!(motion.setting_id, "accessibility.reduce-motion");
        assert_eq!(motion.requested_value, "true");
        assert!(motion.effect.contains("interface accessibility"));

        let banners =
            safe_setting_change_summary("notifications.show-banners", &serde_json::json!(false))
                .expect("notification banners should be allowlisted");
        assert_eq!(banners.setting_id, "notifications.show-banners");
        assert_eq!(banners.requested_value, "false");
        assert!(banners.effect.contains("desktop notification preference"));
    }

    #[test]
    fn safe_setting_change_rejects_arbitrary_settings_and_wrong_values() {
        assert!(safe_setting_change_summary(
            "org.gnome.desktop.interface gtk-theme",
            &serde_json::json!("Adwaita")
        )
        .is_err());
        assert!(safe_setting_change_summary(
            "appearance.color-scheme",
            &serde_json::json!("solarized")
        )
        .is_err());
        assert!(safe_setting_change_summary(
            "notifications.show-banners",
            &serde_json::json!("yes")
        )
        .is_err());
    }

    #[test]
    fn selected_text_prompt_is_invoked_and_bounded_to_visible_selection() {
        let context = SelectedTextContextSummary {
            app: Some("Editor".to_string()),
            window_title: Some("Notes".to_string()),
            text_chars: 18,
        };
        let prompt =
            selected_text_context_prompt("selected paragraph", &context, Some("Explain this"));
        assert!(prompt.contains("explicitly invoked selected-text context"));
        assert!(prompt.contains("Use only the selected text"));
        assert!(prompt.contains("do not claim to inspect the rest of the screen"));
        assert!(prompt.contains("require explicit user confirmation"));
    }

    #[test]
    fn writing_tools_prompt_is_invoked_and_bounded_to_visible_selection() {
        let context = SelectedTextContextSummary {
            app: Some("Editor".to_string()),
            window_title: Some("Draft".to_string()),
            text_chars: 18,
        };
        let prompt =
            writing_tools_prompt("selected paragraph", &context, Some("Make this clearer"));
        assert!(prompt.contains("explicitly invoked writing assistance"));
        assert!(prompt.contains("Use only the selected text"));
        assert!(prompt.contains("Return ready-to-use text first"));
        assert!(prompt.contains("Do not take OS actions from this writing flow"));
        assert!(prompt.contains("do not claim to inspect the rest of the screen"));
    }

    #[test]
    fn notification_context_prompt_is_invoked_and_bounded_to_one_notification() {
        let context = NotificationContextSummary {
            source: "user-invoked-notification",
            app: Some("Calendar".to_string()),
            title_chars: 16,
            body_chars: 38,
            action_label: Some("Open".to_string()),
        };
        let prompt = notification_context_prompt(
            &context,
            "Meeting soon",
            "Design review starts in ten minutes",
            Some("What should I do?"),
        );
        assert!(prompt.contains("explicitly invoked notification context"));
        assert!(prompt.contains("Use only this invoked notification summary"));
        assert!(prompt.contains("do not claim to inspect notification history"));
        assert!(prompt.contains("other notifications"));
        assert!(prompt.contains("secrets, hidden windows"));
        assert!(prompt.contains("require explicit user confirmation"));
    }

    #[test]
    fn screen_context_prompt_is_bounded_to_provided_visible_context() {
        let context = ScreenContextSummary {
            source: "screenshot".to_string(),
            app: Some("Settings".to_string()),
            window_title: Some("Network".to_string()),
            visible_text_chars: 24,
            visual_summary_chars: 34,
        };
        let prompt = screen_context_prompt(
            &context,
            "Wi-Fi disconnected",
            "Settings window with Network selected",
            Some("What should I fix?"),
        );
        assert!(prompt.contains("explicitly invoked screen context"));
        assert!(prompt.contains("Use only the provided visible text"));
        assert!(prompt.contains("do not claim to inspect live pixels"));
        assert!(prompt.contains("hidden windows"));
        assert!(prompt.contains("require explicit user confirmation"));
    }

    fn unique_state_dir(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("goblins-os-ai-test-{label}-{stamp}"))
    }
}
