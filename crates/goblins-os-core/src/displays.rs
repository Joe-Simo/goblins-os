//! Display and compositor status for Settings.
//!
//! Mutable resolution, scale, mirroring, and arrangement changes go through
//! Mutter's stable DisplayConfig D-Bus API. The core owns the allowlist and live
//! serial checks so Settings never writes arbitrary display state or reports a
//! successful apply when the compositor gate is absent.

use std::{env, fs};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::bounded::{bounded_command_output, probe_timeout, BoundedCommandError};
use crate::session_bridge::{
    DisplayConfigLogicalMonitor, DisplayConfigMonitor, SessionBridgeResult,
};

const MUTTER_DISPLAY_CONFIG_DEST: &str = "org.gnome.Mutter.DisplayConfig";
const MUTTER_DISPLAY_CONFIG_PATH: &str = "/org/gnome/Mutter/DisplayConfig";
const MUTTER_DISPLAY_CONFIG_GET_CURRENT_STATE: &str =
    "org.gnome.Mutter.DisplayConfig.GetCurrentState";
const MUTTER_DISPLAY_CONFIG_APPLY_MONITORS: &str =
    "org.gnome.Mutter.DisplayConfig.ApplyMonitorsConfig";

#[derive(Serialize)]
pub struct DisplaysStatus {
    source: &'static str,
    session_type: String,
    desktop: String,
    current_desktop: String,
    wayland_display: Option<String>,
    x11_display: Option<String>,
    gdbus_available: bool,
    mutter_display_config_available: bool,
    mutter_display_apply_allowed: bool,
    display_config_serial: Option<u32>,
    xrandr_available: bool,
    outputs: Vec<DisplayOutputStatus>,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DisplayOutputStatus {
    name: String,
    connected: bool,
    primary: bool,
    current_mode: Option<String>,
    position: Option<String>,
    detail: String,
}

#[derive(Deserialize)]
pub struct ApplyDisplaysRequest {
    serial: u32,
    method: String,
    #[serde(default)]
    confirm_persistent: bool,
    logical_monitors: Vec<LogicalMonitorRequest>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct LogicalMonitorRequest {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<MonitorConfigRequest>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct MonitorConfigRequest {
    connector: String,
    mode_id: String,
}

#[derive(Serialize)]
pub struct ApplyDisplaysOutcome {
    ok: bool,
    text: String,
    method: String,
    serial: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayApplyMethod {
    Verify,
    Temporary,
    Persistent,
}

#[derive(Debug, PartialEq, Eq)]
enum DisplayConfigError {
    Missing,
    Failed(String),
}

pub async fn displays_status() -> Json<DisplaysStatus> {
    Json(build_displays_status())
}

pub async fn apply_displays(
    Json(request): Json<ApplyDisplaysRequest>,
) -> (StatusCode, Json<ApplyDisplaysOutcome>) {
    let (status, outcome) = apply_displays_outcome(request);
    (status, Json(outcome))
}

fn build_displays_status() -> DisplaysStatus {
    let session_type = env_string("XDG_SESSION_TYPE", "unconfigured");
    let desktop = env_string("DESKTOP_SESSION", "unconfigured");
    let current_desktop = env_string("XDG_CURRENT_DESKTOP", "unconfigured");
    let wayland_display = env_optional("WAYLAND_DISPLAY");
    let x11_display = env_optional("DISPLAY");
    let gdbus_available = executable_exists("gdbus");
    let current_state = if gdbus_available {
        mutter_current_state().ok()
    } else {
        None
    };
    let mutter_display_config_available = current_state.is_some();
    let display_config_serial = current_state
        .as_deref()
        .and_then(parse_current_state_serial);
    let mutter_display_apply_allowed =
        gdbus_available && mutter_display_config_apply_allowed().unwrap_or(false);
    let xrandr_available = executable_exists("xrandr");
    let outputs = if xrandr_available {
        xrandr_outputs().unwrap_or_default()
    } else {
        Vec::new()
    };
    let detail = displays_detail(
        wayland_display.as_deref(),
        x11_display.as_deref(),
        mutter_display_config_available,
        xrandr_available,
        outputs.len(),
    );

    DisplaysStatus {
        source: "goblins-os-core",
        session_type,
        desktop,
        current_desktop,
        wayland_display,
        x11_display,
        gdbus_available,
        mutter_display_config_available,
        mutter_display_apply_allowed,
        display_config_serial,
        xrandr_available,
        outputs,
        detail,
    }
}

fn mutter_current_state() -> Result<String, DisplayConfigError> {
    match crate::session_bridge::display_config_get_current_state() {
        SessionBridgeResult::Success(stdout) => return Ok(stdout),
        SessionBridgeResult::Failed(detail) => return Err(DisplayConfigError::Failed(detail)),
        SessionBridgeResult::Unavailable => {}
    }
    gdbus_call(&[MUTTER_DISPLAY_CONFIG_GET_CURRENT_STATE])
}

fn mutter_display_config_apply_allowed() -> Result<bool, DisplayConfigError> {
    let reply = match crate::session_bridge::display_config_get_apply_allowed() {
        SessionBridgeResult::Success(stdout) => stdout,
        SessionBridgeResult::Failed(detail) => return Err(DisplayConfigError::Failed(detail)),
        SessionBridgeResult::Unavailable => gdbus_call(&[
            "org.freedesktop.DBus.Properties.Get",
            MUTTER_DISPLAY_CONFIG_DEST,
            "ApplyMonitorsConfigAllowed",
        ])?,
    };
    Ok(parse_gdbus_bool(&reply).unwrap_or(false))
}

fn apply_displays_outcome(request: ApplyDisplaysRequest) -> (StatusCode, ApplyDisplaysOutcome) {
    let method = match parse_apply_method(&request.method) {
        Ok(method) => method,
        Err(message) => {
            return apply_displays_response(
                StatusCode::BAD_REQUEST,
                false,
                message,
                request.method,
                request.serial,
            )
        }
    };
    if method == DisplayApplyMethod::Persistent && !request.confirm_persistent {
        return apply_displays_response(
            StatusCode::BAD_REQUEST,
            false,
            "Persistent display changes require an explicit keep confirmation.",
            request.method,
            request.serial,
        );
    }
    if request.serial == 0 {
        return apply_displays_response(
            StatusCode::BAD_REQUEST,
            false,
            "Display changes require the current compositor serial.",
            request.method,
            request.serial,
        );
    }
    if let Err(message) = validate_logical_monitors(&request.logical_monitors) {
        return apply_displays_response(
            StatusCode::BAD_REQUEST,
            false,
            &message,
            request.method,
            request.serial,
        );
    }
    if !executable_exists("gdbus") {
        return apply_displays_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Display configuration is read-only because the desktop bridge is missing.",
            request.method,
            request.serial,
        );
    }
    match mutter_display_config_apply_allowed() {
        Ok(true) => {}
        Ok(false) => {
            return apply_displays_response(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                "Display configuration changes are blocked by the current desktop session.",
                request.method,
                request.serial,
            )
        }
        Err(DisplayConfigError::Missing) => {
            return apply_displays_response(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                "Display configuration is read-only because the compositor DisplayConfig service is missing.",
                request.method,
                request.serial,
            )
        }
        Err(DisplayConfigError::Failed(detail)) => {
            return apply_displays_response(
                StatusCode::BAD_GATEWAY,
                false,
                &format!("Display configuration cannot be changed right now: {detail}"),
                request.method,
                request.serial,
            )
        }
    }
    match mutter_current_state().and_then(|state| {
        parse_current_state_serial(&state).ok_or_else(|| {
            DisplayConfigError::Failed("compositor did not report a display serial".to_string())
        })
    }) {
        Ok(current_serial) if current_serial == request.serial => {}
        Ok(_) => {
            return apply_displays_response(
                StatusCode::CONFLICT,
                false,
                "Display layout changed before apply; reload the display panel and try again.",
                request.method,
                request.serial,
            )
        }
        Err(DisplayConfigError::Missing) => {
            return apply_displays_response(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                "Display configuration is read-only because the compositor DisplayConfig service is missing.",
                request.method,
                request.serial,
            )
        }
        Err(DisplayConfigError::Failed(detail)) => {
            return apply_displays_response(
                StatusCode::BAD_GATEWAY,
                false,
                &format!("Display configuration cannot be read before apply: {detail}"),
                request.method,
                request.serial,
            )
        }
    }

    let logical_monitors = encode_logical_monitors(&request.logical_monitors);
    let method_value = apply_method_value(method).to_string();
    let serial = request.serial.to_string();
    match mutter_apply_monitors_config(
        request.serial,
        apply_method_value(method),
        &request.logical_monitors,
        &serial,
        &method_value,
        &logical_monitors,
    ) {
        Ok(_) => apply_displays_response(
            StatusCode::OK,
            true,
            apply_success_text(method),
            request.method,
            request.serial,
        ),
        Err(DisplayConfigError::Missing) => apply_displays_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Display configuration is read-only because the desktop bridge is missing.",
            request.method,
            request.serial,
        ),
        Err(DisplayConfigError::Failed(detail)) => apply_displays_response(
            StatusCode::BAD_GATEWAY,
            false,
            &format!("The compositor rejected the display configuration: {detail}"),
            request.method,
            request.serial,
        ),
    }
}

fn mutter_apply_monitors_config(
    serial: u32,
    method: u32,
    request_monitors: &[LogicalMonitorRequest],
    serial_text: &str,
    method_text: &str,
    logical_monitors_text: &str,
) -> Result<String, DisplayConfigError> {
    match crate::session_bridge::display_config_apply_monitors(
        serial,
        method,
        bridge_logical_monitors(request_monitors),
    ) {
        SessionBridgeResult::Success(stdout) => return Ok(stdout),
        SessionBridgeResult::Failed(detail) => return Err(DisplayConfigError::Failed(detail)),
        SessionBridgeResult::Unavailable => {}
    }
    gdbus_call(&[
        MUTTER_DISPLAY_CONFIG_APPLY_MONITORS,
        serial_text,
        method_text,
        logical_monitors_text,
        "{}",
    ])
}

fn bridge_logical_monitors(
    monitors: &[LogicalMonitorRequest],
) -> Vec<DisplayConfigLogicalMonitor<'_>> {
    monitors
        .iter()
        .map(|monitor| DisplayConfigLogicalMonitor {
            x: monitor.x,
            y: monitor.y,
            scale: monitor.scale,
            transform: monitor.transform,
            primary: monitor.primary,
            monitors: monitor
                .monitors
                .iter()
                .map(|physical| DisplayConfigMonitor {
                    connector: physical.connector.as_str(),
                    mode_id: physical.mode_id.as_str(),
                })
                .collect(),
        })
        .collect()
}

fn xrandr_outputs() -> Option<Vec<DisplayOutputStatus>> {
    let output = bounded_command_output("xrandr", &["--query"], probe_timeout()).ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(parse_xrandr_outputs(&stdout))
}

fn gdbus_call(args: &[&str]) -> Result<String, DisplayConfigError> {
    let mut full_args = vec![
        "call",
        "--session",
        "--dest",
        MUTTER_DISPLAY_CONFIG_DEST,
        "--object-path",
        MUTTER_DISPLAY_CONFIG_PATH,
        "--method",
    ];
    full_args.extend_from_slice(args);
    let output = match bounded_command_output("gdbus", &full_args, probe_timeout()) {
        Ok(output) => output,
        Err(BoundedCommandError::Missing) => return Err(DisplayConfigError::Missing),
        Err(BoundedCommandError::TimedOut | BoundedCommandError::Failed) => {
            return Err(DisplayConfigError::Failed(
                "desktop bridge could not be started".to_string(),
            ))
        }
    };
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let detail = String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Err(DisplayConfigError::Failed(if detail.is_empty() {
        "desktop bridge returned a failure without details".to_string()
    } else {
        detail
    }))
}

fn parse_apply_method(value: &str) -> Result<DisplayApplyMethod, &'static str> {
    match value.trim() {
        "verify" => Ok(DisplayApplyMethod::Verify),
        "temporary" => Ok(DisplayApplyMethod::Temporary),
        "persistent" => Ok(DisplayApplyMethod::Persistent),
        _ => Err("Display apply method must be verify, temporary, or persistent."),
    }
}

