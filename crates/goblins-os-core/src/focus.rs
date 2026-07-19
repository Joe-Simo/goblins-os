//! Focus modes substrate (status + schedule evaluation + arm/disarm writes).
//!
//! The macOS "Focus" altitude: user-named modes (Do Not Disturb, Work, …) that can
//! auto-arm on a weekday+time schedule, all ultimately driving the one stable DND
//! key (`show-banners`, owned by `notifications.rs`). This module ships the
//! host-testable foundation — the schedule evaluator (incl. midnight-wrap), the
//! mode/schedule JSON model stored in `org.goblins.os.focus`, and an honest-gated
//! status route, the first arm/disarm/tick write path, and narrow mode/schedule
//! CRUD. Per-app breakthrough application remains the deliberate follow-up.

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::{bounded_command_output, bounded_session_command_output, probe_timeout};

const FOCUS_SCHEMA: &str = "org.goblins.os.focus";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct FocusMode {
    id: String,
    name: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct FocusSchedule {
    #[serde(default)]
    id: String,
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

#[derive(Deserialize)]
pub struct SetFocusModeRequest {
    id: String,
    name: Option<String>,
    #[serde(default)]
    delete: bool,
}

#[derive(Deserialize)]
pub struct SetFocusScheduleRequest {
    id: String,
    mode: Option<String>,
    start: Option<u32>,
    end: Option<u32>,
    weekdays: Option<Vec<u8>>,
    #[serde(default)]
    delete: bool,
}

#[derive(Serialize)]
pub struct FocusConfigOutcome {
    ok: bool,
    text: String,
    modes: Vec<FocusMode>,
    schedules: Vec<FocusSchedule>,
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

pub async fn set_focus_mode(
    Json(request): Json<SetFocusModeRequest>,
) -> (StatusCode, Json<FocusConfigOutcome>) {
    set_focus_mode_outcome(request)
}

pub async fn set_focus_schedule(
    Json(request): Json<SetFocusScheduleRequest>,
) -> (StatusCode, Json<FocusConfigOutcome>) {
    set_focus_schedule_outcome(request)
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

fn set_focus_mode_outcome(request: SetFocusModeRequest) -> (StatusCode, Json<FocusConfigOutcome>) {
    let schema = match focus_config_schema_or_error() {
        Ok(schema) => schema,
        Err(outcome) => return outcome,
    };
    let modes = match read_focus_modes(&schema) {
        Ok(modes) => modes,
        Err(text) => {
            return focus_config_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                text,
                vec![],
                vec![],
            )
        }
    };
    let schedules = match read_focus_schedules(&schema) {
        Ok(schedules) => schedules,
        Err(text) => {
            return focus_config_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                text,
                modes,
                vec![],
            )
        }
    };

    if request.delete {
        let mode_id = request.id.trim();
        if !focus_mode_id_is_safe(mode_id) {
            return focus_config_outcome(
                StatusCode::BAD_REQUEST,
                false,
                "Focus mode ids must be 1-64 ASCII letters, numbers, '.', '-', '_', or ':'.",
                modes,
                schedules,
            );
        }
        let active_mode = setting_string(&schema, "active-mode").unwrap_or_default();
        let updated_modes =
            match delete_focus_mode(modes.clone(), &schedules, mode_id, &active_mode) {
                Ok(updated_modes) => updated_modes,
                Err((status, text)) => {
                    return focus_config_outcome(status, false, text, modes, schedules);
                }
            };
        if let Err(text) = set_focus_json(&schema, "modes", &updated_modes) {
            return focus_config_outcome(StatusCode::BAD_GATEWAY, false, text, modes, schedules);
        }
        return focus_config_outcome(
            StatusCode::OK,
            true,
            format!("Focus mode '{mode_id}' was deleted."),
            updated_modes,
            schedules,
        );
    }

    let mode = match normalize_focus_mode_request(&request) {
        Ok(mode) => mode,
        Err(text) => {
            return focus_config_outcome(StatusCode::BAD_REQUEST, false, text, modes, schedules);
        }
    };
    let updated_modes = match upsert_focus_mode(modes.clone(), mode.clone()) {
        Ok(updated_modes) => updated_modes,
        Err(text) => {
            return focus_config_outcome(StatusCode::BAD_REQUEST, false, text, modes, schedules);
        }
    };
    if let Err(text) = set_focus_json(&schema, "modes", &updated_modes) {
        return focus_config_outcome(StatusCode::BAD_GATEWAY, false, text, modes, schedules);
    }
    focus_config_outcome(
        StatusCode::OK,
        true,
        format!("Focus mode '{}' was saved.", mode.id),
        updated_modes,
        schedules,
    )
}

fn set_focus_schedule_outcome(
    request: SetFocusScheduleRequest,
) -> (StatusCode, Json<FocusConfigOutcome>) {
    let schema = match focus_config_schema_or_error() {
        Ok(schema) => schema,
        Err(outcome) => return outcome,
    };
    let modes = match read_focus_modes(&schema) {
        Ok(modes) => modes,
        Err(text) => {
            return focus_config_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                text,
                vec![],
                vec![],
            )
        }
    };
    let schedules = match read_focus_schedules(&schema) {
        Ok(schedules) => schedules,
        Err(text) => {
            return focus_config_outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                text,
                modes,
                vec![],
            )
        }
    };

    if request.delete {
        let schedule_id = request.id.trim();
        if !focus_schedule_id_is_safe(schedule_id) {
            return focus_config_outcome(
                StatusCode::BAD_REQUEST,
                false,
                "Focus schedule ids must be 1-64 ASCII letters, numbers, '.', '-', '_', or ':'.",
                modes,
                schedules,
            );
        }
        let updated_schedules = match delete_focus_schedule(schedules.clone(), schedule_id) {
            Ok(updated_schedules) => updated_schedules,
            Err((status, text)) => {
                return focus_config_outcome(status, false, text, modes, schedules);
            }
        };
        if let Err(text) = set_focus_json(&schema, "schedules", &updated_schedules) {
            return focus_config_outcome(StatusCode::BAD_GATEWAY, false, text, modes, schedules);
        }
        return focus_config_outcome(
            StatusCode::OK,
            true,
            format!("Focus schedule '{schedule_id}' was deleted."),
            modes,
            updated_schedules,
        );
    }

    let schedule = match normalize_focus_schedule_request(&request, &modes) {
        Ok(schedule) => schedule,
        Err(text) => {
            return focus_config_outcome(StatusCode::BAD_REQUEST, false, text, modes, schedules);
        }
    };
    let updated_schedules = match upsert_focus_schedule(schedules.clone(), schedule.clone()) {
        Ok(updated_schedules) => updated_schedules,
        Err(text) => {
            return focus_config_outcome(StatusCode::BAD_REQUEST, false, text, modes, schedules);
        }
    };
    if let Err(text) = set_focus_json(&schema, "schedules", &updated_schedules) {
        return focus_config_outcome(StatusCode::BAD_GATEWAY, false, text, modes, schedules);
    }
    focus_config_outcome(
        StatusCode::OK,
        true,
        format!("Focus schedule '{}' was saved.", schedule.id),
        modes,
        updated_schedules,
    )
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
    let mut schedule_ids = Vec::new();
    for schedule in &schedules {
        if !schedule.id.is_empty() {
            if !focus_schedule_id_is_safe(&schedule.id) {
                return Err("A configured Focus schedule has an invalid id.".to_string());
            }
            if schedule_ids.iter().any(|id| id == &schedule.id) {
                return Err(format!("Focus schedule '{}' is duplicated.", schedule.id));
            }
            schedule_ids.push(schedule.id.clone());
        }
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

fn normalize_focus_mode_request(request: &SetFocusModeRequest) -> Result<FocusMode, String> {
    let id = request.id.trim().to_string();
    if !focus_mode_id_is_safe(&id) {
        return Err(
            "Focus mode ids must be 1-64 ASCII letters, numbers, '.', '-', '_', or ':'."
                .to_string(),
        );
    }
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Focus mode names must be 1-80 visible characters.".to_string())?
        .to_string();
    if !focus_mode_name_is_safe(&name) {
        return Err("Focus mode names must be 1-80 visible characters.".to_string());
    }
    Ok(FocusMode { id, name })
}

fn upsert_focus_mode(mut modes: Vec<FocusMode>, mode: FocusMode) -> Result<Vec<FocusMode>, String> {
    if let Some(existing) = modes.iter_mut().find(|existing| existing.id == mode.id) {
        existing.name = mode.name;
    } else {
        if modes.len() >= 20 {
            return Err("Focus supports up to 20 configured modes.".to_string());
        }
        modes.push(mode);
    }
    Ok(modes)
}

fn delete_focus_mode(
    modes: Vec<FocusMode>,
    schedules: &[FocusSchedule],
    id: &str,
    active_mode: &str,
) -> Result<Vec<FocusMode>, (StatusCode, String)> {
    if active_mode == id {
        return Err((
            StatusCode::CONFLICT,
            "Turn Focus off before deleting the active mode.".to_string(),
        ));
    }
    if schedules.iter().any(|schedule| schedule.mode == id) {
        return Err((
            StatusCode::CONFLICT,
            "Delete schedules that use this Focus mode before deleting the mode.".to_string(),
        ));
    }
    let original_len = modes.len();
    let updated: Vec<FocusMode> = modes.into_iter().filter(|mode| mode.id != id).collect();
    if updated.len() == original_len {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Focus mode '{id}' is not configured."),
        ));
    }
    Ok(updated)
}

