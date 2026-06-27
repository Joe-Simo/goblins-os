//! Keychain / "Passwords & Keys" status (Secret Service).
//!
//! The macOS "Keychain" altitude: report that secrets are kept in the system
//! keyring (gnome-keyring, the Secret Service provider) and whether the
//! Passwords & Keys manager (seahorse) is available to manage them. Read-only and
//! honest-gated — Goblins OS never shows or exports a secret here; it points at the
//! real store. Creating/editing secrets stays with the dedicated app.

use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct KeychainStatus {
    source: &'static str,
    /// Whether the Secret Service provider (gnome-keyring) is installed.
    secret_service_available: bool,
    /// Whether the Passwords & Keys manager app (seahorse) is installed.
    manager_app_available: bool,
    detail: String,
}

pub async fn keychain_status() -> Json<KeychainStatus> {
    Json(build_keychain_status())
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
    use super::keychain_detail;

    #[test]
    fn detail_reflects_availability() {
        assert!(keychain_detail(true, true).contains("Manage them"));
        assert!(keychain_detail(true, false).contains("system keyring"));
        assert!(!keychain_detail(true, false).contains("Manage them"));
        assert!(keychain_detail(false, false).contains("unavailable"));
    }
}
