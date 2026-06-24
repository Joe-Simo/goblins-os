//! The Goblins OS ⌘-Space launcher — a bespoke, all-Rust GTK4 overlay in the
//! mold of macOS Spotlight, themed in the Goblins-native language.
//!
//! One field over translucent glass. Type and it fuzzy-searches the apps you've
//! built (the OS ships none — `GET /v1/apps`), jumps to Settings sections, does
//! quick math and unit conversions, and offers a standing "Build a new app:
//! <query>" action that posts to the on-device builder (`POST /v1/apps/builds`).
//! Wired to Super+Space in the goblins-os session, with assistant and context
//! modes on the adjacent Goblins AI shortcuts. Outside a Linux native-desktop
//! build the crate degrades to a one-line status print.

use std::{env, error::Error};

const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
const VISUAL_CONTEXT_SUBTITLE: &str = "Capture the screen, then ask with local-only visual context";

type LauncherResult<T> = Result<T, Box<dyn Error>>;

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn bounded_context_value(value: &str, max_chars: usize) -> Option<String> {
    let mut output = String::new();
    let mut pending_space = false;
    let mut count = 0usize;

    for ch in value.chars() {
        if ch.is_control() || ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !output.is_empty() {
            output.push(' ');
            count += 1;
            if count >= max_chars {
                break;
            }
        }
        output.push(ch);
        count += 1;
        pending_space = false;
        if count >= max_chars {
            break;
        }
    }

    let trimmed = output.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Clone)]
struct LauncherConfig {
    core_url: String,
    mode: LauncherMode,
}

impl LauncherConfig {
    fn from_env() -> Self {
        Self {
            core_url: env::var("GOBLINS_OS_CORE_URL")
                .or_else(|_| env::var("OPENAI_OS_CORE_URL"))
                .unwrap_or_else(|_| DEFAULT_CORE_URL.into()),
            mode: LauncherMode::from_args_and_env(),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LauncherMode {
    Normal,
    Assistant,
    SelectedText,
    WritingTools,
    ScreenContext,
    VisualContext,
}

impl LauncherMode {
    fn from_args_and_env() -> Self {
        Self::from_values(
            env::args().skip(1),
            env::var("GOBLINS_OS_LAUNCHER_MODE").ok(),
        )
    }

    fn from_values<I>(args: I, mode_env: Option<String>) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        for arg in args {
            match arg.as_ref() {
                "--assistant" | "--ai" => return Self::Assistant,
                "--selected-text" => return Self::SelectedText,
                "--writing-tools" | "--write-with-goblins" => return Self::WritingTools,
                "--screen-context" => return Self::ScreenContext,
                "--visual-context" | "--screenshot-context" => return Self::VisualContext,
                _ => {}
            }
        }
        match mode_env
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("assistant" | "ai") => Self::Assistant,
            Some("selected-text") => Self::SelectedText,
            Some("writing-tools" | "write-with-goblins" | "writing") => Self::WritingTools,
            Some("screen-context" | "screen") => Self::ScreenContext,
            Some("visual-context" | "screenshot-context" | "screenshot" | "visual") => {
                Self::VisualContext
            }
            _ => Self::Normal,
        }
    }
}

fn main() -> LauncherResult<()> {
    let config = LauncherConfig::from_env();
    run_launcher(config)
}

// ── Quick compute: math + unit conversion (dependency-free, fully tested) ─────
// These are pure functions so the launcher's "instant answer" behavior is unit-
// tested by the gate (`cargo test`) without a display server.

/// A fast pre-filter: does the query plausibly contain an arithmetic expression?
/// (At least one digit and one operator — so "todo list" never reads as math.)
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn looks_like_math(query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return false;
    }
    let has_digit = q.chars().any(|c| c.is_ascii_digit());
    let has_op = q
        .chars()
        .any(|c| matches!(c, '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')'));
    has_digit
        && has_op
        && q.chars().all(|c| {
            c.is_ascii_digit()
                || c.is_ascii_whitespace()
                || matches!(c, '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')' | '.' | ',')
        })
}

/// Evaluate a simple arithmetic expression (`+ - * / % ^`, parentheses, unary
/// minus, decimals). Returns a tidily-formatted result, or None if it doesn't
/// parse cleanly. A bare number is rejected — it carries no answer.
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn eval_math(query: &str) -> Option<String> {
    if !looks_like_math(query) {
        return None;
    }
    let cleaned: String = query.chars().filter(|c| *c != ',').collect();
    let tokens = tokenize_math(&cleaned)?;
    let mut parser = MathParser {
        tokens: &tokens,
        pos: 0,
    };
    let value = parser.expr()?;
    if parser.pos != parser.tokens.len() {
        return None;
    }
    if !value.is_finite() {
        return None;
    }
    Some(format_number(value))
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
#[derive(Clone, Copy, PartialEq)]
enum MathToken {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn tokenize_math(input: &str) -> Option<Vec<MathToken>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let slice: String = chars[start..i].iter().collect();
            tokens.push(MathToken::Num(slice.parse().ok()?));
            continue;
        }
        tokens.push(match c {
            '+' => MathToken::Plus,
            '-' => MathToken::Minus,
            '*' => MathToken::Star,
            '/' => MathToken::Slash,
            '%' => MathToken::Percent,
            '^' => MathToken::Caret,
            '(' => MathToken::LParen,
            ')' => MathToken::RParen,
            _ => return None,
        });
        i += 1;
    }
    Some(tokens)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
struct MathParser<'a> {
    tokens: &'a [MathToken],
    pos: usize,
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
impl MathParser<'_> {
    fn peek(&self) -> Option<MathToken> {
        self.tokens.get(self.pos).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut value = self.term()?;
        while let Some(op) = self.peek() {
            match op {
                MathToken::Plus => {
                    self.pos += 1;
                    value += self.term()?;
                }
                MathToken::Minus => {
                    self.pos += 1;
                    value -= self.term()?;
                }
                _ => break,
            }
        }
        Some(value)
    }

