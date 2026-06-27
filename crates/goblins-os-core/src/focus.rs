//! Focus modes substrate (status + schedule evaluation + arm/disarm writes).
//!
//! The macOS "Focus" altitude: user-named modes (Do Not Disturb, Work, …) that can
//! auto-arm on a weekday+time schedule, all ultimately driving the one stable DND
//! key (`show-banners`, owned by `notifications.rs`). This module ships the
//! host-testable foundation — the schedule evaluator (incl. midnight-wrap), the
//! mode/schedule JSON model stored in `org.goblins.os.focus`, and an honest-gated
//! status route, and the first arm/disarm/tick write path. Per-app breakthrough
//! allowlists, the schedule timer unit, and the Settings/Control-Center/menu-bar
//! surfaces are the deliberate follow-up.

use std::process::{Command, Stdio};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

const FOCUS_SCHEMA: &str = "org.goblins.os.focus";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct FocusMode {
    id: String,
    name: String,
}

#[derive(Clone, Deserialize, PartialEq, Eq, Debug)]
struct FocusSchedule {
    mode: String,
    /// Minutes from midnight (local).
    start: u32,
    end: u32,
    /// ISO weekdays the schedule applies to (1 = Monday … 7 = Sunday).
    weekdays: Vec<u8>,
}

#[derive(Serialize)]
pub struct FocusStatus {
    source: &'static str,
    available: bool,
    active_mode: String,
    /// The mode the schedules say should be armed right now, if any.
    scheduled_mode: Option<String>,
    armed_by_schedule: bool,
    modes: Vec<FocusMode>,
    detail: String,
}

#[derive(Deserialize)]
pub struct ActivateFocusRequest {
    mode: String,
}

#[derive(Serialize)]
pub struct FocusActionOutcome {
    ok: bool,
    active_mode: String,
    text: String,
}

pub async fn focus_status() -> Json<FocusStatus> {
    Json(build_focus_status())
}

pub async fn activate_focus(
    Json(request): Json<ActivateFocusRequest>,
) -> (StatusCode, Json<FocusActionOutcome>) {
    activate_focus_outcome(request.mode, false)
}

pub async fn deactivate_focus() -> (StatusCode, Json<FocusActionOutcome>) {
    deactivate_focus_outcome()
}

pub async fn focus_tick() -> (StatusCode, Json<FocusActionOutcome>) {
    focus_tick_outcome()
}

fn build_focus_status() -> FocusStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available, FOCUS_SCHEMA);
    if !focus_schema_ready(&schema) {
        return FocusStatus {
            source: "goblins-os-core",
            available: false,
            active_mode: String::new(),
            scheduled_mode: None,
            armed_by_schedule: false,
            modes: Vec::new(),
            detail: if schema.available {
                "Focus is unavailable here because the installed Goblins Focus schema is incomplete."
                    .to_string()
            } else {
                "Focus is unavailable here (the Goblins Focus schema is not installed).".to_string()
            },
        };
    }

    let active_mode = setting_string(&schema, "active-mode").unwrap_or_default();
    let armed_by_schedule = setting_bool(&schema, "armed-by-schedule").unwrap_or(false);
    let modes = setting_string(&schema, "modes")
        .and_then(|raw| parse_focus_modes(&raw).ok())
        .unwrap_or_default();
    let schedules = setting_string(&schema, "schedules")
        .and_then(|raw| parse_focus_schedules(&raw).ok())
        .unwrap_or_default();

    let scheduled_mode = local_now().and_then(|(weekday, minutes)| {
        scheduled_mode_for(&schedules, weekday, minutes).map(str::to_string)
    });

    let detail = if active_mode.is_empty() {
        match &scheduled_mode {
            Some(mode) => format!("Focus is off. A schedule would arm “{mode}” now."),
            None => "Focus is off.".to_string(),
        }
    } else if armed_by_schedule {
        format!("Focus mode “{active_mode}” is active from a schedule.")
    } else {
        format!("Focus mode “{active_mode}” is active.")
    };

    FocusStatus {
        source: "goblins-os-core",
        available: true,
        active_mode,
        scheduled_mode,
        armed_by_schedule,
        modes,
        detail,
    }
}

