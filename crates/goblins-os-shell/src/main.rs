use std::{
    env,
    error::Error,
    fmt,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

// Rc/RefCell hold the Build Studio's active session id, shared between the sidebar
// and the composer so opening a saved build continues it. Native desktop only.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use std::{cell::RefCell, rc::Rc};

const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";
const DEFAULT_CORE_WAIT_SECS: u64 = 45;
const MAX_CORE_BODY_BYTES: usize = 1024 * 1024;

type ShellResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone)]
struct ShellConfig {
    core_url: String,
    core_wait: Duration,
}

#[derive(Debug, PartialEq, Eq)]
struct HttpEndpoint {
    host: String,
    port: u16,
}

#[derive(Debug, PartialEq, Eq)]
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Clone, Copy)]
struct BootState {
    core_ready: bool,
    installer_state: &'static str,
}

#[derive(Clone)]
struct ShellState {
    boot: BootState,
    auth: Option<AuthStatus>,
    session_gate: Option<SessionGateStatus>,
    installer: Option<InstallerReadiness>,
    services: Vec<ServiceCatalogEntry>,
    local_models: Option<LocalModelCatalog>,
    resident: Option<ResidentStatus>,
    apps: Vec<BuiltApp>,
    voice: Option<VoiceStatus>,
    engine: Option<EngineStatus>,
    codex: Option<CodexStatus>,
}

/// The active engine + key state, used by the Studio's engine switcher.
#[derive(Clone, Deserialize)]
struct EngineStatus {
    engine: String,
    configured: bool,
}

/// Codex CLI presence + sign-in, used to enable the Codex engine in the switcher.
#[derive(Clone, Deserialize)]
struct CodexStatus {
    installed: bool,
    authenticated: bool,
}

/// Local-voice capability from the OS core. The home only mirrors it: voice is
/// offered when ready and greyed (with a reason) until the local models are added.
#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct VoiceStatus {
    available: bool,
    detail: String,
    #[serde(default = "default_voice_wake_word")]
    wake_word: String,
    #[serde(default)]
    wake_phrases: Vec<String>,
    #[serde(default)]
    wake_listening: Option<VoiceCapability>,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct VoiceCapability {
    ready: bool,
    detail: String,
}

impl VoiceStatus {
    fn wake_word(&self) -> &str {
        let word = self.wake_word.trim();
        if word.is_empty() {
            "Goblin"
        } else {
            word
        }
    }

    fn wake_phrases(&self) -> String {
        if self.wake_phrases.is_empty() {
            "Goblin".to_string()
        } else {
            self.wake_phrases.join(", ")
        }
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    fn wake_tooltip(&self) -> String {
        let listening = self
            .wake_listening
            .as_ref()
            .map(|listener| {
                if listener.ready {
                    "Background wake listening is ready.".to_string()
                } else {
                    listener.detail.clone()
                }
            })
            .unwrap_or_else(|| format!("Press the voice button, then say {}.", self.wake_word()));
        format!(
            "Wake phrases: {}. {} {}",
            self.wake_phrases(),
            self.detail,
            listening
        )
    }
}

fn default_voice_wake_word() -> String {
    "Goblin".to_string()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn voice_wake_word(status: Option<&VoiceStatus>) -> String {
    status
        .map(|status| status.wake_word().to_string())
        .unwrap_or_else(default_voice_wake_word)
}

/// An app the user built from intent — the OS has no pre-installed apps, so this
/// is the home's content: the things the on-device model designed and the OS kept.
#[derive(Clone, Deserialize)]
struct BuiltApp {
    name: String,
    intent: String,
    #[serde(default)]
    plan: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    created_at: String,
}

#[derive(Deserialize)]
struct AppList {
    apps: Vec<BuiltApp>,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Deserialize)]
struct BuildOutcome {
    ok: bool,
    text: String,
    app: Option<BuiltApp>,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Deserialize)]
struct ConverseOutcome {
    ok: bool,
    transcript: String,
    reply: String,
    text: String,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Deserialize)]
struct StudioMessage {
    role: String,
    text: String,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Deserialize)]
struct StudioSessionView {
    id: String,
    name: String,
    thread: Vec<StudioMessage>,
    files: Vec<String>,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Deserialize)]
struct StudioTurnView {
    ok: bool,
    text: String,
    session: Option<StudioSessionView>,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Deserialize)]
struct StudioSessionList {
    sessions: Vec<StudioSummary>,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Deserialize)]
struct StudioSummary {
    id: String,
    name: String,
}

#[derive(Clone, Deserialize)]
struct AuthStatus {
    configured: bool,
    authenticated: bool,
    provider: String,
    session_storage: String,
    message: String,
}

#[derive(Clone, Deserialize)]
struct SessionGateStatus {
    generated_at: String,
    source: String,
    unlocked: bool,
    mode: Option<String>,
    first_boot_mode: Option<String>,
    lock: SessionLock,
}

#[derive(Clone, Deserialize)]
struct SessionLock {
    state: String,
    reason: String,
    openai_account_required: bool,
    local_mode_available: bool,
    state_path: String,
}

#[derive(Clone, Deserialize)]
struct InstallerReadiness {
    source: String,
    stages: Vec<InstallerStage>,
    privacy_note: String,
    storage_note: String,
}

#[derive(Clone, Deserialize)]
struct InstallerStage {
    label: String,
    state: String,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct ServiceCatalog {
    services: Vec<ServiceCatalogEntry>,
}

#[derive(Clone, Deserialize)]
struct ServiceCatalogEntry {
    id: String,
    name: String,
    role: String,
    launch: String,
    status: String,
}

#[derive(Clone, Deserialize)]
struct LocalModelCatalog {
    install_policy: String,
    hardware: HardwareReport,
    models: Vec<LocalModelOption>,
}

#[derive(Clone, Deserialize)]
struct HardwareReport {
    ram_gb: u64,
    gpu_vram_gb: Option<u64>,
    runtime: RuntimeReport,
}

#[derive(Clone, Deserialize)]
struct RuntimeReport {
    selected: Option<String>,
    ollama: bool,
    vllm: bool,
    lm_studio: bool,
}

#[derive(Clone, Deserialize)]
struct LocalModelOption {
    name: String,
    source: String,
    state: String,
    reasons: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct ResidentStatus {
    process: ResidentProcess,
    engine: ResidentEngine,
    capabilities: Vec<ResidentCapability>,
}

#[derive(Clone, Deserialize)]
struct ResidentProcess {
    state: String,
    mode: String,
    heartbeat_age_secs: Option<u64>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct ResidentEngine {
    selected: String,
    cloud_relay_configured: bool,
    local_relay_configured: bool,
}

#[derive(Clone, Deserialize)]
struct ResidentCapability {
    label: String,
    state: String,
    detail: String,
}

#[derive(Debug, PartialEq, Eq)]
enum CoreFetchError {
    Status(u16),
    Malformed,
    Transport,
    Decode,
}

impl fmt::Display for CoreFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) => write!(formatter, "core returned HTTP {status}"),
            Self::Malformed => formatter.write_str("core response was malformed"),
            Self::Transport => formatter.write_str("core connection failed"),
            Self::Decode => formatter.write_str("core response JSON did not match the OS contract"),
        }
    }
}

/// A standalone window the launcher (⌘-Space) can open without disturbing the
/// running session shell: a single built app's detail, or the Build Studio.
enum StandaloneTarget {
    Studio,
    App(String),
}

/// Parse the launcher's deep-link from argv (or the env fallback the launcher can
/// also set): `--studio` or `--open-app <name>`.
fn standalone_target() -> Option<StandaloneTarget> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--studio" => return Some(StandaloneTarget::Studio),
            "--open-app" => return args.next().map(StandaloneTarget::App),
            _ => {}
        }
    }
    match env::var("GOBLINS_OS_SHELL_OPEN_APP") {
        Ok(name) if !name.is_empty() => Some(StandaloneTarget::App(name)),
        _ => None,
    }
}

fn main() -> ShellResult<()> {
    let config = ShellConfig::from_env();

    // Launcher deep-link: open a built app or the Build Studio in its own window
    // under a distinct application id, so it never collides with — or re-presents —
    // the always-running session shell.
    if let Some(target) = standalone_target() {
        return run_standalone(config, target);
    }

    let boot_state = inspect_boot_state(&config);
    let shell_state = load_shell_state(&config, boot_state);

    println!("Goblins OS native shell session started");
    println!("core={}", config.core_url);
    println!("app_model=codex-builds-apps");
    println!("session_owner=rust");
    println!("shell_mode=native-desktop");
    println!(
        "core_state={}",
        if boot_state.core_ready {
            "ready"
        } else {
            "waiting"
        }
    );
    println!("installer_state={}", boot_state.installer_state);
    println!("{}", shell_state_summary(&shell_state));

    run_native_shell(config, shell_state)
}

impl ShellConfig {
    fn from_env() -> Self {
        Self {
            core_url: env::var("GOBLINS_OS_CORE_URL")
                .or_else(|_| env::var("OPENAI_OS_CORE_URL"))
                .unwrap_or_else(|_| DEFAULT_CORE_URL.into()),
            core_wait: Duration::from_secs(env_u64(
                "GOBLINS_OS_SHELL_CORE_WAIT_SECS",
                DEFAULT_CORE_WAIT_SECS,
            )),
        }
    }
}

fn inspect_boot_state(config: &ShellConfig) -> BootState {
    let core_ready = wait_for_core(&config.core_url, config.core_wait);
    let installer_state = if core_ready {
        status_label(http_status(&config.core_url, "/v1/installer/readiness"))
    } else {
        "unreachable"
    };

    BootState {
        core_ready,
        installer_state,
    }
}