fn apply_method_value(method: DisplayApplyMethod) -> u32 {
    match method {
        DisplayApplyMethod::Verify => 0,
        DisplayApplyMethod::Temporary => 1,
        DisplayApplyMethod::Persistent => 2,
    }
}

fn apply_success_text(method: DisplayApplyMethod) -> &'static str {
    match method {
        DisplayApplyMethod::Verify => "Display configuration was verified by the compositor.",
        DisplayApplyMethod::Temporary => {
            "Display configuration was applied temporarily. Confirm it to keep the layout."
        }
        DisplayApplyMethod::Persistent => "Display configuration was saved.",
    }
}

fn validate_logical_monitors(monitors: &[LogicalMonitorRequest]) -> Result<(), String> {
    if monitors.is_empty() {
        return Err("At least one logical monitor is required.".to_string());
    }
    if monitors.len() > 8 {
        return Err("Display layout changes are limited to eight logical monitors.".to_string());
    }
    let primary_count = monitors.iter().filter(|monitor| monitor.primary).count();
    if primary_count != 1 {
        return Err("Exactly one logical monitor must be primary.".to_string());
    }
    let mut seen_connectors = std::collections::HashSet::new();
    for monitor in monitors {
        if !(-65535..=65535).contains(&monitor.x) || !(-65535..=65535).contains(&monitor.y) {
            return Err("Display positions must stay within compositor layout bounds.".to_string());
        }
        if !monitor.scale.is_finite() || monitor.scale < 1.0 || monitor.scale > 4.0 {
            return Err("Display scale must be between 1.0 and 4.0.".to_string());
        }
        if monitor.transform > 7 {
            return Err(
                "Display transform must be a Wayland transform value from 0 through 7.".to_string(),
            );
        }
        if monitor.monitors.is_empty() {
            return Err("Each logical monitor needs at least one physical monitor.".to_string());
        }
        if monitor.monitors.len() > 4 {
            return Err("Each logical monitor is limited to four mirrored outputs.".to_string());
        }
        for physical in &monitor.monitors {
            if !display_connector_is_safe(&physical.connector) {
                return Err("Display connector names must be safe desktop IDs.".to_string());
            }
            if !display_mode_id_is_safe(&physical.mode_id) {
                return Err("Display mode IDs must be safe compositor mode IDs.".to_string());
            }
            if !seen_connectors.insert(physical.connector.clone()) {
                return Err(
                    "A physical display can appear in only one logical monitor.".to_string()
                );
            }
        }
    }
    Ok(())
}