fn activate_focus_outcome(
    requested_mode: String,
    armed_by_schedule: bool,
) -> (StatusCode, Json<FocusActionOutcome>) {
    let schema = match focus_schema_or_error() {
        Ok(schema) => schema,
        Err(outcome) => return outcome,
    };
    let modes = match setting_string(&schema, "modes")
        .ok_or_else(|| "Focus modes are not reported by this session.".to_string())
        .and_then(|raw| parse_focus_modes(&raw))
    {
        Ok(modes) => modes,
        Err(text) => return focus_outcome(StatusCode::SERVICE_UNAVAILABLE, false, "", text),
    };
    let mode_id = requested_mode.trim().to_string();
    if !focus_mode_id_is_safe(&mode_id) {
        return focus_outcome(
            StatusCode::BAD_REQUEST,
            false,
            "",
            "Focus mode ids must be 1-64 ASCII letters, numbers, '.', '-', '_', or ':'.",
        );
    }
    if !modes.iter().any(|mode| mode.id == mode_id) {
        return focus_outcome(
            StatusCode::BAD_REQUEST,
            false,
            "",
            format!("Focus mode '{mode_id}' is not configured."),
        );
    }

    let active_mode = setting_string(&schema, "active-mode").unwrap_or_default();
    let snapshot = if active_mode.is_empty() {
        match crate::notifications::read_notification_banners() {
            Ok(value) => Some(value),
            Err(text) => return focus_outcome(StatusCode::SERVICE_UNAVAILABLE, false, "", text),
        }
    } else {
        None
    };

    if let Some(value) = snapshot {
        if let Err(text) = set_focus_string(&schema, "restore-banners", bool_snapshot(value)) {
            return focus_outcome(StatusCode::BAD_GATEWAY, false, &active_mode, text);
        }
    }
    if let Err(text) = set_focus_bool(&schema, "armed-by-schedule", armed_by_schedule) {
        if snapshot.is_some() {
            let _ = set_focus_string(&schema, "restore-banners", "");
        }
        return focus_outcome(StatusCode::BAD_GATEWAY, false, &active_mode, text);
    }

    let (notification_status, notification_text) =
        crate::notifications::apply_notification_banners(false);
    if !notification_status.is_success() {
        if snapshot.is_some() {
            let _ = set_focus_bool(&schema, "armed-by-schedule", false);
            let _ = set_focus_string(&schema, "restore-banners", "");
        }
        return focus_outcome(
            notification_status,
            false,
            &active_mode,
            format!("Focus could not silence notification banners: {notification_text}"),
        );
    }

    if let Err(text) = set_focus_string(&schema, "active-mode", &mode_id) {
        if let Some(value) = snapshot {
            let _ = crate::notifications::apply_notification_banners(value);
            let _ = set_focus_string(&schema, "restore-banners", "");
        }
        let _ = set_focus_bool(&schema, "armed-by-schedule", false);
        return focus_outcome(StatusCode::BAD_GATEWAY, false, &active_mode, text);
    }

    focus_outcome(
        StatusCode::OK,
        true,
        &mode_id,
        if armed_by_schedule {
            format!("Focus mode '{mode_id}' is active from the current schedule.")
        } else {
            format!("Focus mode '{mode_id}' is active. Notification banners are silenced.")
        },
    )
}

