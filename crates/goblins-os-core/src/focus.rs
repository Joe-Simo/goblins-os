//! Focus modes substrate (read-only status + schedule evaluation).
//!
//! The macOS "Focus" altitude: user-named modes (Do Not Disturb, Work, …) that can
//! auto-arm on a weekday+time schedule, all ultimately driving the one stable DND
//! key (`show-banners`, owned by `notifications.rs`). This module ships the
//! host-testable foundation — the schedule evaluator (incl. midnight-wrap), the
//! mode/schedule JSON model stored in `org.goblins.os.focus`, and an honest-gated
//! status route. Arm/disarm writes, the per-app breakthrough allowlist, the
//! schedule timer, and the Settings/Control-Center/menu-bar surfaces are the
//! deliberate follow-up; nothing here writes — it reports.

use std::process::{Command, Stdio};

use axum::Json;
use serde::{Deserialize, Serialize};

const FOCUS_SCHEMA: &str = "org.goblins.os.focus";

#[derive(Clone, Serialize, Deserialize)]
pub struct FocusMode {
    id: String,
    name: String,
}

#[derive(Clone, Deserialize)]
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
    modes: Vec<FocusMode>,
    detail: String,
}

pub async fn focus_status() -> Json<FocusStatus> {
    Json(build_focus_status())
}

fn build_focus_status() -> FocusStatus {
    let gsettings_available = gsettings(&["list-schemas"]).is_ok();
    let schema = schema_snapshot(gsettings_available, FOCUS_SCHEMA);
    if !schema.available {
        return FocusStatus {
            source: "goblins-os-core",
            available: false,
            active_mode: String::new(),
            scheduled_mode: None,
            modes: Vec::new(),
            detail: "Focus is unavailable here (the Goblins Focus schema is not installed)."
                .to_string(),
        };
    }

    let active_mode = setting_string(&schema, "active-mode").unwrap_or_default();
    let modes = setting_string(&schema, "modes")
        .and_then(|raw| serde_json::from_str::<Vec<FocusMode>>(&raw).ok())
        .unwrap_or_default();
    let schedules = setting_string(&schema, "schedules")
        .and_then(|raw| serde_json::from_str::<Vec<FocusSchedule>>(&raw).ok())
        .unwrap_or_default();

    let scheduled_mode = local_now().and_then(|(weekday, minutes)| {
        scheduled_mode_for(&schedules, weekday, minutes).map(str::to_string)
    });

    let detail = if active_mode.is_empty() {
        match &scheduled_mode {
            Some(mode) => format!("Focus is off. A schedule would arm “{mode}” now."),
            None => "Focus is off.".to_string(),
        }
    } else {
        format!("Focus mode “{active_mode}” is active.")
    };

    FocusStatus {
        source: "goblins-os-core",
        available: true,
        active_mode,
        scheduled_mode,
        modes,
        detail,
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
}

fn schema_snapshot(gsettings_available: bool, schema: &str) -> SchemaSnapshot {
    let available = gsettings_available
        && gsettings(&["list-keys", schema])
            .map(|out| !out.trim().is_empty())
            .unwrap_or(false);
    SchemaSnapshot { available }
}

fn setting_string(schema: &SchemaSnapshot, key: &str) -> Option<String> {
    if !schema.available {
        return None;
    }
    gsettings(&["get", FOCUS_SCHEMA, key])
        .ok()
        .map(|value| unquote_gsettings_string(&value))
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
    use super::{parse_local_now, schedule_active, unquote_gsettings_string};

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
}
