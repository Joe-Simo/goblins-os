//! Desktop notification preferences for Settings.
//!
//! Goblins OS keeps notification preferences behind an allowlisted settings
//! bridge with per-application path validation so the Settings GUI cannot
//! mutate arbitrary schemas or paths.

use std::process::Command;

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

const NOTIFICATIONS_SCHEMA: &str = "org.gnome.desktop.notifications";
const NOTIFICATION_APPLICATION_SCHEMA: &str = "org.gnome.desktop.notifications.application";
const NOTIFICATION_APPLICATION_BASE_PATH: &str = "/org/gnome/desktop/notifications/application/";

#[derive(Serialize)]
pub struct NotificationsStatus {
    source: &'static str,
    gsettings_available: bool,
    schema_available: bool,
    application_schema_available: bool,
    show_banners: Option<bool>,
    show_in_lock_screen: Option<bool>,
    application_children: Vec<String>,
    applications: Vec<NotificationApplicationStatus>,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NotificationApplicationStatus {
    child: String,
    application_id: Option<String>,
    label: String,
    enable: Option<bool>,
    show_banners: Option<bool>,
    enable_sound_alerts: Option<bool>,
    show_in_lock_screen: Option<bool>,
    details_in_lock_screen: Option<bool>,
    force_expanded: Option<bool>,
    detail: String,
}

#[derive(Deserialize)]
pub struct SetNotificationPreferenceRequest {
    target: NotificationPreferenceTarget,
    child: Option<String>,
    value: bool,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum NotificationPreferenceTarget {
    ShowBanners,
    ShowInLockScreen,
    ApplicationEnable,
    ApplicationShowBanners,
    ApplicationSoundAlerts,
    ApplicationShowInLockScreen,
    ApplicationDetailsInLockScreen,
    ApplicationForceExpanded,
}

#[derive(Serialize)]
pub struct NotificationPreferenceOutcome {
    ok: bool,
    target: &'static str,
    text: String,
}

enum GSettingsError {
    Missing,
    Failed(String),
}

struct SchemaSnapshot {
    available: bool,
    keys: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NotificationPreferenceScope {
    Global,
    Application,
}

#[derive(Clone, Copy)]
struct NotificationPreferenceSpec {
    target: &'static str,
    schema: &'static str,
    key: &'static str,
    label: &'static str,
    scope: NotificationPreferenceScope,
}

pub async fn notifications_status() -> Json<NotificationsStatus> {
    Json(build_notifications_status())
}

pub async fn set_notification_preference(
    Json(request): Json<SetNotificationPreferenceRequest>,
) -> (StatusCode, Json<NotificationPreferenceOutcome>) {
    notification_preference_outcome(request)
}

pub(crate) fn apply_ai_notification_banners(value: bool) -> (StatusCode, String) {
    apply_notification_banners(value)
}

pub(crate) fn apply_notification_banners(value: bool) -> (StatusCode, String) {
    let request = SetNotificationPreferenceRequest {
        target: NotificationPreferenceTarget::ShowBanners,
        child: None,
        value,
    };
    let (status, Json(outcome)) = notification_preference_outcome(request);
    (status, outcome.text)
}

pub(crate) fn read_notification_banners() -> Result<bool, String> {
    if gsettings(&["list-schemas"]).is_err() {
        return Err("Desktop preferences are not ready, so Focus cannot read notification banners in this session.".to_string());
    }

    let schema = schema_snapshot(true, NOTIFICATIONS_SCHEMA);
    if !schema.available || !schema.has_key("show-banners") {
        return Err(
            "Show notification banners is not ready because the desktop session does not report that preference."
                .to_string(),
        );
    }

    setting_bool(&schema, NOTIFICATIONS_SCHEMA, "show-banners")
        .ok_or_else(|| "Show notification banners is not reported by this session.".to_string())
}

fn build_notifications_status() -> NotificationsStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available, NOTIFICATIONS_SCHEMA);
    let application_schema = schema_snapshot(gsettings_available, NOTIFICATION_APPLICATION_SCHEMA);
    let application_children = sorted_unique(
        setting_strv(&schema, NOTIFICATIONS_SCHEMA, "application-children").unwrap_or_default(),
    );
    let applications = if application_schema.available {
        application_children
            .iter()
            .map(|child| notification_application_status(&application_schema, child))
            .collect()
    } else {
        Vec::new()
    };

