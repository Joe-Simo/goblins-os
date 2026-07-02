use std::{
    env,
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

const DEFAULT_SOCKET: &str = "/run/goblins-os-session/session-bridge.sock";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const BRIDGE_IO_TIMEOUT: Duration = Duration::from_millis(2_000);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SessionBridgeResult {
    Unavailable,
    Success(String),
    Failed(String),
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum BridgeRequest<'a> {
    GSettings {
        args: Vec<&'a str>,
    },
    OpenPreview {
        path: String,
        kind: &'a str,
    },
    Wpctl {
        args: Vec<&'a str>,
    },
    PermissionStoreDelete {
        table: &'a str,
        id: &'a str,
        app: &'a str,
    },
    DisplayConfigGetCurrentState,
    DisplayConfigGetApplyAllowed,
    DisplayConfigApplyMonitors {
        serial: u32,
        method: u32,
        logical_monitors: Vec<DisplayConfigLogicalMonitor<'a>>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DisplayConfigLogicalMonitor<'a> {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) scale: f64,
    pub(crate) transform: u32,
    pub(crate) primary: bool,
    pub(crate) monitors: Vec<DisplayConfigMonitor<'a>>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DisplayConfigMonitor<'a> {
    pub(crate) connector: &'a str,
    pub(crate) mode_id: &'a str,
}

#[derive(Deserialize)]
struct BridgeResponse {
    ok: bool,
    stdout: String,
    detail: String,
}

pub(crate) fn gsettings(args: &[&str]) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::GSettings {
        args: args.to_vec(),
    })
}

pub(crate) fn open_preview(path: &Path, kind: &'static str) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::OpenPreview {
        path: path.display().to_string(),
        kind,
    })
}

pub(crate) fn wpctl(args: &[&str]) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::Wpctl {
        args: args.to_vec(),
    })
}

pub(crate) fn permission_store_delete_permission(
    table: &str,
    id: &str,
    app: &str,
) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::PermissionStoreDelete { table, id, app })
}

pub(crate) fn display_config_get_current_state() -> SessionBridgeResult {
    call_bridge(&BridgeRequest::DisplayConfigGetCurrentState)
}

pub(crate) fn display_config_get_apply_allowed() -> SessionBridgeResult {
    call_bridge(&BridgeRequest::DisplayConfigGetApplyAllowed)
}

pub(crate) fn display_config_apply_monitors(
    serial: u32,
    method: u32,
    logical_monitors: Vec<DisplayConfigLogicalMonitor<'_>>,
) -> SessionBridgeResult {
    call_bridge(&BridgeRequest::DisplayConfigApplyMonitors {
        serial,
        method,
        logical_monitors,
    })
}

fn call_bridge(request: &BridgeRequest<'_>) -> SessionBridgeResult {
    let socket = socket_path();
    if !socket.exists() {
        return SessionBridgeResult::Unavailable;
    }

    let mut stream = match UnixStream::connect(&socket) {
        Ok(stream) => stream,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SessionBridgeResult::Unavailable;
        }
        Err(error) => {
            return SessionBridgeResult::Failed(format!(
                "Goblins OS session bridge is not reachable: {error}"
            ));
        }
    };
    if let Err(error) = stream.set_read_timeout(Some(BRIDGE_IO_TIMEOUT)) {
        return SessionBridgeResult::Failed(format!(
            "Goblins OS session bridge read timeout could not be set: {error}"
        ));
    }
    if let Err(error) = stream.set_write_timeout(Some(BRIDGE_IO_TIMEOUT)) {
        return SessionBridgeResult::Failed(format!(
            "Goblins OS session bridge write timeout could not be set: {error}"
        ));
    }
    let request = match serde_json::to_vec(request) {
        Ok(request) => request,
        Err(_) => {
            return SessionBridgeResult::Failed(
                "Goblins OS session bridge request could not be encoded.".to_string(),
            );
        }
    };
    if let Err(error) = stream.write_all(&request) {
        return SessionBridgeResult::Failed(format!(
            "Goblins OS session bridge request failed: {error}"
        ));
    }
    let _ = stream.shutdown(Shutdown::Write);

    let mut response = String::new();
    if let Err(error) = stream
        .take(MAX_RESPONSE_BYTES as u64)
        .read_to_string(&mut response)
    {
        return SessionBridgeResult::Failed(format!(
            "Goblins OS session bridge did not answer before the core bridge timeout: {error}"
        ));
    }
    match serde_json::from_str::<BridgeResponse>(&response) {
        Ok(response) if response.ok => SessionBridgeResult::Success(response.stdout),
        Ok(response) => SessionBridgeResult::Failed(if response.detail.is_empty() {
            "Goblins OS session bridge rejected the request.".to_string()
        } else {
            response.detail
        }),
        Err(_) => SessionBridgeResult::Failed(
            "Goblins OS session bridge returned an invalid response.".to_string(),
        ),
    }
}

fn socket_path() -> PathBuf {
    env::var_os("GOBLINS_OS_SESSION_BRIDGE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET))
}

#[cfg(test)]
mod tests {
    use super::{gsettings, wpctl, SessionBridgeResult};

    #[test]
    fn absent_bridge_reports_unavailable_for_host_tests() {
        if std::env::var_os("GOBLINS_OS_SESSION_BRIDGE_SOCKET").is_none() {
            assert_eq!(
                gsettings(&["list-schemas"]),
                SessionBridgeResult::Unavailable
            );
            assert_eq!(wpctl(&["status"]), SessionBridgeResult::Unavailable);
        }
    }
}