fn load_shell_state(config: &ShellConfig, boot: BootState) -> ShellState {
    if !boot.core_ready {
        return ShellState {
            boot,
            auth: None,
            session_gate: None,
            installer: None,
            services: Vec::new(),
            local_models: None,
            resident: None,
            apps: Vec::new(),
            voice: None,
            engine: None,
            codex: None,
        };
    }

    let auth = get_core_json::<AuthStatus>(&config.core_url, "/v1/auth/openai/status").ok();
    let session_gate =
        get_core_json::<SessionGateStatus>(&config.core_url, "/v1/session/gate").ok();
    let installer =
        get_core_json::<InstallerReadiness>(&config.core_url, "/v1/installer/readiness").ok();
    let services = get_core_json::<ServiceCatalog>(&config.core_url, "/v1/services")
        .map(|catalog| catalog.services)
        .unwrap_or_default();
    let local_models =
        get_core_json::<LocalModelCatalog>(&config.core_url, "/v1/local-models").ok();
    let resident = get_core_json::<ResidentStatus>(&config.core_url, "/v1/ai/runtime/status").ok();
    let apps = get_core_json::<AppList>(&config.core_url, "/v1/apps")
        .map(|list| list.apps)
        .unwrap_or_default();
    let voice = get_core_json::<VoiceStatus>(&config.core_url, "/v1/voice/status").ok();
    let engine = get_core_json::<EngineStatus>(&config.core_url, "/v1/models/openai-key").ok();
    let codex = get_core_json::<CodexStatus>(&config.core_url, "/v1/codex/status").ok();

    ShellState {
        boot,
        auth,
        session_gate,
        installer,
        services,
        local_models,
        resident,
        apps,
        voice,
        engine,
        codex,
    }
}