fn normalize_focus_schedule_request(
    request: &SetFocusScheduleRequest,
    modes: &[FocusMode],
) -> Result<FocusSchedule, String> {
    let id = request.id.trim().to_string();
    if !focus_schedule_id_is_safe(&id) {
        return Err(
            "Focus schedule ids must be 1-64 ASCII letters, numbers, '.', '-', '_', or ':'."
                .to_string(),
        );
    }
    let mode = request
        .mode
        .as_deref()
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
        .ok_or_else(|| "Focus schedules must be saved with a configured mode.".to_string())?
        .to_string();
    if !focus_mode_id_is_safe(&mode) || !modes.iter().any(|candidate| candidate.id == mode) {
        return Err("Focus schedules must be saved with a configured mode.".to_string());
    }
    let start = request
        .start
        .ok_or_else(|| "Focus schedules need a start minute.".to_string())?;
    let end = request
        .end
        .ok_or_else(|| "Focus schedules need an end minute.".to_string())?;
    if start > 1439 || end > 1439 {
        return Err("Focus schedules must use minutes within a local day.".to_string());
    }
    if start == end {
        return Err("Focus schedules need a non-empty time window.".to_string());
    }
    let weekdays = normalized_weekdays(
        request
            .weekdays
            .clone()
            .ok_or_else(|| "Focus schedules must use ISO weekdays 1 through 7.".to_string())?,
    )?;

    Ok(FocusSchedule {
        id,
        mode,
        start,
        end,
        weekdays,
    })
}

