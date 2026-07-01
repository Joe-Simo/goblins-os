use std::{
    env, fs,
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};

const DEFAULT_SOCKET: &str = "/run/goblins-os-session/session-bridge.sock";
const SOCKET_GROUP: &str = "goblins-session-bridge";
const MAX_REQUEST_BYTES: usize = 64 * 1024;

const KEYBOARD_SCHEMA: &str = "org.gnome.desktop.peripherals.keyboard";
const MOUSE_SCHEMA: &str = "org.gnome.desktop.peripherals.mouse";
const TOUCHPAD_SCHEMA: &str = "org.gnome.desktop.peripherals.touchpad";
const INPUT_SOURCES_SCHEMA: &str = "org.gnome.desktop.input-sources";
const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
const A11Y_APPS_SCHEMA: &str = "org.gnome.desktop.a11y.applications";
const A11Y_INTERFACE_SCHEMA: &str = "org.gnome.desktop.a11y.interface";
const A11Y_KEYBOARD_SCHEMA: &str = "org.gnome.desktop.a11y.keyboard";
const A11Y_MAGNIFIER_SCHEMA: &str = "org.gnome.desktop.a11y.magnifier";
const A11Y_MOUSE_SCHEMA: &str = "org.gnome.desktop.a11y.mouse";
const COLOR_SCHEMA: &str = "org.gnome.settings-daemon.plugins.color";
const FOCUS_SCHEMA: &str = "org.goblins.os.focus";
const NOTIFICATIONS_SCHEMA: &str = "org.gnome.desktop.notifications";
const NOTIFICATION_APPLICATION_SCHEMA: &str = "org.gnome.desktop.notifications.application";
const NOTIFICATION_APPLICATION_BASE_PATH: &str = "/org/gnome/desktop/notifications/application/";
const WM_SCHEMA: &str = "org.goblins.shell.extensions.wm";
const PERMISSION_STORE_DEST: &str = "org.freedesktop.impl.portal.PermissionStore";
const PERMISSION_STORE_PATH: &str = "/org/freedesktop/impl/portal/PermissionStore";
const PERMISSION_STORE_DELETE_PERMISSION: &str =
    "org.freedesktop.impl.portal.PermissionStore.DeletePermission";

const KEYBOARD_KEYS: &[&str] = &[
    "repeat",
    "delay",
    "repeat-interval",
    "remember-numlock-state",
];
const MOUSE_KEYS: &[&str] = &[
    "speed",
    "natural-scroll",
    "left-handed",
    "middle-click-emulation",
];
const TOUCHPAD_KEYS: &[&str] = &[
    "speed",
    "tap-to-click",
    "natural-scroll",
    "two-finger-scrolling-enabled",
    "disable-while-typing",
];
const INPUT_SOURCE_KEYS: &[&str] = &["sources", "current", "xkb-options"];
const INTERFACE_KEYS: &[&str] = &["enable-animations", "text-scaling-factor"];
const A11Y_APPS_KEYS: &[&str] = &[
    "screen-reader-enabled",
    "screen-keyboard-enabled",
    "screen-magnifier-enabled",
];
const A11Y_INTERFACE_KEYS: &[&str] = &["high-contrast"];
const A11Y_KEYBOARD_KEYS: &[&str] = &[
    "stickykeys-enable",
    "slowkeys-enable",
    "bouncekeys-enable",
    "mousekeys-enable",
];
const A11Y_MAGNIFIER_KEYS: &[&str] = &["mag-factor", "lens-mode"];
const A11Y_MOUSE_KEYS: &[&str] = &["dwell-click-enabled"];
const COLOR_KEYS: &[&str] = &[
    "night-light-enabled",
    "night-light-schedule-automatic",
    "night-light-temperature",
];
const FOCUS_KEYS: &[&str] = &[
    "active-mode",
    "modes",
    "schedules",
    "armed-by-schedule",
    "restore-banners",
    "restore-apps",
];
const NOTIFICATION_KEYS: &[&str] = &[
    "show-banners",
    "show-in-lock-screen",
    "application-children",
];
const NOTIFICATION_APPLICATION_KEYS: &[&str] = &[
    "application-id",
    "enable",
    "show-banners",
    "enable-sound-alerts",
    "show-in-lock-screen",
    "details-in-lock-screen",
    "force-expanded",
];
const WM_KEYS: &[&str] = &[
    "mission-control",
    "app-expose",
    "window-switcher",
    "window-hud",
    "snap-left",
    "snap-right",
    "snap-top-left",
    "snap-top-right",
    "snap-bottom-left",
    "snap-bottom-right",
    "restore-window",
    "center-window",
    "space-left",
    "space-right",
];

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum BridgeRequest {
    Ping,
    GSettings {
        args: Vec<String>,
    },
    OpenPreview {
        path: String,
        kind: String,
    },
    PermissionStoreDelete {
        table: String,
        id: String,
        app: String,
    },
}

