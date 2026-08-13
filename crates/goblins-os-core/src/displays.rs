//! Display and compositor status for Settings.
//!
//! Mutable resolution, scale, mirroring, and arrangement changes go through
//! Mutter's stable DisplayConfig D-Bus API. The core owns the allowlist and live
//! serial checks so Settings never writes arbitrary display state or reports a
//! successful apply when the compositor gate is absent.

use std::{
    env, fs,
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::bounded::{bounded_session_command_output, probe_timeout, BoundedCommandError};
use crate::session_bridge::{
    DisplayConfigLogicalMonitor, DisplayConfigMonitor, SessionBridgeResult,
};

const MUTTER_DISPLAY_CONFIG_DEST: &str = "org.gnome.Mutter.DisplayConfig";
const MUTTER_DISPLAY_CONFIG_PATH: &str = "/org/gnome/Mutter/DisplayConfig";
const MUTTER_DISPLAY_CONFIG_GET_CURRENT_STATE: &str =
    "org.gnome.Mutter.DisplayConfig.GetCurrentState";
const MUTTER_DISPLAY_CONFIG_APPLY_MONITORS: &str =
    "org.gnome.Mutter.DisplayConfig.ApplyMonitorsConfig";
/// Bound for the ApplyMonitorsConfig write. A real modeset across several
/// monitors can take far longer than a status read, so the apply call gets its
/// own wider bound while reads stay on `probe_timeout()`.
const APPLY_MONITORS_TIMEOUT: Duration = Duration::from_secs(30);

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
    logical_monitors: Vec<DisplayLogicalMonitorStatus>,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DisplayOutputStatus {
    name: String,
    connected: bool,
    primary: bool,
    current_mode: Option<String>,
    current_mode_id: Option<String>,
    position: Option<String>,
    modes: Vec<DisplayModeStatus>,
    detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DisplayModeStatus {
    id: String,
    label: String,
    width: u32,
    height: u32,
    refresh_hz: f64,
    preferred: bool,
    current: bool,
    supported_scales: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DisplayLogicalMonitorStatus {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<MonitorConfigRequest>,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
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

pub async fn displays_status() -> (StatusCode, Json<DisplaysStatus>) {
    match tokio::task::spawn_blocking(build_displays_status).await {
        Ok(status) => (StatusCode::OK, Json(status)),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(displays_worker_failed_status()),
        ),
    }
}

pub async fn apply_displays(
    Json(request): Json<ApplyDisplaysRequest>,
) -> (StatusCode, Json<ApplyDisplaysOutcome>) {
    let method = request.method.clone();
    let serial = request.serial;
    let Some(permit) = display_mutation_permit() else {
        let (status, outcome) = apply_displays_response(
            StatusCode::CONFLICT,
            false,
            "Another display configuration change is already in progress.",
            method,
            serial,
        );
        return (status, Json(outcome));
    };

    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        apply_displays_outcome(request)
    })
    .await
    {
        Ok((status, outcome)) => (status, Json(outcome)),
        Err(_) => {
            let (status, outcome) = apply_displays_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                false,
                "The display configuration worker stopped unexpectedly. No change is reported.",
                method,
                serial,
            );
            (status, Json(outcome))
        }
    }
}

fn display_mutation_permit() -> Option<OwnedSemaphorePermit> {
    static LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(LIMITER.get_or_init(|| Arc::new(Semaphore::new(1))))
        .try_acquire_owned()
        .ok()
}

