//! Read-only display and compositor status for Settings.
//!
//! Mutable resolution, scale, mirroring, and arrangement changes need a
//! policy-aware desktop display route. Until that exists, the core reports the
//! actual session and query capability instead of letting the GUI fake display
//! controls.

use std::{env, fs, process::Command};

use axum::Json;
use serde::Serialize;

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

pub async fn displays_status() -> Json<DisplaysStatus> {
    Json(build_displays_status())
}

fn build_displays_status() -> DisplaysStatus {
    let session_type = env_string("XDG_SESSION_TYPE", "unconfigured");
    let desktop = env_string("DESKTOP_SESSION", "unconfigured");
    let current_desktop = env_string("XDG_CURRENT_DESKTOP", "unconfigured");
    let wayland_display = env_optional("WAYLAND_DISPLAY");
    let x11_display = env_optional("DISPLAY");
    let gdbus_available = executable_exists("gdbus");
    let mutter_display_config_available =
        gdbus_available && mutter_display_config_reachable().unwrap_or(false);
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
        xrandr_available,
        outputs,
        detail,
    }
}

fn mutter_display_config_reachable() -> Option<bool> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Mutter.DisplayConfig",
            "--object-path",
            "/org/gnome/Mutter/DisplayConfig",
            "--method",
            "org.gnome.Mutter.DisplayConfig.GetCurrentState",
        ])
        .output()
        .ok()?;
    Some(output.status.success())
}

fn xrandr_outputs() -> Option<Vec<DisplayOutputStatus>> {
    let output = Command::new("xrandr").arg("--query").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(parse_xrandr_outputs(&stdout))
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
            "{output_count} display output(s) were reported; full display configuration is not reachable from this runtime."
        );
    }
    if wayland_display.is_none() && x11_display.is_none() {
        return "No active Wayland or X11 display handle is visible to the core runtime."
            .to_string();
    }
    if !xrandr_available {
        return "A desktop display handle is visible, but display configuration is not reachable."
            .to_string();
    }
    "A desktop display handle is visible, but monitor configuration could not be queried from this runtime.".to_string()
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
        display_output_detail, displays_detail, parse_xrandr_output_line, parse_xrandr_outputs,
        split_display_geometry,
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
}
