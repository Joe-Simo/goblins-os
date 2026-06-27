use std::{
    env, fs,
    io::{self, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ashpd::desktop::screenshot::Screenshot;

/// The on-device core's loopback address (also where voice/AI handoffs talk).
const CORE_HOST: &str = "127.0.0.1:8787";
/// Cap on the OCR response we read back (recognized text can be large, but not
/// unbounded) and on how long capture will wait on the core before moving on.
const OCR_MAX_BODY: u64 = 1_048_576;
const OCR_TIMEOUT: Duration = Duration::from_secs(30);

const LAUNCHER_BIN: &str = "/usr/libexec/goblins-os/goblins-os-launcher";
const SOURCE_ENV: &str = "GOBLINS_OS_SCREEN_CONTEXT_SOURCE";
const SUMMARY_ENV: &str = "GOBLINS_OS_VISUAL_CONTEXT_SUMMARY";
const SCREENSHOT_PATH_ENV: &str = "GOBLINS_OS_SCREENSHOT_CONTEXT_PATH";

/// Upper bound on how long to wait for the portal to answer. The consent dialog
/// needs human reaction time, so the bound is generous — but finite, so a wedged
/// backend or a dialog the user walks away from still degrades to the manual
/// fallback (and the launcher still opens) instead of hanging forever.
const PORTAL_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    let mut launcher = Command::new(LAUNCHER_BIN);
    launcher.arg("--visual-context");

    match capture_screenshot() {
        Ok(path) => {
            // Live Text: recognize text on-device so the model receives the real
            // words instead of asking the user to retype them. Best-effort — a
            // missing/declined OCR runtime just falls back to the plain summary.
            let recognized = recognized_text(&path);
            launcher.env(SOURCE_ENV, source_value());
            launcher.env(SCREENSHOT_PATH_ENV, path.as_os_str());
            launcher.env(
                SUMMARY_ENV,
                screenshot_summary(&path, recognized.as_deref()),
            );
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

/// Capture the whole screen through xdg-desktop-portal's Screenshot interface on
/// `org.freedesktop.portal.Desktop`
/// (`org.freedesktop.portal.Screenshot.Screenshot`).
///
/// GNOME 42+ rejects external callers of the old `org.gnome.Shell.Screenshot`
/// service, so the portal is the sanctioned capture path on Wayland: it routes
/// the request through the desktop's own consent prompt, which is exactly the
/// explicit-consent model Goblins OS already applies to screen context. The
/// captured pixels are copied into our private runtime folder and never leave
/// the machine — the launcher only ever hands the model text.
fn capture_screenshot() -> Result<PathBuf, String> {
    let dir =
        screenshot_dir().map_err(|error| format!("could not prepare runtime folder: {error}"))?;
    let requested = dir.join(format!(
        "goblins-screenshot-context-{}.png",
        timestamp_millis()
    ));

    let source = portal_screenshot()?;
    if !source.is_file() {
        return Err(compact_detail(&format!(
            "the desktop screenshot portal reported {} but no file was found",
            source.display()
        )));
    }

    fs::copy(&source, &requested)
        .map_err(|error| format!("could not copy captured screenshot: {error}"))?;
    // The runtime dir is already 0700, but pin the image itself to owner-only so
    // its bytes stay private regardless of whatever mode the portal wrote at the
    // source (the document-portal copy can land group/other-readable).
    set_owner_only(&requested)
        .map_err(|error| format!("could not lock down the captured screenshot: {error}"))?;
    Ok(requested)
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Drive the portal Screenshot request to completion and return the local file
/// path it wrote. The portal answers asynchronously (through a `Response`
/// signal), so the request runs on a single-threaded Tokio runtime that blocks
/// for the one result this helper needs. `interactive` is left off so the whole
/// screen is captured without an area picker, matching the previous auto-capture.
fn portal_screenshot() -> Result<PathBuf, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start the screenshot runtime: {error}"))?;

    let capture = async {
        let screenshot = Screenshot::request()
            .interactive(false)
            .modal(true)
            .send()
            .await
            .map_err(|error| format!("desktop screenshot portal request failed: {error}"))?
            .response()
            .map_err(|error| format!("desktop screenshot portal declined the capture: {error}"))?;
        Ok::<String, String>(screenshot.uri().to_string())
    };

    let uri = runtime.block_on(async {
        match tokio::time::timeout(PORTAL_TIMEOUT, capture).await {
            Ok(result) => result,
            Err(_) => Err(format!(
                "desktop screenshot portal did not respond within {}s",
                PORTAL_TIMEOUT.as_secs()
            )),
        }
    })?;

    portal_uri_to_path(&uri)
}

/// Translate the portal's `file://` screenshot URI into a filesystem path,
/// decoding any percent-escapes (spaces, unicode) the portal added.
fn portal_uri_to_path(uri: &str) -> Result<PathBuf, String> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| format!("portal returned a non-file screenshot URI: {uri}"))?;
    // The authority is empty for local files (`file:///path`), but may name a
    // host such as `localhost` (`file://localhost/path`); keep only the path.
    let authority_trimmed = match rest.find('/') {
        Some(index) => &rest[index..],
        None => return Err(format!("portal returned a path-less screenshot URI: {uri}")),
    };
    // A `file://` URI carries no query/fragment and any literal `?`/`#` in the
    // path would be percent-escaped, so an unescaped one can only be a delimiter:
    // drop it (and anything after) before decoding rather than folding it in.
    let encoded_path = match authority_trimmed.find(['?', '#']) {
        Some(cut) => &authority_trimmed[..cut],
        None => authority_trimmed,
    };
    Ok(decode_uri_path(encoded_path))
}

#[cfg(unix)]
fn decode_uri_path(encoded: &str) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;

    // Linux paths are bytes, not necessarily UTF-8; decode the percent-escapes
    // back to the exact bytes the portal wrote.
    let bytes = percent_encoding::percent_decode_str(encoded).collect::<Vec<u8>>();
    PathBuf::from(std::ffi::OsStr::from_bytes(&bytes))
}

#[cfg(not(unix))]
fn decode_uri_path(encoded: &str) -> PathBuf {
    let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8_lossy();
    PathBuf::from(decoded.as_ref())
}

fn screenshot_dir() -> io::Result<PathBuf> {
    let base = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set"))?;
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

/// Recognize text in the captured image via the local core's on-device OCR. Pure
/// best-effort: any failure (core down, no Tesseract, declined) returns None and
/// the caller keeps the plain summary — OCR never blocks the handoff.
fn recognized_text(image_path: &Path) -> Option<String> {
    let body = serde_json::json!({ "image_path": image_path.to_string_lossy() }).to_string();
    let response = http_post_local("/v1/ocr/recognize", &body)?;

    #[derive(serde::Deserialize)]
    struct OcrResponse {
        ok: bool,
        text: String,
    }
    let parsed: OcrResponse = serde_json::from_str(&response).ok()?;
    let text = parsed.text.trim();
    (parsed.ok && !text.is_empty()).then(|| text.to_string())
}

/// Minimal loopback HTTP POST to the core (no HTTP-client dep). `Connection:
/// close` lets us read the whole body to EOF; the read is capped and time-bounded.
fn http_post_local(path: &str, body: &str) -> Option<String> {
    let mut stream = TcpStream::connect(CORE_HOST).ok()?;
    stream.set_read_timeout(Some(OCR_TIMEOUT)).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = Vec::new();
    stream.take(OCR_MAX_BODY).read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw);
    let body_start = text.find("\r\n\r\n")? + 4;
    Some(text[body_start..].to_string())
}

/// The text-only handoff summary the launcher passes to the model. When on-device
/// OCR found text, it is included directly (so the model reads the real words);
/// otherwise the calm "describe or paste" copy is kept.
fn screenshot_summary(_path: &Path, recognized: Option<&str>) -> String {
    let base = "Goblins OS captured a screenshot locally for this request. The image pixels stay local; this text-only handoff has not sent the screenshot to the model.";
    match recognized
        .map(|text| sanitize_context_value(text, 2000))
        .filter(|text| !text.is_empty())
    {
        Some(text) => format!(
            "{base} Text recognized on-device in the screenshot: {text} Describe what matters, then press Return."
        ),
        None => format!(
            "{base} Describe what matters or paste recognized text, then press Return."
        ),
    }
}

fn capture_failure_summary(detail: &str) -> String {
    let detail = sanitize_context_value(detail, 220);
    format!(
        "Screenshot capture did not complete. Describe the screenshot or paste recognized text, then press Return. No pixels were sent to the model. Capture detail: {detail}"
    )
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
        let path = Path::new("/run/user/1000/goblins-os/screenshot-context/a.png");
        let summary = screenshot_summary(path, None);
        assert!(summary.contains("captured a screenshot locally"));
        assert!(summary.contains("pixels stay local"));
        assert!(summary.contains("text-only handoff"));
        assert!(summary.contains("not sent the screenshot to the model"));
        // With no recognized text, the calm "describe or paste" copy is kept.
        assert!(summary.contains("paste recognized text"));
    }

    #[test]
    fn summary_includes_recognized_text_when_present() {
        let path = Path::new("/run/user/1000/goblins-os/screenshot-context/a.png");
        // Recognized text is sanitized to one line and folded into the handoff so
        // the model reads the real words, not a "paste it yourself" instruction.
        let summary = screenshot_summary(path, Some("Invoice\nTotal: $42"));
        assert!(summary.contains("recognized on-device"));
        assert!(summary.contains("Invoice Total: $42"));
        assert!(!summary.contains("paste recognized text"));
    }

    #[test]
    fn failure_summary_has_recovery_copy() {
        let summary = capture_failure_summary("desktop screenshot portal was unavailable");
        assert!(summary.contains("Screenshot capture did not complete"));
        assert!(summary.contains("No pixels were sent to the model"));
        assert!(summary.contains("Describe the screenshot"));
    }

    #[test]
    fn maps_portal_file_uri_to_path() {
        // The portal hands back a percent-escaped file:// URI; we decode it to a
        // real path (note the space restored from %20).
        assert_eq!(
            portal_uri_to_path("file:///run/user/1000/doc/ab12/Screenshot%20from%202026.png")
                .unwrap(),
            PathBuf::from("/run/user/1000/doc/ab12/Screenshot from 2026.png")
        );
        assert!(portal_uri_to_path("not a file uri").is_err());
        assert!(portal_uri_to_path("https://example.test/x.png").is_err());
        // An empty authority and a named host both resolve to the same path.
        assert_eq!(
            portal_uri_to_path("file://localhost/tmp/shot.png").unwrap(),
            PathBuf::from("/tmp/shot.png")
        );
        // Defensive: a stray query/fragment is a delimiter, not part of the path.
        assert_eq!(
            portal_uri_to_path("file:///tmp/a%20b.png?token=1#frag").unwrap(),
            PathBuf::from("/tmp/a b.png")
        );
    }

    #[test]
    fn sanitizes_context_values_to_single_line() {
        assert_eq!(sanitize_context_value(" a\n b\t c ", 8), "a b c");
        assert_eq!(sanitize_context_value("abcdefgh", 4), "abcd");
    }
}
