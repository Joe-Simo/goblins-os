//! Goblins OS Today — source-gated widgets panel.
//!
//! This crate owns the GTK surface for the Today view and consumes the existing
//! `/v1/today/status` substrate. It intentionally does not use GTK layer shell:
//! upstream gtk4-layer-shell documents that GNOME Wayland is unsupported, and
//! Goblins OS builds on GNOME rather than swapping the compositor contract. The
//! qemu pass can still prove a shell-owned edge/menu entry that launches this app.

#![cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code, unused_imports)
)]

use std::{
    env,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    process::Command,
    time::Duration,
};

use serde::Deserialize;

const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";
const CORE_URL_ENV: &str = "GOBLINS_OS_CORE_URL";
const LEGACY_CORE_URL_ENV: &str = "OPENAI_OS_CORE_URL";
const HTTP_MAX_BODY: u64 = 512 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_millis(900);

const FALLBACK_WIDGETS: &[(&str, &str, &str)] = &[
    ("date", "Date & agenda", "none"),
    ("world-clock", "World Clock", "none"),
    ("weather", "Weather", "location"),
    ("calendar", "Calendar", "calendar account"),
    ("brief", "Daily Brief", "on-device model"),
];

type TodayResult<T> = Result<T, String>;

#[derive(Clone)]
struct TodayConfig {
    core_url: String,
}

impl TodayConfig {
    fn from_env() -> Self {
        let core_url = env::var(CORE_URL_ENV)
            .or_else(|_| env::var(LEGACY_CORE_URL_ENV))
            .ok()
            .and_then(|value| loopback_http_url(&value))
            .unwrap_or_else(|| DEFAULT_CORE_URL.to_string());
        Self { core_url }
    }
}