fn encode_logical_monitors(monitors: &[LogicalMonitorRequest]) -> String {
    let encoded = monitors
        .iter()
        .map(|monitor| {
            let physical = monitor
                .monitors
                .iter()
                .map(|physical| {
                    format!(
                        "('{}', '{}', {{}})",
                        escape_gvariant_string(&physical.connector),
                        escape_gvariant_string(&physical.mode_id)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "({}, {}, {}, uint32 {}, {}, [{}])",
                monitor.x,
                monitor.y,
                encode_display_scale(monitor.scale),
                monitor.transform,
                if monitor.primary { "true" } else { "false" },
                physical
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{encoded}]")
}

fn encode_display_scale(scale: f64) -> String {
    let mut text = format!("{scale:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.push('0');
    }
    text
}

fn parse_current_state_serial(reply: &str) -> Option<u32> {
    let trimmed = reply.trim().trim_start_matches('(').trim_start();
    let trimmed = trimmed
        .strip_prefix("uint32")
        .unwrap_or(trimmed)
        .trim_start();
    let mut digits = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if !digits.is_empty() {
            break;
        }
        if !ch.is_whitespace() {
            return None;
        }
    }
    digits.parse().ok()
}

fn parse_gdbus_bool(reply: &str) -> Option<bool> {
    if reply.contains("true") {
        Some(true)
    } else if reply.contains("false") {
        Some(false)
    } else {
        None
    }
}

fn display_connector_is_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn display_mode_id_is_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'@'))
}