#[derive(Serialize)]
struct BridgeResponse {
    ok: bool,
    stdout: String,
    detail: String,
}

fn main() {
    if env::args().any(|arg| arg == "--self-test") {
        match self_test() {
            Ok(()) => {
                println!("goblins-os-session-bridge self-test passed");
                return;
            }
            Err(error) => {
                eprintln!("goblins-os-session-bridge self-test failed: {error}");
                std::process::exit(1);
            }
        }
    }

    if let Err(error) = run_server() {
        eprintln!("goblins-os-session-bridge: {error}");
        std::process::exit(1);
    }
}

fn run_server() -> Result<(), String> {
    let socket = socket_path();
    prepare_socket_parent(&socket)?;
    remove_stale_socket(&socket)?;
    let listener = UnixListener::bind(&socket)
        .map_err(|error| format!("could not bind {}: {error}", socket.display()))?;
    set_group_mode(&socket, 0o660)?;

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let response = handle_stream(&mut stream);
                let _ = write_response(&mut stream, &response);
            }
            Err(error) => eprintln!("goblins-os-session-bridge: connection failed: {error}"),
        }
    }
    Ok(())
}

fn handle_stream(stream: &mut UnixStream) -> BridgeResponse {
    let mut body = String::new();
    if let Err(error) = stream
        .take(MAX_REQUEST_BYTES as u64)
        .read_to_string(&mut body)
    {
        return failure(format!("could not read request: {error}"));
    }
    let request = match serde_json::from_str::<BridgeRequest>(&body) {
        Ok(request) => request,
        Err(error) => return failure(format!("could not decode request: {error}")),
    };
    handle_request(request)
}

fn handle_request(request: BridgeRequest) -> BridgeResponse {
    match request {
        BridgeRequest::Ping => success("pong".to_string()),
        BridgeRequest::GSettings { args } => gsettings_response(args),
        BridgeRequest::OpenPreview { path, kind } => open_preview_response(&path, &kind),
        BridgeRequest::PermissionStoreDelete { table, id, app } => {
            permission_store_delete_response(&table, &id, &app)
        }
    }
}

fn gsettings_response(args: Vec<String>) -> BridgeResponse {
    if let Err(error) = validate_gsettings_args(&args) {
        return failure(error);
    }
    match Command::new("gsettings")
        .args(&args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            success(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => failure(command_error_detail(&output.stderr, &output.stdout)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            failure("gsettings is unavailable in this desktop session.")
        }
        Err(_) => failure("gsettings could not run in this desktop session."),
    }
}

fn open_preview_response(path: &str, kind: &str) -> BridgeResponse {
    let path = PathBuf::from(path.trim());
    if let Err(error) = validate_preview_open(&path, kind) {
        return failure(error);
    }
    match Command::new("xdg-open")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => success("opened".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            failure("xdg-open is unavailable in this desktop session.")
        }
        Err(_) => failure("the desktop session could not open that preview file."),
    }
}

