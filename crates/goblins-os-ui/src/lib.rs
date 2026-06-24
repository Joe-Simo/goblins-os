//! Shared desktop-UI helpers for the native Goblins OS apps.
//!
//! Whole-OS theming is identical on every native surface (installer, login,
//! settings, shell): read the standard `org.gnome.desktop.interface color-scheme`
//! preference — the exact key GNOME's Settings writes — apply the matching
//! stylesheet, and re-theme live when it changes. This crate is the single home
//! for that logic so it can never drift between apps. Outside a Linux
//! native-desktop build the crate compiles to nothing.

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
mod theming {
    use std::env;

    use gtk4::prelude::*;

    /// The standard desktop schema that owns the Light/Dark/Auto preference, so
    /// Goblins OS themes exactly like the rest of the desktop.
    const COLOR_SCHEME_SCHEMA: &str = "org.gnome.desktop.interface";

    /// True when the standard color-scheme schema is installed; guards every read
    /// and write so a missing schema (e.g. a minimal container) degrades to the
    /// light default instead of aborting.
    pub fn color_scheme_available() -> bool {
        gtk4::gio::SettingsSchemaSource::default()
            .and_then(|source| source.lookup(COLOR_SCHEME_SCHEMA, true))
            .is_some()
    }

    /// The desktop's color-scheme preference: "default", "prefer-dark", or
    /// "prefer-light" (or "default" when the schema is absent).
    pub fn system_color_scheme() -> String {
        if !color_scheme_available() {
            return "default".to_string();
        }
        gtk4::gio::Settings::new(COLOR_SCHEME_SCHEMA)
            .string("color-scheme")
            .to_string()
    }

    /// Resolve the active scheme to a concrete dark flag. An ops/render override
    /// (`GOBLINS_OS_THEME`) wins for headless capture; otherwise the standard
    /// desktop preference decides.
    pub fn resolve_dark() -> bool {
        if let Ok(forced) = env::var("GOBLINS_OS_THEME") {
            match forced.trim() {
                "dark" => return true,
                "light" => return false,
                _ => {}
            }
        }
        system_color_scheme() == "prefer-dark"
    }

    /// Write the standard desktop preference. Because every themed app reads this
    /// one key, choosing here themes the whole desktop, exactly like GNOME's own
    /// Appearance toggle. Returns false if the schema is unavailable.
    pub fn set_color_scheme(scheme: &str) -> bool {
        if !color_scheme_available() {
            return false;
        }
        gtk4::gio::Settings::new(COLOR_SCHEME_SCHEMA)
            .set_string("color-scheme", scheme)
            .is_ok()
    }

    /// Compose an app's structural CSS with the active scheme's design tokens and
    /// load it into the default display.
    pub fn apply_css(app_css: &str, dark: bool) {
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&goblins_os_design::native_css(app_css, dark));
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    /// One-call theming for a native app: apply the active scheme now (and the GTK
    /// prefer-dark flag), then re-theme live whenever the standard color-scheme
    /// preference changes — from this app, GNOME Settings, or any other app. This
    /// single call replaces the per-app theming quartet (resolve/read/apply/watch).
    pub fn init_theming(app_css: &'static str) {
        let dark = resolve_dark();
        if let Some(settings) = gtk4::Settings::default() {
            settings.set_gtk_font_name(Some("Inter 11"));
            settings.set_gtk_application_prefer_dark_theme(dark);
        }
        apply_css(app_css, dark);

        if !color_scheme_available() {
            return;
        }
        let settings = gtk4::gio::Settings::new(COLOR_SCHEME_SCHEMA);
        settings.connect_changed(Some("color-scheme"), move |_, _| {
            let dark = resolve_dark();
            if let Some(gtk_settings) = gtk4::Settings::default() {
                gtk_settings.set_gtk_font_name(Some("Inter 11"));
                gtk_settings.set_gtk_application_prefer_dark_theme(dark);
            }
            apply_css(app_css, dark);
        });
        // Keep the GSettings handle (and its listener) alive for the app lifetime.
        std::mem::forget(settings);
    }

    /// A monoblossom `gtk4::Picture` at `px` from a fixed asset variant — for
    /// surfaces that keep one look regardless of scheme (the night-gradient
    /// login hero, the Build Studio sidebar). Pictures cannot be recolored by
    /// CSS, so the caller picks the variant that matches its surface.
    pub fn brand_mark(path: &str, px: i32) -> gtk4::Image {
        // gtk4::Image with a fixed pixel size — never GtkPicture, whose
        // aspect-tracking measurement balloons its allocation when a sparse
        // column hands it slack (render-verified on the install-progress page).
        let mark = gtk4::Image::new();
        mark.set_pixel_size(px);
        mark.set_halign(gtk4::Align::Center);
        mark.set_valign(gtk4::Align::Center);
        mark.set_hexpand(false);
        mark.set_vexpand(false);
        set_mark_asset(&mark, path, px);
        mark
    }