fn upsert_focus_schedule(
    mut schedules: Vec<FocusSchedule>,
    schedule: FocusSchedule,
) -> Result<Vec<FocusSchedule>, String> {
    if let Some(existing) = schedules
        .iter_mut()
        .find(|existing| existing.id == schedule.id)
    {
        *existing = schedule;
    } else {
        if schedules.len() >= 64 {
            return Err("Focus supports up to 64 schedules.".to_string());
        }
        schedules.push(schedule);
    }
    Ok(schedules)
}

fn delete_focus_schedule(
    schedules: Vec<FocusSchedule>,
    id: &str,
) -> Result<Vec<FocusSchedule>, (StatusCode, String)> {
    let original_len = schedules.len();
    let updated: Vec<FocusSchedule> = schedules
        .into_iter()
        .filter(|schedule| schedule.id != id)
        .collect();
    if updated.len() == original_len {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Focus schedule '{id}' is not configured."),
        ));
    }
    Ok(updated)
}

fn normalized_weekdays(mut weekdays: Vec<u8>) -> Result<Vec<u8>, String> {
    weekdays.sort_unstable();
    weekdays.dedup();
    if weekdays.is_empty() || weekdays.iter().any(|weekday| !(1..=7).contains(weekday)) {
        return Err("Focus schedules must use ISO weekdays 1 through 7.".to_string());
    }
    Ok(weekdays)
}

