use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const LAUNCHER_BIN: &str = "/usr/libexec/goblins-os/goblins-os-launcher";
const SOURCE_ENV: &str = "GOBLINS_OS_SCREEN_CONTEXT_SOURCE";
const SUMMARY_ENV: &str = "GOBLINS_OS_VISUAL_CONTEXT_SUMMARY";
const SCREENSHOT_PATH_ENV: &str = "GOBLINS_OS_SCREENSHOT_CONTEXT_PATH";

fn main() {
    let mut launcher = Command::new(LAUNCHER_BIN);
    launcher.arg("--visual-context");

    match capture_screenshot() {
        Ok(path) => {
            launcher.env(SOURCE_ENV, source_value());
            launcher.env(SCREENSHOT_PATH_ENV, path.as_os_str());
            launcher.env(SUMMARY_ENV, screenshot_summary(&path));
        }
        Err(detail) => {
            launcher.env(SOURCE_ENV, source_value());
            launcher.env(SUMMARY_ENV, capture_failure_summary(&detail));
        }
    }

    if let Err(error) = launcher.spawn() {
        eprintln!("goblins-os-screenshot-context: failed to open launcher: {error}");
    }
}

fn capture_screenshot() -> Result<PathBuf, String> {
    let dir =
        screenshot_dir().map_err(|error| format!("could not prepare runtime folder: {error}"))?;
    let requested = dir.join(format!(
        "goblins-screenshot-context-{}.png",
        timestamp_millis()
    ));
    let requested_text = requested.to_string_lossy().to_string();

    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell",
            "--object-path",
            "/org/gnome/Shell/Screenshot",
            "--method",
            "org.gnome.Shell.Screenshot.Screenshot",
            "false",
            "true",
            &requested_text,
        ])
        .output()
        .map_err(|error| format!("could not run GNOME screenshot service: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(compact_detail(&format!("{stdout} {stderr}")));
    }

    if requested.is_file() {
        return Ok(requested);
    }

    if let Some(used) = parse_gdbus_screenshot_path(&stdout) {
        let used_path = PathBuf::from(used);
        if used_path.is_file() {
            fs::copy(&used_path, &requested)
                .map_err(|error| format!("could not copy captured screenshot: {error}"))?;
            return Ok(requested);
        }
    }

    Err(compact_detail(&format!(
        "GNOME screenshot service did not produce a file. {stdout} {stderr}"
    )))
}

fn screenshot_dir() -> io::Result<PathBuf> {
    let base = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::temp_dir().join(format!("goblins-os-screenshot-context-{}", safe_user()))
        });
    let dir = base.join("goblins-os").join("screenshot-context");
    fs::create_dir_all(&dir)?;
    set_private_permissions(&dir)?;
    Ok(dir)
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn source_value() -> String {
    env::var(SOURCE_ENV)
        .ok()
        .map(|value| sanitize_context_value(&value, 120))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "screenshot-capture".to_string())
}

fn screenshot_summary(_path: &Path) -> String {
    "Goblins OS captured a screenshot locally for this request. The image pixels stay local; this text-only handoff has not sent the screenshot to the model. Describe what matters or paste recognized text, then press Return.".to_string()
}

fn capture_failure_summary(detail: &str) -> String {
    let detail = sanitize_context_value(detail, 220);
    format!(
        "Screenshot capture did not complete. Describe the screenshot or paste recognized text, then press Return. No pixels were sent to the model. Capture detail: {detail}"
    )
}

fn parse_gdbus_screenshot_path(output: &str) -> Option<String> {
    output
        .rsplit_once(", '")
        .and_then(|(_, tail)| tail.trim().strip_suffix("')"))
        .map(ToString::to_string)
        .filter(|path| !path.is_empty())
}

fn safe_user() -> String {
    let value = env::var("USER").unwrap_or_else(|_| "user".to_string());
    let safe = sanitize_context_value(&value, 48)
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if safe.is_empty() {
        "user".to_string()
    } else {
        safe
    }
}

fn sanitize_context_value(value: &str, max_chars: usize) -> String {
    value
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

fn compact_detail(detail: &str) -> String {
    let text = sanitize_context_value(detail, 260);
    if text.is_empty() {
        "No detail returned.".to_string()
    } else {
        text
    }
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_is_explicitly_local_and_text_only() {
        let summary = screenshot_summary(Path::new(
            "/run/user/1000/goblins-os/screenshot-context/a.png",
        ));
        assert!(summary.contains("captured a screenshot locally"));
        assert!(summary.contains("pixels stay local"));
        assert!(summary.contains("text-only handoff"));
        assert!(summary.contains("not sent the screenshot to the model"));
    }

    #[test]
    fn failure_summary_has_recovery_copy() {
        let summary = capture_failure_summary("org.gnome.Shell.Screenshot unavailable");
        assert!(summary.contains("Screenshot capture did not complete"));
        assert!(summary.contains("No pixels were sent to the model"));
        assert!(summary.contains("Describe the screenshot"));
    }

    #[test]
    fn parses_gdbus_filename_used() {
        assert_eq!(
            parse_gdbus_screenshot_path("(true, '/tmp/gnome-shell-screenshot.png')").as_deref(),
            Some("/tmp/gnome-shell-screenshot.png")
        );
        assert_eq!(parse_gdbus_screenshot_path("(false, '')"), None);
    }

    #[test]
    fn sanitizes_context_values_to_single_line() {
        assert_eq!(sanitize_context_value(" a\n b\t c ", 8), "a b c");
        assert_eq!(sanitize_context_value("abcdefgh", 4), "abcd");
    }
}