    /// A monoblossom that follows the active scheme — the black mark on light
    /// surfaces, the white mark on dark — switching live with the standard
    /// color-scheme preference (via the GTK prefer-dark flag that
    /// `init_theming` keeps in sync on every scheme change).
    pub fn themed_brand_mark(px: i32) -> gtk4::Image {
        let mark = brand_mark(themed_mark_asset(resolve_dark()), px);
        if let Some(settings) = gtk4::Settings::default() {
            set_mark_asset(
                &mark,
                themed_mark_asset(settings.is_gtk_application_prefer_dark_theme()),
                px,
            );
            let weak = mark.downgrade();
            settings.connect_gtk_application_prefer_dark_theme_notify(move |settings| {
                if let Some(mark) = weak.upgrade() {
                    set_mark_asset(
                        &mark,
                        themed_mark_asset(settings.is_gtk_application_prefer_dark_theme()),
                        px,
                    );
                }
            });
        }
        mark
    }

    /// macOS-style traffic-light controls for Goblins OS crafted windows.
    /// The windows are client-decorated so the OS chrome stays consistent under
    /// Wayland, Xvfb renders, and the headless GNOME desktop proof.
    pub fn window_controls(window: &gtk4::ApplicationWindow) -> gtk4::Box {
        let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        controls.add_css_class("gos-window-controls");
        controls.set_valign(gtk4::Align::Center);

        let close = control_button("window-close-symbolic", "Close", "gos-window-close");
        let weak = window.downgrade();
        close.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
        });
        controls.append(&close);

        let minimize = control_button(
            "window-minimize-symbolic",
            "Minimize",
            "gos-window-minimize",
        );
        let weak = window.downgrade();
        minimize.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                window.minimize();
            }
        });
        controls.append(&minimize);

        let zoom = control_button("window-maximize-symbolic", "Zoom", "gos-window-zoom");
        let weak = window.downgrade();
        zoom.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                if window.is_maximized() {
                    window.unmaximize();
                } else {
                    window.maximize();
                }
            }
        });
        controls.append(&zoom);

        controls
    }

    fn control_button(icon_name: &str, tooltip: &str, class_name: &str) -> gtk4::Button {
        let button = gtk4::Button::new();
        button.add_css_class("gos-window-control");
        button.add_css_class(class_name);
        button.set_tooltip_text(Some(tooltip));
        button.set_focus_on_click(false);
        button.set_valign(gtk4::Align::Center);
        let accessible_label = match class_name {
            "gos-window-close" => "Close window",
            "gos-window-minimize" => "Minimize window",
            "gos-window-zoom" => "Maximize or restore window",
            _ => tooltip,
        };
        button.update_property(&[
            gtk4::accessible::Property::Label(accessible_label),
            gtk4::accessible::Property::Description(accessible_label),
        ]);

        let icon = gtk4::Image::from_icon_name(icon_name);
        icon.set_pixel_size(8);
        button.set_child(Some(&icon));
        button
    }

    /// The Goblins OS *system* mark variant for the active scheme: ink mark on
    /// light, white mark on dark. This is the OS's own identity (menu bar, lock,
    /// installer, shell) — the OpenAI bloom is reserved for provider surfaces.
    fn themed_mark_asset(dark: bool) -> &'static str {
        if dark {
            goblins_os_design::GOBLINS_MARK_LIGHT
        } else {
            goblins_os_design::GOBLINS_MARK_DARK
        }
    }

    fn set_mark_asset(mark: &gtk4::Image, path: &str, px: i32) {
        if let Ok(pixbuf) = gtk4::gdk_pixbuf::Pixbuf::from_file_at_size(path, px, px) {
            mark.set_paintable(Some(&gtk4::gdk::Texture::for_pixbuf(&pixbuf)));
        }
    }

    /// A compact status pill ("Core", "OpenAI", …) tinted by readiness. Centered
    /// vertically so a taller top bar can never stretch it into an oval.
    pub fn status_pill(text: &str, ready: bool) -> gtk4::Label {
        let label = gtk4::Label::new(Some(text));
        label.add_css_class("gos-status-pill");
        label.add_css_class(if ready { "gos-ready" } else { "gos-waiting" });
        label.set_valign(gtk4::Align::Center);
        label
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
pub use theming::*;