    NotificationsStatus {
        source: "goblins-os-core",
        gsettings_available,
        schema_available: schema.available,
        application_schema_available: application_schema.available,
        show_banners: setting_bool(&schema, NOTIFICATIONS_SCHEMA, "show-banners"),
        show_in_lock_screen: setting_bool(&schema, NOTIFICATIONS_SCHEMA, "show-in-lock-screen"),
        application_children,
        applications,
        detail: notifications_detail(
            gsettings_available,
            schema.available,
            application_schema.available,
        ),
    }
}

fn notification_application_status(
    schema: &SchemaSnapshot,
    child: &str,
) -> NotificationApplicationStatus {
    let Some(path) = notification_application_path(child) else {
        let child = child.trim().to_string();
        return NotificationApplicationStatus {
            label: notification_application_label(&child, None),
            detail: format!("{child} is not a valid notification settings path component."),
            child,
            application_id: None,
            enable: None,
            show_banners: None,
            enable_sound_alerts: None,
            show_in_lock_screen: None,
            details_in_lock_screen: None,
            force_expanded: None,
        };
    };
    let application_id = setting_string_at_path(
        schema,
        NOTIFICATION_APPLICATION_SCHEMA,
        &path,
        "application-id",
    );
    let label = notification_application_label(child, application_id.as_deref());

    NotificationApplicationStatus {
        child: child.to_string(),
        application_id: application_id.clone(),
        label,
        enable: setting_bool_at_path(schema, NOTIFICATION_APPLICATION_SCHEMA, &path, "enable"),
        show_banners: setting_bool_at_path(
            schema,
            NOTIFICATION_APPLICATION_SCHEMA,
            &path,
            "show-banners",
        ),
        enable_sound_alerts: setting_bool_at_path(
            schema,
            NOTIFICATION_APPLICATION_SCHEMA,
            &path,
            "enable-sound-alerts",
        ),
        show_in_lock_screen: setting_bool_at_path(
            schema,
            NOTIFICATION_APPLICATION_SCHEMA,
            &path,
            "show-in-lock-screen",
        ),
        details_in_lock_screen: setting_bool_at_path(
            schema,
            NOTIFICATION_APPLICATION_SCHEMA,
            &path,
            "details-in-lock-screen",
        ),
        force_expanded: setting_bool_at_path(
            schema,
            NOTIFICATION_APPLICATION_SCHEMA,
            &path,
            "force-expanded",
        ),
        detail: notification_application_record_detail(child, application_id.as_deref()),
    }
}

fn notification_preference_outcome(
    request: SetNotificationPreferenceRequest,
) -> (StatusCode, Json<NotificationPreferenceOutcome>) {
    let spec = notification_preference_spec(request.target);
    if gsettings(&["list-schemas"]).is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(NotificationPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so notification preferences cannot be changed in this session.".to_string(),
            }),
        );
    }

    let schema = schema_snapshot(true, spec.schema);
    if !schema.available || !schema.has_key(spec.key) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(NotificationPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: format!(
                    "{} is not ready because the required preference is not reported by this desktop session.",
                    spec.label
                ),
            }),
        );
    }

    let schema_arg = match preference_schema_arg(spec, request.child.as_deref()) {
        Ok(schema_arg) => schema_arg,
        Err((status, text)) => {
            return (
                status,
                Json(NotificationPreferenceOutcome {
                    ok: false,
                    target: spec.target,
                    text,
                }),
            );
        }
    };
    let encoded_value = request.value.to_string();
    match gsettings(&["set", &schema_arg, spec.key, &encoded_value]) {
        Ok(_) => (
            StatusCode::OK,
            Json(NotificationPreferenceOutcome {
                ok: true,
                target: spec.target,
                text: notification_preference_success_detail(spec, request.value),
            }),
        ),
        Err(GSettingsError::Missing) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(NotificationPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: "Desktop preferences are not ready, so notification preferences cannot be changed in this session.".to_string(),
            }),
        ),
        Err(GSettingsError::Failed(detail)) => (
            StatusCode::BAD_GATEWAY,
            Json(NotificationPreferenceOutcome {
                ok: false,
                target: spec.target,
                text: if detail.is_empty() {
                    format!("{} could not be saved by the desktop session.", spec.label)
                } else {
                    format!("{} could not be saved: {detail}", spec.label)
                },
            }),
        ),
    }
}