fn permission_store_delete_response(table: &str, id: &str, app: &str) -> BridgeResponse {
    if let Err(error) = validate_permission_store_delete(table, id, app) {
        return failure(error);
    }
    match Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            PERMISSION_STORE_DEST,
            "--object-path",
            PERMISSION_STORE_PATH,
            "--method",
            PERMISSION_STORE_DELETE_PERMISSION,
            table,
            id,
            app,
        ])
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) if output.status.success() => {
            success(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => failure(command_error_detail(&output.stderr, &output.stdout)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            failure("gdbus is unavailable in this desktop session.")
        }
        Err(_) => failure("the desktop session could not update app permissions."),
    }
}

fn validate_gsettings_args(args: &[String]) -> Result<(), String> {
    match args {
        [command] if command == "list-schemas" => Ok(()),
        [command, schema] if command == "list-keys" => validate_list_keys_schema(schema),
        [command, schema, key] if command == "get" => validate_schema_key(schema, key),
        [command, schema, key] if command == "reset" => {
            let (base_schema, path) = validate_schema_arg(schema)?;
            if !path.is_empty() {
                return Err("gsettings reset is not allowed for path-scoped schemas.".to_string());
            }
            if base_schema != WM_SCHEMA {
                return Err(
                    "gsettings reset is only allowed for Goblins-owned shortcuts.".to_string(),
                );
            }
            validate_schema_key(schema, key)
        }
        [command, schema, key, value] if command == "set" => {
            validate_schema_key(schema, key)?;
            validate_gsettings_value(value)
        }
        _ => Err("unsupported session bridge gsettings operation.".to_string()),
    }
}

fn validate_schema_key(schema_arg: &str, key: &str) -> Result<(), String> {
    let (schema, path) = validate_schema_arg(schema_arg)?;
    let allowed = match schema {
        KEYBOARD_SCHEMA => KEYBOARD_KEYS,
        MOUSE_SCHEMA => MOUSE_KEYS,
        TOUCHPAD_SCHEMA => TOUCHPAD_KEYS,
        INPUT_SOURCES_SCHEMA => INPUT_SOURCE_KEYS,
        INTERFACE_SCHEMA => INTERFACE_KEYS,
        A11Y_APPS_SCHEMA => A11Y_APPS_KEYS,
        A11Y_INTERFACE_SCHEMA => A11Y_INTERFACE_KEYS,
        A11Y_KEYBOARD_SCHEMA => A11Y_KEYBOARD_KEYS,
        A11Y_MAGNIFIER_SCHEMA => A11Y_MAGNIFIER_KEYS,
        A11Y_MOUSE_SCHEMA => A11Y_MOUSE_KEYS,
        COLOR_SCHEMA => COLOR_KEYS,
        FOCUS_SCHEMA => FOCUS_KEYS,
        NOTIFICATIONS_SCHEMA => NOTIFICATION_KEYS,
        NOTIFICATION_APPLICATION_SCHEMA => NOTIFICATION_APPLICATION_KEYS,
        WM_SCHEMA => WM_KEYS,
        _ => return Err(format!("{schema} is not an allowlisted session schema.")),
    };
    if !allowed.iter().any(|candidate| candidate == &key) {
        return Err(format!(
            "{schema}.{key} is not allowlisted for the session bridge."
        ));
    }
    if schema != NOTIFICATION_APPLICATION_SCHEMA && !path.is_empty() {
        return Err(
            "path-scoped gsettings access is only allowed for notification applications."
                .to_string(),
        );
    }
    Ok(())
}

