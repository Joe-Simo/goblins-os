//! Goblins OS Visual Look Up — user-invoked region capture to an on-device VLM.
//!
//! This helper owns only the desktop surface: it checks the local core vision
//! gate first, asks the xdg-desktop-portal Screenshot picker for an interactive
//! region, copies the image into a private runtime directory, POSTs the path to
//! `/v1/ai/visual-lookup`, removes the temporary file, and renders an honest
//! branded identification card. Pixels go only to the loopback core/runtime path.

#![cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code, unused_imports)
)]

use std::{
    env, fs,
    io::{self, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ashpd::desktop::screenshot::Screenshot;
use serde::{Deserialize, Serialize};

const DEFAULT_CORE_HOST: &str = "127.0.0.1:8787";
const CORE_URL_ENV: &str = "GOBLINS_OS_CORE_URL";
const LEGACY_CORE_URL_ENV: &str = "OPENAI_OS_CORE_URL";
const HTTP_MAX_BODY: u64 = 1_048_576;
const HTTP_TIMEOUT: Duration = Duration::from_secs(95);
const PORTAL_TIMEOUT: Duration = Duration::from_secs(120);

type VisualResult<T> = Result<T, String>;

#[derive(Clone)]
struct VisualLookupConfig {
    core_host: String,
}

impl VisualLookupConfig {
    fn from_env() -> Self {
        let core_host = env::var(CORE_URL_ENV)
            .or_else(|_| env::var(LEGACY_CORE_URL_ENV))
            .ok()
            .and_then(|url| loopback_http_host(&url))
            .unwrap_or_else(|| DEFAULT_CORE_HOST.to_string());
        Self { core_host }
    }
}

#[derive(Clone, Deserialize)]
struct VisionStatus {
    #[allow(dead_code)]
    source: String,
    runtime_configured: bool,
    detail: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct Identification {
    name: String,
    category: String,
    confidence: String,
    description: String,
    follow_ups: Vec<String>,
}

#[derive(Deserialize)]
struct VisualLookupOutcome {
    ok: bool,
    identification: Option<Identification>,
    detail: String,
}

#[derive(Clone)]
struct VisualLookupCard {
    title: String,
    category: String,
    confidence: String,
    description: String,
    follow_ups: Vec<String>,
    ready: bool,
}

fn main() {
    if let Err(detail) = run_visual_lookup(VisualLookupConfig::from_env()) {
        eprintln!("goblins-os-visual-lookup: {detail}");
    }
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_visual_lookup(config: VisualLookupConfig) -> VisualResult<()> {
    let _ = config.core_host.as_str();
    println!("goblins_os_visual_lookup=unavailable");
    println!("visual_lookup_reason=build_requires_linux_native_desktop_feature");
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_visual_lookup(config: VisualLookupConfig) -> VisualResult<()> {
    let card = visual_lookup_card(&config);
    native::show_card(card)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn visual_lookup_card(config: &VisualLookupConfig) -> VisualLookupCard {
    match vision_status(&config.core_host) {
        Ok(status) if !status.runtime_configured => VisualLookupCard {
            title: "Visual Look Up".to_string(),
            category: "Vision model required".to_string(),
            confidence: "No image captured".to_string(),
            description: status.detail,
            follow_ups: vec!["Open AI & Models".to_string()],
            ready: false,
        },
        Ok(_) => match capture_region() {
            Ok(path) => {
                let outcome = identify_image(&config.core_host, &path);
                let _ = fs::remove_file(&path);
                match outcome {
                    Ok(outcome) => card_from_outcome(outcome),
                    Err(detail) => {
                        failure_card("Visual Look Up", "Identification did not complete", &detail)
                    }
                }
            }
            Err(detail) => failure_card(
                "Visual Look Up",
                "Region capture did not complete",
                &format!(
                    "No pixels were sent to the model. Capture detail: {}",
                    sanitize_context_value(&detail, 220)
                ),
            ),
        },
        Err(detail) => failure_card("Visual Look Up", "Waiting for core", &detail),
    }
}

fn vision_status(core_host: &str) -> VisualResult<VisionStatus> {
    let body = http_request_local(core_host, "GET", "/v1/vision/status", None)?;
    serde_json::from_str(&body).map_err(|_| "Goblins OS returned unreadable vision status.".into())
}

fn identify_image(core_host: &str, image_path: &Path) -> VisualResult<VisualLookupOutcome> {
    let body = serde_json::json!({
        "image_path": image_path.to_string_lossy(),
        "hint": "User-invoked Visual Look Up region capture"
    })
    .to_string();
    let reply = http_request_local(core_host, "POST", "/v1/ai/visual-lookup", Some(&body))?;
    serde_json::from_str(&reply)
        .map_err(|_| "Goblins OS returned an unreadable Visual Look Up response.".into())
}

fn card_from_outcome(outcome: VisualLookupOutcome) -> VisualLookupCard {
    match (outcome.ok, outcome.identification) {
        (true, Some(card)) => VisualLookupCard {
            title: card_title(&card.name, &card.confidence),
            category: readable_category(&card.category),
            confidence: confidence_copy(&card.confidence),
            description: non_empty(card.description)
                .unwrap_or_else(|| "The vision model did not return a description.".to_string()),
            follow_ups: normalized_follow_ups(card.follow_ups),
            ready: true,
        },
        _ => failure_card(
            "Visual Look Up",
            "Waiting for vision model",
            &outcome.detail,
        ),
    }
}

fn failure_card(title: &str, category: &str, detail: &str) -> VisualLookupCard {
    VisualLookupCard {
        title: title.to_string(),
        category: category.to_string(),
        confidence: "Not identified".to_string(),
        description: sanitize_context_value(detail, 420),
        follow_ups: vec!["Open AI & Models".to_string()],
        ready: false,
    }
}

fn card_title(name: &str, confidence: &str) -> String {
    let name = non_empty(name.to_string()).unwrap_or_else(|| "Unidentified subject".to_string());
    if confidence.trim().eq_ignore_ascii_case("low") {
        format!("Best guess: {name}")
    } else {
        format!("Likely {name}")
    }
}

fn confidence_copy(confidence: &str) -> String {
    match confidence.trim().to_ascii_lowercase().as_str() {
        "high" => "High confidence".to_string(),
        "medium" => "Medium confidence".to_string(),
        "low" => "Best guess".to_string(),
        _ => "Confidence not reported".to_string(),
    }
}

fn readable_category(category: &str) -> String {
    match category.trim().to_ascii_lowercase().as_str() {
        "plant" => "Plant".to_string(),
        "animal" => "Animal".to_string(),
        "landmark" => "Landmark".to_string(),
        "artwork" => "Artwork".to_string(),
        "product" => "Product".to_string(),
        "food" => "Food".to_string(),
        "other" | "" => "Subject".to_string(),
        value => readable_token(value),
    }
}

fn normalized_follow_ups(values: Vec<String>) -> Vec<String> {
    let mut out = values
        .into_iter()
        .filter_map(non_empty)
        .map(|value| sanitize_context_value(&value, 42))
        .filter(|value| !value.is_empty())
        .take(3)
        .collect::<Vec<_>>();
    if out.is_empty() {
        out = vec![
            "Search the web".to_string(),
            "Ask Goblin about this".to_string(),
            "Copy name".to_string(),
        ];
    }
    out
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn readable_token(value: &str) -> String {
    value
        .split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn capture_region() -> VisualResult<PathBuf> {
    let dir = visual_lookup_dir()
        .map_err(|error| format!("could not prepare runtime folder: {error}"))?;
    let target = dir.join(format!("goblins-visual-lookup-{}.png", timestamp_millis()));

    let source = portal_screenshot(true)?;
    if !source.is_file() {
        return Err(format!(
            "the desktop screenshot portal reported {} but no file was found",
            source.display()
        ));
    }

    fs::copy(&source, &target)
        .map_err(|error| format!("could not copy captured region: {error}"))?;
    set_owner_only(&target)
        .map_err(|error| format!("could not lock down the captured region: {error}"))?;
    Ok(target)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn portal_screenshot(interactive: bool) -> VisualResult<PathBuf> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start the screenshot runtime: {error}"))?;

    let capture = async {
        let screenshot = Screenshot::request()
            .interactive(interactive)
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

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn visual_lookup_dir() -> io::Result<PathBuf> {
    let base = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set"))?;
    let dir = base.join("goblins-os").join("visual-lookup");
    fs::create_dir_all(&dir)?;
    set_private_permissions(&dir)?;
    Ok(dir)
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

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn portal_uri_to_path(uri: &str) -> VisualResult<PathBuf> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| format!("portal returned a non-file screenshot URI: {uri}"))?;
    let authority_trimmed = match rest.find('/') {
        Some(index) => &rest[index..],
        None => return Err(format!("portal returned a path-less screenshot URI: {uri}")),
    };
    let encoded_path = match authority_trimmed.find(['?', '#']) {
        Some(index) => &authority_trimmed[..index],
        None => authority_trimmed,
    };
    Ok(decode_uri_path(encoded_path))
}

#[cfg(unix)]
fn decode_uri_path(encoded: &str) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;

    let bytes = percent_encoding::percent_decode_str(encoded).collect::<Vec<u8>>();
    PathBuf::from(std::ffi::OsStr::from_bytes(&bytes))
}

#[cfg(not(unix))]
fn decode_uri_path(encoded: &str) -> PathBuf {
    let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8_lossy();
    PathBuf::from(decoded.as_ref())
}

fn http_request_local(
    core_host: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> VisualResult<String> {
    let mut stream = TcpStream::connect(core_host)
        .map_err(|_| "Goblins OS core is not ready for Visual Look Up.".to_string())?;
    stream.set_read_timeout(Some(HTTP_TIMEOUT)).ok();
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();

    let body = body.unwrap_or("");
    let request = if method == "POST" {
        format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    } else {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
    };
    stream
        .write_all(request.as_bytes())
        .map_err(|_| "Goblins OS core did not accept the Visual Look Up request.".to_string())?;
    let mut raw = Vec::new();
    stream
        .take(HTTP_MAX_BODY)
        .read_to_end(&mut raw)
        .map_err(|_| "Goblins OS core did not finish the Visual Look Up response.".to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let body_start = text
        .find("\r\n\r\n")
        .ok_or_else(|| "Goblins OS core returned an invalid HTTP response.".to_string())?
        + 4;
    Ok(text[body_start..].to_string())
}

fn loopback_http_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let host_port = if let Some(after_bracket) = rest.strip_prefix('[') {
        let (host, tail) = after_bracket.split_once(']')?;
        let port = tail.strip_prefix(':')?.split('/').next()?;
        (host.to_string(), port.to_string())
    } else {
        let before_path = rest.split('/').next().unwrap_or("");
        let (host, port) = before_path.split_once(':')?;
        (host.to_string(), port.to_string())
    };
    let (host, port) = host_port;
    let loopback = matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1");
    let numeric_port = port.parse::<u16>().ok()?;
    loopback.then(|| {
        if host == "::1" {
            format!("[::1]:{numeric_port}")
        } else {
            format!("{host}:{numeric_port}")
        }
    })
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

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
mod native {
    use std::process::{Command, Stdio};

    use gtk4::prelude::*;

    use super::{copy_to_clipboard, search_web, VisualLookupCard, VisualResult};

    const APP_ID: &str = "org.goblins.OS.VisualLookup";
    const SETTINGS_BIN: &str = "/usr/libexec/goblins-os/goblins-os-settings";
    const LAUNCHER_BIN: &str = "/usr/libexec/goblins-os/goblins-os-launcher";

    const CARD_CSS: &str = r#"
.gos-visual-root {
  padding: 18px;
}
.gos-visual-card {
  padding: 22px;
  min-width: 420px;
  border-radius: 22px;
  background: @gos_material_ultra_thick;
  border: 1px solid @gos_material_border;
  box-shadow: 0 1px 0 @gos_material_sheen inset,
              0 28px 72px @gos_material_shadow;
}
.gos-visual-eyebrow {
  color: @gos_ink_muted;
  font-size: 11px;
  font-weight: 700;
}
.gos-visual-title {
  color: @gos_ink;
  font-size: 17px;
  font-weight: 700;
}
.gos-visual-description {
  color: @gos_ink_secondary;
  font-size: 13px;
}
.gos-visual-chip {
  min-height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  color: @gos_ink;
  background: @gos_material_regular;
  border: 1px solid alpha(@gos_primary_border, 0.34);
}
.gos-visual-actions {
  margin-top: 4px;
}
.gos-visual-action {
  min-height: 38px;
}
"#;

    pub fn show_card(card: VisualLookupCard) -> VisualResult<()> {
        let app = gtk4::Application::builder().application_id(APP_ID).build();
        app.connect_activate(move |app| {
            goblins_os_ui::init_theming(CARD_CSS);
            build_window(app, &card);
        });
        app.run_with_args(&["goblins-os-visual-lookup"]);
        Ok(())
    }

    fn build_window(app: &gtk4::Application, card: &VisualLookupCard) {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("Visual Look Up")
            .decorated(false)
            .resizable(false)
            .default_width(472)
            .build();
        window.add_css_class("gos-window");
        window.add_css_class("gos-visual-root");

        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let panel = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
        panel.add_css_class("gos-visual-card");

        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        header.append(&goblins_os_ui::themed_brand_mark(18));
        let header_copy = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        header_copy.append(&label("Visual Look Up", &["gos-visual-eyebrow"]));
        header_copy.append(&label(&card.title, &["gos-visual-title"]));
        header.append(&header_copy);
        panel.append(&header);

        let detail = label(&card.description, &["gos-visual-description"]);
        detail.set_wrap(true);
        detail.set_xalign(0.0);
        panel.append(&detail);

        let chips = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        chips.append(&label(&card.category, &["gos-visual-chip"]));
        chips.append(&label(&card.confidence, &["gos-visual-chip"]));
        panel.append(&chips);

        let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        actions.add_css_class("gos-visual-actions");
        for follow_up in &card.follow_ups {
            let button = gtk4::Button::with_label(follow_up);
            button.add_css_class("gos-primary-button");
            button.add_css_class("gos-visual-action");
            wire_action(&button, follow_up, card);
            actions.append(&button);
        }
        panel.append(&actions);

        root.append(&panel);
        window.set_child(Some(&root));
        window.present();
    }

    fn wire_action(button: &gtk4::Button, action: &str, card: &VisualLookupCard) {
        let action_id = action.to_ascii_lowercase();
        let title = card.title.clone();
        let ready = card.ready;
        button.connect_clicked(move |_| {
            if action_id.contains("copy") {
                let _ = copy_to_clipboard(&title);
            } else if action_id.contains("search") && ready {
                let _ = search_web(&title);
            } else if action_id.contains("ask") && ready {
                let _ = Command::new(LAUNCHER_BIN)
                    .args(["--assistant"])
                    .env("GOBLINS_OS_LAUNCHER_PREFILL", format!("What is {title}?"))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            } else {
                let _ = Command::new(SETTINGS_BIN)
                    .arg("--panel=models")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        });
    }

    fn label(text: &str, classes: &[&str]) -> gtk4::Label {
        let label = gtk4::Label::new(Some(text));
        for class in classes {
            label.add_css_class(class);
        }
        label
    }
}

fn copy_to_clipboard(text: &str) -> bool {
    let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    child.wait().map(|status| status.success()).unwrap_or(false)
}

fn search_web(query: &str) -> bool {
    let encoded = query
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            value => format!("%{value:02X}").chars().collect(),
        })
        .collect::<String>();
    Command::new("xdg-open")
        .arg(format!("https://www.google.com/search?q={encoded}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_core_url_extracts_only_local_hosts() {
        assert_eq!(
            loopback_http_host("http://127.0.0.1:8787").as_deref(),
            Some("127.0.0.1:8787")
        );
        assert_eq!(
            loopback_http_host("http://localhost:8787/v1").as_deref(),
            Some("localhost:8787")
        );
        assert_eq!(
            loopback_http_host("http://[::1]:8787").as_deref(),
            Some("[::1]:8787")
        );
        assert!(loopback_http_host("http://10.0.0.2:8787").is_none());
        assert!(loopback_http_host("https://example.com:443").is_none());
        assert!(loopback_http_host("http://localhost").is_none());
    }

    #[test]
    fn low_confidence_card_uses_best_guess_copy() {
        assert_eq!(card_title("Red maple", "low"), "Best guess: Red maple");
        assert_eq!(card_title("Red maple", "high"), "Likely Red maple");
        assert_eq!(confidence_copy("low"), "Best guess");
    }

    #[test]
    fn categories_and_followups_are_normalized() {
        assert_eq!(readable_category("plant"), "Plant");
        assert_eq!(readable_category("historic_site"), "Historic Site");
        assert_eq!(
            normalized_follow_ups(vec!["\nCopy name\t".to_string()]),
            vec!["Copy name".to_string()]
        );
        assert_eq!(normalized_follow_ups(Vec::new()).len(), 3);
    }

    #[test]
    fn maps_portal_file_uri_to_private_path_source() {
        assert_eq!(
            portal_uri_to_path("file:///run/user/1000/doc/ab12/Screenshot%20from%202026.png")
                .unwrap(),
            PathBuf::from("/run/user/1000/doc/ab12/Screenshot from 2026.png")
        );
        assert!(portal_uri_to_path("https://example.test/x.png").is_err());
        assert_eq!(
            portal_uri_to_path("file://localhost/tmp/shot.png").unwrap(),
            PathBuf::from("/tmp/shot.png")
        );
    }

    #[test]
    fn sanitizes_failure_copy() {
        assert_eq!(sanitize_context_value(" a\n b\t c ", 8), "a b c");
        assert_eq!(sanitize_context_value("abcdefgh", 4), "abcd");
    }
}