fn preference_schema_arg(
    spec: NotificationPreferenceSpec,
    child: Option<&str>,
) -> Result<String, (StatusCode, String)> {
    if spec.scope == NotificationPreferenceScope::Global {
        return Ok(spec.schema.to_string());
    }

    let Some(child) = child.map(str::trim).filter(|child| !child.is_empty()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{} requires a registered application entry.", spec.label),
        ));
    };
    let Some(path) = notification_application_path(child) else {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{child} is not a valid notification settings path component."),
        ));
    };

    let global_schema = schema_snapshot(true, NOTIFICATIONS_SCHEMA);
    let registered = setting_strv(&global_schema, NOTIFICATIONS_SCHEMA, "application-children")
        .unwrap_or_default()
        .into_iter()
        .any(|candidate| candidate == child);
    if !registered {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{child} is not a registered notification application entry."),
        ));
    }

    Ok(format!("{}:{path}", spec.schema))
}

impl SchemaSnapshot {
    fn unavailable() -> Self {
        Self {
            available: false,
            keys: Vec::new(),
        }
    }

    fn has_key(&self, key: &str) -> bool {
        self.keys.iter().any(|candidate| candidate == key)
    }
}

fn schema_snapshot(gsettings_available: bool, schema: &str) -> SchemaSnapshot {
    if !gsettings_available {
        return SchemaSnapshot::unavailable();
    }

    match gsettings(&["list-keys", schema]) {
        Ok(stdout) => SchemaSnapshot {
            available: true,
            keys: stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect(),
        },
        Err(_) => SchemaSnapshot::unavailable(),
    }
}

fn setting_bool(schema: &SchemaSnapshot, schema_name: &str, key: &str) -> Option<bool> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", schema_name, key])
        .ok()
        .and_then(|value| parse_gsettings_bool(&value))
}

fn setting_strv(schema: &SchemaSnapshot, schema_name: &str, key: &str) -> Option<Vec<String>> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", schema_name, key])
        .ok()
        .and_then(|value| parse_gsettings_strv(&value))
}

fn setting_bool_at_path(
    schema: &SchemaSnapshot,
    schema_name: &str,
    path: &str,
    key: &str,
) -> Option<bool> {
    if !schema.has_key(key) {
        return None;
    }
    let schema_arg = format!("{schema_name}:{path}");
    gsettings(&["get", &schema_arg, key])
        .ok()
        .and_then(|value| parse_gsettings_bool(&value))
}

fn setting_string_at_path(
    schema: &SchemaSnapshot,
    schema_name: &str,
    path: &str,
    key: &str,
) -> Option<String> {
    if !schema.has_key(key) {
        return None;
    }
    let schema_arg = format!("{schema_name}:{path}");
    gsettings(&["get", &schema_arg, key])
        .ok()
        .and_then(|value| parse_gsettings_string(&value))
}

fn parse_gsettings_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_gsettings_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value = trimmed
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .unwrap_or(trimmed);
    Some(value.to_string())
}

