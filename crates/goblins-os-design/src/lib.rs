//! The Goblins OS native design system.
//!
//! Every native surface (installer, login, shell, settings) prepends its own
//! structural CSS and then loads [`GOBLINS_NATIVE_CSS`] last, so this stylesheet
//! is the single authoritative visual layer for the whole OS. It is expressed in
//! GTK4-native CSS with a `@define-color` token foundation: one calm, premium,
//! Goblins-native language for color, type, spacing, radius, elevation, and
//! motion shared by first boot through daily use.
//!
//! The macOS 27 Sketch UI kit is used here as a local reference for token axes,
//! not as a shipped asset source: light/dark, content-area vs over-glass,
//! label/fill hierarchy, material thickness, control sizes, and state vocabulary
//! are translated into Inter + GTK tokens without bundling Apple fonts, symbols,
//! wallpapers, templates, or first-party app layouts.

/// Directory where the OpenAI brand marks are installed in the OS image.
pub const BRAND_DIR: &str = "/usr/share/goblins-os/brand";

/// The OpenAI monoblossom symbol (black), for light surfaces. High-resolution
/// PNG so it renders crisply at any UI size without an extra image loader.
pub const OPENAI_MARK_DARK: &str = "/usr/share/goblins-os/brand/OpenAI-black-monoblossom.png";

/// The OpenAI monoblossom symbol (white), for night surfaces. This marks the
/// OpenAI *provider* (Codex / Sign in with OpenAI / API key) — NOT the OS itself.
pub const OPENAI_MARK_LIGHT: &str = "/usr/share/goblins-os/brand/OpenAI-white-monoblossom.png";

/// The Goblins OS *system* mark (ink), for light surfaces. This is the product's
/// own identity — distinct from the OpenAI provider bloom — shown in the menu bar,
/// lock/login, installer, and app tiles.
pub const GOBLINS_MARK_DARK: &str = "/usr/share/goblins-os/brand/Goblins-black-mark.svg";

/// The Goblins OS system mark (white), for night surfaces (menu bar, lock, hero).
pub const GOBLINS_MARK_LIGHT: &str = "/usr/share/goblins-os/brand/Goblins-white-mark.svg";

// ── Motion tokens (the macOS-blend half of the language) ─────────────────────
// One motion vocabulary for the whole OS. Durations are expressed in ms and the
// easing curves are GTK4 `cubic-bezier()` strings, so a Rust animation (the
// launcher fade-scale) and a CSS transition reach for the SAME numbers. macOS
// motion is short, decisive, and slightly springy on arrival — never bouncy on a
// productivity surface. GTK disables every CSS transition automatically when the
// desktop's Reduce Motion is on, and the Rust animators honor it via the GTK
// `gtk-enable-animations` setting, so the reduced-motion path is a clean cut.

/// Press / immediate tactile feedback — the control responds the instant it's touched.
pub const MOTION_INSTANT_MS: u32 = 90;
/// Hover, focus, small state changes (row highlight, pill relabel).
pub const MOTION_FAST_MS: u32 = 140;
/// The base tempo for view transitions and crossfades.
pub const MOTION_BASE_MS: u32 = 220;
/// Larger surfaces arriving/leaving — window open/close, overlay, panel slide.
pub const MOTION_SLOW_MS: u32 = 320;

/// The standard ease — a snappy, decisive ease-out (macOS "standard" curve).
/// Most transitions in the OS use this.
pub const MOTION_EASE_STANDARD: &str = "cubic-bezier(0.32, 0.72, 0, 1)";
/// The emphasized/spring ease — a gentle overshoot for surfaces that *arrive*
/// (the launcher, control center, panels). Restrained: one soft settle, no bounce.
pub const MOTION_EASE_SPRING: &str = "cubic-bezier(0.34, 1.4, 0.64, 1)";

/// The launcher / control-center overlay fade-scale duration (Rust animator).
pub const MOTION_OVERLAY_MS: u64 = 180;
/// The scale the overlay grows FROM on open (and TO on close): a subtle 4% pop.
pub const MOTION_OVERLAY_SCALE_FROM: f64 = 0.96;

/// Compose an app's structural CSS with the shared native design system for the
/// chosen color scheme. The theme tokens are emitted first (light or dark), then
/// the app's own CSS, then the shared structural rules — so every rule resolves
/// its colors from whichever token set is active. This is how Goblins OS offers
/// Light, Dark, and Auto across the whole OS from a single stylesheet.
pub fn native_css(app_css: &str, dark: bool) -> String {
    let tokens = if dark { DARK_TOKENS } else { LIGHT_TOKENS };
    format!("{tokens}\n{app_css}\n{GOBLINS_NATIVE_CSS}")
}

/// Light color scheme — white window surfaces and macOS-kit label ink.
/// Token names are shared with [`DARK_TOKENS`]; only the values differ, so every
/// structural rule themes automatically. Functional status hues (ready/waiting/
/// blocked) map to the kit's light and dark system values.
const LIGHT_TOKENS: &str = r#"
@define-color gos_canvas              #ffffff;
@define-color gos_canvas_top          #ffffff;
@define-color gos_surface             #ffffff;
@define-color gos_surface_muted       #f7f7f8;
@define-color gos_surface_sunken      #f2f2f4;

/* macOS 27 UI-kit semantic roles, translated to Goblins OS + Inter. These
   mirror the kit's label/fill/separator axes while staying project-owned. */
@define-color gos_label_primary       rgba(0, 0, 0, 0.85);
@define-color gos_label_secondary     rgba(0, 0, 0, 0.50);
@define-color gos_label_tertiary      rgba(0, 0, 0, 0.25);
@define-color gos_label_quaternary    rgba(0, 0, 0, 0.10);
@define-color gos_label_quinary       rgba(0, 0, 0, 0.05);
@define-color gos_label_sixth         rgba(0, 0, 0, 0.03);
@define-color gos_fill_primary        rgba(0, 0, 0, 0.10);
@define-color gos_fill_secondary      rgba(0, 0, 0, 0.08);
@define-color gos_fill_tertiary       rgba(0, 0, 0, 0.05);
@define-color gos_fill_quaternary     rgba(0, 0, 0, 0.03);
@define-color gos_fill_quinary        rgba(0, 0, 0, 0.02);
@define-color gos_separator           rgba(60, 60, 67, 0.29);

@define-color gos_system_red          rgba(255, 56, 60, 1);
@define-color gos_system_orange       rgba(255, 141, 40, 1);
@define-color gos_system_yellow       rgba(255, 204, 0, 1);
@define-color gos_system_green        rgba(52, 199, 89, 1);
@define-color gos_system_blue         rgba(0, 136, 255, 1);

@define-color gos_ink                 rgba(0, 0, 0, 0.85);
@define-color gos_ink_secondary       #3d3d42;
@define-color gos_ink_muted           #6e6e77;
@define-color gos_ink_faint           #74747e;

@define-color gos_hairline            rgba(0, 0, 0, 0.08);
@define-color gos_hairline_strong     rgba(0, 0, 0, 0.14);

@define-color gos_night_top           #161616;
@define-color gos_night_bottom        #000000;
@define-color gos_on_night            #f5f5f7;
@define-color gos_on_night_muted      rgba(245, 245, 247, 0.66);
@define-color gos_on_night_hairline   rgba(245, 245, 247, 0.16);

@define-color gos_accent              rgba(0, 136, 255, 1);
@define-color gos_ready               rgba(52, 199, 89, 1);
@define-color gos_ready_soft          rgba(52, 199, 89, 0.14);
@define-color gos_waiting             rgba(255, 141, 40, 1);
@define-color gos_blocked             rgba(255, 56, 60, 1);
@define-color gos_focus               rgba(0, 136, 255, 0.42);
@define-color gos_panel_sheen         rgba(255, 255, 255, 0.70);

/* macOS-blend vibrancy materials — the one translucent material language shared
   by every crafted chrome surface (launcher, control center, menu bar, dock).
   The compositor shows the wallpaper through these alphas; the top sheen + hairline
   give the glass real depth. Five thicknesses, matching the kit's material tiers. */
@define-color gos_material_ultra_thick rgba(252, 252, 253, 0.94);
@define-color gos_material_thick      rgba(252, 252, 253, 0.86);
@define-color gos_material_regular    rgba(250, 250, 252, 0.72);
@define-color gos_material_thin       rgba(255, 255, 255, 0.52);
@define-color gos_material_ultra_thin rgba(255, 255, 255, 0.42);
@define-color gos_material_border     rgba(0, 0, 0, 0.10);
@define-color gos_material_sheen      rgba(255, 255, 255, 0.85);
@define-color gos_material_shadow     rgba(13, 13, 12, 0.28);
@define-color gos_material_hover      rgba(0, 0, 0, 0.05);
@define-color gos_material_active     rgba(0, 0, 0, 0.09);

@define-color gos_primary_top         rgba(0, 136, 255, 1);
@define-color gos_primary_bottom      rgba(0, 120, 240, 1);
@define-color gos_primary_border      rgba(0, 92, 190, 0.48);
@define-color gos_on_primary          #ffffff;

@define-color gos_studio_bg           #ffffff;
@define-color gos_studio_sidebar      #f7f7f8;
@define-color gos_studio_panel        #ffffff;
@define-color gos_studio_border       rgba(0, 0, 0, 0.09);
@define-color gos_studio_border_soft  rgba(0, 0, 0, 0.06);
@define-color gos_studio_text         #1a1a1f;
@define-color gos_studio_text_muted   #5e5e68;
@define-color gos_studio_text_faint   #82828c;
@define-color gos_studio_hover        rgba(0, 0, 0, 0.04);
@define-color gos_studio_active       rgba(0, 0, 0, 0.07);
@define-color gos_studio_bubble       rgba(0, 0, 0, 0.05);
@define-color gos_studio_input        #ffffff;

/* Studio functional accents use the same system-blue action role. */
@define-color gos_studio_send_top     rgba(0, 136, 255, 1);
@define-color gos_studio_send_bottom  rgba(0, 120, 240, 1);
@define-color gos_studio_send_hover   rgba(0, 102, 214, 1);
@define-color gos_studio_send_text    #ffffff;
/* Studio tool chrome stays in the label/fill system instead of turning into a
   terminal palette. Tool labels and diffs read in ink weights (added = strong
   ink, removed = faint), and the working/done dots are muted-ink -> solid-ink. */