fn deactivate_focus_outcome() -> (StatusCode, Json<FocusActionOutcome>) {
    let schema = match focus_schema_or_error() {
        Ok(schema) => schema,
        Err(outcome) => return outcome,
    };
    let active_mode = setting_string(&schema, "active-mode").unwrap_or_default();
    if active_mode.is_empty() {
        let _ = set_focus_bool(&schema, "armed-by-schedule", false);
        return focus_outcome(StatusCode::OK, true, "", "Focus is already off.");
    }

    let restore_banners =
        setting_string(&schema, "restore-banners").and_then(|value| parse_optional_bool(&value));
    if let Some(value) = restore_banners {
        let (status, text) = crate::notifications::apply_notification_banners(value);
        if !status.is_success() {
            return focus_outcome(
                status,
                false,
                &active_mode,
                format!("Focus could not restore notification banners: {text}"),
            );
        }
    }

    if let Err(text) = set_focus_string(&schema, "active-mode", "") {
        return focus_outcome(StatusCode::BAD_GATEWAY, false, &active_mode, text);
    }
    let _ = set_focus_bool(&schema, "armed-by-schedule", false);
    let _ = set_focus_string(&schema, "restore-banners", "");

    focus_outcome(
        StatusCode::OK,
        true,
        "",
        if restore_banners.is_some() {
            "Focus is off. Notification banners were restored from the saved snapshot.".to_string()
        } else {
            "Focus is off. No saved notification snapshot was available, so the current banner preference was left unchanged.".to_string()
        },
    )
}

fn focus_tick_outcome() -> (StatusCode, Json<FocusActionOutcome>) {
    let schema = match focus_schema_or_error() {
        Ok(schema) => schema,
        Err(outcome) => return outcome,
    };
    let active_mode = setting_string(&schema, "active-mode").unwrap_or_default();
    let armed_by_schedule = setting_bool(&schema, "armed-by-schedule").unwrap_or(false);
    let schedules = match setting_string(&schema, "schedules")
        .ok_or_else(|| "Focus schedules are not reported by this session.".to_string())
        .and_then(|raw| parse_focus_schedules(&raw))
    {
        Ok(schedules) => schedules,
        Err(text) => {
            return focus_outcome(StatusCode::SERVICE_UNAVAILABLE, false, &active_mode, text)
        }
    };
    let Some((weekday, minutes)) = local_now() else {
        return focus_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            &active_mode,
            "Focus schedules could not be evaluated because local time is unavailable.",
        );
    };
    let scheduled_mode = scheduled_mode_for(&schedules, weekday, minutes);

    match focus_tick_decision(&active_mode, armed_by_schedule, scheduled_mode) {
        FocusTickDecision::Activate(mode) => activate_focus_outcome(mode, true),
        FocusTickDecision::Deactivate => deactivate_focus_outcome(),
        FocusTickDecision::NoChange => focus_outcome(
            StatusCode::OK,
            true,
            &active_mode,
            "Focus schedule tick made no changes.",
        ),
    }
}

/// Is a schedule active at the given local weekday (1=Mon..7=Sun) and minute of
/// day? Handles overnight schedules where `end < start` (e.g. 22:00–07:00). A
/// zero-length window (`start == end`) is never active. Pure + unit-tested.
fn schedule_active(start: u32, end: u32, weekdays: &[u8], weekday: u8, minutes: u32) -> bool {
    if !weekdays.contains(&weekday) {
        return false;
    }
    if start == end {
        return false;
    }
    if start < end {
        minutes >= start && minutes < end
    } else {
        // Overnight wrap: active from `start` to midnight, then midnight to `end`.
        minutes >= start || minutes < end
    }
}

/// The mode the first matching schedule arms at this time, if any. Pure.
fn scheduled_mode_for(schedules: &[FocusSchedule], weekday: u8, minutes: u32) -> Option<&str> {
    schedules
        .iter()
        .find(|s| schedule_active(s.start, s.end, &s.weekdays, weekday, minutes))
        .map(|s| s.mode.as_str())
}

#[derive(PartialEq, Eq, Debug)]
enum FocusTickDecision {
    Activate(String),
    Deactivate,
    NoChange,
}