fn parse_gsettings_strv(value: &str) -> Option<Vec<String>> {
    let mut text = value.trim();
    if let Some(rest) = text.strip_prefix("@as ") {
        text = rest.trim();
    }
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    let mut values = Vec::new();
    let mut rest = inner.trim();

    while !rest.is_empty() {
        if rest.starts_with(',') {
            rest = rest[1..].trim_start();
            continue;
        }
        if !rest.starts_with('\'') {
            return None;
        }
        let mut escaped = false;
        let mut value = String::new();
        let mut end_index = None;
        for (index, ch) in rest[1..].char_indices() {
            if escaped {
                value.push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '\'' {
                end_index = Some(index + 2);
                break;
            }
            value.push(ch);
        }
        let end_index = end_index?;
        values.push(value);
        rest = rest[end_index..].trim_start();
        if rest.starts_with(',') {
            rest = rest[1..].trim_start();
        } else if !rest.is_empty() {
            return None;
        }
    }

    Some(values)
}

fn notification_preference_spec(
    target: NotificationPreferenceTarget,
) -> NotificationPreferenceSpec {
    match target {
        NotificationPreferenceTarget::ShowBanners => NotificationPreferenceSpec {
            target: "show-banners",
            schema: NOTIFICATIONS_SCHEMA,
            key: "show-banners",
            label: "Show notification banners",
            scope: NotificationPreferenceScope::Global,
        },
        NotificationPreferenceTarget::ShowInLockScreen => NotificationPreferenceSpec {
            target: "show-in-lock-screen",
            schema: NOTIFICATIONS_SCHEMA,
            key: "show-in-lock-screen",
            label: "Show notifications on lock screen",
            scope: NotificationPreferenceScope::Global,
        },
        NotificationPreferenceTarget::ApplicationEnable => NotificationPreferenceSpec {
            target: "application-enable",
            schema: NOTIFICATION_APPLICATION_SCHEMA,
            key: "enable",
            label: "Application notifications",
            scope: NotificationPreferenceScope::Application,
        },
        NotificationPreferenceTarget::ApplicationShowBanners => NotificationPreferenceSpec {
            target: "application-show-banners",
            schema: NOTIFICATION_APPLICATION_SCHEMA,
            key: "show-banners",
            label: "Application banners",
            scope: NotificationPreferenceScope::Application,
        },
        NotificationPreferenceTarget::ApplicationSoundAlerts => NotificationPreferenceSpec {
            target: "application-sound-alerts",
            schema: NOTIFICATION_APPLICATION_SCHEMA,
            key: "enable-sound-alerts",
            label: "Application sound alerts",
            scope: NotificationPreferenceScope::Application,
        },
        NotificationPreferenceTarget::ApplicationShowInLockScreen => NotificationPreferenceSpec {
            target: "application-show-in-lock-screen",
            schema: NOTIFICATION_APPLICATION_SCHEMA,
            key: "show-in-lock-screen",
            label: "Application lock-screen visibility",
            scope: NotificationPreferenceScope::Application,
        },
        NotificationPreferenceTarget::ApplicationDetailsInLockScreen => {
            NotificationPreferenceSpec {
                target: "application-details-in-lock-screen",
                schema: NOTIFICATION_APPLICATION_SCHEMA,
                key: "details-in-lock-screen",
                label: "Application lock-screen details",
                scope: NotificationPreferenceScope::Application,
            }
        }
        NotificationPreferenceTarget::ApplicationForceExpanded => NotificationPreferenceSpec {
            target: "application-force-expanded",
            schema: NOTIFICATION_APPLICATION_SCHEMA,
            key: "force-expanded",
            label: "Application expanded banners",
            scope: NotificationPreferenceScope::Application,
        },
    }
}

fn notifications_detail(
    gsettings_available: bool,
    schema_available: bool,
    application_schema_available: bool,
) -> String {
    if !gsettings_available {
        return "Desktop preferences are not ready, so notification preferences are read-only in this session.".to_string();
    }
    if !schema_available {
        return "The standard notification preferences are not supported in this session."
            .to_string();
    }
    if !application_schema_available {
        return "Global notification preferences are available; per-application notification controls are not reported by this desktop session.".to_string();
    }
    "Notification preferences are ready for this desktop.".to_string()
}

fn notification_preference_success_detail(
    spec: NotificationPreferenceSpec,
    enabled: bool,
) -> String {
    match spec.target {
        "show-banners" => notification_banners_detail(enabled).to_string(),
        "show-in-lock-screen" => lock_screen_notifications_detail(enabled).to_string(),
        "application-enable" => notification_app_enable_detail(enabled).to_string(),
        "application-show-banners" => notification_app_banner_detail(enabled).to_string(),
        "application-sound-alerts" => notification_app_sound_detail(enabled).to_string(),
        "application-show-in-lock-screen" => {
            notification_app_lock_screen_detail(enabled).to_string()
        }
        "application-details-in-lock-screen" => {
            notification_app_lock_screen_details_detail(enabled).to_string()
        }
        "application-force-expanded" => notification_app_expand_detail(enabled).to_string(),
        _ => "Notification preference saved.".to_string(),
    }
}

fn notification_application_path(child: &str) -> Option<String> {
    let child = child.trim();
    if child.is_empty() || child.contains('/') || child.contains('\0') {
        return None;
    }

    let path = format!("{NOTIFICATION_APPLICATION_BASE_PATH}{child}/");
    gsettings_path_is_valid(&path).then_some(path)
}

fn gsettings_path_is_valid(path: &str) -> bool {
    path.starts_with('/') && path.ends_with('/') && !path.contains("//") && !path.contains('\0')
}

fn notification_application_label(child: &str, application_id: Option<&str>) -> String {
    let candidate = application_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| child.trim());
    candidate
        .strip_suffix(".desktop")
        .unwrap_or(candidate)
        .to_string()
}