@define-color gos_studio_diff_add     #2f2f34;
@define-color gos_studio_diff_del     #8a8a92;
@define-color gos_studio_dot_working  #6e6e77;
@define-color gos_studio_dot_done     #1a1a1f;
@define-color gos_studio_dot_active   #1a1a1f;

/* ── Elevation ink (scheme-aware shadows) ─────────────────────────────────────
   macOS lifts layers with soft drop shadows on light, and with hairlines + a
   top sheen on dark (a near-black drop shadow on a dark canvas only muddies it).
   These baked-alpha inks let one box-shadow rule read correctly in both schemes:
   window = the floating settings window, panel = grouped cards, raise = hover. */
@define-color gos_shadow_window       rgba(13, 13, 12, 0.22);
@define-color gos_shadow_panel        rgba(13, 13, 12, 0.10);
@define-color gos_shadow_raise        rgba(13, 13, 12, 0.06);
@define-color gos_shadow_ambient      rgba(13, 13, 12, 0.16);

/* ── Sidebar category tints (colored rounded icon tiles) ──────────────────────
   Each Settings category carries a calm, saturated tile with a white glyph — the
   single strongest "system settings" cue, translated to project-owned hues (no
   Apple assets). Dark variants stay vivid against graphite. */
@define-color gos_tint_blue           rgba(0, 122, 255, 1);
@define-color gos_tint_teal           rgba(0, 178, 168, 1);
@define-color gos_tint_indigo         rgba(74, 92, 230, 1);
@define-color gos_tint_purple         rgba(162, 92, 222, 1);
@define-color gos_tint_pink           rgba(255, 64, 120, 1);
@define-color gos_tint_red            rgba(255, 69, 58, 1);
@define-color gos_tint_orange         rgba(255, 149, 10, 1);
@define-color gos_tint_yellow         rgba(240, 185, 20, 1);
@define-color gos_tint_green          rgba(40, 190, 90, 1);
@define-color gos_tint_graphite       rgba(99, 101, 113, 1);
@define-color gos_on_tint             #ffffff;
"#;

/// Dark color scheme — macOS-kit dark window surfaces and light ink; the same
/// design language, inverted. Night surfaces (login/lock/hero) are already dark, so they
/// hold across both schemes.
const DARK_TOKENS: &str = r#"
@define-color gos_canvas              #1e1e1e;
@define-color gos_canvas_top          #1e1e1e;
@define-color gos_surface             #1e1e1e;
@define-color gos_surface_muted       #242426;
@define-color gos_surface_sunken      #2c2c2e;

/* Same semantic axes as light mode. Alpha roles match the local macOS 27 kit;
   the exact font remains Inter, and these tokens are owned by Goblins OS. */
@define-color gos_label_primary       rgba(255, 255, 255, 1.00);
@define-color gos_label_secondary     rgba(255, 255, 255, 0.55);
@define-color gos_label_tertiary      rgba(255, 255, 255, 0.25);
@define-color gos_label_quaternary    rgba(255, 255, 255, 0.10);
@define-color gos_label_quinary       rgba(255, 255, 255, 0.05);
@define-color gos_label_sixth         rgba(255, 255, 255, 0.03);
@define-color gos_fill_primary        rgba(255, 255, 255, 0.10);
@define-color gos_fill_secondary      rgba(255, 255, 255, 0.08);
@define-color gos_fill_tertiary       rgba(255, 255, 255, 0.05);
@define-color gos_fill_quaternary     rgba(255, 255, 255, 0.03);
@define-color gos_fill_quinary        rgba(255, 255, 255, 0.02);
@define-color gos_separator           rgba(84, 84, 88, 0.65);

@define-color gos_system_red          rgba(255, 66, 69, 1);
@define-color gos_system_orange       rgba(255, 146, 48, 1);
@define-color gos_system_yellow       rgba(255, 214, 0, 1);
@define-color gos_system_green        rgba(48, 209, 88, 1);
@define-color gos_system_blue         rgba(0, 145, 255, 1);

@define-color gos_ink                 rgba(255, 255, 255, 1);
@define-color gos_ink_secondary       #c4c4cc;
@define-color gos_ink_muted           #9a9aa2;
@define-color gos_ink_faint           #8d8d97;

@define-color gos_hairline            rgba(255, 255, 255, 0.10);
@define-color gos_hairline_strong     rgba(255, 255, 255, 0.17);

@define-color gos_night_top           #161616;
@define-color gos_night_bottom        #000000;
@define-color gos_on_night            #f5f5f7;
@define-color gos_on_night_muted      rgba(245, 245, 247, 0.66);
@define-color gos_on_night_hairline   rgba(245, 245, 247, 0.16);

@define-color gos_accent              rgba(0, 145, 255, 1);
@define-color gos_ready               rgba(48, 209, 88, 1);
@define-color gos_ready_soft          rgba(48, 209, 88, 0.16);
@define-color gos_waiting             rgba(255, 146, 48, 1);
@define-color gos_blocked             rgba(255, 66, 69, 1);
@define-color gos_focus               rgba(0, 145, 255, 0.50);
@define-color gos_panel_sheen         rgba(255, 255, 255, 0.06);

/* The same vibrancy material tiers, inverted for night: graphite glass over the
   dark wallpaper. The sheen is a faint top-edge highlight, not a wash. */
@define-color gos_material_ultra_thick rgba(32, 32, 38, 0.94);
@define-color gos_material_thick      rgba(30, 30, 35, 0.88);
@define-color gos_material_regular    rgba(26, 26, 31, 0.76);
@define-color gos_material_thin       rgba(36, 36, 42, 0.55);
@define-color gos_material_ultra_thin rgba(44, 44, 52, 0.42);
@define-color gos_material_border     rgba(255, 255, 255, 0.12);
@define-color gos_material_sheen      rgba(255, 255, 255, 0.10);
@define-color gos_material_shadow     rgba(0, 0, 0, 0.55);
@define-color gos_material_hover      rgba(255, 255, 255, 0.07);
@define-color gos_material_active     rgba(255, 255, 255, 0.11);

/* Dark primary CTAs use the kit's dark blue, matching the same system action
   role as light mode. */
@define-color gos_primary_top         rgba(0, 145, 255, 1);
@define-color gos_primary_bottom      rgba(10, 153, 255, 1);
@define-color gos_primary_border      rgba(92, 184, 255, 0.46);
@define-color gos_on_primary          #ffffff;

@define-color gos_studio_bg           #1e1e1e;
@define-color gos_studio_sidebar      #242426;
@define-color gos_studio_panel        #1e1e1e;
@define-color gos_studio_border       rgba(255, 255, 255, 0.08);
@define-color gos_studio_border_soft  rgba(255, 255, 255, 0.06);
@define-color gos_studio_text         #ededf0;
@define-color gos_studio_text_muted   #a6a6b0;
@define-color gos_studio_text_faint   #90909a;
@define-color gos_studio_hover        rgba(255, 255, 255, 0.05);
@define-color gos_studio_active       rgba(255, 255, 255, 0.08);
@define-color gos_studio_bubble       rgba(255, 255, 255, 0.08);
@define-color gos_studio_input        #1e1e1e;

/* Studio functional accents use the same dark system-blue action role. */
@define-color gos_studio_send_top     rgba(0, 145, 255, 1);
@define-color gos_studio_send_bottom  rgba(10, 153, 255, 1);
@define-color gos_studio_send_hover   rgba(92, 184, 255, 1);
@define-color gos_studio_send_text    #ffffff;
/* Same label/fill discipline inverted for dark. Added = bright ink, removed =
   faint; working = muted, done = bright. */
@define-color gos_studio_diff_add     #ededf0;
@define-color gos_studio_diff_del     #8d8d97;
@define-color gos_studio_dot_working  #9a9aa2;
@define-color gos_studio_dot_done     #ededf0;
@define-color gos_studio_dot_active   #ededf0;

/* Elevation ink, inverted for night: the window keeps a real drop shadow so it
   separates from the desktop, grouped cards lean on hairlines + sheen, and the
   hover raise is suppressed (a black shadow on graphite only reads as grime). */
@define-color gos_shadow_window       rgba(0, 0, 0, 0.52);
@define-color gos_shadow_panel        rgba(0, 0, 0, 0.34);
@define-color gos_shadow_raise        rgba(0, 0, 0, 0.0);
@define-color gos_shadow_ambient      rgba(0, 0, 0, 0.42);

/* Same category tints, brightened so the tiles stay luminous on graphite. */
@define-color gos_tint_blue           rgba(10, 132, 255, 1);
@define-color gos_tint_teal           rgba(38, 200, 190, 1);
@define-color gos_tint_indigo         rgba(98, 114, 240, 1);
@define-color gos_tint_purple         rgba(182, 120, 235, 1);
@define-color gos_tint_pink           rgba(255, 92, 142, 1);
@define-color gos_tint_red            rgba(255, 88, 80, 1);
@define-color gos_tint_orange         rgba(255, 165, 42, 1);
@define-color gos_tint_yellow         rgba(245, 200, 52, 1);
@define-color gos_tint_green          rgba(48, 209, 100, 1);
@define-color gos_tint_graphite       rgba(120, 122, 134, 1);
@define-color gos_on_tint             #ffffff;
"#;

pub const GOBLINS_NATIVE_CSS: &str = r#"

/* ── Base ────────────────────────────────────────────────────────────── */
* {
  /* Inter is the shipped brand-appropriate runtime font. */
  font-family: "Inter", "Noto Sans", sans-serif;
  font-weight: 400;
  letter-spacing: 0;
  outline: none;
}

window {
  background: @gos_canvas;
  color: @gos_ink;
}

.gos-root,
.gos-login-root,
.gos-installer-root,
.gos-settings-root {
  padding: 24px;
  background:
    linear-gradient(180deg, alpha(@gos_surface_muted, 0.54), alpha(@gos_canvas, 1.0) 42%),
    @gos_canvas;
}

/* Unlocked, the shell is a WINDOW on the desktop, not a full-screen takeover: a
   rounded, shadowed surface floating over the wallpaper. The window itself is
   transparent, so the corners and the drop shadow reveal the live desktop behind.
   (The first-boot lock / identity gate keeps the opaque full-screen canvas.) */
