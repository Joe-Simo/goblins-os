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

/// A reusable macOS-style vibrancy backdrop: a single-child container that paints
/// a real GSK Gaussian blur of the desktop wallpaper behind its child, then a thin
/// translucent tint — the "material" the compositor cannot give an isolated app
/// surface on Wayland. Because the blur is pure GSK over an asset the app already
/// owns (the shipped wallpaper), it renders under the software (cairo) renderer
/// too, so it appears in the headless screenshot captures, not only on real GPUs.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
mod vibrancy {
    use std::env;

    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;
    use gtk4::{gdk, glib, graphene};

    /// macOS-ish defaults for a panel-scale surface (control center, launcher).
    const DEFAULT_BLUR_RADIUS: f64 = 30.0;
    const DEFAULT_CORNER_RADIUS: f32 = 16.0;
    /// A gentle saturation boost mirrors NSVisualEffectView's "vibrancy" — the
    /// material reads a touch more alive than the raw wallpaper, never garish.
    const DEFAULT_SATURATION: f32 = 1.28;

    /// A luminance-preserving saturation matrix (Rec. 709 weights), column-major
    /// for `graphene::Matrix::from_float` / GSK's `push_color_matrix` (`out = M·rgba`).
    fn saturation_matrix(s: f32) -> gtk4::graphene::Matrix {
        let (lr, lg, lb) = (0.2126_f32, 0.7152, 0.0722);
        let inv = 1.0 - s;
        // Each group of four is a column = one input channel's contribution to
        // (out_r, out_g, out_b, out_a). Kept as an aligned grid for legibility.
        #[rustfmt::skip]
        let m = [
            s + inv * lr, inv * lr,     inv * lr,     0.0, // input R
            inv * lg,     s + inv * lg, inv * lg,     0.0, // input G
            inv * lb,     inv * lb,     s + inv * lb, 0.0, // input B
            0.0,          0.0,          0.0,          1.0, // input A (alpha through)
        ];
        gtk4::graphene::Matrix::from_float(m)
    }

    /// Candidate wallpaper files, in load-preference order. A rasterized PNG is
    /// tried first because it always loads via gdk-pixbuf; the SVG is the fallback
    /// for bases that ship a librsvg pixbuf loader (the Fedora base does not, so
    /// the image should install a PNG-rasterized wallpaper for vibrancy to read).
    /// A render/dev override points at a checkout's assets.
    fn wallpaper_candidates(dark: bool) -> Vec<String> {
        let variant = if dark {
            "goblins-os-dark"
        } else {
            "goblins-os-light"
        };
        if let Ok(dir) = env::var("GOBLINS_OS_WALLPAPER_DIR") {
            return vec![
                format!("{dir}/{variant}.png"),
                format!("{dir}/{variant}.svg"),
            ];
        }
        let svg = if dark {
            goblins_os_design::GOBLINS_WALLPAPER_DARK
        } else {
            goblins_os_design::GOBLINS_WALLPAPER_LIGHT
        };
        let mut candidates = Vec::new();
        if let Some(base) = svg.strip_suffix(".svg") {
            candidates.push(format!("{base}.png"));
        }
        candidates.push(svg.to_string());
        candidates
    }

    fn load_wallpaper_texture(dark: bool) -> Option<gdk::Texture> {
        // Rasterize the wallpaper at a generous size; the blur hides any softness,
        // and cover-fitting handles the panel's aspect ratio.
        for path in wallpaper_candidates(dark) {
            if let Ok(pixbuf) = gtk4::gdk_pixbuf::Pixbuf::from_file_at_size(&path, 1280, 800) {
                return Some(gdk::Texture::for_pixbuf(&pixbuf));
            }
        }
        None
    }

    /// The neutral material tint over the blur — a thin scrim, not an opaque fill,
    /// so the blurred wallpaper reads through (the whole point of vibrancy). Doubles
    /// as a graceful fallback: if the wallpaper asset is missing, the surface keeps
    /// this translucent material instead of going empty.
    fn default_tint(dark: bool) -> gdk::RGBA {
        if dark {
            gdk::RGBA::new(0.110, 0.110, 0.133, 0.58)
        } else {
            gdk::RGBA::new(0.980, 0.980, 0.988, 0.60)
        }
    }

    /// Cover-fit a `tw`×`th` texture into `target`, preserving aspect, centered —
    /// like CSS `background-size: cover`, so the wallpaper never letterboxes.
    fn cover_rect(tw: f32, th: f32, target: &graphene::Rect) -> graphene::Rect {
        if tw <= 0.0 || th <= 0.0 {
            return graphene::Rect::new(target.x(), target.y(), target.width(), target.height());
        }
        let scale = (target.width() / tw).max(target.height() / th);
        let cw = tw * scale;
        let ch = th * scale;
        let cx = target.x() + (target.width() - cw) / 2.0;
        let cy = target.y() + (target.height() - ch) / 2.0;
        graphene::Rect::new(cx, cy, cw, ch)
    }

    mod imp {
        use std::cell::{Cell, RefCell};

        use gtk4::prelude::*;
        use gtk4::subclass::prelude::*;
        use gtk4::{gdk, glib, graphene, gsk};