fn validate_list_keys_schema(schema: &str) -> Result<(), String> {
    if schema.contains(':') {
        return Err("gsettings list-keys must use an allowlisted base schema.".to_string());
    }
    let (base_schema, path) = validate_schema_arg_for_list_keys(schema)?;
    if !path.is_empty() {
        return Err("gsettings list-keys must not use path-scoped access.".to_string());
    }
    match base_schema {
        KEYBOARD_SCHEMA
        | MOUSE_SCHEMA
        | TOUCHPAD_SCHEMA
        | INPUT_SOURCES_SCHEMA
        | INTERFACE_SCHEMA
        | A11Y_APPS_SCHEMA
        | A11Y_INTERFACE_SCHEMA
        | A11Y_KEYBOARD_SCHEMA
        | A11Y_MAGNIFIER_SCHEMA
        | A11Y_MOUSE_SCHEMA
        | COLOR_SCHEMA
        | FOCUS_SCHEMA
        | NOTIFICATIONS_SCHEMA
        | NOTIFICATION_APPLICATION_SCHEMA
        | WM_SCHEMA => Ok(()),
        _ => Err(format!(
            "{base_schema} is not an allowlisted session schema."
        )),
    }
}

fn validate_schema_arg_for_list_keys(schema_arg: &str) -> Result<(&str, &str), String> {
    let (schema, path) = schema_arg.split_once(':').unwrap_or((schema_arg, ""));
    if schema.trim() != schema || schema.is_empty() {
        return Err("session bridge schema names must be exact.".to_string());
    }
    Ok((schema, path))
}

fn validate_schema_arg(schema_arg: &str) -> Result<(&str, &str), String> {
    let (schema, path) = schema_arg.split_once(':').unwrap_or((schema_arg, ""));
    if schema.trim() != schema || schema.is_empty() {
        return Err("session bridge schema names must be exact.".to_string());
    }
    match schema {
        KEYBOARD_SCHEMA
        | MOUSE_SCHEMA
        | TOUCHPAD_SCHEMA
        | INPUT_SOURCES_SCHEMA
        | INTERFACE_SCHEMA
        | A11Y_APPS_SCHEMA
        | A11Y_INTERFACE_SCHEMA
        | A11Y_KEYBOARD_SCHEMA
        | A11Y_MAGNIFIER_SCHEMA
        | A11Y_MOUSE_SCHEMA
        | COLOR_SCHEMA
        | FOCUS_SCHEMA
        | NOTIFICATIONS_SCHEMA
        | WM_SCHEMA => {
            if path.is_empty() {
                Ok((schema, path))
            } else {
                Err("this session schema does not support path-scoped access.".to_string())
            }
        }
        NOTIFICATION_APPLICATION_SCHEMA => {
            if notification_path_is_valid(path) {
                Ok((schema, path))
            } else {
                Err("notification application path is not allowlisted.".to_string())
            }
        }
        _ => Err(format!("{schema} is not an allowlisted session schema.")),
    }
}

