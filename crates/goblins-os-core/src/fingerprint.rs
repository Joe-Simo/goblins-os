//! Fingerprint unlock capability substrate.
//!
//! This reports the fprintd/authselect/PAM readiness needed before Settings can
//! offer enrollment. It deliberately does not enroll fingers or claim live auth
//! success; that requires a real reader and qemu/hardware proof.

use axum::Json;
use serde::Serialize;
use std::{env, path::Path, process::Command};

const DEFAULT_LOGIN_USER: &str = "goblin";
const FPRINTD_DBUS_SERVICE: &str =
    "/usr/share/dbus-1/system-services/net.reactivated.Fprint.service";
const FPRINTD_SYSTEMD_SERVICE: &str = "/usr/lib/systemd/system/fprintd.service";
const FPRINTD_DAEMON: &str = "/usr/libexec/fprintd";

#[derive(Serialize)]
pub struct FingerprintStatus {
    source: &'static str,
    target_user: String,
    available: bool,
    fprintd_available: bool,
    pam_module_available: bool,
    authselect_available: bool,
    authselect_fingerprint_enabled: bool,
    dbus_service_available: bool,
    reader_available: Option<bool>,
    enrolled_fingers: Vec<String>,
    detail: String,
}

struct FingerprintProbe {
    reader_available: Option<bool>,
    enrolled_fingers: Vec<String>,
}

pub async fn fingerprint_status() -> Json<FingerprintStatus> {
    Json(build_fingerprint_status())
}

fn build_fingerprint_status() -> FingerprintStatus {
    let target_user = env::var("GOBLINS_OS_FINGERPRINT_USER")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_LOGIN_USER.to_string());
    let fprintd_available = fprintd_available();
    let pam_module_available = pam_module_present();
    let authselect_available = command_present("authselect");
    let authselect_fingerprint_enabled = authselect_available
        && command_output("authselect", &["current"])
            .map(|text| authselect_has_fingerprint(&text))
            .unwrap_or(false);
    let dbus_service_available = path_exists(FPRINTD_DBUS_SERVICE);
    let probe = if fprintd_available {
        probe_fprintd_list(&target_user)
    } else {
        FingerprintProbe {
            reader_available: None,
            enrolled_fingers: Vec::new(),
        }
    };
    let available = fprintd_available && pam_module_available && authselect_fingerprint_enabled;
    let detail = fingerprint_detail(
        fprintd_available,
        pam_module_available,
        authselect_available,
        authselect_fingerprint_enabled,
        probe.reader_available,
        !probe.enrolled_fingers.is_empty(),
    )
    .to_string();

    FingerprintStatus {
        source: "goblins-os-core",
        target_user,
        available,
        fprintd_available,
        pam_module_available,
        authselect_available,
        authselect_fingerprint_enabled,
        dbus_service_available,
        reader_available: probe.reader_available,
        enrolled_fingers: probe.enrolled_fingers,
        detail,
    }
}

fn fprintd_available() -> bool {
    command_present("fprintd-list")
        && command_present("fprintd-enroll")
        && command_present("fprintd-delete")
        && command_present("fprintd-verify")
        && (path_exists(FPRINTD_DBUS_SERVICE)
            || path_exists(FPRINTD_SYSTEMD_SERVICE)
            || path_exists(FPRINTD_DAEMON))
}

fn probe_fprintd_list(target_user: &str) -> FingerprintProbe {
    match Command::new("fprintd-list").arg(target_user).output() {
        Ok(output) => parse_fprintd_list_output(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        ),
        Err(_) => FingerprintProbe {
            reader_available: None,
            enrolled_fingers: Vec::new(),
        },
    }
}

fn parse_fprintd_list_output(success: bool, stdout: &str, stderr: &str) -> FingerprintProbe {
    let combined = format!("{stdout}\n{stderr}");
    let lower = combined.to_ascii_lowercase();
    let enrolled_fingers = enrolled_fingers_from_text(&combined);
    let reader_available = if lower.contains("no devices available")
        || lower.contains("no fingerprint devices")
        || lower.contains("no devices found")
    {
        Some(false)
    } else if success
        && (lower.contains("using device")
            || lower.contains("device at ")
            || lower.contains("fingerprints for user"))
    {
        Some(true)
    } else {
        None
    };

    FingerprintProbe {
        reader_available,
        enrolled_fingers,
    }
}

fn enrolled_fingers_from_text(text: &str) -> Vec<String> {
    const FINGER_IDS: &[(&str, &str)] = &[
        ("left-thumb", "Left thumb"),
        ("left-index-finger", "Left index finger"),
        ("left-middle-finger", "Left middle finger"),
        ("left-ring-finger", "Left ring finger"),
        ("left-little-finger", "Left little finger"),
        ("right-thumb", "Right thumb"),
        ("right-index-finger", "Right index finger"),
        ("right-middle-finger", "Right middle finger"),
        ("right-ring-finger", "Right ring finger"),
        ("right-little-finger", "Right little finger"),
    ];
    let lower = text.to_ascii_lowercase();
    FINGER_IDS
        .iter()
        .filter(|(id, _)| lower.contains(id))
        .map(|(_, label)| (*label).to_string())
        .collect()
}

