use std::{
    env,
    error::Error,
    fmt, thread,
    time::{Duration, Instant},
};

use goblins_os_core_client::{initialize, ClientKind, CoreClient, Response};
use serde::Deserialize;

// Rc/RefCell hold the Build Studio's active session id, shared between the sidebar
// and the composer so opening a saved build continues it. Native desktop only.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const DEFAULT_CORE_WAIT_SECS: u64 = 45;
// Voice can consume 30s capture + 120s STT + 3600s inference + 60s TTS +
// 60s playback. The 65-minute client ceiling covers that bounded 3870s path.
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
const LONG_CORE_JOB_TIMEOUT: Duration = Duration::from_secs(65 * 60);
#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
const CONTROL_REQUEST_TIMEOUT: Duration = Duration::from_millis(1500);

type ShellResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone)]
struct ShellConfig {
    core: CoreClient,
    core_wait: Duration,
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

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn engine_display(engine: &str) -> &'static str {
    match engine {
        "local-gpt-oss" => "GPT-OSS",
        "codex" => "Codex",
        "openai-api" => "Your OpenAI API key",
        "cloud-openai" => "Managed OpenAI cloud",
        _ => "Engine unavailable",
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn engine_route_disclosure(engine: Option<&str>) -> &'static str {
    match engine {
        Some("codex") => "OpenAI account via Codex — requests leave this device for OpenAI.",
        Some("openai-api") => {
            "OpenAI hosted models — requests leave this device using your API key."
        }
        Some("cloud-openai") => {
            "Managed OpenAI cloud — requests leave this device through your organization's protected service."
        }
        Some("local-gpt-oss") => "On-device GPT-OSS — requests stay on this computer.",
        _ => "Engine status is unavailable. Reconnect to Goblins OS before building.",
    }
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
    #[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
    Malformed,
    Transport,
    Decode,
}

impl fmt::Display for CoreFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) => write!(formatter, "core returned HTTP {status}"),
            #[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
            Self::Malformed => formatter.write_str("core response was malformed"),
            Self::Transport => formatter.write_str("core connection failed"),
            Self::Decode => formatter.write_str("core response JSON did not match the OS contract"),
        }
    }
}

/// A standalone window the launcher (⌘-Space) can open without disturbing the
/// running session shell: a single built app's detail, the Build Studio, or a
/// qemu-only text input proof surface.
#[derive(Debug, PartialEq, Eq)]
enum StandaloneTarget {
    Studio,
    App(String),
    TextShortcutsProof(TextShortcutsProofMode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextShortcutsProofMode {
    Normal,
    Passthrough,
    Password,
    Dismiss,
    Candidate,
    CandidateRender,
    LiveRuntimeRender,
}

/// Parse the launcher's deep-link from argv (or the env fallback the launcher can
/// also set): `--studio`, `--open-app <name>`, or the qemu-only Text Shortcuts
/// proof surface.
fn standalone_target() -> Option<StandaloneTarget> {
    standalone_target_from_args(env::args().skip(1)).or_else(|| {
        match env::var("GOBLINS_OS_SHELL_OPEN_APP") {
            Ok(name) if !name.is_empty() => Some(StandaloneTarget::App(name)),
            _ => None,
        }
    })
}

fn standalone_target_from_args(args: impl Iterator<Item = String>) -> Option<StandaloneTarget> {
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--studio" => return Some(StandaloneTarget::Studio),
            "--open-app" => return args.next().map(StandaloneTarget::App),
            "--text-shortcuts-proof" => {
                return args
                    .next()
                    .and_then(|mode| text_shortcuts_proof_mode(&mode))
                    .map(StandaloneTarget::TextShortcutsProof);
            }
            _ => {}
        }
    }
    None
}