fn focus_tick_decision(
    active_mode: &str,
    armed_by_schedule: bool,
    scheduled_mode: Option<&str>,
) -> FocusTickDecision {
    match scheduled_mode {
        Some(mode) if active_mode.is_empty() => FocusTickDecision::Activate(mode.to_string()),
        Some(mode) if armed_by_schedule && active_mode != mode => {
            FocusTickDecision::Activate(mode.to_string())
        }
        None if armed_by_schedule && !active_mode.is_empty() => FocusTickDecision::Deactivate,
        _ => FocusTickDecision::NoChange,
    }
}

fn parse_focus_modes(raw: &str) -> Result<Vec<FocusMode>, String> {
    let modes: Vec<FocusMode> = serde_json::from_str(raw)
        .map_err(|_| "Focus modes could not be decoded from settings.".to_string())?;
    let mut normalized = Vec::with_capacity(modes.len());
    for mode in modes {
        let id = mode.id.trim().to_string();
        let name = mode.name.trim().to_string();
        if !focus_mode_id_is_safe(&id) {
            return Err("A configured Focus mode has an invalid id.".to_string());
        }
        if name.is_empty() {
            return Err(format!("Focus mode '{id}' has no display name."));
        }
        if normalized
            .iter()
            .any(|candidate: &FocusMode| candidate.id == id)
        {
            return Err(format!("Focus mode '{id}' is duplicated."));
        }
        normalized.push(FocusMode { id, name });
    }
    Ok(normalized)
}

fn parse_focus_schedules(raw: &str) -> Result<Vec<FocusSchedule>, String> {
    let schedules: Vec<FocusSchedule> = serde_json::from_str(raw)
        .map_err(|_| "Focus schedules could not be decoded from settings.".to_string())?;
    for schedule in &schedules {
        if !focus_mode_id_is_safe(&schedule.mode) {
            return Err("A configured Focus schedule references an invalid mode id.".to_string());
        }
        if schedule.start > 1439 || schedule.end > 1439 {
            return Err("Focus schedules must use minutes within a local day.".to_string());
        }
        if schedule.weekdays.is_empty()
            || schedule
                .weekdays
                .iter()
                .any(|weekday| !(1..=7).contains(weekday))
        {
            return Err("Focus schedules must use ISO weekdays 1 through 7.".to_string());
        }
    }
    Ok(schedules)
}

fn focus_mode_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
}

fn focus_schema_ready(schema: &SchemaSnapshot) -> bool {
    schema.available
        && [
            "active-mode",
            "modes",
            "schedules",
            "armed-by-schedule",
            "restore-banners",
            "restore-apps",
        ]
        .iter()
        .all(|key| schema.has_key(key))
}

fn focus_schema_or_error() -> Result<SchemaSnapshot, (StatusCode, Json<FocusActionOutcome>)> {
    if gsettings(&["list-schemas"]).is_err() {
        return Err(focus_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "",
            "Desktop preferences are not ready, so Focus cannot be changed in this session.",
        ));
    }
    let schema = schema_snapshot(true, FOCUS_SCHEMA);
    if !focus_schema_ready(&schema) {
        return Err(focus_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "",
            if schema.available {
                "Focus is unavailable here because the installed Goblins Focus schema is incomplete."
            } else {
                "Focus is unavailable here (the Goblins Focus schema is not installed)."
            },
        ));
    }
    Ok(schema)
}

fn focus_outcome(
    status: StatusCode,
    ok: bool,
    active_mode: &str,
    text: impl Into<String>,
) -> (StatusCode, Json<FocusActionOutcome>) {
    (
        status,
        Json(FocusActionOutcome {
            ok,
            active_mode: active_mode.to_string(),
            text: text.into(),
        }),
    )
}

/// Local (timezone-aware) weekday + minute-of-day via `date`, so schedule
/// evaluation honors the system clock without pulling a time-zone crate.
fn local_now() -> Option<(u8, u32)> {
    let output = Command::new("date")
        .arg("+%u:%H:%M")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_local_now(text.trim())
}

