use std::{
    env,
    io::{Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const DEFAULT_SOCKET: &str = "/run/goblins-os-session/session-bridge.sock";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SessionBridgeResult {
    Unavailable,
    Success(String),
    Failed(String),
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum BridgeRequest<'a> {
    GSettings { args: Vec<&'a str> },
    OpenPreview { path: String, kind: &'a str },
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
            "Goblins OS session bridge response failed: {error}"
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
    use super::{gsettings, SessionBridgeResult};

    #[test]
    fn absent_bridge_reports_unavailable_for_host_tests() {
        if std::env::var_os("GOBLINS_OS_SESSION_BRIDGE_SOCKET").is_none() {
            assert_eq!(
                gsettings(&["list-schemas"]),
                SessionBridgeResult::Unavailable
            );
        }
    }
}