fn notification_application_record_detail(child: &str, application_id: Option<&str>) -> String {
    match application_id.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => format!("Application ID {id} · notification registry {child}."),
        None => format!("Notification registry {child}; application ID is not reported."),
    }
}

fn notification_banners_detail(enabled: bool) -> &'static str {
    if enabled {
        "Apps can interrupt the desktop with notification banners when the shell receives them."
    } else {
        "Apps can still record notifications, but banners will not interrupt the desktop."
    }
}

fn lock_screen_notifications_detail(enabled: bool) -> &'static str {
    if enabled {
        "Notifications may appear while the session is locked, subject to each app's own notification policy."
    } else {
        "Notifications stay hidden from the lock screen until you unlock the session."
    }
}

fn notification_app_enable_detail(enabled: bool) -> &'static str {
    if enabled {
        "This application can deliver notifications when global delivery allows it."
    } else {
        "This application is muted in the desktop notification registry."
    }
}

fn notification_app_banner_detail(enabled: bool) -> &'static str {
    if enabled {
        "Banners can appear for this application when notifications arrive."
    } else {
        "Notifications can still be recorded, but banners are hidden for this application."
    }
}

fn notification_app_sound_detail(enabled: bool) -> &'static str {
    if enabled {
        "Notifications from this application may play sound alerts."
    } else {
        "Notifications from this application stay silent."
    }
}

fn notification_app_lock_screen_detail(enabled: bool) -> &'static str {
    if enabled {
        "Notifications from this application may appear while the session is locked."
    } else {
        "Notifications from this application stay hidden while the session is locked."
    }
}

fn notification_app_lock_screen_details_detail(enabled: bool) -> &'static str {
    if enabled {
        "Notification summaries and bodies may be visible before unlock."
    } else {
        "Lock-screen notifications avoid exposing summaries and message bodies."
    }
}

fn notification_app_expand_detail(enabled: bool) -> &'static str {
    if enabled {
        "Banners from this application open expanded when the shell supports it."
    } else {
        "Banners from this application use the shell's normal compact presentation."
    }
}