#[derive(Clone, Deserialize)]
struct TodayStatus {
    #[allow(dead_code)]
    source: String,
    schema_available: bool,
    widgets: Vec<WidgetInfo>,
    layout: Vec<String>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct WidgetInfo {
    id: String,
    name: String,
    requires: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WidgetCard {
    id: String,
    title: String,
    value: String,
    detail: String,
    ready: bool,
}

#[derive(Clone)]
struct TodayViewModel {
    headline: String,
    detail: String,
    cards: Vec<WidgetCard>,
}

fn main() {
    if let Err(detail) = run_today(TodayConfig::from_env()) {
        eprintln!("goblins-os-today: {detail}");
    }
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_today(config: TodayConfig) -> TodayResult<()> {
    let _ = config.core_url.as_str();
    println!("goblins_os_today=unavailable");
    println!("today_reason=build_requires_linux_native_desktop_feature");
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_today(config: TodayConfig) -> TodayResult<()> {
    let status =
        fetch_today_status(&config.core_url).unwrap_or_else(|detail| unavailable_status(&detail));
    let view = view_model_from_status(&status, &local_date_label(), &local_time_label());
    native::show_today(view)
}

fn fetch_today_status(core_url: &str) -> TodayResult<TodayStatus> {
    let (status, body) = http_request(core_url, "GET", "/v1/today/status")?;
    if status != 200 {
        return Err(format!("Goblins OS core returned HTTP {status} for Today."));
    }
    serde_json::from_str(&body).map_err(|_| "Goblins OS returned unreadable Today status.".into())
}

fn unavailable_status(detail: &str) -> TodayStatus {
    TodayStatus {
        source: "goblins-os-today".to_string(),
        schema_available: false,
        widgets: fallback_widgets(),
        layout: FALLBACK_WIDGETS
            .iter()
            .map(|(id, _, _)| (*id).to_string())
            .collect(),
        detail: format!(
            "Waiting for Today preferences from Goblins OS core. {}",
            sanitize_copy(detail, 180)
        ),
    }
}

fn fallback_widgets() -> Vec<WidgetInfo> {
    FALLBACK_WIDGETS
        .iter()
        .map(|(id, name, requires)| WidgetInfo {
            id: (*id).to_string(),
            name: (*name).to_string(),
            requires: (*requires).to_string(),
        })
        .collect()
}

fn view_model_from_status(
    status: &TodayStatus,
    date_label: &str,
    time_label: &str,
) -> TodayViewModel {
    let cards = widget_cards(status, date_label, time_label);
    let headline = if status.schema_available {
        "Today".to_string()
    } else {
        "Today is using defaults".to_string()
    };
    TodayViewModel {
        headline,
        detail: sanitize_copy(&status.detail, 220),
        cards,
    }
}

fn widget_cards(status: &TodayStatus, date_label: &str, time_label: &str) -> Vec<WidgetCard> {
    let widgets = if status.widgets.is_empty() {
        fallback_widgets()
    } else {
        status.widgets.clone()
    };
    let ordered = normalize_layout(&status.layout, &widgets);
    ordered
        .into_iter()
        .filter_map(|id| widgets.iter().find(|widget| widget.id == id))
        .map(|widget| widget_card(widget, date_label, time_label))
        .collect()
}

fn normalize_layout(layout: &[String], widgets: &[WidgetInfo]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let known = widgets
        .iter()
        .map(|widget| widget.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let ordered = layout
        .iter()
        .filter(|id| known.contains(id.as_str()))
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect::<Vec<_>>();
    if ordered.is_empty() {
        widgets.iter().map(|widget| widget.id.clone()).collect()
    } else {
        ordered
    }
}

fn widget_card(widget: &WidgetInfo, date_label: &str, time_label: &str) -> WidgetCard {
    match widget.id.as_str() {
        "date" => WidgetCard {
            id: widget.id.clone(),
            title: widget.name.clone(),
            value: non_empty(date_label).unwrap_or_else(|| "Today".to_string()),
            detail: "Calendar events appear here after an account is connected.".to_string(),
            ready: true,
        },
        "world-clock" => WidgetCard {
            id: widget.id.clone(),
            title: widget.name.clone(),
            value: non_empty(time_label).unwrap_or_else(|| "Local time".to_string()),
            detail: "Showing this device's local clock. More cities are a later layout edit."
                .to_string(),
            ready: true,
        },
        "weather" => WidgetCard {
            id: widget.id.clone(),
            title: widget.name.clone(),
            value: "Location needed".to_string(),
            detail:
                "Weather stays empty until location services and a weather source are available."
                    .to_string(),
            ready: false,
        },
        "calendar" => WidgetCard {
            id: widget.id.clone(),
            title: widget.name.clone(),
            value: "No calendar account".to_string(),
            detail: "Connect a calendar account before Today can show upcoming events.".to_string(),
            ready: false,
        },
        "brief" => WidgetCard {
            id: widget.id.clone(),
            title: widget.name.clone(),
            value: "Local model required".to_string(),
            detail: "Add a local model before Today can create an on-device daily brief."
                .to_string(),
            ready: false,
        },
        _ => WidgetCard {
            id: widget.id.clone(),
            title: widget.name.clone(),
            value: if widget.requires == "none" {
                "Ready".to_string()
            } else {
                "Source needed".to_string()
            },
            detail: if widget.requires == "none" {
                "This widget is ready to render.".to_string()
            } else {
                format!(
                    "This widget needs {} before it can show live data.",
                    widget.requires
                )
            },
            ready: widget.requires == "none",
        },
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn local_date_label() -> String {
    command_text("date", &["+%A, %B %-d"]).unwrap_or_else(|| "Today".to_string())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn local_time_label() -> String {
    command_text("date", &["+%-I:%M %p"]).unwrap_or_else(|| "Local time".to_string())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn command_text(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .and_then(|value| non_empty(&value))
}

fn http_request(core_url: &str, method: &str, path: &str) -> TodayResult<(u16, String)> {
    let rest = core_url
        .strip_prefix("http://")
        .ok_or_else(|| "Today only connects to the local Goblins OS core.".to_string())?;
    let host_port = rest.split('/').next().unwrap_or("");
    let mut addrs = host_port
        .to_socket_addrs()
        .map_err(|_| "Goblins OS core is not ready for Today.".to_string())?;
    let address = addrs
        .next()
        .ok_or_else(|| "Goblins OS core is not ready for Today.".to_string())?;
    let mut stream = TcpStream::connect_timeout(&address, HTTP_TIMEOUT)
        .map_err(|_| "Goblins OS core is not ready for Today.".to_string())?;
    stream.set_read_timeout(Some(HTTP_TIMEOUT)).ok();
    stream.set_write_timeout(Some(HTTP_TIMEOUT)).ok();
    let request =
        format!("{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|_| "Goblins OS core did not accept the Today request.".to_string())?;

    let mut raw = Vec::new();
    stream
        .take(HTTP_MAX_BODY)
        .read_to_end(&mut raw)
        .map_err(|_| "Goblins OS core did not finish the Today response.".to_string())?;
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> TodayResult<(u16, String)> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "Goblins OS core returned an invalid Today response.".to_string())?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "Goblins OS core returned an invalid Today status.".to_string())?;
    Ok((status, body.to_string()))
}

fn loopback_http_url(url: &str) -> Option<String> {
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
            format!("http://[::1]:{numeric_port}")
        } else {
            format!("http://{host}:{numeric_port}")
        }
    })
}

fn sanitize_copy(value: &str, max_chars: usize) -> String {
    value
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
mod native {
    use gtk4::prelude::*;

    use super::{TodayResult, TodayViewModel, WidgetCard};

    const APP_ID: &str = "org.goblins.OS.Today";

    const TODAY_CSS: &str = r#"
.gos-today-window {
  padding: 16px;
}
.gos-today-panel {
  min-width: 380px;
  padding: 20px;
  border-radius: 22px;
  background: @gos_material_ultra_thick;
  border: 1px solid @gos_material_border;
  box-shadow: 0 1px 0 @gos_material_sheen inset,
              0 28px 72px @gos_material_shadow;
}
.gos-today-title {
  color: @gos_ink;
  font-size: 18px;
  font-weight: 700;
}
.gos-today-detail {
  color: @gos_ink_secondary;
  font-size: 13px;
}
.gos-today-card {
  padding: 14px;
  border-radius: 14px;
  background: @gos_material_regular;
  border: 1px solid @gos_material_border;
}
.gos-today-card.is-empty {
  background: @gos_surface_sunken;
  border-color: @gos_hairline;
}
.gos-today-card-title {
  color: @gos_ink_muted;
  font-size: 11px;
  font-weight: 700;
}
.gos-today-card-value {
  color: @gos_ink;
  font-size: 15px;
  font-weight: 700;
}
.gos-today-card-detail {
  color: @gos_ink_secondary;
  font-size: 12px;
}
"#;

    pub fn show_today(view: TodayViewModel) -> TodayResult<()> {
        let app = gtk4::Application::builder().application_id(APP_ID).build();
        app.connect_activate(move |app| {
            goblins_os_ui::init_theming(TODAY_CSS);
            build_window(app, &view);
        });
        app.run_with_args(&["goblins-os-today"]);
        Ok(())
    }

    fn build_window(app: &gtk4::Application, view: &TodayViewModel) {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("Today")
            .decorated(false)
            .resizable(false)
            .default_width(430)
            .default_height(640)
            .build();
        window.add_css_class("gos-window");
        window.add_css_class("gos-today-window");

        let panel = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
        panel.add_css_class("gos-today-panel");

        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        header.append(&goblins_os_ui::themed_brand_mark(18));
        let header_copy = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
        header_copy.append(&label(&view.headline, &["gos-today-title"]));
        let detail = label(&view.detail, &["gos-today-detail"]);
        detail.set_wrap(true);
        header_copy.append(&detail);
        header.append(&header_copy);
        panel.append(&header);

        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        for card in &view.cards {
            list.append(&widget_card(card));
        }
        panel.append(&list);

        window.set_child(Some(&panel));
        window.present();
    }

    fn widget_card(card: &WidgetCard) -> gtk4::Box {
        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 5);
        root.add_css_class("gos-today-card");
        if !card.ready {
            root.add_css_class("is-empty");
        }
        root.append(&label(&card.title, &["gos-today-card-title"]));
        root.append(&label(&card.value, &["gos-today-card-value"]));
        let detail = label(&card.detail, &["gos-today-card-detail"]);
        detail.set_wrap(true);
        root.append(&detail);
        root
    }

    fn label(text: &str, classes: &[&str]) -> gtk4::Label {
        let label = gtk4::Label::new(Some(text));
        label.set_xalign(0.0);
        for class in classes {
            label.add_css_class(class);
        }
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(layout: Vec<&str>) -> TodayStatus {
        TodayStatus {
            source: "goblins-os-core".to_string(),
            schema_available: true,
            widgets: fallback_widgets(),
            layout: layout.into_iter().map(str::to_string).collect(),
            detail: "Today widgets are configured.".to_string(),
        }
    }

    #[test]
    fn loopback_core_urls_are_required() {
        assert_eq!(
            loopback_http_url("http://127.0.0.1:8787").as_deref(),
            Some("http://127.0.0.1:8787")
        );
        assert_eq!(
            loopback_http_url("http://localhost:8787/v1").as_deref(),
            Some("http://localhost:8787")
        );
        assert_eq!(
            loopback_http_url("http://[::1]:8787").as_deref(),
            Some("http://[::1]:8787")
        );
        assert!(loopback_http_url("https://localhost:8787").is_none());
        assert!(loopback_http_url("http://example.com:8787").is_none());
        assert!(loopback_http_url("http://localhost").is_none());
    }

    #[test]
    fn layout_keeps_known_widgets_once_and_falls_back_when_empty() {
        let widgets = fallback_widgets();
        assert_eq!(
            normalize_layout(
                &[
                    "weather".to_string(),
                    "bogus".to_string(),
                    "weather".to_string(),
                    "date".to_string(),
                ],
                &widgets,
            ),
            vec!["weather".to_string(), "date".to_string()]
        );
        assert_eq!(normalize_layout(&[], &widgets).len(), widgets.len());
    }

    #[test]
    fn unavailable_widgets_have_honest_empty_states() {
        let cards = widget_cards(
            &status(vec!["weather", "calendar", "brief"]),
            "Saturday, June 27",
            "9:41 AM",
        );
        assert_eq!(cards.len(), 3);
        assert!(cards.iter().all(|card| !card.ready));
        assert!(cards[0].detail.contains("location services"));
        assert!(cards[1].detail.contains("calendar account"));
        assert!(cards[2].detail.contains("local model"));
    }

    #[test]
    fn local_date_and_clock_cards_are_ready_without_remote_data() {
        let cards = widget_cards(
            &status(vec!["date", "world-clock"]),
            "Saturday, June 27",
            "9:41 AM",
        );
        assert_eq!(cards[0].value, "Saturday, June 27");
        assert_eq!(cards[1].value, "9:41 AM");
        assert!(cards.iter().all(|card| card.ready));
    }

    #[test]
    fn parses_http_status_and_body() {
        let (status, body) =
            parse_http_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, "{}");
        assert!(parse_http_response(b"not http").is_err());
    }

    #[test]
    fn sanitizes_multiline_core_detail() {
        assert_eq!(sanitize_copy(" a\n b\t c ", 20), "a b c");
        assert_eq!(sanitize_copy("abcdef", 4), "abcd");
    }
}