    fn term(&mut self) -> Option<f64> {
        let mut value = self.power()?;
        while let Some(op) = self.peek() {
            match op {
                MathToken::Star => {
                    self.pos += 1;
                    value *= self.power()?;
                }
                MathToken::Slash => {
                    self.pos += 1;
                    let divisor = self.power()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    value /= divisor;
                }
                MathToken::Percent => {
                    self.pos += 1;
                    let divisor = self.power()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    value %= divisor;
                }
                _ => break,
            }
        }
        Some(value)
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if let Some(MathToken::Caret) = self.peek() {
            self.pos += 1;
            let exp = self.power()?;
            return Some(base.powf(exp));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            MathToken::Minus => {
                self.pos += 1;
                Some(-self.unary()?)
            }
            MathToken::Plus => {
                self.pos += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            MathToken::Num(value) => {
                self.pos += 1;
                Some(value)
            }
            MathToken::LParen => {
                self.pos += 1;
                let value = self.expr()?;
                if self.peek()? != MathToken::RParen {
                    return None;
                }
                self.pos += 1;
                Some(value)
            }
            _ => None,
        }
    }
}

/// Round to a tidy decimal and drop trailing zeros so 1/3 reads "0.3333333" and
/// 2+2 reads "4", never "4.0000000".
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn format_number(value: f64) -> String {
    let rounded = (value * 1e7).round() / 1e7;
    let mut text = format!("{rounded:.7}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".to_string();
    }
    text
}

/// Convert "<number> <unit> to|in <unit>" across length, mass, temperature,
/// data, and time. Returns a formatted "<value> <unit>" answer, or None.
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn convert_units(query: &str) -> Option<String> {
    let lower = query.trim().to_lowercase();
    let (head, target) = split_once_any(&lower, &[" to ", " in ", " into ", ">"])?;
    let head = head.trim();
    let target = target.trim().trim_end_matches('s');

    // The amount is the leading number; the rest of the head is the source unit.
    let mut split = head.len();
    for (idx, ch) in head.char_indices() {
        if !(ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == '+' || ch.is_ascii_whitespace())
        {
            split = idx;
            break;
        }
    }
    let amount: f64 = head[..split].trim().replace(' ', "").parse().ok()?;
    let source = head[split..].trim().trim_end_matches('s');
    if source.is_empty() {
        return None;
    }

    // Temperature is affine (offset + scale), so it gets its own path.
    if let (Some(from_temp), Some(to_temp)) = (temperature_kind(source), temperature_kind(target)) {
        let celsius = from_temp.normalize(amount);
        let value = to_temp.denormalize(celsius);
        return Some(format!("{} {}", format_number(value), to_temp.label()));
    }

    let (from_dim, from_factor) = unit_factor(source)?;
    let (to_dim, to_factor) = unit_factor(target)?;
    if from_dim != to_dim {
        return None;
    }
    let value = amount * from_factor / to_factor;
    Some(format!(
        "{} {}",
        format_number(value),
        canonical_unit(target)
    ))
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn split_once_any<'a>(text: &'a str, seps: &[&str]) -> Option<(&'a str, &'a str)> {
    let mut best: Option<(usize, usize)> = None;
    for sep in seps {
        if let Some(idx) = text.find(sep) {
            if best.is_none_or(|(b, _)| idx < b) {
                best = Some((idx, sep.len()));
            }
        }
    }
    best.map(|(idx, len)| (&text[..idx], &text[idx + len..]))
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
#[derive(Clone, Copy)]
enum Temp {
    C,
    F,
    K,
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
impl Temp {
    /// A value in THIS unit normalized to the base scale (Celsius).
    fn normalize(self, value: f64) -> f64 {
        match self {
            Temp::C => value,
            Temp::F => (value - 32.0) * 5.0 / 9.0,
            Temp::K => value - 273.15,
        }
    }
    /// A base-scale (Celsius) value expressed back in THIS unit.
    fn denormalize(self, celsius: f64) -> f64 {
        match self {
            Temp::C => celsius,
            Temp::F => celsius * 9.0 / 5.0 + 32.0,
            Temp::K => celsius + 273.15,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Temp::C => "°C",
            Temp::F => "°F",
            Temp::K => "K",
        }
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn temperature_kind(unit: &str) -> Option<Temp> {
    match unit {
        "c" | "celsius" | "°c" | "centigrade" => Some(Temp::C),
        "f" | "fahrenheit" | "°f" => Some(Temp::F),
        "k" | "kelvin" => Some(Temp::K),
        _ => None,
    }
}

/// (dimension tag, factor to the dimension's base unit).
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn unit_factor(unit: &str) -> Option<(&'static str, f64)> {
    let table: &[(&str, &str, f64)] = &[
        // length → base metre
        ("mm", "len", 0.001),
        ("millimeter", "len", 0.001),
        ("cm", "len", 0.01),
        ("centimeter", "len", 0.01),
        ("m", "len", 1.0),
        ("meter", "len", 1.0),
        ("metre", "len", 1.0),
        ("km", "len", 1000.0),
        ("kilometer", "len", 1000.0),
        ("in", "len", 0.0254),
        ("inch", "len", 0.0254),
        ("ft", "len", 0.3048),
        ("foot", "len", 0.3048),
        ("feet", "len", 0.3048),
        ("yd", "len", 0.9144),
        ("yard", "len", 0.9144),
        ("mi", "len", 1609.344),
        ("mile", "len", 1609.344),
        // mass → base gram
        ("mg", "mass", 0.001),
        ("g", "mass", 1.0),
        ("gram", "mass", 1.0),
        ("kg", "mass", 1000.0),
        ("kilogram", "mass", 1000.0),
        ("lb", "mass", 453.59237),
        ("pound", "mass", 453.59237),
        ("oz", "mass", 28.349523),
        ("ounce", "mass", 28.349523),
        // data → base byte
        ("byte", "data", 1.0),
        ("kb", "data", 1000.0),
        ("kib", "data", 1024.0),
        ("mb", "data", 1.0e6),
        ("mib", "data", 1_048_576.0),
        ("gb", "data", 1.0e9),
        ("gib", "data", 1_073_741_824.0),
        ("tb", "data", 1.0e12),
        // time → base second
        ("sec", "time", 1.0),
        ("second", "time", 1.0),
        ("min", "time", 60.0),
        ("minute", "time", 60.0),
        ("hr", "time", 3600.0),
        ("hour", "time", 3600.0),
        ("day", "time", 86400.0),
        ("week", "time", 604800.0),
    ];
    table
        .iter()
        .find(|(name, _, _)| *name == unit)
        .map(|(_, dim, factor)| (*dim, *factor))
}

/// A tidy display label for a target unit (the user's own spelling, capitalized
/// abbreviations left as typed).
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn canonical_unit(unit: &str) -> String {
    unit.to_string()
}

/// A fuzzy subsequence score: every query char must appear in order in the
/// candidate. Consecutive runs and word-boundary hits score higher, and a clean
/// prefix wins — so "set" ranks "Settings" above "Reset". None = no match.
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let cand = candidate.to_lowercase();
    let cand_chars: Vec<char> = cand.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();

    let mut score = 0;
    let mut qi = 0;
    let mut prev_match: Option<usize> = None;
    for (ci, &cc) in cand_chars.iter().enumerate() {
        if qi >= query_chars.len() {
            break;
        }
        if cc == query_chars[qi] {
            score += 1;
            if prev_match == Some(ci.wrapping_sub(1)) {
                score += 5; // consecutive run
            }
            let at_boundary = ci == 0
                || cand_chars
                    .get(ci.wrapping_sub(1))
                    .is_some_and(|c| !c.is_alphanumeric());
            if at_boundary {
                score += 8;
            }
            prev_match = Some(ci);
            qi += 1;
        }
    }
    if qi != query_chars.len() {
        return None;
    }
    if cand.starts_with(&query) {
        score += 20;
    }
    // Slight preference for shorter candidates (a tighter match).
    score -= (cand_chars.len() as i32) / 12;
    Some(score)
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_launcher(config: LauncherConfig) -> LauncherResult<()> {
    let _ = config.core_url.as_str();
    let _ = config.mode;
    println!("goblins_os_launcher=unavailable");
    println!("launcher_reason=build_requires_linux_native_desktop_feature");
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use native::run_launcher;

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
mod native {
    use std::{
        cell::RefCell,
        io::{Read, Write},
        net::{TcpStream, ToSocketAddrs},
        process::Command,
        rc::Rc,
        sync::mpsc,
        thread,
        time::Duration,
    };

    use gtk::gdk;
    use gtk::glib;
    use gtk::prelude::*;
    use gtk4 as gtk;
    use serde::Deserialize;

    use super::{
        bounded_context_value, convert_units, eval_math, fuzzy_score, LauncherConfig, LauncherMode,
        LauncherResult, VISUAL_CONTEXT_SUBTITLE,
    };

    const MAX_BODY_BYTES: u64 = 1024 * 1024;
    const APP_ID: &str = "org.goblins.OS.Launcher";
    const SELECTED_TEXT_CONTEXT_ENV: &str = "GOBLINS_OS_SELECTED_TEXT_CONTEXT";
    const SCREEN_CONTEXT_TEXT_ENV: &str = "GOBLINS_OS_SCREEN_CONTEXT_TEXT";
    const VISUAL_CONTEXT_SUMMARY_ENV: &str = "GOBLINS_OS_VISUAL_CONTEXT_SUMMARY";
    const SCREEN_CONTEXT_SOURCE_ENV: &str = "GOBLINS_OS_SCREEN_CONTEXT_SOURCE";
    const CONTEXT_APP_ENV: &str = "GOBLINS_OS_CONTEXT_APP";
    const CONTEXT_WINDOW_TITLE_ENV: &str = "GOBLINS_OS_CONTEXT_WINDOW_TITLE";
    const CONTEXT_TEXT_MAX_CHARS: usize = 4_000;
    const CONTEXT_METADATA_MAX_CHARS: usize = 180;

    fn placeholder_for_mode(mode: LauncherMode) -> &'static str {
        match mode {
            LauncherMode::Normal => "Search apps, settings, math — or describe an app to build",
            LauncherMode::Assistant => "Ask Goblin or describe what you need",
            LauncherMode::SelectedText => "Paste selected text, then press Return",
            LauncherMode::WritingTools => {
                "Paste text to rewrite, proofread, summarize, or change tone"
            }
            LauncherMode::ScreenContext => "Describe visible content, then press Return",
            LauncherMode::VisualContext => {
                "Describe a screenshot or visual details, then press Return"
            }
        }
    }

    #[derive(Clone, Deserialize)]
    struct BuiltApp {
        name: String,
        #[serde(default)]
        intent: String,
        #[serde(default)]
        source: String,
    }

    #[derive(Deserialize)]
    struct AppList {
        apps: Vec<BuiltApp>,
    }

    #[derive(Clone, Deserialize)]
    struct AiActionCatalog {
        actions: Vec<AiActionStatus>,
    }

    #[derive(Clone, Deserialize)]
    struct AiActionStatus {
        id: String,
        enabled: bool,
        reason: String,
    }

    #[derive(Clone)]
    struct AiActions {
        ask: AiActionAvailability,
        selected_text: AiActionAvailability,
        writing_tools: AiActionAvailability,
        screen_context: AiActionAvailability,
        build: AiActionAvailability,
    }

    #[derive(Clone)]
    struct AiActionAvailability {
        enabled: bool,
        reason: String,
    }

    /// What activating a result does.
    #[derive(Clone)]
    enum Action {
        /// Non-activating guidance row.
        Noop,
        /// Open a built app in a standalone Build Studio window.
        OpenApp(String),
        /// Open the launcher in another first-class mode.
        OpenLauncherMode(LauncherMode),
        /// Capture a screenshot through the OS helper, then open visual context.
        OpenScreenshotContext,
        /// Launch Settings, optionally on a specific panel.
        OpenSettings(Option<&'static str>),
        /// Open the Build Studio.
        OpenStudio,
        /// Toggle the desktop Light/Dark scheme.
        ToggleScheme,
        /// Copy a computed answer to the clipboard.
        Copy(String),
        /// Ask the system assistant through the Goblins AI runtime.
        AskGoblins(String),
        /// Ask the selected-text context endpoint with user-invoked context.
        AskSelectedText(String),
        /// Rewrite, proofread, summarize, or change selected text after invocation.
        WriteWithGoblins(String),
        /// Ask the screen-context endpoint with user-invoked visible context.
        AskScreenContext(String),
        /// Ask the screen-context endpoint with a user-invoked visual summary.
        AskVisualContext(String),
        /// Build a new app from the typed query via the on-device builder.
        Build(String),
    }

    #[derive(Clone)]
    struct LauncherItem {
        icon: &'static str,
        title: String,
        subtitle: String,
        kind: &'static str,
        /// A computed answer reads large (the title slot holds the expression).
        answer: Option<String>,
        action: Action,
    }

    pub fn run_launcher(config: LauncherConfig) -> LauncherResult<()> {
        let apps = Rc::new(fetch_apps(&config.core_url));
        let ai_actions = Rc::new(fetch_ai_actions(&config.core_url));
        let app = gtk::Application::builder().application_id(APP_ID).build();
        app.connect_activate(move |app| {
            goblins_os_ui::init_theming("");
            build_window(app, &config, apps.clone(), ai_actions.clone());
        });
        // The launcher parses its own environment, not GTK CLI args.
        app.run_with_args(&["goblins-os-launcher"]);
        Ok(())
    }

    fn build_window(
        app: &gtk::Application,
        config: &LauncherConfig,
        apps: Rc<Vec<BuiltApp>>,
        ai_actions: Rc<AiActions>,
    ) {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Goblins OS Launcher")
            .decorated(false)
            .resizable(false)
            .default_width(720)
            .build();
        window.add_css_class("gos-launcher-root");
        window.add_css_class("gos-window");

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("gos-launcher");

        // Query field: native themed search icon + the command entry.
        let field = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        field.add_css_class("gos-launcher-field");
        let glyph = gtk::Image::from_icon_name("system-search-symbolic");
        glyph.set_pixel_size(18);
        glyph.add_css_class("gos-launcher-glyph");
        glyph.update_property(&[
            gtk::accessible::Property::Label("Search"),
            gtk::accessible::Property::Description("Search field icon"),
        ]);
        let entry = gtk::Entry::new();
        entry.add_css_class("gos-launcher-entry");
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some(placeholder_for_mode(config.mode)));
        entry.update_property(&[
            gtk::accessible::Property::Label("Search Goblins OS"),
            gtk::accessible::Property::Description(placeholder_for_mode(config.mode)),
        ]);
        field.append(&glyph);
        field.append(&entry);
        card.append(&field);

        let sep = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sep.add_css_class("gos-launcher-sep");
        card.append(&sep);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_min_content_height(360);
        scroll.set_max_content_height(360);
        scroll.add_css_class("gos-launcher-scroll");
        scroll.set_child(Some(&list));
        card.append(&scroll);

        window.set_child(Some(&card));

        let ui = Rc::new(LauncherUi {
            core_url: config.core_url.clone(),
            window: window.clone(),
            entry: entry.clone(),
            list,
            scroll,
            apps,
            ai_actions,
            mode: config.mode,
            items: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            selected: RefCell::new(0),
            building: RefCell::new(false),
        });

        // Rebuild results on every keystroke.
        {
            let ui = ui.clone();
            entry.connect_changed(move |entry| {
                refresh_results(&ui, &entry.text());
            });
        }
        // Enter activates the selection; Up/Down move it; Escape dismisses.
        {
            let ui = ui.clone();
            entry.connect_activate(move |_| activate_selected(&ui));
        }
        let keys = gtk::EventControllerKey::new();
        {
            let ui = ui.clone();
            keys.connect_key_pressed(move |_, key, _code, _state| match key {
                gdk::Key::Escape => {
                    ui.window.close();
                    glib::Propagation::Stop
                }
                gdk::Key::Down => {
                    move_selection(&ui, 1);
                    glib::Propagation::Stop
                }
                gdk::Key::Up => {
                    move_selection(&ui, -1);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            });
        }
        window.add_controller(keys);

        // A Spotlight overlay dismisses when it loses focus. The screenshot
        // harness can explicitly hold the window open without switching the app
        // into an alternate data path.
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

        let initial = initial_query_for_mode(config.mode).unwrap_or_default();
        if !initial.is_empty() {
            entry.set_text(&initial);
        }
        refresh_results(&ui, &initial);
        present_with_fade(&window);
        entry.grab_focus();
    }

    struct LauncherUi {
        core_url: String,
        window: gtk::ApplicationWindow,
        entry: gtk::Entry,
        list: gtk::Box,
        scroll: gtk::ScrolledWindow,
        apps: Rc<Vec<BuiltApp>>,
        ai_actions: Rc<AiActions>,
        mode: LauncherMode,
        items: RefCell<Vec<LauncherItem>>,
        rows: RefCell<Vec<gtk::Widget>>,
        selected: RefCell<usize>,
        building: RefCell<bool>,
    }

    /// A calm fade-in on the MOTION_OVERLAY tempo — honoring Reduce Motion (a clean
    /// cut when the desktop disables animations). GTK4 can't scale a top-level via
    /// CSS, so the macOS-style arrival is expressed as an opacity ramp.
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
            let total = goblins_os_design::MOTION_OVERLAY_MS as f64;
            let t = (elapsed / total).clamp(0.0, 1.0);
            // Ease-out cubic for a soft settle.
            let eased = 1.0 - (1.0 - t).powi(3);
            window.set_opacity(eased);
            if t >= 1.0 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn refresh_results(ui: &Rc<LauncherUi>, query: &str) {
        if *ui.building.borrow() {
            return;
        }
        let items = build_items(query, &ui.apps, &ui.ai_actions, ui.mode);
        // Rebuild the visible rows.
        while let Some(child) = ui.list.first_child() {
            ui.list.remove(&child);
        }
        let mut rows: Vec<gtk::Widget> = Vec::new();
        if items.is_empty() {
            let empty = gtk::Label::new(Some("No matches. Press Return to build it."));
            empty.add_css_class("gos-launcher-empty");
            empty.set_xalign(0.0);
            ui.list.append(&empty);
        } else {
            for (index, item) in items.iter().enumerate() {
                let row = result_row(item);
                {
                    let ui = ui.clone();
                    row.connect_clicked(move |_| {
                        *ui.selected.borrow_mut() = index;
                        activate_selected(&ui);
                    });
                }
                ui.list.append(&row);
                rows.push(row.upcast());
            }
        }
        *ui.items.borrow_mut() = items;
        *ui.rows.borrow_mut() = rows;
        *ui.selected.borrow_mut() = 0;
        apply_selection(ui);
    }

    fn result_row(item: &LauncherItem) -> gtk::Button {
        let row = gtk::Button::new();
        row.add_css_class("gos-launcher-row");
        let accessible_label = item.answer.as_deref().unwrap_or(&item.title);
        let accessible_description = if item.subtitle.is_empty() {
            item.kind.to_string()
        } else {
            format!("{}; {}", item.subtitle, item.kind)
        };
        row.set_tooltip_text(Some(&format!(
            "{accessible_label}: {accessible_description}"
        )));
        row.update_property(&[
            gtk::accessible::Property::Label(accessible_label),
            gtk::accessible::Property::Description(accessible_description.as_str()),
            gtk::accessible::Property::KeyShortcuts("Return Space"),
        ]);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        let icon = gtk::Label::new(Some(item.icon));
        icon.add_css_class("gos-launcher-row-icon");
        icon.set_valign(gtk::Align::Center);
        content.append(&icon);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        if let Some(answer) = &item.answer {
            let title = gtk::Label::new(Some(answer));
            title.add_css_class("gos-launcher-answer");
            title.set_xalign(0.0);
            title.set_wrap(false);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&title);
        } else {
            let title = gtk::Label::new(Some(&item.title));
            title.add_css_class("gos-launcher-row-title");
            title.set_xalign(0.0);
            title.set_wrap(false);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&title);
        }
        if !item.subtitle.is_empty() {
            let sub = gtk::Label::new(Some(&item.subtitle));
            sub.add_css_class("gos-launcher-row-sub");
            sub.set_xalign(0.0);
            sub.set_wrap(false);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&sub);
        }
        content.append(&text);

        let kind = gtk::Label::new(Some(item.kind));
        kind.add_css_class("gos-launcher-kind");
        kind.set_valign(gtk::Align::Center);
        content.append(&kind);
        row.set_child(Some(&content));
        row
    }

    fn move_selection(ui: &Rc<LauncherUi>, delta: i32) {
        let len = ui.items.borrow().len();
        if len == 0 {
            return;
        }
        let current = *ui.selected.borrow() as i32;
        let next = (current + delta).rem_euclid(len as i32) as usize;
        *ui.selected.borrow_mut() = next;
        apply_selection(ui);
    }

    fn apply_selection(ui: &Rc<LauncherUi>) {
        let selected = *ui.selected.borrow();
        for (index, row) in ui.rows.borrow().iter().enumerate() {
            if index == selected {
                row.add_css_class("is-selected");
                // Keep the highlighted row in view as the user arrows down.
                let adj = ui.scroll.vadjustment();
                if let Some(bounds) = row.compute_bounds(&ui.list) {
                    let top = bounds.y() as f64;
                    let bottom = top + bounds.height() as f64;
                    if top < adj.value() {
                        adj.set_value(top);
                    } else if bottom > adj.value() + adj.page_size() {
                        adj.set_value(bottom - adj.page_size());
                    }
                }
            } else {
                row.remove_css_class("is-selected");
            }
        }
    }

    fn activate_selected(ui: &Rc<LauncherUi>) {
        if *ui.building.borrow() {
            return;
        }
        let selected = *ui.selected.borrow();
        let action = match ui.items.borrow().get(selected) {
            Some(item) => item.action.clone(),
            None => return,
        };
        match action {
            Action::Noop => {}
            Action::OpenApp(name) => {
                spawn_shell(&["--open-app", &name]);
                ui.window.close();
            }
            Action::OpenLauncherMode(mode) => {
                if let Some(arg) = launcher_arg_for_mode(mode) {
                    spawn(LAUNCHER_BIN, &[arg]);
                }
                ui.window.close();
            }
            Action::OpenScreenshotContext => {
                spawn(SCREENSHOT_CONTEXT_BIN, &[]);
                ui.window.close();
            }
            Action::OpenSettings(panel) => {
                match panel {
                    Some(panel) => spawn(SETTINGS_BIN, &[&format!("--panel={panel}")]),
                    None => spawn(SETTINGS_BIN, &[]),
                }
                ui.window.close();
            }
            Action::OpenStudio => {
                spawn_shell(&["--studio"]);
                ui.window.close();
            }
            Action::ToggleScheme => {
                let dark = goblins_os_ui::resolve_dark();
                goblins_os_ui::set_color_scheme(if dark { "default" } else { "prefer-dark" });
                ui.window.close();
            }
            Action::Copy(text) => {
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&text);
                }
                ui.window.close();
            }
            Action::AskGoblins(query) => ask_goblins(ui, &query),
            Action::AskSelectedText(text) => ask_selected_text(ui, &text),
            Action::WriteWithGoblins(text) => write_with_goblins(ui, &text),
            Action::AskScreenContext(text) => ask_screen_context(ui, &text),
            Action::AskVisualContext(text) => ask_visual_context(ui, &text),
            Action::Build(query) => start_build(ui, &query),
        }
    }

    const LAUNCHER_BIN: &str = "/usr/libexec/goblins-os/goblins-os-launcher";
    const SCREENSHOT_CONTEXT_BIN: &str = "/usr/libexec/goblins-os/goblins-os-screenshot-context";
    const SETTINGS_BIN: &str = "/usr/libexec/goblins-os/goblins-os-settings";
    const SHELL_BIN: &str = "/usr/libexec/goblins-os/goblins-os-shell";

    fn launcher_arg_for_mode(mode: LauncherMode) -> Option<&'static str> {
        match mode {
            LauncherMode::Normal => None,
            LauncherMode::Assistant => Some("--assistant"),
            LauncherMode::SelectedText => Some("--selected-text"),
            LauncherMode::WritingTools => Some("--writing-tools"),
            LauncherMode::ScreenContext => Some("--screen-context"),
            LauncherMode::VisualContext => Some("--visual-context"),
        }
    }