/// Parse `date +%u:%H:%M` output ("3:14:25" → Wed, 14:25). Pure + unit-tested.
fn parse_local_now(text: &str) -> Option<(u8, u32)> {
    let mut parts = text.split(':');
    let weekday: u8 = parts.next()?.parse().ok()?;
    let hour: u32 = parts.next()?.parse().ok()?;
    let minute: u32 = parts.next()?.parse().ok()?;
    if !(1..=7).contains(&weekday) || hour > 23 || minute > 59 {
        return None;
    }
    Some((weekday, hour * 60 + minute))
}

struct SchemaSnapshot {
    available: bool,
    keys: Vec<String>,
}

fn schema_snapshot(gsettings_available: bool, schema: &str) -> SchemaSnapshot {
    if !gsettings_available {
        return SchemaSnapshot {
            available: false,
            keys: Vec::new(),
        };
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
        Err(_) => SchemaSnapshot {
            available: false,
            keys: Vec::new(),
        },
    }
}

fn setting_string(schema: &SchemaSnapshot, key: &str) -> Option<String> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", FOCUS_SCHEMA, key])
        .ok()
        .map(|value| unquote_gsettings_string(&value))
}

fn setting_bool(schema: &SchemaSnapshot, key: &str) -> Option<bool> {
    if !schema.has_key(key) {
        return None;
    }
    gsettings(&["get", FOCUS_SCHEMA, key])
        .ok()
        .and_then(|value| parse_gsettings_bool(&value))
}

fn set_focus_string(schema: &SchemaSnapshot, key: &str, value: &str) -> Result<(), String> {
    if !schema.has_key(key) {
        return Err(format!(
            "Focus cannot save {key} because the schema key is missing."
        ));
    }
    gsettings(&["set", FOCUS_SCHEMA, key, &encode_gsettings_string(value)])
        .map(|_| ())
        .map_err(|_| format!("Focus could not save {key} in this desktop session."))
}

fn set_focus_bool(schema: &SchemaSnapshot, key: &str, value: bool) -> Result<(), String> {
    if !schema.has_key(key) {
        return Err(format!(
            "Focus cannot save {key} because the schema key is missing."
        ));
    }
    let encoded = value.to_string();
    gsettings(&["set", FOCUS_SCHEMA, key, &encoded])
        .map(|_| ())
        .map_err(|_| format!("Focus could not save {key} in this desktop session."))
}

impl SchemaSnapshot {
    fn has_key(&self, key: &str) -> bool {
        self.keys.iter().any(|candidate| candidate == key)
    }
}

/// Strip the surrounding single quotes gsettings prints for a string value and
/// unescape `\'`/`\\`. Pure + unit-tested.
fn unquote_gsettings_string(value: &str) -> String {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or(trimmed);
    inner.replace("\\'", "'").replace("\\\\", "\\")
}