window.gos-windowed {
  background: transparent;
}

window.gos-windowed .gos-root {
  margin: 28px;
  border-radius: 16px;
  border: 1px solid @gos_hairline;
  box-shadow: 0 1px 0 @gos_panel_sheen inset,
              0 24px 62px @gos_shadow_window;
}

/* ── Top bars ────────────────────────────────────────────────────────── */
.gos-top-bar,
.gos-login-top,
.gos-installer-top,
.gos-settings-top {
  padding: 12px 16px;
  min-height: 32px;
  border: 1px solid @gos_hairline;
  border-radius: 14px;
  background: alpha(@gos_surface, 0.82);
  box-shadow: 0 1px 0 @gos_panel_sheen inset,
              0 8px 24px @gos_shadow_raise;
}

.gos-installer-body,
.gos-login-body,
.gos-settings-body {
  margin-top: 18px;
}

.gos-window-controls {
  margin-right: 12px;
}

.gos-window-control {
  min-width: 12px;
  min-height: 12px;
  padding: 0;
  border-radius: 999px;
  border: 1px solid @gos_hairline_strong;
  color: transparent;
  box-shadow: 0 1px 0 alpha(@gos_material_sheen, 0.55) inset,
              0 2px 5px rgba(13, 13, 12, 0.10);
  transition: color 140ms cubic-bezier(0.32, 0.72, 0, 1),
              box-shadow 140ms cubic-bezier(0.32, 0.72, 0, 1);
}

.gos-window-close {
  background: #ff5f57;
  border-color: rgba(122, 20, 24, 0.22);
}

.gos-window-minimize {
  background: #ffbd2e;
  border-color: rgba(116, 76, 9, 0.24);
}

.gos-window-zoom {
  background: #28c840;
  border-color: rgba(26, 98, 33, 0.24);
}

.gos-window-control:hover {
  color: rgba(0, 0, 0, 0.58);
  box-shadow: 0 1px 0 rgba(255, 255, 255, 0.72) inset,
              0 4px 10px rgba(13, 13, 12, 0.16);
}

.gos-window-control:active {
  box-shadow: 0 1px 0 rgba(255, 255, 255, 0.48) inset;
}

/* ── Light panels ────────────────────────────────────────────────────── */
.gos-workspace-panel,
.gos-system-panel,
.gos-state-panel,
.gos-checks-panel,
.gos-install-panel,
.gos-model-panel,
.gos-side-panel,
.gos-main-panel {
  padding: 28px;
  border: 1px solid @gos_hairline;
  border-radius: 14px;
  background: alpha(@gos_surface, 0.9);
  /* Layered macOS-grade elevation: a hairline top sheen, a tight contact shadow,
     and a soft ambient cast — depth without heaviness. */
  box-shadow: 0 1px 0 @gos_panel_sheen inset,
              0 2px 6px @gos_shadow_panel,
              0 12px 34px @gos_shadow_ambient;
}

/* ── Night surfaces (login, lock, hero, identity) ────────────────────── */
.gos-session-lock,
.gos-lock-panel,
.gos-identity-panel,
.gos-hero-panel {
  padding: 34px;
  border: 1px solid @gos_on_night_hairline;
  border-radius: 16px;
  color: @gos_on_night;
  background: linear-gradient(157deg, @gos_night_top, @gos_night_bottom);
  /* A faint top-edge sheen (light catching the upper lip) gives the graphite
     surface real depth and matches the inset highlight the light panels carry,
     so both schemes share one elevation language. */
  box-shadow: 0 1px 0 rgba(255, 255, 255, 0.08) inset,
              0 14px 36px rgba(10, 10, 9, 0.22);
}

/* Kickers on the night panels are scheme-invariant like everything else on
   them — the paper-scheme kicker token would read muddy in light. */
.gos-identity-panel .gos-kicker,
.gos-hero-panel .gos-kicker {
  color: @gos_on_night_muted;
}

/* The onboarding scroller is structural — its viewport must never paint a
   theme background over the page's canvas gradient. */
.gos-onboarding-root scrolledwindow,
.gos-onboarding-root viewport {
  background: transparent;
}

/* ── Brand + meta type ───────────────────────────────────────────────── */
.gos-brand {
  color: @gos_ink;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0;
}

.gos-kicker,
.gos-panel-source,
.gos-resident-title {
  color: @gos_label_secondary;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.6px;
  text-transform: uppercase;
}

.gos-muted,
.gos-footnote,
.gos-row-copy,
.gos-app-copy,
.gos-launch-feedback,
.gos-state-label {
  color: @gos_label_secondary;
  font-size: 14px;
}

.gos-footnote {
  color: @gos_label_tertiary;
  font-size: 12px;
}

/* ── Display + hero type ─────────────────────────────────────────────── */
.gos-section-title {
  color: @gos_ink;
  font-size: 28px;
  font-weight: 600;
  letter-spacing: 0;
}

.gos-hero-title,
.gos-lock-title {
  /* Hero and lock titles always sit on the dark night gradient, so the title is
     white in both schemes by design — intentionally literal, not tokenized. */
  color: #ffffff;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-hero-title {
  font-size: 42px;
}

.gos-lock-title {
  font-size: 52px;
  letter-spacing: 0;
}

.gos-hero-copy,
.gos-lock-copy,
.gos-feedback {
  color: @gos_on_night_muted;
  font-size: 15px;
}

/* ── Rows, tiles, commands ───────────────────────────────────────────── */
.gos-row,
.gos-system-row,
.gos-command {
  padding: 14px 16px;
  border: 1px solid @gos_hairline;
  border-radius: 10px;
  background: @gos_fill_tertiary;
  transition: box-shadow 160ms ease-out, background 160ms ease-out, border 160ms ease-out;
}

.gos-row:hover,
.gos-system-row:hover,
.gos-command:hover {
  background: @gos_fill_secondary;
  border: 1px solid @gos_hairline_strong;
  box-shadow: 0 6px 16px @gos_shadow_raise;
}

.gos-app-grid,
.gos-mode-row {
  margin-top: 6px;
}

.gos-app-tile {
  min-width: 244px;
  min-height: 150px;
  padding: 20px;
  border: 1px solid @gos_hairline;
  border-radius: 14px;
  background: linear-gradient(180deg, @gos_surface, @gos_surface_muted);
  box-shadow: 0 8px 24px @gos_shadow_raise;
  transition: box-shadow 160ms ease-out, border 160ms ease-out;
}

.gos-app-tile:hover {
  border: 1px solid @gos_hairline_strong;
  box-shadow: 0 8px 20px @gos_shadow_raise;
}

.gos-row-title,
.gos-app-title {
  color: @gos_ink;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0;
}

.gos-app-title {
  font-size: 16px;
}

/* ── Actions ─────────────────────────────────────────────────────────── */
.gos-primary-action,
.gos-secondary-action,
.gos-local-action,
.gos-permission-action,
.gos-mode,
.gos-mode-selected,
.gos-disabled-action {
  min-height: 46px;
  padding: 0 20px;
  border-radius: 10px;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0;
  transition: box-shadow 160ms ease-out, background 160ms ease-out,
              border 160ms ease-out, opacity 160ms ease-out;
}

.gos-primary-action {
  color: @gos_on_primary;
  background: linear-gradient(180deg, @gos_primary_top, @gos_primary_bottom);
  border: 1px solid @gos_primary_border;
  box-shadow: 0 6px 16px rgba(13, 13, 12, 0.16);
}

.gos-primary-action:hover {
  box-shadow: 0 10px 24px rgba(13, 13, 12, 0.20);
}

/* Night-surface primary actions use the same system-blue action role. */
.gos-identity-panel .gos-primary-action,
.gos-lock-panel .gos-primary-action,
.gos-session-lock .gos-primary-action,
.gos-hero-panel .gos-primary-action {
  color: #ffffff;
  background: linear-gradient(180deg, @gos_primary_top, @gos_primary_bottom);
  border: 1px solid @gos_primary_border;
  box-shadow: 0 8px 20px rgba(0, 92, 190, 0.20);
}

.gos-secondary-action {
  color: @gos_on_night;
  border: 1px solid @gos_on_night_hairline;
  background: rgba(250, 249, 246, 0.06);
}

.gos-secondary-action:hover {
  background: rgba(250, 249, 246, 0.12);
}

.gos-local-action,
.gos-permission-action,
.gos-mode {
  color: @gos_ink;
  border: 1px solid @gos_hairline_strong;
  background: @gos_surface;
}

.gos-local-action:hover,
.gos-permission-action:hover,
.gos-mode:hover {
  box-shadow: 0 8px 20px @gos_shadow_raise;
}

.gos-mode-selected {
  color: @gos_on_primary;
  background: linear-gradient(180deg, @gos_primary_top, @gos_primary_bottom);
  border: 1px solid @gos_primary_border;
}

.gos-disabled-action {
  color: @gos_ink_faint;
  border: 1px solid @gos_hairline;
  background: @gos_surface_sunken;
  opacity: 0.7;
}

/* On night surfaces a disabled action is a true ghost — dimmed label,
   transparent fill, half-strength hairline — unmistakably inert next to a
   live sibling. */
.gos-identity-panel .gos-disabled-action,
.gos-lock-panel .gos-disabled-action,
.gos-hero-panel .gos-disabled-action,
.gos-session-lock .gos-disabled-action {
  color: rgba(245, 245, 247, 0.40);
  border: 1px solid rgba(245, 245, 247, 0.06);
  background: transparent;
  opacity: 1;
}

/* ── Status pills ────────────────────────────────────────────────────── */
.gos-status-pill {
  padding: 5px 10px;
  border: 1px solid transparent;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0;
}

/* Inline + top-bar status pills carry functional color. */
.gos-ready {
  color: @gos_ready;
  background: @gos_ready_soft;
  border-color: alpha(@gos_ready, 0.34);
}

.gos-waiting {
  color: @gos_waiting;
  background: alpha(@gos_waiting, 0.12);
  border-color: alpha(@gos_waiting, 0.36);
}

/* App tiles stay neutral by default — readiness reads from the calm surface, so
   color is reserved for attention states and only tints the status line, drawing
   the eye to what needs action rather than flooding the launcher with green. */