    fn spawn(program: &str, args: &[&str]) {
        let _ = Command::new(program).args(args).spawn();
    }

    fn spawn_shell(args: &[&str]) {
        spawn(SHELL_BIN, args);
    }

    /// Kick off an on-device build from the launcher: the build row goes to a
    /// working state while the Goblins AI runtime designs the app, then the new app
    /// opens in a standalone Build Studio window.
    fn start_build(ui: &Rc<LauncherUi>, query: &str) {
        let query = query.trim().to_string();
        if query.is_empty() {
            return;
        }
        *ui.building.borrow_mut() = true;
        ui.entry.set_sensitive(false);

        while let Some(child) = ui.list.first_child() {
            ui.list.remove(&child);
        }
        let working = gtk::Label::new(Some(&format!("Building “{query}” on-device…")));
        working.add_css_class("gos-launcher-empty");
        working.set_xalign(0.0);
        ui.list.append(&working);

        let (tx, rx) = mpsc::channel::<Result<String, String>>();
        let core_url = ui.core_url.clone();
        let intent = query.clone();
        thread::spawn(move || {
            let _ = tx.send(submit_build(&core_url, &intent));
        });

        let ui = ui.clone();
        glib::timeout_add_local(Duration::from_millis(90), move || match rx.try_recv() {
            Ok(Ok(name)) => {
                spawn_shell(&["--open-app", &name]);
                ui.window.close();
                glib::ControlFlow::Break
            }
            Ok(Err(detail)) => {
                *ui.building.borrow_mut() = false;
                ui.entry.set_sensitive(true);
                while let Some(child) = ui.list.first_child() {
                    ui.list.remove(&child);
                }
                let error = gtk::Label::new(Some(&detail));
                error.add_css_class("gos-launcher-empty");
                error.set_xalign(0.0);
                error.set_wrap(true);
                ui.list.append(&error);
                ui.entry.grab_focus();
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                *ui.building.borrow_mut() = false;
                ui.entry.set_sensitive(true);
                glib::ControlFlow::Break
            }
        });
    }

