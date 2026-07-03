//! Keychain / "Passwords & Keys" status (Secret Service).
//!
//! The macOS "Keychain" altitude: report that secrets are kept in the system
//! keyring (gnome-keyring, the Secret Service provider) and whether the
//! Passwords & Keys manager (seahorse) is available to manage them. Read-only and
//! honest-gated — Goblins OS never shows or exports a secret here; it points at the
//! real store. Creating/editing secrets stays with the dedicated app.

use axum::Json;
use serde::Serialize;

use crate::bounded::{bounded_command_output, probe_timeout};

const SECRET_SERVICE_DEST: &str = "org.freedesktop.secrets";
const SECRET_SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SECRET_SERVICE_INTERFACE: &str = "org.freedesktop.Secret.Service";
const SECRET_COLLECTION_INTERFACE: &str = "org.freedesktop.Secret.Collection";

#[derive(Serialize)]
pub struct KeychainStatus {
    source: &'static str,
    /// Whether the Secret Service provider (gnome-keyring) is installed.
    secret_service_available: bool,
    /// Whether the Passwords & Keys manager app (seahorse) is installed.
    manager_app_available: bool,
    detail: String,
}

#[derive(Serialize)]
pub struct KeychainCollectionsStatus {
    source: &'static str,
    available: bool,
    collections: Vec<KeychainCollectionStatus>,
    detail: String,
}

#[derive(Serialize)]
pub struct KeychainCollectionStatus {
    id: String,
    label: String,
    locked: Option<bool>,
    item_count: Option<usize>,
    detail: String,
}

pub async fn keychain_status() -> Json<KeychainStatus> {
    Json(build_keychain_status())
}

pub async fn keychain_collections() -> Json<KeychainCollectionsStatus> {
    Json(build_keychain_collections_status())
}

fn build_keychain_status() -> KeychainStatus {
    let secret_service = binary_present("gnome-keyring-daemon");
    let manager = binary_present("seahorse");
    KeychainStatus {
        source: "goblins-os-core",
        secret_service_available: secret_service,
        manager_app_available: manager,
        detail: keychain_detail(secret_service, manager).to_string(),
    }
}

fn build_keychain_collections_status() -> KeychainCollectionsStatus {
    let secret_service = binary_present("gnome-keyring-daemon");
    if !secret_service {
        return keychain_collections_response(
            false,
            Vec::new(),
            "The system keyring is unavailable on this device.",
        );
    }
    if !binary_present("gdbus") {
        return keychain_collections_response(
            false,
            Vec::new(),
            "Keychain collection metadata is unavailable because the desktop D-Bus bridge is missing.",
        );
    }

    let paths = match secret_collection_paths() {
        Ok(paths) => paths,
        Err(_) => {
            return keychain_collections_response(
                false,
                Vec::new(),
                "Secret Service is installed, but collection metadata could not be read in this session. Unlock your login keyring and try again.",
            )
        }
    };
    let collections = paths
        .into_iter()
        .map(|path| secret_collection_status(&path))
        .collect::<Vec<_>>();
    let detail = if collections.is_empty() {
        "Secret Service is available and reported no saved password collections. Secret values are never returned by Goblins OS.".to_string()
    } else {
        format!(
            "{} keychain collection{} reported. Metadata only; secret values are never returned by Goblins OS.",
            collections.len(),
            if collections.len() == 1 { "" } else { "s" }
        )
    };

    KeychainCollectionsStatus {
        source: "goblins-os-core",
        available: true,
        collections,
        detail,
    }
}

fn keychain_collections_response(
    available: bool,
    collections: Vec<KeychainCollectionStatus>,
    detail: &str,
) -> KeychainCollectionsStatus {
    KeychainCollectionsStatus {
        source: "goblins-os-core",
        available,
        collections,
        detail: detail.to_string(),
    }
}

fn secret_collection_paths() -> Result<Vec<String>, String> {
    let output = secret_property(SECRET_SERVICE_PATH, SECRET_SERVICE_INTERFACE, "Collections")?;
    Ok(parse_gdbus_object_paths(&output))
}

fn secret_collection_status(path: &str) -> KeychainCollectionStatus {
    let label = secret_property(path, SECRET_COLLECTION_INTERFACE, "Label")
        .ok()
        .and_then(|reply| parse_gdbus_string_variant(&reply))
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| keychain_collection_fallback_label(path));
    let locked = secret_property(path, SECRET_COLLECTION_INTERFACE, "Locked")
        .ok()
        .and_then(|reply| parse_gdbus_bool_variant(&reply));
    let item_count = secret_property(path, SECRET_COLLECTION_INTERFACE, "Items")
        .ok()
        .map(|reply| parse_gdbus_object_paths(&reply).len());
    let detail = keychain_collection_detail(locked, item_count);

    KeychainCollectionStatus {
        id: keychain_collection_id(path, &label),
        label,
        locked,
        item_count,
        detail,
    }
}