fn parse_gsettings_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn bool_snapshot(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn parse_optional_bool(value: &str) -> Option<bool> {
    match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn encode_gsettings_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn gsettings(args: &[&str]) -> Result<String, ()> {
    let output = Command::new("gsettings")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| ())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        encode_gsettings_string, focus_mode_id_is_safe, focus_tick_decision, parse_focus_modes,
        parse_focus_schedules, parse_gsettings_bool, parse_local_now, parse_optional_bool,
        schedule_active, unquote_gsettings_string, FocusTickDecision,
    };

    #[test]
    fn schedule_active_matches_weekday_and_window() {
        let weekdays = [1, 2, 3, 4, 5]; // Mon–Fri
                                        // 09:00 (540) to 17:00 (1020)
        assert!(schedule_active(540, 1020, &weekdays, 3, 600)); // Wed 10:00 → active
        assert!(!schedule_active(540, 1020, &weekdays, 3, 480)); // Wed 08:00 → before
        assert!(!schedule_active(540, 1020, &weekdays, 3, 1020)); // 17:00 exact → end-exclusive
        assert!(!schedule_active(540, 1020, &weekdays, 6, 600)); // Sat → wrong weekday
    }

    #[test]
    fn schedule_active_handles_overnight_wrap() {
        let weekdays = [1, 2, 3, 4, 5, 6, 7];
        // 22:00 (1320) to 07:00 (420)
        assert!(schedule_active(1320, 420, &weekdays, 1, 1380)); // 23:00 → active
        assert!(schedule_active(1320, 420, &weekdays, 1, 60)); // 01:00 → active
        assert!(!schedule_active(1320, 420, &weekdays, 1, 600)); // 10:00 → inactive
                                                                 // Zero-length window is never active.
        assert!(!schedule_active(600, 600, &weekdays, 1, 600));
    }

    #[test]
    fn parses_local_now() {
        assert_eq!(parse_local_now("3:14:25"), Some((3, 14 * 60 + 25)));
        assert_eq!(parse_local_now("7:00:00"), Some((7, 0)));
        assert_eq!(parse_local_now("8:00:00"), None); // weekday out of range
        assert_eq!(parse_local_now("3:24:00"), None); // hour out of range
        assert_eq!(parse_local_now("garbage"), None);
    }

    #[test]
    fn unquotes_gsettings_strings() {
        assert_eq!(unquote_gsettings_string("'work'"), "work");
        assert_eq!(unquote_gsettings_string("''"), "");
        assert_eq!(unquote_gsettings_string("'[]'"), "[]");
        assert_eq!(unquote_gsettings_string("'it\\'s'"), "it's");
    }

    #[test]
    fn focus_mode_ids_and_json_are_validated() {
        assert!(focus_mode_id_is_safe("work"));
        assert!(focus_mode_id_is_safe("do-not-disturb"));
        assert!(!focus_mode_id_is_safe(""));
        assert!(!focus_mode_id_is_safe("work;rm"));

        let modes = parse_focus_modes(r#"[{"id":"work","name":" Work " }]"#).unwrap();
        assert_eq!(modes[0].id, "work");
        assert_eq!(modes[0].name, "Work");
        assert!(
            parse_focus_modes(r#"[{"id":"work","name":"Work"},{"id":"work","name":"Deep"}]"#)
                .is_err()
        );
        assert!(parse_focus_modes(r#"[{"id":"bad/id","name":"Bad"}]"#).is_err());

        assert!(parse_focus_schedules(
            r#"[{"mode":"work","start":540,"end":1020,"weekdays":[1,2,3,4,5]}]"#
        )
        .is_ok());
        assert!(
            parse_focus_schedules(r#"[{"mode":"work","start":1440,"end":1,"weekdays":[1]}]"#)
                .is_err()
        );
        assert!(
            parse_focus_schedules(r#"[{"mode":"work","start":1,"end":2,"weekdays":[0]}]"#).is_err()
        );
    }

    #[test]
    fn focus_tick_respects_manual_focus_and_schedule_ownership() {
        assert_eq!(
            focus_tick_decision("", false, Some("work")),
            FocusTickDecision::Activate("work".to_string())
        );
        assert_eq!(
            focus_tick_decision("work", true, Some("deep-work")),
            FocusTickDecision::Activate("deep-work".to_string())
        );
        assert_eq!(
            focus_tick_decision("work", true, None),
            FocusTickDecision::Deactivate
        );
        assert_eq!(
            focus_tick_decision("personal", false, Some("work")),
            FocusTickDecision::NoChange
        );
        assert_eq!(
            focus_tick_decision("work", true, Some("work")),
            FocusTickDecision::NoChange
        );
    }

    #[test]
    fn focus_gsettings_scalar_helpers_are_stable() {
        assert_eq!(parse_gsettings_bool("true"), Some(true));
        assert_eq!(parse_gsettings_bool("'true'"), None);
        assert_eq!(parse_optional_bool("false"), Some(false));
        assert_eq!(parse_optional_bool(""), None);
        assert_eq!(encode_gsettings_string("work"), "'work'");
        assert_eq!(encode_gsettings_string("it\\'s"), "'it\\\\\\'s'");
    }
}