.gos-app-gated .gos-state-label,
.gos-app-waiting .gos-state-label {
  color: @gos_waiting;
}

.gos-app-blocked .gos-state-label,
.gos-model-unavailable {
  color: @gos_blocked;
}

.gos-app-blocked {
  opacity: 0.72;
}

.gos-model-available {
  border: 1px solid @gos_ready_soft;
}

.gos-model-unavailable {
  opacity: 0.6;
  border: 1px solid rgba(140, 58, 46, 0.2);
}

/* ── Resident strip + stage ──────────────────────────────────────────── */
.gos-resident {
  padding: 16px 20px;
  border: 1px solid @gos_hairline;
  border-radius: 16px;
  background: alpha(@gos_surface, 0.9);
  box-shadow: 0 8px 24px @gos_shadow_panel;
}

.gos-stage {
  padding: 4px;
}

/* ── Interaction polish ──────────────────────────────────────────────────
   Immediate, tactile feedback on press (HIG: respond the instant a control is
   touched). The whole OS uses the calm, restrained motion appropriate to a
   productivity surface — no springy flourishes — and GTK disables every
   transition here automatically when the desktop's Reduce Motion
   (gtk-enable-animations) is off, so the reduced-motion path is a clean cut. */
/* Ring only for keyboard navigation — implicit window-open or pointer focus
   paints no ring (Adwaita behavior), so a focused control never competes with
   the active/selected treatment. */
button:focus:focus-visible {
  box-shadow: 0 0 0 3px @gos_focus;
}

button:active {
  opacity: 0.9;
}

/* A pressed primary sinks into the surface — its raised elevation drops to the
   resting tier for the duration of the press, the physical "pushed in" feel. */
.gos-primary-action:active,
.gos-onboarding-primary:active,
.gos-home-build:active,
.gos-mode-selected:active {
  box-shadow: 0 1px 2px rgba(13, 13, 12, 0.18);
}

/* ── Onboarding (first boot): full-bleed, centered, calm ─────────────────
   Apple-minimal pacing — one screen, one focus, generous whitespace — in the
   Goblins-native light palette. */
.gos-onboarding-root {
  padding: 48px;
  background:
    linear-gradient(180deg, alpha(@gos_surface_muted, 0.58), alpha(@gos_canvas, 1.0) 46%),
    @gos_canvas;
}

.gos-onboarding-kicker {
  margin-bottom: 22px;
  color: @gos_ink_muted;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0;
  text-transform: none;
}