        pub struct VibrancyBackdrop {
            pub child: RefCell<Option<gtk4::Widget>>,
            pub texture: RefCell<Option<gdk::Texture>>,
            pub blur_radius: Cell<f64>,
            pub corner_radius: Cell<f32>,
            pub tint: RefCell<gdk::RGBA>,
            pub saturation: Cell<f32>,
        }

        impl Default for VibrancyBackdrop {
            fn default() -> Self {
                Self {
                    child: RefCell::new(None),
                    texture: RefCell::new(None),
                    blur_radius: Cell::new(super::DEFAULT_BLUR_RADIUS),
                    corner_radius: Cell::new(super::DEFAULT_CORNER_RADIUS),
                    tint: RefCell::new(super::default_tint(true)),
                    saturation: Cell::new(super::DEFAULT_SATURATION),
                }
            }
        }

        #[glib::object_subclass]
        impl ObjectSubclass for VibrancyBackdrop {
            const NAME: &'static str = "GosVibrancyBackdrop";
            type Type = super::VibrancyBackdrop;
            type ParentType = gtk4::Widget;
        }

        impl ObjectImpl for VibrancyBackdrop {
            fn dispose(&self) {
                if let Some(child) = self.child.borrow_mut().take() {
                    child.unparent();
                }
            }
        }

        impl WidgetImpl for VibrancyBackdrop {
            fn measure(
                &self,
                orientation: gtk4::Orientation,
                for_size: i32,
            ) -> (i32, i32, i32, i32) {
                match self.child.borrow().as_ref() {
                    Some(child) => child.measure(orientation, for_size),
                    None => (0, 0, -1, -1),
                }
            }

            fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
                if let Some(child) = self.child.borrow().as_ref() {
                    child.allocate(width, height, baseline, None);
                }
            }

            fn snapshot(&self, snapshot: &gtk4::Snapshot) {
                let widget = self.obj();
                let w = widget.width() as f32;
                let h = widget.height() as f32;
                if w > 0.0 && h > 0.0 {
                    let r = self.corner_radius.get();
                    let clip = gsk::RoundedRect::new(
                        graphene::Rect::new(0.0, 0.0, w, h),
                        graphene::Size::new(r, r),
                        graphene::Size::new(r, r),
                        graphene::Size::new(r, r),
                        graphene::Size::new(r, r),
                    );
                    snapshot.push_rounded_clip(&clip);

                    if let Some(texture) = self.texture.borrow().as_ref() {
                        // Overdraw by the blur kernel so the edges sample real
                        // pixels instead of fading into transparency; the clip
                        // above then crops the bleed back to the rounded bounds.
                        let blur = self.blur_radius.get();
                        let bleed = (blur as f32) * 2.0;
                        let target =
                            graphene::Rect::new(-bleed, -bleed, w + bleed * 2.0, h + bleed * 2.0);
                        let cover = super::cover_rect(
                            texture.width() as f32,
                            texture.height() as f32,
                            &target,
                        );
                        snapshot.push_blur(blur);
                        // Saturation boost (the "vibrancy" half of the material),
                        // applied to the wallpaper before the blur smooths it.
                        snapshot.push_color_matrix(
                            &super::saturation_matrix(self.saturation.get()),
                            &graphene::Vec4::new(0.0, 0.0, 0.0, 0.0),
                        );
                        snapshot.append_texture(texture, &cover);
                        snapshot.pop();
                        snapshot.pop();
                    }

                    let tint = *self.tint.borrow();
                    snapshot.append_color(&tint, &graphene::Rect::new(0.0, 0.0, w, h));
                    snapshot.pop();
                }

                if let Some(child) = self.child.borrow().as_ref() {
                    widget.snapshot_child(child, snapshot);
                }
            }
        }
    }

    glib::wrapper! {
        /// A vibrancy material backdrop wrapping one child. See module docs.
        pub struct VibrancyBackdrop(ObjectSubclass<imp::VibrancyBackdrop>)
            @extends gtk4::Widget;
    }

    impl VibrancyBackdrop {
        /// Wrap `child` on a blurred-wallpaper material for the active scheme.
        pub fn new(dark: bool, child: &impl IsA<gtk4::Widget>) -> Self {
            let obj: Self = glib::Object::builder().build();
            let child = child.as_ref().clone();
            child.set_parent(&obj);
            {
                let imp = obj.imp();
                *imp.tint.borrow_mut() = default_tint(dark);
                if let Some(texture) = load_wallpaper_texture(dark) {
                    *imp.texture.borrow_mut() = Some(texture);
                }
                *imp.child.borrow_mut() = Some(child);
            }
            obj
        }

        /// Tune the Gaussian blur radius (device-independent pixels).
        pub fn set_blur_radius(&self, radius: f64) {
            self.imp().blur_radius.set(radius);
            self.queue_draw();
        }

        /// Tune the rounded-corner radius of the material (match the card's CSS).
        pub fn set_corner_radius(&self, radius: f32) {
            self.imp().corner_radius.set(radius);
            self.queue_draw();
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
pub use theming::*;

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
pub use vibrancy::VibrancyBackdrop;