fn displays_worker_failed_status() -> DisplaysStatus {
    DisplaysStatus {
        source: "goblins-os-core",
        session_type: "unavailable".to_string(),
        desktop: "unavailable".to_string(),
        current_desktop: "unavailable".to_string(),
        wayland_display: None,
        x11_display: None,
        gdbus_available: false,
        mutter_display_config_available: false,
        mutter_display_apply_allowed: false,
        display_config_serial: None,
        xrandr_available: false,
        outputs: Vec::new(),
        logical_monitors: Vec::new(),
        detail: "Display status could not be inspected because its worker stopped unexpectedly."
            .to_string(),
    }
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
    let parsed_display_config = current_state
        .as_deref()
        .and_then(parse_display_config_state);
    let display_config_serial = parsed_display_config
        .as_ref()
        .map(|state| state.serial)
        .or_else(|| {
            current_state
                .as_deref()
                .and_then(parse_current_state_serial)
        });
    let mutter_display_apply_allowed =
        gdbus_available && mutter_display_config_apply_allowed().unwrap_or(false);
    let xrandr_available = executable_exists("xrandr");
    let outputs = parsed_display_config
        .as_ref()
        .map(|state| state.outputs.clone())
        .filter(|outputs| !outputs.is_empty())
        .unwrap_or_else(|| {
            if xrandr_available {
                xrandr_outputs().unwrap_or_default()
            } else {
                Vec::new()
            }
        });
    let logical_monitors = parsed_display_config
        .map(|state| state.logical_monitors)
        .unwrap_or_default();
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
        logical_monitors,
        detail,
    }
}

fn mutter_current_state() -> Result<String, DisplayConfigError> {
    match crate::session_bridge::display_config_get_current_state() {
        SessionBridgeResult::Success(stdout) => return Ok(stdout),
        SessionBridgeResult::Failed(detail) => return Err(DisplayConfigError::Failed(detail)),
        SessionBridgeResult::Unavailable => {}
    }
    gdbus_call(&[MUTTER_DISPLAY_CONFIG_GET_CURRENT_STATE], probe_timeout())
}