fn shell_state_summary(shell_state: &ShellState) -> String {
    let auth = shell_state
        .auth
        .as_ref()
        .map(|auth| {
            format!(
                "{}:{}:{}:{}",
                auth.provider,
                if auth.authenticated {
                    "authenticated"
                } else if auth.configured {
                    "provider-ready"
                } else {
                    "locked"
                },
                auth.session_storage,
                auth.message
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    let installer = shell_state
        .installer
        .as_ref()
        .map(|installer| {
            let first_stage = installer
                .stages
                .first()
                .map(|stage| format!("{}:{}:{}", stage.label, stage.state, stage.detail))
                .unwrap_or_else(|| "none".to_string());

            format!(
                "{}:{} stages privacy={} storage={}",
                installer.source, first_stage, installer.privacy_note, installer.storage_note
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let session_gate = shell_state
        .session_gate
        .as_ref()
        .map(|gate| {
            format!(
                "{}:{} unlocked={} mode={} first_boot={} lock={}:{}:{}:{}:{}",
                gate.source,
                gate.generated_at,
                gate.unlocked,
                gate.mode.as_deref().unwrap_or("none"),
                gate.first_boot_mode.as_deref().unwrap_or("none"),
                gate.lock.state,
                gate.lock.reason,
                gate.lock.openai_account_required,
                gate.lock.local_mode_available,
                gate.lock.state_path
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    let first_service = shell_state
        .services
        .first()
        .map(|service| {
            format!(
                "{}:{}:{}:{}:{}",
                service.id, service.name, service.role, service.launch, service.status
            )
        })
        .unwrap_or_else(|| "none".to_string());

    let local_models = shell_state
        .local_models
        .as_ref()
        .map(|catalog| {
            let model = catalog
                .models
                .first()
                .map(|model| {
                    format!(
                        "{}:{}:{}:{}",
                        model.name,
                        model.source,
                        model.state,
                        model.reasons.first().map(String::as_str).unwrap_or("ready")
                    )
                })
                .unwrap_or_else(|| "none".to_string());

            format!(
                "{} hardware={}GB vram={} runtime={} model={}",
                catalog.install_policy,
                catalog.hardware.ram_gb,
                catalog
                    .hardware
                    .gpu_vram_gb
                    .map(|vram| format!("{vram}GB"))
                    .unwrap_or_else(|| "not-detected".to_string()),
                runtime_label(&catalog.hardware.runtime),
                model
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let resident = shell_state
        .resident
        .as_ref()
        .map(|resident| {
            let first_capability = resident
                .capabilities
                .first()
                .map(|capability| {
                    format!(
                        "{}:{}:{}",
                        capability.label, capability.state, capability.detail
                    )
                })
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{}:{}:{}:{}:{}:{}:{} capabilities={} first_capability=[{}]",
                resident.process.state,
                resident.process.mode,
                resident.engine.selected,
                resident.engine.cloud_relay_configured,
                resident.engine.local_relay_configured,
                resident
                    .process
                    .heartbeat_age_secs
                    .map(|age| format!("{age}s"))
                    .unwrap_or_else(|| "waiting".to_string()),
                resident.process.detail,
                resident.capabilities.len(),
                first_capability
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    let first_app = shell_state
        .apps
        .first()
        .map(|app| {
            format!(
                "{}:{}:{}:source={}:plan_chars={}",
                app.name,
                app.intent,
                app.created_at,
                app.source,
                app.plan.len()
            )
        })
        .unwrap_or_else(|| "none".to_string());

    let voice = shell_state
        .voice
        .as_ref()
        .map(|voice| {
            format!(
                "available={} wake_word={} phrases={} detail={}",
                voice.available,
                voice.wake_word(),
                voice.wake_phrases(),
                voice.detail
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    let engine = shell_state
        .engine
        .as_ref()
        .map(|engine| format!("{}:key-configured={}", engine.engine, engine.configured))
        .unwrap_or_else(|| "unavailable".to_string());
    let codex = shell_state
        .codex
        .as_ref()
        .map(|codex| {
            format!(
                "installed={}:authed={}",
                codex.installed, codex.authenticated
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    format!(
        "os_state=core:{} installer_boot:{} auth=[{}] session_gate=[{}] installer=[{}] services={} first_service=[{}] local_models=[{}] resident=[{}] apps={} first_app=[{}] voice=[{}] engine=[{}] codex=[{}]",
        if shell_state.boot.core_ready {
            "ready"
        } else {
            "waiting"
        },
        shell_state.boot.installer_state,
        auth,
        session_gate,
        installer,
        shell_state.services.len(),
        first_service,
        local_models,
        resident,
        shell_state.apps.len(),
        first_app,
        voice,
        engine,
        codex
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_native_shell(config: ShellConfig, shell_state: ShellState) -> ShellResult<()> {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.Shell")
        .build();

    application.connect_activate(move |app| {
        goblins_os_ui::init_theming(GOBLINS_OS_CSS);
        let state = Rc::new(RefCell::new(shell_state.clone()));

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Goblins OS")
            .decorated(false)
            .build();

        window.set_child(Some(&build_desktop(
            &config,
            &state.borrow(),
            Some(&window),
        )));
        // A real OS, not a screen takeover: once the session is unlocked the home
        // is a calm window ON the desktop — the wallpaper, menu bar, and dock show
        // around it, and built apps and utilities open as their own windows. Only
        // the first-boot lock / identity gate legitimately owns the whole screen.
        if session_is_unlocked(&state.borrow()) {
            window.add_css_class("gos-windowed");
            window.set_default_size(940, 700);
        } else {
            window.set_default_size(1440, 900);
            window.fullscreen();
        }
        // Capture affordance (off by default, like the other GOBLINS_OS_RENDER_*
        // envs): the hardware-gate harness maximizes each surface so a framebuffer
        // screendump captures it filling the work area (keeping window chrome + the
        // menu bar/dock) — solving the foreground/z-order ambiguity of windowed
        // capture without any compositor/session change.
        if std::env::var_os("GOBLINS_OS_RENDER_FULLSCREEN").is_some() {
            window.maximize();
        }
        window.present();

        if !session_is_unlocked(&state.borrow()) {
            let refresh_config = config.clone();
            let refresh_state = state.clone();
            let window = window.downgrade();
            gtk::glib::timeout_add_local(Duration::from_secs(2), move || {
                let Some(window) = window.upgrade() else {
                    return gtk::glib::ControlFlow::Break;
                };
                if session_is_unlocked(&refresh_state.borrow()) {
                    return gtk::glib::ControlFlow::Break;
                }

                let boot_state = inspect_boot_state(&refresh_config);
                let next = load_shell_state(&refresh_config, boot_state);
                if session_gate_signature(&refresh_state.borrow()) == session_gate_signature(&next)
                {
                    return gtk::glib::ControlFlow::Continue;
                }

                let next_unlocked = session_is_unlocked(&next);
                let view = build_desktop(&refresh_config, &next, Some(&window));
                *refresh_state.borrow_mut() = next;
                window.set_child(Some(&view));

                if next_unlocked {
                    // Leaving the lock gate: drop fullscreen and settle into a
                    // rounded, shadowed window on the desktop.
                    window.unfullscreen();
                    window.add_css_class("gos-windowed");
                    window.set_default_size(940, 700);
                    gtk::glib::ControlFlow::Break
                } else {
                    gtk::glib::ControlFlow::Continue
                }
            });
        }
    });

    application.run();
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_desktop(
    config: &ShellConfig,
    shell_state: &ShellState,
    window: Option<&gtk4::ApplicationWindow>,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-root");

    // A whisper-quiet window header — just the brand. The desktop menu bar and the
    // dock are the OS chrome now; Settings lives in the dock, not in every window.
    let top_bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    top_bar.add_css_class("gos-top-bar");
    if let Some(window) = window {
        top_bar.append(&goblins_os_ui::window_controls(window));
    }
    top_bar.append(&goblins_os_ui::themed_brand_mark(22));
    let brand = label("Goblins OS", &["gos-brand"]);
    brand.set_wrap(false);
    top_bar.append(&brand);
    top_bar.append(&spacer());
    root.append(&top_bar);

    if !session_is_unlocked(shell_state) {
        let lock = build_session_locked_panel(config, shell_state);
        lock.set_size_request(660, -1);
        lock.set_halign(gtk::Align::Center);

        // Center the lock hero in the viewport for a calm, intentional first view.
        let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
        center.set_vexpand(true);
        center.set_valign(gtk::Align::Center);
        center.append(&lock);
        root.append(&center);
        return root;
    }

    // The home (a calm command field) and the Build Studio (the full agent
    // surface) share the unlocked desktop through a crossfade stack. The home is
    // the entry; the Studio is where you switch engines and build across turns.
    let body = gtk::Stack::new();
    body.set_transition_type(gtk::StackTransitionType::Crossfade);
    body.set_transition_duration(200);
    body.set_vexpand(true);

    // The app-detail page: opening a built app shows its identity, the intent the
    // user gave, the plan the on-device model designed, and a route into the Build
    // Studio where the app lives and runs.
    let app_detail = gtk::Box::new(gtk::Orientation::Vertical, 0);
    app_detail.set_vexpand(true);

    let home = build_home(config, shell_state, &body, &app_detail);
    home.set_vexpand(true);
    let studio = build_studio(config, shell_state, &body);
    studio.set_vexpand(true);
    body.add_named(&home, Some("home"));
    body.add_named(&studio, Some("studio"));
    body.add_named(&app_detail, Some("app-detail"));

    // Open on the home by default. Launcher deep-links use the standalone window
    // path below so every non-home surface is backed by real core state.
    body.set_visible_child_name("home");
    root.append(&body);

    root
}

/// The Command-Space home: a single centered build field on calm paper. The user
/// describes an app, the Goblins AI runtime designs it, and the things they
/// build accrue beneath as a quiet ledger. This IS the OS — there is no app grid.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_home(
    config: &ShellConfig,
    shell_state: &ShellState,
    stack: &gtk4::Stack,
    detail: &gtk4::Box,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_vexpand(true);
    center.set_valign(gtk::Align::Center);
    center.set_halign(gtk::Align::Center);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_size_request(620, -1);
    column.set_halign(gtk::Align::Center);

    let kicker = centered("BUILD", &["gos-home-kicker"], false);
    column.append(&kicker);
    column.append(&centered(
        "What do you want to make?",
        &["gos-home-headline"],
        false,
    ));
    column.append(&centered(
        "Describe an app in a sentence. The on-device model designs it, and Goblins OS keeps it — nothing else comes pre-installed.",
        &["gos-home-sub"],
        true,
    ));

    let field = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    field.add_css_class("gos-home-field");
    let entry = gtk::Entry::new();
    entry.add_css_class("gos-home-entry");
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some("Describe an app — a quiet focus-session timer"));
    let build = button("Build", &["gos-home-build"]);
    field.append(&entry);
    field.append(&build);
    column.append(&field);
    // The working animation — a calm three-dot "thinking" pulse shown only while
    // the on-device model is designing the app. Hidden (and so paused) at rest.
    // Dots and status share one fixed-height slot so the hero never shifts when
    // the working state toggles.
    let status_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    status_slot.add_css_class("gos-home-status-slot");
    let thinking = thinking_dots();
    thinking.set_visible(false);
    thinking.set_margin_top(16);
    status_slot.append(&thinking);

    let status = centered(
        "On-device GPT-OSS — your apps stay on this machine.",
        &["gos-home-status"],
        true,
    );
    status_slot.append(&status);
    column.append(&status_slot);

    // Voice: ask Goblin and hear it answer, all on-device. Offered when the local
    // Whisper/Piper models are present; greyed (with a reason) until then.
    let voice_available = shell_state
        .voice
        .as_ref()
        .is_some_and(|voice| voice.available);
    let voice_word = voice_wake_word(shell_state.voice.as_ref());
    let voice = button(&format!("Say {voice_word}"), &["gos-home-voice"]);
    voice.set_halign(gtk::Align::Center);
    voice.set_margin_top(14);
    voice.set_sensitive(voice_available);
    let voice_tooltip = shell_state
        .voice
        .as_ref()
        .map(VoiceStatus::wake_tooltip)
        .unwrap_or_else(|| {
            "Goblin voice runs on local Whisper and Piper models. Add the missing voice components."
                .to_string()
        });
    voice.set_tooltip_text(Some(&voice_tooltip));
    column.append(&voice);

    // A quiet route into the full Build Studio — the multi-turn agent surface
    // where you switch engines (GPT-OSS · Codex · your key) and build across turns.
    let open_studio = button("Open Build Studio", &["gos-home-studio-link"]);
    open_studio.set_halign(gtk::Align::Center);
    open_studio.set_margin_top(10);
    {
        let stack = stack.clone();
        open_studio.connect_clicked(move |_| stack.set_visible_child_name("studio"));
    }
    column.append(&open_studio);

    // A quiet route to Settings (engine, OpenAI account, network, privacy). The
    // dock also opens it; the home keeps a calm text link so the window is
    // self-sufficient without reaching for the dock.
    let open_settings = button("Settings", &["gos-home-studio-link"]);
    open_settings.set_halign(gtk::Align::Center);
    open_settings.set_margin_top(6);
    open_settings.connect_clicked(move |_| {
        let _ = launch_local_action("settings");
    });
    column.append(&open_settings);

    // The built-apps ledger. The kicker and empty line are persistent so a build
    // can reveal the first app in place without rebuilding the home.
    let ledger_kicker = label("YOUR APPS", &["gos-home-ledger-kicker"]);
    let ledger = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let empty = centered(
        "Nothing installed, nothing assumed. The apps you build live here.",
        &["gos-home-empty"],
        true,
    );
    let apps = shell_state.apps.clone();
    if apps.is_empty() {
        ledger_kicker.set_visible(false);
        column.append(&empty);
    } else {
        empty.set_visible(false);
        for app in apps.iter() {
            ledger.append(&build_app_row(app, stack, detail));
        }
    }
    column.append(&ledger_kicker);
    // Wrap the ledger so an unbounded number of built apps stays reachable by
    // scrolling instead of truncating. The inner `ledger` Box is unchanged, so
    // `finish_build` can still prepend a freshly built row in place.
    let ledger_scroll = gtk::ScrolledWindow::new();
    ledger_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    ledger_scroll.set_max_content_height(360);
    ledger_scroll.set_propagate_natural_height(true);
    ledger_scroll.set_child(Some(&ledger));
    column.append(&ledger_scroll);

    let ui = BuildUi {
        core_url: config.core_url.clone(),
        entry: entry.clone(),
        build: build.clone(),
        status,
        thinking,
        ledger,
        ledger_kicker,
        empty,
        stack: stack.clone(),
        detail: detail.clone(),
    };
    {
        let ui = ui.clone();
        build.connect_clicked(move |_| start_build(&ui));
    }
    {
        let ui = ui.clone();
        entry.connect_activate(move |_| start_build(&ui));
    }
    {
        let ui = ui.clone();
        let voice_button = voice.clone();
        let voice_word = voice_word.clone();
        voice.connect_clicked(move |_| start_voice(&ui, &voice_button, &voice_word));
    }

    center.append(&column);
    center
}

/// Handles to the home's live widgets, bundled so the async build flow can move
/// the UI between rest, working, and result states from a single place.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone)]
struct BuildUi {
    core_url: String,
    entry: gtk4::Entry,
    build: gtk4::Button,
    status: gtk4::Label,
    thinking: gtk4::Box,
    ledger: gtk4::Box,
    ledger_kicker: gtk4::Label,
    empty: gtk4::Label,
    // The desktop body stack + the app-detail page, so a freshly-built app's row
    // opens its detail view the same way the pre-built rows do.
    stack: gtk4::Stack,
    detail: gtk4::Box,
}

/// A calm three-dot thinking pulse. Each dot breathes on the frame clock with a
/// staggered phase — the OpenAI-style "working" cadence, rendered in monochrome.
/// Tick callbacks fire only while the widget is mapped, so hiding it pauses it.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn thinking_dots() -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    row.add_css_class("gos-thinking");
    row.set_halign(gtk::Align::Center);
    // Honor the desktop reduced-motion preference (GNOME maps it onto
    // gtk-enable-animations). Raw frame-clock tick callbacks are NOT paused by
    // that setting the way built-in widget animations are, so we check it
    // ourselves: when motion is reduced, show a calm STATIC three-dot indicator
    // instead of the breathing pulse.
    let animations_enabled = gtk::Settings::default()
        .map(|settings| settings.is_gtk_enable_animations())
        .unwrap_or(true);
    for index in 0..3 {
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("gos-thinking-dot");
        dot.set_size_request(9, 9);
        if animations_enabled {
            let offset = f64::from(index) * (1.1 / 3.0);
            let _animation = dot.add_tick_callback(move |dot, clock| {
                let seconds = clock.frame_time() as f64 / 1_000_000.0;
                let omega = std::f64::consts::TAU / 1.1;
                let phase = (((seconds + offset) * omega).sin() * 0.5) + 0.5;
                dot.set_opacity(0.28 + 0.72 * phase);
                gtk::glib::ControlFlow::Continue
            });
        } else {
            // Steady, legible "working" indicator — no breathing under reduced motion.
            dot.set_opacity(0.72);
        }
        row.append(&dot);
    }
    row
}

/// Begin an app build without freezing the OS: the model call runs on a worker
/// thread while the home shows the thinking pulse, and the result is applied back
/// on the main loop. This is what makes the working animation real, not decorative.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn start_build(ui: &BuildUi) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let intent = ui.entry.text().to_string();
    let intent = intent.trim().to_string();
    ui.status.remove_css_class("gos-home-status-working");
    ui.status.remove_css_class("gos-home-status-error");
    if intent.is_empty() {
        ui.status.add_css_class("gos-home-status-error");
        ui.status.set_text("Describe the app you want first.");
        return;
    }

    ui.build.set_sensitive(false);
    ui.entry.set_sensitive(false);
    ui.status.add_css_class("gos-home-status-working");
    ui.status.set_text("Designing your app…");
    ui.thinking.set_visible(true);

    let (tx, rx) = std::sync::mpsc::channel::<Result<BuiltApp, String>>();
    let core_url = ui.core_url.clone();
    std::thread::spawn(move || {
        let _ = tx.send(submit_build(&core_url, &intent));
    });

    let ui = ui.clone();
    let _poll =
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(90), move || {
            match rx.try_recv() {
                Ok(result) => {
                    finish_build(&ui, result);
                    gtk::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finish_build(
                        &ui,
                        Err("The build worker stopped unexpectedly.".to_string()),
                    );
                    gtk::glib::ControlFlow::Break
                }
            }
        });
}

/// Apply a finished build to the home: stop the pulse, restore the field, and
/// either reveal the freshly built app at the top of the ledger or show the error.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn finish_build(ui: &BuildUi, result: Result<BuiltApp, String>) {
    use gtk4::prelude::*;

    ui.thinking.set_visible(false);
    ui.build.set_sensitive(true);
    ui.entry.set_sensitive(true);
    ui.status.remove_css_class("gos-home-status-working");

    match result {
        Ok(app) => {
            ui.entry.set_text("");
            ui.status.remove_css_class("gos-home-status-error");
            ui.status.set_text(&format!("Built {}.", app.name));
            ui.empty.set_visible(false);
            ui.ledger_kicker.set_visible(true);
            ui.ledger
                .prepend(&build_app_row(&app, &ui.stack, &ui.detail));
        }
        Err(detail) => {
            ui.status.add_css_class("gos-home-status-error");
            ui.status.set_text(&detail);
        }
    }
}

/// Ask Goblin: capture, transcribe, answer, and speak — entirely on-device —
/// on a worker thread so the home shows the same calm pulse while it listens and
/// thinks. The reply is both spoken (by the core) and shown here.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn start_voice(ui: &BuildUi, voice: &gtk4::Button, wake_word: &str) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    ui.status.remove_css_class("gos-home-status-error");
    ui.status.add_css_class("gos-home-status-working");
    ui.status.set_text(&format!("Listening for {wake_word}…"));
    ui.thinking.set_visible(true);
    ui.build.set_sensitive(false);
    voice.set_sensitive(false);

    let (tx, rx) = std::sync::mpsc::channel::<Result<(String, String), String>>();
    let core_url = ui.core_url.clone();
    std::thread::spawn(move || {
        let _ = tx.send(converse(&core_url));
    });

    let ui = ui.clone();
    let voice = voice.clone();
    let _poll =
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(90), move || {
            match rx.try_recv() {
                Ok(result) => {
                    finish_voice(&ui, &voice, result);
                    gtk::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finish_voice(&ui, &voice, Err("Voice stopped unexpectedly.".to_string()));
                    gtk::glib::ControlFlow::Break
                }
            }
        });
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn finish_voice(ui: &BuildUi, voice: &gtk4::Button, result: Result<(String, String), String>) {
    use gtk4::prelude::*;

    ui.thinking.set_visible(false);
    ui.build.set_sensitive(true);
    voice.set_sensitive(true);
    ui.status.remove_css_class("gos-home-status-working");

    match result {
        Ok((_transcript, reply)) => {
            ui.status.remove_css_class("gos-home-status-error");
            ui.status.set_text(&truncate_intent(&reply));
        }
        Err(detail) => {
            ui.status.add_css_class("gos-home-status-error");
            ui.status.set_text(&detail);
        }
    }
}

/// Drive one on-device voice turn through the core and return (heard, spoken).
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn converse(core_url: &str) -> Result<(String, String), String> {
    let response = http_post_response(core_url, "/v1/voice/converse", "{}")
        .map_err(|_| "Goblins OS could not reach the on-device voice service.".to_string())?;
    let outcome: ConverseOutcome = serde_json::from_slice(&response.body)
        .map_err(|_| "Goblins OS could not read the voice result.".to_string())?;
    if (200..=299).contains(&response.status) && outcome.ok {
        Ok((outcome.transcript, outcome.reply))
    } else {
        Err(outcome.text)
    }
}

/// A centered label for the home hero — overrides the default left alignment so
/// single lines and wrapped copy both read centered under the field.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn centered(text: &str, classes: &[&str], wrap: bool) -> gtk4::Label {
    use gtk4::prelude::*;

    let label = label(text, classes);
    label.set_halign(gtk4::Align::Center);
    label.set_justify(gtk4::Justification::Center);
    label.set_xalign(0.5);
    label.set_wrap(wrap);
    if wrap {
        label.set_max_width_chars(52);
    }
    label
}

/// One row in the built-apps ledger: name + truncated intent on the left, a
/// human "Built …" time on the right. Clicking opens the app's detail view.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_app_row(app: &BuiltApp, stack: &gtk4::Stack, detail: &gtk4::Box) -> gtk4::Button {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let row = gtk::Button::new();
    row.add_css_class("gos-home-app-row");
    row.set_hexpand(true);

    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&label(&app.name, &["gos-home-app-name"]));
    text.append(&label(
        &truncate_intent(&app.intent),
        &["gos-home-app-meta"],
    ));
    line.append(&text);
    line.append(&label(
        &relative_time(&app.created_at),
        &["gos-home-app-time"],
    ));
    row.set_child(Some(&line));

    // Open this app's detail view: identity, the intent the user gave, the plan
    // the on-device model designed, and a route into the Build Studio.
    {
        let stack = stack.clone();
        let detail = detail.clone();
        let app = app.clone();
        row.connect_clicked(move |_| {
            populate_app_detail(&detail, &app, &stack);
            stack.set_visible_child_name("app-detail");
        });
    }
    row
}

/// The built-app detail view: a calm, focused surface showing what the user asked
/// for, what the on-device model designed, which engine built it, and the way into
/// the Build Studio where the app lives and runs. Reuses the home's design idiom.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn populate_app_detail(page: &gtk4::Box, app: &BuiltApp, stack: &gtk4::Stack) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    while let Some(child) = page.first_child() {
        page.remove(&child);
    }
    page.add_css_class("gos-home-root");

    // Top-anchored like every other content surface — a vertically centered
    // detail would open with a dead band under the top bar.
    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_valign(gtk::Align::Start);
    center.set_margin_top(24);
    center.set_halign(gtk::Align::Center);
    center.set_vexpand(true);
    center.set_hexpand(true);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_halign(gtk::Align::Center);
    column.set_size_request(620, -1);

    let back = button("← Home", &["gos-home-settings", "gos-detail-back"]);
    back.set_halign(gtk::Align::Start);
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("home"));
    }
    column.append(&back);

    let title = label(&app.name, &["gos-section-title"]);
    title.set_margin_top(8);
    title.set_wrap(false);
    column.append(&title);

    // Meta: which engine built it + when. The source is shown as a calm neutral
    // badge, never an invented claim.
    let meta = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    meta.set_halign(gtk::Align::Start);
    meta.set_margin_top(10);
    meta.append(&label(&engine_label(&app.source), &["gos-studio-badge"]));
    meta.append(&label(
        &relative_time(&app.created_at),
        &["gos-home-app-time"],
    ));
    column.append(&meta);

    column.append(&{
        let l = label("WHAT YOU ASKED FOR", &["gos-home-ledger-kicker"]);
        l.set_margin_top(28);
        l
    });
    let intent = label(app.intent.trim(), &["gos-row-copy", "gos-detail-body"]);
    intent.set_wrap(true);
    intent.set_xalign(0.0);
    let intent_row = gtk::Box::new(gtk::Orientation::Vertical, 0);
    intent_row.add_css_class("gos-row");
    intent_row.append(&intent);
    column.append(&intent_row);

    if !app.plan.trim().is_empty() {
        column.append(&{
            let l = label("WHAT GOBLINS OS BUILT", &["gos-home-ledger-kicker"]);
            l.set_margin_top(20);
            l
        });
        // The model designs in Markdown; render it to Pango markup so the build
        // result reads as a finished document — never raw ** / ## on screen.
        let plan = gtk::Label::new(None);
        plan.set_markup(&markdown_to_pango(app.plan.trim()));
        plan.add_css_class("gos-row-copy");
        plan.add_css_class("gos-detail-body");
        plan.add_css_class("gos-prose");
        plan.set_wrap(true);
        plan.set_xalign(0.0);
        let plan_row = gtk::Box::new(gtk::Orientation::Vertical, 0);
        plan_row.add_css_class("gos-row");
        plan_row.append(&plan);
        column.append(&plan_row);
    }

    let open = button("Open in Build Studio", &["gos-home-build"]);
    open.set_halign(gtk::Align::Start);
    open.set_margin_top(28);
    {
        let stack = stack.clone();
        open.connect_clicked(move |_| stack.set_visible_child_name("studio"));
    }
    column.append(&open);

    center.append(&column);
    page.append(&center);
}

/// Render the model's Markdown plan to Pango markup so the app-detail surface reads
/// as a finished document, not a debug dump. Pango has only inline tags, so block
/// structure becomes blank lines + indents; all text is XML-escaped so set_markup
/// never chokes. Bold/italics/code map to <b>/<i>/<tt>; headings to bold lines;
/// ordered/unordered lists to numbered/bulleted indented rows.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn markdown_to_pango(src: &str) -> String {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    fn esc(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    let mut out = String::new();
    // Each open list pushes Some(counter) for ordered, None for bullet.
    let mut lists: Vec<Option<u64>> = Vec::new();

    for event in Parser::new(src) {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str("<b>");
            }
            Event::End(TagEnd::Heading(_)) => out.push_str("</b>\n"),
            Event::Start(Tag::Strong) => out.push_str("<b>"),
            Event::End(TagEnd::Strong) => out.push_str("</b>"),
            Event::Start(Tag::Emphasis) => out.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => out.push_str("</i>"),
            Event::Start(Tag::Paragraph) => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Event::End(TagEnd::Paragraph) => out.push('\n'),
            Event::Start(Tag::List(first)) => lists.push(first),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let depth = lists.len().saturating_sub(1);
                out.push_str(&"    ".repeat(depth));
                match lists.last_mut() {
                    Some(Some(n)) => {
                        out.push_str(&format!("{n}. "));
                        *n += 1;
                    }
                    _ => out.push_str("•  "),
                }
            }
            Event::End(TagEnd::Item) => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Event::Code(text) => {
                out.push_str("<tt>");
                out.push_str(&esc(&text));
                out.push_str("</tt>");
            }
            Event::Start(Tag::CodeBlock(_)) => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("<tt>");
            }
            Event::End(TagEnd::CodeBlock) => {
                if out.ends_with('\n') {
                    out.pop();
                }
                out.push_str("</tt>\n");
            }
            Event::Text(text) => out.push_str(&esc(&text)),
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push('\n'),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// A friendly label for the engine that built an app — never invents a value and
/// never shows a raw internal slug (an unknown source is de-slugified for display).
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn engine_label(source: &str) -> String {
    match source {
        "local-gpt-oss" | "gpt-oss" | "local" => "GPT-OSS".to_string(),
        "codex" => "Codex".to_string(),
        "openai-api" | "openai" => "Your OpenAI API key".to_string(),
        "" => "On-device".to_string(),
        other => {
            let cleaned = other.replace('-', " ");
            let mut chars = cleaned.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => cleaned,
            }
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn truncate_intent(intent: &str) -> String {
    let intent = intent.trim();
    let first_line = intent.lines().next().unwrap_or(intent);
    if first_line.chars().count() > 72 {
        let cut: String = first_line.chars().take(71).collect();
        format!("{}…", cut.trim_end())
    } else {
        first_line.to_string()
    }
}

/// The Build Studio: a dark, developer-grade agent surface. A sidebar of your
/// builds and their threads on the left; a center conversation with the agent's
/// tool calls and changed-file diffs; and a composer whose model picker is the
/// engine switch (GPT-OSS · Codex · your key). One surface, whichever brain runs.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_studio(config: &ShellConfig, shell_state: &ShellState, stack: &gtk4::Stack) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("gos-studio-root");

    // ── Sidebar: builds → threads ──
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.add_css_class("gos-studio-sidebar");

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    // The brand/title row carries the topbar's hairline so the header rule reads
    // as one continuous line across both panes of the two-pane layout.
    head.add_css_class("gos-studio-sidebar-head");
    // The Studio sidebar follows the scheme (paper in Light, graphite in Dark),
    // so its mark follows too.
    head.append(&goblins_os_ui::themed_brand_mark(18));
    head.append(&label("Build Studio", &["gos-studio-wordmark"]));
    head.append(&spacer());
    // The header badge names the engine the next build will run on — the same
    // source resolved for the composer's model picker — so it stays consistent
    // with the app-detail badge and tells the user something actionable, rather
    // than tagging the surface with its implementation language.
    let header_engine = shell_state
        .engine
        .as_ref()
        .map(|engine| engine.engine.as_str())
        .unwrap_or("local-gpt-oss");
    let engine_badge = label(&engine_label(header_engine), &["gos-studio-badge"]);
    head.append(&engine_badge);
    sidebar.append(&head);

    let search = gtk::Entry::new();
    search.add_css_class("gos-studio-search");
    search.set_placeholder_text(Some("Search builds"));
    sidebar.append(&search);

    sidebar.append(&label("Builds", &["gos-studio-section"]));
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    // Collect each clickable row with the session id it opens and a lowercase search
    // key, so the conversation/title (created below) can be wired to it and the
    // search field can filter the list live.
    let mut studio_rows: Vec<(gtk::Button, String)> = Vec::new();
    let mut search_rows: Vec<(gtk::Widget, String)> = Vec::new();
    for project in studio_projects(config, shell_state) {
        let row = sidebar_project(&project);
        search_rows.push((row.clone().upcast(), project.name.to_lowercase()));
        studio_rows.push((row.clone(), project.id.clone()));
        list.append(&row);
        for thread in &project.threads {
            let thread_row = sidebar_thread(thread);
            search_rows.push((thread_row.clone().upcast(), thread.title.to_lowercase()));
            studio_rows.push((thread_row.clone(), thread.id.clone()));
            list.append(&thread_row);
        }
    }
    // First-run ledger: with no saved builds the list would otherwise be a tall
    // blank gap. A centered, honest placeholder makes the empty state read as a
    // designed starting point rather than a broken column.
    if studio_rows.is_empty() {
        let empty = label(
            "No builds yet. Describe one below to start your first thread.",
            &["gos-studio-empty"],
        );
        empty.set_halign(gtk::Align::Center);
        empty.set_justify(gtk::Justification::Center);
        empty.set_margin_top(12);
        list.append(&empty);
    }
    let list_scroll = gtk::ScrolledWindow::new();
    list_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    list_scroll.set_child(Some(&list));
    list_scroll.set_vexpand(true);
    sidebar.append(&list_scroll);

    let new_build = button("+ New build", &["gos-studio-add"]);
    sidebar.append(&new_build);
    let home = button("← Home", &["gos-studio-home"]);
    {
        let stack = stack.clone();
        home.connect_clicked(move |_| stack.set_visible_child_name("home"));
    }
    sidebar.append(&home);
    root.append(&sidebar);

    // ── Main: topbar + conversation + composer + footer ──
    let main = gtk::Box::new(gtk::Orientation::Vertical, 0);
    main.add_css_class("gos-studio-main");
    main.set_hexpand(true);

    let session = latest_studio_session(&config.core_url);

    // The build currently open in the center, shared with the composer so a
    // follow-up turn continues it instead of forking a new session.
    let active_id = Rc::new(RefCell::new(
        session
            .as_ref()
            .map(|view| view.id.clone())
            .unwrap_or_default(),
    ));

    let topbar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    topbar.add_css_class("gos-studio-topbar");
    let title_text = session
        .as_ref()
        .map(thread_title)
        .unwrap_or_else(|| "New build".to_string());
    let title = studio_text(&title_text, "gos-studio-thread-title", true, false);
    topbar.append(&title);
    if let Some(view) = &session {
        topbar.append(&label(&view.name, &["gos-studio-crumb"]));
    }
    topbar.append(&spacer());
    main.append(&topbar);

    // Conversation
    let conv = gtk::Box::new(gtk::Orientation::Vertical, 16);
    conv.add_css_class("gos-studio-conv");
    if let Some(view) = &session {
        rebuild_conversation(&conv, view);
    } else {
        conv.append(&label(
            "Describe what you want to build. The agent answers, runs its tools, and the changed files appear here — one thread per build.",
            &["gos-studio-empty"],
        ));
    }
    let conv_scroll = gtk::ScrolledWindow::new();
    conv_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    conv_scroll.set_child(Some(&conv));
    conv_scroll.set_vexpand(true);
    conv_scroll.add_css_class("gos-studio-conv-scroll");
    main.append(&conv_scroll);

    // Composer — the build input with the live engine switch (GPT-OSS / Codex / your key).
    let composer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    composer.add_css_class("gos-studio-composer");
    let input = gtk::Entry::new();
    input.add_css_class("gos-studio-input");
    input.set_placeholder_text(Some("Ask anything, or describe what to build…"));
    composer.append(&input);

    let active_engine = shell_state
        .engine
        .as_ref()
        .map(|engine| engine.engine.clone())
        .unwrap_or_else(|| "local-gpt-oss".to_string());
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    controls.add_css_class("gos-studio-controls");
    controls.append(&engine_pill(config, &active_engine, &engine_badge));
    controls.append(&spacer());
    let thinking = thinking_dots();
    thinking.set_visible(false);
    thinking.set_valign(gtk::Align::Center);
    controls.append(&thinking);
    let send = button("↑", &["gos-studio-send"]);
    controls.append(&send);
    composer.append(&controls);
    main.append(&composer);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("gos-studio-footer");
    footer.append(&label("Local checkout", &["gos-studio-crumb"]));
    footer.append(&spacer());
    footer.append(&label("main", &["gos-studio-crumb"]));
    main.append(&footer);

    root.append(&main);

    // Wire the sidebar now that the conversation and title exist: clicking a row
    // opens that saved session; "+ New build" clears to the empty composer state;
    // the search field filters the list live.
    for (row, id) in &studio_rows {
        wire_studio_open(row, &config.core_url, id, &conv, &title, &active_id);
    }
    {
        let conv = conv.clone();
        let input = input.clone();
        let title = title.clone();
        let active_id = active_id.clone();
        new_build
            .connect_clicked(move |_| reset_studio_to_new_build(&conv, &input, &title, &active_id));
    }
    {
        let search_rows = search_rows;
        search.connect_changed(move |entry| {
            let query = entry.text().to_string().to_lowercase();
            let query = query.trim();
            for (row, key) in &search_rows {
                row.set_visible(query.is_empty() || key.contains(query));
            }
        });
    }

    let ui = StudioUi {
        core_url: config.core_url.clone(),
        input: input.clone(),
        send: send.clone(),
        thinking,
        conv,
        title,
        app_id: active_id,
    };
    {
        let ui = ui.clone();
        send.connect_clicked(move |_| start_studio_turn(&ui));
    }
    {
        let ui = ui.clone();
        input.connect_activate(move |_| start_studio_turn(&ui));
    }

    root
}

/// Live widgets of the Studio, bundled so an async build turn can move the surface
/// between rest, working, and result states from one place.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone)]
struct StudioUi {
    core_url: String,
    input: gtk4::Entry,
    send: gtk4::Button,
    thinking: gtk4::Box,
    conv: gtk4::Box,
    title: gtk4::Label,
    /// The saved build currently open in the center, so a follow-up turn continues
    /// it instead of forking a new session. Empty = a fresh "new build".
    app_id: Rc<RefCell<String>>,
}

/// A build shown in the sidebar (a project) and its threads (conversations).
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
struct StudioProject {
    /// The saved session id this row opens.
    id: String,
    name: String,
    time: String,
    dot: &'static str,
    threads: Vec<StudioThreadItem>,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
struct StudioThreadItem {
    /// The saved session id this row opens.
    id: String,
    title: String,
    time: String,
    dot: &'static str,
    active: bool,
}

/// A single-line label with optional end-ellipsis — used for sidebar titles and
/// the thread title, which must never wrap or push the layout.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn studio_text(text: &str, class: &str, ellipsize: bool, markup: bool) -> gtk4::Label {
    use gtk4::prelude::*;

    let line = if markup {
        // Model-authored bodies are Markdown; render them as Pango so headings,
        // emphasis, lists, and code read as formatted text rather than literal `**`.
        let line = gtk4::Label::new(None);
        line.set_markup(&markdown_to_pango(text));
        line
    } else {
        gtk4::Label::new(Some(text))
    };
    line.set_xalign(0.0);
    if !class.is_empty() {
        line.add_css_class(class);
    }
    if ellipsize {
        line.set_wrap(false);
        line.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    } else {
        line.set_wrap(true);
    }
    line
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn studio_dot(variant: &str) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.set_size_request(8, 8);
    dot.set_valign(gtk::Align::Center);
    dot.add_css_class("gos-studio-dot");
    if !variant.is_empty() {
        dot.add_css_class(variant);
    }
    dot
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn sidebar_project(project: &StudioProject) -> gtk4::Button {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&studio_dot(project.dot));
    let name = studio_text(&project.name, "", true, false);
    name.set_hexpand(true);
    row.append(&name);
    row.append(&studio_text(&project.time, "gos-studio-time", false, false));

    let button = gtk::Button::new();
    button.add_css_class("gos-studio-project");
    button.set_hexpand(true);
    button.set_child(Some(&row));
    button
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn sidebar_thread(thread: &StudioThreadItem) -> gtk4::Button {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&studio_dot(thread.dot));
    let title = studio_text(&thread.title, "", true, false);
    title.set_hexpand(true);
    row.append(&title);
    row.append(&studio_text(&thread.time, "gos-studio-time", false, false));

    let button = gtk::Button::new();
    button.add_css_class("gos-studio-thread-item");
    if thread.active {
        button.add_css_class("is-active");
    }
    button.set_hexpand(true);
    button.set_child(Some(&row));
    button
}

/// The model picker in the composer — our engine switch. Click to cycle GPT-OSS →
/// Codex → your key; the core validates (Codex needs sign-in, the key engine
/// needs a key, both need the internet), and the pill relabels on success.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn engine_pill(config: &ShellConfig, active: &str, badge: &gtk4::Label) -> gtk4::Button {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let pill = button(
        &format!("{} ▾", engine_display(active)),
        &["gos-studio-control", "gos-studio-engine"],
    );
    let core_url = config.core_url.clone();
    let badge = badge.clone();
    pill.connect_clicked(move |pill| {
        let current = engine_from_display(pill.label().map(|g| g.to_string()).unwrap_or_default());
        let next = next_engine(current);
        if set_engine_shell(&core_url, next).is_ok() {
            pill.set_label(&format!("{} ▾", engine_display(next)));
            // Keep the Studio header badge consistent with the composer pill —
            // both now name the engine the next build runs on. The badge uses
            // engine_label (matching how it was built above), not engine_display.
            badge.set_text(&engine_label(next));
        }
    });
    pill
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn engine_display(engine: &str) -> &'static str {
    match engine {
        "codex" => "Codex",
        "openai-api" => "Your OpenAI API key",
        _ => "GPT-OSS",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn engine_from_display(label: String) -> &'static str {
    if label.starts_with("Codex") {
        "codex"
    } else if label.starts_with("Your OpenAI API key") {
        "openai-api"
    } else {
        "local-gpt-oss"
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn next_engine(current: &str) -> &'static str {
    match current {
        "local-gpt-oss" => "codex",
        "codex" => "openai-api",
        _ => "local-gpt-oss",
    }
}

/// Append the agent conversation (messages, then a changed-files block) to `conv`.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn rebuild_conversation(conv: &gtk4::Box, view: &StudioSessionView) {
    use gtk4::prelude::*;

    while let Some(child) = conv.first_child() {
        conv.remove(&child);
    }
    for message in &view.thread {
        conv.append(&studio_message(&message.role, &message.text));
    }
    if !view.files.is_empty() {
        conv.append(&studio_diff_block(
            &format!("Changed files ({})", view.files.len()),
            &view.files,
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn studio_message(role: &str, text: &str) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let row = gtk::Box::new(gtk::Orientation::Vertical, 5);
    row.add_css_class("gos-studio-msg");
    if role == "you" {
        row.add_css_class("gos-studio-msg-you");
    }
    row.append(&label(
        if role == "you" { "You" } else { "Agent" },
        &["gos-studio-msg-role"],
    ));
    row.append(&studio_text(
        text.trim(),
        "gos-studio-msg-text",
        false,
        true,
    ));
    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn studio_diff_block(head: &str, files: &[String]) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let block = gtk::Box::new(gtk::Orientation::Vertical, 4);
    block.add_css_class("gos-studio-block");
    block.append(&label(head, &["gos-studio-block-head"]));
    for path in files {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let file = studio_text(path, "gos-studio-diff-file", true, false);
        file.set_hexpand(true);
        row.append(&file);
        block.append(&row);
    }
    block
}

/// The first user message is the thread's title in the top bar (truncated).
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn thread_title(view: &StudioSessionView) -> String {
    view.thread
        .iter()
        .find(|message| message.role == "you")
        .map(|message| truncate_intent(&message.text))
        .unwrap_or_else(|| view.name.clone())
}

/// Start an async build turn so the engine runs without freezing the surface;
/// the thinking pulse animates while it works.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn start_studio_turn(ui: &StudioUi) {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let message = ui.input.text().to_string();
    let message = message.trim().to_string();
    if message.is_empty() {
        return;
    }

    ui.send.set_sensitive(false);
    ui.input.set_sensitive(false);
    ui.thinking.set_visible(true);

    let (tx, rx) = std::sync::mpsc::channel::<Result<StudioSessionView, String>>();
    let core_url = ui.core_url.clone();
    let app_id = ui.app_id.borrow().clone();
    std::thread::spawn(move || {
        let _ = tx.send(studio_turn_request(&core_url, &message, &app_id));
    });

    let ui = ui.clone();
    let _poll =
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(90), move || {
            match rx.try_recv() {
                Ok(result) => {
                    finish_studio_turn(&ui, result);
                    gtk::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finish_studio_turn(
                        &ui,
                        Err("The build worker stopped unexpectedly.".to_string()),
                    );
                    gtk::glib::ControlFlow::Break
                }
            }
        });
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn finish_studio_turn(ui: &StudioUi, result: Result<StudioSessionView, String>) {
    use gtk4::prelude::*;

    ui.thinking.set_visible(false);
    ui.send.set_sensitive(true);
    ui.input.set_sensitive(true);

    match result {
        Ok(view) => {
            // Pin the composer to the session the core just wrote, so the next turn
            // continues this build (whether it was a fresh build or a continuation).
            *ui.app_id.borrow_mut() = view.id.clone();
            ui.input.set_text("");
            ui.title.set_text(&thread_title(&view));
            rebuild_conversation(&ui.conv, &view);
        }
        Err(detail) => {
            ui.conv.append(&studio_message("agent", &detail));
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn load_studio_session(core_url: &str, id: &str) -> Option<StudioSessionView> {
    if id.is_empty() {
        return None;
    }
    get_core_json::<StudioTurnView>(core_url, &format!("/v1/studio/session?app_id={id}"))
        .ok()?
        .session
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn latest_studio_session(core_url: &str) -> Option<StudioSessionView> {
    let list = get_core_json::<StudioSessionList>(core_url, "/v1/studio/sessions").ok()?;
    let id = list.sessions.first()?.id.clone();
    load_studio_session(core_url, &id)
}

/// Open a saved build in place: load its session and rebuild the conversation and
/// title. Rows without a saved session id are ignored.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn wire_studio_open(
    row: &gtk4::Button,
    core_url: &str,
    id: &str,
    conv: &gtk4::Box,
    title: &gtk4::Label,
    app_id: &Rc<RefCell<String>>,
) {
    use gtk4::prelude::*;

    if id.is_empty() {
        return;
    }
    let core_url = core_url.to_string();
    let id = id.to_string();
    let conv = conv.clone();
    let title = title.clone();
    let app_id = app_id.clone();
    row.connect_clicked(move |_| {
        if let Some(view) = load_studio_session(&core_url, &id) {
            // Make the composer continue this build, not fork a new one.
            *app_id.borrow_mut() = view.id.clone();
            rebuild_conversation(&conv, &view);
            title.set_text(&thread_title(&view));
        }
    });
}

/// Reset the center to the empty "new build" state so the next turn opens a fresh
/// session (the core mints a new id when no app_id is sent).
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn reset_studio_to_new_build(
    conv: &gtk4::Box,
    input: &gtk4::Entry,
    title: &gtk4::Label,
    app_id: &Rc<RefCell<String>>,
) {
    use gtk4::prelude::*;

    while let Some(child) = conv.first_child() {
        conv.remove(&child);
    }
    conv.append(&label(
        "Describe what you want to build. The agent answers, runs its tools, and the changed files appear here — one thread per build.",
        &["gos-studio-empty"],
    ));
    input.set_text("");
    title.set_text("New build");
    // Next turn has no app_id, so the core mints a fresh session.
    app_id.borrow_mut().clear();
}

/// The sidebar's builds. Each saved Studio session is a build with one thread.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn studio_projects(config: &ShellConfig, _shell_state: &ShellState) -> Vec<StudioProject> {
    let Ok(list) = get_core_json::<StudioSessionList>(&config.core_url, "/v1/studio/sessions")
    else {
        return Vec::new();
    };
    list.sessions
        .into_iter()
        .enumerate()
        .map(|(index, summary)| StudioProject {
            id: summary.id.clone(),
            name: summary.name.clone(),
            time: String::new(),
            dot: "is-done",
            threads: vec![StudioThreadItem {
                id: summary.id,
                title: summary.name,
                time: String::new(),
                dot: "is-done",
                active: index == 0,
            }],
        })
        .collect()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn studio_turn_request(
    core_url: &str,
    message: &str,
    app_id: &str,
) -> Result<StudioSessionView, String> {
    // With an app_id the core continues that build; without it, it mints a new one.
    let body = if app_id.is_empty() {
        serde_json::json!({ "message": message }).to_string()
    } else {
        serde_json::json!({ "message": message, "app_id": app_id }).to_string()
    };
    let response = http_post_response(core_url, "/v1/studio/turn", &body)
        .map_err(|_| "Goblins OS could not reach the build engine.".to_string())?;
    let outcome: StudioTurnView = serde_json::from_slice(&response.body)
        .map_err(|_| "Goblins OS could not read the build result.".to_string())?;
    if (200..=299).contains(&response.status) && outcome.ok {
        outcome
            .session
            .ok_or_else(|| "The build returned no session.".to_string())
    } else {
        Err(outcome.text)
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_engine_shell(core_url: &str, engine: &str) -> Result<(), CoreFetchError> {
    let body = serde_json::json!({ "engine": engine }).to_string();
    let response = http_post_response(core_url, "/v1/models/engine", &body)?;
    if (200..=299).contains(&response.status) {
        Ok(())
    } else {
        Err(CoreFetchError::Status(response.status))
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_session_locked_panel(config: &ShellConfig, shell_state: &ShellState) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let auth_configured = shell_state
        .auth
        .as_ref()
        .is_some_and(|auth| auth.configured);
    let auth_authenticated = shell_state
        .auth
        .as_ref()
        .is_some_and(|auth| auth.authenticated);
    let lock_reason = shell_state
        .session_gate
        .as_ref()
        .map(|gate| gate.lock.reason.as_str())
        .unwrap_or("Waiting for local OS services.");
    let first_boot_pending = first_boot_setup_pending(shell_state);

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 18);
    panel.add_css_class("gos-session-lock");
    // The lock is a Goblins OS system surface: its own mark, its own name. The
    // OpenAI provider is named on the action below, not as the screen's identity.
    let lock_mark = goblins_os_ui::brand_mark(goblins_os_design::GOBLINS_MARK_LIGHT, 44);
    lock_mark.set_halign(gtk::Align::Start);
    panel.append(&lock_mark);
    panel.append(&label("Session locked", &["gos-kicker"]));
    panel.append(&label(
        if first_boot_pending {
            "Welcome to Goblins OS"
        } else {
            "Welcome back"
        },
        &["gos-lock-title"],
    ));
    panel.append(&label(lock_reason, &["gos-lock-copy"]));

    let feedback = label(
        "The native login gate owns desktop unlock.",
        &["gos-lock-copy"],
    );

    let sign_in = button(
        if auth_authenticated {
            "OpenAI account ready"
        } else if auth_configured {
            "Sign in with OpenAI"
        } else {
            "OpenAI sign-in not set up"
        },
        if auth_configured && !auth_authenticated {
            &["gos-primary-action"]
        } else {
            &["gos-disabled-action"]
        },
    );
    if auth_configured && !auth_authenticated {
        let core_url = config.core_url.clone();
        let feedback = feedback.clone();
        sign_in.connect_clicked(move |_| match openai_login_destination(&core_url) {
            Ok(destination) => {
                feedback.set_text("Opening the configured OpenAI account provider.");
                if let Err(error) = gtk::gio::AppInfo::launch_default_for_uri(
                    &destination,
                    None::<&gtk::gio::AppLaunchContext>,
                ) {
                    feedback.set_text("The desktop could not open the OpenAI account provider.");
                    eprintln!("shell_lock_openai_launch_error={error}");
                }
            }
            Err(error) => {
                feedback.set_text("OpenAI account handoff did not start.");
                eprintln!("shell_lock_openai_start_error={error}");
            }
        });
    }
    panel.append(&sign_in);

    // When first boot is pending, opening setup IS the one real action on this
    // screen (sign-in is still gated), so it leads as the prominent light primary;
    // otherwise it's the calmer secondary beside an active "Sign in with OpenAI".
    let open_login = button(
        if first_boot_pending {
            "Open first-boot setup"
        } else {
            "Open login gate"
        },
        if first_boot_pending {
            &["gos-primary-action"]
        } else {
            &["gos-secondary-action"]
        },
    );
    {
        let feedback = feedback.clone();
        open_login.connect_clicked(move |_| {
            match if first_boot_pending {
                launch_first_boot_installer()
            } else {
                launch_login_gate()
            } {
                Ok(()) => feedback.set_text(if first_boot_pending {
                    "Opening native Goblins OS first-boot setup."
                } else {
                    "Opening the native Goblins OS login gate."
                }),
                Err(error) => {
                    feedback.set_text(if first_boot_pending {
                        "The first-boot setup could not be opened."
                    } else {
                        "The native login gate could not be opened."
                    });
                    eprintln!("shell_lock_gate_launch_error={error}");
                }
            }
        });
    }
    panel.append(&open_login);
    panel.append(&feedback);

    panel
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn label(text: &str, classes: &[&str]) -> gtk4::Label {
    use gtk4::prelude::*;

    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);

    for class in classes {
        label.add_css_class(class);
    }

    label
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn button(text: &str, classes: &[&str]) -> gtk4::Button {
    use gtk4::prelude::*;

    let button = gtk4::Button::with_label(text);

    for class in classes {
        button.add_css_class(class);
    }

    button
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn spacer() -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    spacer
}

/// Open the launcher's deep-link target as its own rounded, shadowed window —
/// reusing the very same home/Studio/app-detail surfaces the session shell builds,
/// so a launched app looks identical to opening it from the home ledger.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_standalone(config: ShellConfig, target: StandaloneTarget) -> ShellResult<()> {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let boot_state = inspect_boot_state(&config);
    let shell_state = load_shell_state(&config, boot_state);

    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.Shell.App")
        .build();

    application.connect_activate(move |app| {
        goblins_os_ui::init_theming(GOBLINS_OS_CSS);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Goblins OS")
            .decorated(false)
            .build();
        window.add_css_class("gos-windowed");
        window.set_default_size(940, 700);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("gos-root");

        let top_bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        top_bar.add_css_class("gos-top-bar");
        top_bar.append(&goblins_os_ui::window_controls(&window));
        top_bar.append(&goblins_os_ui::themed_brand_mark(22));
        let brand = label("Goblins OS", &["gos-brand"]);
        brand.set_wrap(false);
        top_bar.append(&brand);
        top_bar.append(&spacer());
        root.append(&top_bar);

        let body = gtk::Stack::new();
        body.set_transition_type(gtk::StackTransitionType::Crossfade);
        body.set_transition_duration(200);
        body.set_vexpand(true);

        let app_detail = gtk::Box::new(gtk::Orientation::Vertical, 0);
        app_detail.set_vexpand(true);
        let home = build_home(&config, &shell_state, &body, &app_detail);
        let studio = build_studio(&config, &shell_state, &body);
        body.add_named(&home, Some("home"));
        body.add_named(&studio, Some("studio"));
        body.add_named(&app_detail, Some("app-detail"));

        match &target {
            StandaloneTarget::Studio => body.set_visible_child_name("studio"),
            StandaloneTarget::App(name) => {
                if let Some(app) = shell_state.apps.iter().find(|app| &app.name == name) {
                    populate_app_detail(&app_detail, app, &body);
                    body.set_visible_child_name("app-detail");
                } else {
                    body.set_visible_child_name("home");
                }
            }
        }

        root.append(&body);
        window.set_child(Some(&root));
        window.present();
    });

    application.run_with_args(&["goblins-os-shell"]);
    Ok(())
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_standalone(config: ShellConfig, target: StandaloneTarget) -> ShellResult<()> {
    let _ = config.core_url.as_str();
    if let StandaloneTarget::App(name) = &target {
        let _ = name.as_str();
    }
    println!("native_shell_state=unavailable");
    println!("native_shell_reason=build_requires_linux_native_desktop_feature");
    Ok(())
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_native_shell(_config: ShellConfig, _shell_state: ShellState) -> ShellResult<()> {
    println!("native_shell_state=unavailable");
    println!("native_shell_reason=build_requires_linux_native_desktop_feature");

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn wait_for_core(core_url: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if matches!(http_status(core_url, "/health"), Some(200)) {
            return true;
        }

        thread::sleep(Duration::from_millis(750));
    }

    false
}

fn http_status(base_url: &str, path: &str) -> Option<u16> {
    let endpoint = parse_http_endpoint(base_url)?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .ok()?
        .next()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(600)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(900)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(900)))
        .ok()?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        endpoint.host
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut buffer = [0_u8; 128];
    let read = stream.read(&mut buffer).ok()?;
    let response = std::str::from_utf8(&buffer[..read]).ok()?;
    let status = response.split_whitespace().nth(1)?;

    status.parse().ok()
}

fn get_core_json<T>(base_url: &str, path: &str) -> Result<T, CoreFetchError>
where
    T: for<'de> Deserialize<'de>,
{
    let body = http_get_body(base_url, path)?;

    serde_json::from_slice(&body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn openai_login_destination(core_url: &str) -> Result<String, CoreFetchError> {
    let response = http_get_response(core_url, "/v1/auth/openai/start")?;
    openai_login_destination_from_response(&response)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn openai_login_destination_from_response(
    response: &HttpResponse,
) -> Result<String, CoreFetchError> {
    if !(300..=399).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    header_value(&response.headers, "location")
        .map(ToString::to_string)
        .ok_or(CoreFetchError::Malformed)
}

fn http_get_body(base_url: &str, path: &str) -> Result<Vec<u8>, CoreFetchError> {
    let response = http_get_response(base_url, path)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(response.body)
}

fn http_get_response(base_url: &str, path: &str) -> Result<HttpResponse, CoreFetchError> {
    let endpoint = parse_http_endpoint(base_url).ok_or(CoreFetchError::Malformed)?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| CoreFetchError::Transport)?
        .next()
        .ok_or(CoreFetchError::Transport)?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(700))
        .map_err(|_| CoreFetchError::Transport)?;

    stream
        .set_read_timeout(Some(Duration::from_millis(1200)))
        .map_err(|_| CoreFetchError::Transport)?;
    stream
        .set_write_timeout(Some(Duration::from_millis(900)))
        .map_err(|_| CoreFetchError::Transport)?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        endpoint.host
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|_| CoreFetchError::Transport)?;

    let mut response = Vec::new();
    stream
        .take(MAX_CORE_BODY_BYTES as u64)
        .read_to_end(&mut response)
        .map_err(|_| CoreFetchError::Transport)?;

    parse_http_response(&response)
}

/// POST JSON to the local OS service with a long read window: building an app
/// runs the Goblins AI runtime, which can take many seconds, so this must not
/// time out early.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn http_post_response(
    base_url: &str,
    path: &str,
    body: &str,
) -> Result<HttpResponse, CoreFetchError> {
    let endpoint = parse_http_endpoint(base_url).ok_or(CoreFetchError::Malformed)?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| CoreFetchError::Transport)?
        .next()
        .ok_or(CoreFetchError::Transport)?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(700))
        .map_err(|_| CoreFetchError::Transport)?;

    stream
        .set_read_timeout(Some(Duration::from_secs(180)))
        .map_err(|_| CoreFetchError::Transport)?;
    stream
        .set_write_timeout(Some(Duration::from_millis(2000)))
        .map_err(|_| CoreFetchError::Transport)?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        endpoint.host,
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|_| CoreFetchError::Transport)?;

    let mut response = Vec::new();
    stream
        .take(MAX_CORE_BODY_BYTES as u64)
        .read_to_end(&mut response)
        .map_err(|_| CoreFetchError::Transport)?;

    parse_http_response(&response)
}

/// Ask the Goblins AI runtime to build an app from intent; return the built app's
/// name on success or a calm, user-facing error line.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn submit_build(core_url: &str, intent: &str) -> Result<BuiltApp, String> {
    let body = serde_json::json!({ "intent": intent }).to_string();
    let response = http_post_response(core_url, "/v1/apps/builds", &body)
        .map_err(|_| "Goblins OS could not reach the on-device model engine.".to_string())?;
    let outcome: BuildOutcome = serde_json::from_slice(&response.body)
        .map_err(|_| "Goblins OS could not read the build result.".to_string())?;
    if (200..=299).contains(&response.status) && outcome.ok {
        outcome
            .app
            .ok_or_else(|| "Goblins OS built the app but returned no record of it.".to_string())
    } else {
        Err(outcome.text)
    }
}

/// Render a stored epoch-seconds timestamp as a calm human "Built …" phrase.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn relative_time(created_at: &str) -> String {
    let Ok(then) = created_at.trim().parse::<u64>() else {
        return "Built".to_string();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(then);
    let delta = now.saturating_sub(then);
    if delta < 90 {
        "Built just now".to_string()
    } else if delta < 3600 {
        format!("Built {}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("Built {}h ago", delta / 3600)
    } else if delta < 172_800 {
        "Built yesterday".to_string()
    } else {
        format!("Built {}d ago", delta / 86_400)
    }
}

#[cfg(test)]
fn parse_http_body(response: &[u8]) -> Result<Vec<u8>, CoreFetchError> {
    let response = parse_http_response(response)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(response.body)
}

fn parse_http_response(response: &[u8]) -> Result<HttpResponse, CoreFetchError> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(CoreFetchError::Malformed)?;
    let headers =
        std::str::from_utf8(&response[..header_end]).map_err(|_| CoreFetchError::Malformed)?;
    let mut lines = headers.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or(CoreFetchError::Malformed)?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();

    Ok(HttpResponse {
        status,
        headers,
        body: response[(header_end + 4)..].to_vec(),
    })
}

fn parse_http_endpoint(url: &str) -> Option<HttpEndpoint> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?.trim();

    if authority.is_empty() {
        return None;
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (authority, 80),
    };

    if host.is_empty() {
        return None;
    }

    Some(HttpEndpoint {
        host: host.to_string(),
        port,
    })
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn status_label(status: Option<u16>) -> &'static str {
    match status {
        Some(200..=299) => "ready",
        Some(500..=599) => "blocked",
        Some(_) => "waiting",
        None => "unreachable",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop", not(test)))]
fn launch_local_action(action: &str) -> Result<Option<String>, std::io::Error> {
    let Some((program, args, message)) = local_action_command(action) else {
        return Ok(None);
    };

    std::process::Command::new(program).args(args).spawn()?;
    Ok(Some(message.to_string()))
}

#[cfg(test)]
fn launch_local_action(action: &str) -> Result<Option<String>, std::io::Error> {
    Ok(local_action_command(action).map(|(_, _, message)| message.to_string()))
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn local_action_command(
    action: &str,
) -> Option<(&'static str, &'static [&'static str], &'static str)> {
    match action {
        "settings" => Some((
            "/usr/libexec/goblins-os/goblins-os-settings",
            &[],
            "Opening native Goblins OS Settings.",
        )),
        "recovery" => Some((
            "/usr/libexec/goblins-os/goblins-os-settings",
            &["--panel=recovery"],
            "Opening native Goblins OS Recovery.",
        )),
        "policy" => Some((
            "/usr/libexec/goblins-os/goblins-os-settings",
            &["--panel=policy"],
            "Opening native Goblins OS Policy controls.",
        )),
        _ => None,
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn launch_login_gate() -> Result<(), std::io::Error> {
    std::process::Command::new("/usr/libexec/goblins-os/goblins-os-login").spawn()?;
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn launch_first_boot_installer() -> Result<(), std::io::Error> {
    std::process::Command::new("/usr/libexec/goblins-os/goblins-os-installer").spawn()?;
    Ok(())
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn session_is_unlocked(shell_state: &ShellState) -> bool {
    shell_state
        .session_gate
        .as_ref()
        .is_some_and(|gate| gate.unlocked)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn first_boot_setup_pending(shell_state: &ShellState) -> bool {
    shell_state.session_gate.as_ref().is_some_and(|gate| {
        gate.first_boot_mode.is_none() || gate.lock.state == "waiting-for-first-boot"
    })
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn session_gate_signature(shell_state: &ShellState) -> (bool, Option<String>, Option<String>) {
    shell_state
        .session_gate
        .as_ref()
        .map(|gate| {
            (
                gate.unlocked,
                gate.mode.clone(),
                gate.first_boot_mode.clone(),
            )
        })
        .unwrap_or((false, None, None))
}

fn runtime_label(runtime: &RuntimeReport) -> String {
    let mut names = Vec::new();

    if let Some(selected) = &runtime.selected {
        names.push(selected.clone());
    }

    if runtime.ollama {
        names.push("Ollama".to_string());
    }

    if runtime.vllm {
        names.push("vLLM".to_string());
    }

    if runtime.lm_studio {
        names.push("LM Studio".to_string());
    }

    if names.is_empty() {
        "required".to_string()
    } else {
        names.join(", ")
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const GOBLINS_OS_CSS: &str = "";

#[cfg(test)]
mod tests {
    use super::{
        launch_local_action, local_action_command, openai_login_destination_from_response,
        parse_http_body, parse_http_endpoint, parse_http_response, status_label, CoreFetchError,
        HttpEndpoint, HttpResponse,
    };

    #[test]
    fn voice_entrypoint_uses_goblin_wake_word_copy() {
        let source = include_str!("main.rs");

        assert!(source.contains("Say {voice_word}"));
        assert!(source.contains("Listening for {wake_word}…"));
        assert!(source.contains("Wake phrases: {}."));
        assert!(source.contains("Background wake listening is ready."));
        let apple_assistant = ["si", "ri"].join("");
        let passive_claim = ["always", " listening"].join("");
        let lower = source.to_ascii_lowercase();
        assert!(!lower.contains(&apple_assistant));
        assert!(!lower.contains(&passive_claim));
    }

    #[test]
    fn shell_lock_copy_hides_backend_core_language() {
        let source = include_str!("main.rs");

        assert!(source.contains("Waiting for local OS services."));
        let old_lock_copy = [
            "Waiting for the Goblins OS session gate",
            " from the local core.",
        ]
        .join("");
        assert!(!source.contains(&old_lock_copy));
    }

    #[test]
    fn parses_localhost_with_port() {
        assert_eq!(
            parse_http_endpoint("http://127.0.0.1:8787"),
            Some(HttpEndpoint {
                host: "127.0.0.1".to_string(),
                port: 8787,
            })
        );
    }

    #[test]
    fn defaults_http_port() {
        assert_eq!(
            parse_http_endpoint("http://localhost/v1/readiness"),
            Some(HttpEndpoint {
                host: "localhost".to_string(),
                port: 80,
            })
        );
    }

    #[test]
    fn rejects_non_http_urls() {
        assert_eq!(parse_http_endpoint("https://127.0.0.1:8787"), None);
    }

    #[test]
    fn labels_status_safely() {
        assert_eq!(status_label(Some(204)), "ready");
        assert_eq!(status_label(Some(503)), "blocked");
        assert_eq!(status_label(Some(404)), "waiting");
        assert_eq!(status_label(None), "unreachable");
    }

    #[test]
    fn parses_http_json_body() {
        let body = parse_http_body(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
        )
        .unwrap();

        assert_eq!(body, br#"{"ok":true}"#);
    }

    #[test]
    fn rejects_non_success_http_body() {
        assert_eq!(
            parse_http_body(b"HTTP/1.1 503 Service Unavailable\r\n\r\n{}"),
            Err(CoreFetchError::Status(503))
        );
    }

    #[test]
    fn parses_redirect_location_header() {
        let response = parse_http_response(
            b"HTTP/1.1 302 Found\r\nLocation: https://auth.openai.example/start\r\n\r\n",
        )
        .unwrap();

        assert_eq!(response.status, 302);
        assert_eq!(
            openai_login_destination_from_response(&response),
            Ok("https://auth.openai.example/start".to_string())
        );
    }

    #[test]
    fn rejects_auth_start_without_redirect_location() {
        let response = HttpResponse {
            status: 302,
            headers: Vec::new(),
            body: Vec::new(),
        };

        assert_eq!(
            openai_login_destination_from_response(&response),
            Err(CoreFetchError::Malformed)
        );
    }

    #[test]
    fn rejects_non_redirect_auth_start_response() {
        let response = HttpResponse {
            status: 501,
            headers: Vec::new(),
            body: Vec::new(),
        };

        assert_eq!(
            openai_login_destination_from_response(&response),
            Err(CoreFetchError::Status(501))
        );
    }

    #[test]
    fn maps_settings_and_recovery_to_native_apps() {
        let settings = local_action_command("settings").expect("settings action");
        assert_eq!(settings.0, "/usr/libexec/goblins-os/goblins-os-settings");
        assert!(settings.1.is_empty());

        let recovery = local_action_command("recovery").expect("recovery action");
        assert_eq!(recovery.0, "/usr/libexec/goblins-os/goblins-os-settings");
        assert_eq!(recovery.1, ["--panel=recovery"]);
        assert_eq!(
            launch_local_action("settings").unwrap(),
            Some("Opening native Goblins OS Settings.".to_string())
        );
    }
}