fn text_shortcuts_proof_mode(mode: &str) -> Option<TextShortcutsProofMode> {
    match mode {
        "normal" => Some(TextShortcutsProofMode::Normal),
        "passthrough" => Some(TextShortcutsProofMode::Passthrough),
        "password" => Some(TextShortcutsProofMode::Password),
        "dismiss" => Some(TextShortcutsProofMode::Dismiss),
        "candidate" => Some(TextShortcutsProofMode::Candidate),
        "candidate-render" => Some(TextShortcutsProofMode::CandidateRender),
        "live-runtime-render" => Some(TextShortcutsProofMode::LiveRuntimeRender),
        _ => None,
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn text_shortcuts_proof_application_id(mode: TextShortcutsProofMode) -> &'static str {
    match mode {
        TextShortcutsProofMode::Normal => "org.goblins.OS.Shell.TextShortcutsProof.Normal",
        TextShortcutsProofMode::Passthrough => {
            "org.goblins.OS.Shell.TextShortcutsProof.Passthrough"
        }
        TextShortcutsProofMode::Password => "org.goblins.OS.Shell.TextShortcutsProof.Password",
        TextShortcutsProofMode::Dismiss => "org.goblins.OS.Shell.TextShortcutsProof.Dismiss",
        TextShortcutsProofMode::Candidate => "org.goblins.OS.Shell.TextShortcutsProof.Candidate",
        TextShortcutsProofMode::CandidateRender => {
            "org.goblins.OS.Shell.TextShortcutsProof.CandidateRender"
        }
        TextShortcutsProofMode::LiveRuntimeRender => {
            "org.goblins.OS.Shell.TextShortcutsProof.LiveRuntimeRender"
        }
    }
}

fn main() -> ShellResult<()> {
    let core = initialize(ClientKind::Shell)?;
    let config = ShellConfig::from_env(core);

    // Launcher deep-link: open a built app or the Build Studio in its own window
    // under a distinct application id, so it never collides with — or re-presents —
    // the always-running session shell.
    if let Some(target) = standalone_target() {
        return run_standalone(config, target);
    }

    let boot_state = inspect_boot_state(&config);
    let shell_state = load_shell_state(&config, boot_state);

    println!("Goblins OS native shell session started");
    println!("core=capability-socket");
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
    fn from_env(core: CoreClient) -> Self {
        Self {
            core,
            core_wait: Duration::from_secs(env_u64(
                "GOBLINS_OS_SHELL_CORE_WAIT_SECS",
                DEFAULT_CORE_WAIT_SECS,
            )),
        }
    }
}

fn inspect_boot_state(config: &ShellConfig) -> BootState {
    let core_ready = wait_for_core(&config.core, config.core_wait);
    let installer_state = if core_ready {
        status_label(http_status(&config.core, "/v1/installer/readiness"))
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

    let auth = get_core_json::<AuthStatus>(&config.core, "/v1/auth/openai/status").ok();
    let session_gate = get_core_json::<SessionGateStatus>(&config.core, "/v1/session/gate").ok();
    let installer =
        get_core_json::<InstallerReadiness>(&config.core, "/v1/installer/readiness").ok();
    let services = get_core_json::<ServiceCatalog>(&config.core, "/v1/services")
        .map(|catalog| catalog.services)
        .unwrap_or_default();
    let local_models = get_core_json::<LocalModelCatalog>(&config.core, "/v1/local-models").ok();
    let resident = get_core_json::<ResidentStatus>(&config.core, "/v1/ai/runtime/status").ok();
    let apps = get_core_json::<AppList>(&config.core, "/v1/apps")
        .map(|list| list.apps)
        .unwrap_or_default();
    let voice = get_core_json::<VoiceStatus>(&config.core, "/v1/voice/status").ok();
    let engine = get_core_json::<EngineStatus>(&config.core, "/v1/models/openai-key").ok();
    let codex = get_core_json::<CodexStatus>(&config.core, "/v1/codex/status").ok();

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

    let mut application_builder =
        gtk::Application::builder().application_id("org.goblins.OS.Shell");
    if std::env::var_os("GOBLINS_OS_CAPTURE_NON_UNIQUE").is_some() {
        application_builder = application_builder.flags(gtk::gio::ApplicationFlags::NON_UNIQUE);
    }
    let application = application_builder.build();

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

    // The headline + subhead already establish the build context, so no eyebrow
    // sits above them — a calmer top-of-page with one less competing element.
    column.append(&centered(
        "What do you want to make?",
        &["gos-home-headline"],
        false,
    ));
    column.append(&centered(
        "Describe an app in a sentence. Goblins AI designs it with your active engine, and Goblins OS keeps it — nothing else comes pre-installed.",
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
    let engine_available = shell_state.engine.is_some();
    build.set_sensitive(engine_available);
    if !engine_available {
        build.set_tooltip_text(Some(
            "Reconnect to Goblins OS before sending a build request.",
        ));
    }
    field.append(&entry);
    field.append(&build);
    column.append(&field);
    // The working animation — a calm three-dot "thinking" pulse shown only while
    // the active engine is designing the app. Hidden (and so paused) at rest.
    // Dots and status share one fixed-height slot so the hero never shifts when
    // the working state toggles.
    let status_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    status_slot.add_css_class("gos-home-status-slot");
    let thinking = thinking_dots();
    thinking.set_visible(false);
    thinking.set_margin_top(16);
    status_slot.append(&thinking);

    let status = centered(
        engine_route_disclosure(
            shell_state
                .engine
                .as_ref()
                .map(|engine| engine.engine.as_str()),
        ),
        &["gos-home-status"],
        true,
    );
    status_slot.append(&status);
    column.append(&status_slot);

    // The home's secondary actions ride one affordance ladder, ranked by weight.
    // The route into the Build Studio is the prominent secondary — the gateway to
    // the whole agent surface — so it takes the calm outlined pill. Voice and
    // Settings are quieter, clearly-affordanced quiet buttons beneath it, so visual
    // weight tracks importance instead of letting a novelty action shout.

    // The prominent secondary: the way into the full Build Studio — the multi-turn
    // agent surface where you switch engines (GPT-OSS · Codex · protected service key).
    let open_studio = button("Open Build Studio", &["gos-home-voice"]);
    open_studio.set_halign(gtk::Align::Center);
    open_studio.set_margin_top(14);
    {
        let stack = stack.clone();
        open_studio.connect_clicked(move |_| stack.set_visible_child_name("studio"));
    }
    column.append(&open_studio);

    // Voice: ask Goblin and hear it answer, all on-device. Offered when the local
    // Whisper/Piper models are present; the quiet button dims (with a reason) until
    // then. A tier below the Studio pill — it's a delight, not the main route.
    let voice_available = shell_state
        .voice
        .as_ref()
        .is_some_and(|voice| voice.available);
    let voice_word = voice_wake_word(shell_state.voice.as_ref());
    let voice = button(&format!("Say {voice_word}"), &["gos-home-settings"]);
    voice.set_halign(gtk::Align::Center);
    voice.set_margin_top(8);
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

    // The quietest tertiary: a route to Settings (engine, OpenAI account, network,
    // privacy). The dock also opens it; the home keeps a calm, affordanced link so
    // the window is self-sufficient without reaching for the dock.
    let open_settings = button("Settings", &["gos-home-settings"]);
    open_settings.set_halign(gtk::Align::Center);
    open_settings.set_margin_top(2);
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
        core: config.core.clone(),
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
    core: CoreClient,
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
/// staggered phase — the restrained "working" cadence, rendered in monochrome.
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
    let core = ui.core.clone();
    std::thread::spawn(move || {
        let _ = tx.send(submit_build(&core, &intent));
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
    let core = ui.core.clone();
    std::thread::spawn(move || {
        let _ = tx.send(converse(&core));
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
fn converse(core: &CoreClient) -> Result<(String, String), String> {
    let response = http_post_response(core, "/v1/voice/converse", "{}")
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

/// The built-app detail view: a calm, focused surface showing the original app
/// request, what the on-device model designed, which engine built it, and the way into
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
        let l = label("BUILD BRIEF", &["gos-home-ledger-kicker"]);
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
            let l = label("BUILD SUMMARY", &["gos-home-ledger-kicker"]);
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
/// engine switch (GPT-OSS · Codex · protected service key). One surface, whichever brain runs.
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
    // The active engine is named exactly once on this surface — by the composer's
    // interactive GPT-OSS picker, which is the single source of truth for which
    // brain the next build runs on. A second static engine pill up here would only
    // restate that label and make the reader wonder whether the two mean different
    // things, so the header stays a clean wordmark row.
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

    let session = latest_studio_session(&config.core);

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
        conv.append(&studio_empty_state());
    }
    let conv_scroll = gtk::ScrolledWindow::new();
    conv_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    conv_scroll.set_child(Some(&conv));
    conv_scroll.set_vexpand(true);
    conv_scroll.add_css_class("gos-studio-conv-scroll");
    main.append(&conv_scroll);

    // Composer — the build input with the live engine switch (GPT-OSS / Codex / protected service key).
    let composer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    composer.add_css_class("gos-studio-composer");
    let input = gtk::Entry::new();
    input.add_css_class("gos-studio-input");
    input.set_placeholder_text(Some("Ask anything, or describe what to build…"));
    composer.append(&input);

    let active_engine = shell_state
        .engine
        .as_ref()
        .map(|engine| engine.engine.clone());
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    controls.add_css_class("gos-studio-controls");
    let codex_ready = shell_state
        .codex
        .as_ref()
        .is_some_and(|codex| codex.installed && codex.authenticated);
    let key_ready = shell_state
        .engine
        .as_ref()
        .is_some_and(|engine| engine.configured);
    controls.append(&engine_picker(
        config,
        active_engine.as_deref(),
        codex_ready,
        key_ready,
    ));
    controls.append(&spacer());
    let thinking = thinking_dots();
    thinking.set_visible(false);
    thinking.set_valign(gtk::Align::Center);
    controls.append(&thinking);
    let send = button("↑", &["gos-studio-send"]);
    send.set_tooltip_text(Some("Send build request"));
    send.update_property(&[
        gtk::accessible::Property::Label("Send build request"),
        gtk::accessible::Property::Description(
            "Send this request to the active Goblins AI engine.",
        ),
    ]);
    send.set_sensitive(active_engine.is_some());
    if active_engine.is_none() {
        send.set_tooltip_text(Some(
            "Reconnect to Goblins OS before sending a build request.",
        ));
    }
    controls.append(&send);
    composer.append(&controls);
    main.append(&composer);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("gos-studio-footer");
    // Seat the footer on exactly the composer card's outer box so the composer and
    // the "Local checkout / main" footer read as one content column, not two offset
    // boxes. The composer card's outer margin is 22px (its CSS `margin`), so the
    // footer takes the same 22px on both sides; its own CSS padding then insets the
    // crumbs within that shared column. Symmetric L/R keeps the column optically
    // centered in the panel regardless of the conversation scrollbar above it.
    footer.set_margin_start(22);
    footer.set_margin_end(22);
    footer.append(&label("Local checkout", &["gos-studio-crumb"]));
    footer.append(&spacer());
    footer.append(&label("main", &["gos-studio-crumb"]));
    // A bottom gutter so the footer is a seated status bar, not content hugging the
    // window chrome — matching the airy top of the panel (Fix: bottom padding).
    footer.set_margin_bottom(14);
    main.append(&footer);

    root.append(&main);

    // Wire the sidebar now that the conversation and title exist: clicking a row
    // opens that saved session; "+ New build" clears to the empty composer state;
    // the search field filters the list live.
    for (row, id) in &studio_rows {
        wire_studio_open(row, &config.core, id, &conv, &title, &active_id);
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
        core: config.core.clone(),
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
    core: CoreClient,
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

/// The model picker in the composer. Every route is named explicitly with its
/// readiness instead of hiding a provider change behind a cycling button.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn engine_picker(
    config: &ShellConfig,
    active: Option<&str>,
    codex_ready: bool,
    key_ready: bool,
) -> gtk4::MenuButton {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let picker = gtk::MenuButton::new();
    picker.set_label(active.map(engine_display).unwrap_or("Engine unavailable"));
    // Studio's picker sits at the bottom edge of the composer. MenuButton owns
    // popup placement, so its direction must agree with the popover preference.
    // Let GTK draw the one direction-aware arrow instead of duplicating it in
    // the label.
    picker.set_direction(gtk::ArrowType::Up);
    picker.add_css_class("gos-studio-control");
    picker.add_css_class("gos-studio-engine");
    picker.set_tooltip_text(Some("Choose the engine for this build"));
    picker.update_property(&[
        gtk::accessible::Property::Label("Choose Goblins AI engine"),
        gtk::accessible::Property::Description(
            "Choose on-device GPT-OSS, your OpenAI account through Codex, or hosted models using an administrator-installed protected service credential.",
        ),
    ]);

    let popover = gtk::Popover::new();
    // The picker sits on the bottom edge of Studio. Keep the complete engine
    // menu and its inline readiness feedback inside the application window.
    popover.set_position(gtk::PositionType::Top);
    popover.add_css_class("gos-studio-engine-popover");
    let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    list.add_css_class("gos-studio-engine-list");

    let feedback = label(
        engine_route_disclosure(active),
        &["gos-studio-engine-feedback"],
    );
    feedback.set_wrap(true);
    feedback.set_xalign(0.0);
    list.append(&feedback);

    let options = [
        (
            "local-gpt-oss",
            "On-device · GPT-OSS",
            active.is_some(),
            if active.is_some() {
                "Runs on this computer. No prompt leaves the device."
            } else {
                "Engine status is unavailable until Goblins OS reconnects."
            },
        ),
        (
            "codex",
            "OpenAI account · Codex",
            codex_ready,
            if codex_ready {
                "Uses your OpenAI account through Codex."
            } else {
                "Sign in to Codex in Settings before choosing this engine."
            },
        ),
        (
            "openai-api",
            "OpenAI hosted · Your API key",
            key_ready,
            if key_ready {
                "Uses OpenAI hosted models with your stored API key."
            } else {
                "Ask a device administrator to install an OpenAI API key before choosing this engine."
            },
        ),
    ];

    for (engine, title, ready, detail) in options {
        let option = gtk::Button::new();
        option.add_css_class("gos-studio-engine-option");
        let option_copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
        option_copy.append(&label(title, &["gos-studio-engine-option-title"]));
        let detail_label = label(detail, &["gos-studio-engine-option-detail"]);
        detail_label.set_wrap(true);
        option_copy.append(&detail_label);
        option.set_child(Some(&option_copy));
        option.set_hexpand(true);
        option.set_sensitive(ready);
        option.set_tooltip_text(Some(detail));
        option.update_property(&[
            gtk::accessible::Property::Label(title),
            gtk::accessible::Property::Description(detail),
        ]);
        let picker = picker.clone();
        let popover = popover.clone();
        let feedback = feedback.clone();
        let core = config.core.clone();
        option.connect_clicked(move |button| {
            button.set_sensitive(false);
            feedback.set_text(&format!("Switching to {}…", engine_display(engine)));

            let (tx, rx) = std::sync::mpsc::channel();
            let request_url = core.clone();
            std::thread::spawn(move || {
                let _ = tx.send(set_engine_shell(&request_url, engine));
            });

            let button = button.clone();
            let picker = picker.clone();
            let popover = popover.clone();
            let feedback = feedback.clone();
            let _poll = gtk::glib::timeout_add_local(Duration::from_millis(75), move || {
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        picker.set_label(engine_display(engine));
                        feedback.set_text(engine_route_disclosure(Some(engine)));
                        popover.popdown();
                        button.set_sensitive(ready);
                        gtk::glib::ControlFlow::Break
                    }
                    Ok(Err(error)) => {
                        feedback.set_text(
                            "Goblins OS could not switch engines. Review readiness in Settings and try again.",
                        );
                        eprintln!("studio_engine_switch_error={error:?}");
                        button.set_sensitive(ready);
                        gtk::glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        gtk::glib::ControlFlow::Continue
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        feedback.set_text(
                            "Goblins OS could not switch engines. Review readiness in Settings and try again.",
                        );
                        button.set_sensitive(ready);
                        gtk::glib::ControlFlow::Break
                    }
                }
            });
        });
        list.append(&option);
    }

    popover.set_child(Some(&list));
    picker.set_popover(Some(&popover));
    picker
}

/// The Studio's first-run / new-build empty state for the conversation pane. The
/// center is the dominant area of the surface, so a top-aligned paragraph leaves a
/// large unconsidered void below it. Instead this returns a vertically- and
/// horizontally-centered column — a quiet brand mark over the centered prompt — so
/// the empty surface reads as calm, deliberate breathing room rather than missing
/// content. It vexpands to claim the pane so the lockup settles at true center.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn studio_empty_state() -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    // Outer wrapper claims the full conversation pane and centers the lockup —
    // the same vexpand + valign(Center) idiom the home hero uses, so the inner
    // group (which does not expand) settles at the optical center of the void.
    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.add_css_class("gos-studio-empty-state");
    center.set_vexpand(true);
    center.set_hexpand(true);
    center.set_valign(gtk::Align::Center);
    center.set_halign(gtk::Align::Center);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);
    column.set_halign(gtk::Align::Center);

    // A quiet, scheme-following mark anchors the void without shouting — the same
    // monoblossom the sidebar head uses, dimmed so it reads as a watermark.
    let mark = goblins_os_ui::themed_brand_mark(30);
    mark.set_opacity(0.3);
    mark.set_halign(gtk::Align::Center);
    column.append(&mark);

    let prompt = label(
        "Describe what you want to build. The agent answers, runs its tools, and the changed files appear here — one thread per build.",
        &["gos-studio-empty"],
    );
    prompt.set_halign(gtk::Align::Center);
    prompt.set_justify(gtk::Justification::Center);
    prompt.set_xalign(0.5);
    prompt.set_wrap(true);
    prompt.set_max_width_chars(46);
    column.append(&prompt);

    center.append(&column);
    center
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
    let core = ui.core.clone();
    let app_id = ui.app_id.borrow().clone();
    std::thread::spawn(move || {
        let _ = tx.send(studio_turn_request(&core, &message, &app_id));
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
fn load_studio_session(core: &CoreClient, id: &str) -> Option<StudioSessionView> {
    if id.is_empty() {
        return None;
    }
    get_core_json::<StudioTurnView>(core, &format!("/v1/studio/session?app_id={id}"))
        .ok()?
        .session
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn latest_studio_session(core: &CoreClient) -> Option<StudioSessionView> {
    let list = get_core_json::<StudioSessionList>(core, "/v1/studio/sessions").ok()?;
    let id = list.sessions.first()?.id.clone();
    load_studio_session(core, &id)
}

/// Open a saved build in place: load its session and rebuild the conversation and
/// title. Rows without a saved session id are ignored.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn wire_studio_open(
    row: &gtk4::Button,
    core: &CoreClient,
    id: &str,
    conv: &gtk4::Box,
    title: &gtk4::Label,
    app_id: &Rc<RefCell<String>>,
) {
    use gtk4::prelude::*;

    if id.is_empty() {
        return;
    }
    let core = core.clone();
    let id = id.to_string();
    let conv = conv.clone();
    let title = title.clone();
    let app_id = app_id.clone();
    row.connect_clicked(move |_| {
        if let Some(view) = load_studio_session(&core, &id) {
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
    conv.append(&studio_empty_state());
    input.set_text("");
    title.set_text("New build");
    // Next turn has no app_id, so the core mints a fresh session.
    app_id.borrow_mut().clear();
}

/// The sidebar's builds. Each saved Studio session is a build with one thread.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn studio_projects(config: &ShellConfig, _shell_state: &ShellState) -> Vec<StudioProject> {
    let Ok(list) = get_core_json::<StudioSessionList>(&config.core, "/v1/studio/sessions") else {
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
    core: &CoreClient,
    message: &str,
    app_id: &str,
) -> Result<StudioSessionView, String> {
    // With an app_id the core continues that build; without it, it mints a new one.
    let body = if app_id.is_empty() {
        serde_json::json!({ "message": message }).to_string()
    } else {
        serde_json::json!({ "message": message, "app_id": app_id }).to_string()
    };
    let response = http_post_response(core, "/v1/studio/turn", &body)
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
fn set_engine_shell(core: &CoreClient, engine: &str) -> Result<(), CoreFetchError> {
    let body = serde_json::json!({ "engine": engine }).to_string();
    let response = http_post_control_response(core, "/v1/models/engine", &body)?;
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
        let core = config.core.clone();
        let feedback = feedback.clone();
        sign_in.connect_clicked(move |_| match openai_login_destination(&core) {
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

    if let StandaloneTarget::TextShortcutsProof(mode) = target {
        return run_text_shortcuts_proof_window(mode);
    }

    let boot_state = inspect_boot_state(&config);
    let shell_state = load_shell_state(&config, boot_state);

    let mut application_builder =
        gtk::Application::builder().application_id("org.goblins.OS.Shell.App");
    if std::env::var_os("GOBLINS_OS_CAPTURE_NON_UNIQUE").is_some() {
        application_builder = application_builder.flags(gtk::gio::ApplicationFlags::NON_UNIQUE);
    }
    let application = application_builder.build();

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
            StandaloneTarget::TextShortcutsProof(_) => {
                unreachable!("Text Shortcuts proof targets use a dedicated GTK window")
            }
        }

        root.append(&body);
        window.set_child(Some(&root));
        window.present();
    });

    application.run_with_args(&["goblins-os-shell"]);
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Copy, Debug, Default)]
struct TextShortcutsLiveLedgerState {
    focused_field_callback: bool,
    process_key_event_callback: bool,
    ibus_commit_delivered: bool,
    candidate_popup_published: bool,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
impl TextShortcutsLiveLedgerState {
    fn runtime_ready(self) -> bool {
        self.focused_field_callback
            && self.process_key_event_callback
            && self.ibus_commit_delivered
            && self.candidate_popup_published
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn text_shortcuts_live_ledger_state(path: &str) -> TextShortcutsLiveLedgerState {
    let Ok(ledger) = std::fs::read_to_string(path) else {
        return TextShortcutsLiveLedgerState::default();
    };
    let mut state = TextShortcutsLiveLedgerState::default();
    for record in ledger
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
    {
        let event = record.get("event").and_then(serde_json::Value::as_str);
        let callback = record.get("callback").and_then(serde_json::Value::as_str);
        if event == Some("callback") && callback == Some("focus-in") {
            state.focused_field_callback = true;
        }
        if event == Some("callback") && callback == Some("process-key-event") {
            state.process_key_event_callback = true;
        }
        if event == Some("operations")
            && record
                .get("operation_types")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|operations| {
                    operations
                        .iter()
                        .any(|operation| operation.as_str() == Some("commit-text"))
                })
        {
            state.ibus_commit_delivered = true;
        }
        if event == Some("native-candidate-popup")
            && record.get("renderer").and_then(serde_json::Value::as_str)
                == Some("native-ibus-lookup-table")
            && record.get("action").and_then(serde_json::Value::as_str) == Some("show-candidate")
            && record.get("published").and_then(serde_json::Value::as_bool) == Some(true)
        {
            state.candidate_popup_published = true;
        }
    }
    state
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn bool_word(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn write_text_shortcuts_live_render_proof(
    path: &str,
    entry_text: &str,
    state: TextShortcutsLiveLedgerState,
) -> std::io::Result<()> {
    let entry_text = entry_text.replace(['\n', '\r'], " ");
    let runtime_ready = state.runtime_ready();
    std::fs::write(
        path,
        format!(
            concat!(
                "surface=goblins-textshortcuts-live-ibus-runtime-render\n",
                "entry_text={entry_text}\n",
                "focused_field_callback={focused}\n",
                "process_key_event_callback={process_key}\n",
                "ibus_commit_delivered={commit}\n",
                "candidate_popup_published={popup}\n",
                "renderer=native-ibus-lookup-table\n",
                "cursor_anchor=ibus-input-context\n",
                "synthetic_overlay=false\n",
                "native_candidate_popup_ready_claim={popup}\n",
                "live_overlay_claim={popup}\n",
                "runtime_ready_claim={runtime_ready}\n",
                "core_readiness_flip=live\n",
            ),
            entry_text = entry_text,
            focused = bool_word(state.focused_field_callback),
            process_key = bool_word(state.process_key_event_callback),
            commit = bool_word(state.ibus_commit_delivered),
            popup = bool_word(state.candidate_popup_published),
            runtime_ready = bool_word(runtime_ready),
        ),
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn prewrite_static_text_shortcuts_proof(
    mode: TextShortcutsProofMode,
    proof_file: Option<&str>,
) -> std::io::Result<()> {
    let Some(path) = proof_file else {
        return Ok(());
    };
    let payload = match mode {
        TextShortcutsProofMode::Candidate => Some(
            "replacement=on my way\naccept_on=word-boundary\ndismiss_key=Escape\nrendered_bubble_ready_claim=false\n",
        ),
        TextShortcutsProofMode::CandidateRender => Some(concat!(
            "surface=goblins-os-shell-text-shortcuts-candidate-bubble-render\n",
            "render_intent_surface=goblins-textshortcuts-accept-bubble-render-intent\n",
            "layout_surface=goblins-textshortcuts-accept-bubble-layout\n",
            "frame_surface=goblins-textshortcuts-accept-bubble-frame\n",
            "replacement=on my way\n",
            "accept_on=word-boundary\n",
            "dismiss_key=Escape\n",
            "style_class=gos-text-shortcuts-candidate\n",
            "text_style_class=gos-text-shortcuts-candidate-text\n",
            "hint_style_class=gos-text-shortcuts-candidate-hint\n",
            "font_family=Inter\n",
            "screenshot=31-text-shortcuts-candidate-bubble-render.png\n",
            "rendered_candidate_surface=true\n",
            "rendered_bubble_ready_claim=false\n",
            "live_overlay_claim=false\n",
            "runtime_ready_claim=false\n",
        )),
        _ => None,
    };
    if let Some(payload) = payload {
        std::fs::write(path, payload)?;
    }
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_text_shortcuts_proof_window(mode: TextShortcutsProofMode) -> ShellResult<()> {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let proof_file = env::var("GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE").ok();
    let proof_events_file = env::var("GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS").ok();
    prewrite_static_text_shortcuts_proof(mode, proof_file.as_deref())?;
    if let Some(path) = &proof_file {
        if !matches!(
            mode,
            TextShortcutsProofMode::Candidate | TextShortcutsProofMode::CandidateRender
        ) {
            std::fs::write(path, "")?;
        }
    }

    let application = gtk::Application::builder()
        .application_id(text_shortcuts_proof_application_id(mode))
        .build();

    application.connect_activate(move |app| {
        goblins_os_ui::init_theming(GOBLINS_OS_CSS);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Goblins OS Text Shortcuts Proof")
            .decorated(false)
            .build();
        window.add_css_class("gos-windowed");
        window.set_default_size(560, 220);

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

        let center = gtk::Box::new(gtk::Orientation::Vertical, 12);
        center.set_margin_top(28);
        center.set_margin_bottom(28);
        center.set_margin_start(36);
        center.set_margin_end(36);
        center.set_valign(gtk::Align::Center);
        center.set_vexpand(true);

        let title = match mode {
            TextShortcutsProofMode::Normal => "Text Shortcuts proof field",
            TextShortcutsProofMode::Passthrough => "Pass-through proof field",
            TextShortcutsProofMode::Password => "Password-field refusal proof",
            TextShortcutsProofMode::Dismiss => "Escape-dismiss proof",
            TextShortcutsProofMode::Candidate => "Text Shortcuts candidate",
            TextShortcutsProofMode::CandidateRender => "Text Shortcuts candidate render",
            TextShortcutsProofMode::LiveRuntimeRender => "Live IBus runtime render proof",
        };
        center.append(&label(title, &["gos-card-title"]));

        let entry = gtk::Entry::new();
        entry.set_hexpand(true);
        entry.set_input_purpose(match mode {
            TextShortcutsProofMode::Normal => gtk::InputPurpose::FreeForm,
            TextShortcutsProofMode::Passthrough => gtk::InputPurpose::FreeForm,
            TextShortcutsProofMode::Password => gtk::InputPurpose::Password,
            TextShortcutsProofMode::Dismiss => gtk::InputPurpose::FreeForm,
            TextShortcutsProofMode::Candidate => gtk::InputPurpose::FreeForm,
            TextShortcutsProofMode::CandidateRender => gtk::InputPurpose::FreeForm,
            TextShortcutsProofMode::LiveRuntimeRender => gtk::InputPurpose::FreeForm,
        });
        if mode == TextShortcutsProofMode::Password {
            entry.set_visibility(false);
        }
        entry.set_placeholder_text(Some(match mode {
            TextShortcutsProofMode::Normal => "Type omw.",
            TextShortcutsProofMode::Passthrough => "Type hello.",
            TextShortcutsProofMode::Password => "Password field",
            TextShortcutsProofMode::Dismiss => "Type omw, then press Escape",
            TextShortcutsProofMode::Candidate => "omw",
            TextShortcutsProofMode::CandidateRender => "omw",
            TextShortcutsProofMode::LiveRuntimeRender => "Type omw, then .",
        }));
        if matches!(
            mode,
            TextShortcutsProofMode::Candidate | TextShortcutsProofMode::CandidateRender
        ) {
            entry.set_text("omw");
        }

        if !matches!(
            mode,
            TextShortcutsProofMode::Candidate | TextShortcutsProofMode::CandidateRender
        ) {
            if let Some(path) = proof_file.clone() {
                let events_path = proof_events_file.clone();
                let live_runtime_render = mode == TextShortcutsProofMode::LiveRuntimeRender;
                entry.connect_changed(move |entry| {
                    if live_runtime_render {
                        let state = events_path
                            .as_deref()
                            .map(text_shortcuts_live_ledger_state)
                            .unwrap_or_default();
                        let _ = write_text_shortcuts_live_render_proof(
                            &path,
                            entry.text().as_str(),
                            state,
                        );
                    } else {
                        let _ = std::fs::write(&path, entry.text().as_str());
                    }
                });
            }
        }

        center.append(&entry);
        if mode == TextShortcutsProofMode::LiveRuntimeRender {
            if let Some(path) = proof_file.clone() {
                write_text_shortcuts_live_render_proof(
                    &path,
                    entry.text().as_str(),
                    TextShortcutsLiveLedgerState::default(),
                )
                .ok();
            }
            let entry = entry.clone();
            let proof_path = proof_file.clone();
            let events_path = proof_events_file.clone();
            let _ = gtk::glib::timeout_add_local(Duration::from_millis(150), move || {
                let state = events_path
                    .as_deref()
                    .map(text_shortcuts_live_ledger_state)
                    .unwrap_or_default();
                if let Some(path) = &proof_path {
                    let _ =
                        write_text_shortcuts_live_render_proof(path, entry.text().as_str(), state);
                }
                gtk::glib::ControlFlow::Continue
            });
        }
        if matches!(
            mode,
            TextShortcutsProofMode::Candidate | TextShortcutsProofMode::CandidateRender
        ) {
            let candidate = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            candidate.add_css_class("gos-text-shortcuts-candidate");
            candidate.append(&label("on my way", &["gos-text-shortcuts-candidate-text"]));
            let hint = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            hint.add_css_class("gos-text-shortcuts-candidate-hint");
            hint.append(&label("Space", &["gos-text-shortcuts-keycap"]));
            hint.append(&label("Esc", &["gos-text-shortcuts-keycap"]));
            candidate.append(&spacer());
            candidate.append(&hint);
            center.append(&candidate);
        }
        root.append(&center);
        window.set_child(Some(&root));
        window.present();

        let focus_attempts = Rc::new(Cell::new(0u8));
        let entry_for_idle = entry.clone();
        let focus_attempts_for_idle = focus_attempts.clone();
        gtk::glib::idle_add_local_once(move || {
            entry_for_idle.grab_focus();
            focus_attempts_for_idle.set(1);
        });
        let entry_for_timer = entry.clone();
        let focus_attempts_for_timer = focus_attempts;
        let _ = gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
            entry_for_timer.grab_focus();
            let attempts = focus_attempts_for_timer.get().saturating_add(1);
            focus_attempts_for_timer.set(attempts);
            if attempts >= 16 {
                gtk::glib::ControlFlow::Break
            } else {
                gtk::glib::ControlFlow::Continue
            }
        });
    });

    application.run_with_args(&["goblins-os-shell"]);
    Ok(())
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_standalone(config: ShellConfig, target: StandaloneTarget) -> ShellResult<()> {
    let _ = &config.core;
    match &target {
        StandaloneTarget::App(name) => {
            let _ = name.as_str();
        }
        StandaloneTarget::TextShortcutsProof(mode) => {
            let _ = mode;
        }
        StandaloneTarget::Studio => {}
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

fn wait_for_core(core: &CoreClient, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if matches!(http_status(core, "/health"), Some(200)) {
            return true;
        }

        thread::sleep(Duration::from_millis(750));
    }

    false
}

fn http_status(core: &CoreClient, path: &str) -> Option<u16> {
    core.get(path, Duration::from_millis(900))
        .ok()
        .map(|response| response.status)
}

fn get_core_json<T>(core: &CoreClient, path: &str) -> Result<T, CoreFetchError>
where
    T: for<'de> Deserialize<'de>,
{
    let body = http_get_body(core, path)?;

    serde_json::from_slice(&body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn openai_login_destination(core: &CoreClient) -> Result<String, CoreFetchError> {
    let response = http_get_response(core, "/v1/auth/openai/start")?;
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

fn http_get_body(core: &CoreClient, path: &str) -> Result<Vec<u8>, CoreFetchError> {
    let response = http_get_response(core, path)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(response.body)
}

fn core_response(response: Response) -> HttpResponse {
    let headers = response
        .header("location")
        .map(|value| vec![("location".to_string(), value.to_string())])
        .unwrap_or_default();
    HttpResponse {
        status: response.status,
        headers,
        body: response.body,
    }
}

fn http_get_response(core: &CoreClient, path: &str) -> Result<HttpResponse, CoreFetchError> {
    core.get(path, Duration::from_millis(1200))
        .map(core_response)
        .map_err(|_| CoreFetchError::Transport)
}

/// POST JSON to the local OS service with a long read window: building an app
/// runs the Goblins AI runtime, which can take many seconds, so this must not
/// time out early.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn http_post_response(
    core: &CoreClient,
    path: &str,
    body: &str,
) -> Result<HttpResponse, CoreFetchError> {
    core.post_json(path, body.as_bytes(), shell_request_timeout(path))
        .map(core_response)
        .map_err(|_| CoreFetchError::Transport)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn shell_request_timeout(path: &str) -> Duration {
    match path {
        "/v1/apps/builds" | "/v1/studio/turn" | "/v1/voice/converse" => LONG_CORE_JOB_TIMEOUT,
        _ => Duration::from_secs(5),
    }
}

/// Short, non-generative OS control requests must never inherit the model turn's
/// three-minute read window. They run off the GTK thread and fail promptly.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn http_post_control_response(
    core: &CoreClient,
    path: &str,
    body: &str,
) -> Result<HttpResponse, CoreFetchError> {
    core.post_json(path, body.as_bytes(), CONTROL_REQUEST_TIMEOUT)
        .map(core_response)
        .map_err(|_| CoreFetchError::Transport)
}

/// Ask the Goblins AI runtime to build an app from intent; return the built app's
/// name on success or a calm, user-facing error line.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn submit_build(core: &CoreClient, intent: &str) -> Result<BuiltApp, String> {
    let body = serde_json::json!({ "intent": intent }).to_string();
    let response = http_post_response(core, "/v1/apps/builds", &body)
        .map_err(|_| "Goblins OS could not reach the active model engine.".to_string())?;
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

// Shell-scoped CSS layered over the shared design theme (native_css injects this
// after the scheme tokens, so every rule resolves @gos_* from the active scheme,
// and before the structural rules, so these compound selectors win on specificity).
//
// The two stacked left-rail buttons play different roles and must not look alike:
// "+ New build" is the create action Studio exists for, so it carries the accent
// as a tinted primary; "← Home" is a navigation escape hatch, so it sits back as a
// quiet ghost link — chrome, not a peer of the create action. Both treatments use
// the studio + accent tokens, so light and dark hold automatically.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const GOBLINS_OS_CSS: &str = r#"
.gos-studio-sidebar .gos-studio-add {
  color: @gos_accent;
  background: alpha(@gos_accent, 0.12);
  border: 1px solid alpha(@gos_accent, 0.55);
  font-weight: 700;
}

.gos-studio-sidebar .gos-studio-add:hover {
  color: @gos_accent;
  background: alpha(@gos_accent, 0.18);
  border-color: @gos_accent;
}

.gos-studio-sidebar .gos-studio-home {
  margin-top: 4px;
  min-height: 28px;
  color: @gos_studio_text_faint;
  background: transparent;
  border: none;
  font-weight: 500;
}

.gos-studio-sidebar .gos-studio-home:hover {
  color: @gos_studio_text_muted;
  background: @gos_studio_hover;
}

/* ── Titlebar hairline: full-bleed to the window chrome ───────────────────────
   The shared .gos-root carries 24px padding for the home body, which also insets
   the topbar — so its bottom hairline floated short of the window's rounded edges
   (visible gaps left + right). Pull the topbar out to the window's inner chrome
   with negative margins (only when windowed/unlocked; the full-screen lock gate
   keeps the padded canvas), so the divider runs edge to edge like the Studio's
   column/row hairlines already do. App CSS is emitted before the shared rules, so
   the extra ancestor class lifts specificity enough to win the override. */
window.gos-windowed .gos-top-bar {
  margin: -24px -24px 0 -24px;
}

/* ── Home build pill: balanced inside the field ───────────────────────────────
   The entry's text sits 18px from the field's left edge (6px field pad + 12px
   entry pad), but the Build pill hugged the right with only the 6px field pad —
   an uneven lead-in. Add a 12px right margin (→ 18px right gap, matching the left
   text lead-in token) and trim the pill's horizontal inset to the same 18px
   lateral rhythm, so its breathing reads even against the ~9px vertical gap. The
   `> button` lifts specificity over the shared `.gos-home-field .gos-home-build`. */
.gos-home-field > button.gos-home-build {
  margin-right: 12px;
  padding: 0 18px;
}

/* ── Home hero field: scheme-equivalent resting + focus elevation ─────────────
   Light lifts the field with a soft drop shadow (@gos_shadow_raise), but that ink
   is fully transparent in dark — so dark rendered flat. Add a 1px accent-tinted
   ring (@gos_focus carries baked alpha and is defined in both schemes) UNDER the
   existing drop shadow: in light the shadow leads and the ring is a faint seat; in
   dark the shadow vanishes and the ring carries the lift — equivalent elevation in
   both. Focus tightens to the same 3px @gos_focus ring every other OS input uses,
   so resting→focus ladders identically per scheme. Ancestor class wins specificity
   over the shared single-class .gos-home-field. */
.gos-root .gos-home-field {
  box-shadow: 0 0 0 1px alpha(@gos_focus, 0.45),
              0 1px 0 @gos_panel_sheen inset,
              0 8px 24px @gos_shadow_raise;
}

.gos-root .gos-home-field:focus-within {
  border: 1px solid @gos_focus;
  box-shadow: 0 0 0 3px @gos_focus,
              0 1px 0 @gos_panel_sheen inset,
              0 12px 32px @gos_shadow_raise;
}

.gos-text-shortcuts-candidate {
  margin-top: -2px;
  padding: 8px 10px;
  min-height: 36px;
  border-radius: 12px;
  border: 1px solid alpha(@gos_accent, 0.38);
  background: alpha(@gos_accent, 0.12);
  box-shadow: 0 8px 24px @gos_shadow_raise;
}

.gos-text-shortcuts-candidate-text {
  color: @gos_ink;
  font-size: 13px;
  font-weight: 650;
}

.gos-text-shortcuts-candidate-hint {
  color: @gos_ink_muted;
}

.gos-text-shortcuts-keycap {
  min-width: 34px;
  padding: 3px 7px;
  border-radius: 8px;
  color: @gos_ink_secondary;
  background: alpha(@gos_surface_sunken, 0.72);
  font-size: 11px;
  font-weight: 650;
}
"#;

#[cfg(test)]
mod tests {
    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    use super::text_shortcuts_live_ledger_state;
    use super::{
        engine_display, engine_route_disclosure, launch_local_action, local_action_command,
        openai_login_destination_from_response, shell_request_timeout, standalone_target_from_args,
        status_label, CoreFetchError, HttpResponse, StandaloneTarget, TextShortcutsProofMode,
        CONTROL_REQUEST_TIMEOUT, LONG_CORE_JOB_TIMEOUT,
    };

    #[test]
    fn build_studio_and_voice_timeouts_cover_their_bounded_core_jobs() {
        for path in ["/v1/apps/builds", "/v1/studio/turn", "/v1/voice/converse"] {
            assert_eq!(shell_request_timeout(path), LONG_CORE_JOB_TIMEOUT);
        }
        assert_eq!(LONG_CORE_JOB_TIMEOUT, std::time::Duration::from_secs(3900));
        assert!(LONG_CORE_JOB_TIMEOUT <= goblins_os_core_client::MAX_READ_TIMEOUT);
        assert_eq!(
            CONTROL_REQUEST_TIMEOUT,
            std::time::Duration::from_millis(1500)
        );
    }

    #[test]
    fn engine_copy_discloses_locality_and_studio_uses_an_explicit_menu() {
        assert_eq!(engine_display("local-gpt-oss"), "GPT-OSS");
        assert_eq!(engine_display("cloud-openai"), "Managed OpenAI cloud");
        assert_eq!(engine_display("unknown"), "Engine unavailable");
        assert!(engine_route_disclosure(Some("local-gpt-oss")).contains("stay"));
        for cloud in ["codex", "openai-api", "cloud-openai"] {
            assert!(
                engine_route_disclosure(Some(cloud)).contains("leave this device"),
                "{cloud} must disclose network egress"
            );
        }
        assert!(engine_route_disclosure(None).contains("unavailable"));

        let source = include_str!("main.rs");
        let engine_picker_source = source
            .split_once("fn engine_picker(")
            .expect("engine picker implementation")
            .1
            .split_once("fn studio_empty_state()")
            .expect("engine picker implementation boundary")
            .0;
        assert!(engine_picker_source.contains("let picker = gtk::MenuButton::new()"));
        assert!(engine_picker_source.contains("picker.set_direction(gtk::ArrowType::Up)"));
        assert!(engine_picker_source.contains("popover.set_position(gtk::PositionType::Top)"));
        assert!(source.contains("Send build request"));
        assert!(!source.contains(&["fn next_", "engine("].concat()));
        assert!(!source.contains(&["fn engine_from_", "display("].concat()));
    }

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
    fn parses_text_shortcuts_proof_targets() {
        assert_eq!(
            standalone_target_from_args(
                ["--text-shortcuts-proof", "normal"]
                    .map(String::from)
                    .into_iter()
            ),
            Some(StandaloneTarget::TextShortcutsProof(
                TextShortcutsProofMode::Normal
            ))
        );
        assert_eq!(
            standalone_target_from_args(
                ["--text-shortcuts-proof", "password"]
                    .map(String::from)
                    .into_iter()
            ),
            Some(StandaloneTarget::TextShortcutsProof(
                TextShortcutsProofMode::Password
            ))
        );
        assert_eq!(
            standalone_target_from_args(
                ["--text-shortcuts-proof", "passthrough"]
                    .map(String::from)
                    .into_iter()
            ),
            Some(StandaloneTarget::TextShortcutsProof(
                TextShortcutsProofMode::Passthrough
            ))
        );
        assert_eq!(
            standalone_target_from_args(
                ["--text-shortcuts-proof", "dismiss"]
                    .map(String::from)
                    .into_iter()
            ),
            Some(StandaloneTarget::TextShortcutsProof(
                TextShortcutsProofMode::Dismiss
            ))
        );
        assert_eq!(
            standalone_target_from_args(
                ["--text-shortcuts-proof", "candidate"]
                    .map(String::from)
                    .into_iter()
            ),
            Some(StandaloneTarget::TextShortcutsProof(
                TextShortcutsProofMode::Candidate
            ))
        );
        assert_eq!(
            standalone_target_from_args(
                ["--text-shortcuts-proof", "candidate-render"]
                    .map(String::from)
                    .into_iter()
            ),
            Some(StandaloneTarget::TextShortcutsProof(
                TextShortcutsProofMode::CandidateRender
            ))
        );
        assert_eq!(
            standalone_target_from_args(
                ["--text-shortcuts-proof", "live-runtime-render"]
                    .map(String::from)
                    .into_iter()
            ),
            Some(StandaloneTarget::TextShortcutsProof(
                TextShortcutsProofMode::LiveRuntimeRender
            ))
        );
        assert_eq!(
            standalone_target_from_args(
                ["--text-shortcuts-proof", "bogus"]
                    .map(String::from)
                    .into_iter()
            ),
            None
        );
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn live_text_shortcuts_ledger_requires_the_native_ibus_popup() {
        let path = std::env::temp_dir().join(format!(
            "goblins-text-shortcuts-native-popup-ledger-{}.jsonl",
            std::process::id()
        ));
        let records = concat!(
            "{\"event\":\"callback\",\"callback\":\"focus-in\"}\n",
            "{\"event\":\"callback\",\"callback\":\"process-key-event\"}\n",
            "{\"event\":\"operations\",\"operation_types\":[\"commit-text\"]}\n",
            "{\"event\":\"native-candidate-popup\",\"renderer\":\"native-ibus-lookup-table\",",
            "\"action\":\"show-candidate\",\"published\":true}\n",
        );
        std::fs::write(&path, records).unwrap();
        let state = text_shortcuts_live_ledger_state(path.to_str().unwrap());
        std::fs::remove_file(path).unwrap();

        assert!(state.focused_field_callback);
        assert!(state.process_key_event_callback);
        assert!(state.ibus_commit_delivered);
        assert!(state.candidate_popup_published);
        assert!(state.runtime_ready());
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
    fn labels_status_safely() {
        assert_eq!(status_label(Some(204)), "ready");
        assert_eq!(status_label(Some(503)), "blocked");
        assert_eq!(status_label(Some(404)), "waiting");
        assert_eq!(status_label(None), "unreachable");
    }

    #[test]
    fn parses_redirect_location_header() {
        let response = HttpResponse {
            status: 302,
            headers: vec![(
                "location".to_string(),
                "https://auth.openai.example/start".to_string(),
            )],
            body: Vec::new(),
        };

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