fn mutter_display_config_apply_allowed() -> Result<bool, DisplayConfigError> {
    let reply = match crate::session_bridge::display_config_get_apply_allowed() {
        SessionBridgeResult::Success(stdout) => stdout,
        SessionBridgeResult::Failed(detail) => return Err(DisplayConfigError::Failed(detail)),
        SessionBridgeResult::Unavailable => gdbus_call(
            &[
                "org.freedesktop.DBus.Properties.Get",
                MUTTER_DISPLAY_CONFIG_DEST,
                "ApplyMonitorsConfigAllowed",
            ],
            probe_timeout(),
        )?,
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
    let current_config = match mutter_current_state().and_then(|state| {
        parse_display_config_state(&state).ok_or_else(|| {
            DisplayConfigError::Failed(
                "compositor did not report a usable display configuration".to_string(),
            )
        })
    }) {
        Ok(config) if config.serial == request.serial => config,
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
    };
    if let Err(message) =
        validate_layout_against_current_state(&request.logical_monitors, &current_config)
    {
        return apply_displays_response(
            StatusCode::BAD_REQUEST,
            false,
            &message,
            request.method,
            request.serial,
        );
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
        Ok(_) => {
            // Mutter advances its serial when a configuration is applied. Return
            // that generation so the confirmation flow can keep or revert the
            // preview without a stale follow-up request.
            let applied_serial = mutter_current_state()
                .ok()
                .and_then(|state| parse_current_state_serial(&state))
                .unwrap_or(request.serial);
            apply_displays_response(
                StatusCode::OK,
                true,
                apply_success_text(method),
                request.method,
                applied_serial,
            )
        }
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
    gdbus_call(
        &[
            MUTTER_DISPLAY_CONFIG_APPLY_MONITORS,
            serial_text,
            method_text,
            logical_monitors_text,
            "{}",
        ],
        APPLY_MONITORS_TIMEOUT,
    )
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

#[derive(Clone, Debug, PartialEq)]
struct ParsedDisplayConfigState {
    serial: u32,
    physical: Vec<ParsedPhysicalMonitor>,
    outputs: Vec<DisplayOutputStatus>,
    logical_monitors: Vec<DisplayLogicalMonitorStatus>,
}

#[derive(Clone, Debug, PartialEq)]
struct ParsedPhysicalMonitor {
    connector: String,
    modes: Vec<DisplayModeStatus>,
}

/// Parse the stable `GetCurrentState` GVariant into the exact mode IDs and
/// logical layout needed for an ApplyMonitorsConfig round trip. This parser is
/// deliberately structural rather than regex-based: commas and nested arrays
/// inside property dictionaries cannot shift the fields Settings relies on.
fn parse_display_config_state(reply: &str) -> Option<ParsedDisplayConfigState> {
    let state = gvariant_tuple(reply)?;
    if state.len() < 3 {
        return None;
    }
    let serial = parse_gvariant_u32(state[0])?;
    let physical = parse_physical_monitors(state[1]);
    let logical_monitors = parse_logical_monitors(state[2], &physical);
    if physical.is_empty() || logical_monitors.is_empty() {
        return None;
    }
    let outputs = display_outputs_from_config(&physical, &logical_monitors);
    Some(ParsedDisplayConfigState {
        serial,
        physical,
        outputs,
        logical_monitors,
    })
}

fn parse_physical_monitors(value: &str) -> Vec<ParsedPhysicalMonitor> {
    gvariant_array(value)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| {
            let fields = gvariant_tuple(entry)?;
            if fields.len() < 2 {
                return None;
            }
            let identity = gvariant_tuple(fields[0])?;
            let connector = identity.first().and_then(|value| gvariant_string(value))?;
            if !display_connector_is_safe(&connector) {
                return None;
            }
            let modes = gvariant_array(fields[1])?
                .into_iter()
                .filter_map(parse_display_mode)
                .collect::<Vec<_>>();
            Some(ParsedPhysicalMonitor { connector, modes })
        })
        .collect()
}

fn parse_display_mode(value: &str) -> Option<DisplayModeStatus> {
    let fields = gvariant_tuple(value)?;
    if fields.len() < 7 {
        return None;
    }
    let id = gvariant_string(fields[0])?;
    if !display_mode_id_is_safe(&id) {
        return None;
    }
    let width = parse_gvariant_u32(fields[1])?;
    let height = parse_gvariant_u32(fields[2])?;
    let refresh_hz = parse_gvariant_f64(fields[3])?;
    if width == 0 || height == 0 || !refresh_hz.is_finite() || refresh_hz <= 0.0 {
        return None;
    }
    let supported_scales = gvariant_array(fields[5])
        .unwrap_or_default()
        .into_iter()
        .filter_map(parse_gvariant_f64)
        .filter(|scale| scale.is_finite() && (1.0..=4.0).contains(scale))
        .collect::<Vec<_>>();
    let properties = fields[6];
    let current = gvariant_property_bool(properties, "is-current").unwrap_or(false);
    let preferred = gvariant_property_bool(properties, "is-preferred").unwrap_or(false);
    Some(DisplayModeStatus {
        label: display_mode_label(width, height, refresh_hz),
        id,
        width,
        height,
        refresh_hz,
        preferred,
        current,
        supported_scales,
    })
}

fn parse_logical_monitors(
    value: &str,
    physical: &[ParsedPhysicalMonitor],
) -> Vec<DisplayLogicalMonitorStatus> {
    gvariant_array(value)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| {
            let fields = gvariant_tuple(entry)?;
            if fields.len() < 6 {
                return None;
            }
            let x = parse_gvariant_i32(fields[0])?;
            let y = parse_gvariant_i32(fields[1])?;
            let scale = parse_gvariant_f64(fields[2])?;
            let transform = parse_gvariant_u32(fields[3])?;
            let primary = parse_gvariant_bool(fields[4])?;
            let monitors = gvariant_array(fields[5])?
                .into_iter()
                .filter_map(|identity| {
                    let fields = gvariant_tuple(identity)?;
                    let connector = fields.first().and_then(|value| gvariant_string(value))?;
                    let monitor = physical
                        .iter()
                        .find(|monitor| monitor.connector == connector)?;
                    let mode_id = monitor
                        .modes
                        .iter()
                        .find(|mode| mode.current)
                        .or_else(|| monitor.modes.iter().find(|mode| mode.preferred))
                        .or_else(|| monitor.modes.first())?
                        .id
                        .clone();
                    Some(MonitorConfigRequest { connector, mode_id })
                })
                .collect::<Vec<_>>();
            if monitors.is_empty()
                || !scale.is_finite()
                || !(1.0..=4.0).contains(&scale)
                || transform > 7
            {
                return None;
            }
            Some(DisplayLogicalMonitorStatus {
                x,
                y,
                scale,
                transform,
                primary,
                monitors,
            })
        })
        .collect()
}

fn display_outputs_from_config(
    physical: &[ParsedPhysicalMonitor],
    logical: &[DisplayLogicalMonitorStatus],
) -> Vec<DisplayOutputStatus> {
    physical
        .iter()
        .map(|monitor| {
            let layout = logical.iter().find(|logical| {
                logical
                    .monitors
                    .iter()
                    .any(|candidate| candidate.connector == monitor.connector)
            });
            let current = monitor.modes.iter().find(|mode| mode.current).or_else(|| {
                layout.and_then(|logical| {
                    let mode_id = logical
                        .monitors
                        .iter()
                        .find(|candidate| candidate.connector == monitor.connector)
                        .map(|candidate| candidate.mode_id.as_str())?;
                    monitor.modes.iter().find(|mode| mode.id == mode_id)
                })
            });
            let primary = layout.is_some_and(|logical| logical.primary);
            let current_mode = current.map(|mode| mode.label.clone());
            let current_mode_id = current.map(|mode| mode.id.clone());
            let position = layout.map(|logical| format!("{:+}{:+}", logical.x, logical.y));
            DisplayOutputStatus {
                name: monitor.connector.clone(),
                connected: true,
                primary,
                current_mode: current_mode.clone(),
                current_mode_id,
                position,
                modes: monitor.modes.clone(),
                detail: display_output_detail(
                    &monitor.connector,
                    true,
                    primary,
                    current_mode.as_deref(),
                ),
            }
        })
        .collect()
}

fn display_mode_label(width: u32, height: u32, refresh_hz: f64) -> String {
    let refresh = if (refresh_hz.round() - refresh_hz).abs() < 0.01 {
        format!("{refresh_hz:.0}")
    } else {
        format!("{refresh_hz:.2}")
    };
    format!("{width} × {height} · {refresh} Hz")
}

fn gvariant_tuple(value: &str) -> Option<Vec<&str>> {
    gvariant_container(value, '(', ')')
}

fn gvariant_array(value: &str) -> Option<Vec<&str>> {
    gvariant_container(value, '[', ']')
}

fn gvariant_container(value: &str, open: char, close: char) -> Option<Vec<&str>> {
    let value = value.trim();
    if !value.starts_with(open) || !value.ends_with(close) {
        return None;
    }
    split_gvariant_top_level(&value[open.len_utf8()..value.len() - close.len_utf8()])
}

fn split_gvariant_top_level(value: &str) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut round = 0_u16;
    let mut square = 0_u16;
    let mut curly = 0_u16;
    let mut angle = 0_u16;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '\'' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '\'' => quoted = true,
            '(' => round = round.checked_add(1)?,
            ')' => round = round.checked_sub(1)?,
            '[' => square = square.checked_add(1)?,
            ']' => square = square.checked_sub(1)?,
            '{' => curly = curly.checked_add(1)?,
            '}' => curly = curly.checked_sub(1)?,
            '<' => angle = angle.checked_add(1)?,
            '>' => angle = angle.checked_sub(1)?,
            ',' if round == 0 && square == 0 && curly == 0 && angle == 0 => {
                let part = value[start..index].trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted || round != 0 || square != 0 || curly != 0 || angle != 0 {
        return None;
    }
    let tail = value[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    Some(parts)
}

fn gvariant_string(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut output = String::new();
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            output.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            output.push(ch);
        }
    }
    (!escaped).then_some(output)
}