.gos-onboarding-title {
  margin-bottom: 18px;
  color: @gos_ink;
  font-size: 48px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-onboarding-subtitle {
  margin-bottom: 40px;
  color: @gos_ink_secondary;
  font-size: 18px;
}

.gos-onboarding-primary {
  min-width: 340px;
  min-height: 54px;
  margin-bottom: 12px;
  padding: 0 30px;
  border-radius: 13px;
  font-size: 16px;
  font-weight: 700;
  color: @gos_on_primary;
  background: linear-gradient(180deg, @gos_primary_top, @gos_primary_bottom);
  border: 1px solid @gos_primary_border;
  box-shadow: 0 7px 18px rgba(13, 13, 12, 0.17);
}

.gos-onboarding-primary:hover {
  box-shadow: 0 10px 24px rgba(13, 13, 12, 0.21);
}

.gos-onboarding-primary:focus:focus-visible {
  box-shadow: 0 0 0 3px @gos_focus,
              0 8px 20px rgba(13, 13, 12, 0.22);
}

/* A disabled primary must read as disabled — the install flow leaves Continue and
   "Erase disk and install" disabled until a disk is chosen and the exact phrase is
   typed, and an honest OS never shows a heavy, clickable-looking button it won’t
   act on. */
/* A disabled primary recedes into the same calm sunken pill every other disabled
   control uses (.gos-disabled-action) — the dark gradient is replaced by a sunken
   surface with faint ink, so it reads as genuinely inert, not a pale dark button. */
.gos-onboarding-primary:disabled {
  color: @gos_ink_faint;
  background: @gos_surface_sunken;
  border: 1px solid @gos_hairline;
  box-shadow: none;
  opacity: 0.7;
}

.gos-onboarding-secondary {
  min-width: 340px;
  min-height: 50px;
  margin-bottom: 6px;
  padding: 0 30px;
  border-radius: 13px;
  font-size: 15px;
  /* One weight lighter than the 700 primary so the primary clearly out-ranks it,
     and a defined sunken fill (not a near-page-white wash) so it reads as a real
     surface on both light and dark canvases. */
  font-weight: 600;
  color: @gos_ink;
  background: @gos_surface_sunken;
  border: 1px solid @gos_hairline_strong;
}

.gos-onboarding-secondary:hover {
  background: @gos_surface;
  border-color: @gos_focus;
}

.gos-onboarding-quiet {
  min-height: 32px;
  color: @gos_ink_muted;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 14px;
  font-weight: 600;
}

.gos-onboarding-quiet:hover {
  color: @gos_ink;
}

/* Safety-critical install caption — one tier brighter than a footnote. */
.gos-install-warning {
  margin-top: 28px;
  color: @gos_ink_muted;
  font-size: 13px;
}

.gos-onboarding-footnote {
  /* The caption belongs to the copy above it, not to the quiet actions below —
     the bottom margin keeps the action pair reading as one group. */
  margin-top: 28px;
  margin-bottom: 14px;
  color: @gos_ink_faint;
  font-size: 13px;
}

/* ── Command-Space home (the shell desktop) ──────────────────────────────
   The entire home is one centered command field: describe an app and the
   on-device GPT-OSS model builds it. Calm, macOS-grade, radically minimal. */
.gos-home-root {
  padding: 24px 28px;
  background:
    linear-gradient(180deg, alpha(@gos_surface_muted, 0.36), alpha(@gos_canvas, 1.0) 52%),
    @gos_canvas;
}

.gos-home-wordmark {
  color: @gos_ink_secondary;
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0;
}

.gos-home-kicker {
  margin-bottom: 22px;
  color: @gos_ink_faint;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 2.4px;
  text-transform: uppercase;
}

.gos-home-headline {
  color: @gos_ink;
  font-size: 38px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-home-sub {
  margin-top: 14px;
  margin-bottom: 30px;
  color: @gos_ink_secondary;
  font-size: 15px;
  font-weight: 400;
  letter-spacing: 0;
}

.gos-home-field {
  padding: 6px;
  border: 1px solid @gos_hairline_strong;
  border-radius: 14px;
  background: @gos_surface;
  box-shadow: 0 1px 0 @gos_panel_sheen inset,
              0 8px 24px @gos_shadow_raise;
  transition: box-shadow 180ms ease-out, border 180ms ease-out;
}

.gos-home-field:focus-within {
  border: 1px solid @gos_focus;
  box-shadow: 0 1px 0 @gos_panel_sheen inset,
              0 12px 32px @gos_shadow_raise;
}

.gos-home-entry {
  min-height: 52px;
  padding: 0 12px;
  border: none;
  background: transparent;
  box-shadow: none;
  color: @gos_ink;
  font-size: 16px;
  font-weight: 400;
  letter-spacing: 0;
}

.gos-home-entry:focus {
  box-shadow: none;
  outline: none;
}

.gos-home-entry > text {
  background: transparent;
  color: @gos_ink;
  caret-color: @gos_ink;
}

.gos-home-build {
  min-height: 46px;
  padding: 0 22px;
  border-radius: 12px;
  color: @gos_on_primary;
  background: linear-gradient(180deg, @gos_primary_top, @gos_primary_bottom);
  border: 1px solid @gos_primary_border;
  font-size: 15px;
  font-weight: 700;
  letter-spacing: 0;
  box-shadow: 0 6px 16px rgba(13, 13, 12, 0.16);
  transition: box-shadow 160ms ease-out, opacity 160ms ease-out;
}

.gos-home-build:hover {
  box-shadow: 0 10px 24px rgba(13, 13, 12, 0.20);
}

/* Inset in the build field, the Build pill is a CHILD of one material: the field
   owns the elevation (no double shadow), and its corners are concentric with the
   field (field radius 16 − field padding 6 = 10). */
.gos-home-field .gos-home-build {
  box-shadow: none;
  border-radius: 10px;
}

.gos-home-field .gos-home-build:hover {
  box-shadow: none;
  background: linear-gradient(180deg, @gos_primary_top, @gos_primary_bottom);
  opacity: 0.92;
}

.gos-home-build:disabled {
  color: @gos_ink_faint;
  background: @gos_surface_sunken;
  border: 1px solid @gos_hairline;
  box-shadow: none;
  opacity: 0.7;
}

/* Dots + status share one fixed-height slot so toggling the working state
   never shifts the hero stack. */
.gos-home-status-slot {
  min-height: 72px;
}

.gos-home-status {
  margin-top: 14px;
  color: @gos_ink_faint;
  font-size: 13px;
  font-weight: 400;
}

.gos-home-status-working {
  color: @gos_ink_secondary;
  font-weight: 600;
}

.gos-home-status-error {
  color: @gos_blocked;
  font-weight: 600;
}

.gos-home-ledger-kicker {
  margin-top: 46px;
  margin-bottom: 6px;
  color: @gos_ink_faint;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 1.8px;
  text-transform: uppercase;
}

.gos-home-app-row {
  padding: 14px 16px;
  border: 1px solid transparent;
  border-radius: 12px;
  background: transparent;
  transition: background 160ms ease-out, border 160ms ease-out, box-shadow 160ms ease-out;
}

.gos-home-app-row:hover {
  background: @gos_surface;
  border: 1px solid @gos_hairline;
  box-shadow: 0 8px 20px @gos_shadow_raise;
}

.gos-home-app-name {
  color: @gos_ink;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0;
}

.gos-home-app-meta {
  color: @gos_ink_muted;
  font-size: 12px;
  font-weight: 400;
}

.gos-home-app-time {
  color: @gos_ink_faint;
  font-size: 12px;
  font-weight: 500;
}

.gos-home-app-more {
  margin-top: 4px;
  color: @gos_ink_faint;
  font-size: 12px;
  font-weight: 500;
}

.gos-home-empty {
  margin-top: 40px;
  color: @gos_ink_faint;
  font-size: 13px;
  font-weight: 400;
}

.gos-home-settings {
  min-height: 32px;
  padding: 0 12px;
  color: @gos_ink_muted;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 13px;
  font-weight: 600;
}

.gos-home-settings:hover {
  color: @gos_ink;
}

/* The detail page's back link: cancel the quiet button's own side padding so
   the arrow glyph sits flush on the content column's left edge. */
.gos-detail-back {
  margin-left: -12px;
}

/* The built-app payload (your intent, the OS's plan) is content, not caption —
   full ink in both schemes. */
.gos-detail-body {
  color: @gos_ink;
  font-size: 15px;
}

/* Three-dot "thinking" pulse shown while the on-device model is working. The
   breathing is animated per-dot on the frame clock (opacity); these rules only
   set the resting shape and the calm OpenAI-style spacing. */
.gos-thinking {
  margin-top: 16px;
  margin-bottom: 2px;
}

/* Inside a panel card the card's own padding paces the rhythm — the pulse
   carries no extra margins, keeping the content optically centered. */
.gos-net-panel .gos-thinking {
  margin-top: 0;
  margin-bottom: 0;
}

.gos-thinking-dot {
  min-width: 9px;
  min-height: 9px;
  border-radius: 999px;
  background: @gos_ink;
}

/* Talk-to-the-OS affordance on the home. A calm pill when local voice is ready;
   recedes to a quiet, disabled ghost when the voice models aren’t present yet. */
.gos-home-voice {
  min-height: 40px;
  padding: 0 18px;
  border-radius: 999px;
  color: @gos_ink;
  background: alpha(@gos_surface, 0.7);
  border: 1px solid @gos_hairline_strong;
  font-size: 14px;
  font-weight: 600;
  transition: box-shadow 160ms ease-out, background 160ms ease-out;
}

.gos-home-voice:hover {
  background: @gos_surface;
  box-shadow: 0 8px 20px @gos_shadow_raise;
}

.gos-home-voice:disabled {
  color: @gos_ink_faint;
  border-color: @gos_hairline;
  background: transparent;
  opacity: 0.85;
}

/* ── Build Studio: a minimal, multi-engine agent surface ─────────────────
   Switch the engine (GPT-OSS · Codex · your key), describe what to build, and
   watch the agent answer and produce files — one calm GUI, whichever brain runs. */
.gos-home-studio-link {
  min-height: 36px;
  color: @gos_ink_muted;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 13px;
  font-weight: 600;
}

.gos-home-studio-link:hover {
  color: @gos_ink;
}

/* The Studio is the OS's developer surface; it themes with the OS (Light, Dark,
   or Auto) through the gos_studio_* tokens. Functional color follows the same
   scheme-specific system tones as the rest of the shell. */
.gos-studio-root {
  background: @gos_studio_bg;
}

.gos-studio-sidebar {
  min-width: 248px;
  padding: 12px 16px 16px 16px;
  background: @gos_studio_sidebar;
  border-right: 1px solid @gos_studio_border_soft;
}

.gos-studio-wordmark {
  color: @gos_studio_text;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-studio-badge {
  padding: 2px 7px;
  border-radius: 6px;
  border: 1px solid @gos_studio_border;
  color: @gos_studio_text_muted;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.4px;
  text-transform: uppercase;
}

.gos-studio-search {
  min-height: 34px;
  margin-top: 10px;
  padding: 0 12px;
  border-radius: 8px;
  border: 1px solid @gos_studio_border;
  background: @gos_studio_input;
  color: @gos_studio_text_muted;
  font-size: 13px;
}

.gos-studio-search > text {
  color: @gos_studio_text;
  caret-color: @gos_studio_text;
}

.gos-studio-section {
  margin-top: 16px;
  margin-bottom: 6px;
  color: @gos_studio_text_faint;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 1.2px;
  text-transform: uppercase;
}

.gos-studio-project {
  padding: 6px 8px;
  border-radius: 8px;
  color: @gos_studio_text;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 13px;
  font-weight: 600;
}

.gos-studio-project:hover {
  background: @gos_studio_hover;
}

.gos-studio-thread-item {
  padding: 8px 8px 8px 20px;
  border-radius: 8px;
  color: @gos_studio_text_muted;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 13px;
  font-weight: 500;
}

.gos-studio-thread-item:hover {
  background: @gos_studio_hover;
  color: @gos_studio_text;
}

.gos-studio-thread-item.is-active {
  background: @gos_studio_active;
  color: @gos_studio_text;
}

.gos-studio-time {
  color: @gos_studio_text_faint;
  font-size: 11px;
}

/* On the selected row the hover tint eats faint-text contrast — lift the
   timestamp one tier so metadata stays legible where the user is looking. */
.gos-studio-thread-item.is-active .gos-studio-time {
  color: @gos_studio_text_muted;
}

.gos-studio-dot {
  min-width: 8px;
  min-height: 8px;
  border-radius: 999px;
  background: @gos_studio_text_faint;
}

.gos-studio-dot.is-working {
  background: @gos_studio_dot_working;
}

.gos-studio-dot.is-done {
  background: @gos_studio_dot_done;
}

.gos-studio-dot.is-active {
  background: @gos_studio_dot_active;
}

.gos-studio-add {
  margin-top: 8px;
  min-height: 34px;
  color: @gos_studio_text_muted;
  background: transparent;
  border: 1px dashed @gos_studio_border;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
}

/* Navigation sibling of the create button — solid hairline, never the
   dashed create affordance. */
.gos-studio-home {
  margin-top: 8px;
  min-height: 34px;
  color: @gos_studio_text_muted;
  background: transparent;
  border: 1px solid @gos_studio_border;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
}

.gos-studio-home:hover {
  color: @gos_studio_text;
  background: @gos_studio_hover;
}

.gos-studio-add:hover {
  color: @gos_studio_text;
  border-color: @gos_studio_text_faint;
}

.gos-studio-main {
  background: @gos_studio_bg;
}

.gos-studio-topbar {
  padding: 12px 18px;
  border-bottom: 1px solid @gos_studio_border_soft;
  background: alpha(@gos_studio_panel, 0.78);
}

.gos-studio-thread-title {
  color: @gos_studio_text;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-studio-crumb {
  color: @gos_studio_text_muted;
  font-size: 13px;
}

.gos-studio-conv {
  padding: 20px 22px;
}

.gos-studio-conv-scroll {
  background: @gos_studio_bg;
}

.gos-studio-msg-role {
  color: @gos_studio_text_faint;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.6px;
  text-transform: uppercase;
}

/* A new turn (yours or the agent's) opens with more air than the cards
   inside a turn, so the conversation groups by turn at a glance. */
.gos-studio-msg:not(:first-child) {
  margin-top: 20px;
}

.gos-studio-msg-you {
  padding: 12px 16px;
  border-radius: 12px;
  background: @gos_studio_bubble;
}

.gos-studio-msg-text {
  color: @gos_studio_text;
  font-size: 14px;
}

.gos-studio-msg-you .gos-studio-msg-text {
  color: @gos_studio_text;
}

/* Tool-call and changed-files blocks: raised cards with monospace detail, the
   way the agent's work reads in a real coding agent. */
.gos-studio-block {
  padding: 12px 16px;
  border-radius: 12px;
  border: 1px solid @gos_studio_border;
  background: @gos_studio_panel;
}

.gos-studio-block-head {
  margin-bottom: 8px;
  color: @gos_studio_text_muted;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 1px;
  text-transform: uppercase;
}

.gos-studio-diff-file {
  color: @gos_studio_text_muted;
  font-family: monospace;
  font-size: 12px;
}

.gos-studio-diff-add {
  color: @gos_studio_diff_add;
  font-family: monospace;
  font-size: 12px;
  font-weight: 700;
}

.gos-studio-diff-del {
  color: @gos_studio_diff_del;
  font-family: monospace;
  font-size: 12px;
  font-weight: 700;
}

/* Composer: the input with a model/reasoning/mode/access control row and a round
   send button. The model picker is our engine switch. */
.gos-studio-composer {
  margin: 14px 22px 0 22px;
  padding: 12px 16px;
  border-radius: 16px;
  border: 1px solid @gos_studio_border;
  background: @gos_studio_panel;
}

.gos-studio-input {
  min-height: 28px;
  color: @gos_studio_text;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 14px;
}

.gos-studio-input > text {
  color: @gos_studio_text;
  caret-color: @gos_studio_text;
}

.gos-studio-controls {
  margin-top: 10px;
}

.gos-studio-control {
  min-height: 30px;
  padding: 0 11px;
  border-radius: 8px;
  color: @gos_studio_text_muted;
  background: @gos_studio_hover;
  border: 1px solid @gos_studio_border;
  font-size: 12px;
  font-weight: 600;
}

.gos-studio-control:hover {
  color: @gos_studio_text;
  background: @gos_studio_active;
}

/* Keyboard focus for the quiet composer controls — a 2px ring sized to their
   smaller scale, so every Studio control is reachable without a pointer. */
.gos-studio-control:focus:focus-visible {
  box-shadow: 0 0 0 2px @gos_focus;
}

.gos-studio-engine {
  color: @gos_studio_text;
  background: @gos_studio_active;
  border-color: @gos_studio_border;
}

.gos-studio-send {
  min-width: 36px;
  min-height: 36px;
  border-radius: 999px;
  color: @gos_studio_send_text;
  background: linear-gradient(180deg, @gos_studio_send_top, @gos_studio_send_bottom);
  border: none;
  font-size: 15px;
  font-weight: 700;
}

.gos-studio-send:hover {
  background: linear-gradient(180deg, @gos_studio_send_hover, @gos_studio_send_top);
}

.gos-studio-send:disabled {
  opacity: 0.4;
}

.gos-studio-footer {
  padding: 8px 18px;
  border-top: 1px solid @gos_studio_border_soft;
  color: @gos_studio_text_muted;
  font-size: 12px;
}

.gos-studio-empty {
  color: @gos_studio_text_muted;
  font-size: 14px;
}

/* ── Onboarding network step (installer) ─────────────────────────────────
   Get connected so the OS can download GPT-OSS and let the AI build apps —
   prominent but never blocking; an offline user is always free to continue. */
.gos-net-panel {
  margin-top: 26px;
  padding: 20px;
  border: 1px solid @gos_hairline;
  border-radius: 16px;
  background: alpha(@gos_surface, 0.92);
  box-shadow: 0 1px 0 @gos_panel_sheen inset,
              0 8px 24px @gos_shadow_panel;
}

.gos-net-title {
  margin-bottom: 16px;
  color: @gos_ink;
  font-size: 42px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-net-status-label {
  color: @gos_ink;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-net-dot {
  min-width: 10px;
  min-height: 10px;
  border-radius: 999px;
  background: @gos_ink_faint;
}

.gos-net-dot.is-online {
  background: @gos_ready;
}

.gos-net-dot.is-connecting {
  background: @gos_waiting;
}

.gos-net-dot.is-blocked {
  background: @gos_blocked;
  opacity: 0.8;
}

.gos-net-rescan {
  min-height: 32px;
  padding: 0 10px;
  /* Flush with the card's content rail; the padding stays as hit area. */
  margin-right: -10px;
  color: @gos_ink_muted;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 13px;
  font-weight: 600;
}

.gos-net-rescan:hover {
  color: @gos_ink;
}

.gos-net-row {
  padding: 13px 14px;
  border: 1px solid @gos_hairline;
  border-radius: 12px;
  background: alpha(@gos_surface, 0.7);
  color: @gos_ink_secondary;
  font-size: 13px;
}

.gos-net-row.gos-blocked-soft {
  border: 1px solid rgba(140, 58, 46, 0.2);
  color: @gos_blocked;
}

.gos-net-ssid {
  padding: 12px 16px;
  border: 1px solid @gos_hairline;
  border-radius: 12px;
  background: alpha(@gos_surface, 0.7);
  transition: box-shadow 160ms ease-out, background 160ms ease-out, border 160ms ease-out;
}

.gos-net-ssid:hover {
  background: @gos_surface;
  border: 1px solid @gos_hairline_strong;
  box-shadow: 0 6px 16px @gos_shadow_raise;
}

.gos-net-ssid-selected {
  border: 1px solid @gos_hairline_strong;
  background: @gos_surface;
  box-shadow: 0 1px 0 @gos_panel_sheen inset;
}

.gos-net-ssid-name {
  color: @gos_ink;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-net-meta {
  color: @gos_ink_muted;
  font-size: 12px;
  font-weight: 600;
}

.gos-net-passfield {
  min-height: 46px;
  padding: 0 14px;
  border: 1px solid @gos_hairline_strong;
  border-radius: 12px;
  background: @gos_surface;
  color: @gos_ink;
  font-size: 14px;
}

.gos-net-passfield:focus {
  box-shadow: 0 0 0 3px @gos_focus;
}

.gos-net-join {
  min-height: 46px;
  padding: 0 20px;
  border-radius: 12px;
  font-size: 14px;
  font-weight: 700;
  color: @gos_ink;
  border: 1px solid @gos_hairline_strong;
  background: @gos_surface;
}

.gos-net-join:hover {
  box-shadow: 0 8px 20px @gos_shadow_raise;
}

.gos-net-helper {
  color: @gos_ink_faint;
  font-size: 12px;
}

/* ── Guided setup depth: appearance, accessibility, first app ─────────────
   These pages are native first-boot surfaces, not documentation. They write the
   standard GNOME appearance/accessibility keys and can hand a first app intent to
   the same loopback build daemon the desktop launcher uses. */
.gos-setup-choice-grid {
  margin-top: 24px;
}

.gos-setup-choice {
  min-height: 104px;
  padding: 16px;
  border: 1px solid @gos_hairline;
  border-radius: 14px;
  background: alpha(@gos_surface, 0.76);
  transition: box-shadow 160ms cubic-bezier(0.32, 0.72, 0, 1),
              background 160ms cubic-bezier(0.32, 0.72, 0, 1),
              border 160ms cubic-bezier(0.32, 0.72, 0, 1);
}

.gos-setup-choice:hover {
  background: @gos_surface;
  border: 1px solid @gos_hairline_strong;
  box-shadow: 0 8px 20px @gos_shadow_raise;
}

.gos-setup-first-app-entry {
  min-height: 52px;
  padding: 0 16px;
  border: 1px solid @gos_hairline_strong;
  border-radius: 14px;
  background: @gos_surface;
  color: @gos_ink;
  font-size: 15px;
}

.gos-setup-first-app-entry:focus {
  box-shadow: 0 0 0 3px @gos_focus;
}

/* ── Install to this computer (the installer's destructive disk flow) ─────
   A calm, Apple-minimal sequence built on the onboarding card (gos-net-panel):
   choose a disk, review, type the exact device-anchored phrase to consent, then
   an honestly-indeterminate install. Safety reads through clarity and restraint,
   never alarm chrome. Every color is token-driven, so the whole flow themes
   Light/Dark/Auto with the rest of the OS. */

/* Disk rows — a selectable list inside the network-style card. */
.gos-install-disk {
  padding: 14px 16px;
  border: 1px solid @gos_hairline;
  border-radius: 12px;
  background: alpha(@gos_surface, 0.7);
  transition: box-shadow 160ms ease-out, background 160ms ease-out, border 160ms ease-out;
}

.gos-install-disk.is-eligible:hover {
  background: @gos_surface;
  border: 1px solid @gos_hairline_strong;
  box-shadow: 0 6px 16px @gos_shadow_raise;
}

.gos-install-disk.is-selected {
  border: 1px solid @gos_ink;
  background: @gos_surface;
  box-shadow: 0 1px 0 @gos_panel_sheen inset,
              0 8px 20px @gos_shadow_raise;
}

/* A selectable disk row is a real control — it gets the same focus ring as every
   other control so keyboard users can see the target before they commit a wipe. */
.gos-install-disk:focus {
  box-shadow: 0 0 0 3px @gos_focus;
}

/* A blocked disk recedes through a calm sunken surface and faint body text rather
   than a flat opacity — opacity dimmed the disqualifying reason below legibility.
   The reason itself (.gos-install-disk-state.is-blocked) stays full-strength red. */
.gos-install-disk.is-blocked {
  border: 1px solid rgba(140, 58, 46, 0.2);
  background: @gos_surface_muted;
}

.gos-install-disk.is-blocked .gos-install-disk-model,
.gos-install-disk.is-blocked .gos-install-disk-path,
.gos-install-disk.is-blocked .gos-install-disk-kind {
  color: @gos_ink_faint;
}

.gos-install-disk-model {
  color: @gos_ink;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-install-disk-kind {
  color: @gos_ink_muted;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 1.4px;
}

.gos-install-disk-path {
  color: @gos_ink_secondary;
  font-family: monospace;
  font-size: 12px;
}

.gos-install-disk-state {
  color: @gos_ready;
  font-size: 12px;
  font-weight: 700;
}

.gos-install-disk-state.is-blocked {
  color: @gos_blocked;
  font-weight: 600;
}

.gos-dual-boot-choice {
  padding: 12px 14px;
  border: 1px solid @gos_hairline;
  border-radius: 12px;
  background: alpha(@gos_surface, 0.78);
  transition: box-shadow 160ms ease-out, background 160ms ease-out, border 160ms ease-out;
}

.gos-dual-boot-choice:hover {
  background: @gos_surface;
  border: 1px solid @gos_hairline_strong;
  box-shadow: 0 6px 16px @gos_shadow_raise;
}

.gos-dual-boot-choice:focus {
  box-shadow: 0 0 0 3px @gos_focus;
}

.gos-dual-boot-choice-title {
  color: @gos_ink;
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-dual-boot-choice-detail {
  color: @gos_ink_secondary;
  font-size: 12px;
  font-weight: 600;
}

/* Review + done rows (review_row / step_row). */
.gos-install-row-title {
  color: @gos_ink_muted;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 1.4px;
  text-transform: uppercase;
}

.gos-install-row-value {
  margin-top: 2px;
  color: @gos_ink;
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0;
}

.gos-install-row-long {
  padding-top: 13px;
  padding-bottom: 13px;
}

.gos-install-row-lines {
  margin-top: 7px;
}

.gos-install-row-line {
  color: @gos_ink_secondary;
  font-size: 14px;
  font-weight: 600;
}

/* Optional dry-run command disclosure (Review). */
.gos-install-command {
  padding: 12px 16px;
  border: 1px solid @gos_hairline_strong;
  border-radius: 12px;
  background: @gos_surface_sunken;
  color: @gos_ink;
  font-family: monospace;
  font-size: 12px;
}

/* Destructive confirmation — the hero phrase the user must type, and the entry.
   The phrase is selectable (so the device path is verifiable) and never masked —
   masking a phrase the user must read would defeat the safety it provides. */
.gos-install-ack-phrase {
  padding: 12px 16px;
  border: 1px solid @gos_hairline_strong;
  border-radius: 12px;
  background: @gos_surface_sunken;
  color: @gos_ink;
  font-family: monospace;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0.4px;
}

.gos-install-ack-entry {
  min-height: 46px;
  padding: 0 14px;
  border: 1px solid @gos_hairline_strong;
  border-radius: 12px;
  background: @gos_surface;
  color: @gos_ink;
  font-family: monospace;
  font-size: 14px;
}

.gos-install-ack-entry:focus {
  box-shadow: 0 0 0 3px @gos_focus;
}

.gos-install-ack-entry > text {
  color: @gos_ink;
  caret-color: @gos_ink;
  font-family: monospace;
  font-size: 14px;
}

/* Exact match: a calm ready border — the moment consent is complete. */
.gos-install-ack-entry.is-matching {
  border: 1px solid @gos_ready;
  background: @gos_ready_soft;
}

/* Diverged from the phrase: a gentle blocked tint shown ONLY when the prefix is
   wrong, so correcting a typo clears it — never an alarm on every keystroke. */
.gos-install-ack-entry.is-diverged {
  border: 1px solid @gos_blocked;
}

/* Progress phase line — the latest real bootc stdout line, or the honest static
   line. Never a percentage or an invented timer. */
.gos-install-phase {
  margin-top: 4px;
  color: @gos_ink_secondary;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0;
}

/* Done. */
.gos-install-done-mark {
  color: @gos_ready;
  font-size: 56px;
  font-weight: 700;
}

.gos-install-error-detail {
  color: @gos_ink;
  font-family: monospace;
  font-size: 13px;
}

/* ── macOS-blend chrome material + motion ────────────────────────────────────
   The shared window/overlay glass and the canonical motion curve (mirrored from
   the MOTION_* tokens above — the standard ease cubic-bezier(0.32, 0.72, 0, 1)).
   Crafted chrome (the launcher, the control center) is a transparent top-level
   whose inner card paints this material, so the wallpaper shows through — real
   vibrancy at the window level (GPU backdrop-blur is the compositor/hardware gate). */
.gos-window {
  background: transparent;
}

.gos-material {
  border: 1px solid @gos_material_border;
  border-radius: 16px;
  background: @gos_material_regular;
  box-shadow: 0 1px 0 @gos_material_sheen inset,
              0 30px 80px @gos_material_shadow;
}

/* ── Command-Space launcher ─────────────────────────────────────────────────
   A bespoke all-Rust overlay: one calm search field over the thick glass, fuzzy
   results, and a standing "Build a new app" action at the foot — the OS's thesis
   one keystroke away. The whole card fades+scales in on the MOTION_OVERLAY curve. */
.gos-launcher-root {
  background: transparent;
  padding: 0;
}

.gos-launcher {
  padding: 10px;
  border: 1px solid @gos_material_border;
  border-radius: 16px;
  background: @gos_material_ultra_thick;
  box-shadow: 0 1px 0 @gos_material_sheen inset,
              0 36px 96px @gos_material_shadow;
}

.gos-launcher-field {
  padding: 8px 14px 8px 16px;
}

.gos-launcher-glyph {
  color: @gos_ink_muted;
  font-size: 19px;
}

.gos-launcher-entry {
  min-height: 40px;
  border: none;
  background: transparent;
  box-shadow: none;
  color: @gos_ink;
  font-size: 20px;
  font-weight: 400;
  letter-spacing: 0;
}

.gos-launcher-entry:focus { box-shadow: none; outline: none; }
.gos-launcher-entry > text {
  background: transparent;
  color: @gos_ink;
  caret-color: @gos_ink;
}
.gos-launcher-entry > text selection,
.gos-launcher-entry text selection {
  color: @gos_on_primary;
  background-color: alpha(@gos_accent, 0.30);
}

.gos-launcher-sep {
  margin: 4px 8px;
  min-height: 1px;
  background: @gos_material_border;
}

.gos-launcher-scroll { background: transparent; }

.gos-launcher-section {
  margin: 10px 12px 2px 12px;
  color: @gos_ink_faint;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 1.6px;
  text-transform: uppercase;
}

.gos-launcher-row {
  padding: 9px 12px;
  border-radius: 12px;
  background: transparent;
  border: 1px solid transparent;
  box-shadow: none;
  transition: background 140ms cubic-bezier(0.32, 0.72, 0, 1),
              border 140ms cubic-bezier(0.32, 0.72, 0, 1);
}

.gos-launcher-row:hover { background: @gos_material_hover; }
.gos-launcher-row:focus:focus-visible {
  box-shadow: 0 0 0 3px @gos_focus;
}

/* The keyboard selection uses a neutral material with an accent hairline. It
   reads as the Enter target without flooding the launcher in system blue. */
.gos-launcher-row.is-selected {
  background: @gos_material_active;
  border-color: @gos_primary_border;
  box-shadow: 0 1px 0 alpha(@gos_material_sheen, 0.36) inset,
              inset 0 0 0 1px alpha(@gos_primary_border, 0.74);
}
.gos-launcher-row.is-selected:focus:focus-visible {
  box-shadow: 0 0 0 3px @gos_focus,
              0 1px 0 alpha(@gos_material_sheen, 0.36) inset,
              inset 0 0 0 1px @gos_primary_border;
}

.gos-launcher-row-icon {
  color: @gos_ink_secondary;
  font-size: 16px;
  min-width: 22px;
}
.gos-launcher-row.is-selected .gos-launcher-row-icon { color: @gos_accent; }

.gos-launcher-row-title {
  color: @gos_ink;
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0;
}
.gos-launcher-row.is-selected .gos-launcher-row-title { color: @gos_ink; }

.gos-launcher-row-sub { color: @gos_ink_muted; font-size: 12px; }
.gos-launcher-row.is-selected .gos-launcher-row-sub { color: @gos_ink_secondary; }

.gos-launcher-kind {
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid @gos_material_border;
  color: @gos_ink_muted;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.6px;
  text-transform: uppercase;
}
.gos-launcher-row.is-selected .gos-launcher-kind {
  color: @gos_ink_secondary;
  border-color: alpha(@gos_primary_border, 0.78);
}

/* A computed quick-answer (math / unit conversion) — the result reads large,
   like a system launcher calculation headline. */
.gos-launcher-answer {
  color: @gos_ink;
  font-size: 20px;
  font-weight: 700;
  letter-spacing: 0;
}
.gos-launcher-row.is-selected .gos-launcher-answer { color: @gos_ink; }

.gos-launcher-empty {
  margin: 16px 12px;
  color: @gos_ink_faint;
  font-size: 13px;
}

/* ── Control center (menu-bar quick settings) ────────────────────────────────
   A bespoke all-Rust glass panel the menu bar opens: the scheme toggle, the
   engine switch (GPT-OSS ⇄ Codex), Wi-Fi, and sound/brightness — the controls
   reached for without opening Settings. Same vibrancy material as the launcher. */
.gos-cc-root { background: transparent; padding: 0; }

.gos-cc {
  padding: 14px;
  border: 1px solid @gos_material_border;
  border-radius: 16px;
  background: @gos_material_ultra_thick;
  box-shadow: 0 1px 0 @gos_material_sheen inset,
              0 28px 72px @gos_material_shadow;
}

.gos-cc-title {
  color: @gos_ink;
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0;
}

.gos-cc-section {
  margin: 12px 2px 6px 2px;
  color: @gos_ink_faint;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0;
  text-transform: none;
}

/* Toggle tiles — Wi-Fi, Dark Mode: a rounded square with neutral active material.
   The icon/status carries the accent so the panel does not become one-note blue. */
.gos-cc-tile {
  min-width: 150px;
  min-height: 64px;
  padding: 12px 14px;
  border-radius: 16px;
  border: 1px solid @gos_material_border;
  background: @gos_material_ultra_thin;
  transition: background 140ms cubic-bezier(0.32, 0.72, 0, 1),
              border 140ms cubic-bezier(0.32, 0.72, 0, 1);
}
.gos-cc-tile:hover { background: @gos_material_hover; }
.gos-cc-tile:focus:focus-visible {
  box-shadow: 0 0 0 3px @gos_focus;
}
.gos-cc-tile.is-on {
  background: @gos_material_regular;
  border-color: @gos_primary_border;
  box-shadow: 0 1px 0 alpha(@gos_material_sheen, 0.46) inset,
              inset 0 0 0 1px alpha(@gos_primary_border, 0.58);
}
.gos-cc-tile-glyph { font-size: 18px; color: @gos_ink_secondary; }
.gos-cc-tile.is-on .gos-cc-tile-glyph { color: @gos_accent; }
.gos-cc-tile-label { color: @gos_ink; font-size: 13px; font-weight: 600; }
.gos-cc-tile.is-on .gos-cc-tile-label { color: @gos_ink; }
.gos-cc-tile-state { color: @gos_ink_muted; font-size: 11px; }
.gos-cc-tile.is-on .gos-cc-tile-state { color: @gos_ink_secondary; }

/* The engine switch — a segmented control; the active segment reads selected
   without stealing the primary-action color from real commands. */
.gos-cc-engine {
  padding: 4px;
  border-radius: 14px;
  border: 1px solid @gos_material_border;
  background: @gos_material_ultra_thin;
}
.gos-cc-seg {
  min-height: 34px;
  padding: 0 16px;
  border-radius: 10px;
  border: none;
  background: transparent;
  color: @gos_ink_secondary;
  font-size: 13px;
  font-weight: 600;
  transition: background 140ms cubic-bezier(0.32, 0.72, 0, 1);
}
.gos-cc-seg:hover { background: @gos_material_hover; }
.gos-cc-seg:focus:focus-visible {
  box-shadow: 0 0 0 3px @gos_focus;
}
.gos-cc-seg.is-active {
  color: @gos_ink;
  background: @gos_material_active;
  box-shadow: inset 0 0 0 1px alpha(@gos_primary_border, 0.60);
}

/* Slider rows — sound + brightness; the GtkScale themes to the system-blue track. */
.gos-cc-slider-row { padding: 8px 4px; }
.gos-cc-slider-glyph { font-size: 16px; color: @gos_ink_secondary; min-width: 22px; }
.gos-cc-slider-row scale trough {
  min-height: 6px;
  border-radius: 999px;
  background: @gos_material_active;
}
.gos-cc-slider-row scale highlight {
  border-radius: 999px;
  background: alpha(@gos_accent, 0.72);
}
.gos-cc-slider-row scale slider {
  min-width: 18px;
  min-height: 18px;
  border-radius: 999px;
  background: @gos_surface;
  border: 1px solid @gos_material_border;
  box-shadow: 0 1px 3px @gos_material_shadow;
}
.gos-cc-slider-row.is-disabled { opacity: 0.55; }

.gos-cc-note { margin-top: 6px; color: @gos_ink_faint; font-size: 11px; }

.gos-cc-link {
  min-height: 32px;
  color: @gos_ink_muted;
  background: transparent;
  border: none;
  box-shadow: none;
  font-size: 12px;
  font-weight: 600;
}
.gos-cc-link:hover { color: @gos_ink; }
.gos-cc-link:focus:focus-visible {
  color: @gos_ink;
  box-shadow: 0 0 0 3px @gos_focus;
}
/* Quick-action chip (the Goblins AI actions) — a real button surface, not a
   bare text link, like macOS Control Center's chipped actions. */
.gos-cc-action {
  min-height: 38px;
  padding: 7px 12px;
  color: @gos_ink;
  background-color: @gos_surface_muted;
  border: 1px solid @gos_material_border;
  border-radius: 11px;
  box-shadow: none;
  font-size: 12px;
  font-weight: 600;
}
.gos-cc-action:hover { background-color: @gos_surface_sunken; }
.gos-cc-action:active { background-color: @gos_material_active; }
.gos-cc-action:focus:focus-visible { box-shadow: 0 0 0 3px @gos_focus; }
.gos-cc-action:disabled {
  color: @gos_ink_muted;
  opacity: 0.6;
}
"#;

#[cfg(test)]
mod tests {
    use super::{
        native_css, GOBLINS_NATIVE_CSS, MOTION_EASE_STANDARD, MOTION_OVERLAY_MS,
        MOTION_OVERLAY_SCALE_FROM,
    };

    #[test]
    fn motion_curve_and_materials_are_wired_into_the_css() {
        // The canonical easing token is the one the chrome transitions actually use —
        // a drift between the const and the CSS would split the OS's motion vocabulary.
        assert!(GOBLINS_NATIVE_CSS.contains(MOTION_EASE_STANDARD));
        // The five material tiers are defined in both schemes, and the crafted
        // chrome consumes the tiers it needs for launcher/control-center glass.
        for token_name in [
            "gos_material_ultra_thick",
            "gos_material_thick",
            "gos_material_regular",
            "gos_material_thin",
            "gos_material_ultra_thin",
            "gos_material_border",
            "gos_material_sheen",
        ] {
            assert!(native_css("", false).contains(token_name));
            assert!(native_css("", true).contains(token_name));
        }
        assert!(GOBLINS_NATIVE_CSS.contains("@gos_material_ultra_thick"));
        assert!(GOBLINS_NATIVE_CSS.contains("@gos_material_ultra_thin"));
        assert!(GOBLINS_NATIVE_CSS.contains("@gos_material_regular"));
        // The overlay animator's numbers are sane (a brief fade, a subtle pop).
        const {
            assert!(MOTION_OVERLAY_MS >= 100 && MOTION_OVERLAY_MS <= 400);
            assert!(MOTION_OVERLAY_SCALE_FROM > 0.85 && MOTION_OVERLAY_SCALE_FROM < 1.0);
        }
    }

    #[test]
    fn launcher_and_control_center_surfaces_are_styled() {
        for class in [
            ".gos-launcher",
            ".gos-launcher-entry",
            ".gos-launcher-row",
            ".gos-cc",
            ".gos-cc-tile",
            ".gos-cc-engine",
        ] {
            assert!(
                GOBLINS_NATIVE_CSS.contains(class),
                "shared native CSS must style {class}"
            );
        }
    }

    #[test]
    fn launcher_and_control_center_have_visible_focus_states() {
        for selector in [
            ".gos-launcher-row:focus:focus-visible",
            ".gos-launcher-row.is-selected:focus:focus-visible",
            ".gos-cc-tile:focus:focus-visible",
            ".gos-cc-seg:focus:focus-visible",
            ".gos-cc-link:focus:focus-visible",
        ] {
            assert!(
                GOBLINS_NATIVE_CSS.contains(selector),
                "shared native CSS must expose visible focus for {selector}"
            );
        }
    }

    #[test]
    fn launcher_and_control_center_selection_stays_material_not_primary_flood() {
        let css = GOBLINS_NATIVE_CSS;
        assert!(css.contains(".gos-launcher-entry > text selection"));
        assert!(css.contains("background-color: alpha(@gos_accent, 0.30);"));
        let launcher_selected = css
            .split(".gos-launcher-row.is-selected {")
            .nth(1)
            .and_then(|block| block.split(".gos-launcher-row.is-selected:focus").next())
            .expect("launcher selected block is present");
        assert!(launcher_selected.contains("background: @gos_material_active;"));
        assert!(!launcher_selected.contains("linear-gradient(180deg, @gos_primary"));

        let control_tile_on = css
            .split(".gos-cc-tile.is-on {")
            .nth(1)
            .and_then(|block| block.split(".gos-cc-tile-glyph").next())
            .expect("control center active tile block is present");
        assert!(control_tile_on.contains("background: @gos_material_regular;"));
        assert!(!control_tile_on.contains("linear-gradient(180deg, @gos_primary"));

        let control_segment_active = css
            .split(".gos-cc-seg.is-active {")
            .nth(1)
            .and_then(|block| block.split("/* Slider rows").next())
            .expect("control center active segment block is present");
        assert!(control_segment_active.contains("background: @gos_material_active;"));
        assert!(!control_segment_active.contains("linear-gradient(180deg, @gos_primary"));
    }

    #[test]
    fn macos27_reference_axes_are_translated_without_apple_assets() {
        let light = native_css("", false);
        let dark = native_css("", true);

        for token in [
            "@define-color gos_label_primary",
            "@define-color gos_fill_primary",
            "@define-color gos_separator",
            "@define-color gos_system_blue",
            "@define-color gos_material_ultra_thick",
            "@define-color gos_material_ultra_thin",
            "@define-color gos_focus",
        ] {
            assert!(light.contains(token));
            assert!(dark.contains(token));
        }
        for exact in [
            "@define-color gos_canvas              #ffffff",
            "@define-color gos_system_blue         rgba(0, 136, 255, 1)",
            "@define-color gos_system_green        rgba(52, 199, 89, 1)",
            "@define-color gos_system_red          rgba(255, 56, 60, 1)",
        ] {
            assert!(light.contains(exact));
        }
        for exact in [
            "@define-color gos_canvas              #1e1e1e",
            "@define-color gos_system_blue         rgba(0, 145, 255, 1)",
            "@define-color gos_system_green        rgba(48, 209, 88, 1)",
            "@define-color gos_system_red          rgba(255, 66, 69, 1)",
        ] {
            assert!(dark.contains(exact));
        }

        assert!(GOBLINS_NATIVE_CSS.contains("@gos_label_secondary"));
        assert!(GOBLINS_NATIVE_CSS.contains("@gos_fill_tertiary"));
        assert!(GOBLINS_NATIVE_CSS.contains("border-radius: 16px"));
        assert!(GOBLINS_NATIVE_CSS.contains(".gos-window-zoom"));
        assert!(GOBLINS_NATIVE_CSS.contains("Inter"));
        for forbidden in ["SFPro", "SF Pro", "San Francisco"] {
            assert!(
                !GOBLINS_NATIVE_CSS.contains(forbidden),
                "shared CSS must not bundle or request Apple font naming: {forbidden}"
            );
        }
    }

    #[test]
    fn native_css_emits_tokens_then_app_then_rules() {
        let css = native_css(".app { color: #101211; }", false);

        // App CSS sits between the token block and the shared structural rules so
        // every rule (and the app's own) resolves colors from the active scheme.
        let tokens_at = css
            .find("@define-color gos_canvas")
            .expect("tokens present");
        let app_at = css
            .find(".app { color: #101211; }")
            .expect("app css present");
        let rules_at = css.find(".gos-root").expect("shared rules present");
        assert!(tokens_at < app_at && app_at < rules_at);
    }

    #[test]
    fn light_and_dark_share_token_names_but_invert_values() {
        let light = native_css("", false);
        let dark = native_css("", true);

        // The two schemes share token names (so every rule themes automatically)...
        for token in [
            "@define-color gos_canvas",
            "@define-color gos_ink",
            "@define-color gos_studio_bg",
        ] {
            assert!(light.contains(token) && dark.contains(token));
        }
        // ...but invert the values: light canvas is white, dark canvas is the kit's window black.
        assert!(light.contains("#ffffff") && light.contains("#f7f7f8"));
        assert!(dark.contains("#1e1e1e"));
        assert_ne!(light, dark);
        // The structural rules are token-driven, never raw literals for ink.
        assert!(GOBLINS_NATIVE_CSS.contains("@gos_ink"));
    }

    #[test]
    fn every_app_surface_class_is_styled() {
        // Guards against a refactor silently dropping a surface the apps depend on.
        for class in [
            ".gos-login-root",
            ".gos-installer-root",
            ".gos-settings-root",
            ".gos-identity-panel",
            ".gos-app-tile",
            ".gos-primary-action",
            ".gos-status-pill",
            ".gos-section-title",
            ".gos-resident",
        ] {
            assert!(
                GOBLINS_NATIVE_CSS.contains(class),
                "shared native CSS must style {class}"
            );
        }
    }

    #[test]
    fn status_pills_are_calm_state_labels_not_debug_badges() {
        assert!(GOBLINS_NATIVE_CSS.contains(".gos-status-pill {\n  padding: 5px 10px;"));
        assert!(GOBLINS_NATIVE_CSS.contains("font-size: 11px;"));
        assert!(GOBLINS_NATIVE_CSS.contains("letter-spacing: 0;"));
        assert!(GOBLINS_NATIVE_CSS.contains("border-color: alpha(@gos_ready, 0.34);"));
        assert!(GOBLINS_NATIVE_CSS.contains("background: alpha(@gos_waiting, 0.12);"));
        assert!(!GOBLINS_NATIVE_CSS.contains(".gos-status-pill {\n  padding: 6px 11px;"));
        let status_pill_block = GOBLINS_NATIVE_CSS
            .split(".gos-status-pill {")
            .nth(1)
            .and_then(|block| block.split("/* Inline").next())
            .expect("shared status pill CSS block is present");
        assert!(!status_pill_block.contains("text-transform"));
    }

    #[test]
    fn onboarding_kickers_are_title_case_not_shouted_labels() {
        let block = GOBLINS_NATIVE_CSS
            .split(".gos-onboarding-kicker {")
            .nth(1)
            .and_then(|block| block.split(".gos-onboarding-title").next())
            .expect("onboarding kicker CSS block is present");

        assert!(block.contains("font-weight: 600;"));
        assert!(block.contains("letter-spacing: 0;"));
        assert!(block.contains("text-transform: none;"));
        assert!(!block.contains("text-transform: uppercase"));
        let old_wide_tracking = ["letter-spacing: ", "2.2px"].concat();
        assert!(!block.contains(&old_wide_tracking));
    }

    #[test]
    fn shared_css_stays_native_os_focused() {
        let forbidden = [
            concat!("Web", "View"),
            concat!("web", "view"),
            concat!("ki", "osk"),
            concat!("vi", "te"),
            concat!("next", ".config"),
            concat!(".", "tsx"),
        ];

        for forbidden in forbidden {
            assert!(
                !GOBLINS_NATIVE_CSS.contains(forbidden),
                "shared native CSS must not reference {forbidden}"
            );
        }
    }
}
