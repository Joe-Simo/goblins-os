//! The Goblins OS control center — a bespoke, all-Rust GTK4 glass panel the menu
//! bar opens in the Goblins-native language.
//!
//! It gathers the controls reached for without opening Settings: the Light/Dark
//! scheme, AI mode, Wi-Fi, and sound/brightness.
//! Every control drives a REAL backend — the desktop color-scheme key, the OS
//! core's engine endpoint, NetworkManager, WirePlumber, and the backlight — and a
//! control whose backend is absent (no audio sink, no backlight in a VM) recedes
//! to a disabled state with an honest reason, never an invented value. Outside a Linux
//! native-desktop build the crate degrades to a one-line status print.

use std::{env, error::Error};

const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";

type ControlResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone)]
struct ControlConfig {
    core_url: String,
}

impl ControlConfig {
    fn from_env() -> Self {
        Self {
            core_url: env::var("GOBLINS_OS_CORE_URL")
                .or_else(|_| env::var("OPENAI_OS_CORE_URL"))
                .unwrap_or_else(|_| DEFAULT_CORE_URL.into()),
        }
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
#[derive(Clone, Debug, serde::Deserialize, PartialEq, Eq)]
struct FocusStatus {
    available: bool,
    active_mode: String,
    scheduled_mode: Option<String>,
    armed_by_schedule: bool,
    modes: Vec<FocusMode>,
    detail: String,
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
#[derive(Clone, Debug, serde::Deserialize, PartialEq, Eq)]
struct FocusMode {
    id: String,
    name: String,
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct FocusTileCopy {
    state: String,
    detail: String,
    active: bool,
    opens_settings: bool,
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn focus_tile_copy(status: Option<&FocusStatus>) -> FocusTileCopy {
    let Some(status) = status else {
        return FocusTileCopy {
            state: "Unavailable".to_string(),
            detail: "Focus status is unavailable because Goblins OS core did not respond."
                .to_string(),
            active: false,
            opens_settings: false,
        };
    };

    if !status.available {
        return FocusTileCopy {
            state: "Unavailable".to_string(),
            detail: if status.detail.trim().is_empty() {
                "Focus is unavailable in this session.".to_string()
            } else {
                status.detail.clone()
            },
            active: false,
            opens_settings: false,
        };
    }

    let active_mode = status.active_mode.trim();
    if active_mode.is_empty() {
        let detail = if status.modes.is_empty() {
            "Focus is off. No Focus modes are configured yet.".to_string()
        } else if let Some(name) = status
            .scheduled_mode
            .as_deref()
            .and_then(|id| focus_mode_name(&status.modes, id))
        {
            format!("Focus is off. {name} matches the current schedule.")
        } else if status.detail.trim().is_empty() {
            "Focus is off.".to_string()
        } else {
            status.detail.clone()
        };
        return FocusTileCopy {
            state: "Off".to_string(),
            detail: format!("{detail} Open Settings to manage Focus."),
            active: false,
            opens_settings: true,
        };
    }

    match focus_mode_name(&status.modes, active_mode) {
        Some(name) => FocusTileCopy {
            state: name.to_string(),
            detail: if status.armed_by_schedule {
                format!("{name} is active from a schedule. Open Settings to change Focus.")
            } else {
                format!("{name} is active. Open Settings to change Focus.")
            },
            active: true,
            opens_settings: true,
        },
        None => FocusTileCopy {
            state: "Unknown mode".to_string(),
            detail: format!(
                "Focus reports active mode '{active_mode}', but that mode is not in the configured Focus list."
            ),
            active: true,
            opens_settings: true,
        },
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn focus_mode_name<'a>(modes: &'a [FocusMode], id: &str) -> Option<&'a str> {
    modes
        .iter()
        .find(|mode| mode.id == id)
        .map(|mode| mode.name.as_str())
}

fn main() -> ControlResult<()> {
    run_control_center(ControlConfig::from_env())
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_control_center(config: ControlConfig) -> ControlResult<()> {
    let _ = config.core_url.as_str();
    println!("goblins_os_control_center=unavailable");
    println!("control_center_reason=build_requires_linux_native_desktop_feature");
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use native::run_control_center;

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
mod native {
    use std::{
        cell::Cell,
        io::{Read, Write},
        net::{TcpStream, ToSocketAddrs},
        process::Command,
        rc::Rc,
        time::Duration,
    };

    use gtk::gdk;
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use serde::Deserialize;

    use super::{focus_tile_copy, ControlConfig, ControlResult, FocusStatus};

    const APP_ID: &str = "org.goblins.OS.ControlCenter";
    const MAX_BODY_BYTES: u64 = 256 * 1024;

    // Panel-local refinements layered over the shared design tokens. These don't
    // invent new color — they reach for the SAME `@gos_*` tokens the design crate
    // exports and only re-rank weight, so the panel reads as one calm system in
    // both schemes:
    //  · a status tile (Wi-Fi) drops the raised/hover affordance of a selectable
    //    tile and sinks into a flat inset, so what's tappable (Appearance) is legible;
    //  · the selected-tile ring comes down to the panel's muted accent weight
    //    (matching the segmented control) instead of the full-strength border —
    //    in dark `@gos_primary_border` is a bright sky-blue, so the heavy ring was
    //    the loudest accent on the surface;
    //  · the AI action chips lift onto a raised fill with a clearer border so they
    //    don't sink into the graphite in dark, and the `AI Settings…` destination
    //    is demoted to a quiet full-width row, distinct from the action verbs.
    const PANEL_CSS: &str = r#"
/* Selected tile (Appearance · Dark) — calm it to the segmented control's accent
   weight: one muted inset ring, no full-strength border that shouts in dark. */
.gos-cc-root .gos-cc-tile.is-on {
  border-color: alpha(@gos_primary_border, 0.42);
  box-shadow: 0 1px 0 alpha(@gos_material_sheen, 0.46) inset,
              inset 0 0 0 1px alpha(@gos_primary_border, 0.34);
}

/* Status tile (Wi-Fi) — a read-out, not a selectable card. Sink it into a flat
   inset with no shadow lift and no hover affordance, so the eye reads it as
   information rather than a control to tap. */
.gos-cc-root .gos-cc-tile.gos-cc-tile-status {
  background: @gos_surface_sunken;
  border-color: transparent;
  box-shadow: inset 0 0 0 1px alpha(@gos_material_sheen, 0.18);
}
.gos-cc-root .gos-cc-tile-status:hover { background: @gos_surface_sunken; }

/* AI action chips — lift onto the raised control fill with a clearer border so
   the six-up grid reads with real figure/ground in dark instead of melting into
   the panel graphite. */
.gos-cc-root .gos-cc-action {
  background-color: @gos_control_raised;
  border-color: @gos_hairline_strong;
}
.gos-cc-root .gos-cc-action:hover { background-color: @gos_material_hover; }

/* `AI Settings…` is a destination, not an action verb — demote it from the chip
   grid to a quiet full-width row so it never reads as a sixth launcher. Both the
   resting and hover selectors carry the full `.gos-cc-action.gos-cc-action-settings`
   qualifier so each out-ranks the raised-chip `.gos-cc-root .gos-cc-action[:hover]`
   rule above on specificity — otherwise the hover tie left the chip fill showing
   and the demotion never landed. */
.gos-cc-root .gos-cc-action.gos-cc-action-settings {
  background-color: transparent;
  border-color: transparent;
  box-shadow: none;
  color: @gos_ink_muted;
  font-weight: 600;
}
.gos-cc-root .gos-cc-action.gos-cc-action-settings:hover {
  background-color: @gos_surface_sunken;
  color: @gos_ink;
}
"#;
    const SINK: &str = "@DEFAULT_AUDIO_SINK@";
    const LAUNCHER_BIN: &str = "/usr/libexec/goblins-os/goblins-os-launcher";
    const SCREENSHOT_CONTEXT_BIN: &str = "/usr/libexec/goblins-os/goblins-os-screenshot-context";

    #[derive(Deserialize)]
    struct AiActionCatalog {
        actions: Vec<AiActionStatus>,
    }

    #[derive(Deserialize)]
    struct AiActionStatus {
        id: String,
        enabled: bool,
        reason: String,
    }

    struct AiActionAvailability {
        enabled: bool,
        reason: String,
    }

    pub fn run_control_center(config: ControlConfig) -> ControlResult<()> {
        let app = gtk::Application::builder().application_id(APP_ID).build();
        app.connect_activate(move |app| {
            goblins_os_ui::init_theming(PANEL_CSS);
            build_window(app, &config);
        });
        app.run_with_args(&["goblins-os-control-center"]);
        Ok(())
    }

    fn build_window(app: &gtk::Application, config: &ControlConfig) {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Goblins OS Control Center")
            .decorated(false)
            .resizable(false)
            .default_width(360)
            .build();
        window.add_css_class("gos-cc-root");
        window.add_css_class("gos-window");

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("gos-cc");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.append(&goblins_os_ui::themed_brand_mark(16));
        let title = gtk::Label::new(Some("Control Center"));
        title.add_css_class("gos-cc-title");
        title.set_xalign(0.0);
        header.append(&title);
        card.append(&header);

        // ── Quick tiles: Wi-Fi · Dark Mode ──
        card.append(&section("Connection & Appearance"));
        let tiles = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        tiles.set_homogeneous(true);
        tiles.append(&wifi_tile());
        tiles.append(&scheme_tile());
        card.append(&tiles);

        // ── Focus ──
        card.append(&section("Focus"));
        card.append(&focus_tile(&config.core_url, &window));

        // ── AI mode ──
        card.append(&section("AI Mode"));
        card.append(&engine_switch(&config.core_url));

        // ── Goblins AI entry points ──
        card.append(&section("Goblins AI"));
        let ask_availability = ai_action_availability(&config.core_url, "ask-goblins");
        let selected_text_availability =
            ai_action_availability(&config.core_url, "ask-selected-text");
        let writing_tools_availability =
            ai_action_availability(&config.core_url, "write-with-goblins");
        let screen_context_availability =
            ai_action_availability(&config.core_url, "summarize-screen");
        let ai_actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let ai_primary = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        ai_primary.set_homogeneous(true);
        ai_primary.append(&ai_action_button(
            "Ask Goblin…",
            &["--assistant"],
            &ask_availability,
            &window,
        ));
        ai_primary.append(&ai_action_button(
            "Write…",
            &["--writing-tools"],
            &writing_tools_availability,
            &window,
        ));
        let ai_context = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        ai_context.set_homogeneous(true);
        ai_context.append(&ai_action_button(
            "Selected Text…",
            &["--selected-text"],
            &selected_text_availability,
            &window,
        ));
        ai_context.append(&ai_action_button(
            "Screen Context…",
            &["--screen-context"],
            &screen_context_availability,
            &window,
        ));
        // The fifth (odd) action verb stays ON the two-up grid rather than
        // stretching full-width: it occupies the left column at chip width while
        // the right column is held open by an empty filler, so all five verbs
        // share one alignment grammar instead of one lone full-bleed chip.
        let ai_extra = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        ai_extra.set_homogeneous(true);
        let screenshot_btn = ai_tool_button(
            "Screenshot…",
            SCREENSHOT_CONTEXT_BIN,
            &[],
            &screen_context_availability,
            &window,
        );
        ai_extra.append(&screenshot_btn);
        // An empty second cell keeps Screenshot at the grid's column width (the
        // homogeneous row splits 50/50) without inventing a sixth control.
        ai_extra.append(&gtk::Box::new(gtk::Orientation::Horizontal, 0));
        // `AI Settings…` is a destination, not an action verb. Demote it to a quiet
        // full-width row below the verbs so it never reads as a sixth launcher.
        let ai_settings_btn = tool_button(
            "AI Settings…",
            "/usr/libexec/goblins-os/goblins-os-settings",
            &["--panel=models"],
            &window,
        );
        style_ai_action(&ai_settings_btn);
        ai_settings_btn.add_css_class("gos-cc-action-settings");
        ai_settings_btn.set_hexpand(true);
        ai_actions.append(&ai_primary);
        ai_actions.append(&ai_context);
        ai_actions.append(&ai_extra);
        ai_actions.append(&ai_settings_btn);
        card.append(&ai_actions);
        if let Some(reason) = first_unavailable_ai_reason(&[
            &ask_availability,
            &writing_tools_availability,
            &selected_text_availability,
            &screen_context_availability,
        ]) {
            let note = gtk::Label::new(Some(reason));
            note.add_css_class("gos-cc-note");
            note.set_xalign(0.0);
            note.set_wrap(true);
            card.append(&note);
        }

        // ── Sound ──
        card.append(&section("Sound"));
        card.append(&slider_row(
            "Volume",
            "audio-volume-high-symbolic",
            volume_percent(),
            |value| {
                let _ = run(
                    "wpctl",
                    &["set-volume", SINK, &format!("{:.2}", value / 100.0)],
                );
            },
            "No audio output device.",
        ));

        // ── Display brightness ──
        card.append(&section("Display"));
        card.append(&slider_row(
            "Display brightness",
            "display-brightness-symbolic",
            brightness_percent(),
            |value| {
                let _ = run(
                    "brightnessctl",
                    &[
                        "-c",
                        "backlight",
                        "set",
                        &format!("{}%", value.round() as i64),
                    ],
                );
            },
            "No adjustable display.",
        ));

        let settings = gtk::Button::with_label("Open Settings…");
        settings.add_css_class("gos-cc-link");
        settings.set_halign(gtk::Align::Start);
        settings.set_margin_top(8);
        settings.set_tooltip_text(Some("Open Settings"));
        set_accessible_label_description(
            &settings,
            "Open Settings",
            "Open Settings and close Control Center",
        );
        {
            let weak = window.downgrade();
            settings.connect_clicked(move |_| {
                let _ = Command::new("/usr/libexec/goblins-os/goblins-os-settings").spawn();
                if let Some(win) = weak.upgrade() {
                    win.close();
                }
            });
        }
        card.append(&settings);

        // Wrap the card in a real GSK backdrop-blur vibrancy material: the blurred
        // wallpaper the compositor cannot give an isolated surface, drawn in-process
        // from the shipped wallpaper so it survives the headless software renderer.
        let backdrop = goblins_os_ui::VibrancyBackdrop::new(goblins_os_ui::resolve_dark(), &card);
        window.set_child(Some(&backdrop));

        // Escape dismisses; losing focus dismisses (a real popover) unless the
        // render harness pins the only captured window on screen.
        let keys = gtk::EventControllerKey::new();
        {
            let weak = window.downgrade();
            keys.connect_key_pressed(move |_, key, _code, _state| {
                if key == gdk::Key::Escape {
                    if let Some(win) = weak.upgrade() {
                        win.close();
                    }
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        window.add_controller(keys);
        if std::env::var("GOBLINS_OS_RENDER_HOLD_WINDOW").is_err() {
            let weak = window.downgrade();
            window.connect_is_active_notify(move |win| {
                if !win.is_active() {
                    if let Some(win) = weak.upgrade() {
                        win.close();
                    }
                }
            });
        }

        present_with_fade(&window);
    }

    fn section(text: &str) -> gtk::Label {
        let label = gtk::Label::new(Some(text));
        label.add_css_class("gos-cc-section");
        label.set_xalign(0.0);
        set_accessible_label_description(&label, text, text);
        label
    }

    /// A toggle tile (symbolic icon + label + state), accented when on.
    fn make_tile(
        icon_name: &str,
        name: &str,
        state_text: &str,
        on: bool,
    ) -> (gtk::Button, gtk::Label) {
        let button = gtk::Button::new();
        button.add_css_class("gos-cc-tile");
        update_tile_accessibility(&button, name, state_text);
        if on {
            button.add_css_class("is-on");
        }
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let glyph_label = icon(icon_name, "gos-cc-tile-glyph", 22);
        glyph_label.set_halign(gtk::Align::Start);
        let name_label = gtk::Label::new(Some(name));
        name_label.add_css_class("gos-cc-tile-label");
        name_label.set_xalign(0.0);
        let state = gtk::Label::new(Some(state_text));
        state.add_css_class("gos-cc-tile-state");
        state.set_xalign(0.0);
        inner.append(&glyph_label);
        inner.append(&name_label);
        inner.append(&state);
        button.set_child(Some(&inner));
        (button, state)
    }

    fn update_tile_accessibility(button: &gtk::Button, name: &str, state_text: &str) {
        let description = format!("{name}: {state_text}");
        button.set_tooltip_text(Some(&description));
        set_accessible_label_description(button, name, &description);
    }

    fn scheme_tile() -> gtk::Button {
        let dark = goblins_os_ui::resolve_dark();
        let (tile, state) = make_tile(
            "preferences-desktop-appearance-symbolic",
            "Appearance",
            if dark { "Dark" } else { "Light" },
            dark,
        );
        let state_weak = state.downgrade();
        let tile_weak = tile.downgrade();
        tile.connect_clicked(move |_| {
            let now_dark = goblins_os_ui::resolve_dark();
            let next = if now_dark { "default" } else { "prefer-dark" };
            if goblins_os_ui::set_color_scheme(next) {
                if let (Some(state), Some(tile)) = (state_weak.upgrade(), tile_weak.upgrade()) {
                    let dark = next == "prefer-dark";
                    state.set_text(if dark { "Dark" } else { "Light" });
                    update_tile_accessibility(
                        &tile,
                        "Appearance",
                        if dark { "Dark" } else { "Light" },
                    );
                    if dark {
                        tile.add_css_class("is-on");
                    } else {
                        tile.remove_css_class("is-on");
                    }
                }
            }
        });
        tile
    }

    fn focus_tile(core_url: &str, window: &gtk::ApplicationWindow) -> gtk::Button {
        let status = focus_status(core_url);
        let copy = focus_tile_copy(status.as_ref());
        let (tile, _state) = make_tile(
            "preferences-system-notifications-symbolic",
            "Focus",
            &copy.state,
            copy.active,
        );
        tile.set_hexpand(true);
        tile.set_halign(gtk::Align::Fill);
        update_tile_accessibility(&tile, "Focus", &copy.detail);
        if !copy.opens_settings {
            tile.set_sensitive(false);
            return tile;
        }

        let weak = window.downgrade();
        tile.connect_clicked(move |_| {
            let _ = Command::new("/usr/libexec/goblins-os/goblins-os-settings")
                .arg("--panel=notifications")
                .spawn();
            if let Some(win) = weak.upgrade() {
                win.close();
            }
        });
        tile
    }

    fn wifi_tile() -> gtk::Button {
        let enabled = wifi_enabled();
        let on = enabled.unwrap_or(false);
        let state_text = match (enabled, wifi_name()) {
            (Some(true), Some(name)) if !name.is_empty() => name,
            (Some(true), _) => "On".to_string(),
            (Some(false), _) => "Off".to_string(),
            (None, _) => "Unavailable in this session".to_string(),
        };
        let (tile, state) = make_tile("network-wireless-symbolic", "Wi-Fi", &state_text, on);
        // Wi-Fi is a status read-out sitting beside the selectable Appearance tile.
        // Mark it as a status surface so it sinks into a flat inset and sheds the
        // raised/hover affordance — the eye should read it as information, and reach
        // for Appearance as the tappable control.
        tile.add_css_class("gos-cc-tile-status");
        if enabled.is_none() {
            update_tile_accessibility(&tile, "Wi-Fi", "Unavailable in this session");
            tile.set_sensitive(false);
            return tile;
        }
        let state_weak = state.downgrade();
        let tile_weak = tile.downgrade();
        tile.connect_clicked(move |_| {
            let now_on = wifi_enabled().unwrap_or(false);
            let target = if now_on { "off" } else { "on" };
            let _ = run("nmcli", &["radio", "wifi", target]);
            if let (Some(state), Some(tile)) = (state_weak.upgrade(), tile_weak.upgrade()) {
                let on = target == "on";
                let label = if on {
                    wifi_name()
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| "On".to_string())
                } else {
                    "Off".to_string()
                };
                state.set_text(&label);
                update_tile_accessibility(&tile, "Wi-Fi", &label);
                if on {
                    tile.add_css_class("is-on");
                } else {
                    tile.remove_css_class("is-on");
                }
            }
        });
        tile
    }

    /// A two-segment AI mode switch. The active segment reflects the OS core's
    /// current mode; clicking a segment posts the switch (the core validates
    /// account requirements, and only a 2xx moves the highlight).
    fn engine_switch(core_url: &str) -> gtk::Box {
        let current = current_engine(core_url);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("gos-cc-engine");
        row.set_homogeneous(true);

        let local = gtk::Button::with_label("On-device · GPT-OSS");
        local.add_css_class("gos-cc-seg");
        let codex = gtk::Button::with_label("OpenAI · Codex");
        codex.add_css_class("gos-cc-seg");
        // Honest three-way: highlight on-device only for local-gpt-oss, codex only
        // for codex, and neither for the hosted openai-api engine or an unreachable
        // core (None) — never optimistically claim local.
        if current.as_deref() == Some("local-gpt-oss") {
            local.add_css_class("is-active");
        } else if current.as_deref() == Some("codex") {
            codex.add_css_class("is-active");
        }
        update_engine_accessibility(&local, &codex, current.as_deref());

        let wire =
            |button: &gtk::Button, engine: &'static str, sibling: &gtk::Button, url: String| {
                let button_weak = button.downgrade();
                let sibling_weak = sibling.downgrade();
                button.connect_clicked(move |_| {
                    if set_engine(&url, engine) {
                        if let (Some(button), Some(sibling)) =
                            (button_weak.upgrade(), sibling_weak.upgrade())
                        {
                            button.add_css_class("is-active");
                            sibling.remove_css_class("is-active");
                            if engine == "codex" {
                                update_engine_accessibility(&sibling, &button, Some(engine));
                            } else {
                                update_engine_accessibility(&button, &sibling, Some(engine));
                            }
                        }
                    }
                });
            };
        wire(&local, "local-gpt-oss", &codex, core_url.to_string());
        wire(&codex, "codex", &local, core_url.to_string());

        row.append(&local);
        row.append(&codex);
        row
    }

    fn update_engine_accessibility(
        local: &gtk::Button,
        codex: &gtk::Button,
        selected: Option<&str>,
    ) {
        // Each segment is "current" only for its own engine; openai-api / None
        // (unreachable core) report neither segment as current.
        let local_current = selected == Some("local-gpt-oss");
        let codex_current = selected == Some("codex");
        set_segment_accessibility(
            local,
            "Use on-device GPT-OSS",
            "on-device GPT-OSS",
            local_current,
        );
        set_segment_accessibility(codex, "Use OpenAI Codex", "OpenAI Codex", codex_current);
    }

    fn set_segment_accessibility(
        button: &gtk::Button,
        label: &str,
        engine_name: &str,
        current: bool,
    ) {
        let description = if current {
            format!("Current build engine: {engine_name}")
        } else {
            format!("Switch build engine to {engine_name}")
        };
        button.set_tooltip_text(Some(&description));
        set_accessible_label_description(button, label, &description);
    }

    /// A labeled slider row. When the backend value is None, the row reads as
    /// disabled with an honest reason rather than presenting an invented position.
    fn slider_row(
        label: &str,
        icon_name: &str,
        value: Option<f64>,
        on_change: impl Fn(f64) + 'static,
        unavailable: &str,
    ) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
        row.add_css_class("gos-cc-slider-row");

        let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let glyph_label = icon(icon_name, "gos-cc-slider-glyph", 18);
        glyph_label.set_valign(gtk::Align::Center);
        set_accessible_label_description(&glyph_label, label, label);
        line.append(&glyph_label);

        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_hexpand(true);
        scale.set_draw_value(false);
        scale.set_tooltip_text(Some(label));
        match value {
            Some(value) => {
                // Set the position BEFORE wiring the signal so opening the panel
                // never re-issues the current value back to the device.
                scale.set_value(value);
                let description = percent_description(label, value);
                set_accessible_label_description(&scale, label, &description);
                let label_text = label.to_string();
                // GtkScale fires value-changed continuously while dragging. Update
                // the accessible description synchronously (cheap, in-process), but
                // coalesce the backend subprocess through a trailing-edge ~50ms
                // timer so at most one call is in flight and the settled value
                // always applies.
                let on_change = Rc::new(on_change);
                let latest = Rc::new(Cell::new(value));
                let pending = Rc::new(Cell::new(false));
                scale.connect_value_changed(move |scale| {
                    let value = scale.value();
                    let description = percent_description(&label_text, value);
                    set_accessible_label_description(scale, &label_text, &description);
                    latest.set(value);
                    if pending.replace(true) {
                        return;
                    }
                    let on_change = Rc::clone(&on_change);
                    let latest = Rc::clone(&latest);
                    let pending = Rc::clone(&pending);
                    glib::timeout_add_local_once(Duration::from_millis(50), move || {
                        pending.set(false);
                        (*on_change)(latest.get());
                    });
                });
            }
            None => {
                row.add_css_class("is-disabled");
                scale.set_sensitive(false);
                set_accessible_label_description(&scale, label, unavailable);
            }
        }
        line.append(&scale);
        row.append(&line);

        if value.is_none() {
            let note = gtk::Label::new(Some(unavailable));
            note.add_css_class("gos-cc-note");
            note.set_xalign(0.0);
            row.append(&note);
        }
        row
    }

    fn percent_description(label: &str, value: f64) -> String {
        format!("{label}: {} percent", value.round() as i64)
    }

    fn icon(icon_name: &str, class_name: &str, px: i32) -> gtk::Image {
        let image = gtk::Image::from_icon_name(icon_name);
        image.add_css_class(class_name);
        image.set_pixel_size(px);
        image
    }

    fn set_accessible_label_description<W>(widget: &W, label: &str, description: &str)
    where
        W: gtk::glib::object::IsA<gtk::Accessible>,
    {
        widget.update_property(&[
            gtk::accessible::Property::Label(label),
            gtk::accessible::Property::Description(description),
        ]);
    }

    fn tool_button(
        label: &str,
        program: &'static str,
        args: &'static [&'static str],
        window: &gtk::ApplicationWindow,
    ) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        button.add_css_class("gos-cc-link");
        button.set_halign(gtk::Align::Fill);
        let weak = window.downgrade();
        button.connect_clicked(move |_| {
            let _ = Command::new(program).args(args).spawn();
            if let Some(win) = weak.upgrade() {
                win.close();
            }
        });
        button
    }

    fn ai_action_button(
        label: &str,
        args: &'static [&'static str],
        availability: &AiActionAvailability,
        window: &gtk::ApplicationWindow,
    ) -> gtk::Button {
        ai_tool_button(label, LAUNCHER_BIN, args, availability, window)
    }

    fn ai_tool_button(
        label: &str,
        program: &'static str,
        args: &'static [&'static str],
        availability: &AiActionAvailability,
        window: &gtk::ApplicationWindow,
    ) -> gtk::Button {
        let button = tool_button(label, program, args, window);
        style_ai_action(&button);
        if !availability.enabled {
            button.set_sensitive(false);
            button.set_tooltip_text(Some(&availability.reason));
        }
        button
    }

    /// Restyle a `tool_button` into a Goblins AI action chip. The label sits
    /// left-aligned, matching the Wi-Fi/Appearance tiles and the rest of the
    /// OS's card language, so both tile families in the panel share one
    /// alignment grammar instead of mixing left- and center-aligned content.
    fn style_ai_action(button: &gtk::Button) {
        // The AI actions read as real chips, not the bare text link used for
        // the trailing "Open Settings…" affordance.
        button.remove_css_class("gos-cc-link");
        button.add_css_class("gos-cc-action");
        if let Some(label) = button.child().and_downcast::<gtk::Label>() {
            label.set_xalign(0.0);
            label.set_halign(gtk::Align::Start);
        }
    }

    fn first_unavailable_ai_reason<'a>(actions: &[&'a AiActionAvailability]) -> Option<&'a str> {
        actions
            .iter()
            .find(|availability| !availability.enabled)
            .map(|availability| availability.reason.as_str())
    }

    fn present_with_fade(window: &gtk::ApplicationWindow) {
        let animate = gtk::Settings::default()
            .map(|s| s.is_gtk_enable_animations())
            .unwrap_or(true);
        if !animate {
            window.set_opacity(1.0);
            window.present();
            return;
        }
        window.set_opacity(0.0);
        window.present();
        let start = std::time::Instant::now();
        let weak = window.downgrade();
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(window) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let elapsed = start.elapsed().as_millis() as f64;
            let t = (elapsed / goblins_os_design::MOTION_OVERLAY_MS as f64).clamp(0.0, 1.0);
            window.set_opacity(1.0 - (1.0 - t).powi(3));
            if t >= 1.0 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    // ── Real backends ────────────────────────────────────────────────────────

    fn run(program: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(program).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// WirePlumber default-sink volume as a 0–100 percentage.
    fn volume_percent() -> Option<f64> {
        let out = run("wpctl", &["get-volume", SINK])?;
        // "Volume: 0.65" (optionally "[MUTED]").
        let value: f64 = out.split_whitespace().nth(1)?.parse().ok()?;
        Some((value * 100.0).clamp(0.0, 100.0))
    }

    /// Backlight brightness as a 0–100 percentage (None when there's no backlight).
    fn brightness_percent() -> Option<f64> {
        // Pin to the backlight class so the read addresses the same device the
        // setter (`brightnessctl -c backlight set`) writes — `-m` alone enumerates
        // every device (e.g. keyboard LEDs) and the first row isn't guaranteed to
        // be the display backlight.
        let out = run("brightnessctl", &["-m", "-c", "backlight"])?;
        // "name,type,current,percent,max" → e.g. "intel_backlight,backlight,3500,35%,10000"
        let percent = out
            .lines()
            .find(|line| line.split(',').nth(1) == Some("backlight"))?
            .split(',')
            .nth(3)?
            .trim_end_matches('%');
        percent.parse::<f64>().ok()
    }

    fn wifi_enabled() -> Option<bool> {
        let out = run("nmcli", &["-t", "-f", "WIFI", "g"])?;
        Some(out.trim() == "enabled")
    }

    fn wifi_name() -> Option<String> {
        let out = run("nmcli", &["-t", "-f", "active,ssid", "dev", "wifi"])?;
        out.lines()
            .find_map(|line| line.strip_prefix("yes:"))
            .map(|ssid| {
                // nmcli terse mode backslash-escapes literal `:` and `\` inside
                // values (e.g. SSID `Café:5G` → `Café\:5G`). Unescape in a single
                // left-to-right pass so `\\` adjacent to `\:` is handled correctly.
                let mut out = String::with_capacity(ssid.len());
                let mut chars = ssid.chars();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        if let Some(n) = chars.next() {
                            out.push(n);
                        }
                    } else {
                        out.push(c);
                    }
                }
                out
            })
    }

    #[derive(Deserialize)]
    struct EngineStatus {
        engine: String,
    }

    fn current_engine(core_url: &str) -> Option<String> {
        let (status, body) = http_request(core_url, "GET", "/v1/models/openai-key", None).ok()?;
        if !(200..=299).contains(&status) {
            return None;
        }
        serde_json::from_slice::<EngineStatus>(&body)
            .ok()
            .map(|status| status.engine)
    }

    fn ai_action_availability(core_url: &str, id: &str) -> AiActionAvailability {
        let fallback = "Set up local or hosted AI in Models before asking Goblins AI.".to_string();
        let Some(catalog) = get_json::<AiActionCatalog>(core_url, "/v1/ai/actions") else {
            return AiActionAvailability {
                enabled: false,
                reason: fallback,
            };
        };
        catalog
            .actions
            .into_iter()
            .find(|action| action.id == id)
            .map(|action| AiActionAvailability {
                enabled: action.enabled,
                reason: action.reason,
            })
            .unwrap_or_else(|| AiActionAvailability {
                enabled: false,
                reason: "This Goblins AI action is not registered in the OS action catalog."
                    .to_string(),
            })
    }

    fn focus_status(core_url: &str) -> Option<FocusStatus> {
        get_json(core_url, "/v1/focus/status")
    }

    fn get_json<T: for<'de> Deserialize<'de>>(core_url: &str, path: &str) -> Option<T> {
        let (status, body) = http_request(core_url, "GET", path, None).ok()?;
        if !(200..=299).contains(&status) {
            return None;
        }
        serde_json::from_slice(&body).ok()
    }

    fn set_engine(core_url: &str, engine: &str) -> bool {
        let body = serde_json::json!({ "engine": engine }).to_string();
        matches!(
            http_request(core_url, "POST", "/v1/models/engine", Some(&body)),
            Ok((200..=299, _))
        )
    }

    fn http_request(
        core_url: &str,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, Vec<u8>), ()> {
        let rest = core_url.strip_prefix("http://").ok_or(())?;
        let authority = rest.split('/').next().ok_or(())?;
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().map_err(|_| ())?),
            None => (authority, 80),
        };
        let address = (host, port)
            .to_socket_addrs()
            .map_err(|_| ())?
            .next()
            .ok_or(())?;
        let mut stream =
            TcpStream::connect_timeout(&address, Duration::from_millis(700)).map_err(|_| ())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|_| ())?;
        stream
            .set_write_timeout(Some(Duration::from_millis(2000)))
            .map_err(|_| ())?;

        let request = match body {
            Some(payload) => format!(
                "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            ),
            None => format!(
                "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
            ),
        };
        stream.write_all(request.as_bytes()).map_err(|_| ())?;

        let mut raw = Vec::new();
        stream
            .take(MAX_BODY_BYTES)
            .read_to_end(&mut raw)
            .map_err(|_| ())?;
        let header_end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or(())?;
        let head = std::str::from_utf8(&raw[..header_end]).map_err(|_| ())?;
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or(())?;
        Ok((status, raw[header_end + 4..].to_vec()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn control_center_controls_are_accessible_and_title_case() {
        let source = include_str!("main.rs");
        assert!(source.contains("Connection & Appearance"));
        assert!(source.contains("AI Mode"));
        assert!(source.contains("Goblins AI"));
        assert!(source.contains("set_accessible_label_description"));
        assert!(source.contains("Use on-device GPT-OSS"));
        assert!(source.contains("Use OpenAI Codex"));
        assert!(source.contains("Volume"));
        assert!(source.contains("percent_description"));
        assert!(source.contains("Display brightness"));
        assert!(source.contains("/v1/focus/status"));
        assert!(source.contains("--panel=notifications"));
        assert!(source.contains("focus_tile_copy"));
        assert!(source.contains("Ask Goblin…"));
        assert!(source.contains("Open Settings and close Control Center"));
        let legacy_render_env = ["GOBLINS", "OS", "CONTROL", "CENTER", "DEMO"].join("_");
        let legacy_render_ssid = ["Goblins", "5G"].join("-");
        assert!(!source.contains(&legacy_render_env));
        assert!(!source.contains(&legacy_render_ssid));

        for legacy in [
            ["CONNECTION", "&", "APPEARANCE"].join(" "),
            ["BUILD", "ENGINE"].join(" "),
            ["GOBLINS", "AI"].join(" "),
            ["SO", "UND"].join(""),
            ["DIS", "PLAY"].join(""),
        ] {
            assert!(
                !source.contains(&legacy),
                "Control Center section labels should stay title case: {legacy}"
            );
        }
    }

    #[test]
    fn focus_tile_copy_reports_core_truth() {
        let work = super::FocusMode {
            id: "work".to_string(),
            name: "Work".to_string(),
        };
        let status = super::FocusStatus {
            available: true,
            active_mode: "work".to_string(),
            scheduled_mode: Some("work".to_string()),
            armed_by_schedule: true,
            modes: vec![work],
            detail: "Focus mode 'work' is active from a schedule.".to_string(),
        };

        let copy = super::focus_tile_copy(Some(&status));
        assert_eq!(copy.state, "Work");
        assert!(copy.active);
        assert!(copy.opens_settings);
        assert!(copy.detail.contains("schedule"));
    }

    #[test]
    fn focus_tile_copy_degrades_without_guessing() {
        let unavailable = super::focus_tile_copy(None);
        assert_eq!(unavailable.state, "Unavailable");
        assert!(!unavailable.active);
        assert!(!unavailable.opens_settings);
        assert!(unavailable.detail.contains("core did not respond"));

        let unknown_active = super::FocusStatus {
            available: true,
            active_mode: "deep".to_string(),
            scheduled_mode: None,
            armed_by_schedule: false,
            modes: Vec::new(),
            detail: "Focus mode 'deep' is active.".to_string(),
        };
        let copy = super::focus_tile_copy(Some(&unknown_active));
        assert_eq!(copy.state, "Unknown mode");
        assert!(copy.active);
        assert!(copy.opens_settings);
        assert!(copy.detail.contains("not in the configured Focus list"));
    }
}