fn parse_gvariant_u32(value: &str) -> Option<u32> {
    gvariant_scalar(value).parse().ok()
}

fn parse_gvariant_i32(value: &str) -> Option<i32> {
    gvariant_scalar(value).parse().ok()
}

fn parse_gvariant_f64(value: &str) -> Option<f64> {
    gvariant_scalar(value)
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn parse_gvariant_bool(value: &str) -> Option<bool> {
    match gvariant_scalar(value) {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn gvariant_scalar(value: &str) -> &str {
    let mut value = value.trim();
    if let Some(inner) = value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
    {
        value = inner.trim();
    }
    for prefix in ["uint32 ", "int32 ", "double "] {
        if let Some(inner) = value.strip_prefix(prefix) {
            return inner.trim();
        }
    }
    value
}

fn gvariant_property_bool(properties: &str, key: &str) -> Option<bool> {
    let needle = format!("'{key}'");
    let after_key = properties.split_once(&needle)?.1;
    let value = after_key.split_once(':')?.1.trim_start();
    if value.starts_with("<true>") || value.starts_with("true") {
        Some(true)
    } else if value.starts_with("<false>") || value.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn xrandr_outputs() -> Option<Vec<DisplayOutputStatus>> {
    let output = bounded_session_command_output("xrandr", &["--query"], probe_timeout()).ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(parse_xrandr_outputs(&stdout))
}

fn gdbus_call(args: &[&str], timeout: Duration) -> Result<String, DisplayConfigError> {
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
    let output = match bounded_session_command_output("gdbus", &full_args, timeout) {
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
        DisplayApplyMethod::Persistent => {
            "Display confirmation was handed to the desktop. Choose Keep Changes in the system dialog to save it."
        }
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

/// Bind every mutable display identifier to the live compositor snapshot whose
/// serial the caller presented. Safe-looking connector and mode strings are not
/// sufficient authorization: they must be exact currently connected IDs, and
/// the requested scale must be one Mutter reported for that exact mode.
fn validate_layout_against_current_state(
    monitors: &[LogicalMonitorRequest],
    current: &ParsedDisplayConfigState,
) -> Result<(), String> {
    for logical in monitors {
        let mut mirrored_mode: Option<(u32, u32, f64)> = None;
        for requested in &logical.monitors {
            let physical = current
                .physical
                .iter()
                .find(|candidate| candidate.connector == requested.connector)
                .ok_or_else(|| {
                    "Display layout contains a connector that is not currently connected."
                        .to_string()
                })?;
            let mode = physical
                .modes
                .iter()
                .find(|candidate| candidate.id == requested.mode_id)
                .ok_or_else(|| {
                    "Display layout contains a mode that the connected display did not report."
                        .to_string()
                })?;
            if !mode
                .supported_scales
                .iter()
                .any(|scale| (*scale - logical.scale).abs() < 0.001)
            {
                return Err(
                    "Display scale is not supported by the selected display mode.".to_string(),
                );
            }
            if let Some((width, height, refresh_hz)) = mirrored_mode {
                if mode.width != width
                    || mode.height != height
                    || (mode.refresh_hz - refresh_hz).abs() >= 0.1
                {
                    return Err(
                        "Mirrored displays must use matching resolution and refresh modes."
                            .to_string(),
                    );
                }
            } else {
                mirrored_mode = Some((mode.width, mode.height, mode.refresh_hz));
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
        current_mode_id: None,
        position,
        modes: Vec::new(),
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
        display_mutation_permit, display_output_detail, displays_detail, encode_logical_monitors,
        parse_apply_method, parse_current_state_serial, parse_display_config_state,
        parse_gdbus_bool, parse_xrandr_output_line, parse_xrandr_outputs, split_display_geometry,
        validate_layout_against_current_state, validate_logical_monitors, DisplayApplyMethod,
        LogicalMonitorRequest, MonitorConfigRequest,
    };

    #[test]
    fn display_mutations_have_single_request_admission() {
        let first = display_mutation_permit().expect("first display mutation is admitted");
        assert!(display_mutation_permit().is_none());
        drop(first);
        assert!(display_mutation_permit().is_some());
    }

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
    fn parses_display_config_layout_and_exact_mode_ids() {
        let state = "(uint32 42, \
            [(('eDP-1', 'Vendor', 'Panel', 'serial'), \
              [('2560x1440@59.951', 2560, 1440, 59.951, 1.0, [1.0, 1.25, 2.0], \
                {'is-current': <true>, 'is-preferred': <true>}), \
               ('1920x1080@60.000', 1920, 1080, 60.0, 1.0, [1.0, 2.0], {})], \
              {'is-builtin': <true>}), \
             (('HDMI-1', 'Vendor', 'Screen', 'serial2'), \
              [('1920x1080@60.000', 1920, 1080, 60.0, 1.0, [1.0], \
                {'is-current': <true>, 'is-preferred': <true>})], {})], \
            [(0, 0, 1.25, uint32 0, true, [('eDP-1', 'Vendor', 'Panel', 'serial')], {}), \
             (2048, 0, 1.0, uint32 1, false, [('HDMI-1', 'Vendor', 'Screen', 'serial2')], {})], \
            {'layout-mode': <uint32 1>})";

        let parsed = parse_display_config_state(state).expect("DisplayConfig state");
        assert_eq!(parsed.serial, 42);
        assert_eq!(parsed.outputs.len(), 2);
        assert_eq!(
            parsed.outputs[0].current_mode_id.as_deref(),
            Some("2560x1440@59.951")
        );
        assert_eq!(parsed.outputs[0].position.as_deref(), Some("+0+0"));
        assert!(parsed.outputs[0].primary);
        assert_eq!(parsed.outputs[0].modes.len(), 2);
        assert_eq!(parsed.logical_monitors.len(), 2);
        assert_eq!(parsed.logical_monitors[0].scale, 1.25);
        assert_eq!(parsed.logical_monitors[1].transform, 1);
        assert_eq!(
            parsed.logical_monitors[1].monitors[0].mode_id,
            "1920x1080@60.000"
        );
    }

    #[test]
    fn malformed_display_config_state_fails_closed() {
        assert!(parse_display_config_state("(uint32 4, [broken], [], {})").is_none());
        assert!(parse_display_config_state("not a tuple").is_none());
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

    #[test]
    fn display_apply_ids_and_scales_must_exist_in_live_mutter_state() {
        let state = "(uint32 9, \
            [(('eDP-1', 'Vendor', 'Panel', 'serial'), \
              [('2560x1440@60.000', 2560, 1440, 60.0, 1.0, [1.0, 2.0], \
                {'is-current': <true>, 'is-preferred': <true>})], {})], \
            [(0, 0, 1.0, uint32 0, true, \
              [('eDP-1', 'Vendor', 'Panel', 'serial')], {})], {})";
        let current = parse_display_config_state(state).expect("DisplayConfig state");
        let valid = vec![LogicalMonitorRequest {
            x: 0,
            y: 0,
            scale: 2.0,
            transform: 0,
            primary: true,
            monitors: vec![MonitorConfigRequest {
                connector: "eDP-1".to_string(),
                mode_id: "2560x1440@60.000".to_string(),
            }],
        }];
        assert!(validate_layout_against_current_state(&valid, &current).is_ok());

        let mut unknown_connector = valid.clone();
        unknown_connector[0].monitors[0].connector = "DP-404".to_string();
        assert!(validate_layout_against_current_state(&unknown_connector, &current).is_err());

        let mut unknown_mode = valid.clone();
        unknown_mode[0].monitors[0].mode_id = "9999x9999@99.000".to_string();
        assert!(validate_layout_against_current_state(&unknown_mode, &current).is_err());

        let mut unsupported_scale = valid;
        unsupported_scale[0].scale = 1.25;
        assert!(validate_layout_against_current_state(&unsupported_scale, &current).is_err());
    }
}