    /// Ask Goblin from the global launcher. This uses the same
    /// OS-owned core relay as Settings and Studio, so a missing model/key returns
    /// the real setup message instead of an invented answer.
    fn ask_goblins(ui: &Rc<LauncherUi>, query: &str) {
        let query = query.trim().to_string();
        if query.is_empty() {
            return;
        }
        *ui.building.borrow_mut() = true;
        ui.entry.set_sensitive(false);

        while let Some(child) = ui.list.first_child() {
            ui.list.remove(&child);
        }
        let working = gtk::Label::new(Some(&format!("Asking Goblins AI about “{query}”…")));
        working.add_css_class("gos-launcher-empty");
        working.set_xalign(0.0);
        working.set_wrap(true);
        ui.list.append(&working);

        let (tx, rx) = mpsc::channel::<Result<String, String>>();
        let core_url = ui.core_url.clone();
        let question = query.clone();
        thread::spawn(move || {
            let _ = tx.send(submit_question(&core_url, &question));
        });

        let ui = ui.clone();
        glib::timeout_add_local(Duration::from_millis(90), move || match rx.try_recv() {
            Ok(Ok(answer)) => {
                *ui.building.borrow_mut() = false;
                ui.entry.set_sensitive(true);
                while let Some(child) = ui.list.first_child() {
                    ui.list.remove(&child);
                }
                let answer = gtk::Label::new(Some(&answer));
                answer.add_css_class("gos-launcher-empty");
                answer.set_xalign(0.0);
                answer.set_wrap(true);
                ui.list.append(&answer);
                ui.entry.grab_focus();
                glib::ControlFlow::Break
            }
            Ok(Err(detail)) => {
                *ui.building.borrow_mut() = false;
                ui.entry.set_sensitive(true);
                while let Some(child) = ui.list.first_child() {
                    ui.list.remove(&child);
                }
                let error = gtk::Label::new(Some(&detail));
                error.add_css_class("gos-launcher-empty");
                error.set_xalign(0.0);
                error.set_wrap(true);
                ui.list.append(&error);
                ui.entry.grab_focus();
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                *ui.building.borrow_mut() = false;
                ui.entry.set_sensitive(true);
                glib::ControlFlow::Break
            }
        });
    }

    fn ask_selected_text(ui: &Rc<LauncherUi>, text: &str) {
        ask_context(
            ui,
            text,
            "Asking Goblins AI about selected text",
            submit_selected_text_context,
        );
    }

    fn write_with_goblins(ui: &Rc<LauncherUi>, text: &str) {
        ask_context(
            ui,
            text,
            "Writing with Goblin",
            submit_writing_tools_context,
        );
    }