fn escape_gvariant_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn apply_displays_response(
    status: StatusCode,
    ok: bool,
    text: &str,
    method: String,
    serial: u32,
) -> (StatusCode, ApplyDisplaysOutcome) {
    (
        status,
        ApplyDisplaysOutcome {
            ok,
            text: text.to_string(),
            method,
            serial,
        },
    )
}

fn parse_xrandr_outputs(stdout: &str) -> Vec<DisplayOutputStatus> {
    stdout
        .lines()
        .filter_map(parse_xrandr_output_line)
        .collect()
}

fn parse_xrandr_output_line(line: &str) -> Option<DisplayOutputStatus> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("Screen ") {
        return None;
    }

    let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
    let name = tokens.first()?.to_string();
    let connected = tokens.get(1).is_some_and(|token| *token == "connected");
    if !connected && tokens.get(1).is_none_or(|token| *token != "disconnected") {
        return None;
    }
    let primary = tokens.contains(&"primary");
    let geometry = tokens
        .iter()
        .find(|token| display_geometry_token(token))
        .copied();
    let (current_mode, position) = geometry.map(split_display_geometry).unwrap_or((None, None));
    let detail = display_output_detail(&name, connected, primary, current_mode.as_deref());

    Some(DisplayOutputStatus {
        name,
        connected,
        primary,
        current_mode,
        position,
        detail,
    })
}