fn notification_path_is_valid(path: &str) -> bool {
    let Some(child) = path
        .strip_prefix(NOTIFICATION_APPLICATION_BASE_PATH)
        .and_then(|value| value.strip_suffix('/'))
    else {
        return false;
    };
    !child.is_empty()
        && child.len() <= 240
        && child
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn validate_gsettings_value(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 16 * 1024 || value.contains('\0') {
        return Err("gsettings values must be non-empty, bounded, and NUL-free.".to_string());
    }
    if value.chars().any(|ch| ch.is_control() && ch != '\t') {
        return Err("gsettings values cannot contain control characters.".to_string());
    }
    Ok(())
}

fn validate_preview_open(path: &Path, kind: &str) -> Result<(), String> {
    if kind != "pdf" && kind != "image" {
        return Err("Preview opens only allowlisted PDF or image files.".to_string());
    }
    if !path.is_absolute() || !path.is_file() {
        return Err("Preview paths must be existing absolute regular files.".to_string());
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .ok_or_else(|| "Preview files must have a supported extension.".to_string())?;
    let allowed = match kind {
        "pdf" => matches!(extension.as_str(), "pdf" | "ps"),
        "image" => matches!(
            extension.as_str(),
            "bmp" | "gif" | "jpeg" | "jpg" | "png" | "tif" | "tiff" | "webp"
        ),
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err("Preview file extension does not match the requested preview kind.".to_string())
    }
}

fn validate_permission_store_delete(table: &str, id: &str, app: &str) -> Result<(), String> {
    if !matches!(table, "location" | "background" | "notifications") {
        return Err("PermissionStore deletes are limited to app-keyed tables.".to_string());
    }
    if !permission_id_is_safe(id) || !permission_id_is_safe(app) {
        return Err("PermissionStore app and resource ids must be safe desktop ids.".to_string());
    }
    if id != app {
        return Err(
            "PermissionStore deletes from Settings must target app-keyed grants.".to_string(),
        );
    }
    Ok(())
}

fn permission_id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn write_response(stream: &mut UnixStream, response: &BridgeResponse) -> Result<(), String> {
    let json = serde_json::to_vec(response).map_err(|error| error.to_string())?;
    stream.write_all(&json).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())
}

fn socket_path() -> PathBuf {
    env::var_os("GOBLINS_OS_SESSION_BRIDGE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET))
}

fn prepare_socket_parent(socket: &Path) -> Result<(), String> {
    let parent = socket
        .parent()
        .ok_or_else(|| "session bridge socket needs a parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    set_group_mode(parent, 0o770)
}

fn remove_stale_socket(socket: &Path) -> Result<(), String> {
    if !socket.exists() {
        return Ok(());
    }
    let metadata = fs::metadata(socket)
        .map_err(|error| format!("could not inspect {}: {error}", socket.display()))?;
    if !metadata.file_type().is_socket() {
        return Err(format!(
            "refusing to replace non-socket {}",
            socket.display()
        ));
    }
    fs::remove_file(socket).map_err(|error| format!("could not remove stale socket: {error}"))
}

fn set_group_mode(path: &Path, mode: u32) -> Result<(), String> {
    let _ = Command::new("chgrp")
        .args([SOCKET_GROUP, path.to_string_lossy().as_ref()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("could not chmod {}: {error}", path.display()))
}

fn command_error_detail(stderr: &[u8], stdout: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    if stdout.is_empty() {
        "session command failed without a message.".to_string()
    } else {
        stdout
    }
}

fn success(stdout: String) -> BridgeResponse {
    BridgeResponse {
        ok: true,
        stdout,
        detail: String::new(),
    }
}

fn failure(detail: impl Into<String>) -> BridgeResponse {
    BridgeResponse {
        ok: false,
        stdout: String::new(),
        detail: detail.into(),
    }
}

fn self_test() -> Result<(), String> {
    validate_gsettings_args(&["list-schemas".to_string()])?;
    validate_gsettings_args(&["list-keys".to_string(), INPUT_SOURCES_SCHEMA.to_string()])?;
    validate_gsettings_args(&[
        "set".to_string(),
        INPUT_SOURCES_SCHEMA.to_string(),
        "sources".to_string(),
        "[('xkb', 'us')]".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        FOCUS_SCHEMA.to_string(),
        "active-mode".to_string(),
        "'work'".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        INTERFACE_SCHEMA.to_string(),
        "enable-animations".to_string(),
        "false".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        KEYBOARD_SCHEMA.to_string(),
        "repeat".to_string(),
        "true".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        MOUSE_SCHEMA.to_string(),
        "speed".to_string(),
        "0.25".to_string(),
    ])?;
    validate_gsettings_args(&[
        "set".to_string(),
        TOUCHPAD_SCHEMA.to_string(),
        "tap-to-click".to_string(),
        "true".to_string(),
    ])?;
    validate_gsettings_args(&[
        "get".to_string(),
        format!("{NOTIFICATION_APPLICATION_SCHEMA}:{NOTIFICATION_APPLICATION_BASE_PATH}org-gnome-Console/"),
        "enable".to_string(),
    ])?;
    validate_gsettings_args(&[
        "reset".to_string(),
        WM_SCHEMA.to_string(),
        "window-hud".to_string(),
    ])?;
    validate_permission_store_delete(
        "location",
        "org.goblins.GatePrivacyProof",
        "org.goblins.GatePrivacyProof",
    )?;
    if validate_gsettings_args(&[
        "set".to_string(),
        "org.gnome.desktop.background".to_string(),
        "picture-uri".to_string(),
        "'file:///tmp/x'".to_string(),
    ])
    .is_ok()
    {
        return Err("non-allowlisted schema was accepted.".to_string());
    }
    if validate_gsettings_args(&[
        "set".to_string(),
        WM_SCHEMA.to_string(),
        "window-hud".to_string(),
        "bad\nvalue".to_string(),
    ])
    .is_ok()
    {
        return Err("control-character gsettings value was accepted.".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        validate_gsettings_args, validate_permission_store_delete, validate_schema_arg,
        FOCUS_SCHEMA, INPUT_SOURCES_SCHEMA, KEYBOARD_SCHEMA, MOUSE_SCHEMA,
        NOTIFICATION_APPLICATION_BASE_PATH, NOTIFICATION_APPLICATION_SCHEMA, TOUCHPAD_SCHEMA,
        WM_SCHEMA,
    };

    #[test]
    fn gsettings_allowlist_accepts_owned_session_keys() {
        assert!(validate_gsettings_args(&["list-schemas".to_string()]).is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            INPUT_SOURCES_SCHEMA.to_string(),
            "current".to_string(),
            "1".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            FOCUS_SCHEMA.to_string(),
            "modes".to_string(),
            "'[]'".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            KEYBOARD_SCHEMA.to_string(),
            "repeat-interval".to_string(),
            "uint32 30".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            MOUSE_SCHEMA.to_string(),
            "left-handed".to_string(),
            "false".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            TOUCHPAD_SCHEMA.to_string(),
            "disable-while-typing".to_string(),
            "true".to_string(),
        ])
        .is_ok());
        assert!(validate_gsettings_args(&[
            "reset".to_string(),
            WM_SCHEMA.to_string(),
            "mission-control".to_string(),
        ])
        .is_ok());
    }

    #[test]
    fn gsettings_allowlist_rejects_arbitrary_session_writes() {
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            "org.gnome.desktop.background".to_string(),
            "picture-uri".to_string(),
            "'file:///tmp/wallpaper'".to_string(),
        ])
        .is_err());
        assert!(validate_gsettings_args(&[
            "set".to_string(),
            INPUT_SOURCES_SCHEMA.to_string(),
            "sources".to_string(),
            "bad\nvalue".to_string(),
        ])
        .is_err());
    }

    #[test]
    fn path_scoped_schema_is_only_for_notification_apps() {
        let path = format!(
            "{NOTIFICATION_APPLICATION_SCHEMA}:{NOTIFICATION_APPLICATION_BASE_PATH}org-gnome-Console/"
        );
        assert!(validate_schema_arg(&path).is_ok());
        assert!(validate_schema_arg(&format!(
            "{INPUT_SOURCES_SCHEMA}:{NOTIFICATION_APPLICATION_BASE_PATH}org-gnome-Console/"
        ))
        .is_err());
    }

    #[test]
    fn permission_store_delete_is_limited_to_app_keyed_safe_grants() {
        assert!(validate_permission_store_delete(
            "location",
            "org.goblins.GatePrivacyProof",
            "org.goblins.GatePrivacyProof",
        )
        .is_ok());
        assert!(validate_permission_store_delete(
            "devices",
            "camera",
            "org.goblins.GatePrivacyProof",
        )
        .is_err());
        assert!(validate_permission_store_delete(
            "location",
            "camera",
            "org.goblins.GatePrivacyProof",
        )
        .is_err());
        assert!(validate_permission_store_delete(
            "location",
            "org.goblins.GatePrivacyProof;rm",
            "org.goblins.GatePrivacyProof;rm",
        )
        .is_err());
    }
}