    fn ask_screen_context(ui: &Rc<LauncherUi>, text: &str) {
        ask_context(
            ui,
            text,
            "Asking Goblins AI about visible context",
            submit_screen_context,
        );
    }

    fn ask_visual_context(ui: &Rc<LauncherUi>, text: &str) {
        ask_context(
            ui,
            text,
            "Asking Goblins AI about the visual context",
            submit_visual_context,
        );
    }

    fn ask_context(
        ui: &Rc<LauncherUi>,
        context: &str,
        working_label: &'static str,
        submit: fn(&str, &str) -> Result<String, String>,
    ) {
        let context = context.trim().to_string();
        if context.is_empty() {
            return;
        }
        *ui.building.borrow_mut() = true;
        ui.entry.set_sensitive(false);

        while let Some(child) = ui.list.first_child() {
            ui.list.remove(&child);
        }
        let working = gtk::Label::new(Some(&format!("{working_label}…")));
        working.add_css_class("gos-launcher-empty");
        working.set_xalign(0.0);
        working.set_wrap(true);
        ui.list.append(&working);

        let (tx, rx) = mpsc::channel::<Result<String, String>>();
        let core_url = ui.core_url.clone();
        thread::spawn(move || {
            let _ = tx.send(submit(&core_url, &context));
        });

        let ui = ui.clone();
        glib::timeout_add_local(Duration::from_millis(90), move || match rx.try_recv() {
            Ok(Ok(answer)) => {
                *ui.building.borrow_mut() = false;
                ui.entry.set_sensitive(true);
                while let Some(child) = ui.list.first_child() {
                    ui.list.remove(&child);
                }
                let answer = gtk::Label::new(Some(&answer));
                answer.add_css_class("gos-launcher-empty");
                answer.set_xalign(0.0);
                answer.set_wrap(true);
                ui.list.append(&answer);
                ui.entry.grab_focus();
                glib::ControlFlow::Break
            }
            Ok(Err(detail)) => {
                *ui.building.borrow_mut() = false;
                ui.entry.set_sensitive(true);
                while let Some(child) = ui.list.first_child() {
                    ui.list.remove(&child);
                }
                let error = gtk::Label::new(Some(&detail));
                error.add_css_class("gos-launcher-empty");
                error.set_xalign(0.0);
                error.set_wrap(true);
                ui.list.append(&error);
                ui.entry.grab_focus();
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                *ui.building.borrow_mut() = false;
                ui.entry.set_sensitive(true);
                glib::ControlFlow::Break
            }
        });
    }

    /// Compose the ranked result list from the query: computed answers first, then
    /// fuzzy-matched built apps, Settings sections, and quick actions, then the
    /// standing "Build a new app" action at the foot.
    fn build_items(
        query: &str,
        apps: &[BuiltApp],
        ai_actions: &AiActions,
        mode: LauncherMode,
    ) -> Vec<LauncherItem> {
        let trimmed = query.trim();
        let mut items: Vec<LauncherItem> = Vec::new();

        match mode {
            LauncherMode::Assistant if trimmed.is_empty() => {
                items.push(assistant_prompt_item(&ai_actions.ask));
                items.push(context_launcher_item(
                    "Ask about selected text",
                    "Use selected-text context after explicit invocation",
                    "Text",
                    "\u{2726}",
                    &ai_actions.selected_text,
                    LauncherMode::SelectedText,
                ));
                items.push(context_launcher_item(
                    "Write with Goblin",
                    "Rewrite, proofread, summarize, or change tone for selected text",
                    "Writing",
                    "\u{270D}",
                    &ai_actions.writing_tools,
                    LauncherMode::WritingTools,
                ));
                items.push(context_launcher_item(
                    "Summarize screen context",
                    "Use provided visible context without silent capture",
                    "Screen",
                    "\u{25A3}",
                    &ai_actions.screen_context,
                    LauncherMode::ScreenContext,
                ));
                items.push(context_launcher_item(
                    "Ask about a screenshot",
                    VISUAL_CONTEXT_SUBTITLE,
                    "Visual",
                    "\u{25A7}",
                    &ai_actions.screen_context,
                    LauncherMode::VisualContext,
                ));
                items.push(LauncherItem {
                    icon: "\u{25C8}",
                    title: "Goblins AI Settings".to_string(),
                    subtitle: "Models, permissions, and recent action history".to_string(),
                    kind: "Setup",
                    answer: None,
                    action: Action::OpenSettings(Some("models")),
                });
                return items;
            }
            LauncherMode::SelectedText => {
                if trimmed.is_empty() {
                    items.push(context_mode_prompt_item(
                        "Ask about selected text",
                        "Paste or type selected text above. Goblins OS does not read selection silently.",
                        "Text",
                        "\u{2726}",
                        &ai_actions.selected_text,
                    ));
                    return items;
                }
                items.push(ai_action_item(
                    "Ask about selected text",
                    trimmed,
                    "Text",
                    "\u{2726}",
                    &ai_actions.selected_text,
                    Action::AskSelectedText(trimmed.to_string()),
                ));
                return items;
            }
            LauncherMode::WritingTools => {
                if trimmed.is_empty() {
                    items.push(context_mode_prompt_item(
                        "Write with Goblin",
                        "Paste or type text above. Goblins OS does not read selection silently.",
                        "Writing",
                        "\u{270D}",
                        &ai_actions.writing_tools,
                    ));
                    return items;
                }
                items.push(ai_action_item(
                    "Write with Goblin",
                    trimmed,
                    "Writing",
                    "\u{270D}",
                    &ai_actions.writing_tools,
                    Action::WriteWithGoblins(trimmed.to_string()),
                ));
                return items;
            }
            LauncherMode::ScreenContext => {
                if trimmed.is_empty() {
                    items.push(context_mode_prompt_item(
                        "Summarize screen context",
                        "Describe visible content or paste recognized text. Nothing is captured silently.",
                        "Screen",
                        "\u{25A3}",
                        &ai_actions.screen_context,
                    ));
                    return items;
                }
                items.push(ai_action_item(
                    "Summarize screen context",
                    trimmed,
                    "Screen",
                    "\u{25A3}",
                    &ai_actions.screen_context,
                    Action::AskScreenContext(trimmed.to_string()),
                ));
                return items;
            }
            LauncherMode::VisualContext => {
                if trimmed.is_empty() {
                    items.push(context_mode_prompt_item(
                        "Ask about a screenshot",
                        "Describe the screenshot or paste recognized text. Goblins OS does not capture pixels silently.",
                        "Visual",
                        "\u{25A7}",
                        &ai_actions.screen_context,
                    ));
                    return items;
                }
                items.push(ai_action_item(
                    "Ask about a screenshot",
                    trimmed,
                    "Visual",
                    "\u{25A7}",
                    &ai_actions.screen_context,
                    Action::AskVisualContext(trimmed.to_string()),
                ));
                return items;
            }
            _ => {}
        }

        // 1) Instant answer (math / unit conversion) — always first when present.
        if let Some(answer) = eval_math(trimmed) {
            items.push(LauncherItem {
                icon: "=",
                title: trimmed.to_string(),
                subtitle: "Press Return to copy".to_string(),
                kind: "Math",
                answer: Some(answer.clone()),
                action: Action::Copy(answer),
            });
        } else if let Some(answer) = convert_units(trimmed) {
            items.push(LauncherItem {
                icon: "\u{21C4}", // ⇄
                title: trimmed.to_string(),
                subtitle: "Press Return to copy".to_string(),
                kind: "Convert",
                answer: Some(answer.clone()),
                action: Action::Copy(answer),
            });
        }

        // 2) Built apps (the OS ships none — these are the user's). Match the name
        //    OR the intent, so "budget" finds an app described as a budget splitter.
        let mut scored: Vec<(i32, &BuiltApp)> = apps
            .iter()
            .filter_map(|app| {
                fuzzy_score(trimmed, &app.name)
                    .into_iter()
                    .chain(fuzzy_score(trimmed, &app.intent))
                    .max()
                    .map(|score| (score, app))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, app) in scored.into_iter().take(6) {
            items.push(LauncherItem {
                icon: "\u{25C9}", // ◉
                title: app.name.clone(),
                subtitle: source_label(&app.source).to_string(),
                kind: "App",
                answer: None,
                action: Action::OpenApp(app.name.clone()),
            });
        }

        // 3) Settings sections + quick actions.
        for entry in STATIC_ENTRIES {
            if let Some(score) = best_score(trimmed, entry.keywords) {
                items.push((entry.make)(score));
            }
        }
        // Sort the static block by score but keep computed answers / apps on top by
        // re-sorting only the tail we just appended is fiddly; instead we score the
        // static entries inline and trust their high keyword scores to rank well.

        // 4) The standing build action — the OS's thesis, one keystroke away.
        if !trimmed.is_empty() {
            items.push(ai_action_item(
                "Ask Goblin",
                trimmed,
                "AI",
                "\u{2726}",
                &ai_actions.ask,
                Action::AskGoblins(trimmed.to_string()),
            ));
            items.push(ai_action_item(
                "Build a new app",
                trimmed,
                "Build",
                "+",
                &ai_actions.build,
                Action::Build(trimmed.to_string()),
            ));
        }
        items
    }

    fn assistant_prompt_item(availability: &AiActionAvailability) -> LauncherItem {
        if availability.enabled {
            LauncherItem {
                icon: "\u{2726}",
                title: "Ask Goblin".to_string(),
                subtitle: "Type a question above, then press Return".to_string(),
                kind: "AI",
                answer: None,
                action: Action::OpenSettings(Some("models")),
            }
        } else {
            LauncherItem {
                icon: "\u{2726}",
                title: "Set up Goblins AI".to_string(),
                subtitle: availability.reason.clone(),
                kind: "Setup",
                answer: None,
                action: Action::OpenSettings(setup_panel_for_reason(&availability.reason)),
            }
        }
    }

    fn context_launcher_item(
        title: &str,
        subtitle: &str,
        kind: &'static str,
        icon: &'static str,
        availability: &AiActionAvailability,
        mode: LauncherMode,
    ) -> LauncherItem {
        if availability.enabled {
            let action = if matches!(mode, LauncherMode::VisualContext) {
                Action::OpenScreenshotContext
            } else {
                Action::OpenLauncherMode(mode)
            };
            LauncherItem {
                icon,
                title: title.to_string(),
                subtitle: subtitle.to_string(),
                kind,
                answer: None,
                action,
            }
        } else {
            LauncherItem {
                icon,
                title: format!("Set up {title}"),
                subtitle: availability.reason.clone(),
                kind: "Setup",
                answer: None,
                action: Action::OpenSettings(setup_panel_for_reason(&availability.reason)),
            }
        }
    }

    fn context_mode_prompt_item(
        title: &str,
        subtitle: &str,
        kind: &'static str,
        icon: &'static str,
        availability: &AiActionAvailability,
    ) -> LauncherItem {
        if availability.enabled {
            LauncherItem {
                icon,
                title: title.to_string(),
                subtitle: subtitle.to_string(),
                kind,
                answer: None,
                action: Action::Noop,
            }
        } else {
            LauncherItem {
                icon,
                title: format!("Set up {title}"),
                subtitle: availability.reason.clone(),
                kind: "Setup",
                answer: None,
                action: Action::OpenSettings(setup_panel_for_reason(&availability.reason)),
            }
        }
    }

    fn ai_action_item(
        label: &str,
        query: &str,
        kind: &'static str,
        icon: &'static str,
        availability: &AiActionAvailability,
        enabled_action: Action,
    ) -> LauncherItem {
        if availability.enabled {
            LauncherItem {
                icon,
                title: format!("{label}: {query}"),
                subtitle: "Uses Goblins AI actions with OS-owned model access".to_string(),
                kind,
                answer: None,
                action: enabled_action,
            }
        } else {
            LauncherItem {
                icon,
                title: format!("Set up {label}: {query}"),
                subtitle: availability.reason.clone(),
                kind: "Setup",
                answer: None,
                action: Action::OpenSettings(setup_panel_for_reason(&availability.reason)),
            }
        }
    }

    fn setup_panel_for_reason(reason: &str) -> Option<&'static str> {
        let lower = reason.to_ascii_lowercase();
        if lower.contains("permission") || lower.contains("denied") || lower.contains("policy") {
            Some("policy")
        } else {
            Some("models")
        }
    }