fn focus_mode_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
}

fn focus_schedule_id_is_safe(id: &str) -> bool {
    focus_mode_id_is_safe(id)
}

fn focus_mode_name_is_safe(name: &str) -> bool {
    !name.is_empty() && name.len() <= 80 && !name.chars().any(char::is_control)
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

fn focus_config_schema_or_error() -> Result<SchemaSnapshot, (StatusCode, Json<FocusConfigOutcome>)>
{
    if gsettings(&["list-schemas"]).is_err() {
        return Err(focus_config_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Desktop preferences are not ready, so Focus modes and schedules cannot be changed in this session.",
            Vec::new(),
            Vec::new(),
        ));
    }
    let schema = schema_snapshot(true, FOCUS_SCHEMA);
    if !focus_schema_ready(&schema) {
        return Err(focus_config_outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            if schema.available {
                "Focus is unavailable here because the installed Goblins Focus schema is incomplete."
            } else {
                "Focus is unavailable here (the Goblins Focus schema is not installed)."
            },
            Vec::new(),
            Vec::new(),
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

fn focus_config_outcome(
    status: StatusCode,
    ok: bool,
    text: impl Into<String>,
    modes: Vec<FocusMode>,
    schedules: Vec<FocusSchedule>,
) -> (StatusCode, Json<FocusConfigOutcome>) {
    (
        status,
        Json(FocusConfigOutcome {
            ok,
            text: text.into(),
            modes,
            schedules,
        }),
    )
}

fn read_focus_modes(schema: &SchemaSnapshot) -> Result<Vec<FocusMode>, String> {
    setting_string(schema, "modes")
        .ok_or_else(|| "Focus modes are not reported by this session.".to_string())
        .and_then(|raw| parse_focus_modes(&raw))
}

fn read_focus_schedules(schema: &SchemaSnapshot) -> Result<Vec<FocusSchedule>, String> {
    setting_string(schema, "schedules")
        .ok_or_else(|| "Focus schedules are not reported by this session.".to_string())
        .and_then(|raw| parse_focus_schedules(&raw))
}

fn set_focus_json<T: Serialize>(
    schema: &SchemaSnapshot,
    key: &str,
    value: &T,
) -> Result<(), String> {
    let encoded = serde_json::to_string(value)
        .map_err(|_| format!("Focus could not encode {key} as JSON."))?;
    set_focus_string(schema, key, &encoded)
}

/// Local (timezone-aware) weekday + minute-of-day via `date`, so schedule
/// evaluation honors the system clock without pulling a time-zone crate.
fn local_now() -> Option<(u8, u32)> {
    let output = bounded_command_output("date", &["+%u:%H:%M"], probe_timeout()).ok()?;
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
    match crate::session_bridge::gsettings(args) {
        crate::session_bridge::SessionBridgeResult::Success(stdout) => return Ok(stdout),
        crate::session_bridge::SessionBridgeResult::Failed(_) => return Err(()),
        crate::session_bridge::SessionBridgeResult::Unavailable => {}
    }
    let output =
        bounded_session_command_output("gsettings", args, probe_timeout()).map_err(|_| ())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        delete_focus_mode, delete_focus_schedule, encode_gsettings_string, focus_mode_id_is_safe,
        focus_tick_decision, normalize_focus_mode_request, normalize_focus_schedule_request,
        parse_focus_modes, parse_focus_schedules, parse_gsettings_bool, parse_local_now,
        parse_optional_bool, schedule_active, unquote_gsettings_string, upsert_focus_mode,
        upsert_focus_schedule, FocusMode, FocusSchedule, FocusTickDecision, SetFocusModeRequest,
        SetFocusScheduleRequest,
    };
    use axum::http::StatusCode;

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
            r#"[{"id":"weekday","mode":"work","start":540,"end":1020,"weekdays":[1,2,3,4,5]}]"#
        )
        .is_ok());
        assert!(
            parse_focus_schedules(
                r#"[{"id":"weekday","mode":"work","start":540,"end":1020,"weekdays":[1]},{"id":"weekday","mode":"deep","start":600,"end":700,"weekdays":[2]}]"#
            )
            .is_err()
        );
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

    #[test]
    fn focus_mode_crud_validates_names_and_references() {
        let work = normalize_focus_mode_request(&SetFocusModeRequest {
            id: "work".to_string(),
            name: Some(" Work ".to_string()),
            delete: false,
        })
        .unwrap();
        assert_eq!(work.name, "Work");
        let modes = upsert_focus_mode(Vec::new(), work).unwrap();
        assert_eq!(modes.len(), 1);

        let renamed = normalize_focus_mode_request(&SetFocusModeRequest {
            id: "work".to_string(),
            name: Some("Deep Work".to_string()),
            delete: false,
        })
        .unwrap();
        let modes = upsert_focus_mode(modes, renamed).unwrap();
        assert_eq!(modes[0].name, "Deep Work");

        assert!(normalize_focus_mode_request(&SetFocusModeRequest {
            id: "bad/id".to_string(),
            name: Some("Bad".to_string()),
            delete: false,
        })
        .is_err());
        assert!(normalize_focus_mode_request(&SetFocusModeRequest {
            id: "empty-name".to_string(),
            name: Some(" ".to_string()),
            delete: false,
        })
        .is_err());

        let schedules = vec![FocusSchedule {
            id: "weekday".to_string(),
            mode: "work".to_string(),
            start: 540,
            end: 1020,
            weekdays: vec![1, 2, 3, 4, 5],
        }];
        let Err((status, text)) = delete_focus_mode(modes.clone(), &schedules, "work", "") else {
            panic!("deleting a scheduled mode should fail");
        };
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(text.contains("Delete schedules"));

        let Err((status, _)) = delete_focus_mode(modes.clone(), &[], "work", "work") else {
            panic!("deleting an active mode should fail");
        };
        assert_eq!(status, StatusCode::CONFLICT);

        let modes = delete_focus_mode(modes, &[], "work", "").unwrap();
        assert!(modes.is_empty());
    }

    #[test]
    fn focus_schedule_crud_requires_configured_modes_and_normalizes_weekdays() {
        let modes = vec![FocusMode {
            id: "work".to_string(),
            name: "Work".to_string(),
        }];
        let schedule = normalize_focus_schedule_request(
            &SetFocusScheduleRequest {
                id: "weekday".to_string(),
                mode: Some("work".to_string()),
                start: Some(540),
                end: Some(1020),
                weekdays: Some(vec![5, 1, 1, 3]),
                delete: false,
            },
            &modes,
        )
        .unwrap();
        assert_eq!(schedule.weekdays, vec![1, 3, 5]);

        let schedules = upsert_focus_schedule(Vec::new(), schedule).unwrap();
        assert_eq!(schedules.len(), 1);

        let replacement = normalize_focus_schedule_request(
            &SetFocusScheduleRequest {
                id: "weekday".to_string(),
                mode: Some("work".to_string()),
                start: Some(600),
                end: Some(900),
                weekdays: Some(vec![2]),
                delete: false,
            },
            &modes,
        )
        .unwrap();
        let schedules = upsert_focus_schedule(schedules, replacement).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].start, 600);

        assert!(normalize_focus_schedule_request(
            &SetFocusScheduleRequest {
                id: "unknown-mode".to_string(),
                mode: Some("personal".to_string()),
                start: Some(1),
                end: Some(2),
                weekdays: Some(vec![1]),
                delete: false,
            },
            &modes,
        )
        .is_err());
        assert!(normalize_focus_schedule_request(
            &SetFocusScheduleRequest {
                id: "empty-window".to_string(),
                mode: Some("work".to_string()),
                start: Some(60),
                end: Some(60),
                weekdays: Some(vec![1]),
                delete: false,
            },
            &modes,
        )
        .is_err());

        let schedules = delete_focus_schedule(schedules, "weekday").unwrap();
        assert!(schedules.is_empty());
    }
}