fn authselect_has_fingerprint(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim().trim_start_matches('-').trim() == "with-fingerprint")
}

fn fingerprint_detail(
    fprintd_available: bool,
    pam_module_available: bool,
    authselect_available: bool,
    authselect_fingerprint_enabled: bool,
    reader_available: Option<bool>,
    has_enrollment: bool,
) -> &'static str {
    if !fprintd_available {
        return "Fingerprint unlock is unavailable because fprintd is not installed on this image.";
    }
    if !pam_module_available {
        return "Fingerprint unlock needs the fprintd PAM module before it can be offered.";
    }
    if !authselect_available {
        return "Fingerprint unlock needs authselect before PAM can be configured safely.";
    }
    if !authselect_fingerprint_enabled {
        return "Fingerprint unlock is packaged, but PAM has not enabled the authselect fingerprint feature.";
    }
    match (reader_available, has_enrollment) {
        (Some(true), true) => {
            "Fingerprint unlock is configured for this device. Your password remains available as a fallback."
        }
        (Some(true), false) => {
            "Fingerprint support is ready, but no fingerprints are enrolled yet. Your password remains available as a fallback."
        }
        (Some(false), _) => "No fingerprint reader found on this device. Your password remains available.",
        (None, _) => {
            "Fingerprint support is packaged and PAM-enabled; reader detection needs the fprintd service in a desktop session. Your password remains available as a fallback."
        }
    }
}

fn pam_module_present() -> bool {
    env::var("GOBLINS_OS_PAM_FPRINTD_PATH")
        .ok()
        .map(|path| path_exists(&path))
        .unwrap_or_else(|| {
            path_exists("/usr/lib64/security/pam_fprintd.so")
                || path_exists("/usr/lib/security/pam_fprintd.so")
        })
}

fn command_output(binary: &str, args: &[&str]) -> Option<String> {
    Command::new(binary).args(args).output().ok().map(|output| {
        format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn command_present(binary: &str) -> bool {
    Path::new("/usr/sbin").join(binary).is_file()
        || Path::new("/usr/bin").join(binary).is_file()
        || env::var_os("PATH")
            .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

fn path_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

#[cfg(test)]
mod tests {
    use super::{
        authselect_has_fingerprint, enrolled_fingers_from_text, fingerprint_detail,
        parse_fprintd_list_output,
    };

    #[test]
    fn authselect_feature_parser_requires_fingerprint_feature() {
        assert!(authselect_has_fingerprint(
            "Profile ID: local\nEnabled features:\n- with-silent-lastlog\n- with-fingerprint\n"
        ));
        assert!(!authselect_has_fingerprint(
            "Profile ID: local\nEnabled features:\n- with-silent-lastlog\n"
        ));
    }

    #[test]
    fn fprintd_list_parser_reports_no_reader_without_fabricating_enrollment() {
        let probe = parse_fprintd_list_output(false, "", "No devices available");
        assert_eq!(probe.reader_available, Some(false));
        assert!(probe.enrolled_fingers.is_empty());
    }

    #[test]
    fn fprintd_list_parser_extracts_reader_and_enrolled_fingers() {
        let output = "\
found 1 devices
Using device /net/reactivated/Fprint/Device/0
Fingerprints for user goblin on Synaptics reader:
 - #0: right-index-finger
 - #1: left-thumb
";
        let probe = parse_fprintd_list_output(true, output, "");
        assert_eq!(probe.reader_available, Some(true));
        assert_eq!(
            probe.enrolled_fingers,
            vec!["Left thumb".to_string(), "Right index finger".to_string()]
        );
        assert_eq!(
            enrolled_fingers_from_text(output),
            vec!["Left thumb".to_string(), "Right index finger".to_string()]
        );
    }

    #[test]
    fn fingerprint_detail_stays_honest_about_missing_proof() {
        assert!(fingerprint_detail(false, false, false, false, None, false)
            .contains("fprintd is not installed"));
        assert!(fingerprint_detail(true, true, true, false, None, false)
            .contains("authselect fingerprint feature"));
        assert!(
            fingerprint_detail(true, true, true, true, Some(false), false)
                .contains("No fingerprint reader")
        );
        assert!(fingerprint_detail(true, true, true, true, None, false)
            .contains("reader detection needs"));
        assert!(fingerprint_detail(true, true, true, true, Some(true), true)
            .contains("password remains available"));
    }
}