fn display_geometry_token(token: &&str) -> bool {
    let parts = token.split('+').collect::<Vec<_>>();
    parts.len() >= 3
        && parts[0].contains('x')
        && parts[0]
            .split('x')
            .all(|value| !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()))
        && parts[1..]
            .iter()
            .all(|value| !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit()))
}

fn split_display_geometry(token: &str) -> (Option<String>, Option<String>) {
    let parts = token.split('+').collect::<Vec<_>>();
    let mode = parts.first().map(|value| (*value).to_string());
    let position = if parts.len() >= 3 {
        Some(format!("+{}+{}", parts[1], parts[2]))
    } else {
        None
    };
    (mode, position)
}

fn display_output_detail(
    name: &str,
    connected: bool,
    primary: bool,
    current_mode: Option<&str>,
) -> String {
    if !connected {
        return format!("{name} is disconnected.");
    }
    match (primary, current_mode) {
        (true, Some(mode)) => format!("{name} is the primary display at {mode}."),
        (false, Some(mode)) => format!("{name} is connected at {mode}."),
        (true, None) => format!("{name} is the primary display; current mode is not reported."),
        (false, None) => format!("{name} is connected; current mode is not reported."),
    }
}

fn displays_detail(
    wayland_display: Option<&str>,
    x11_display: Option<&str>,
    mutter_display_config_available: bool,
    xrandr_available: bool,
    output_count: usize,
) -> String {
    if mutter_display_config_available && output_count > 0 {
        return format!(
            "Display configuration is reachable; {output_count} display output(s) were reported."
        );
    }
    if mutter_display_config_available {
        return "Display configuration is reachable. Resolution and arrangement remain read-only until supported controls are available.".to_string();
    }
    if output_count > 0 {
        return format!(
            "{output_count} display output(s) were reported; the full display configuration can't be read for this session yet."
        );
    }
    if wayland_display.is_none() && x11_display.is_none() {
        return "No active Wayland or X11 display is detected for this session yet.".to_string();
    }
    if !xrandr_available {
        return "A desktop display is active, but its configuration can't be read right now."
            .to_string();
    }
    "A desktop display is active, but its monitor configuration can't be read for this session."
        .to_string()
}