    struct StaticEntry {
        keywords: &'static [&'static str],
        make: fn(i32) -> LauncherItem,
    }

    fn best_score(query: &str, keywords: &[&str]) -> Option<i32> {
        if query.trim().is_empty() {
            return None;
        }
        keywords.iter().filter_map(|k| fuzzy_score(query, k)).max()
    }

    const STATIC_ENTRIES: &[StaticEntry] = &[
        StaticEntry {
            keywords: &["Settings", "Overview", "preferences"],
            make: |_| LauncherItem {
                icon: "\u{2699}", // ⚙
                title: "Settings".to_string(),
                subtitle: "Overview · engine · network · privacy".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(None),
            },
        },
        StaticEntry {
            keywords: &["Desktop & Dock", "dock", "desktop", "window controls"],
            make: |_| LauncherItem {
                icon: "\u{25C8}", // ◈
                title: "Settings — Desktop & Dock".to_string(),
                subtitle: "Dock, app launcher, and window controls".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("desktop-dock")),
            },
        },
        StaticEntry {
            keywords: &[
                "Menu Bar",
                "Control Center",
                "top bar",
                "quick settings",
                "clock",
            ],
            make: |_| LauncherItem {
                icon: "\u{25C8}",
                title: "Settings — Menu Bar & Control Center".to_string(),
                subtitle: "Top bar, quick settings, and clock".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("menu-bar-control-center")),
            },
        },
        StaticEntry {
            keywords: &["Lock Screen", "lock", "screen lock", "blank screen"],
            make: |_| LauncherItem {
                icon: "\u{25C8}",
                title: "Settings — Lock Screen".to_string(),
                subtitle: "Screen lock and lock-screen privacy".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("lock-screen")),
            },
        },
        StaticEntry {
            keywords: &["Date & Time", "date", "time", "clock", "timezone"],
            make: |_| LauncherItem {
                icon: "\u{25C8}",
                title: "Settings — Date & Time".to_string(),
                subtitle: "Clock, time zone, and calendar format".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("date-time")),
            },
        },
        StaticEntry {
            keywords: &[
                "Language & Region",
                "language",
                "region",
                "locale",
                "formats",
            ],
            make: |_| LauncherItem {
                icon: "\u{25C8}",
                title: "Settings — Language & Region".to_string(),
                subtitle: "Language, region, and input language".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("language-region")),
            },
        },
        StaticEntry {
            keywords: &[
                "Security",
                "password",
                "firewall",
                "secure storage",
                "secrets",
            ],
            make: |_| LauncherItem {
                icon: "\u{25C8}",
                title: "Settings — Security".to_string(),
                subtitle: "Credentials, boot integrity, and secret boundaries".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("security")),
            },
        },
        StaticEntry {
            keywords: &[
                "Write with Goblin",
                "writing tools",
                "rewrite",
                "proofread",
                "summarize text",
                "change tone",
            ],
            make: |_| LauncherItem {
                icon: "\u{270D}",
                title: "Write with Goblin".to_string(),
                subtitle: "Rewrite, proofread, summarize, or change tone for selected text"
                    .to_string(),
                kind: "Writing",
                answer: None,
                action: Action::OpenLauncherMode(LauncherMode::WritingTools),
            },
        },
        StaticEntry {
            keywords: &[
                "Screenshot",
                "visual context",
                "screen image",
                "Ask Goblin about screenshot",
                "OCR",
            ],
            make: |_| LauncherItem {
                icon: "\u{25A7}",
                title: "Ask Goblin about a screenshot".to_string(),
                subtitle: VISUAL_CONTEXT_SUBTITLE.to_string(),
                kind: "Visual",
                answer: None,
                action: Action::OpenScreenshotContext,
            },
        },
        StaticEntry {
            keywords: &[
                "Ask Goblin",
                "Goblins AI",
                "assistant",
                "selected text",
                "screenshot",
            ],
            make: |_| LauncherItem {
                icon: "\u{2726}",
                title: "Settings — Goblins AI".to_string(),
                subtitle: "Assistant actions, permissions, and model setup".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("models")),
            },
        },
        StaticEntry {
            keywords: &["Models", "engine", "GPT-OSS", "Codex", "OpenAI"],
            make: |_| LauncherItem {
                icon: "\u{25C8}", // ◈
                title: "Settings — Models".to_string(),
                subtitle: "On-device GPT-OSS · OpenAI account".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("models")),
            },
        },
        StaticEntry {
            keywords: &["Policy", "permissions", "consent"],
            make: |_| LauncherItem {
                icon: "\u{25C8}",
                title: "Settings — Policy".to_string(),
                subtitle: "Permissions and consent".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("policy")),
            },
        },
        StaticEntry {
            keywords: &["Recovery", "rollback", "reset"],
            make: |_| LauncherItem {
                icon: "\u{25C8}",
                title: "Settings — Recovery".to_string(),
                subtitle: "Rollback and recovery".to_string(),
                kind: "Settings",
                answer: None,
                action: Action::OpenSettings(Some("recovery")),
            },
        },
        StaticEntry {
            keywords: &["Build Studio", "studio", "agent", "build"],
            make: |_| LauncherItem {
                icon: "\u{25C9}",
                title: "Open Build Studio".to_string(),
                subtitle: "Create and refine Goblins OS apps".to_string(),
                kind: "Action",
                answer: None,
                action: Action::OpenStudio,
            },
        },
        StaticEntry {
            keywords: &["Appearance", "dark mode", "light mode", "theme", "toggle"],
            make: |_| LauncherItem {
                icon: "\u{25D1}", // ◑
                title: "Toggle Light / Dark".to_string(),
                subtitle: "Switch the desktop appearance".to_string(),
                kind: "Action",
                answer: None,
                action: Action::ToggleScheme,
            },
        },
    ];

    fn source_label(source: &str) -> &'static str {
        match source {
            "local-gpt-oss" | "gpt-oss" | "local" => "Built on-device · GPT-OSS",
            "codex" => "Built with Build Studio",
            "openai-api" | "openai" => "Built with your OpenAI key",
            _ => "Built app",
        }
    }

    // ── Minimal HTTP-to-core client (loopback only) ──────────────────────────
    // The same compact, dependency-free TCP client the shell uses; the launcher
    // only needs to read the app list and post a build.

    fn fetch_apps(core_url: &str) -> Vec<BuiltApp> {
        get_json::<AppList>(core_url, "/v1/apps")
            .map(|list| list.apps)
            .unwrap_or_default()
    }

    fn fetch_ai_actions(core_url: &str) -> AiActions {
        let fallback =
            "Goblins AI setup is not ready. Open Models to configure GPT-OSS or your OpenAI key."
                .to_string();
        let Some(catalog) = get_json::<AiActionCatalog>(core_url, "/v1/ai/actions") else {
            return AiActions {
                ask: AiActionAvailability {
                    enabled: false,
                    reason: fallback.clone(),
                },
                selected_text: AiActionAvailability {
                    enabled: false,
                    reason: fallback.clone(),
                },
                writing_tools: AiActionAvailability {
                    enabled: false,
                    reason: fallback.clone(),
                },
                screen_context: AiActionAvailability {
                    enabled: false,
                    reason: fallback.clone(),
                },
                build: AiActionAvailability {
                    enabled: false,
                    reason: fallback,
                },
            };
        };
        AiActions {
            ask: action_availability(&catalog, "ask-goblins"),
            selected_text: action_availability(&catalog, "ask-selected-text"),
            writing_tools: action_availability(&catalog, "write-with-goblins"),
            screen_context: action_availability(&catalog, "summarize-screen"),
            build: action_availability(&catalog, "build-app"),
        }
    }

    fn action_availability(catalog: &AiActionCatalog, id: &str) -> AiActionAvailability {
        catalog
            .actions
            .iter()
            .find(|action| action.id == id)
            .map(|action| AiActionAvailability {
                enabled: action.enabled,
                reason: action.reason.clone(),
            })
            .unwrap_or_else(|| AiActionAvailability {
                enabled: false,
                reason: "This Goblins AI action is not registered in the OS action catalog."
                    .to_string(),
            })
    }

    /// Render hook: the value of `GOBLINS_OS_RENDER_QUERY` is pre-typed into the
    /// field for deterministic screenshot QA. It never seeds apps or answers.
    fn render_query() -> Option<String> {
        match std::env::var("GOBLINS_OS_RENDER_QUERY") {
            Ok(value) if !matches!(value.trim(), "" | "1" | "true") => Some(value),
            _ => None,
        }
    }

    fn initial_query_for_mode(mode: LauncherMode) -> Option<String> {
        render_query().or_else(|| match mode {
            LauncherMode::SelectedText => {
                env_context_value(SELECTED_TEXT_CONTEXT_ENV, CONTEXT_TEXT_MAX_CHARS)
            }
            LauncherMode::WritingTools => {
                env_context_value(SELECTED_TEXT_CONTEXT_ENV, CONTEXT_TEXT_MAX_CHARS)
            }
            LauncherMode::ScreenContext => {
                env_context_value(SCREEN_CONTEXT_TEXT_ENV, CONTEXT_TEXT_MAX_CHARS)
            }
            LauncherMode::VisualContext => {
                env_context_value(VISUAL_CONTEXT_SUMMARY_ENV, CONTEXT_TEXT_MAX_CHARS)
            }
            _ => None,
        })
    }

    fn env_context_value(name: &str, max_chars: usize) -> Option<String> {
        std::env::var(name)
            .ok()
            .and_then(|value| bounded_context_value(&value, max_chars))
    }

    fn env_context_value_or(name: &str, fallback: &str, max_chars: usize) -> String {
        env_context_value(name, max_chars).unwrap_or_else(|| fallback.to_string())
    }

    fn context_app_name(fallback: &str) -> String {
        env_context_value_or(CONTEXT_APP_ENV, fallback, CONTEXT_METADATA_MAX_CHARS)
    }

    fn context_window_title(fallback: &str) -> String {
        env_context_value_or(
            CONTEXT_WINDOW_TITLE_ENV,
            fallback,
            CONTEXT_METADATA_MAX_CHARS,
        )
    }

    fn submit_build(core_url: &str, intent: &str) -> Result<String, String> {
        let body = serde_json::json!({ "intent": intent }).to_string();
        let response = http_request(core_url, "POST", "/v1/apps/builds", Some(&body))
            .map_err(|_| "Goblins OS could not reach the on-device builder.".to_string())?;
        #[derive(Deserialize)]
        struct BuildOutcome {
            ok: bool,
            #[serde(default)]
            text: String,
            app: Option<BuiltApp>,
        }
        let outcome: BuildOutcome = serde_json::from_slice(&response.1)
            .map_err(|_| "Goblins OS could not read the build result.".to_string())?;
        if (200..=299).contains(&response.0) && outcome.ok {
            outcome
                .app
                .map(|app| app.name)
                .ok_or_else(|| "The build returned no app record.".to_string())
        } else if outcome.text.is_empty() {
            Err("The build did not complete.".to_string())
        } else {
            Err(outcome.text)
        }
    }

    fn submit_question(core_url: &str, question: &str) -> Result<String, String> {
        let body = serde_json::json!({ "message": question }).to_string();
        let response = http_request(core_url, "POST", "/v1/ai/runtime", Some(&body))
            .map_err(|_| "Goblins OS could not reach Goblins AI.".to_string())?;
        #[derive(Deserialize)]
        struct ResidentOutcome {
            text: String,
        }
        let outcome: ResidentOutcome = serde_json::from_slice(&response.1)
            .map_err(|_| "Goblins OS could not read the assistant response.".to_string())?;
        if (200..=299).contains(&response.0) {
            Ok(outcome.text)
        } else {
            Err(outcome.text)
        }
    }

    fn submit_selected_text_context(core_url: &str, selected_text: &str) -> Result<String, String> {
        let app = context_app_name("Goblins OS Launcher");
        let window_title = context_window_title("Selected text context");
        let body = serde_json::json!({
            "text": selected_text,
            "app": app,
            "window_title": window_title,
            "question": "Explain this selected text, summarize the important points, and suggest one safe next action."
        })
        .to_string();
        submit_ai_context(
            core_url,
            "/v1/ai/selected-text-context",
            &body,
            "Goblins OS could not reach selected-text context.",
            "Goblins OS could not read the selected-text answer.",
        )
    }

    fn submit_writing_tools_context(core_url: &str, selected_text: &str) -> Result<String, String> {
        let app = context_app_name("Goblins OS Launcher");
        let window_title = context_window_title("Writing tools");
        let body = serde_json::json!({
            "text": selected_text,
            "app": app,
            "window_title": window_title,
            "question": "Rewrite, proofread, summarize, or adjust this text. Preserve meaning unless a change is clearly requested. Return ready-to-use text first."
        })
        .to_string();
        submit_ai_context(
            core_url,
            "/v1/ai/write-selected-text",
            &body,
            "Goblins OS could not reach writing tools.",
            "Goblins OS could not read the writing result.",
        )
    }

    fn submit_screen_context(core_url: &str, visible_context: &str) -> Result<String, String> {
        let source = env_context_value_or(
            SCREEN_CONTEXT_SOURCE_ENV,
            "launcher-screen-context",
            CONTEXT_METADATA_MAX_CHARS,
        );
        let app = context_app_name("Goblins OS Launcher");
        let window_title = context_window_title("Screen context");
        let body = serde_json::json!({
            "source": source,
            "app": app,
            "window_title": window_title,
            "visible_text": visible_context,
            "question": "Summarize what is visible, identify likely next steps, and ask before changing anything."
        })
        .to_string();
        submit_ai_context(
            core_url,
            "/v1/ai/screen-context",
            &body,
            "Goblins OS could not reach screen context.",
            "Goblins OS could not read the screen-context answer.",
        )
    }

    fn submit_visual_context(core_url: &str, visual_summary: &str) -> Result<String, String> {
        let source = env_context_value_or(
            SCREEN_CONTEXT_SOURCE_ENV,
            "launcher-visual-context",
            CONTEXT_METADATA_MAX_CHARS,
        );
        let app = context_app_name("Goblins OS Launcher");
        let window_title = context_window_title("Screenshot context");
        let body = serde_json::json!({
            "source": source,
            "app": app,
            "window_title": window_title,
            "visual_summary": visual_summary,
            "question": "Summarize the provided screenshot or visual description, identify likely next steps, and ask before changing anything."
        })
        .to_string();
        submit_ai_context(
            core_url,
            "/v1/ai/screen-context",
            &body,
            "Goblins OS could not reach screenshot context.",
            "Goblins OS could not read the screenshot-context answer.",
        )
    }

    fn submit_ai_context(
        core_url: &str,
        path: &str,
        body: &str,
        reach_error: &str,
        parse_error: &str,
    ) -> Result<String, String> {
        let response = http_request(core_url, "POST", path, Some(body))
            .map_err(|_| reach_error.to_string())?;
        #[derive(Deserialize)]
        struct ContextOutcome {
            text: String,
        }
        let outcome: ContextOutcome =
            serde_json::from_slice(&response.1).map_err(|_| parse_error.to_string())?;
        if (200..=299).contains(&response.0) {
            Ok(outcome.text)
        } else {
            Err(outcome.text)
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(core_url: &str, path: &str) -> Option<T> {
        let (status, body) = http_request(core_url, "GET", path, None).ok()?;
        if !(200..=299).contains(&status) {
            return None;
        }
        serde_json::from_slice(&body).ok()
    }

    /// One blocking request to the loopback core. Returns (status, body). A long
    /// read window: a build runs the Goblins AI runtime and legitimately takes seconds.
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
            .set_read_timeout(Some(Duration::from_secs(180)))
            .map_err(|_| ())?;
        stream
            .set_write_timeout(Some(Duration::from_millis(2000)))
            .map_err(|_| ())?;

        let request = match body {
            Some(payload) => format!(
                "POST {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
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
    use super::{
        bounded_context_value, convert_units, eval_math, fuzzy_score, looks_like_math,
        LauncherMode, VISUAL_CONTEXT_SUBTITLE,
    };

    #[test]
    fn evaluates_arithmetic_with_precedence() {
        assert_eq!(eval_math("2 + 2"), Some("4".to_string()));
        assert_eq!(eval_math("2 + 3 * 4"), Some("14".to_string()));
        assert_eq!(eval_math("(2 + 3) * 4"), Some("20".to_string()));
        assert_eq!(eval_math("2 ^ 10"), Some("1024".to_string()));
        assert_eq!(eval_math("10 / 4"), Some("2.5".to_string()));
        assert_eq!(eval_math("-3 + 5"), Some("2".to_string()));
        assert_eq!(eval_math("1,000 + 1"), Some("1001".to_string()));
    }

    #[test]
    fn rejects_non_math_and_bare_numbers() {
        assert_eq!(eval_math("todo list"), None);
        assert_eq!(eval_math("42"), None);
        assert_eq!(eval_math("5 / 0"), None);
        assert!(!looks_like_math("focus timer"));
        assert!(looks_like_math("3*7"));
    }

    #[test]
    fn converts_units_across_dimensions() {
        assert_eq!(convert_units("100 cm to m"), Some("1 m".to_string()));
        assert_eq!(convert_units("1 km in m"), Some("1000 m".to_string()));
        assert_eq!(convert_units("0 c to f"), Some("32 °F".to_string()));
        assert_eq!(convert_units("212 f to c"), Some("100 °C".to_string()));
        assert_eq!(convert_units("1 kg to g"), Some("1000 g".to_string()));
        // Cross-dimension nonsense never resolves.
        assert_eq!(convert_units("5 kg to m"), None);
        assert_eq!(convert_units("hello to world"), None);
    }

    #[test]
    fn fuzzy_ranks_prefix_and_boundary_matches_higher() {
        let prefix = fuzzy_score("set", "Settings").unwrap();
        let scattered = fuzzy_score("set", "Reset target").unwrap();
        assert!(prefix > scattered);
        assert_eq!(fuzzy_score("xyz", "Settings"), None);
        // An empty query matches everything (neutral score).
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }

    #[test]
    fn launcher_modes_cover_ai_context_entrypoints() {
        assert!(matches!(
            LauncherMode::from_values(["--assistant"], None),
            LauncherMode::Assistant
        ));
        assert!(matches!(
            LauncherMode::from_values(["--selected-text"], None),
            LauncherMode::SelectedText
        ));
        assert!(matches!(
            LauncherMode::from_values(["--writing-tools"], None),
            LauncherMode::WritingTools
        ));
        assert!(matches!(
            LauncherMode::from_values(["--write-with-goblins"], None),
            LauncherMode::WritingTools
        ));
        assert!(matches!(
            LauncherMode::from_values(["--screen-context"], None),
            LauncherMode::ScreenContext
        ));
        assert!(matches!(
            LauncherMode::from_values(["--visual-context"], None),
            LauncherMode::VisualContext
        ));
        assert!(matches!(
            LauncherMode::from_values(["--screenshot-context"], None),
            LauncherMode::VisualContext
        ));
        assert!(matches!(
            LauncherMode::from_values(std::iter::empty::<&str>(), Some("screen".to_string())),
            LauncherMode::ScreenContext
        ));
        assert!(matches!(
            LauncherMode::from_values(std::iter::empty::<&str>(), Some("writing".to_string())),
            LauncherMode::WritingTools
        ));
        assert!(matches!(
            LauncherMode::from_values(std::iter::empty::<&str>(), Some("screenshot".to_string())),
            LauncherMode::VisualContext
        ));
    }

    #[test]
    fn launcher_search_field_uses_native_accessible_icon() {
        let source = include_str!("main.rs");
        assert!(source.contains("gtk::Image::from_icon_name(\"system-search-symbolic\")"));
        assert!(source.contains("Search Goblins OS"));
        assert!(source.contains("Search field icon"));

        let old_escape = ['\\', 'u', '{', '2', '3', '1', '5', '}']
            .iter()
            .collect::<String>();
        let old_glyph = char::from_u32(0x2315).unwrap().to_string();
        let old_comment = ["telephone", "recorder"].join("-");
        assert!(!source.contains(&old_escape));
        assert!(!source.contains(&old_glyph));
        assert!(!source.contains(&old_comment));
    }

    #[test]
    fn launcher_result_rows_use_native_button_semantics() {
        let source = include_str!("main.rs");
        assert!(source.contains("fn result_row(item: &LauncherItem) -> gtk::Button"));
        assert!(source.contains("gtk::accessible::Property::Label(accessible_label)"));
        assert!(source.contains("gtk::accessible::Property::KeyShortcuts(\"Return Space\")"));
        assert!(source.contains("row.connect_clicked(move |_|"));
    }

    #[test]
    fn visual_context_copy_is_os_owned_not_toolkit_branding() {
        assert!(VISUAL_CONTEXT_SUBTITLE.contains("Capture the screen"));
        assert!(VISUAL_CONTEXT_SUBTITLE.contains("local-only visual context"));
        assert!(!VISUAL_CONTEXT_SUBTITLE.contains("GNOME"));
        assert!(!VISUAL_CONTEXT_SUBTITLE.contains("gdbus"));
        assert!(!VISUAL_CONTEXT_SUBTITLE.contains("D-Bus"));
    }

    #[test]
    fn ai_action_copy_is_os_owned_not_backend_plumbing() {
        let source = include_str!("main.rs");
        assert!(source.contains("Uses Goblins AI actions with OS-owned model access"));
        assert!(source.contains("Built with Build Studio"));
        assert!(source.contains("Create and refine Goblins OS apps"));
        assert!(source.contains("Ask Goblin or describe what you need"));
        assert!(source.contains("Ask Goblin"));
        assert!(source.contains("Write with Goblin"));

        let old_phrase = ["action registry", "relay"].join(" and ");
        assert!(!source.contains(&old_phrase));
        let old_builder_label = ["Built with", "Codex"].join(" ");
        assert!(!source.contains(&old_builder_label));
        let old_studio_label = ["multi-turn", "agent surface"].join(" ");
        assert!(!source.contains(&old_studio_label));
    }

    #[test]
    fn context_metadata_is_bounded_and_single_line() {
        assert_eq!(
            bounded_context_value("  Passwords\nWindow\tTitle  ", 64).as_deref(),
            Some("Passwords Window Title")
        );
        assert_eq!(bounded_context_value("\n\t ", 64), None);
        assert_eq!(
            bounded_context_value("abcdefgh", 4).as_deref(),
            Some("abcd")
        );
    }
}