fn secret_property(object_path: &str, interface: &str, property: &str) -> Result<String, String> {
    let output = bounded_command_output(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            SECRET_SERVICE_DEST,
            "--object-path",
            object_path,
            "--method",
            "org.freedesktop.DBus.Properties.Get",
            interface,
            property,
        ],
        probe_timeout(),
    )
    .map_err(|_| "desktop D-Bus bridge could not be started".to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        "Secret Service property query failed".to_string()
    } else {
        stderr
    })
}

fn parse_gdbus_object_paths(reply: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut rest = reply;
    while let Some(start) = rest.find("objectpath '") {
        let after_start = &rest[start + "objectpath '".len()..];
        let Some(end) = after_start.find('\'') else {
            break;
        };
        let path = &after_start[..end];
        if path.starts_with("/org/freedesktop/secrets/") && !paths.iter().any(|seen| seen == path) {
            paths.push(path.to_string());
        }
        rest = &after_start[end + 1..];
    }
    paths
}

fn parse_gdbus_string_variant(reply: &str) -> Option<String> {
    parse_gdbus_quoted_values(reply)
        .into_iter()
        .find(|value| !value.starts_with("/org/freedesktop/secrets/"))
}

fn parse_gdbus_quoted_values(reply: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = reply.chars();
    while let Some(ch) = chars.next() {
        if ch != '\'' {
            continue;
        }
        let mut value = String::new();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        value.push(escaped);
                    }
                }
                '\'' => {
                    values.push(value);
                    break;
                }
                _ => value.push(ch),
            }
        }
    }
    values
}

fn parse_gdbus_bool_variant(reply: &str) -> Option<bool> {
    if reply.contains("<true>") || reply.contains("true") {
        Some(true)
    } else if reply.contains("<false>") || reply.contains("false") {
        Some(false)
    } else {
        None
    }
}

fn keychain_collection_fallback_label(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .map(|part| part.replace('_', " "))
        .unwrap_or_else(|| "Keychain collection".to_string())
}

fn keychain_collection_id(path: &str, label: &str) -> String {
    let mut id = label
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_whitespace() || matches!(ch, '-' | '_') {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>();
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    id = id.trim_matches('-').chars().take(48).collect();
    if id.is_empty() {
        path.rsplit('/')
            .next()
            .filter(|part| !part.is_empty())
            .unwrap_or("collection")
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(48)
            .collect()
    } else {
        id
    }
}

fn keychain_collection_detail(locked: Option<bool>, item_count: Option<usize>) -> String {
    let lock_detail = match locked {
        Some(true) => "Locked collection.",
        Some(false) => "Unlocked collection.",
        None => "Lock state is not reported by this session.",
    };
    let item_detail = match item_count {
        Some(1) => "1 saved item reported.",
        Some(count) => return format!("{lock_detail} {count} saved items reported. Metadata only; no secret values are returned."),
        None => "Saved-item count is not reported.",
    };
    format!("{lock_detail} {item_detail} Metadata only; no secret values are returned.")
}

/// Honest copy for each availability combination. Pure + unit-tested.
fn keychain_detail(secret_service: bool, manager: bool) -> &'static str {
    match (secret_service, manager) {
        (true, true) => {
            "Your passwords and keys are kept in the system keyring. Manage them in Passwords & Keys."
        }
        (true, false) => "Your passwords and keys are kept in the system keyring.",
        (false, _) => "The system keyring is unavailable on this device.",
    }
}

fn binary_present(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{
        keychain_collection_detail, keychain_collection_id, keychain_detail,
        parse_gdbus_bool_variant, parse_gdbus_object_paths, parse_gdbus_string_variant,
    };

    #[test]
    fn detail_reflects_availability() {
        assert!(keychain_detail(true, true).contains("Manage them"));
        assert!(keychain_detail(true, false).contains("system keyring"));
        assert!(!keychain_detail(true, false).contains("Manage them"));
        assert!(keychain_detail(false, false).contains("unavailable"));
    }

    #[test]
    fn parses_secret_service_metadata_without_secret_values() {
        let collections = parse_gdbus_object_paths(
            "(<[objectpath '/org/freedesktop/secrets/collection/login', objectpath '/org/freedesktop/secrets/collection/session']>,)",
        );
        assert_eq!(
            collections,
            vec![
                "/org/freedesktop/secrets/collection/login".to_string(),
                "/org/freedesktop/secrets/collection/session".to_string()
            ]
        );
        assert_eq!(
            parse_gdbus_string_variant("(<\"ignored\">, 'Login keyring')"),
            Some("Login keyring".to_string())
        );
        assert_eq!(parse_gdbus_bool_variant("(<true>,)"), Some(true));
        assert_eq!(parse_gdbus_bool_variant("(<false>,)"), Some(false));

        let detail = keychain_collection_detail(Some(false), Some(3));
        assert!(detail.contains("Metadata only"));
        assert!(detail.contains("no secret values"));
        assert!(!detail.contains("password"));
    }

    #[test]
    fn collection_ids_are_stable_without_returning_paths() {
        assert_eq!(
            keychain_collection_id("/org/freedesktop/secrets/collection/login", "Login keyring"),
            "login-keyring"
        );
        assert_eq!(
            keychain_collection_id("/org/freedesktop/secrets/collection/session", ""),
            "session"
        );
    }
}