fn sorted_unique(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn gsettings(args: &[&str]) -> Result<String, GSettingsError> {
    match crate::session_bridge::gsettings(args) {
        crate::session_bridge::SessionBridgeResult::Success(stdout) => return Ok(stdout),
        crate::session_bridge::SessionBridgeResult::Failed(detail) => {
            return Err(GSettingsError::Failed(detail));
        }
        crate::session_bridge::SessionBridgeResult::Unavailable => {}
    }
    let output = Command::new("gsettings")
        .args(args)
        .output()
        .map_err(|_| GSettingsError::Missing)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(GSettingsError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        lock_screen_notifications_detail, notification_app_banner_detail,
        notification_app_enable_detail, notification_app_expand_detail,
        notification_app_lock_screen_detail, notification_app_lock_screen_details_detail,
        notification_app_sound_detail, notification_application_label,
        notification_application_path, notification_application_record_detail,
        notification_banners_detail, notification_preference_spec,
        notification_preference_success_detail, parse_gsettings_bool, parse_gsettings_string,
        parse_gsettings_strv, sorted_unique, NotificationPreferenceTarget,
    };

    #[test]
    fn gsettings_values_parse() {
        assert_eq!(parse_gsettings_bool("true\n"), Some(true));
        assert_eq!(parse_gsettings_bool("false"), Some(false));
        assert_eq!(parse_gsettings_bool("'false'"), None);
        assert_eq!(
            parse_gsettings_string("'org.gnome.Console.desktop'\n"),
            Some("org.gnome.Console.desktop".to_string())
        );
        assert_eq!(parse_gsettings_string(""), None);
        assert_eq!(parse_gsettings_strv("@as []"), Some(Vec::new()));
        assert_eq!(
            parse_gsettings_strv("['org-gnome-Console', 'org-gnome-TextEditor']"),
            Some(vec![
                "org-gnome-Console".to_string(),
                "org-gnome-TextEditor".to_string()
            ])
        );
        assert_eq!(parse_gsettings_strv("not a list"), None);
    }

    #[test]
    fn notification_application_path_stays_constrained() {
        assert_eq!(
            notification_application_path("org-gnome-Console").as_deref(),
            Some("/org/gnome/desktop/notifications/application/org-gnome-Console/")
        );
        assert!(notification_application_path("").is_none());
        assert!(notification_application_path("bad/path").is_none());
        assert!(notification_application_path("bad\0path").is_none());
    }

    #[test]
    fn notification_application_copy_stays_truthful() {
        assert_eq!(
            notification_application_label("org-gnome-Console", Some("org.gnome.Console.desktop")),
            "org.gnome.Console"
        );
        assert_eq!(
            notification_application_label("org-gnome-Console", Some("")),
            "org-gnome-Console"
        );
        assert!(notification_application_record_detail(
            "org-gnome-Console",
            Some("org.gnome.Console.desktop")
        )
        .contains("Application ID"));
        assert!(
            notification_application_record_detail("org-gnome-Console", None)
                .contains("not reported")
        );
    }

    #[test]
    fn notification_preference_specs_and_copy_are_allowlisted() {
        let banners = notification_preference_spec(NotificationPreferenceTarget::ShowBanners);
        assert_eq!(banners.target, "show-banners");
        assert_eq!(
            notification_preference_success_detail(banners, false),
            notification_banners_detail(false)
        );

        let app_sounds =
            notification_preference_spec(NotificationPreferenceTarget::ApplicationSoundAlerts);
        assert_eq!(app_sounds.target, "application-sound-alerts");
        assert!(notification_preference_success_detail(app_sounds, false).contains("silent"));

        assert!(lock_screen_notifications_detail(true).contains("locked"));
        assert!(notification_app_enable_detail(true).contains("can deliver"));
        assert!(notification_app_banner_detail(false).contains("hidden"));
        assert!(notification_app_sound_detail(false).contains("silent"));
        assert!(notification_app_lock_screen_detail(false).contains("hidden"));
        assert!(notification_app_lock_screen_details_detail(true).contains("summaries"));
        assert!(notification_app_expand_detail(false).contains("compact"));
    }

    #[test]
    fn notification_children_are_stable_for_display() {
        assert_eq!(
            sorted_unique(vec!["z".to_string(), "a".to_string(), "z".to_string()]),
            vec!["a".to_string(), "z".to_string()]
        );
    }
}
