//! The Goblins OS markup editor — the macOS "Markup" idiom for a captured
//! screenshot. The screenshot thumbnail (a goblins-wm actor) hands this binary the
//! freshly written PNG; the person draws arrows, boxes, highlights, and text, then
//! saves the annotated image or copies it to the clipboard.
//!
//! Annotations are stored in IMAGE coordinates and the canvas renders through a
//! single image-space cairo transform, so the on-screen preview and the exported
//! PNG are pixel-identical. Outside a Linux native-desktop build the crate degrades
//! to a one-line status print.

use std::error::Error;

type MarkupResult<T> = Result<T, Box<dyn Error>>;

fn main() -> MarkupResult<()> {
    run()
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run() -> MarkupResult<()> {
    println!("goblins_os_markup=unavailable");
    println!("markup_reason=build_requires_linux_native_desktop_feature");
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run() -> MarkupResult<()> {
    native::run()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
mod native {
    use std::cell::{Cell, RefCell};
    use std::f64::consts::PI;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use gtk::cairo::{
        Context as CairoContext, FontSlant, FontWeight, Format, ImageSurface, LineCap,
    };
    use gtk::gdk;
    use gtk::gdk::prelude::GdkCairoContextExt;
    use gtk::gdk_pixbuf::Pixbuf;
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;

    use super::MarkupResult;

    const APP_ID: &str = "org.goblins.OS.Markup";

    /// Markup-specific chrome layered on the shared design tokens.
    const MARKUP_CSS: &str = "
.gos-markup-root { background: @gos_canvas; }
.gos-markup-toolbar {
  padding: 8px 12px;
  background: alpha(@gos_surface, 0.92);
  border-bottom: 1px solid @gos_hairline;
}
.gos-markup-title { color: @gos_ink; font-weight: 700; font-size: 13px; }
.gos-markup-tool {
  min-width: 34px; min-height: 30px;
  padding: 2px 10px; border-radius: 9px;
  border: 1px solid transparent; background: transparent;
  color: @gos_ink_secondary; font-size: 14px; font-weight: 600;
}
.gos-markup-tool:hover { background: @gos_surface_muted; }
.gos-markup-tool.is-active {
  background: alpha(@gos_accent, 0.16);
  border-color: alpha(@gos_accent, 0.5);
  color: @gos_ink;
}
.gos-markup-swatch {
  min-width: 20px; min-height: 20px; padding: 0;
  border-radius: 999px; border: 2px solid alpha(@gos_ink, 0.18);
}
.gos-markup-swatch.is-active { border-color: @gos_ink; }
.gos-markup-action {
  padding: 5px 14px; border-radius: 9px;
  border: 1px solid @gos_hairline; background: @gos_surface_muted;
  color: @gos_ink; font-weight: 600;
}
.gos-markup-action.is-primary {
  background: @gos_accent; color: @gos_on_primary; border-color: transparent;
}
.gos-markup-status { color: @gos_ink_muted; font-size: 12px; padding: 6px 12px; }
.gos-markup-canvas { background: @gos_surface_muted; }
.gos-markup-text-entry { min-height: 30px; border-radius: 8px; }
.gos-markup-swatch.sw-red { background-color: rgb(255, 69, 58); }
.gos-markup-swatch.sw-yellow { background-color: rgb(255, 214, 10); }
.gos-markup-swatch.sw-green { background-color: rgb(50, 215, 75); }
.gos-markup-swatch.sw-blue { background-color: rgb(10, 132, 255); }
.gos-markup-swatch.sw-white { background-color: rgb(245, 245, 247); }
";

    type Rgba = (f64, f64, f64, f64);

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Tool {
        Arrow,
        Box,
        Highlight,
        Text,
    }

    #[derive(Clone)]
    struct Annotation {
        tool: Tool,
        color: Rgba,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        text: String,
    }

    struct State {
        anns: Vec<Annotation>,
        live: Option<Annotation>,
        tool: Tool,
        color: Rgba,
        stroke: f64,
    }

    pub fn run() -> MarkupResult<()> {
        let path = std::env::args().nth(1);
        let app = gtk::Application::builder().application_id(APP_ID).build();
        app.connect_activate(move |app| {
            goblins_os_ui::init_theming(MARKUP_CSS);
            build_window(app, path.clone());
        });
        app.run_with_args(&["goblins-os-markup"]);
        Ok(())
    }

    fn build_window(app: &gtk::Application, path: Option<String>) {
        let pixbuf = path
            .as_deref()
            .and_then(|p| Pixbuf::from_file(p).ok())
            .unwrap_or_else(placeholder_pixbuf);
        let pixbuf = Rc::new(pixbuf);
        // A sensible stroke weight relative to the capture's resolution.
        let stroke = (pixbuf.width() as f64 / 320.0).max(3.0);

        let mut anns = Vec::new();
        if std::env::var_os("GOBLINS_OS_RENDER_DEMO").is_some() {
            anns = demo_annotations(pixbuf.width() as f64, pixbuf.height() as f64);
        }

        let state = Rc::new(RefCell::new(State {
            anns,
            live: None,
            tool: Tool::Arrow,
            color: ACCENT_RED,
            stroke,
        }));
        let view = Rc::new(Cell::new((1.0_f64, 0.0_f64, 0.0_f64)));
        let drag_start = Rc::new(Cell::new((0.0_f64, 0.0_f64)));

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Markup")
            .default_width(1040)
            .default_height(720)
            .build();
        window.add_css_class("gos-markup-root");

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        // ── Canvas ──
        let canvas = gtk::DrawingArea::new();
        canvas.add_css_class("gos-markup-canvas");
        canvas.set_hexpand(true);
        canvas.set_vexpand(true);
        {
            let state = state.clone();
            let pixbuf = pixbuf.clone();
            let view = view.clone();
            canvas.set_draw_func(move |_area, cr, w, h| {
                let st = state.borrow();
                let iw = pixbuf.width() as f64;
                let ih = pixbuf.height() as f64;
                if iw <= 0.0 || ih <= 0.0 {
                    return;
                }
                let scale = (w as f64 / iw).min(h as f64 / ih);
                let dw = iw * scale;
                let dh = ih * scale;
                let ox = (w as f64 - dw) / 2.0;
                let oy = (h as f64 - dh) / 2.0;
                view.set((scale, ox, oy));

                cr.save().ok();
                cr.translate(ox, oy);
                cr.scale(scale, scale);
                cr.rectangle(0.0, 0.0, iw, ih);
                cr.clip();
                paint_scene(cr, &pixbuf, &st.anns, st.live.as_ref(), st.stroke);
                cr.restore().ok();
            });
        }

        // ── Drag-to-draw ──
        let entry = gtk::Entry::new();
        entry.add_css_class("gos-markup-text-entry");
        entry.set_placeholder_text(Some("Text to place…"));
        entry.set_width_chars(16);

        let drag = gtk::GestureDrag::new();
        {
            let state = state.clone();
            let view = view.clone();
            let drag_start = drag_start.clone();
            let canvas_w = canvas.clone();
            drag.connect_drag_begin(move |_g, x, y| {
                drag_start.set((x, y));
                let (ix, iy) = screen_to_image(view.get(), x, y);
                let mut st = state.borrow_mut();
                if st.tool != Tool::Text {
                    let (tool, color) = (st.tool, st.color);
                    st.live = Some(Annotation {
                        tool,
                        color,
                        x0: ix,
                        y0: iy,
                        x1: ix,
                        y1: iy,
                        text: String::new(),
                    });
                }
                canvas_w.queue_draw();
            });
        }
        {
            let state = state.clone();
            let view = view.clone();
            let drag_start = drag_start.clone();
            let canvas_w = canvas.clone();
            drag.connect_drag_update(move |_g, off_x, off_y| {
                let (sx, sy) = drag_start.get();
                let (ix, iy) = screen_to_image(view.get(), sx + off_x, sy + off_y);
                let mut st = state.borrow_mut();
                if let Some(live) = st.live.as_mut() {
                    live.x1 = ix;
                    live.y1 = iy;
                }
                drop(st);
                canvas_w.queue_draw();
            });
        }
        {
            let state = state.clone();
            let view = view.clone();
            let drag_start = drag_start.clone();
            let canvas_w = canvas.clone();
            let entry_w = entry.clone();
            drag.connect_drag_end(move |_g, off_x, off_y| {
                let (sx, sy) = drag_start.get();
                let (ix, iy) = screen_to_image(view.get(), sx, sy);
                let mut st = state.borrow_mut();
                if st.tool == Tool::Text {
                    let text = entry_w.text().to_string();
                    if !text.trim().is_empty() {
                        let color = st.color;
                        st.anns.push(Annotation {
                            tool: Tool::Text,
                            color,
                            x0: ix,
                            y0: iy,
                            x1: ix,
                            y1: iy,
                            text,
                        });
                    }
                } else if let Some(mut live) = st.live.take() {
                    let (ex, ey) = screen_to_image(view.get(), sx + off_x, sy + off_y);
                    live.x1 = ex;
                    live.y1 = ey;
                    // Ignore a stray click that produced no shape.
                    if (live.x1 - live.x0).abs() > 1.0 || (live.y1 - live.y0).abs() > 1.0 {
                        st.anns.push(live);
                    }
                }
                drop(st);
                canvas_w.queue_draw();
            });
        }
        canvas.add_controller(drag);

        // ── Toolbar ──
        let status = gtk::Label::new(Some("Drag to annotate · Backspace to undo the last mark"));
        status.add_css_class("gos-markup-status");
        status.set_xalign(0.0);

        let toolbar = build_toolbar(
            &state,
            &entry,
            &status,
            pixbuf.clone(),
            path.clone(),
            &window,
        );
        root.append(&toolbar);
        root.append(&canvas);
        root.append(&status);

        // Backspace removes the last annotation (a quiet, expected undo).
        let keys = gtk::EventControllerKey::new();
        {
            let state = state.clone();
            let canvas_w = canvas.clone();
            keys.connect_key_pressed(move |_c, key, _code, _mods| {
                if key == gdk::Key::BackSpace {
                    let mut st = state.borrow_mut();
                    st.anns.pop();
                    drop(st);
                    canvas_w.queue_draw();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        window.add_controller(keys);

        window.set_child(Some(&root));
        window.present();
    }

    fn build_toolbar(
        state: &Rc<RefCell<State>>,
        entry: &gtk::Entry,
        status: &gtk::Label,
        pixbuf: Rc<Pixbuf>,
        path: Option<String>,
        window: &gtk::ApplicationWindow,
    ) -> gtk::Box {
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        toolbar.add_css_class("gos-markup-toolbar");

        let title = gtk::Label::new(Some("Markup"));
        title.add_css_class("gos-markup-title");
        toolbar.append(&title);
        toolbar.append(&spacer(12));

        // Tools.
        let tools = [
            (Tool::Arrow, "↗", "Arrow"),
            (Tool::Box, "▢", "Box"),
            (Tool::Highlight, "▰", "Highlight"),
            (Tool::Text, "T", "Text"),
        ];
        let tool_buttons: Rc<RefCell<Vec<(Tool, gtk::Button)>>> = Rc::new(RefCell::new(Vec::new()));
        let tool_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        for (tool, glyph, name) in tools {
            let button = gtk::Button::with_label(glyph);
            button.add_css_class("gos-markup-tool");
            button.set_tooltip_text(Some(name));
            set_accessible(&button, name);
            if tool == Tool::Arrow {
                button.add_css_class("is-active");
            }
            {
                let state = state.clone();
                let tool_buttons = tool_buttons.clone();
                button.connect_clicked(move |_| {
                    state.borrow_mut().tool = tool;
                    for (t, b) in tool_buttons.borrow().iter() {
                        if *t == tool {
                            b.add_css_class("is-active");
                        } else {
                            b.remove_css_class("is-active");
                        }
                    }
                });
            }
            tool_buttons.borrow_mut().push((tool, button.clone()));
            tool_box.append(&button);
        }
        toolbar.append(&tool_box);
        toolbar.append(&spacer(10));

        // Colors (keyed by CSS class, not the float tuple — exact float equality
        // is a clippy hazard, and the class also paints the swatch via the theme).
        let swatches: Rc<RefCell<Vec<(&'static str, gtk::Button)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let color_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for (color, name, class) in PALETTE {
            let button = gtk::Button::new();
            button.add_css_class("gos-markup-swatch");
            button.add_css_class(class);
            button.set_tooltip_text(Some(name));
            set_accessible(&button, name);
            if class == "sw-red" {
                button.add_css_class("is-active");
            }
            {
                let state = state.clone();
                let swatches = swatches.clone();
                button.connect_clicked(move |_| {
                    state.borrow_mut().color = color;
                    for (c, b) in swatches.borrow().iter() {
                        if *c == class {
                            b.add_css_class("is-active");
                        } else {
                            b.remove_css_class("is-active");
                        }
                    }
                });
            }
            swatches.borrow_mut().push((class, button.clone()));
            color_box.append(&button);
        }
        toolbar.append(&color_box);
        toolbar.append(&spacer(10));

        entry.set_hexpand(false);
        toolbar.append(entry);

        toolbar.append(&grow());

        // Actions.
        let copy = gtk::Button::with_label("Copy");
        copy.add_css_class("gos-markup-action");
        set_accessible(&copy, "Copy annotated image to clipboard");
        {
            let state = state.clone();
            let pixbuf = pixbuf.clone();
            let status = status.clone();
            copy.connect_clicked(move |btn| {
                let st = state.borrow();
                match export_surface(&pixbuf, &st.anns, st.stroke).and_then(surface_to_texture) {
                    Some(texture) => {
                        copy_to_clipboard(btn, &texture);
                        status.set_text("Copied to clipboard");
                    }
                    None => status.set_text("Couldn’t copy the image"),
                }
            });
        }
        toolbar.append(&copy);

        let save = gtk::Button::with_label("Save");
        save.add_css_class("gos-markup-action");
        save.add_css_class("is-primary");
        set_accessible(&save, "Save annotated image");
        {
            let state = state.clone();
            let pixbuf = pixbuf.clone();
            let status = status.clone();
            let window = window.clone();
            save.connect_clicked(move |_| {
                let st = state.borrow();
                match export_surface(&pixbuf, &st.anns, st.stroke)
                    .and_then(surface_to_texture)
                    .and_then(|texture| save_png(&texture, path.as_deref()))
                {
                    Some(out) => {
                        status.set_text(&format!("Saved · {}", out.display()));
                        window.close();
                    }
                    None => status.set_text("Couldn’t save the image"),
                }
            });
        }
        toolbar.append(&save);

        toolbar
    }

    // ── Rendering ──

    fn paint_scene(
        cr: &CairoContext,
        pixbuf: &Pixbuf,
        anns: &[Annotation],
        live: Option<&Annotation>,
        stroke: f64,
    ) {
        cr.set_source_pixbuf(pixbuf, 0.0, 0.0);
        cr.paint().ok();
        for ann in anns {
            draw_annotation(cr, ann, stroke);
        }
        if let Some(ann) = live {
            draw_annotation(cr, ann, stroke);
        }
    }

    fn draw_annotation(cr: &CairoContext, a: &Annotation, stroke: f64) {
        let (r, g, b, al) = a.color;
        match a.tool {
            Tool::Arrow => {
                cr.set_source_rgba(r, g, b, al);
                cr.set_line_width(stroke);
                cr.set_line_cap(LineCap::Round);
                cr.move_to(a.x0, a.y0);
                cr.line_to(a.x1, a.y1);
                let angle = (a.y1 - a.y0).atan2(a.x1 - a.x0);
                let head = (stroke * 3.6).max(12.0);
                for spread in [PI - 0.45, PI + 0.45] {
                    cr.move_to(a.x1, a.y1);
                    cr.line_to(
                        a.x1 + head * (angle + spread).cos(),
                        a.y1 + head * (angle + spread).sin(),
                    );
                }
                cr.stroke().ok();
            }
            Tool::Box => {
                cr.set_source_rgba(r, g, b, al);
                cr.set_line_width(stroke);
                let x = a.x0.min(a.x1);
                let y = a.y0.min(a.y1);
                cr.rectangle(x, y, (a.x1 - a.x0).abs(), (a.y1 - a.y0).abs());
                cr.stroke().ok();
            }
            Tool::Highlight => {
                cr.set_source_rgba(r, g, b, 0.30);
                cr.set_line_width(stroke * 5.0);
                cr.set_line_cap(LineCap::Round);
                cr.move_to(a.x0, a.y0);
                cr.line_to(a.x1, a.y1);
                cr.stroke().ok();
            }
            Tool::Text => {
                cr.set_source_rgba(r, g, b, al);
                cr.select_font_face("Inter", FontSlant::Normal, FontWeight::Bold);
                cr.set_font_size((stroke * 6.5).max(20.0));
                cr.move_to(a.x0, a.y0);
                cr.show_text(&a.text).ok();
            }
        }
    }

    fn export_surface(pixbuf: &Pixbuf, anns: &[Annotation], stroke: f64) -> Option<ImageSurface> {
        let iw = pixbuf.width();
        let ih = pixbuf.height();
        let surface = ImageSurface::create(Format::ARgb32, iw, ih).ok()?;
        {
            let cr = CairoContext::new(&surface).ok()?;
            cr.rectangle(0.0, 0.0, iw as f64, ih as f64);
            cr.clip();
            paint_scene(&cr, pixbuf, anns, None, stroke);
        }
        surface.flush();
        Some(surface)
    }

    /// Build a GPU texture from the rendered cairo surface — the bridge to both the
    /// clipboard and PNG export (cairo's own `write_to_png` needs an optional feature
    /// the GTK stack doesn't enable; `gdk::Texture` always encodes PNG).
    fn surface_to_texture(mut surface: ImageSurface) -> Option<gdk::MemoryTexture> {
        let width = surface.width();
        let height = surface.height();
        let stride = surface.stride();
        let bytes = {
            let data = surface.data().ok()?;
            glib::Bytes::from(&data[..])
        };
        Some(gdk::MemoryTexture::new(
            width,
            height,
            gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &bytes,
            stride as usize,
        ))
    }

    fn save_png(texture: &gdk::MemoryTexture, source: Option<&str>) -> Option<PathBuf> {
        let out = annotated_path(source);
        texture.save_to_png(&out).ok()?;
        Some(out)
    }

    fn copy_to_clipboard(widget: &impl IsA<gtk::Widget>, texture: &gdk::MemoryTexture) {
        widget.as_ref().clipboard().set_texture(texture);
    }

    // ── Helpers ──

    fn screen_to_image(view: (f64, f64, f64), sx: f64, sy: f64) -> (f64, f64) {
        let (scale, ox, oy) = view;
        if scale <= 0.0 {
            return (sx, sy);
        }
        ((sx - ox) / scale, (sy - oy) / scale)
    }

    fn annotated_path(source: Option<&str>) -> PathBuf {
        if let Some(src) = source {
            let p = Path::new(src);
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Screenshot");
            let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
            return dir.join(format!("{stem}-annotated.png"));
        }
        let mut dir = glib::user_special_dir(glib::UserDirectory::Pictures)
            .unwrap_or_else(|| PathBuf::from("."));
        dir.push("Screenshots");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("Markup-annotated.png")
    }

    const ACCENT_RED: Rgba = (1.0, 0.27, 0.227, 1.0);
    const PALETTE: [(Rgba, &str, &str); 5] = [
        (ACCENT_RED, "Red", "sw-red"),
        ((1.0, 0.84, 0.04, 1.0), "Yellow", "sw-yellow"),
        ((0.196, 0.843, 0.294, 1.0), "Green", "sw-green"),
        ((0.04, 0.52, 1.0, 1.0), "Blue", "sw-blue"),
        ((0.96, 0.96, 0.97, 1.0), "White", "sw-white"),
    ];

    fn placeholder_pixbuf() -> Pixbuf {
        let pixbuf = Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, true, 8, 1200, 800)
            .expect("allocate placeholder pixbuf");
        pixbuf.fill(0x1b1b22ff);
        pixbuf
    }

    fn demo_annotations(w: f64, h: f64) -> Vec<Annotation> {
        vec![
            Annotation {
                tool: Tool::Box,
                color: ACCENT_RED,
                x0: w * 0.08,
                y0: h * 0.12,
                x1: w * 0.46,
                y1: h * 0.40,
                text: String::new(),
            },
            Annotation {
                tool: Tool::Arrow,
                color: (0.04, 0.52, 1.0, 1.0),
                x0: w * 0.86,
                y0: h * 0.30,
                x1: w * 0.50,
                y1: h * 0.50,
                text: String::new(),
            },
            Annotation {
                tool: Tool::Highlight,
                color: (1.0, 0.84, 0.04, 1.0),
                x0: w * 0.10,
                y0: h * 0.66,
                x1: w * 0.54,
                y1: h * 0.66,
                text: String::new(),
            },
            Annotation {
                tool: Tool::Text,
                color: (0.96, 0.96, 0.97, 1.0),
                x0: w * 0.10,
                y0: h * 0.84,
                x1: w * 0.10,
                y1: h * 0.84,
                text: "Ship it".to_string(),
            },
        ]
    }

    fn spacer(px: i32) -> gtk::Box {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        b.set_size_request(px, 1);
        b
    }

    fn grow() -> gtk::Box {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        b.set_hexpand(true);
        b
    }

    fn set_accessible(widget: &impl IsA<gtk::Accessible>, label: &str) {
        widget.update_property(&[
            gtk::accessible::Property::Label(label),
            gtk::accessible::Property::Description(label),
        ]);
    }
}