fn env_optional(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_string(key: &str, fallback: &str) -> String {
    env_optional(key).unwrap_or_else(|| fallback.to_string())
}

fn executable_exists(binary: &str) -> bool {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .any(|path| {
            let candidate = path.join(binary);
            fs::metadata(candidate)
                .map(|metadata| metadata.is_file())
                .unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::{
        apply_method_value, display_connector_is_safe, display_mode_id_is_safe,
        display_output_detail, displays_detail, encode_logical_monitors, parse_apply_method,
        parse_current_state_serial, parse_gdbus_bool, parse_xrandr_output_line,
        parse_xrandr_outputs, split_display_geometry, validate_logical_monitors,
        DisplayApplyMethod, LogicalMonitorRequest, MonitorConfigRequest,
    };

    #[test]
    fn parses_xrandr_connected_outputs_without_inventing_state() {
        let outputs = parse_xrandr_outputs(
            "Screen 0: minimum 16 x 16, current 2560 x 1440, maximum 32767 x 32767\n\
             eDP-1 connected primary 2560x1440+0+0 (normal left inverted right x axis y axis) 344mm x 194mm\n\
             HDMI-1 disconnected (normal left inverted right x axis y axis)\n",
        );

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].name, "eDP-1");
        assert!(outputs[0].connected);
        assert!(outputs[0].primary);
        assert_eq!(outputs[0].current_mode.as_deref(), Some("2560x1440"));
        assert_eq!(outputs[0].position.as_deref(), Some("+0+0"));
        assert_eq!(outputs[1].name, "HDMI-1");
        assert!(!outputs[1].connected);
        assert_eq!(outputs[1].current_mode, None);
    }

    #[test]
    fn rejects_non_output_xrandr_lines() {
        assert!(parse_xrandr_output_line("Screen 0: current 1 x 1").is_none());
        assert!(parse_xrandr_output_line("2560x1440 60.00*+").is_none());
        assert!(parse_xrandr_output_line("").is_none());
    }

    #[test]
    fn display_copy_stays_truthful_about_query_limits() {
        assert_eq!(
            split_display_geometry("1920x1080+10+20"),
            (Some("1920x1080".to_string()), Some("+10+20".to_string()))
        );
        assert!(
            display_output_detail("eDP-1", true, true, Some("2560x1440"))
                .contains("primary display")
        );
        assert!(display_output_detail("HDMI-1", false, false, None).contains("disconnected"));
        assert!(displays_detail(None, None, false, false, 0).contains("No active Wayland"));
        assert!(displays_detail(Some("wayland-0"), None, true, false, 0)
            .contains("Display configuration is reachable"));
        assert!(displays_detail(None, Some(":0"), false, true, 2).contains("2 display output"));
    }

    #[test]
    fn display_apply_request_encodes_mutter_payload() {
        let monitors = vec![LogicalMonitorRequest {
            x: 0,
            y: 0,
            scale: 1.25,
            transform: 0,
            primary: true,
            monitors: vec![MonitorConfigRequest {
                connector: "eDP-1".to_string(),
                mode_id: "2560x1440@60.000".to_string(),
            }],
        }];

        assert_eq!(
            parse_apply_method("temporary"),
            Ok(DisplayApplyMethod::Temporary)
        );
        assert_eq!(apply_method_value(DisplayApplyMethod::Temporary), 1);
        assert_eq!(
            encode_logical_monitors(&monitors),
            "[(0, 0, 1.25, uint32 0, true, [('eDP-1', '2560x1440@60.000', {})])]"
        );
        assert_eq!(
            parse_current_state_serial("(uint32 42, [], [], {})"),
            Some(42)
        );
        assert_eq!(parse_current_state_serial("(42, [], [], {})"), Some(42));
        assert_eq!(parse_gdbus_bool("(<true>,)"), Some(true));
        assert_eq!(parse_gdbus_bool("(<false>,)"), Some(false));
    }

    #[test]
    fn display_apply_request_rejects_unsafe_layouts() {
        let valid = vec![LogicalMonitorRequest {
            x: 0,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: true,
            monitors: vec![MonitorConfigRequest {
                connector: "HDMI-A-1".to_string(),
                mode_id: "1920x1080@60.000".to_string(),
            }],
        }];
        assert!(validate_logical_monitors(&valid).is_ok());
        assert!(display_connector_is_safe("DP-1"));
        assert!(!display_connector_is_safe("DP/1"));
        assert!(display_mode_id_is_safe("1920x1080@60.000"));
        assert!(!display_mode_id_is_safe("1920x1080;rm"));

        let mut no_primary = valid.clone();
        no_primary[0].primary = false;
        assert!(validate_logical_monitors(&no_primary).is_err());

        let mut bad_scale = valid.clone();
        bad_scale[0].scale = 0.5;
        assert!(validate_logical_monitors(&bad_scale).is_err());

        let mut duplicate = valid.clone();
        duplicate.push(LogicalMonitorRequest {
            x: 1920,
            y: 0,
            scale: 1.0,
            transform: 0,
            primary: false,
            monitors: vec![MonitorConfigRequest {
                connector: "HDMI-A-1".to_string(),
                mode_id: "1920x1080@60.000".to_string(),
            }],
        });
        assert!(validate_logical_monitors(&duplicate).is_err());
    }
}
