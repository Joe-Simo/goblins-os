#![cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]

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

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use goblins_os_ui::status_pill;

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
use gtk4::prelude::*;

const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";
const DEFAULT_CORE_WAIT_SECS: u64 = 45;
const MAX_CORE_BODY_BYTES: usize = 1024 * 1024;
const SETTINGS_DEFAULT_WIDTH: i32 = 1055;
const SETTINGS_DEFAULT_HEIGHT: i32 = 840;
const GNOME_CONTROL_CENTER: &str = "gnome-control-center";
const GNOME_DISK_USAGE_ANALYZER: &str = "baobab";
const GNOME_DISKS: &str = "gnome-disks";
const GNOME_SYSTEM_MONITOR: &str = "gnome-system-monitor";
const GNOME_LOGS: &str = "gnome-logs";
const GNOME_SOFTWARE: &str = "gnome-software";

type SettingsResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone)]
struct SettingsConfig {
    core_url: String,
    core_wait: Duration,
    panel: SettingsPanel,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsPanel {
    Overview,
    Appearance,
    Applications,
    DesktopDock,
    MenuBarControlCenter,
    Network,
    NetworkServices,
    Bluetooth,
    MobileBroadband,
    Sharing,
    Displays,
    ColorManagement,
    Sound,
    Keyboard,
    MouseTrackpad,
    DrawingTablet,
    Accessibility,
    DesktopWallpaper,
    Notifications,
    LockScreen,
    SearchIndexing,
    Multitasking,
    PowerBattery,
    Games,
    PrintersScanners,
    DateTime,
    LanguageRegion,
    UsersAccounts,
    OnlineAccounts,
    PrivacyPermissions,
    Security,
    Wellbeing,
    Models,
    Policy,
    Storage,
    UpdatesAbout,
    Recovery,
    Developer,
}

#[derive(Clone)]
struct SettingsState {
    core_ready: bool,
    system: Option<SettingsSystemStatus>,
    system_image: Option<SystemImageStatus>,
    openai_auth: Option<OpenAIAuthStatus>,
    system_services: Option<SystemServicesStatus>,
    hardware: Option<HardwareStatus>,
    recovery: Option<RecoveryStatus>,
    local_models: Option<LocalModelCatalog>,
    resident: Option<ResidentStatus>,
    ai_actions: Option<AiActionCatalog>,
    ai_action_history: Option<AiActionHistory>,
    policy: Option<PolicyStatus>,
    openai_key: Option<OpenAiKeyStatus>,
    privacy: Option<PrivacyStatus>,
    voice: Option<VoiceStatus>,
    codex: Option<CodexStatus>,
    appearance: Option<AppearanceStatus>,
    network: Option<NetworkStatus>,
    notifications: Option<NotificationsStatus>,
    displays: Option<DisplaysStatus>,
    bluetooth: Option<BluetoothStatus>,
    audio: Option<AudioStatus>,
    input: Option<InputStatus>,
    accessibility: Option<AccessibilityStatus>,
}

/// Read-only system-image deployment status from `GET /v1/system/image`, backed
/// by a real `bootc status` in core. The Settings UI mirrors it; no field here is
/// mutable — update/rollback execution stays an explicit, gated future capability.
#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct SystemImageStatus {
    available: bool,
    #[serde(default)]
    rollback_available: bool,
    #[serde(default)]
    staged_available: bool,
    #[serde(default)]
    booted: Option<ImageDeployment>,
    #[serde(default)]
    rollback: Option<ImageDeployment>,
    #[serde(default)]
    staged: Option<ImageDeployment>,
    detail: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct ImageDeployment {
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    digest_short: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct LocalAccountSummary {
    username: String,
    display_name: String,
    uid: Option<u32>,
    gid: Option<u32>,
    home: String,
    shell: String,
    hostname: String,
    account_type: String,
    admin_groups: Vec<String>,
}

/// NetworkManager-backed connectivity status from Goblins OS. The Settings UI
/// mirrors this as a real system state; Wi-Fi secrets never come back from core.
#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct NetworkStatus {
    source: String,
    manager_available: bool,
    online: bool,
    connectivity: String,
    state: String,
    active: Option<ActiveConnection>,
    #[serde(default)]
    proxy: Option<ProxyStatus>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct ActiveConnection {
    name: String,
    kind: String,
    device: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct ProxyStatus {
    gsettings_available: bool,
    schema_available: bool,
    mode_available: bool,
    mode: String,
    autoconfig_url: Option<String>,
    ignore_hosts: Vec<String>,
    http: ProxyEndpoint,
    https: ProxyEndpoint,
    ftp: ProxyEndpoint,
    socks: ProxyEndpoint,
    detail: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct ProxyEndpoint {
    host: Option<String>,
    port: Option<i32>,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct WifiScan {
    manager_available: bool,
    networks: Vec<WifiNetwork>,
    detail: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct WifiNetwork {
    ssid: String,
    signal: u8,
    security: String,
    in_use: bool,
}

/// Read-only display status from Goblins OS. Resolution,
/// scaling, mirroring, and arrangement stay read-only until core owns a
/// policy-aware desktop display configuration route.
#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct DisplaysStatus {
    session_type: String,
    wayland_display: Option<String>,
    x11_display: Option<String>,
    gdbus_available: bool,
    mutter_display_config_available: bool,
    xrandr_available: bool,
    outputs: Vec<DisplayOutputStatus>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct DisplayOutputStatus {
    name: String,
    connected: bool,
    primary: bool,
    current_mode: Option<String>,
    position: Option<String>,
    detail: String,
}

/// Desktop notification preference status from Goblins OS. Settings only
/// renders reported state and sends allowlisted preference changes back to core.
#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct NotificationsStatus {
    #[allow(dead_code)]
    source: String,
    gsettings_available: bool,
    schema_available: bool,
    application_schema_available: bool,
    show_banners: Option<bool>,
    show_in_lock_screen: Option<bool>,
    application_children: Vec<String>,
    applications: Vec<NotificationApplicationStatus>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct NotificationApplicationStatus {
    child: String,
    label: String,
    enable: Option<bool>,
    show_banners: Option<bool>,
    enable_sound_alerts: Option<bool>,
    show_in_lock_screen: Option<bool>,
    details_in_lock_screen: Option<bool>,
    force_expanded: Option<bool>,
    detail: String,
}

/// OpenAI account handoff status. The GUI never sees
/// tokens; it only mirrors whether the provider and OS-owned session exist.
#[derive(Clone, Deserialize)]
struct OpenAIAuthStatus {
    configured: bool,
    authenticated: bool,
    provider: String,
    session_storage: String,
    message: String,
}

/// Read-only Bluetooth status. Pairing stays disabled
/// until the core owns a protected route for changing adapter state.
#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct BluetoothStatus {
    source: String,
    bluez_available: bool,
    service_active: bool,
    adapter_present: bool,
    powered: Option<bool>,
    discoverable: Option<bool>,
    pairable: Option<bool>,
    adapter: Option<BluetoothAdapterStatus>,
    // Backend detail is retained for compatibility but not shown in product copy.
    #[allow(dead_code)]
    detail: String,
}

#[derive(Clone, Deserialize)]
struct BluetoothAdapterStatus {
    name: Option<String>,
    alias: Option<String>,
    address: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct AudioStatus {
    source: String,
    wireplumber_available: bool,
    output: AudioEndpointStatus,
    input: AudioEndpointStatus,
    #[serde(default)]
    sound: Option<SoundPreferencesStatus>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct AudioEndpointStatus {
    available: bool,
    volume_percent: Option<u8>,
    muted: Option<bool>,
    #[serde(default)]
    default_device_id: Option<String>,
    #[serde(default)]
    devices: Vec<AudioDeviceStatus>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct AudioDeviceStatus {
    id: String,
    name: String,
    active: bool,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct SoundPreferencesStatus {
    gsettings_available: bool,
    schema_available: bool,
    event_sounds: Option<bool>,
    input_feedback_sounds: Option<bool>,
    volume_boost: Option<bool>,
    theme_name: Option<String>,
    detail: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct AppearanceStatus {
    source: String,
    gsettings_available: bool,
    color_scheme_available: bool,
    color_scheme: String,
    theme: String,
    #[serde(default)]
    wallpaper: Option<WallpaperStatus>,
    detail: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct WallpaperStatus {
    gsettings_available: bool,
    schema_available: bool,
    picture_uri: Option<String>,
    picture_uri_dark: Option<String>,
    picture_options_available: bool,
    picture_options: String,
    primary_color: Option<String>,
    secondary_color: Option<String>,
    color_shading_type_available: bool,
    color_shading_type: String,
    detail: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct InputStatus {
    source: String,
    gsettings_available: bool,
    keyboard: KeyboardInputStatus,
    mouse: MouseInputStatus,
    touchpad: TouchpadInputStatus,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct KeyboardInputStatus {
    schema_available: bool,
    repeat: Option<bool>,
    delay_ms: Option<u32>,
    repeat_interval_ms: Option<u32>,
    remember_numlock_state: Option<bool>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct MouseInputStatus {
    schema_available: bool,
    speed: Option<f64>,
    natural_scroll: Option<bool>,
    left_handed: Option<bool>,
    middle_click_emulation: Option<bool>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct TouchpadInputStatus {
    schema_available: bool,
    speed: Option<f64>,
    tap_to_click: Option<bool>,
    natural_scroll: Option<bool>,
    two_finger_scrolling_enabled: Option<bool>,
    disable_while_typing: Option<bool>,
    detail: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct AccessibilityStatus {
    #[allow(dead_code)]
    source: String,
    gsettings_available: bool,
    interface: InterfaceAccessibilityStatus,
    assistive: AssistiveTechnologyStatus,
    display_comfort: DisplayComfortStatus,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct InterfaceAccessibilityStatus {
    schema_available: bool,
    reduce_motion: Option<bool>,
    text_scale: Option<f64>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct AssistiveTechnologyStatus {
    schema_available: bool,
    screen_reader: Option<bool>,
    screen_keyboard: Option<bool>,
    magnifier: Option<bool>,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct DisplayComfortStatus {
    schema_available: bool,
    night_light_enabled: Option<bool>,
    schedule_automatic: Option<bool>,
    temperature: Option<u32>,
    detail: String,
}

#[derive(Deserialize)]
struct WifiConnectOutcome {
    ok: bool,
    #[allow(dead_code)]
    ssid: String,
    text: String,
}

#[derive(Deserialize)]
struct ProxyModeOutcome {
    ok: bool,
    mode: String,
    text: String,
}

#[derive(Deserialize)]
struct BluetoothPowerOutcome {
    ok: bool,
    #[allow(dead_code)]
    powered: bool,
    text: String,
}

#[derive(Deserialize)]
struct InputPreferenceOutcome {
    ok: bool,
    #[allow(dead_code)]
    target: String,
    text: String,
}

#[derive(Deserialize)]
struct AppearanceOutcome {
    ok: bool,
    #[allow(dead_code)]
    color_scheme: String,
    theme: String,
    text: String,
}

#[derive(Deserialize)]
struct WallpaperPlacementOutcome {
    ok: bool,
    placement: String,
    text: String,
}

#[derive(Deserialize)]
struct WallpaperShadingOutcome {
    ok: bool,
    shading: String,
    text: String,
}

#[derive(Deserialize)]
struct AccessibilityPreferenceOutcome {
    ok: bool,
    #[allow(dead_code)]
    target: String,
    text: String,
}

#[derive(Deserialize)]
struct DesktopPrivacyOutcome {
    ok: bool,
    #[allow(dead_code)]
    target: String,
    text: String,
}

#[derive(Deserialize)]
struct NotificationPreferenceOutcome {
    ok: bool,
    #[allow(dead_code)]
    target: String,
    text: String,
}

#[derive(Deserialize)]
struct AudioControlOutcome {
    ok: bool,
    #[allow(dead_code)]
    target: String,
    text: String,
    #[allow(dead_code)]
    volume_percent: Option<u8>,
    #[allow(dead_code)]
    muted: Option<bool>,
}

#[derive(Deserialize)]
struct SoundPreferenceOutcome {
    ok: bool,
    #[allow(dead_code)]
    target: String,
    text: String,
}

/// Codex CLI (the user's OpenAI account) status from Goblins OS. The GUI mirrors
/// install/sign-in state and triggers `codex login`; credentials stay with Codex.
#[derive(Clone, Deserialize)]
struct CodexStatus {
    installed: bool,
    authenticated: bool,
    detail: String,
}

/// Local-voice capability from Goblins OS, mirrored read-only in Settings.
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
}

fn default_voice_wake_word() -> String {
    "Goblin".to_string()
}

fn voice_settings_detail(voice: &VoiceStatus) -> String {
    let listener = voice
        .wake_listening
        .as_ref()
        .map(|listener| {
            if listener.ready {
                "Background wake listening is ready.".to_string()
            } else {
                listener.detail.clone()
            }
        })
        .unwrap_or_else(|| format!("Press the voice button, then say {}.", voice.wake_word()));

    format!(
        "Wake word: {}. Phrases: {}. {} {}",
        voice.wake_word(),
        voice.wake_phrases(),
        voice.detail,
        listener
    )
}

/// Offline / private-mode status from Goblins OS. The toggle only mirrors and
/// flips this; the core enforces the egress gate.
#[derive(Clone, Deserialize)]
struct PrivacyStatus {
    offline: bool,
    detail: String,
    #[serde(default)]
    desktop: Option<DesktopPrivacyStatus>,
    #[serde(default)]
    facilities: Vec<SystemFacility>,
}

#[derive(Clone, Deserialize)]
struct DesktopPrivacyStatus {
    gsettings_available: bool,
    schema_available: bool,
    remember_recent_files: Option<bool>,
    remember_app_usage: Option<bool>,
    remove_old_trash_files: Option<bool>,
    remove_old_temp_files: Option<bool>,
    old_files_age_days: Option<u32>,
    disable_microphone: Option<bool>,
    disable_camera: Option<bool>,
    disable_sound_output: Option<bool>,
    usb_protection: Option<bool>,
    detail: String,
}

/// Status of the optional bring-your-own OpenAI API key. The key itself is never
/// returned by the core — only whether one is configured, the selected model, and
/// where the secret is held. The OS owns the secret; the GUI only mirrors status.
#[derive(Clone, Deserialize)]
struct OpenAiKeyStatus {
    configured: bool,
    model: String,
    engine_selected: bool,
    engine: String,
    storage: String,
}

#[derive(Clone, Deserialize)]
struct SettingsSystemStatus {
    generated_at: String,
    source: String,
    session: SessionSettings,
    identity: IdentitySettings,
    #[serde(default)]
    local_account: Option<LocalAccountSummary>,
    storage: StorageSettings,
    services: ServiceSettings,
}

impl SettingsSystemStatus {
    fn session_has_integrated_device_settings(&self) -> bool {
        configured_runtime_value(&self.session.desktop)
            && self.session.gui_platform == "gnome-session"
            && self.session.shell_mode == "native-desktop"
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Copy)]
struct SettingsSearchItem {
    panel: SettingsPanel,
    title: &'static str,
    terms: &'static [&'static str],
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const SETTINGS_SEARCH_ITEMS: &[SettingsSearchItem] = &[
    SettingsSearchItem {
        panel: SettingsPanel::Overview,
        title: "System health",
        terms: &[
            "overview",
            "status",
            "services",
            "hardware",
            "goblins ai runtime",
            "assistant runtime",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Appearance,
        title: "Color scheme",
        terms: &["appearance", "light", "dark", "theme", "visual system"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Appearance,
        title: "Inter typography",
        terms: &["font", "inter", "type", "text"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Applications,
        title: "Default applications",
        terms: &["apps", "application defaults", "file handlers"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Applications,
        title: "Application permissions",
        terms: &["apps", "permissions", "sandbox", "access"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::DesktopDock,
        title: "Dock",
        terms: &[
            "dock",
            "launcher",
            "app launcher",
            "favorites",
            "running apps",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::DesktopDock,
        title: "Window controls",
        terms: &[
            "windows",
            "window controls",
            "close",
            "minimize",
            "maximize",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::MenuBarControlCenter,
        title: "Menu bar",
        terms: &["menu bar", "top bar", "clock", "status menu", "system menu"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::MenuBarControlCenter,
        title: "Control Center",
        terms: &[
            "control center",
            "quick settings",
            "wifi menu",
            "sound menu",
            "power menu",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::DesktopWallpaper,
        title: "Wallpaper image",
        terms: &["desktop", "background", "picture", "uri"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::DesktopWallpaper,
        title: "Wallpaper placement",
        terms: &[
            "fit",
            "fill",
            "span",
            "center",
            "tile",
            "stretch",
            "placement",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::DesktopWallpaper,
        title: "Wallpaper color",
        terms: &["shading", "primary color", "secondary color", "solid"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Displays,
        title: "Connected displays",
        terms: &[
            "display",
            "monitor",
            "screen",
            "outputs",
            "scale",
            "resolution",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Displays,
        title: "Night Light",
        terms: &[
            "display",
            "blue light",
            "warmth",
            "night-light",
            "night light",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Displays,
        title: "Automatic schedule",
        terms: &[
            "night light",
            "automatic schedule",
            "schedule",
            "sunset",
            "sunrise",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Displays,
        title: "Color temperature",
        terms: &[
            "night light",
            "color temperature",
            "warmth",
            "warmer",
            "cooler",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::ColorManagement,
        title: "Color profiles",
        terms: &["icc", "calibration", "profile", "display color"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Sound,
        title: "Output volume",
        terms: &["audio", "speaker", "volume", "mute", "output"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Sound,
        title: "Input level",
        terms: &["audio", "microphone", "input", "recording"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Sound,
        title: "Volume boost",
        terms: &["audio", "volume above 100", "amplification", "loudness"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Sound,
        title: "Interface sounds",
        terms: &["alert sounds", "event sounds", "sound theme", "feedback"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Notifications,
        title: "Notification banners",
        terms: &["alerts", "banners", "do not disturb", "notifications"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Notifications,
        title: "Lock-screen notifications",
        terms: &["lock screen", "privacy", "notification preview"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Notifications,
        title: "Per-app notifications",
        terms: &["apps", "notification registry", "application alerts"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::LockScreen,
        title: "Screen lock",
        terms: &["lock screen", "screen lock", "blank screen", "login screen"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::LockScreen,
        title: "Lock-screen privacy",
        terms: &[
            "lock screen",
            "notification privacy",
            "notification previews",
            "hide previews",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::SearchIndexing,
        title: "Desktop search providers",
        terms: &["search", "indexing", "file search", "results"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Multitasking,
        title: "Workspaces",
        terms: &[
            "multitasking",
            "workspaces",
            "hot corner",
            "window switching",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PowerBattery,
        title: "Power mode",
        terms: &["power", "battery", "energy", "performance", "power profile"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PowerBattery,
        title: "Sleep and suspend",
        terms: &["sleep", "suspend", "lid", "idle", "battery"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Games,
        title: "Gaming readiness",
        terms: &[
            "games",
            "gaming",
            "vulkan",
            "gamemode",
            "gamescope",
            "mangohud",
            "controller",
            "flatpak",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Games,
        title: "Non-Steam launchers",
        terms: &["heroic", "lutris", "bottles", "umu", "proton", "launcher"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PrintersScanners,
        title: "Printers",
        terms: &["printer", "print", "cups", "queue"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PrintersScanners,
        title: "Scanners",
        terms: &["scanner", "scan", "sane", "device"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::DateTime,
        title: "Date and time",
        terms: &["date", "time", "clock", "calendar", "automatic time"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::DateTime,
        title: "Time zone",
        terms: &["timezone", "time zone", "ntp", "network time"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::LanguageRegion,
        title: "Language",
        terms: &["language", "locale", "region", "display language"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::LanguageRegion,
        title: "Region formats",
        terms: &["region", "formats", "measurement", "calendar format"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Wellbeing,
        title: "Break reminders",
        terms: &["wellbeing", "screen time", "breaks", "attention"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Network,
        title: "Wi-Fi networks",
        terms: &["network", "wifi", "wi-fi", "wireless", "join network"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Network,
        title: "Active connection",
        terms: &["network", "online", "connectivity", "ssid", "ip address"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Network,
        title: "Proxy",
        terms: &[
            "http proxy",
            "https proxy",
            "ignore hosts",
            "pac",
            "auto config",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::NetworkServices,
        title: "Wired network",
        terms: &["ethernet", "wired", "lan", "adapter"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::NetworkServices,
        title: "VPN",
        terms: &["vpn", "tunnel", "advanced network"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Bluetooth,
        title: "Bluetooth power",
        terms: &["bluetooth", "adapter", "radio", "power"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Bluetooth,
        title: "Bluetooth devices",
        terms: &[
            "bluetooth",
            "pair",
            "connect",
            "device",
            "keyboard",
            "headphones",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::MobileBroadband,
        title: "Mobile broadband",
        terms: &["wwan", "cellular", "modem", "sim"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Sharing,
        title: "Remote access",
        terms: &["sharing", "remote desktop", "file sharing", "hostname"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Keyboard,
        title: "Keyboard repeat",
        terms: &["keyboard", "repeat", "delay", "typing"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Keyboard,
        title: "Repeat delay",
        terms: &[
            "keyboard",
            "repeat delay",
            "key repeat delay",
            "typing delay",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Keyboard,
        title: "Repeat interval",
        terms: &[
            "keyboard",
            "repeat interval",
            "repeat speed",
            "key repeat speed",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Keyboard,
        title: "Remember Num Lock",
        terms: &["keyboard", "num lock", "numlock", "number pad"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Keyboard,
        title: "Input sources",
        terms: &["keyboard layout", "input source", "language"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Keyboard,
        title: "Keyboard shortcuts",
        terms: &["shortcut", "hotkey", "key binding"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::MouseTrackpad,
        title: "Pointer speed",
        terms: &["mouse", "trackpad", "pointer", "speed", "acceleration"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::MouseTrackpad,
        title: "Natural scrolling",
        terms: &["mouse", "trackpad", "scroll", "natural"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::MouseTrackpad,
        title: "Touchpad gestures",
        terms: &["trackpad", "touchpad", "tap to click", "gesture"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::DrawingTablet,
        title: "Drawing tablet",
        terms: &["wacom", "tablet", "stylus", "pen", "calibration"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Accessibility,
        title: "Screen reader",
        terms: &["accessibility", "screen reader", "speech"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Accessibility,
        title: "On-screen keyboard",
        terms: &[
            "accessibility",
            "screen keyboard",
            "onscreen keyboard",
            "typing",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Accessibility,
        title: "Text scale",
        terms: &["accessibility", "large text", "text size", "font size"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Accessibility,
        title: "Magnifier",
        terms: &["accessibility", "magnifier", "zoom", "screen zoom"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Accessibility,
        title: "Reduce motion",
        terms: &["accessibility", "animation", "motion", "reduce motion"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::UsersAccounts,
        title: "OpenAI account",
        terms: &["account", "sign in", "openai", "authentication"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::UsersAccounts,
        title: "Codex account",
        terms: &["codex", "login", "authenticated"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::UsersAccounts,
        title: "Local user",
        terms: &[
            "username",
            "home folder",
            "admin",
            "hostname",
            "computer name",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::OnlineAccounts,
        title: "Internet accounts",
        terms: &["online accounts", "cloud accounts", "mail", "calendar"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PrivacyPermissions,
        title: "Private mode",
        terms: &["privacy", "private mode", "history", "app usage"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PrivacyPermissions,
        title: "Recent files",
        terms: &["privacy", "recent documents", "file history"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PrivacyPermissions,
        title: "Protected resources",
        terms: &[
            "camera",
            "microphone",
            "location",
            "screen capture",
            "permissions",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PrivacyPermissions,
        title: "Credentials",
        terms: &["keyring", "secrets", "openai key", "api key", "credentials"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::PrivacyPermissions,
        title: "USB protection",
        terms: &["usb", "thunderbolt", "device access", "protection"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Security,
        title: "Passwords",
        terms: &["security", "password", "local password", "administrator"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Security,
        title: "Firewall",
        terms: &["security", "firewall", "network protection", "firewall-cmd"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Security,
        title: "Secrets storage",
        terms: &["security", "secrets", "keyring", "credentials", "api key"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Models,
        title: "Goblin",
        terms: &[
            "goblin",
            "goblins ai",
            "ask goblins",
            "assistant",
            "system assistant",
            "global assistant",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Models,
        title: "Context actions",
        terms: &[
            "selected text",
            "current window",
            "screenshot",
            "screen context",
            "file context",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Models,
        title: "Confirmed AI actions",
        terms: &[
            "safe settings",
            "change settings",
            "troubleshoot",
            "permission",
            "confirmation",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Models,
        title: "Goblin action history",
        terms: &[
            "history",
            "audit",
            "recent actions",
            "action log",
            "ai privacy",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Models,
        title: "Model engine",
        terms: &["ai", "model", "engine", "gpt-oss", "local model"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Models,
        title: "OpenAI API key",
        terms: &["openai key", "api key", "server side", "hosted model"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Models,
        title: "Goblin wake word",
        terms: &[
            "goblin",
            "hey goblin",
            "wake word",
            "voice",
            "speech",
            "local voice",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Policy,
        title: "Policy profile",
        terms: &["policy", "consumer", "business", "enterprise", "mode"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Policy,
        title: "Permission grants",
        terms: &["permission", "approval", "grant", "boundary"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Storage,
        title: "Free space",
        terms: &["storage", "disk", "capacity", "free space", "full"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Storage,
        title: "Model cache",
        terms: &["storage", "models", "cache", "disk usage"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Storage,
        title: "Cleanup",
        terms: &[
            "storage",
            "cleanup",
            "trash",
            "temporary files",
            "temp files",
            "full disk",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Storage,
        title: "Mounted storage",
        terms: &["mount", "drive", "filesystem", "disks"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::UpdatesAbout,
        title: "OS image",
        terms: &["updates", "about", "bootc", "version", "image"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::UpdatesAbout,
        title: "Software updates",
        terms: &["update", "upgrade", "software", "security"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Recovery,
        title: "Service checks",
        terms: &["recovery", "health", "repair", "services"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Recovery,
        title: "Rollback",
        terms: &["recovery", "rollback", "restore", "reset"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Developer,
        title: "Logs",
        terms: &["diagnostics", "developer", "logs", "journal"],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Developer,
        title: "Service health",
        terms: &[
            "diagnostics",
            "services",
            "goblins ai runtime",
            "assistant runtime",
            "core",
        ],
    },
    SettingsSearchItem {
        panel: SettingsPanel::Developer,
        title: "Hardware detail",
        terms: &["diagnostics", "hardware", "kernel", "device"],
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeviceSettingsReadiness {
    Ready,
    IntegratedDesktopUnavailable,
    WaitingForSession,
    Unavailable,
}

impl DeviceSettingsReadiness {
    fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

fn device_settings_readiness(
    system: Option<&SettingsSystemStatus>,
    control_center_available: bool,
) -> DeviceSettingsReadiness {
    if !control_center_available {
        return DeviceSettingsReadiness::Unavailable;
    }

    match system {
        Some(system) if system.session_has_integrated_device_settings() => {
            DeviceSettingsReadiness::Ready
        }
        Some(_) => DeviceSettingsReadiness::IntegratedDesktopUnavailable,
        None => DeviceSettingsReadiness::WaitingForSession,
    }
}

#[derive(Clone, Deserialize)]
struct SessionSettings {
    desktop: String,
    gui_platform: String,
    shell_mode: String,
    core_url: String,
}

#[derive(Clone, Deserialize)]
struct IdentitySettings {
    provider_configured: bool,
    account_authenticated: bool,
    session_path: String,
}

#[derive(Clone, Deserialize)]
struct StorageSettings {
    model_dir: String,
    installer_state_dir: String,
    session_state_dir: String,
    policy_state_dir: String,
    resident_state_dir: String,
    secrets_dir: String,
}

#[derive(Clone, Deserialize)]
struct ServiceSettings {
    bootc_image: String,
    bootc_available: bool,
    systemctl_available: bool,
    network_manager_available: bool,
}

#[derive(Clone, Deserialize)]
struct SystemServicesStatus {
    generated_at: String,
    source: String,
    manager_available: bool,
    unit_dir: String,
    libexec_dir: String,
    services: Vec<OsServiceStatus>,
}

#[derive(Clone, Deserialize)]
struct OsServiceStatus {
    id: String,
    label: String,
    unit: String,
    binary: Option<String>,
    expected_state: String,
    state: String,
    unit_file: String,
    unit_file_present: bool,
    binary_path: Option<String>,
    binary_present: Option<bool>,
    detail: String,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct HardwareStatus {
    generated_at: String,
    source: String,
    platform: PlatformStatus,
    memory: MemoryStatus,
    #[serde(default)]
    storage: Vec<StorageVolume>,
    #[serde(default)]
    facilities: Vec<SystemFacility>,
}

#[cfg_attr(
    not(all(target_os = "linux", feature = "native-desktop")),
    allow(dead_code)
)]
#[derive(Clone, Deserialize)]
struct StorageVolume {
    id: String,
    mount_point: String,
    total_gb: u64,
    available_gb: u64,
}

#[derive(Clone, Deserialize)]
struct PlatformStatus {
    os: String,
    desktop: String,
    session_type: String,
    current_desktop: String,
}

#[derive(Clone, Deserialize)]
struct MemoryStatus {
    total_gb: u64,
    available_gb: u64,
}

#[derive(Clone, Deserialize)]
struct SystemFacility {
    id: String,
    label: String,
    state: String,
    detail: String,
    evidence: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct RecoveryStatus {
    generated_at: String,
    source: String,
    checks: Vec<RecoveryCheck>,
}

#[derive(Clone, Deserialize)]
struct RecoveryCheck {
    id: String,
    label: String,
    state: String,
    detail: String,
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
    model_dir: String,
    model_dir_available_gb: Option<u64>,
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
    id: String,
    name: String,
    role: String,
    source: String,
    weights_in_os_image: bool,
    download_required: bool,
    minimum_ram_gb: u64,
    minimum_gpu_vram_gb: Option<u64>,
    minimum_free_storage_gb: u64,
    disk_requirement: String,
    state: String,
    reasons: Vec<String>,
    install: LocalModelInstall,
}

#[derive(Clone, Deserialize)]
struct LocalModelInstall {
    state: String,
    consent_required: bool,
    consent_recorded: bool,
    manifest_required: bool,
    verification_required: bool,
    resumable: bool,
    state_path: String,
    target_dir: String,
    manifest_path: String,
    detail: String,
}

#[derive(Deserialize)]
struct LocalModelInstallOutcome {
    ok: bool,
    model_id: String,
    state: String,
    detail: String,
}

#[derive(Clone, Deserialize)]
struct ResidentStatus {
    generated_at: String,
    source: String,
    state_path: String,
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
    relay_contract: String,
}

#[derive(Clone, Deserialize)]
struct ResidentCapability {
    label: String,
    state: String,
    detail: String,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct AiActionCatalog {
    generated_at: String,
    source: String,
    registry_version: String,
    engine: AiEngineStatus,
    permission_model: String,
    actions: Vec<AiActionStatus>,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct AiEngineStatus {
    selected: String,
    ready: bool,
    detail: String,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct AiActionStatus {
    id: String,
    title: String,
    detail: String,
    kind: String,
    contexts: Vec<String>,
    permission: String,
    permission_control: String,
    confirmation: String,
    entrypoints: Vec<String>,
    route_hint: String,
    state: String,
    enabled: bool,
    reason: String,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct AiActionHistory {
    generated_at: u64,
    source: String,
    state_path: String,
    retention: String,
    events: Vec<AiActionHistoryEvent>,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct AiActionHistoryEvent {
    action_id: String,
    title: String,
    outcome: String,
    entrypoint: String,
    permission_control: String,
    confirmation: String,
    occurred_at: u64,
    detail: String,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct SettingsContextAiResponse {
    ok: bool,
    text: String,
    context: Option<SettingsContextAiSummary>,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct SettingsContextAiSummary {
    panel: String,
    topic: String,
    route_hint: String,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct PolicyStatus {
    generated_at: String,
    source: String,
    state_path: String,
    permission_path: String,
    profile: String,
    locked: bool,
    data_boundary: String,
    secret_boundary: String,
    controls: Vec<PolicyControl>,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct PolicyControl {
    id: String,
    label: String,
    state: String,
    profile_state: String,
    grant: Option<PolicyPermissionGrant>,
    detail: String,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
struct PolicyPermissionGrant {
    granted_at: String,
    acknowledgement: String,
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

fn main() -> SettingsResult<()> {
    let config = SettingsConfig::from_env();
    let core_ready = wait_for_core(&config.core_url, config.core_wait);
    let state = load_settings_state(&config, core_ready);

    println!("Goblins OS Settings started");
    println!("settings_panel={}", config.panel.as_str());
    println!("{}", settings_state_summary(&state));
    if settings_state_debug_enabled() {
        eprintln!("{}", settings_state_debug_summary(&state));
    }

    run_native_settings(config, state)
}

impl SettingsConfig {
    fn from_env() -> Self {
        Self {
            core_url: env::var("GOBLINS_OS_CORE_URL")
                .or_else(|_| env::var("OPENAI_OS_CORE_URL"))
                .unwrap_or_else(|_| DEFAULT_CORE_URL.into()),
            core_wait: Duration::from_secs(env_u64(
                "GOBLINS_OS_SETTINGS_CORE_WAIT_SECS",
                DEFAULT_CORE_WAIT_SECS,
            )),
            panel: SettingsPanel::from_args(env::args().skip(1)),
        }
    }
}

impl SettingsPanel {
    fn from_args(args: impl Iterator<Item = String>) -> Self {
        for arg in args {
            let panel_arg = arg.strip_prefix("--panel=").unwrap_or(&arg);
            let normalized = Self::normalize_arg(panel_arg);
            for panel in Self::ALL {
                if normalized == panel.as_str() {
                    return panel;
                }
            }

            for panel in Self::ALL {
                if panel.matches_normalized_alias(&normalized) {
                    return panel;
                }
            }
        }

        Self::Overview
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Appearance => "appearance",
            Self::Applications => "applications",
            Self::DesktopDock => "desktop-dock",
            Self::MenuBarControlCenter => "menu-bar-control-center",
            Self::Network => "network",
            Self::NetworkServices => "network-services",
            Self::Bluetooth => "bluetooth",
            Self::MobileBroadband => "mobile-broadband",
            Self::Sharing => "sharing",
            Self::Displays => "displays",
            Self::ColorManagement => "color-management",
            Self::Sound => "sound",
            Self::Keyboard => "keyboard",
            Self::MouseTrackpad => "mouse-trackpad",
            Self::DrawingTablet => "drawing-tablet",
            Self::Accessibility => "accessibility",
            Self::DesktopWallpaper => "desktop-wallpaper",
            Self::Notifications => "notifications",
            Self::LockScreen => "lock-screen",
            Self::SearchIndexing => "search",
            Self::Multitasking => "multitasking",
            Self::PowerBattery => "power-battery",
            Self::Games => "games",
            Self::PrintersScanners => "printers-scanners",
            Self::DateTime => "date-time",
            Self::LanguageRegion => "language-region",
            Self::UsersAccounts => "users-accounts",
            Self::OnlineAccounts => "online-accounts",
            Self::PrivacyPermissions => "privacy-permissions",
            Self::Security => "security",
            Self::Wellbeing => "wellbeing",
            Self::Models => "models",
            Self::Policy => "policy",
            Self::Storage => "storage",
            Self::UpdatesAbout => "updates-about",
            Self::Recovery => "recovery",
            Self::Developer => "developer",
        }
    }

    fn normalize_arg(arg: &str) -> String {
        arg.trim().to_ascii_lowercase().replace(['_', ' '], "-")
    }

    fn matches_normalized_alias(self, normalized: &str) -> bool {
        self.aliases()
            .iter()
            .any(|alias| normalized == alias.replace(['_', ' '], "-"))
    }

    fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Overview => &["home"],
            Self::Appearance => &["theme", "color-scheme"],
            Self::Applications => &["apps", "default apps", "app permissions"],
            Self::DesktopDock => &[
                "desktop",
                "dock",
                "desktop and dock",
                "app launcher",
                "window controls",
                "desktop surfaces",
            ],
            Self::MenuBarControlCenter => &[
                "menu bar",
                "control center",
                "top bar",
                "quick settings",
                "status menu",
                "system menu",
            ],
            Self::Network => &["wifi", "wi-fi", "proxy", "http proxy", "network proxy"],
            Self::NetworkServices => &["wired", "ethernet", "vpn", "advanced network"],
            Self::Bluetooth => &[],
            Self::MobileBroadband => &["wwan", "cellular", "modem", "mobile network"],
            Self::Sharing => &["remote desktop", "file sharing", "hostname sharing"],
            Self::Displays => &[
                "display",
                "screen",
                "monitor",
                "night-light",
                "night light",
                "brightness",
                "resolution",
                "scaling",
                "arrangement",
            ],
            Self::ColorManagement => &["color", "icc", "calibration", "profile"],
            Self::Sound => &["audio", "voice"],
            Self::Keyboard => &[],
            Self::MouseTrackpad => &["mouse", "trackpad", "pointer"],
            Self::DrawingTablet => &["wacom", "tablet", "stylus", "pen"],
            Self::Accessibility => &[
                "a11y",
                "screen-reader",
                "screen reader",
                "on-screen keyboard",
                "onscreen keyboard",
                "magnifier",
                "zoom",
                "text size",
                "reduce motion",
            ],
            Self::DesktopWallpaper => &[
                "desktop",
                "wallpaper",
                "background",
                "placement",
                "fit",
                "fill",
                "span",
                "center",
                "tile",
                "stretch",
            ],
            Self::Notifications => &[
                "alerts",
                "banners",
                "per-app",
                "notification sound",
                "lock screen",
            ],
            Self::LockScreen => &[
                "lock",
                "lock screen",
                "screen lock",
                "blank screen",
                "login screen",
                "notification privacy",
            ],
            Self::SearchIndexing => &["indexing", "file search", "search results"],
            Self::Multitasking => &["workspaces", "hot corner", "window switching"],
            Self::PowerBattery => &["power", "battery", "sleep", "energy"],
            Self::Games => &[
                "games",
                "gaming",
                "vulkan",
                "gamemode",
                "gamescope",
                "mangohud",
                "controller",
                "heroic",
                "lutris",
                "bottles",
                "umu",
                "proton",
            ],
            Self::PrintersScanners => &["printers", "scanners", "cups", "print"],
            Self::DateTime => &[
                "date",
                "time",
                "clock",
                "timezone",
                "time zone",
                "ntp",
                "calendar",
            ],
            Self::LanguageRegion => &[
                "language",
                "region",
                "locale",
                "formats",
                "input language",
                "keyboard language",
            ],
            Self::UsersAccounts => &[
                "users",
                "accounts",
                "account",
                "identity",
                "local user",
                "administrator",
                "hostname",
                "computer name",
            ],
            Self::OnlineAccounts => &["internet accounts", "cloud accounts", "mail accounts"],
            Self::PrivacyPermissions => &[
                "privacy",
                "permissions",
                "recent files",
                "app usage",
                "microphone",
                "camera",
                "usb",
            ],
            Self::Security => &[
                "security",
                "password",
                "firewall",
                "boot integrity",
                "secure storage",
                "secrets",
            ],
            Self::Wellbeing => &["screen time", "break reminders", "digital wellbeing"],
            Self::Models => &[
                "ai",
                "assistant",
                "ask goblins",
                "openai",
                "codex",
                "gpt-oss",
                "selected text",
                "screenshot",
                "context",
                "actions",
                "history",
                "audit",
                "action log",
            ],
            Self::Policy => &["data-boundary"],
            Self::Storage => &[
                "disk",
                "drives",
                "models-storage",
                "disk usage",
                "free space",
                "cleanup",
            ],
            Self::UpdatesAbout => &["updates", "about", "bootc"],
            Self::Recovery => &["services", "health"],
            Self::Developer => &[
                "developer",
                "diagnostics",
                "debug",
                "core",
                "logs",
                "system monitor",
                "processes",
            ],
        }
    }

    fn sidebar_icon_name(self) -> &'static str {
        match self {
            Self::Overview => "preferences-system-details-symbolic",
            Self::Appearance => "preferences-desktop-appearance-symbolic",
            Self::Applications => "preferences-desktop-apps-symbolic",
            Self::DesktopDock => "preferences-desktop-symbolic",
            Self::MenuBarControlCenter => "preferences-system-symbolic",
            Self::DesktopWallpaper => "preferences-desktop-wallpaper-symbolic",
            Self::Displays => "preferences-desktop-display-symbolic",
            Self::ColorManagement => "preferences-color-symbolic",
            Self::Sound => "audio-speakers-symbolic",
            Self::Notifications => "preferences-system-notifications-symbolic",
            Self::LockScreen => "system-lock-screen-symbolic",
            Self::SearchIndexing => "preferences-system-search-symbolic",
            Self::Multitasking => "preferences-desktop-multitasking-symbolic",
            Self::PowerBattery => "battery-good-symbolic",
            Self::Games => "applications-games-symbolic",
            Self::PrintersScanners => "printer-symbolic",
            Self::DateTime => "preferences-system-time-symbolic",
            Self::LanguageRegion => "preferences-desktop-locale-symbolic",
            Self::Wellbeing => "alarm-symbolic",
            Self::Network => "network-wireless-symbolic",
            Self::NetworkServices => "network-wired-symbolic",
            Self::Bluetooth => "bluetooth-symbolic",
            Self::MobileBroadband => "network-cellular-symbolic",
            Self::Sharing => "preferences-system-sharing-symbolic",
            Self::Keyboard => "preferences-desktop-keyboard-symbolic",
            Self::MouseTrackpad => "input-mouse-symbolic",
            Self::DrawingTablet => "input-tablet-symbolic",
            Self::Accessibility => "preferences-desktop-accessibility-symbolic",
            Self::UsersAccounts => "system-users-symbolic",
            Self::OnlineAccounts => "goa-panel-symbolic",
            Self::PrivacyPermissions => "preferences-system-privacy-symbolic",
            Self::Security => "security-high-symbolic",
            Self::Models => "system-run-symbolic",
            Self::Policy => "changes-prevent-symbolic",
            Self::Storage => "drive-harddisk-symbolic",
            Self::UpdatesAbout => "system-software-install-symbolic",
            Self::Recovery => "system-reboot-symbolic",
            Self::Developer => "applications-engineering-symbolic",
        }
    }

    /// The colored rounded icon-tile class for the sidebar — the macOS-kit
    /// "system settings" signature, translated to project-owned tints. Each
    /// category carries a calm, saturated tile with a white glyph; the CSS
    /// resolves the class to `@gos_tint_*` so light and dark both stay vivid.
    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    fn sidebar_icon_tint(self) -> &'static str {
        match self {
            Self::Network
            | Self::Bluetooth
            | Self::Sharing
            | Self::Displays
            | Self::LockScreen
            | Self::Accessibility
            | Self::OnlineAccounts
            | Self::PrivacyPermissions
            | Self::Security
            | Self::LanguageRegion
            | Self::Applications
            | Self::DesktopDock
            | Self::UpdatesAbout => "gos-tint-blue",
            Self::Sound | Self::DrawingTablet => "gos-tint-pink",
            Self::Notifications => "gos-tint-red",
            Self::DateTime | Self::Recovery | Self::Appearance => "gos-tint-orange",
            Self::Multitasking | Self::PowerBattery | Self::Games | Self::MobileBroadband => {
                "gos-tint-green"
            }
            Self::DesktopWallpaper | Self::Wellbeing => "gos-tint-teal",
            Self::ColorManagement | Self::UsersAccounts => "gos-tint-indigo",
            Self::Models => "gos-tint-purple",
            Self::Overview
            | Self::MenuBarControlCenter
            | Self::NetworkServices
            | Self::Keyboard
            | Self::MouseTrackpad
            | Self::SearchIndexing
            | Self::PrintersScanners
            | Self::Policy
            | Self::Storage
            | Self::Developer => "gos-tint-graphite",
        }
    }

    fn sidebar_owner_description(self) -> &'static str {
        if self.gnome_control_center_panel().is_some() {
            "Goblins OS shows live status here and opens built-in device controls when a deeper panel is needed."
        } else {
            "Goblins OS shows OS-owned controls or truthful read-only state."
        }
    }

    /// Human panel name for the sidebar nav.
    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    fn display_name(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Appearance => "Appearance",
            Self::Applications => "Applications",
            Self::DesktopDock => "Desktop & Dock",
            Self::MenuBarControlCenter => "Menu Bar & Control Center",
            Self::Network => "Network",
            Self::NetworkServices => "Wired & VPN",
            Self::Bluetooth => "Bluetooth",
            Self::MobileBroadband => "Mobile Broadband",
            Self::Sharing => "Sharing",
            Self::Displays => "Displays",
            Self::ColorManagement => "Color",
            Self::Sound => "Sound",
            Self::Keyboard => "Keyboard",
            Self::MouseTrackpad => "Mouse & Trackpad",
            Self::DrawingTablet => "Drawing Tablet",
            Self::Accessibility => "Accessibility",
            Self::DesktopWallpaper => "Desktop & Wallpaper",
            Self::Notifications => "Notifications",
            Self::LockScreen => "Lock Screen",
            Self::SearchIndexing => "Search",
            Self::Multitasking => "Multitasking",
            Self::PowerBattery => "Power & Battery",
            Self::Games => "Games",
            Self::PrintersScanners => "Printers & Scanners",
            Self::DateTime => "Date & Time",
            Self::LanguageRegion => "Language & Region",
            Self::UsersAccounts => "Users & Accounts",
            Self::OnlineAccounts => "Online Accounts",
            Self::PrivacyPermissions => "Privacy & Permissions",
            Self::Security => "Security",
            Self::Wellbeing => "Wellbeing",
            Self::Models => "Goblin & Models",
            Self::Policy => "Policy",
            Self::Storage => "Storage",
            Self::UpdatesAbout => "Updates & About",
            Self::Recovery => "Recovery",
            Self::Developer => "Diagnostics",
        }
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    fn sidebar_group(self) -> &'static str {
        match self {
            Self::Overview
            | Self::Appearance
            | Self::Applications
            | Self::DesktopDock
            | Self::MenuBarControlCenter
            | Self::DesktopWallpaper
            | Self::Displays
            | Self::ColorManagement
            | Self::Sound
            | Self::Notifications
            | Self::LockScreen
            | Self::SearchIndexing
            | Self::Multitasking
            | Self::PowerBattery
            | Self::Games
            | Self::PrintersScanners
            | Self::DateTime
            | Self::LanguageRegion
            | Self::Wellbeing => "System",
            Self::Network
            | Self::NetworkServices
            | Self::Bluetooth
            | Self::MobileBroadband
            | Self::Sharing => "Connectivity",
            Self::Keyboard | Self::MouseTrackpad | Self::DrawingTablet | Self::Accessibility => {
                "Input"
            }
            Self::UsersAccounts
            | Self::OnlineAccounts
            | Self::PrivacyPermissions
            | Self::Security
            | Self::Models
            | Self::Policy => "Accounts & Privacy",
            Self::Storage | Self::UpdatesAbout | Self::Recovery | Self::Developer => "Maintenance",
        }
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    fn summary(self) -> &'static str {
        match self {
            Self::Overview => {
                "Current session, system health, services, hardware, and Goblins AI runtime summary."
            }
            Self::Appearance => "Color scheme, Inter typography, and visual system preferences.",
            Self::Applications => {
                "Default applications, app permissions, and installed application behavior."
            }
            Self::DesktopDock => "Dock, app launcher, window controls, and desktop surface status.",
            Self::MenuBarControlCenter => {
                "Top bar, quick settings, status menus, clock, and control center readiness."
            }
            Self::Network => "Connectivity, active connection, and Wi-Fi management status.",
            Self::NetworkServices => "Wired, VPN, and advanced network services.",
            Self::Bluetooth => "Bluetooth readiness, adapter power, and device status.",
            Self::MobileBroadband => "Cellular, modem, and WWAN settings from system services.",
            Self::Sharing => "Remote desktop, file sharing, and device sharing settings.",
            Self::Displays => "Display session, connected outputs, scaling, and Night Light readiness.",
            Self::ColorManagement => "Display and device color profiles managed by the desktop.",
            Self::Sound => "Audio device readiness, volume, mute, system sounds, and local voice capability.",
            Self::Keyboard => "Keyboard readiness and input-device status.",
            Self::MouseTrackpad => "Pointer and touchpad readiness from Linux input devices.",
            Self::DrawingTablet => "Pen, stylus, and drawing tablet settings.",
            Self::Accessibility => {
                "Assistive technologies, motion, text scale, and accessibility readiness."
            }
            Self::DesktopWallpaper => "Desktop background, session theme, and wallpaper ownership.",
            Self::Notifications => {
                "Desktop notification banner, lock-screen notification, and notification registry preferences."
            }
            Self::LockScreen => "Screen lock, lock-screen privacy, blanking, and sign-in recovery.",
            Self::SearchIndexing => "Desktop search providers, indexing, and result visibility.",
            Self::Multitasking => "Workspace, hot corner, and window switching preferences.",
            Self::PowerBattery => "Power mode, battery, suspend, and energy settings.",
            Self::Games => {
                "Vulkan, GameMode, Gamescope, MangoHud, controllers, audio, Flatpak, and non-Steam launcher readiness."
            }
            Self::PrintersScanners => "Printer, scanner, and device settings.",
            Self::DateTime => "Clock, time zone, automatic time, and calendar format settings.",
            Self::LanguageRegion => "Language, region, formats, and input-language settings.",
            Self::UsersAccounts => "OpenAI account, local session identity, and sign-in state.",
            Self::OnlineAccounts => "Cloud and internet accounts provided by account services.",
            Self::PrivacyPermissions => {
                "Private mode, protected device access, credentials, and permission boundaries."
            }
            Self::Security => "Secret boundaries, the credential keyring, boot-image integrity, and secrets storage.",
            Self::Wellbeing => "Screen time, break reminders, and attention health settings.",
            Self::Models => {
                "Goblin actions, GPT-OSS, Codex, OpenAI API key, local models, and voice engine."
            }
            Self::Policy => "Consumer/business/enterprise policy profile and permission grants.",
            Self::Storage => "Model cache, OS state directories, mounted storage, and free space.",
            Self::UpdatesAbout => "OS image, tooling, identity, and update readiness.",
            Self::Recovery => "Service health, recovery checks, and repair readiness.",
            Self::Developer => "Local diagnostics, logs, service health, and device details.",
        }
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    fn search_text(self) -> String {
        format!(
            "{} {} {} {}",
            self.display_name(),
            self.summary(),
            self.as_str(),
            self.aliases().join(" ")
        )
        .to_ascii_lowercase()
    }

    fn gnome_control_center_panel(self) -> Option<&'static str> {
        match self {
            Self::Applications => Some("applications"),
            Self::LockScreen => Some("privacy"),
            Self::Network => Some("wifi"),
            Self::NetworkServices => Some("network"),
            Self::Bluetooth => Some("bluetooth"),
            Self::MobileBroadband => Some("wwan"),
            Self::Sharing => Some("sharing"),
            Self::Displays => Some("display"),
            Self::ColorManagement => Some("color"),
            Self::Sound => Some("sound"),
            Self::Keyboard => Some("keyboard"),
            Self::MouseTrackpad => Some("mouse"),
            Self::DrawingTablet => Some("wacom"),
            Self::Accessibility => Some("universal-access"),
            Self::DesktopWallpaper => Some("background"),
            Self::Notifications => Some("notifications"),
            Self::SearchIndexing => Some("search"),
            Self::Multitasking => Some("multitasking"),
            Self::PowerBattery => Some("power"),
            Self::PrintersScanners => Some("printers"),
            Self::DateTime => Some("datetime"),
            Self::LanguageRegion => Some("region"),
            Self::UsersAccounts | Self::UpdatesAbout => Some("system"),
            Self::OnlineAccounts => Some("online-accounts"),
            Self::PrivacyPermissions => Some("privacy"),
            Self::Wellbeing => Some("wellbeing"),
            Self::Overview
            | Self::Appearance
            | Self::DesktopDock
            | Self::MenuBarControlCenter
            | Self::Models
            | Self::Policy
            | Self::Security
            | Self::Games
            | Self::Storage
            | Self::Recovery
            | Self::Developer => None,
        }
    }

    const ALL: [Self; 38] = [
        Self::Overview,
        Self::Appearance,
        Self::Applications,
        Self::DesktopDock,
        Self::MenuBarControlCenter,
        Self::DesktopWallpaper,
        Self::Displays,
        Self::ColorManagement,
        Self::Sound,
        Self::Notifications,
        Self::LockScreen,
        Self::SearchIndexing,
        Self::Multitasking,
        Self::PowerBattery,
        Self::Games,
        Self::PrintersScanners,
        Self::DateTime,
        Self::LanguageRegion,
        Self::Wellbeing,
        Self::Network,
        Self::NetworkServices,
        Self::Bluetooth,
        Self::MobileBroadband,
        Self::Sharing,
        Self::Keyboard,
        Self::MouseTrackpad,
        Self::DrawingTablet,
        Self::Accessibility,
        Self::UsersAccounts,
        Self::OnlineAccounts,
        Self::PrivacyPermissions,
        Self::Security,
        Self::Models,
        Self::Policy,
        Self::Storage,
        Self::UpdatesAbout,
        Self::Recovery,
        Self::Developer,
    ];
}

fn load_settings_state(config: &SettingsConfig, core_ready: bool) -> SettingsState {
    if !core_ready {
        return SettingsState {
            core_ready,
            system: None,
            system_image: None,
            openai_auth: None,
            system_services: None,
            hardware: None,
            recovery: None,
            local_models: None,
            resident: None,
            ai_actions: None,
            ai_action_history: None,
            policy: None,
            openai_key: None,
            privacy: None,
            voice: None,
            codex: None,
            appearance: None,
            network: None,
            notifications: None,
            displays: None,
            bluetooth: None,
            audio: None,
            input: None,
            accessibility: None,
        };
    }

    SettingsState {
        core_ready,
        system: get_core_json(&config.core_url, "/v1/settings/system").ok(),
        system_image: get_core_json(&config.core_url, "/v1/system/image").ok(),
        openai_auth: get_core_json(&config.core_url, "/v1/auth/openai/status").ok(),
        system_services: get_core_json(&config.core_url, "/v1/system/services").ok(),
        hardware: get_core_json(&config.core_url, "/v1/system/hardware").ok(),
        recovery: get_core_json(&config.core_url, "/v1/recovery/status").ok(),
        local_models: get_core_json(&config.core_url, "/v1/local-models").ok(),
        resident: get_core_json(&config.core_url, "/v1/ai/runtime/status").ok(),
        ai_actions: get_core_json(&config.core_url, "/v1/ai/actions").ok(),
        ai_action_history: get_core_json(&config.core_url, "/v1/ai/action-history").ok(),
        policy: get_core_json(&config.core_url, "/v1/policy/status").ok(),
        openai_key: get_core_json(&config.core_url, "/v1/models/openai-key").ok(),
        privacy: get_core_json(&config.core_url, "/v1/privacy/status").ok(),
        voice: get_core_json(&config.core_url, "/v1/voice/status").ok(),
        codex: get_core_json(&config.core_url, "/v1/codex/status").ok(),
        appearance: get_core_json(&config.core_url, "/v1/appearance/status").ok(),
        network: get_core_json(&config.core_url, "/v1/network/status").ok(),
        notifications: get_core_json(&config.core_url, "/v1/notifications/status").ok(),
        displays: get_core_json(&config.core_url, "/v1/displays/status").ok(),
        bluetooth: get_core_json(&config.core_url, "/v1/bluetooth/status").ok(),
        audio: get_core_json(&config.core_url, "/v1/audio/status").ok(),
        input: get_core_json(&config.core_url, "/v1/input/status").ok(),
        accessibility: get_core_json(&config.core_url, "/v1/accessibility/status").ok(),
    }
}

fn settings_state_debug_enabled() -> bool {
    env::var("GOBLINS_OS_SETTINGS_DEBUG_STATE")
        .map(|value| {
            matches!(
                value.as_str(),
                "1" | "true" | "TRUE" | "yes" | "on" | "full"
            )
        })
        .unwrap_or(false)
}

fn settings_state_summary(state: &SettingsState) -> String {
    let account = state
        .openai_auth
        .as_ref()
        .map(|auth| {
            if auth.authenticated {
                "signed-in"
            } else if auth.configured {
                "provider-ready"
            } else {
                "local-only"
            }
        })
        .unwrap_or("unavailable");
    let policy = state
        .policy
        .as_ref()
        .map(|policy| {
            if policy.locked {
                "locked"
            } else {
                policy.profile.as_str()
            }
        })
        .unwrap_or("unavailable");
    let models = state
        .local_models
        .as_ref()
        .map(|catalog| {
            let installed = catalog
                .models
                .iter()
                .filter(|model| model.install.state == "installed")
                .count();
            let ready = catalog
                .models
                .iter()
                .filter(|model| model.state == "installable" || model.install.state == "installed")
                .count();
            let blocked = catalog
                .models
                .iter()
                .filter(|model| model.state == "blocked")
                .count();
            format!("installed:{installed},ready:{ready},blocked:{blocked}")
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let ai = state
        .ai_actions
        .as_ref()
        .map(|catalog| {
            let ready = catalog
                .actions
                .iter()
                .filter(|action| action.state == "ready" || action.state == "confirmation-required")
                .count();
            format!(
                "{}:{ready}/{}",
                if catalog.engine.ready {
                    "ready"
                } else {
                    "waiting"
                },
                catalog.actions.len()
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let ai_history = state
        .ai_action_history
        .as_ref()
        .map(|history| history.events.len().to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let network = state
        .network
        .as_ref()
        .map(|network| {
            if !network.manager_available {
                "unavailable"
            } else if network.online {
                "online"
            } else {
                network.connectivity.as_str()
            }
        })
        .unwrap_or("unavailable");
    let bluetooth = state
        .bluetooth
        .as_ref()
        .map(|bluetooth| {
            if !bluetooth.bluez_available {
                "unavailable"
            } else if !bluetooth.service_active {
                "service-waiting"
            } else if bluetooth.adapter_present {
                "adapter-ready"
            } else {
                "adapter-waiting"
            }
        })
        .unwrap_or("unavailable");
    let audio = state
        .audio
        .as_ref()
        .map(|audio| {
            if audio.output.available || audio.input.available {
                "ready"
            } else if audio.wireplumber_available {
                "device-waiting"
            } else {
                "unavailable"
            }
        })
        .unwrap_or("unavailable");
    let input = state
        .input
        .as_ref()
        .map(|input| {
            if input.gsettings_available
                && input.keyboard.schema_available
                && input.mouse.schema_available
                && input.touchpad.schema_available
            {
                "ready"
            } else {
                "partial"
            }
        })
        .unwrap_or("unavailable");
    let accessibility = state
        .accessibility
        .as_ref()
        .map(|accessibility| {
            if accessibility.gsettings_available
                && accessibility.interface.schema_available
                && accessibility.assistive.schema_available
                && accessibility.display_comfort.schema_available
            {
                "ready"
            } else {
                "partial"
            }
        })
        .unwrap_or("unavailable");

    format!(
        "settings_state=core:{} account:{} policy:{} ai:{} history:{} models:{} network:{} bluetooth:{} audio:{} input:{} accessibility:{}",
        if state.core_ready { "ready" } else { "waiting" },
        account,
        policy,
        ai,
        ai_history,
        models,
        network,
        bluetooth,
        audio,
        input,
        accessibility
    )
}

fn settings_state_debug_summary(state: &SettingsState) -> String {
    let system = state
       .system
       .as_ref()
       .map(|system| {
            format!(
                "{}:{} session={}/{}/{} core={} identity=provider:{} account:{} session-path={} storage=models:{} installer:{} session:{} policy:{} resident:{} secrets:{} services=bootc-image:{} bootc:{} systemctl:{} network:{}",
                system.source,
                system.generated_at,
                system.session.desktop,
                system.session.gui_platform,
                system.session.shell_mode,
                system.session.core_url,
                system.identity.provider_configured,
                system.identity.account_authenticated,
                system.identity.session_path,
                system.storage.model_dir,
                system.storage.installer_state_dir,
                system.storage.session_state_dir,
                system.storage.policy_state_dir,
                system.storage.resident_state_dir,
                system.storage.secrets_dir,
                system.services.bootc_image,
                system.services.bootc_available,
                system.services.systemctl_available,
                system.services.network_manager_available
            )
        })
       .unwrap_or_else(|| "unavailable".to_string());
    let openai_auth = state
        .openai_auth
        .as_ref()
        .map(|auth| {
            format!(
                "configured={} authenticated={} provider={} storage={} message={}",
                auth.configured,
                auth.authenticated,
                auth.provider,
                auth.session_storage,
                auth.message
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let system_services = state
        .system_services
        .as_ref()
        .map(|status| {
            let first = status
                .services
                .first()
                .map(|service| {
                    format!(
                        "{}:{}:{}:{}:{}:{}:{}:{:?}:{:?}:{:?}:{}",
                        service.id,
                        service.label,
                        service.unit,
                        service.expected_state,
                        service.state,
                        service.unit_file,
                        service.unit_file_present,
                        service.binary,
                        service.binary_path,
                        service.binary_present,
                        service.detail
                    )
                })
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{}:{} manager={} unit-dir={} libexec={} services={} first=[{}]",
                status.source,
                status.generated_at,
                status.manager_available,
                status.unit_dir,
                status.libexec_dir,
                status.services.len(),
                first
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let hardware = state
        .hardware
        .as_ref()
        .map(|hardware| {
            let first = hardware
                .facilities
                .first()
                .map(|facility| {
                    format!(
                        "{}:{}:{}:{}:{}",
                        facility.id,
                        facility.label,
                        facility.state,
                        facility.detail,
                        facility.evidence.join("|")
                    )
                })
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{}:{} platform={}/{}/{}/{} memory={}/{}GB facilities={} first=[{}]",
                hardware.source,
                hardware.generated_at,
                hardware.platform.os,
                hardware.platform.desktop,
                hardware.platform.session_type,
                hardware.platform.current_desktop,
                hardware.memory.available_gb,
                hardware.memory.total_gb,
                hardware.facilities.len(),
                first
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let recovery = state
        .recovery
        .as_ref()
        .map(|recovery| {
            let first_check = recovery
                .checks
                .first()
                .map(|check| {
                    format!(
                        "{}:{}:{}:{}",
                        check.id, check.label, check.state, check.detail
                    )
                })
                .unwrap_or_else(|| "none".to_string());

            format!(
                "{}:{} checks={} first=[{}]",
                recovery.source,
                recovery.generated_at,
                recovery.checks.len(),
                first_check
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let local_models = state
        .local_models
        .as_ref()
        .map(|catalog| {
            let first = catalog
                .models
                .first()
                .map(|model| {
                    format!(
                        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
                        model.id,
                        model.name,
                        model.source,
                        model.state,
                        model.install.state,
                        model.install.consent_required,
                        model.install.consent_recorded,
                        model.install.manifest_required,
                        model.install.verification_required,
                        model.install.resumable,
                        model.install.state_path,
                        model.install.target_dir,
                        model.install.manifest_path,
                        model.install.detail
                    )
                })
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{} hardware={}GB vram={} storage={} dir={} runtime={} first=[{}]",
                catalog.install_policy,
                catalog.hardware.ram_gb,
                catalog
                    .hardware
                    .gpu_vram_gb
                    .map(|vram| format!("{vram}GB"))
                    .unwrap_or_else(|| "not-detected".to_string()),
                catalog
                    .hardware
                    .model_dir_available_gb
                    .map(|gb| format!("{gb}GB"))
                    .unwrap_or_else(|| "unknown".to_string()),
                catalog.hardware.model_dir,
                runtime_label(&catalog.hardware.runtime),
                first
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let resident = state
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
                "{}:{} path={} process={}:{}:{}:{} engine={}:{}:{} contract={} capabilities={} first_capability=[{}]",
                resident.source,
                resident.generated_at,
                resident.state_path,
                resident.process.state,
                resident.process.mode,
                resident
                   .process
                   .heartbeat_age_secs
                   .map(|age| format!("{age}s"))
                   .unwrap_or_else(|| "waiting".to_string()),
                resident.process.detail,
                resident.engine.selected,
                resident.engine.cloud_relay_configured,
                resident.engine.local_relay_configured,
                resident.engine.relay_contract,
                resident.capabilities.len(),
                first_capability
            )
        })
       .unwrap_or_else(|| "unavailable".to_string());
    let ai_actions = state
        .ai_actions
        .as_ref()
        .map(|catalog| {
            let first = catalog
                .actions
                .first()
                .map(|action| {
                    format!(
                        "{}:{}:{}:{}:{}:{}:{}:{}:{}",
                        action.id,
                        action.title,
                        action.kind,
                        action.permission,
                        action.permission_control,
                        action.confirmation,
                        action.route_hint,
                        action.state,
                        action.reason
                    )
                })
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{}:{} version={} engine={}:{} model={} actions={} first=[{}]",
                catalog.source,
                catalog.generated_at,
                catalog.registry_version,
                catalog.engine.selected,
                catalog.engine.ready,
                catalog.permission_model,
                catalog.actions.len(),
                first
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let policy = state
        .policy
        .as_ref()
        .map(|policy| {
            let first_control = policy
                .controls
                .first()
                .map(|control| {
                    format!(
                        "{}:{}:{}:{}",
                        control.id, control.label, control.state, control.detail
                    )
                })
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{}:{} path={} profile={} locked={} data={} secrets={} controls={} first=[{}]",
                policy.source,
                policy.generated_at,
                policy.state_path,
                policy.profile,
                policy.locked,
                policy.data_boundary,
                policy.secret_boundary,
                policy.controls.len(),
                first_control
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let openai_key = state
        .openai_key
        .as_ref()
        .map(|status| {
            let surfaces = format!(
                "{} voice={} codex={}",
                state
                    .privacy
                    .as_ref()
                    .map(|privacy| format!("offline={}:{}", privacy.offline, privacy.detail))
                    .unwrap_or_else(|| "unavailable".to_string()),
                state
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
                    .unwrap_or_else(|| "unavailable".to_string()),
                state
                    .codex
                    .as_ref()
                    .map(|codex| {
                        format!(
                            "installed={}:auth={}:{}",
                            codex.installed, codex.authenticated, codex.detail
                        )
                    })
                    .unwrap_or_else(|| "unavailable".to_string())
            );
            format!(
                "configured={} engine={} engine_selected={} model={} storage={} privacy={}",
                status.configured,
                status.engine,
                status.engine_selected,
                status.model,
                status.storage,
                surfaces
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let appearance = state
        .appearance
        .as_ref()
        .map(|appearance| {
            format!(
                "{} gsettings={} color-scheme-key={} scheme={} theme={}",
                appearance.source,
                appearance.gsettings_available,
                appearance.color_scheme_available,
                appearance.color_scheme,
                appearance.theme
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let network = state
        .network
        .as_ref()
        .map(|network| {
            format!(
                "{} manager={} online={} state={} connectivity={} active={}",
                network.source,
                network.manager_available,
                network.online,
                network.state,
                network.connectivity,
                network
                    .active
                    .as_ref()
                    .map(|active| format!("{}:{}:{}", active.name, active.kind, active.device))
                    .unwrap_or_else(|| "none".to_string())
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let bluetooth = state
        .bluetooth
        .as_ref()
        .map(|bluetooth| {
            format!(
                "{} bluez={} service={} adapter={} powered={} discoverable={} pairable={}",
                bluetooth.source,
                bluetooth.bluez_available,
                bluetooth.service_active,
                bluetooth.adapter_present,
                option_bool_word(bluetooth.powered),
                option_bool_word(bluetooth.discoverable),
                option_bool_word(bluetooth.pairable)
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let audio = state
        .audio
        .as_ref()
        .map(|audio| {
            format!(
                "{} wireplumber={} output={} input={}",
                audio.source,
                audio.wireplumber_available,
                audio_endpoint_summary(&audio.output),
                audio_endpoint_summary(&audio.input)
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let input = state
        .input
        .as_ref()
        .map(|input| {
            format!(
                "{} gsettings={} keyboard={} mouse={} touchpad={}",
                input.source,
                input.gsettings_available,
                input.keyboard.schema_available,
                input.mouse.schema_available,
                input.touchpad.schema_available
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    format!(
        "settings_state=core:{} system=[{}] openai_auth=[{}] system_services=[{}] hardware=[{}] recovery=[{}] local_models=[{}] resident=[{}] ai_actions=[{}] policy=[{}] openai_key=[{}] appearance=[{}] network=[{}] bluetooth=[{}] audio=[{}] input=[{}]",
        if state.core_ready { "ready" } else { "waiting" },
        system,
        openai_auth,
        system_services,
        hardware,
        recovery,
        local_models,
        resident,
        ai_actions,
        policy,
        openai_key,
        appearance,
        network,
        bluetooth,
        audio,
        input
    )
}

fn audio_endpoint_summary(endpoint: &AudioEndpointStatus) -> String {
    format!(
        "available={} volume={} muted={}",
        endpoint.available,
        endpoint
            .volume_percent
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        option_bool_word(endpoint.muted)
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_native_settings(config: SettingsConfig, state: SettingsState) -> SettingsResult<()> {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.Settings")
        .build();

    application.connect_activate(move |app| {
        goblins_os_ui::init_theming(GOBLINS_OS_SETTINGS_CSS);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(window_title(config.panel).as_str())
            .decorated(false)
            .default_width(SETTINGS_DEFAULT_WIDTH)
            .default_height(SETTINGS_DEFAULT_HEIGHT)
            .build();

        window.add_css_class("gos-settings-window");
        window.set_child(Some(&build_settings(&config, &state, &window)));
        window.present();
    });

    // Run GTK with a neutral argv: Goblins OS has already parsed its own
    // `--panel=` selector, and GTK would otherwise reject that custom flag.
    application.run_with_args(&["goblins-os-settings"]);
    Ok(())
}

/// A panel-specific window title. The overview keeps the bare title; other
/// panels add a suffix so each window is unambiguously addressable (the headless
/// render harness searches by title, and a shared X server must never match a
/// sibling settings window).
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn window_title(panel: SettingsPanel) -> String {
    match panel {
        SettingsPanel::Overview => "Goblins OS Settings".to_string(),
        SettingsPanel::Models => "Goblins OS Settings - AI & Models".to_string(),
        panel => format!("Goblins OS Settings - {}", panel.display_name()),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_settings(
    config: &SettingsConfig,
    state: &SettingsState,
    window: &gtk4::ApplicationWindow,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-settings-root");

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    top.add_css_class("gos-settings-top");
    top.append(&goblins_os_ui::window_controls(window));
    top.append(&goblins_os_ui::themed_brand_mark(22));
    let brand = label("Goblins OS", &["gos-brand"]);
    brand.set_wrap(false);
    top.append(&brand);
    top.append(&label("Settings", &["gos-muted"]));
    top.append(&spacer());
    root.append(&top);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.add_css_class("gos-settings-body");
    body.set_vexpand(true);

    let main = gtk::Box::new(gtk::Orientation::Vertical, 8);
    main.add_css_class("gos-main-panel");
    populate_panel(&main, config.panel, config, state);
    let main_scroll = gtk::ScrolledWindow::new();
    main_scroll.add_css_class("gos-main-scroll");
    main_scroll.set_child(Some(&main));
    main_scroll.set_hexpand(true);
    main_scroll.set_vexpand(true);
    main_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    main_scroll.set_propagate_natural_height(false);

    let side = gtk::Box::new(gtk::Orientation::Vertical, 10);
    side.add_css_class("gos-side-panel");

    let search = gtk::SearchEntry::new();
    search.add_css_class("gos-search-entry");
    search.set_placeholder_text(Some("Search settings"));
    search.set_tooltip_text(Some("Search settings, controls, and categories"));
    set_accessible_label_description(
        &search,
        "Search settings",
        "Search settings, controls, and categories. Press Enter to open the first visible result.",
    );
    side.append(&search);
    let search_status = label("", &["gos-search-status"]);
    search_status.set_visible(false);
    set_accessible_label_description(
        &search_status,
        "Search results",
        "No active settings search.",
    );
    side.append(&search_status);

    // A real nav: every category is a row, grouped like a system settings app,
    // the open one highlighted; clicking swaps the main panel in place.
    let nav_scroll = gtk::ScrolledWindow::new();
    nav_scroll.add_css_class("gos-side-scroll");
    nav_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    nav_scroll.set_overlay_scrolling(false);
    nav_scroll.set_vexpand(true);
    let nav = gtk::Box::new(gtk::Orientation::Vertical, 2);
    nav.add_css_class("gos-side-nav-list");
    let nav_rows: Rc<RefCell<Vec<(SettingsPanel, gtk::Button)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let nav_groups: Rc<RefCell<Vec<(&'static str, gtk::Label)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let nav_match_labels: Rc<RefCell<Vec<(SettingsPanel, gtk::Label)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let current_panel = Rc::new(Cell::new(config.panel));
    let sidebar_context = SidebarNavigationContext {
        nav_rows: nav_rows.clone(),
        current_panel: current_panel.clone(),
        main: main.clone(),
        config: config.clone(),
        state: state.clone(),
        window: window.clone(),
    };
    let mut current_group = "";
    for panel in SettingsPanel::ALL {
        let group = panel.sidebar_group();
        if group != current_group {
            let group_label = label(group, &["gos-nav-group"]);
            nav_groups.borrow_mut().push((group, group_label.clone()));
            nav.append(&group_label);
            current_group = group;
        }
        let row_content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row_content.add_css_class("gos-side-row-content");
        row_content.set_valign(gtk::Align::Center);
        row_content.set_hexpand(true);

        let icon_well = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        icon_well.add_css_class("gos-side-icon-well");
        icon_well.add_css_class(panel.sidebar_icon_tint());
        let icon = gtk::Image::from_icon_name(panel.sidebar_icon_name());
        icon.add_css_class("gos-side-icon");
        icon_well.append(&icon);
        row_content.append(&icon_well);

        let text_stack = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text_stack.add_css_class("gos-side-text-stack");
        text_stack.set_hexpand(true);

        let text = gtk::Label::new(Some(panel.display_name()));
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.add_css_class("gos-side-nav-label");
        text_stack.append(&text);

        let match_label = gtk::Label::new(None);
        match_label.set_xalign(0.0);
        match_label.set_wrap(false);
        match_label.set_visible(false);
        match_label.add_css_class("gos-side-match-label");
        text_stack.append(&match_label);
        row_content.append(&text_stack);

        let row = gtk::Button::new();
        row.set_child(Some(&row_content));
        row.add_css_class("gos-side-nav");
        let sidebar_detail = format!("{} {}", panel.summary(), panel.sidebar_owner_description());
        row.set_tooltip_text(Some(&sidebar_detail));
        let selected = panel == config.panel;
        set_accessible_label_description(
            &row,
            &sidebar_accessible_label(panel, selected),
            &sidebar_accessible_description(panel, selected),
        );
        row.update_property(&[gtk4::accessible::Property::KeyShortcuts("Up Down Home End")]);
        if selected {
            row.add_css_class("is-current");
        }
        {
            let nav_rows = nav_rows.clone();
            let main = main.clone();
            let config = config.clone();
            let state = state.clone();
            let window = window.clone();
            let current_panel = current_panel.clone();
            row.connect_clicked(move |_| {
                select_sidebar_panel(
                    panel,
                    &nav_rows,
                    &current_panel,
                    &main,
                    &config,
                    &state,
                    &window,
                );
            });
        }
        install_sidebar_row_keyboard_navigation(&row, panel, &sidebar_context);
        nav_rows.borrow_mut().push((panel, row.clone()));
        nav_match_labels
            .borrow_mut()
            .push((panel, match_label.clone()));
        nav.append(&row);
    }
    let no_results = system_row(
        "No results",
        "Try Wi-Fi, private mode, OpenAI key, Night Light, volume, keyboard, updates, or recovery.",
    );
    no_results.add_css_class("gos-search-empty");
    no_results.set_visible(false);
    nav.append(&no_results);
    let first_search_match: Rc<RefCell<Option<SettingsPanel>>> = Rc::new(RefCell::new(None));
    {
        let nav_rows = nav_rows.clone();
        let nav_groups = nav_groups.clone();
        let nav_match_labels = nav_match_labels.clone();
        let no_results = no_results.clone();
        let search_status = search_status.clone();
        let first_search_match = first_search_match.clone();
        let current_panel = current_panel.clone();
        search.connect_search_changed(move |entry| {
            let entry_text = entry.text();
            let query = settings_search_query(entry_text.as_str());
            let visible_matches = settings_search_visible_panels(&query);
            let result_count = visible_matches.len();
            let first_match = settings_search_first_match(&query);

            {
                let rows = nav_rows.borrow();
                for (panel, row) in rows.iter() {
                    let matches = visible_matches.contains(panel);
                    let selected = *panel == current_panel.get();
                    row.set_visible(matches);
                    set_accessible_label_description(
                        row,
                        &sidebar_accessible_label(*panel, selected),
                        &sidebar_accessible_description_for_search(*panel, selected, &query),
                    );
                }

                for (group, group_label) in nav_groups.borrow().iter() {
                    let group_visible = query.is_empty()
                        || visible_matches
                            .iter()
                            .any(|panel| panel.sidebar_group() == *group);
                    group_label.set_visible(group_visible);
                }
            }

            for (panel, match_label) in nav_match_labels.borrow().iter() {
                if let Some(preview) = settings_search_preview(*panel, &query) {
                    match_label.set_text(&preview);
                    match_label.set_visible(true);
                } else {
                    match_label.set_text("");
                    match_label.set_visible(false);
                }
            }

            no_results.set_visible(!query.is_empty() && result_count == 0);
            let status_text = settings_search_status_text(&query, result_count);
            search_status.set_visible(status_text.is_some());
            if let Some(status_text) = status_text {
                search_status.set_text(&status_text);
                set_accessible_label_description(&search_status, "Search results", &status_text);
            } else {
                search_status.set_text("");
                set_accessible_label_description(
                    &search_status,
                    "Search results",
                    "No active settings search.",
                );
            }
            entry.update_property(&[gtk4::accessible::Property::Description(
                &settings_search_accessible_description(&query, result_count),
            )]);
            *first_search_match.borrow_mut() = first_match;
        });
    }
    {
        let nav_rows = nav_rows.clone();
        let main = main.clone();
        let config = config.clone();
        let state = state.clone();
        let window = window.clone();
        let first_search_match = first_search_match.clone();
        let current_panel = current_panel.clone();
        search.connect_activate(move |entry| {
            let query = settings_search_query(entry.text().as_str());
            let target = settings_search_first_match(&query).or(*first_search_match.borrow());
            if let Some(panel) = target {
                select_sidebar_panel(
                    panel,
                    &nav_rows,
                    &current_panel,
                    &main,
                    &config,
                    &state,
                    &window,
                );
                focus_sidebar_panel(&nav_rows, panel);
            }
        });
    }
    install_search_entry_keyboard(&search, &nav_rows, &first_search_match);
    nav_scroll.set_child(Some(&nav));
    side.append(&nav_scroll);
    scroll_sidebar_panel_into_view(&nav_scroll, &nav, &nav_rows, config.panel);
    body.append(&side);

    body.append(&main_scroll);
    root.append(&body);
    install_settings_keyboard_shortcuts(
        window,
        &search,
        &nav_rows,
        &current_panel,
        &main,
        config,
        state,
    );
    root
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_search_entry_keyboard(
    search: &gtk4::SearchEntry,
    nav_rows: &Rc<RefCell<Vec<(SettingsPanel, gtk4::Button)>>>,
    first_search_match: &Rc<RefCell<Option<SettingsPanel>>>,
) {
    use gtk4::prelude::*;

    let keys = gtk4::EventControllerKey::new();
    keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let search_entry = search.clone();
    let nav_rows = nav_rows.clone();
    let first_search_match = first_search_match.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        let key_name = key.name().map(|name| name.to_string().to_ascii_lowercase());
        if key_name.as_deref() == Some("escape") && !search_entry.text().is_empty() {
            search_entry.set_text("");
            return gtk4::glib::Propagation::Stop;
        }
        if matches!(key_name.as_deref(), Some("down" | "kp_down")) {
            if let Some(panel) = *first_search_match.borrow() {
                focus_sidebar_panel(&nav_rows, panel);
                return gtk4::glib::Propagation::Stop;
            }
        }

        gtk4::glib::Propagation::Proceed
    });
    search.add_controller(keys);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn scroll_sidebar_panel_into_view(
    nav_scroll: &gtk4::ScrolledWindow,
    nav: &gtk4::Box,
    nav_rows: &Rc<RefCell<Vec<(SettingsPanel, gtk4::Button)>>>,
    panel: SettingsPanel,
) {
    let nav_scroll = nav_scroll.clone();
    let nav = nav.clone();
    let nav_rows = nav_rows.clone();

    gtk4::glib::idle_add_local_once(move || {
        use gtk4::prelude::*;

        let Some(row) = nav_rows
            .borrow()
            .iter()
            .find(|(candidate, _)| *candidate == panel)
            .map(|(_, row)| row.clone())
        else {
            return;
        };

        let Some(bounds) = row.compute_bounds(&nav) else {
            return;
        };

        let adjustment = nav_scroll.vadjustment();
        let row_top = f64::from(bounds.y());
        let row_bottom = row_top + f64::from(bounds.height());
        if let Some(target) = sidebar_scroll_target(
            row_top,
            row_bottom,
            adjustment.value(),
            adjustment.page_size(),
            adjustment.lower(),
            adjustment.upper(),
        ) {
            adjustment.set_value(target);
        }
    });
}

fn sidebar_scroll_target(
    row_top: f64,
    row_bottom: f64,
    viewport_top: f64,
    page_size: f64,
    lower: f64,
    upper: f64,
) -> Option<f64> {
    if page_size <= 0.0 || row_bottom <= row_top {
        return None;
    }

    let inset = 8.0;
    let viewport_bottom = viewport_top + page_size;
    if row_top >= viewport_top + inset && row_bottom <= viewport_bottom - inset {
        return None;
    }

    let breathing_room = (page_size * 0.32).clamp(64.0, 168.0);
    let target = row_top - breathing_room;
    let max_value = (upper - page_size).max(lower);
    Some(target.clamp(lower, max_value))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn select_sidebar_panel(
    panel: SettingsPanel,
    nav_rows: &Rc<RefCell<Vec<(SettingsPanel, gtk4::Button)>>>,
    current_panel: &Rc<Cell<SettingsPanel>>,
    main: &gtk4::Box,
    config: &SettingsConfig,
    state: &SettingsState,
    window: &gtk4::ApplicationWindow,
) {
    use gtk4::prelude::*;

    current_panel.set(panel);
    for (candidate, row) in nav_rows.borrow().iter() {
        let selected = *candidate == panel;
        if selected {
            row.add_css_class("is-current");
        } else {
            row.remove_css_class("is-current");
        }
        set_accessible_label_description(
            row,
            &sidebar_accessible_label(*candidate, selected),
            &sidebar_accessible_description(*candidate, selected),
        );
    }
    window.set_title(Some(window_title(panel).as_str()));
    populate_panel(main, panel, config, state);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
struct SidebarNavigationContext {
    nav_rows: Rc<RefCell<Vec<(SettingsPanel, gtk4::Button)>>>,
    current_panel: Rc<Cell<SettingsPanel>>,
    main: gtk4::Box,
    config: SettingsConfig,
    state: SettingsState,
    window: gtk4::ApplicationWindow,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_settings_keyboard_shortcuts(
    window: &gtk4::ApplicationWindow,
    search: &gtk4::SearchEntry,
    nav_rows: &Rc<RefCell<Vec<(SettingsPanel, gtk4::Button)>>>,
    current_panel: &Rc<Cell<SettingsPanel>>,
    main: &gtk4::Box,
    config: &SettingsConfig,
    state: &SettingsState,
) {
    use gtk4::prelude::*;

    let shortcuts = gtk4::EventControllerKey::new();
    let search_shortcut = search.clone();
    let nav_rows = nav_rows.clone();
    let current_panel = current_panel.clone();
    let main = main.clone();
    let config = config.clone();
    let state = state.clone();
    let window_for_selection = window.clone();
    shortcuts.connect_key_pressed(move |_, key, _, modifiers| {
        let key_name = key.name().map(|name| name.to_string().to_ascii_lowercase());
        if key_name.as_deref() == Some("escape") && !search_shortcut.text().is_empty() {
            search_shortcut.set_text("");
            search_shortcut.grab_focus();
            return gtk4::glib::Propagation::Stop;
        }

        if matches!(key_name.as_deref(), Some("return" | "kp_enter" | "enter"))
            && search_shortcut.has_focus()
            && !search_shortcut.text().is_empty()
        {
            let query = settings_search_query(search_shortcut.text().as_str());
            if let Some(panel) = settings_search_first_match(&query) {
                select_sidebar_panel(
                    panel,
                    &nav_rows,
                    &current_panel,
                    &main,
                    &config,
                    &state,
                    &window_for_selection,
                );
                focus_sidebar_panel(&nav_rows, panel);
                return gtk4::glib::Propagation::Stop;
            }
        }

        if !modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
            return gtk4::glib::Propagation::Proceed;
        }

        match key_name.as_deref() {
            Some("f") => {
                search_shortcut.grab_focus();
                gtk4::glib::Propagation::Stop
            }
            Some("l") => {
                let active_panel = current_panel.get();
                if let Some((_, row)) = nav_rows
                    .borrow()
                    .iter()
                    .find(|(panel, _)| *panel == active_panel)
                {
                    row.grab_focus();
                    gtk4::glib::Propagation::Stop
                } else {
                    gtk4::glib::Propagation::Proceed
                }
            }
            _ => gtk4::glib::Propagation::Proceed,
        }
    });
    window.add_controller(shortcuts);
    search.update_property(&[gtk4::accessible::Property::KeyShortcuts(
        "Ctrl+F Enter Down Escape",
    )]);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_sidebar_row_keyboard_navigation(
    row: &gtk4::Button,
    panel: SettingsPanel,
    context: &SidebarNavigationContext,
) {
    use gtk4::prelude::*;

    let keys = gtk4::EventControllerKey::new();
    let nav_rows = context.nav_rows.clone();
    let current_panel = context.current_panel.clone();
    let main = context.main.clone();
    let config = context.config.clone();
    let state = context.state.clone();
    let window = context.window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        let key_name = key.name().map(|name| name.to_string().to_ascii_lowercase());
        let Some(movement) = key_name.as_deref().and_then(sidebar_movement_from_key_name) else {
            return gtk4::glib::Propagation::Proceed;
        };
        let visible = visible_sidebar_panels(&nav_rows);
        let active_panel = if visible.contains(&panel) {
            panel
        } else if visible.contains(&current_panel.get()) {
            current_panel.get()
        } else {
            panel
        };
        let Some(target_panel) = sidebar_keyboard_target(&visible, active_panel, movement) else {
            return gtk4::glib::Propagation::Proceed;
        };

        select_sidebar_panel(
            target_panel,
            &nav_rows,
            &current_panel,
            &main,
            &config,
            &state,
            &window,
        );
        focus_sidebar_panel(&nav_rows, target_panel);
        gtk4::glib::Propagation::Stop
    });
    row.add_controller(keys);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn visible_sidebar_panels(
    nav_rows: &Rc<RefCell<Vec<(SettingsPanel, gtk4::Button)>>>,
) -> Vec<SettingsPanel> {
    use gtk4::prelude::*;

    nav_rows
        .borrow()
        .iter()
        .filter(|(_, row)| row.is_visible())
        .map(|(panel, _)| *panel)
        .collect()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn focus_sidebar_panel(
    nav_rows: &Rc<RefCell<Vec<(SettingsPanel, gtk4::Button)>>>,
    panel: SettingsPanel,
) {
    use gtk4::prelude::*;

    if let Some((_, row)) = nav_rows
        .borrow()
        .iter()
        .find(|(candidate, _)| *candidate == panel)
    {
        row.grab_focus();
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn sidebar_accessible_label(panel: SettingsPanel, selected: bool) -> String {
    if selected {
        format!("{} settings, current", panel.display_name())
    } else {
        format!("{} settings", panel.display_name())
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn sidebar_accessible_description(panel: SettingsPanel, selected: bool) -> String {
    let detail = format!("{} {}", panel.summary(), panel.sidebar_owner_description());
    if selected {
        format!("Current category. {detail}")
    } else {
        detail
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn sidebar_accessible_description_for_search(
    panel: SettingsPanel,
    selected: bool,
    query: &str,
) -> String {
    let description = sidebar_accessible_description(panel, selected);
    if query.is_empty() {
        return description;
    }

    match settings_search_preview(panel, query) {
        Some(preview) => format!("{description} {preview}."),
        None => description,
    }
}

#[derive(Clone, Copy)]
enum SidebarMovement {
    Previous,
    Next,
    First,
    Last,
}

fn sidebar_movement_from_key_name(key_name: &str) -> Option<SidebarMovement> {
    match key_name {
        "up" | "kp_up" => Some(SidebarMovement::Previous),
        "down" | "kp_down" => Some(SidebarMovement::Next),
        "home" | "kp_home" => Some(SidebarMovement::First),
        "end" | "kp_end" => Some(SidebarMovement::Last),
        _ => None,
    }
}

fn sidebar_keyboard_target(
    visible: &[SettingsPanel],
    active: SettingsPanel,
    movement: SidebarMovement,
) -> Option<SettingsPanel> {
    if visible.is_empty() {
        return None;
    }

    let current_index = visible
        .iter()
        .position(|panel| *panel == active)
        .unwrap_or(0);
    let target_index = match movement {
        SidebarMovement::Previous => current_index.saturating_sub(1),
        SidebarMovement::Next => (current_index + 1).min(visible.len() - 1),
        SidebarMovement::First => 0,
        SidebarMovement::Last => visible.len() - 1,
    };
    visible.get(target_index).copied()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_query(text: &str) -> String {
    normalize_settings_search_text(text)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_matches(panel: SettingsPanel, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    settings_search_text_matches(&panel.search_text(), query)
        || settings_search_items_for_panel(panel, query)
            .next()
            .is_some()
        || settings_search_capabilities_for_panel(panel, query)
            .next()
            .is_some()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_item_matches(item: &SettingsSearchItem, query: &str) -> bool {
    if query.is_empty() {
        return false;
    }

    settings_search_text_matches(
        &format!(
            "{} {} {}",
            item.title,
            item.terms.join(" "),
            item.panel.display_name()
        ),
        query,
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_items_for_panel(
    panel: SettingsPanel,
    query: &str,
) -> impl Iterator<Item = &'static SettingsSearchItem> + '_ {
    SETTINGS_SEARCH_ITEMS
        .iter()
        .filter(move |item| item.panel == panel && settings_search_item_matches(item, query))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_capability_matches(capability: &DeviceSettingsCapability, query: &str) -> bool {
    if query.is_empty() {
        return false;
    }

    settings_search_text_matches(
        &format!("{} {}", capability.title, capability.detail),
        query,
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_text_matches(haystack: &str, query: &str) -> bool {
    let normalized_haystack = normalize_settings_search_text(haystack);
    let terms = query.split_whitespace().collect::<Vec<_>>();
    if terms.is_empty() {
        return false;
    }

    if terms.len() == 1 {
        return settings_search_term_matches(&normalized_haystack, terms[0]);
    }

    if normalized_haystack.contains(query) {
        return true;
    }

    let compact_query = settings_search_compact(query);
    if compact_query.len() >= 3
        && settings_search_compact(&normalized_haystack).contains(&compact_query)
    {
        return true;
    }

    if terms.iter().any(|term| term.len() <= 2) {
        return false;
    }

    terms
        .iter()
        .all(|term| settings_search_term_matches(&normalized_haystack, term))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_term_matches(haystack: &str, term: &str) -> bool {
    if term.len() <= 2 {
        haystack.split_whitespace().any(|token| token == term)
    } else {
        haystack.contains(term)
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_compact(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_capabilities_for_panel(
    panel: SettingsPanel,
    query: &str,
) -> impl Iterator<Item = &'static DeviceSettingsCapability> + '_ {
    device_settings_capabilities(panel)
        .iter()
        .filter(move |capability| settings_search_capability_matches(capability, query))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_matched_setting_count(query: &str) -> usize {
    if query.is_empty() {
        return 0;
    }

    let static_items = SETTINGS_SEARCH_ITEMS
        .iter()
        .filter(|item| settings_search_item_matches(item, query))
        .count();
    let capability_items = SettingsPanel::ALL
        .iter()
        .flat_map(|panel| settings_search_capabilities_for_panel(*panel, query))
        .count();

    static_items + capability_items
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_visible_panels(query: &str) -> Vec<SettingsPanel> {
    SettingsPanel::ALL
        .iter()
        .copied()
        .filter(|panel| settings_search_matches(*panel, query))
        .collect()
}

/// Enter-navigation match strength for a panel against a query. `0` = the panel's
/// own identity (name/slug/aliases/summary) matches; `1` = it matched only through a
/// sub-setting's keywords. Lower sorts first, so "wifi" opens Network rather than a
/// panel that merely lists a Wi-Fi quick toggle in its Control Center.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_match_rank(panel: SettingsPanel, query: &str) -> u8 {
    u8::from(!settings_search_text_matches(&panel.search_text(), query))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_first_match(query: &str) -> Option<SettingsPanel> {
    if query.is_empty() {
        return None;
    }

    // `sort_by_key` is stable, so the sidebar's group order is preserved within each
    // strength tier; the strongest match wins for Enter-navigation.
    let mut matches = settings_search_visible_panels(query);
    matches.sort_by_key(|panel| settings_search_match_rank(*panel, query));
    matches.first().copied()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_status_text(query: &str, result_count: usize) -> Option<String> {
    if query.is_empty() {
        return None;
    }

    let setting_count = settings_search_matched_setting_count(query);
    Some(if setting_count > 0 {
        let setting_word = if setting_count == 1 {
            "setting"
        } else {
            "settings"
        };
        let category_word = if result_count == 1 {
            "category"
        } else {
            "categories"
        };
        format!("{setting_count} matching {setting_word} in {result_count} {category_word}")
    } else {
        match result_count {
            0 => "No matching settings".to_string(),
            1 => "1 matching category".to_string(),
            count => format!("{count} matching categories"),
        }
    })
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_accessible_description(query: &str, result_count: usize) -> String {
    if query.is_empty() {
        return "Search settings, controls, and categories. Type to narrow results.".to_string();
    }

    match settings_search_status_text(query, result_count) {
        Some(status) if result_count > 0 => format!(
            "Search settings, controls, and categories. {status}. Press Enter to open the first visible result. Press Escape to clear search."
        ),
        Some(status) => {
            format!("Search settings, controls, and categories. {status}. Press Escape to clear search.")
        }
        None => {
            "Search settings, controls, and categories. Press Enter to open the first visible result.".to_string()
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_search_preview(panel: SettingsPanel, query: &str) -> Option<String> {
    if query.is_empty() {
        return None;
    }

    let matches = settings_search_items_for_panel(panel, query)
        .map(|item| item.title)
        .chain(
            settings_search_capabilities_for_panel(panel, query).map(|capability| capability.title),
        )
        .take(3)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return None;
    }

    Some(format!("Matches: {}", matches.join(", ")))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn normalize_settings_search_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fill `main` with the chosen panel's content, replacing whatever it showed.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn populate_panel(
    main: &gtk4::Box,
    panel: SettingsPanel,
    config: &SettingsConfig,
    state: &SettingsState,
) {
    use gtk4::prelude::*;

    while let Some(child) = main.first_child() {
        main.remove(&child);
    }
    match panel {
        SettingsPanel::Overview => build_overview(main, state),
        SettingsPanel::Appearance => build_appearance(main, state),
        SettingsPanel::Applications => build_device_settings_panel(main, panel, state),
        SettingsPanel::DesktopDock => build_desktop_dock(main, state),
        SettingsPanel::MenuBarControlCenter => build_menu_bar_control_center(main, state),
        SettingsPanel::Network => build_network(main, state),
        SettingsPanel::NetworkServices => build_device_settings_panel(main, panel, state),
        SettingsPanel::Bluetooth => build_bluetooth(main, state),
        SettingsPanel::MobileBroadband => build_device_settings_panel(main, panel, state),
        SettingsPanel::Sharing => build_device_settings_panel(main, panel, state),
        SettingsPanel::Displays => build_displays(main, state),
        SettingsPanel::ColorManagement => build_device_settings_panel(main, panel, state),
        SettingsPanel::Sound => build_sound(main, state),
        SettingsPanel::Keyboard => build_keyboard(main, state),
        SettingsPanel::MouseTrackpad => build_mouse_trackpad(main, state),
        SettingsPanel::DrawingTablet => build_device_settings_panel(main, panel, state),
        SettingsPanel::Accessibility => build_accessibility(main, state),
        SettingsPanel::DesktopWallpaper => build_desktop_wallpaper(main, state),
        SettingsPanel::Notifications => build_notifications(main, state),
        SettingsPanel::LockScreen => build_device_settings_panel(main, panel, state),
        SettingsPanel::SearchIndexing => build_device_settings_panel(main, panel, state),
        SettingsPanel::Multitasking => build_device_settings_panel(main, panel, state),
        SettingsPanel::PowerBattery => build_device_settings_panel(main, panel, state),
        SettingsPanel::Games => build_games(main),
        SettingsPanel::PrintersScanners => build_device_settings_panel(main, panel, state),
        SettingsPanel::DateTime => build_device_settings_panel(main, panel, state),
        SettingsPanel::LanguageRegion => build_device_settings_panel(main, panel, state),
        SettingsPanel::UsersAccounts => build_users_accounts(main, state),
        SettingsPanel::OnlineAccounts => build_device_settings_panel(main, panel, state),
        SettingsPanel::PrivacyPermissions => build_privacy_permissions(main, state),
        SettingsPanel::Security => build_security(main, state),
        SettingsPanel::Wellbeing => build_device_settings_panel(main, panel, state),
        SettingsPanel::Models => build_models(main, state),
        SettingsPanel::Policy => build_policy(main, config, state),
        SettingsPanel::Storage => build_storage(main, state),
        SettingsPanel::UpdatesAbout => build_updates_about(main, state),
        SettingsPanel::Recovery => build_recovery(main, state),
        SettingsPanel::Developer => build_developer(main, state, config),
    }
    append_settings_ai_help(main, panel, state);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_policy(panel: &gtk4::Box, config: &SettingsConfig, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Policy",
        "OS-owned data boundaries, permission gates, and automation controls for Goblins OS.",
    );
    match &state.policy {
        Some(policy) => {
            append_policy_summary(panel, policy);
            panel.append(&label("Policy state", &["gos-subsection-title"]));
            let profile_title = format!("{} profile", policy_profile_display_name(&policy.profile));
            panel.append(&system_row(
                &profile_title,
                &format!(
                    "{}. Policy state and permission grants stay in private system storage.",
                    if policy.locked { "Locked" } else { "Unlocked" },
                ),
            ));
            panel.append(&system_row("Data boundary", &policy.data_boundary));
            panel.append(&system_row("Secret boundary", &policy.secret_boundary));
            panel.append(&label("Permission controls", &["gos-subsection-title"]));
            let permission_feedback = label(
                "Permission grants require exact acknowledgement and stay in the local OS policy store.",
                &["gos-row-copy"],
            );
            for control in &policy.controls {
                // One full-width card per control: the copy block fills, and the
                // grant action trails inside the same card, so every row shares
                // the column's flush right edge.
                let control_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
                control_row.add_css_class("gos-row");
                let control_copy = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
                control_copy.set_hexpand(true);
                control_copy.append(&label(
                    &format!("{} · {}", control.label, policy_control_status(control)),
                    &["gos-row-title"],
                ));
                control_copy.append(&label(&policy_control_detail(control), &["gos-row-copy"]));
                control_row.append(&control_copy);
                if !policy.locked
                    && control.profile_state == "permission-gated"
                    && control.grant.is_none()
                {
                    let action = button(
                        &format!("Grant {}", control.label),
                        &["gos-permission-action"],
                    );
                    action.set_valign(gtk4::Align::Center);
                    let core_url = config.core_url.clone();
                    let control_id = control.id.clone();
                    let profile = policy.profile.clone();
                    let feedback = permission_feedback.clone();
                    action.connect_clicked(move |_| {
                        let acknowledgement = permission_acknowledgement(&control_id, &profile);
                        match grant_policy_permission(&core_url, &control_id, &acknowledgement) {
                            Ok(detail) => feedback.set_text(&detail),
                            Err(error) => {
                                feedback.set_text("Goblins OS rejected the permission grant.");
                                eprintln!("settings_policy_grant_error={error}");
                            }
                        }
                    });
                    control_row.append(&action);
                }
                panel.append(&control_row);
            }
            panel.append(&permission_feedback);
        }
        None => panel.append(&system_row(
            "Goblins OS policy",
            "Waiting for policy status.",
        )),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_policy_summary(panel: &gtk4::Box, policy: &PolicyStatus) {
    use gtk4::prelude::*;

    let counts = policy_control_counts(&policy.controls);
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-policy-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, row) in [
        health_row(
            "Active profile",
            if policy.locked { "locked" } else { "unlocked" },
            true,
            &policy_profile_summary_detail(policy),
        ),
        health_row(
            "Permission gates",
            &policy_control_counts_label(counts),
            // Affirmative (green) only when nothing is gated or denied. Gating is a
            // guard, not an endorsement, so it reads as a calm/neutral state.
            counts.gated == 0 && counts.denied == 0,
            &format!(
                "{} allowed, {} gated, {} denied, {} explicitly granted.",
                counts.allowed, counts.gated, counts.denied, counts.granted
            ),
        ),
        health_row(
            "Data boundary",
            policy_data_boundary_label(&policy.profile),
            true,
            &policy.data_boundary,
        ),
        health_row(
            "Secret boundary",
            "server-side",
            true,
            &policy.secret_boundary,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        row.add_css_class("gos-policy-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&label("Policy summary", &["gos-subsection-title"]));
    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn policy_profile_summary_detail(policy: &PolicyStatus) -> String {
    match generated_timestamp(&policy.generated_at) {
        Some(generated) => format!(
            "{} profile · policy state updated {generated}.",
            policy_profile_display_name(&policy.profile),
        ),
        None => format!(
            "{} profile · policy state is available.",
            policy_profile_display_name(&policy.profile),
        ),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_overview(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Overview",
        "Goblins OS keeps OpenAI, policy, local models, storage, recovery, device controls, accounts, apps, and connectivity in one Settings experience.",
    );
    append_overview_native_settings(panel, state);

    panel.append(&label("Goblins OS status", &["gos-subsection-title"]));
    append_overview_summary(panel, state);

    panel.append(&label("Diagnostics", &["gos-subsection-title"]));
    match &state.system {
        Some(system) => {
            panel.append(&system_row("Desktop", &desktop_session_detail(system)));
            panel.append(&system_row(
                "OpenAI identity",
                if system.identity.account_authenticated {
                    "Signed in — your OpenAI account session is held in OS-owned secure storage."
                } else if system.identity.provider_configured {
                    "Provider configured — sign in to connect your OpenAI account."
                } else {
                    "Not connected — Goblins OS runs local-only until an OpenAI account is configured."
                },
            ));
            panel.append(&system_row(
                "Local model storage",
                "Model cache and private credential storage are managed by Goblins OS.",
            ));
            panel.append(&system_row(
                "Session readiness",
                "Session and setup state are checked before protected actions are enabled.",
            ));
            panel.append(&system_row(
                "Policy state",
                &state
                    .policy
                    .as_ref()
                    .map(policy_profile_summary_detail)
                    .unwrap_or_else(|| "Waiting for policy status.".to_string()),
            ));
            panel.append(&system_row(
                "System services",
                &format!(
                    "System image {} · Health checks {} · Network {}",
                    ready_word(system.services.bootc_available),
                    ready_word(system.services.systemctl_available),
                    ready_word(system.services.network_manager_available)
                ),
            ));
            panel.append(&system_row(
                "Detailed diagnostics",
                "Diagnostics keeps service health, logs, update history, and device status available without exposing secrets.",
            ));
        }
        None => panel.append(&system_row("Goblins OS", "Waiting for system settings.")),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_overview_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-overview-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, row) in [
        overview_summary_spec(
            "OpenAI account",
            overview_account_label(state.openai_auth.as_ref(), state.system.as_ref()),
            overview_account_ready(state.openai_auth.as_ref(), state.system.as_ref()),
            overview_account_detail(state.openai_auth.as_ref(), state.system.as_ref()),
        ),
        overview_summary_spec(
            "Privacy and policy",
            overview_privacy_policy_label(state.privacy.as_ref(), state.policy.as_ref()),
            overview_privacy_policy_ready(state.privacy.as_ref(), state.policy.as_ref()),
            overview_privacy_policy_detail(state.privacy.as_ref(), state.policy.as_ref()),
        ),
        overview_summary_spec(
            "AI models",
            overview_models_label(state.local_models.as_ref()),
            overview_models_ready(state.local_models.as_ref()),
            overview_models_detail(state.local_models.as_ref()),
        ),
        overview_summary_spec(
            "Storage",
            storage_overall_pressure_label(state.hardware.as_ref(), state.local_models.as_ref()),
            storage_overall_pressure_ready(storage_overall_pressure_label(
                state.hardware.as_ref(),
                state.local_models.as_ref(),
            )),
            overview_storage_detail(
                state.hardware.as_ref(),
                state.local_models.as_ref(),
                state.system.as_ref(),
            ),
        ),
        overview_summary_spec(
            "Network",
            overview_network_label(state.network.as_ref()),
            overview_network_ready(state.network.as_ref()),
            overview_network_detail(state.network.as_ref()),
        ),
        overview_summary_spec(
            "Recovery",
            overview_recovery_label(state.recovery.as_ref()),
            overview_recovery_ready(state.recovery.as_ref()),
            overview_recovery_detail(state.recovery.as_ref()),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let item = health_row(row.title, row.state, row.ready, &row.detail);
        item.add_css_class("gos-overview-summary-tile");
        item.set_hexpand(true);
        grid.attach(&item, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_overview_native_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    let available = gnome_control_center_available();
    let readiness = device_settings_readiness(state.system.as_ref(), available);
    let status = overview_native_desktop_label(state.system.as_ref(), available);
    let ready = overview_native_desktop_ready(state.system.as_ref(), available);
    let detail = overview_native_settings_detail(state.system.as_ref(), available);
    let accessibility = overview_native_settings_accessibility(state.system.as_ref(), available);

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
    row.add_css_class("gos-row");
    row.add_css_class("gos-handoff-summary");
    row.add_css_class("gos-overview-native-settings");
    set_accessible_label_description(&row, "Device controls", &accessibility);

    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    copy.set_hexpand(true);
    copy.append(&label("Device controls", &["gos-row-title"]));
    let detail_label = label(&detail, &["gos-row-copy"]);
    copy.append(&detail_label);
    row.append(&copy);

    let controls = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    controls.add_css_class("gos-handoff-controls");
    controls.set_valign(gtk4::Align::Center);
    controls.set_halign(gtk4::Align::End);
    if !ready {
        let pill = settings_status_pill(status, false);
        pill.set_halign(gtk4::Align::End);
        controls.append(&pill);
    }

    let action = button(
        device_settings_action_label(readiness),
        &["gos-permission-action", "gos-device-handoff-action"],
    );
    action.set_sensitive(ready);
    action.set_valign(gtk4::Align::Center);
    set_accessible_label_description(&action, "Manage device controls", &accessibility);
    if ready {
        action.connect_clicked(move |_| match launch_gnome_control_center() {
            Ok(()) => detail_label.set_text("Opening device controls."),
            Err(error) => {
                detail_label.set_text("Device controls could not be opened from this session.");
                eprintln!("settings_gnome_control_center_launch_error={error}");
            }
        });
    }
    controls.append(&action);
    row.append(&controls);
    panel.append(&row);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
struct OverviewSummarySpec {
    title: &'static str,
    state: &'static str,
    ready: bool,
    detail: String,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn overview_summary_spec(
    title: &'static str,
    state: &'static str,
    ready: bool,
    detail: String,
) -> OverviewSummarySpec {
    OverviewSummarySpec {
        title,
        state,
        ready,
        detail,
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_device_settings_panel(
    panel: &gtk4::Box,
    settings_panel: SettingsPanel,
    state: &SettingsState,
) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        settings_panel.display_name(),
        settings_panel.summary(),
    );
    let available = gnome_control_center_available();
    let readiness = device_settings_readiness(state.system.as_ref(), available);
    append_device_native_coverage(panel, settings_panel);
    if matches!(readiness, DeviceSettingsReadiness::Unavailable) {
        panel.append(&label("System Status", &["gos-subsection-title"]));
        panel.append(&device_settings_context_grid(settings_panel, readiness));
    }
    append_device_native_handoffs(panel, settings_panel);
    panel.append(&label("Controls", &["gos-subsection-title"]));
    let title = settings_panel.display_name();
    let detail = device_settings_integrated_detail(settings_panel);
    append_device_settings_handoff(panel, settings_panel, title, &detail, state.system.as_ref());
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_device_native_handoffs(panel: &gtk4::Box, settings_panel: SettingsPanel) {
    use gtk4::prelude::*;

    if let Some(handoff) = device_native_handoff_spec(settings_panel) {
        panel.append(&label("Related tools", &["gos-subsection-title"]));
        append_native_app_handoff(
            panel,
            handoff.title,
            handoff.app_label,
            handoff.command,
            handoff.purpose,
        );
    }
}

#[derive(Clone, Copy)]
struct NativeAppHandoffSpec {
    title: &'static str,
    app_label: &'static str,
    command: &'static str,
    purpose: &'static str,
}

fn device_native_handoff_spec(settings_panel: SettingsPanel) -> Option<NativeAppHandoffSpec> {
    match settings_panel {
        SettingsPanel::Applications => Some(NativeAppHandoffSpec {
            title: "Software",
            app_label: "Software",
            command: GNOME_SOFTWARE,
            purpose: "review installed applications, software sources, and application updates",
        }),
        _ => None,
    }
}

/// One grouped, rounded card of read-only status tiles — the shared summary-grid
/// idiom (replaces the per-panel `gos-*-summary-grid` class explosion). Each tuple
/// is `(title, state, ready, detail)`, rendered as a `health_row` tile.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn health_summary_group(rows: Vec<(&str, String, bool, String)>) -> gtk4::Grid {
    use gtk4::prelude::*;

    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);
    for (index, (title, state, ready, detail)) in rows.into_iter().enumerate() {
        let row = health_row(title, &state, ready, &detail);
        row.add_css_class("gos-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }
    grid
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_games(panel: &gtk4::Box) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Games",
        "Native gaming substrate, overlay tools, controllers, audio, Flatpak, and user-initiated non-Steam launcher paths.",
    );

    panel.append(&label("Readiness", &["gos-subsection-title"]));
    panel.append(&health_summary_group(vec![
        (
            "GPU and Vulkan",
            "ready".to_string(),
            true,
            "Mesa Vulkan drivers and vulkaninfo ship in the OS image; display-backed proof still validates the active GPU.".to_string(),
        ),
        (
            "Video acceleration",
            "ready".to_string(),
            true,
            "Mesa VA-API drivers plus the VDPAU wrapper ship with vainfo and vdpauinfo for installed-session diagnostics.".to_string(),
        ),
        (
            "GameMode",
            "ready".to_string(),
            true,
            "GameMode is installed so supported games and launchers can request performance tuning.".to_string(),
        ),
        (
            "Gamescope",
            "ready".to_string(),
            true,
            "Gamescope is installed for user-launched game sessions and compatibility testing.".to_string(),
        ),
        (
            "MangoHud",
            "ready".to_string(),
            true,
            "MangoHud is installed for user-enabled frame, frame time, and GPU overlay diagnostics.".to_string(),
        ),
        (
            "Controllers and audio",
            "ready".to_string(),
            true,
            "joystick-support, evtest, usbutils, PipeWire, PipeWire PulseAudio/ALSA compatibility, PipeWire tools, and WirePlumber are part of the OS image for controller and audio readiness.".to_string(),
        ),
        (
            "Flatpak and portals",
            "ready".to_string(),
            true,
            "Flatpak and Goblins OS desktop portals ship in the OS image; launcher and runtime installs remain user-initiated.".to_string(),
        ),
        (
            "Native architecture",
            "ready".to_string(),
            true,
            "Release evidence is captured separately for aarch64 and x86_64 RPMs; Goblins OS does not claim x86-only game runtimes work on Arm unless a launcher installs and verifies that path.".to_string(),
        ),
        (
            "Steam",
            "not installed".to_string(),
            true,
            "Steam and steam-devices are intentionally absent from the base image.".to_string(),
        ),
    ]));

    panel.append(&label("Launchers", &["gos-subsection-title"]));
    panel.append(&system_row(
        "Heroic",
        "Install through Software/Flatpak when you choose to add Epic, GOG, or Amazon game libraries. Availability is checked per architecture at install time.",
    ));
    panel.append(&system_row(
        "Lutris",
        "Install through Software/Flatpak when you choose to manage non-Steam game runners. Availability is checked per architecture at install time.",
    ));
    panel.append(&system_row(
        "Bottles",
        "Install through Software/Flatpak when you choose to manage Windows app and game bottles. Availability is checked per architecture at install time.",
    ));
    panel.append(&system_row(
        "UMU and Proton",
        "Use launchers that manage compatible runtimes explicitly; Goblins OS does not download Proton runtimes without user action.",
    ));

    append_native_app_handoff(
        panel,
        "Software",
        "Software",
        GNOME_SOFTWARE,
        "install Heroic, Lutris, Bottles, or Flatpak runtimes when you choose",
    );
}

/// Security — read-only secret/credential/boot posture from the OS policy and
/// hardware services. Nothing here is mutable: protected changes belong to the
/// system, so every row is honest live status, never a fake toggle.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_security(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Security",
        "Secret boundaries, the credential keyring, boot-image integrity, and where secrets live — real read-only status owned by Goblins OS. Protected changes stay with the system.",
    );

    panel.append(&label("Protection", &["gos-subsection-title"]));
    let mut rows: Vec<(&str, String, bool, String)> = Vec::new();
    match &state.policy {
        Some(policy) => {
            rows.push((
                "Secret boundary",
                "server-side".to_string(),
                true,
                policy.secret_boundary.clone(),
            ));
            rows.push((
                "Data boundary",
                policy_data_boundary_label(&policy.profile).to_string(),
                true,
                policy.data_boundary.clone(),
            ));
        }
        None => rows.push((
            "Secret boundary",
            "server-side".to_string(),
            true,
            "Secrets are held by the OS services and never reach the desktop session.".to_string(),
        )),
    }
    if let Some(facility) = facility_by_id(state, "boot-image") {
        rows.push((
            "Boot image",
            facility_state_label(&facility.state).to_string(),
            facility_state_is_ready(&facility.state),
            facility_user_detail(facility),
        ));
    }
    if let Some(facility) = facility_by_id(state, "keyring") {
        rows.push((
            "Credential keyring",
            facility_state_label(&facility.state).to_string(),
            facility_state_is_ready(&facility.state),
            facility_user_detail(facility),
        ));
    }
    panel.append(&health_summary_group(rows));

    panel.append(&label("Secrets storage", &["gos-subsection-title"]));
    match state.system.as_ref() {
        Some(system) => panel.append(&system_row(
            "OpenAI secrets",
            &format!(
                "Held at {} — root-owned, mode 0600, readable only by the OS services. The desktop session and this app never receive the key.",
                system.storage.secrets_dir
            ),
        )),
        None => panel.append(&system_row(
            "Secrets storage",
            "Waiting for OS storage status from Goblins OS.",
        )),
    }

    panel.append(&label("Sign-in", &["gos-subsection-title"]));
    panel.append(&system_row(
        "Session lock",
        "The Goblins OS session gate keeps the desktop locked until you unlock it. Sign-in and password policy are owned by the system and are not adjustable here.",
    ));
}

/// Desktop & Dock — honest read-only status for the dock, window controls, and
/// desktop surfaces. These are Goblins OS defaults, so this panel reports them
/// truthfully rather than faking controls the OS does not yet expose to Settings.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_desktop_dock(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Desktop & Dock",
        "The Goblins dock, window controls, and desktop surfaces follow the system design. Their status is shown here; layout changes are managed by Goblins OS.",
    );

    let session_ready = state
        .system
        .as_ref()
        .map(SettingsSystemStatus::session_has_integrated_device_settings)
        .unwrap_or(false);
    let dock_state = if session_ready { "active" } else { "checking" };

    panel.append(&label("Desktop", &["gos-subsection-title"]));
    panel.append(&health_summary_group(vec![
        (
            "Dock",
            dock_state.to_string(),
            session_ready,
            "The Goblins dock anchors favorites and running apps with the OS theme.".to_string(),
        ),
        (
            "Window controls",
            "left-aligned".to_string(),
            true,
            "Close, minimize, and zoom sit at the leading edge of every Goblins window, consistent across the OS.".to_string(),
        ),
        (
            "Desktop surfaces",
            "Goblins theme".to_string(),
            true,
            "Rounded windows, the translucent menu bar, and the OS wallpaper are part of the Goblins desktop session.".to_string(),
        ),
    ]));

    panel.append(&label("Adjusting layout", &["gos-subsection-title"]));
    panel.append(&system_row(
        "Managed by the desktop",
        "Dock favorites, position, and the window-button layout are managed by the Goblins desktop and shown here as status.",
    ));
}

/// Menu Bar & Control Center — honest read-only status for the top bar and
/// Control Center. Like the dock, these are Goblins OS defaults; this panel
/// describes them truthfully instead of inventing toggles.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_menu_bar_control_center(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Menu Bar & Control Center",
        "The top menu bar and Control Center are part of the Goblins desktop. Their status is shown here; layout changes are managed by Goblins OS.",
    );

    let session_ready = state
        .system
        .as_ref()
        .map(SettingsSystemStatus::session_has_integrated_device_settings)
        .unwrap_or(false);
    let bar_state = if session_ready { "active" } else { "checking" };

    panel.append(&label("Top bar", &["gos-subsection-title"]));
    panel.append(&health_summary_group(vec![
        (
            "Menu bar",
            bar_state.to_string(),
            session_ready,
            "The translucent top bar carries the Goblins mark, the clock, and system status menus.".to_string(),
        ),
        (
            "Control Center",
            "available".to_string(),
            true,
            "Quick settings for Wi-Fi, sound, and display live in the Control Center, opened from the menu bar.".to_string(),
        ),
        (
            "Clock & date",
            "menu bar".to_string(),
            true,
            "The clock sits in the menu bar; its date and time formats follow Date & Time.".to_string(),
        ),
    ]));

    panel.append(&label("Adjusting the menu bar", &["gos-subsection-title"]));
    panel.append(&system_row(
        "Managed by the desktop",
        "Menu-bar contents and Control Center modules are managed by Goblins OS and shown here as status.",
    ));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_appearance(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Appearance",
        "Controls Goblins OS visual system while preserving Inter as the shipped system font.",
    );
    append_appearance_summary(panel, state);
    append_appearance_settings(panel, state);
    panel.append(&system_row(
        "Typography",
        "Inter 11 is the system font across Goblins OS and included desktop utilities.",
    ));
    panel.append(&system_row(
        "Window style",
        "Rounded client chrome, traffic-light window controls, semantic light/dark tokens, and focused blue selection are owned by Goblins OS.",
    ));
    append_motion_preference(panel, state);
    append_text_scale_preference(panel, state);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_appearance_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Appearance summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-appearance-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        appearance_scheme_summary_spec(state.appearance.as_ref()),
        appearance_typography_summary_spec(),
        appearance_motion_summary_spec(state.accessibility.as_ref()),
        appearance_text_size_summary_spec(state.accessibility.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-appearance-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_network(panel: &gtk4::Box, state: &SettingsState) {
    append_panel_header(
        panel,
        "Network",
        "Manage Wi-Fi, saved networks, proxy, VPN, and connection details. Passwords stay private and are never shown here.",
    );
    append_network_summary(panel, state);
    append_wifi_management(panel, state);
    append_proxy_settings(panel, state);
    append_facility_status(
        panel,
        state,
        "networking",
        "Network facility",
        "No networking facility has been reported by Goblins OS yet.",
    );
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::Network,
        "Network system tool",
        "Open the desktop system tool for wired, VPN, saved-network, and advanced network controls. Goblins OS status stays visible here.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_network_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Network summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-network-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        internet_network_summary_spec(state.network.as_ref()),
        active_connection_summary_spec(state.network.as_ref()),
        network_manager_summary_spec(state.network.as_ref()),
        proxy_network_summary_spec(state.network.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-network-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_wifi_management(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Wi-Fi", &["gos-subsection-title"]));
    let core_url = config_core_url(state);
    match get_core_json::<WifiScan>(&core_url, "/v1/network/wifi/scan") {
        Ok(scan) => {
            panel.append(&system_row(
                "Wi-Fi scan",
                &format!(
                    "{} Network scanning is {}.",
                    polished_network_detail(&scan.detail),
                    ready_word(scan.manager_available),
                ),
            ));
            if scan.networks.is_empty() {
                panel.append(&system_row(
                    "Available networks",
                    if scan.manager_available {
                        "No Wi-Fi networks are available in this session."
                    } else {
                        "Networking is not ready, so Settings cannot scan or join Wi-Fi here."
                    },
                ));
                return;
            }

            let feedback = label(
                "Choose a network to join. Passwords are used once for the join request and are never stored by Settings.",
                &["gos-row-copy"],
            );
            panel.append(&feedback);
            for network in &scan.networks {
                panel.append(&wifi_network_row(&core_url, network, &feedback));
            }
        }
        Err(error) => panel.append(&system_row(
            "Wi-Fi management",
            &format!("Waiting for Wi-Fi scanning: {error}."),
        )),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_proxy_settings(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Proxy", &["gos-subsection-title"]));
    let Some(proxy) = state
        .network
        .as_ref()
        .and_then(|network| network.proxy.as_ref())
    else {
        panel.append(&system_row("Network proxy", "Waiting for proxy settings."));
        return;
    };

    panel.append(&health_row(
        "Proxy settings",
        if proxy.gsettings_available && proxy.schema_available {
            "available"
        } else {
            "unavailable"
        },
        proxy.gsettings_available && proxy.schema_available,
        &proxy.detail,
    ));

    if !proxy.schema_available {
        return;
    }

    if proxy.mode_available {
        let core_url = config_core_url(state);
        panel.append(&choice_row(
            "Proxy mode",
            normalized_proxy_mode(&proxy.mode),
            PROXY_MODE_OPTIONS,
            |value| proxy_mode_detail(value).to_string(),
            move |value| set_proxy_mode(&core_url, value),
        ));
    } else {
        panel.append(&system_row(
            "Proxy mode",
            "Proxy mode is not available in this session.",
        ));
    }

    panel.append(&system_row(
        "Automatic configuration",
        &proxy
            .autoconfig_url
            .as_deref()
            .map(proxy_auto_config_detail)
            .unwrap_or_else(|| {
                "Automatic proxy configuration URL is not available in this session.".to_string()
            }),
    ));
    panel.append(&system_row(
        "Bypass list",
        &proxy_ignore_hosts_detail(&proxy.ignore_hosts),
    ));
    panel.append(&system_row(
        "HTTP proxy",
        &proxy_endpoint_detail(proxy.http.host.clone(), proxy.http.port),
    ));
    panel.append(&system_row(
        "HTTPS proxy",
        &proxy_endpoint_detail(proxy.https.host.clone(), proxy.https.port),
    ));
    panel.append(&system_row(
        "FTP proxy",
        &proxy_endpoint_detail(proxy.ftp.host.clone(), proxy.ftp.port),
    ));
    panel.append(&system_row(
        "SOCKS proxy",
        &proxy_endpoint_detail(proxy.socks.host.clone(), proxy.socks.port),
    ));
    panel.append(&system_row(
        "Proxy details",
        "Mode changes are applied by Goblins OS. Hosts, ports, bypass entries, and proxy authentication stay read-only until a validated editor can manage them without exposing secrets.",
    ));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn wifi_network_row(core_url: &str, network: &WifiNetwork, feedback: &gtk4::Label) -> gtk4::Box {
    use gtk4::prelude::*;

    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    row.add_css_class("gos-row");
    row.add_css_class("gos-wifi-row");

    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    copy.set_hexpand(true);
    copy.append(&label(
        &if network.in_use {
            format!("{} · connected", network.ssid)
        } else {
            network.ssid.clone()
        },
        &["gos-row-title"],
    ));
    copy.append(&label(
        &format!(
            "{}% signal · {}",
            network.signal,
            wifi_security_label(&network.security)
        ),
        &["gos-row-copy"],
    ));
    head.append(&copy);

    let connect = button(
        if network.in_use { "Reconnect" } else { "Join" },
        &["gos-permission-action", "gos-wifi-connect"],
    );
    connect.set_valign(gtk4::Align::Center);
    connect.set_tooltip_text(Some("Join this Wi-Fi network"));
    let connect_label = if network.in_use {
        format!("Reconnect {}", network.ssid)
    } else {
        format!("Join {}", network.ssid)
    };
    set_accessible_label_description(
        &connect,
        &connect_label,
        "Joins the selected Wi-Fi network.",
    );
    head.append(&connect);
    row.append(&head);

    let password_entry = if wifi_requires_password(network) && !network.in_use {
        let entry = gtk4::PasswordEntry::new();
        entry.set_show_peek_icon(true);
        entry.set_placeholder_text(Some("Network password"));
        entry.set_tooltip_text(Some("Wi-Fi password for this network"));
        let password_label = format!("Password for {}", network.ssid);
        set_accessible_label_description(
            &entry,
            &password_label,
            "Password is used once when joining the network.",
        );
        entry.add_css_class("gos-key-entry");
        row.append(&entry);
        Some(entry)
    } else {
        None
    };

    {
        let core_url = core_url.to_string();
        let ssid = network.ssid.clone();
        let requires_password = wifi_requires_password(network) && !network.in_use;
        let password_entry = password_entry.clone();
        let feedback = feedback.clone();
        connect.connect_clicked(move |_| {
            let password = password_entry
                .as_ref()
                .map(|entry| entry.text().to_string());
            if requires_password && password.as_deref().unwrap_or("").trim().is_empty() {
                feedback.set_text("Enter the network password before joining this Wi-Fi network.");
                return;
            }
            match connect_wifi(&core_url, &ssid, password.as_deref()) {
                Ok(detail) => {
                    if let Some(entry) = &password_entry {
                        entry.set_text("");
                    }
                    feedback.set_text(&detail);
                }
                Err(detail) => feedback.set_text(&detail),
            }
        });
    }

    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_bluetooth(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Bluetooth",
        "Manage Bluetooth devices, power, visibility, and connections.",
    );
    append_bluetooth_summary(panel, state);
    match &state.bluetooth {
        Some(bluetooth) => append_bluetooth_power_control(panel, state, bluetooth),
        None => append_facility_status(
            panel,
            state,
            "bluetooth",
            "Bluetooth",
            "Waiting for Bluetooth status.",
        ),
    }
    panel.append(&system_row(
        "Pairing controls",
        "Read-only until secure device actions are available. Pair, connect, disconnect, and forget controls stay disabled for now.",
    ));
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::Bluetooth,
        "Bluetooth system tool",
        "Open the desktop system tool for pairing, connections, device trust, and adapter behavior.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_bluetooth_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Bluetooth summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-bluetooth-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        bluetooth_service_summary_spec(state.bluetooth.as_ref()),
        bluetooth_adapter_summary_spec(state.bluetooth.as_ref()),
        bluetooth_power_summary_spec(state.bluetooth.as_ref()),
        bluetooth_visibility_summary_spec(state.bluetooth.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-bluetooth-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_bluetooth_power_control(
    panel: &gtk4::Box,
    state: &SettingsState,
    bluetooth: &BluetoothStatus,
) {
    use gtk4::prelude::*;

    panel.append(&label("Adapter controls", &["gos-subsection-title"]));

    if !bluetooth.bluez_available {
        panel.append(&system_row(
            "Bluetooth power",
            "Bluetooth support is not ready on this device, so power controls stay disabled.",
        ));
        return;
    }
    if !bluetooth.service_active {
        panel.append(&system_row(
            "Bluetooth power",
            "Bluetooth is not running, so power controls stay disabled.",
        ));
        return;
    }
    if !bluetooth.adapter_present {
        panel.append(&system_row(
            "Bluetooth power",
            "No Bluetooth adapter is connected.",
        ));
        return;
    }

    let Some(powered) = bluetooth.powered else {
        panel.append(&system_row(
            "Bluetooth power",
            "Read-only until Bluetooth reports the current power state.",
        ));
        return;
    };

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
    row.add_css_class("gos-row");
    row.add_css_class("gos-switch-row");

    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    copy.set_hexpand(true);
    let title = label(bluetooth_power_label(powered), &["gos-row-title"]);
    let detail = label(bluetooth_power_detail(powered), &["gos-row-copy"]);
    copy.append(&title);
    copy.append(&detail);
    row.append(&copy);

    let toggle = gtk4::Switch::new();
    toggle.set_active(powered);
    toggle.set_valign(gtk4::Align::Center);
    toggle.set_tooltip_text(Some("Bluetooth power"));
    set_accessible_label_description(&toggle, "Bluetooth power", bluetooth_power_detail(powered));
    row.append(&toggle);
    panel.append(&row);

    let feedback = label(
        "Bluetooth power changes are applied. Pairing will appear when secure device actions are available.",
        &["gos-row-copy"],
    );
    panel.append(&feedback);

    let core_url = config_core_url(state);
    let current_powered = Rc::new(Cell::new(powered));
    let updating_switch = Rc::new(Cell::new(false));
    {
        let title = title.clone();
        let detail = detail.clone();
        let feedback = feedback.clone();
        let current_powered = current_powered.clone();
        let updating_switch = updating_switch.clone();
        toggle.connect_active_notify(move |toggle| {
            if updating_switch.get() {
                return;
            }
            let next_powered = toggle.is_active();
            if next_powered == current_powered.get() {
                return;
            }

            toggle.set_sensitive(false);
            match set_bluetooth_power(&core_url, next_powered) {
                Ok(message) => {
                    current_powered.set(next_powered);
                    title.set_text(bluetooth_power_label(next_powered));
                    let next_detail = bluetooth_power_detail(next_powered);
                    detail.set_text(next_detail);
                    feedback.set_text(&message);
                    toggle.update_property(&[gtk4::accessible::Property::Description(next_detail)]);
                }
                Err(message) => {
                    feedback.set_text(&message);
                    updating_switch.set(true);
                    toggle.set_active(current_powered.get());
                    updating_switch.set(false);
                }
            }
            toggle.set_sensitive(true);
        });
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_displays(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Displays",
        "Display readiness, connected screens, and visual comfort settings from Goblins OS.",
    );
    append_display_summary(panel, state);

    append_night_light_settings(panel, state);
    panel.append(&label("Arrangement", &["gos-subsection-title"]));
    if let Some(displays) = &state.displays {
        if displays.outputs.is_empty() {
            panel.append(&system_row(
                "Detected displays",
                "No individual display outputs were reported by the display system.",
            ));
        } else {
            for output in &displays.outputs {
                panel.append(&system_row(
                    &display_output_title(output),
                    &display_output_detail(output),
                ));
            }
        }
    }
    panel.append(&system_row(
        "Resolution and scaling",
        "Read-only until protected display controls are available. Resolution and scale changes stay disabled for now.",
    ));
    panel.append(&system_row(
        "Display arrangement",
        "Read-only until protected display controls are available. Monitor placement, mirroring, and primary-display changes stay disabled for now.",
    ));
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::Displays,
        "Display system tool",
        "Open the desktop system tool for resolution, scale, refresh rate, mirroring, and arrangement.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_display_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Display summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-display-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        display_session_summary_spec(state.displays.as_ref(), state.hardware.as_ref()),
        display_query_summary_spec(state.displays.as_ref()),
        display_outputs_summary_spec(state.displays.as_ref()),
        display_comfort_summary_spec(state.accessibility.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-display-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_sound(panel: &gtk4::Box, state: &SettingsState) {
    append_panel_header(
        panel,
        "Sound",
        "Audio device readiness, volume, mute, system sounds, and Goblins OS local voice capability.",
    );
    append_sound_summary(panel, state);
    append_audio_controls(panel, state);
    append_sound_preferences(panel, state);
    append_voice_settings(panel, state);
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::Sound,
        "Sound system tool",
        "Open the desktop system tool for advanced input/output devices, alert sounds, and per-device behavior.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_sound_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Sound summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-sound-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        audio_service_summary_spec(state.audio.as_ref()),
        audio_endpoint_summary_spec(
            "Output",
            "output",
            state.audio.as_ref().map(|audio| &audio.output),
        ),
        audio_endpoint_summary_spec(
            "Input",
            "input",
            state.audio.as_ref().map(|audio| &audio.input),
        ),
        sound_preferences_summary_spec(state.audio.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-sound-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_audio_controls(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Output", &["gos-subsection-title"]));
    let Some(audio) = &state.audio else {
        panel.append(&system_row("Output volume", "Waiting for audio status."));
        panel.append(&label("Input", &["gos-subsection-title"]));
        panel.append(&system_row("Input volume", "Waiting for audio status."));
        return;
    };

    panel.append(&health_row(
        "Audio routing",
        if audio.wireplumber_available {
            "available"
        } else {
            "unavailable"
        },
        audio.wireplumber_available,
        &audio.detail,
    ));

    append_audio_endpoint_controls(panel, state, "output", &audio.output);
    panel.append(&label("Input", &["gos-subsection-title"]));
    append_audio_endpoint_controls(panel, state, "input", &audio.input);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_audio_endpoint_controls(
    panel: &gtk4::Box,
    state: &SettingsState,
    target: &'static str,
    endpoint: &AudioEndpointStatus,
) {
    append_audio_device_selection(panel, state, target, endpoint);

    if !endpoint.available {
        panel.append(&system_row(audio_endpoint_title(target), &endpoint.detail));
        return;
    }

    let core_url = config_core_url(state);
    let volume = endpoint.volume_percent.unwrap_or(0);
    panel.append(&slider_row(
        SliderSpec {
            title: audio_volume_title(target),
            detail: audio_volume_detail(target),
            value: f64::from(volume),
            min: 0.0,
            max: 150.0,
            step: 1.0,
        },
        audio_volume_label,
        normalized_audio_volume,
        move |value| set_audio_volume(&core_url, target, value.round() as u8),
    ));

    if let Some(muted) = endpoint.muted {
        let core_url = config_core_url(state);
        panel.append(&switch_row_dynamic(
            audio_mute_title(target),
            muted,
            true,
            move |muted| audio_mute_detail(target, muted).to_string(),
            move |muted| set_audio_mute(&core_url, target, muted),
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_audio_device_selection(
    panel: &gtk4::Box,
    state: &SettingsState,
    target: &'static str,
    endpoint: &AudioEndpointStatus,
) {
    let title = audio_device_choice_title(target);
    if endpoint.devices.is_empty() {
        panel.append(&system_row(
            title,
            &audio_device_unavailable_detail(target, endpoint),
        ));
        return;
    }

    if endpoint.devices.len() == 1 {
        let device = &endpoint.devices[0];
        panel.append(&system_row(
            title,
            &audio_single_device_detail(target, device),
        ));
        return;
    }

    let Some(current_id) = endpoint.default_device_id.as_deref() else {
        panel.append(&system_row(
            title,
            "Audio routing reports multiple devices, but no default is marked in this session.",
        ));
        return;
    };

    let options: Vec<ChoiceOption<'_>> = endpoint
        .devices
        .iter()
        .map(|device| ChoiceOption {
            id: device.id.as_str(),
            label: device.name.as_str(),
        })
        .collect();
    let devices = endpoint.devices.clone();
    let detail_devices = devices.clone();
    let core_url = config_core_url(state);
    panel.append(&choice_row(
        title,
        current_id,
        &options,
        move |device_id| audio_device_choice_detail(target, &detail_devices, device_id),
        move |device_id| set_audio_default_device(&core_url, target, device_id),
    ));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_keyboard(panel: &gtk4::Box, state: &SettingsState) {
    append_panel_header(
        panel,
        "Keyboard",
        "Typing preferences, keyboard repeat, input sources, and shortcuts for this device.",
    );
    append_keyboard_summary(panel, state);
    append_facility_status(
        panel,
        state,
        "input-devices",
        "Keyboard and input devices",
        "Waiting for keyboard and input-device status.",
    );
    append_keyboard_preferences(panel, state);
    panel.append(&system_row(
        "Keyboard shortcuts",
        "Read-only until protected shortcut controls are available. Shortcut editing stays disabled for now.",
    ));
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::Keyboard,
        "Keyboard system tool",
        "Open the desktop system tool for shortcuts, input sources, compose key, and advanced typing behavior.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_mouse_trackpad(panel: &gtk4::Box, state: &SettingsState) {
    append_panel_header(
        panel,
        "Mouse & Trackpad",
        "Pointer, mouse, and touchpad preferences for the current desktop session.",
    );
    append_pointer_summary(panel, state);
    append_facility_status(
        panel,
        state,
        "input-devices",
        "Pointer devices",
        "Waiting for pointer-device status.",
    );
    append_pointer_preferences(panel, state);
    panel.append(&system_row(
        "Gestures",
        "Read-only until protected gesture controls are available. Gesture editing stays disabled for now.",
    ));
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::MouseTrackpad,
        "Pointer system tool",
        "Open the desktop system tool for mouse, trackpad, device-specific pointer, and gesture behavior.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_keyboard_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Keyboard summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-input-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        input_source_summary_spec(state.input.as_ref()),
        keyboard_repeat_summary_spec(state.input.as_ref()),
        keyboard_delay_summary_spec(state.input.as_ref()),
        keyboard_interval_summary_spec(state.input.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-input-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_pointer_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Pointer summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-input-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        input_source_summary_spec(state.input.as_ref()),
        mouse_speed_summary_spec(state.input.as_ref()),
        touchpad_speed_summary_spec(state.input.as_ref()),
        touchpad_tap_summary_spec(state.input.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-input-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_accessibility(panel: &gtk4::Box, state: &SettingsState) {
    append_panel_header(
        panel,
        "Accessibility",
        "Motion and type preferences write system interface keys so system utilities and Goblins OS surfaces follow the same setting.",
    );
    append_accessibility_summary(panel, state);
    append_facility_status(
        panel,
        state,
        "accessibility",
        "Accessibility bus",
        "Waiting for AT-SPI accessibility status.",
    );
    append_assistive_technology_settings(panel, state);
    append_motion_preference(panel, state);
    append_text_scale_preference(panel, state);
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::Accessibility,
        "Accessibility system tool",
        "Open the desktop system tool for additional assistive technologies and device-specific accessibility controls.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_accessibility_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Accessibility summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-accessibility-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        accessibility_text_size_summary_spec(state.accessibility.as_ref()),
        accessibility_motion_summary_spec(state.accessibility.as_ref()),
        assistive_access_summary_spec(state.accessibility.as_ref()),
        accessibility_display_comfort_summary_spec(state.accessibility.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-accessibility-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_desktop_wallpaper(panel: &gtk4::Box, state: &SettingsState) {
    append_panel_header(
        panel,
        "Desktop & Wallpaper",
        "Desktop background preferences for the current session.",
    );
    append_wallpaper_summary(panel, state);
    panel.append(&system_row(
        "Desktop font",
        "Inter is configured as Goblins OS desktop font.",
    ));
    append_background_image_settings(panel, state);
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::DesktopWallpaper,
        "Wallpaper system tool",
        "Open the desktop system tool for the background library and image picker.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_wallpaper_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Wallpaper summary", &["gos-subsection-title"]));
    let wallpaper = state
        .appearance
        .as_ref()
        .and_then(|appearance| appearance.wallpaper.as_ref());
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-wallpaper-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        wallpaper_status_summary_spec(wallpaper),
        wallpaper_light_image_summary_spec(wallpaper),
        wallpaper_dark_image_summary_spec(wallpaper),
        wallpaper_placement_summary_spec(wallpaper),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-wallpaper-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_background_image_settings(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Wallpaper", &["gos-subsection-title"]));
    let Some(wallpaper) = state
        .appearance
        .as_ref()
        .and_then(|appearance| appearance.wallpaper.as_ref())
    else {
        panel.append(&system_row("Wallpaper", "Waiting for wallpaper status."));
        return;
    };

    panel.append(&health_row(
        "Wallpaper settings",
        if wallpaper.gsettings_available && wallpaper.schema_available {
            "available"
        } else {
            "unavailable"
        },
        wallpaper.gsettings_available && wallpaper.schema_available,
        &wallpaper.detail,
    ));

    if !wallpaper.schema_available {
        return;
    }

    panel.append(&system_row(
        "Light wallpaper",
        &wallpaper_uri_detail(
            wallpaper.picture_uri.as_deref(),
            "Light wallpaper preference is not available in this session.",
        ),
    ));
    panel.append(&system_row(
        "Dark wallpaper",
        &wallpaper_uri_detail(
            wallpaper.picture_uri_dark.as_deref(),
            "Dark wallpaper preference is not available in this session.",
        ),
    ));

    if wallpaper.picture_options_available {
        let core_url = config_core_url(state);
        let current = normalized_background_picture_option(&wallpaper.picture_options);
        panel.append(&choice_row(
            "Image placement",
            current,
            BACKGROUND_PICTURE_OPTIONS,
            |value| background_picture_option_detail(value).to_string(),
            move |value| set_wallpaper_placement(&core_url, value),
        ));
    } else {
        panel.append(&system_row(
            "Image placement",
            "Wallpaper placement is not available in this session.",
        ));
    }

    panel.append(&label("Desktop colors", &["gos-subsection-title"]));
    panel.append(&system_row(
        "Primary color",
        &wallpaper_color_detail(
            wallpaper.primary_color.as_deref(),
            "Primary desktop color is not reported in this session.",
        ),
    ));
    panel.append(&system_row(
        "Secondary color",
        &wallpaper_color_detail(
            wallpaper.secondary_color.as_deref(),
            "Secondary desktop color is not reported in this session.",
        ),
    ));
    if wallpaper.color_shading_type_available {
        let core_url = config_core_url(state);
        let current = normalized_background_shading(&wallpaper.color_shading_type);
        panel.append(&choice_row(
            "Color blending",
            current,
            BACKGROUND_SHADING_OPTIONS,
            |value| background_shading_detail(value).to_string(),
            move |value| set_wallpaper_shading(&core_url, value),
        ));
    } else {
        panel.append(&system_row(
            "Color blending",
            "Wallpaper color blending is not available in this session.",
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_notifications(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Notifications",
        "Notification delivery, banners, lock screen visibility, and per-app preferences.",
    );
    append_notifications_summary(panel, state);
    append_notifications_ai_context(panel, state);

    panel.append(&label("Delivery", &["gos-subsection-title"]));
    let Some(notifications) = state.notifications.as_ref() else {
        panel.append(&system_row(
            "Notification delivery",
            "Waiting for notification preferences.",
        ));
        return;
    };

    panel.append(&health_row(
        "Notification settings",
        if notifications.gsettings_available && notifications.schema_available {
            "available"
        } else {
            "unavailable"
        },
        notifications.gsettings_available && notifications.schema_available,
        &notifications.detail,
    ));

    if !notifications.schema_available {
        return;
    }

    append_notification_bool_row(
        panel,
        state,
        "show-banners",
        None,
        "Show notification banners",
        notifications.show_banners,
        notification_banners_detail,
    );

    append_notification_bool_row(
        panel,
        state,
        "show-in-lock-screen",
        None,
        "Show notifications on lock screen",
        notifications.show_in_lock_screen,
        lock_screen_notifications_detail,
    );

    panel.append(&label("Applications", &["gos-subsection-title"]));
    panel.append(&system_row(
        "Per-app notification entries",
        &notification_app_children_detail(&notifications.application_children),
    ));
    if notifications.application_children.is_empty() {
        panel.append(&system_row(
            "Per-app notification controls",
            "No applications have registered notification preferences in this session yet. Applications inherit the global notification settings above.",
        ));
        return;
    }

    if !notifications.application_schema_available {
        panel.append(&system_row(
            "Per-app notification controls",
            "Per-application notification settings are not available in this session.",
        ));
        return;
    }

    for app in &notifications.applications {
        append_notification_application_settings(panel, state, app);
    }
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::Notifications,
        "Notification system tool",
        "Open the desktop system tool for additional per-application notification editing.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_notifications_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Notification summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-notifications-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        notifications_delivery_summary_spec(state.notifications.as_ref()),
        notification_banners_summary_spec(state.notifications.as_ref()),
        notification_lock_screen_summary_spec(state.notifications.as_ref()),
        notification_app_registry_summary_spec(state.notifications.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-notifications-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_notifications_ai_context(panel: &gtk4::Box, state: &SettingsState) {
    let row = match state
        .ai_actions
        .as_ref()
        .and_then(|catalog| ai_action_by_id(catalog, "answer-notification"))
    {
        Some(action) => {
            let detail = format!(
                "From an invoked notification, Goblins OS sends only that notification's title, body, app, and chosen action label through the OS-owned notification context route. {}",
                ai_action_detail(action)
            );
            health_row(
                "Ask Goblin about a notification",
                ai_action_state_label(action),
                ai_action_state_ok(action),
                &detail,
            )
        }
        None if state.ai_actions.is_some() => health_row(
            "Ask Goblin about a notification",
            "missing",
            false,
            "The OS action catalog did not return the notification context action.",
        ),
        None => health_row(
            "Ask Goblin about a notification",
            "checking",
            false,
            "Checking the OS action catalog before showing notification AI readiness.",
        ),
    };

    append_preference_group(panel, "Goblins AI for notifications", vec![row]);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_notification_application_settings(
    panel: &gtk4::Box,
    state: &SettingsState,
    app: &NotificationApplicationStatus,
) {
    panel.append(&system_row(
        &format!("{} notification record", app.label),
        &app.detail,
    ));

    append_notification_bool_row(
        panel,
        state,
        "application-enable",
        Some(&app.child),
        &format!("Allow notifications from {}", app.label),
        app.enable,
        notification_app_enable_detail,
    );
    append_notification_bool_row(
        panel,
        state,
        "application-show-banners",
        Some(&app.child),
        &format!("Banners for {}", app.label),
        app.show_banners,
        notification_app_banner_detail,
    );
    append_notification_bool_row(
        panel,
        state,
        "application-sound-alerts",
        Some(&app.child),
        &format!("Sound alerts for {}", app.label),
        app.enable_sound_alerts,
        notification_app_sound_detail,
    );
    append_notification_bool_row(
        panel,
        state,
        "application-show-in-lock-screen",
        Some(&app.child),
        &format!("Show {} on lock screen", app.label),
        app.show_in_lock_screen,
        notification_app_lock_screen_detail,
    );
    append_notification_bool_row(
        panel,
        state,
        "application-details-in-lock-screen",
        Some(&app.child),
        &format!("Show {} details on lock screen", app.label),
        app.details_in_lock_screen,
        notification_app_lock_screen_details_detail,
    );
    append_notification_bool_row(
        panel,
        state,
        "application-force-expanded",
        Some(&app.child),
        &format!("Expand {} banners", app.label),
        app.force_expanded,
        notification_app_expand_detail,
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_notification_bool_row(
    panel: &gtk4::Box,
    state: &SettingsState,
    target: &'static str,
    child: Option<&str>,
    title: &str,
    value: Option<bool>,
    detail_for_state: fn(bool) -> &'static str,
) {
    if let Some(value) = value {
        let core_url = config_core_url(state);
        let child = child.map(str::to_string);
        panel.append(&switch_row_dynamic(
            title,
            value,
            true,
            move |enabled| detail_for_state(enabled).to_string(),
            move |enabled| {
                set_notification_preference_bool(&core_url, target, child.as_deref(), enabled)
            },
        ));
    } else {
        panel.append(&system_row(
            title,
            "This notification preference is not available in the current desktop session.",
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_users_accounts(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Users & Accounts",
        "Manage local users, device identity, OpenAI account state, and Codex sign-in without showing secrets.",
    );
    append_users_accounts_summary(panel, state);
    let feedback = label("", &["gos-row-copy"]);
    let has_openai_action = append_openai_account_settings(panel, state, &feedback);
    panel.append(&label("Codex", &["gos-subsection-title"]));
    let has_codex_action = append_codex_settings(panel, state, &feedback);
    panel.append(&label("Local User", &["gos-subsection-title"]));
    append_local_user_settings(panel, state);
    panel.append(&label("Desktop", &["gos-subsection-title"]));
    append_desktop_session_identity(panel, state);
    if has_openai_action || has_codex_action {
        feedback.set_text(
            "Account actions open the configured provider or Codex sign-in flow. Secrets stay private.",
        );
        panel.append(&feedback);
    }
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::UsersAccounts,
        "User system tool",
        "Open the desktop system tool for local accounts, passwords, administrator access, and device identity.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_users_accounts_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Account summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-account-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        local_account_summary_spec(state.system.as_ref()),
        openai_account_summary_spec(state.openai_auth.as_ref(), state.system.as_ref()),
        codex_account_summary_spec(state.codex.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-account-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

struct AccountSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn account_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> AccountSummarySpec {
    AccountSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_openai_account_settings(
    panel: &gtk4::Box,
    state: &SettingsState,
    feedback: &gtk4::Label,
) -> bool {
    use gtk4::prelude::*;

    panel.append(&label("OpenAI Account", &["gos-subsection-title"]));
    let core_url = config_core_url(state);
    let mut has_action = false;
    match &state.openai_auth {
        Some(auth) => {
            panel.append(&health_row(
                "OpenAI account",
                if auth.authenticated {
                    "signed in"
                } else if auth.configured {
                    "configured"
                } else {
                    "not configured"
                },
                auth.authenticated,
                &openai_account_detail(auth),
            ));
            if auth.configured && !auth.authenticated {
                let signin = button("Sign in with OpenAI", &["gos-permission-action"]);
                set_accessible_label_description(
                    &signin,
                    "Sign in with OpenAI",
                    "Opens the configured OpenAI account provider. Secrets remain in OS-owned storage.",
                );
                let core_url = core_url.clone();
                let feedback = feedback.clone();
                signin.connect_clicked(move |_| match openai_login_destination(&core_url) {
                    Ok(destination) => {
                        feedback.set_text("Opening the configured OpenAI account provider.");
                        if let Err(error) = gtk4::gio::AppInfo::launch_default_for_uri(
                            &destination,
                            None::<&gtk4::gio::AppLaunchContext>,
                        ) {
                            feedback.set_text(
                                "The desktop could not open the OpenAI account provider.",
                            );
                            eprintln!("settings_openai_account_launch_error={error}");
                        }
                    }
                    Err(error) => {
                        feedback.set_text("OpenAI account handoff did not start.");
                        eprintln!("settings_openai_account_start_error={error}");
                    }
                });
                panel.append(&signin);
                has_action = true;
            }
        }
        None => match &state.system {
            Some(system) => {
                panel.append(&system_row(
                    "Account provider",
                    if system.identity.account_authenticated {
                        "Signed in. The account session is held in OS-owned storage."
                    } else if system.identity.provider_configured {
                        "Provider configured. Sign in to connect your OpenAI account."
                    } else {
                        "Not connected. Goblins OS runs local-only until an account is configured."
                    },
                ));
            }
            None => panel.append(&system_row("Account state", "Waiting for identity status.")),
        },
    }
    has_action
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_local_user_settings(panel: &gtk4::Box, state: &SettingsState) {
    if let Some(system) = &state.system {
        let Some(account) = system.local_account.as_ref() else {
            panel.append(&system_row(
                "Local account",
                "Waiting for local account status.",
            ));
            panel.append(&system_row(
                "User management",
                "Read-only until secure account actions are available. Creating users, changing passwords, and changing administrator rights stay disabled for now.",
            ));
            return;
        };
        panel.append(&system_row(
            "Local account",
            &local_account_identity_detail(account),
        ));
        panel.append(&system_row(
            "Account type",
            &local_account_type_detail(account),
        ));
        panel.append(&system_row("Home folder", &account.home));
        panel.append(&system_row("Login shell", &account.shell));
        panel.append(&system_row("Computer name", &account.hostname));
    } else {
        panel.append(&system_row(
            "Local account",
            "Waiting for local account status.",
        ));
    }
    panel.append(&system_row(
        "User management",
        "Read-only until secure account actions are available. Creating users, changing passwords, and changing administrator rights stay disabled for now.",
    ));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_desktop_session_identity(panel: &gtk4::Box, state: &SettingsState) {
    match &state.system {
        Some(system) => panel.append(&system_row("Desktop", &desktop_session_detail(system))),
        None => panel.append(&system_row("Desktop", "Waiting for desktop status.")),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_privacy_permissions(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Privacy & Permissions",
        "Private mode, desktop portals, keyring readiness, and the policy boundary that controls data movement.",
    );
    append_privacy_summary(panel, state);
    append_privacy_settings(panel, state);
    append_desktop_privacy_settings(panel, state);
    if let Some(policy) = &state.policy {
        panel.append(&system_row("Data boundary", &policy.data_boundary));
        panel.append(&system_row("Secret boundary", &policy.secret_boundary));
    } else {
        panel.append(&system_row("Policy boundary", "Waiting for policy status."));
    }
    append_facility_status(
        panel,
        state,
        "desktop-portals",
        "Desktop portals",
        "Waiting for desktop portal status.",
    );
    append_facility_status(
        panel,
        state,
        "keyring",
        "Keyring",
        "Waiting for keyring status.",
    );
    append_facility_status(
        panel,
        state,
        "policy",
        "Policy prompts",
        "Waiting for protected action prompt status.",
    );
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::PrivacyPermissions,
        "Privacy system tool",
        "Open the desktop system tool for location, camera, microphone, and portal permissions.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_privacy_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Privacy summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-privacy-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        privacy_private_mode_summary_spec(state.privacy.as_ref()),
        desktop_privacy_schema_summary_spec(state.privacy.as_ref()),
        privacy_device_access_summary_spec(state.privacy.as_ref()),
        privacy_cleanup_summary_spec(state.privacy.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-privacy-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_storage(panel: &gtk4::Box, state: &SettingsState) {
    append_panel_header(
        panel,
        "Storage",
        "Disk space, local model cache, and OS-owned state. Full drive inspection opens in system tools.",
    );
    append_storage_summary(panel, state);
    append_storage_pressure_plan(panel, state);

    let mut capacity_rows = Vec::new();
    match &state.hardware {
        Some(hardware) if !hardware.storage.is_empty() => {
            for volume in &hardware.storage {
                capacity_rows.push(storage_volume_row(volume));
            }
        }
        Some(_) => capacity_rows.push(system_row(
            "Mounted storage",
            "No mounted storage volumes were reported by Goblins OS.",
        )),
        None => capacity_rows.push(system_row(
            "Mounted storage",
            "Waiting for storage status from Goblins OS.",
        )),
    }
    append_preference_group(panel, "Capacity", capacity_rows);

    append_storage_cleanup_settings(panel, state);

    let model_rows = vec![match (&state.local_models, &state.system) {
        (Some(catalog), _) => model_cache_capacity_row(catalog),
        (None, Some(_system)) => system_row(
            "Model cache",
            "Waiting for model-cache capacity from Goblins OS.",
        ),
        (None, None) => system_row("Model cache", "Waiting for model-cache capacity."),
    }];
    append_preference_group(panel, "Model cache", model_rows);

    append_preference_group(
        panel,
        "Private storage",
        vec![private_system_storage_boundary_row(state.system.as_ref())],
    );

    append_native_storage_handoffs(panel);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_storage_pressure_plan(panel: &gtk4::Box, state: &SettingsState) {
    let pressure =
        storage_overall_pressure_label(state.hardware.as_ref(), state.local_models.as_ref());
    let detail = storage_pressure_plan_detail(
        pressure,
        native_app_available(GNOME_DISK_USAGE_ANALYZER),
        native_app_available(GNOME_DISKS),
        storage_cleanup_controls_available(state.privacy.as_ref()),
        state.local_models.is_some(),
    );

    append_preference_group(
        panel,
        "Storage pressure plan",
        vec![health_row(
            "Recommended next steps",
            pressure,
            storage_overall_pressure_ready(pressure),
            &detail,
        )],
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_storage_cleanup_settings(panel: &gtk4::Box, state: &SettingsState) {
    let Some(desktop) = state
        .privacy
        .as_ref()
        .and_then(|privacy| privacy.desktop.as_ref())
    else {
        append_preference_group(
            panel,
            "Cleanup",
            vec![system_row(
                "Cleanup options",
                "Waiting for desktop cleanup controls.",
            )],
        );
        return;
    };

    if !(desktop.gsettings_available && desktop.schema_available) {
        append_preference_group(
            panel,
            "Cleanup",
            vec![system_row("Cleanup options", &desktop.detail)],
        );
        return;
    }

    let core_url = config_core_url(state);
    let mut cleanup_rows = Vec::new();
    if let Some(value) = desktop.remove_old_trash_files {
        let core_url = core_url.clone();
        cleanup_rows.push(switch_row_dynamic(
            "Remove aged Trash items",
            value,
            true,
            |enabled| cleanup_trash_detail(enabled).to_string(),
            move |enabled| set_desktop_privacy_bool(&core_url, "remove-old-trash-files", enabled),
        ));
    }
    if let Some(value) = desktop.remove_old_temp_files {
        let core_url = core_url.clone();
        cleanup_rows.push(switch_row_dynamic(
            "Remove aged temporary files",
            value,
            true,
            |enabled| cleanup_temp_detail(enabled).to_string(),
            move |enabled| set_desktop_privacy_bool(&core_url, "remove-old-temp-files", enabled),
        ));
    }
    if let Some(age) = desktop.old_files_age_days {
        let core_url = core_url.clone();
        cleanup_rows.push(slider_row(
            SliderSpec {
                title: "Cleanup age",
                detail:
                    "Trash and temporary files become eligible for cleanup after this many days.",
                value: f64::from(normalized_old_files_age(age)),
                min: 1.0,
                max: 365.0,
                step: 1.0,
            },
            days_label,
            normalized_old_files_age_slider,
            move |value| set_desktop_privacy_number(&core_url, "old-files-age-days", value),
        ));
    }

    if cleanup_rows.is_empty() {
        cleanup_rows.push(system_row(
            "Cleanup options",
            "Desktop cleanup controls are not available in this session.",
        ));
    }
    append_preference_group(panel, "Cleanup", cleanup_rows);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_native_storage_handoffs(panel: &gtk4::Box) {
    use gtk4::prelude::*;

    panel.append(&label("Storage utilities", &["gos-subsection-title"]));
    append_native_app_handoff(
        panel,
        "Disk Usage Analyzer",
        "Disk Usage Analyzer",
        GNOME_DISK_USAGE_ANALYZER,
        "inspect folders, mounted volumes, and where disk space is being used",
    );
    append_native_app_handoff(
        panel,
        "Disks",
        "Disks",
        GNOME_DISKS,
        "inspect drives, partitions, filesystems, and SMART data",
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn model_cache_capacity_row(catalog: &LocalModelCatalog) -> gtk4::Box {
    let detail = model_cache_capacity_detail(catalog);
    health_row(
        "Model cache",
        model_cache_capacity_label(catalog.hardware.model_dir_available_gb),
        model_cache_capacity_ready(catalog.hardware.model_dir_available_gb),
        &detail,
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn private_system_storage_boundary_row(system: Option<&SettingsSystemStatus>) -> gtk4::Box {
    let spec = os_state_storage_summary_spec(system);
    let detail = private_system_storage_boundary_detail(&spec);
    health_row("Private storage boundary", spec.state, spec.ready, &detail)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_storage_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Storage summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-storage-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        storage_pressure_summary_spec(
            state.hardware.as_ref(),
            state.local_models.as_ref(),
            state.system.as_ref(),
        ),
        model_cache_summary_spec(state.local_models.as_ref(), state.system.as_ref()),
        mounted_storage_summary_spec(state.hardware.as_ref()),
        os_state_storage_summary_spec(state.system.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, item.state, item.ready, &item.detail);
        row.add_css_class("gos-storage-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

struct StorageSummarySpec {
    title: &'static str,
    state: &'static str,
    ready: bool,
    detail: String,
}

fn storage_pressure_summary_spec(
    hardware: Option<&HardwareStatus>,
    catalog: Option<&LocalModelCatalog>,
    system: Option<&SettingsSystemStatus>,
) -> StorageSummarySpec {
    let state = storage_overall_pressure_label(hardware, catalog);
    StorageSummarySpec {
        title: "Storage pressure",
        state,
        ready: storage_overall_pressure_ready(state),
        detail: storage_overall_pressure_detail(hardware, catalog, system),
    }
}

fn model_cache_summary_spec(
    catalog: Option<&LocalModelCatalog>,
    system: Option<&SettingsSystemStatus>,
) -> StorageSummarySpec {
    match catalog {
        Some(catalog) => StorageSummarySpec {
            title: "Model cache",
            state: model_cache_capacity_label(catalog.hardware.model_dir_available_gb),
            ready: model_cache_capacity_ready(catalog.hardware.model_dir_available_gb),
            detail: model_cache_capacity_detail(catalog),
        },
        None => match system {
            Some(_system) => StorageSummarySpec {
                title: "Model cache",
                state: "waiting",
                ready: false,
                detail: "Waiting for model-cache capacity from Goblins OS.".to_string(),
            },
            None => StorageSummarySpec {
                title: "Model cache",
                state: "waiting",
                ready: false,
                detail: "Waiting for model-cache capacity.".to_string(),
            },
        },
    }
}

fn mounted_storage_summary_spec(hardware: Option<&HardwareStatus>) -> StorageSummarySpec {
    match hardware {
        Some(hardware) if !hardware.storage.is_empty() => {
            let volume = most_constrained_storage_volume(&hardware.storage)
                .expect("non-empty storage volumes include a constrained volume");
            let state = storage_pressure_label(volume.total_gb, volume.available_gb);
            let count = hardware.storage.len();
            let noun = if count == 1 { "volume" } else { "volumes" };
            StorageSummarySpec {
                title: "Mounted volumes",
                state,
                ready: storage_pressure_ready(volume.total_gb, volume.available_gb),
                detail: format!(
                    "{count} mounted {noun} reported. Most constrained: {} · {} · {}.",
                    storage_volume_title(volume),
                    storage_volume_detail(volume),
                    storage_capacity_percent_text(volume.total_gb, volume.available_gb)
                ),
            }
        }
        Some(_) => StorageSummarySpec {
            title: "Mounted volumes",
            state: "none",
            ready: false,
            detail: "No mounted storage volumes were reported by Goblins OS.".to_string(),
        },
        None => StorageSummarySpec {
            title: "Mounted volumes",
            state: "waiting",
            ready: false,
            detail: "Waiting for mounted-volume capacity.".to_string(),
        },
    }
}

fn os_state_storage_summary_spec(system: Option<&SettingsSystemStatus>) -> StorageSummarySpec {
    match system {
        Some(system) => {
            let missing = missing_storage_path_names(&system.storage);
            if missing.is_empty() {
                StorageSummarySpec {
                    title: "Private system state",
                    state: "configured",
                    ready: true,
                    detail: "Setup, session, policy, assistant runtime, and secret storage are configured. Secrets stay OS-owned and are never displayed.".to_string(),
                }
            } else {
                StorageSummarySpec {
                    title: "Private system state",
                    state: "incomplete",
                    ready: false,
                    detail: format!(
                        "Missing private system storage configuration for {}.",
                        missing.join(", ")
                    ),
                }
            }
        }
        None => StorageSummarySpec {
            title: "Private system state",
            state: "waiting",
            ready: false,
            detail: "Waiting for private system storage status.".to_string(),
        },
    }
}

fn private_system_storage_boundary_detail(spec: &StorageSummarySpec) -> String {
    if spec.ready {
        "Setup, session, policy, assistant runtime, and secret vault data stay in private OS-owned storage. Secrets are never displayed."
            .to_string()
    } else {
        spec.detail.clone()
    }
}

fn missing_storage_path_names(storage: &StorageSettings) -> Vec<&'static str> {
    [
        ("model cache", storage.model_dir.as_str()),
        ("installer", storage.installer_state_dir.as_str()),
        ("session", storage.session_state_dir.as_str()),
        ("policy", storage.policy_state_dir.as_str()),
        ("assistant runtime", storage.resident_state_dir.as_str()),
        ("secrets", storage.secrets_dir.as_str()),
    ]
    .into_iter()
    .filter_map(|(name, path)| path.trim().is_empty().then_some(name))
    .collect()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn storage_volume_row(volume: &StorageVolume) -> gtk4::Box {
    use gtk4::prelude::*;

    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    row.add_css_class("gos-row");
    row.add_css_class("gos-storage-row");

    let title = storage_volume_title(volume);
    let detail = storage_volume_detail(volume);
    set_accessible_label_description(&row, &title, &detail);

    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    copy.set_hexpand(true);
    copy.append(&label(&title, &["gos-row-title"]));
    copy.append(&label(&detail, &["gos-row-copy"]));
    head.append(&copy);

    let status = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    status.set_valign(gtk4::Align::Center);
    status.set_halign(gtk4::Align::End);
    status.append(&label(
        &storage_capacity_percent_text(volume.total_gb, volume.available_gb),
        &["gos-row-value"],
    ));
    status.append(&status_pill(
        storage_pressure_label(volume.total_gb, volume.available_gb),
        storage_pressure_ready(volume.total_gb, volume.available_gb),
    ));
    head.append(&status);
    row.append(&head);

    let fraction = storage_used_fraction(volume.total_gb, volume.available_gb);
    let value_text = storage_capacity_percent_text(volume.total_gb, volume.available_gb);
    let meter = gtk4::ProgressBar::new();
    meter.add_css_class("gos-storage-meter");
    meter.set_fraction(fraction);
    meter.set_show_text(false);
    meter.set_tooltip_text(Some(&detail));
    meter.update_property(&[
        gtk4::accessible::Property::Label(&title),
        gtk4::accessible::Property::Description(&detail),
        gtk4::accessible::Property::ValueMin(0.0),
        gtk4::accessible::Property::ValueMax(1.0),
        gtk4::accessible::Property::ValueNow(fraction),
        gtk4::accessible::Property::ValueText(&value_text),
    ]);
    row.append(&meter);

    row
}

/// Render the live `bootc status` deployments (booted / rollback / staged) as
/// read-only tiles, or the honest "bootc not reachable" detail when the route
/// reports it. This is real deployment state from core, never fabricated.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_system_image_deployment(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    let Some(image) = state.system_image.as_ref() else {
        return;
    };
    panel.append(&label("Deployments", &["gos-subsection-title"]));
    if !image.available {
        panel.append(&system_row("Deployment status", &image.detail));
        return;
    }

    let mut rows: Vec<(&str, String, bool, String)> = Vec::new();
    if let Some(booted) = image.booted.as_ref() {
        rows.push((
            "Booted image",
            deployment_value(booted, "current"),
            true,
            deployment_detail(booted),
        ));
    }
    match (image.rollback_available, image.rollback.as_ref()) {
        (true, Some(rollback)) => rows.push((
            "Rollback image",
            deployment_value(rollback, "available"),
            true,
            deployment_detail(rollback),
        )),
        _ => rows.push((
            "Rollback image",
            "none".to_string(),
            false,
            "No previous deployment is recorded to roll back to yet.".to_string(),
        )),
    }
    if image.staged_available {
        if let Some(staged) = image.staged.as_ref() {
            rows.push((
                "Staged update",
                deployment_value(staged, "staged"),
                true,
                deployment_detail(staged),
            ));
        }
    }
    panel.append(&health_summary_group(rows));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn deployment_value(deployment: &ImageDeployment, fallback: &str) -> String {
    deployment
        .version
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            deployment
                .digest_short
                .clone()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn deployment_detail(deployment: &ImageDeployment) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(image) = deployment
        .image
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        match deployment
            .transport
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            Some(transport) => parts.push(format!("{transport}:{image}")),
            None => parts.push(image.clone()),
        }
    }
    if let Some(digest) = deployment
        .digest_short
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("digest {digest}"));
    }
    if let Some(timestamp) = deployment
        .timestamp
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("deployed {timestamp}"));
    }
    if parts.is_empty() {
        "Reported by bootc status.".to_string()
    } else {
        parts.join(" · ")
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_updates_about(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Updates & About",
        "Software update status, device identity, and recovery readiness.",
    );
    append_updates_about_summary(panel, state);
    match &state.system {
        Some(system) => {
            panel.append(&label("System Image", &["gos-subsection-title"]));
            panel.append(&health_row(
                "Installed image",
                bootc_image_status_label(&system.services.bootc_image),
                bootc_image_configured(&system.services.bootc_image),
                &bootc_image_detail(system),
            ));
            panel.append(&health_row(
                "Update engine",
                ready_word(system.services.bootc_available),
                system.services.bootc_available,
                "Required for checking, applying, and rolling back OS updates.",
            ));

            append_system_image_deployment(panel, state);

            panel.append(&label("Update Readiness", &["gos-subsection-title"]));
            panel.append(&health_row(
                "System health",
                ready_word(system.services.systemctl_available),
                system.services.systemctl_available,
                "Required for update and recovery checks.",
            ));
            panel.append(&health_row(
                "Network",
                ready_word(system.services.network_manager_available),
                system.services.network_manager_available,
                "Internet access is required to check for online updates.",
            ));

            panel.append(&label("Update actions", &["gos-subsection-title"]));
            panel.append(&system_row(
                "Check, apply, and rollback",
                &bootc_update_actions_detail(system),
            ));

            panel.append(&label("About", &["gos-subsection-title"]));
            panel.append(&system_row("Desktop", &desktop_session_detail(system)));
        }
        None => panel.append(&system_row("OS identity", "Waiting for system settings.")),
    }
    append_facility_status(
        panel,
        state,
        "boot-image",
        "Boot image management",
        "Waiting for boot image tooling status.",
    );
    append_native_updates_handoff(panel);
    panel.append(&label("Advanced controls", &["gos-subsection-title"]));
    append_device_settings_handoff(
        panel,
        SettingsPanel::UpdatesAbout,
        "About system tool",
        "Open the desktop system tool for device identity and About controls.",
        state.system.as_ref(),
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_updates_about_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Update summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-updates-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        boot_image_summary_spec(state.system.as_ref()),
        update_readiness_summary_spec(state.system.as_ref()),
        desktop_session_summary_spec(state.system.as_ref()),
        device_identity_summary_spec(state.system.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-updates-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_native_updates_handoff(panel: &gtk4::Box) {
    use gtk4::prelude::*;

    panel.append(&label("Update tools", &["gos-subsection-title"]));
    append_native_app_handoff(
        panel,
        "Software",
        "Software",
        GNOME_SOFTWARE,
        "review application and OS update surfaces",
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_developer(panel: &gtk4::Box, state: &SettingsState, config: &SettingsConfig) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Diagnostics",
        "Service health, app activity, logs, update times, and device status.",
    );
    append_developer_summary(panel, state, config);
    append_native_diagnostics_handoffs(panel);

    panel.append(&label("Diagnostic status", &["gos-subsection-title"]));
    panel.append(&system_row(
        "Local diagnostics",
        if state.core_ready {
            "Settings is connected to local diagnostics."
        } else {
            "Waiting for local diagnostics."
        },
    ));
    panel.append(&health_row(
        "Local services",
        if state.core_ready { "ready" } else { "waiting" },
        state.core_ready,
        "Settings reads local diagnostic status on this device.",
    ));
    if let Some(system) = &state.system {
        panel.append(&system_row(
            "System status",
            &generated_status_sentence(&system.generated_at),
        ));
    }
    if let Some(hardware) = &state.hardware {
        panel.append(&system_row(
            "Hardware status",
            &generated_status_sentence(&hardware.generated_at),
        ));
    }
    if let Some(services) = &state.system_services {
        panel.append(&system_row(
            "Service locations",
            &diagnostic_service_location_detail(services),
        ));
    }
    panel.append(&label("Hardware", &["gos-subsection-title"]));
    append_hardware_settings(panel, state);
    panel.append(&label("Goblins AI runtime", &["gos-subsection-title"]));
    append_resident_settings(panel, state);
    panel.append(&label("System services", &["gos-subsection-title"]));
    append_os_service_settings(panel, state);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_developer_summary(panel: &gtk4::Box, state: &SettingsState, config: &SettingsConfig) {
    use gtk4::prelude::*;

    panel.append(&label("Diagnostics summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-developer-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        developer_core_summary_spec(state.core_ready, config),
        developer_desktop_summary_spec(state.system.as_ref(), state.hardware.as_ref()),
        developer_services_summary_spec(state.system_services.as_ref()),
        developer_resident_summary_spec(state.resident.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-developer-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_native_diagnostics_handoffs(panel: &gtk4::Box) {
    use gtk4::prelude::*;

    panel.append(&label("Diagnostics utilities", &["gos-subsection-title"]));
    append_native_app_handoff(
        panel,
        "System Monitor",
        "System Monitor",
        GNOME_SYSTEM_MONITOR,
        "inspect running processes and resource usage",
    );
    append_native_app_handoff(
        panel,
        "Logs",
        "Logs",
        GNOME_LOGS,
        "review system and service logs",
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_native_app_handoff(
    panel: &gtk4::Box,
    title: &'static str,
    app_label: &'static str,
    command: &'static str,
    purpose: &'static str,
) {
    use gtk4::prelude::*;

    let available = native_app_available(command);
    let detail = native_app_handoff_detail(app_label, purpose, available);

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
    row.add_css_class("gos-row");
    row.add_css_class("gos-switch-row");
    row.add_css_class("gos-device-handoff-row");
    set_accessible_label_description(
        &row,
        title,
        &native_app_handoff_accessibility(app_label, available),
    );

    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    copy.set_hexpand(true);
    copy.append(&label(title, &["gos-row-title"]));
    let detail_label = label(&detail, &["gos-row-copy"]);
    copy.append(&detail_label);
    row.append(&copy);

    let controls = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    controls.add_css_class("gos-handoff-controls");
    controls.set_valign(gtk4::Align::Center);
    controls.set_halign(gtk4::Align::End);
    let status = settings_status_pill(native_handoff_status_label(available), available);
    status.set_halign(gtk4::Align::End);
    controls.append(&status);

    let action = button(
        if available { "Open" } else { "Not Included" },
        &["gos-permission-action", "gos-device-handoff-action"],
    );
    action.set_sensitive(available);
    action.set_valign(gtk4::Align::Center);
    set_accessible_label_description(
        &action,
        title,
        &native_app_handoff_accessibility(app_label, available),
    );
    if available {
        let detail_label = detail_label.clone();
        action.connect_clicked(move |_| match launch_native_app(command) {
            Ok(()) => detail_label.set_text(&format!("Opening {app_label}.")),
            Err(error) => {
                detail_label.set_text(&format!(
                    "{app_label} could not be opened from this session."
                ));
                eprintln!("settings_native_app_launch_error app={app_label:?} error={error}");
            }
        });
    }

    controls.append(&action);
    row.append(&controls);
    panel.append(&row);
}

fn native_handoff_status_label(available: bool) -> &'static str {
    if available {
        "ready"
    } else {
        "not included"
    }
}

fn native_app_handoff_detail(app_label: &str, purpose: &str, available: bool) -> String {
    if available {
        format!("{app_label} lets you {purpose}. Related Goblins OS status stays on this page.")
    } else {
        format!("{app_label} is included in the full Goblins OS image. Start from that image, or add the utility to this build, before opening it here.")
    }
}

fn native_app_handoff_accessibility(app_label: &str, available: bool) -> String {
    if available {
        format!("Open {app_label}.")
    } else {
        format!("{app_label} is not included in this build; this action is disabled.")
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn native_app_available(command: &str) -> bool {
    executable_in_path(command)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn launch_native_app(command: &str) -> std::io::Result<()> {
    std::process::Command::new(command).spawn().map(|_| ())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_os_service_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    match &state.system_services {
        Some(status) => {
            panel.append(&system_row(
                "Service health",
                &diagnostic_service_runtime_detail(status),
            ));
            for service in &status.services {
                panel.append(&system_row(
                    &diagnostic_status_row_title(&service.label, &service.state),
                    &diagnostic_service_detail(service),
                ));
            }
        }
        None => panel.append(&system_row("Service health", "Waiting for service health.")),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_hardware_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    match &state.hardware {
        Some(hardware) => {
            panel.append(&system_row(
                "Hardware platform",
                &diagnostic_hardware_platform_detail(hardware),
            ));
            for facility in &hardware.facilities {
                panel.append(&system_row(
                    &diagnostic_status_row_title(&facility.label, &facility.state),
                    &diagnostic_facility_detail(facility),
                ));
            }
        }
        None => panel.append(&system_row(
            "Hardware",
            "Waiting for the OS hardware manager.",
        )),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_resident_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    match &state.resident {
        Some(resident) => {
            panel.append(&system_row(
                "Goblins AI runtime",
                &resident_process_summary_detail(resident),
            ));
            panel.append(&system_row(
                "Goblins AI route",
                &diagnostic_resident_relay_detail(resident),
            ));
            for capability in &resident.capabilities {
                panel.append(&system_row(
                    &diagnostic_status_row_title(&capability.label, &capability.state),
                    &diagnostic_capability_detail(capability),
                ));
            }
        }
        None => panel.append(&system_row(
            "Goblins AI runtime",
            "Waiting for Goblins AI runtime status from Goblins OS.",
        )),
    }
}

fn diagnostic_status_row_title(label: &str, state: &str) -> String {
    format!("{label} · {}", settings_status_display_label(state))
}

fn diagnostic_service_detail(service: &OsServiceStatus) -> String {
    diagnostic_detail_copy(&service.detail)
}

fn diagnostic_facility_detail(facility: &SystemFacility) -> String {
    diagnostic_detail_copy(&facility.detail)
}

fn diagnostic_capability_detail(capability: &ResidentCapability) -> String {
    diagnostic_detail_copy(&capability.detail)
}

fn diagnostic_detail_copy(detail: &str) -> String {
    let without_paths = detail
        .replace(DEFAULT_CORE_URL, "local diagnostics")
        .replace("/usr/lib/systemd/system", "system service files")
        .replace("/usr/libexec/goblins-os", "OS service tools")
        .replace("/var/lib/goblins-os", "private OS storage");
    settings_detail_display_copy(&without_paths)
}

fn diagnostic_service_location_detail(status: &SystemServicesStatus) -> String {
    if status.unit_dir.trim().is_empty() || status.libexec_dir.trim().is_empty() {
        "Service checks are not fully configured yet.".to_string()
    } else {
        "Service checks are configured for this device.".to_string()
    }
}

fn diagnostic_service_runtime_detail(status: &SystemServicesStatus) -> String {
    if status.manager_available {
        "System services are available for health checks.".to_string()
    } else {
        "System service status is not available yet.".to_string()
    }
}

fn diagnostic_hardware_platform_detail(hardware: &HardwareStatus) -> String {
    let session_ready = configured_runtime_value(&hardware.platform.desktop)
        || configured_runtime_value(&hardware.platform.session_type)
        || configured_runtime_value(&hardware.platform.current_desktop);
    let session = if session_ready {
        "Desktop session status is available"
    } else {
        "Desktop session status is not fully reported"
    };
    format!(
        "{session}. Memory {} of {} GB available.",
        hardware.memory.available_gb, hardware.memory.total_gb
    )
}

fn diagnostic_resident_relay_detail(resident: &ResidentStatus) -> String {
    match (
        resident.engine.cloud_relay_configured,
        resident.engine.local_relay_configured,
    ) {
        (true, true) => "Cloud and local assistant routes are configured.".to_string(),
        (true, false) => "Cloud assistant route is configured; local route is waiting.".to_string(),
        (false, true) => "Local assistant route is configured; cloud route is waiting.".to_string(),
        (false, false) => "Assistant routes are not configured yet.".to_string(),
    }
}

struct DeveloperSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn developer_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> DeveloperSummarySpec {
    DeveloperSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn developer_core_summary_spec(core_ready: bool, _config: &SettingsConfig) -> DeveloperSummarySpec {
    developer_summary_spec(
        "Local diagnostics",
        if core_ready { "ready" } else { "waiting" },
        core_ready,
        if core_ready {
            "Settings is connected to local diagnostics."
        } else {
            "Waiting for local diagnostics."
        },
    )
}

fn developer_desktop_summary_spec(
    system: Option<&SettingsSystemStatus>,
    hardware: Option<&HardwareStatus>,
) -> DeveloperSummarySpec {
    match (system, hardware) {
        (Some(system), Some(hardware)) => developer_summary_spec(
            "Desktop",
            developer_desktop_state(Some(system), Some(hardware)),
            developer_desktop_runtime_ready(Some(system), Some(hardware)),
            developer_desktop_detail(Some(system), Some(hardware)),
        ),
        (Some(system), None) => developer_summary_spec(
            "Desktop",
            developer_desktop_state(Some(system), None),
            developer_desktop_runtime_ready(Some(system), None),
            developer_desktop_detail(Some(system), None),
        ),
        (None, Some(hardware)) => developer_summary_spec(
            "Desktop",
            developer_desktop_state(None, Some(hardware)),
            developer_desktop_runtime_ready(None, Some(hardware)),
            developer_desktop_detail(None, Some(hardware)),
        ),
        (None, None) => {
            developer_summary_spec("Desktop", "waiting", false, "Waiting for desktop status.")
        }
    }
}

fn developer_desktop_state(
    system: Option<&SettingsSystemStatus>,
    hardware: Option<&HardwareStatus>,
) -> &'static str {
    if developer_desktop_runtime_ready(system, hardware) {
        "configured"
    } else {
        "waiting"
    }
}

fn developer_desktop_detail(
    system: Option<&SettingsSystemStatus>,
    hardware: Option<&HardwareStatus>,
) -> String {
    match (system, hardware) {
        (Some(system), Some(hardware)) => {
            let desktop = desktop_session_detail(system);
            let hardware = diagnostic_hardware_platform_detail(hardware);
            format!("{desktop} {hardware}")
        }
        (Some(system), None) => format!(
            "{} Waiting for hardware status.",
            desktop_session_detail(system)
        ),
        (None, Some(hardware)) => format!(
            "{} Waiting for desktop session status.",
            diagnostic_hardware_platform_detail(hardware)
        ),
        (None, None) => "Waiting for desktop and hardware status.".to_string(),
    }
}

fn developer_desktop_runtime_ready(
    system: Option<&SettingsSystemStatus>,
    hardware: Option<&HardwareStatus>,
) -> bool {
    let system_ready = system.is_none_or(|system| {
        configured_runtime_value(&system.session.desktop)
            && configured_runtime_value(&system.session.gui_platform)
            && configured_runtime_value(&system.session.shell_mode)
    });
    let hardware_ready = hardware.is_none_or(|hardware| {
        configured_runtime_value(&hardware.platform.os)
            && configured_runtime_value(&hardware.platform.session_type)
            && configured_runtime_value(&hardware.platform.current_desktop)
    });

    (system.is_some() || hardware.is_some()) && system_ready && hardware_ready
}

fn configured_runtime_value(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    !normalized.is_empty()
        && !matches!(
            normalized.as_str(),
            "unconfigured" | "not-configured" | "unknown" | "none" | "waiting"
        )
}

fn developer_services_summary_spec(status: Option<&SystemServicesStatus>) -> DeveloperSummarySpec {
    let Some(status) = status else {
        return developer_summary_spec(
            "System services",
            "waiting",
            false,
            "Waiting for service status.",
        );
    };

    let counts = developer_service_counts(&status.services);
    developer_summary_spec(
        "System services",
        developer_service_counts_label(status.manager_available, counts),
        status.manager_available && counts.total > 0 && counts.blocked == 0,
        format!(
            "{} of {} services match their expected state. {}",
            counts.ready,
            counts.total,
            generated_status_sentence(&status.generated_at)
        ),
    )
}

fn developer_resident_summary_spec(resident: Option<&ResidentStatus>) -> DeveloperSummarySpec {
    match resident {
        Some(resident) => developer_summary_spec(
            "Goblins AI runtime",
            resident.process.state.as_str(),
            model_resident_ready(resident),
            resident_process_summary_detail(resident),
        ),
        None => developer_summary_spec(
            "Goblins AI runtime",
            "waiting",
            false,
            "Waiting for Goblins AI runtime status.",
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeveloperServiceCounts {
    ready: usize,
    blocked: usize,
    total: usize,
}

fn developer_service_counts(services: &[OsServiceStatus]) -> DeveloperServiceCounts {
    let ready = services
        .iter()
        .filter(|service| service.state == service.expected_state)
        .count();
    DeveloperServiceCounts {
        ready,
        blocked: services.len().saturating_sub(ready),
        total: services.len(),
    }
}

fn developer_service_counts_label(
    manager_available: bool,
    counts: DeveloperServiceCounts,
) -> String {
    if !manager_available {
        "unavailable".to_string()
    } else if counts.total == 0 {
        "empty".to_string()
    } else if counts.blocked == 0 {
        "ready".to_string()
    } else {
        format!("{} need attention", counts.blocked)
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_model_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    match &state.local_models {
        Some(catalog) => {
            let core_url = config_core_url(state);
            panel.append(&system_row(
                "Local model policy",
                &format!(
                    "{} · engine {}",
                    catalog.install_policy,
                    runtime_label(&catalog.hardware.runtime)
                ),
            ));
            for model in &catalog.models {
                panel.append(&local_model_row(&core_url, model));
            }
        }
        None => panel.append(&system_row(
            "Local models",
            "Waiting for the OS model manager.",
        )),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn local_model_row(core_url: &str, model: &LocalModelOption) -> gtk4::Box {
    use gtk4::prelude::*;

    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    row.add_css_class("gos-row");
    row.add_css_class("gos-model-row");

    let title = format!("{} · {}", model.name, local_model_state_label(model));
    let detail = local_model_detail(model);
    set_accessible_label_description(&row, &title, &detail);

    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    copy.set_hexpand(true);
    copy.append(&label(&model.name, &["gos-row-title"]));
    copy.append(&label(&model.role, &["gos-row-copy"]));
    copy.append(&label(&detail, &["gos-row-copy"]));
    head.append(&copy);
    head.append(&status_pill(
        local_model_state_label(model),
        local_model_ready(model),
    ));
    row.append(&head);

    let requirements = label(&local_model_requirements(model), &["gos-row-copy"]);
    requirements.add_css_class("gos-model-requirements");
    row.append(&requirements);

    if let Some(action) = local_model_action_label(model) {
        let feedback = label(local_model_download_disclosure(model), &["gos-row-copy"]);
        let button = button(action, &["gos-permission-action", "gos-model-action"]);
        button.set_tooltip_text(Some(local_model_download_disclosure(model)));
        set_accessible_label_description(
            &button,
            action,
            "Records explicit consent with Goblins OS before any model weight download can start.",
        );
        let core_url = core_url.to_string();
        let model_id = model.id.clone();
        let feedback_clone = feedback.clone();
        button.connect_clicked(move |button| {
            button.set_sensitive(false);
            match install_local_model(&core_url, &model_id) {
                Ok(message) => feedback_clone.set_text(&message),
                Err(error) => {
                    feedback_clone.set_text("Goblins OS could not start that local model install.");
                    eprintln!("settings_local_model_install_error={error}");
                    button.set_sensitive(true);
                }
            }
        });
        row.append(&button);
        row.append(&feedback);
    } else if !model.reasons.is_empty() {
        row.append(&label(&model.reasons.join(" "), &["gos-row-copy"]));
    }

    row
}

/// The bring-your-own OpenAI API key panel. GPT-OSS is the heart of Goblins OS;
/// adding a personal OpenAI API key is the optional way to use OpenAI's hosted
/// models instead. The key is sent once to Goblins OS, which stores it
/// owner-only on disk — it is never displayed back or held in the GUI process.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_openai_key_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("OpenAI models", &["gos-subsection-title"]));

    let status = state.openai_key.as_ref();
    let engine_selected = status.is_some_and(|status| status.engine_selected);
    let configured = status.is_some_and(|status| status.configured);
    let core_url = config_core_url(state);

    // Engine selector — three honest engines: GPT-OSS on this device, the user's
    // OpenAI account via Codex CLI, or a bring-your-own API key. Each option is
    // only selectable once it can actually be honored.
    let _ = engine_selected;
    let active_engine = status
        .map(|status| status.engine.as_str())
        .unwrap_or("local-gpt-oss");
    let codex_ready = state
        .codex
        .as_ref()
        .is_some_and(|codex| codex.installed && codex.authenticated);

    panel.append(&label("Engine", &["gos-kicker"]));
    let choice = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    choice.add_css_class("gos-engine-choice");

    // Two honest primary paths: build on-device with GPT-OSS (keyless, private,
    // the default), or with your OpenAI account through Codex. A bring-your-own
    // API key is an advanced option below, not a co-equal third choice.
    let gpt_btn = button("On-device · GPT-OSS", &["gos-engine-option"]);
    let codex_btn = button("OpenAI account · Codex", &["gos-engine-option"]);
    let hosted_btn = button(
        "Use my OpenAI API key",
        &["gos-engine-option", "gos-engine-advanced"],
    );
    set_accessible_label_description(
        &gpt_btn,
        "Use on-device GPT-OSS",
        "Select the local private engine.",
    );
    set_accessible_label_description(
        &codex_btn,
        "Use OpenAI account through Codex",
        "Select Codex account mode when Codex is ready and signed in.",
    );
    set_accessible_label_description(
        &hosted_btn,
        "Use my OpenAI API key",
        "Select hosted OpenAI models after adding an OS-owned personal API key.",
    );
    // The two primary segments share the column width, flush with the cards' margin.
    gpt_btn.set_hexpand(true);
    codex_btn.set_hexpand(true);
    match active_engine {
        "openai-api" => hosted_btn.add_css_class("gos-engine-active"),
        "codex" => codex_btn.add_css_class("gos-engine-active"),
        _ => gpt_btn.add_css_class("gos-engine-active"),
    }
    codex_btn.set_sensitive(codex_ready);
    hosted_btn.set_sensitive(configured);
    choice.append(&gpt_btn);
    choice.append(&codex_btn);
    panel.append(&choice);

    let engine_feedback = label(engine_active_copy(active_engine), &["gos-row-copy"]);
    panel.append(&engine_feedback);

    // Each engine button moves the active highlight across all three on success.
    {
        let gpt = gpt_btn.clone();
        let codex = codex_btn.clone();
        let hosted = hosted_btn.clone();
        let feedback = engine_feedback.clone();
        let core_url = core_url.clone();
        gpt_btn.connect_clicked(move |_| match set_engine(&core_url, "local-gpt-oss") {
            Ok(detail) => {
                gpt.add_css_class("gos-engine-active");
                codex.remove_css_class("gos-engine-active");
                hosted.remove_css_class("gos-engine-active");
                feedback.set_text(&detail);
            }
            Err(error) => {
                feedback.set_text("Goblins OS could not switch to the on-device engine.");
                eprintln!("settings_engine_error={error}");
            }
        });
    }
    {
        let gpt = gpt_btn.clone();
        let codex = codex_btn.clone();
        let hosted = hosted_btn.clone();
        let feedback = engine_feedback.clone();
        let core_url = core_url.clone();
        codex_btn.connect_clicked(move |_| match set_engine(&core_url, "codex") {
            Ok(detail) => {
                codex.add_css_class("gos-engine-active");
                gpt.remove_css_class("gos-engine-active");
                hosted.remove_css_class("gos-engine-active");
                feedback.set_text(&detail);
            }
            Err(error) => {
                feedback.set_text("Sign in to Codex with your OpenAI account first.");
                eprintln!("settings_engine_error={error}");
            }
        });
    }
    {
        let gpt = gpt_btn.clone();
        let codex = codex_btn.clone();
        let hosted = hosted_btn.clone();
        let feedback = engine_feedback.clone();
        let core_url = core_url.clone();
        hosted_btn.connect_clicked(move |_| match set_engine(&core_url, "openai-api") {
            Ok(detail) => {
                hosted.add_css_class("gos-engine-active");
                gpt.remove_css_class("gos-engine-active");
                codex.remove_css_class("gos-engine-active");
                feedback.set_text(&detail);
            }
            Err(error) => {
                feedback.set_text(
                    "Add an OpenAI API key first, then switch to OpenAI's hosted models.",
                );
                eprintln!("settings_engine_error={error}");
            }
        });
    }

    // Codex sign-in: the honest way to use a real OpenAI account. The OS triggers
    // `codex login` (browser) and never sees the credentials — Codex owns them.
    append_codex_settings(panel, state, &engine_feedback);

    // Advanced: a bring-your-own OpenAI API key (hosted models), kept out of the
    // primary On-device / OpenAI-account choice above.
    panel.append(&label(
        "Advanced · OpenAI API key",
        &["gos-subsection-title"],
    ));
    hosted_btn.set_hexpand(false);
    hosted_btn.set_halign(gtk4::Align::Start);
    panel.append(&hosted_btn);

    match status {
        Some(status) => panel.append(&system_row(
            if status.configured {
                "Personal key · configured"
            } else {
                "Personal key · not set"
            },
            &if status.configured {
                format!(
                    "Hosted model {} · held owner-only at {}",
                    status.model, status.storage
                )
            } else {
                "Add a personal OpenAI API key to use OpenAI's hosted models. Goblins OS stays on GPT-OSS until you do.".to_string()
            },
        )),
        None => panel.append(&system_row(
            "Personal key",
            "Waiting for key status.",
        )),
    }

    let field = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    field.add_css_class("gos-key-field");

    let entry = gtk4::PasswordEntry::new();
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some("sk-…"));
    set_accessible_label_description(
        &entry,
        "OpenAI API key",
        "Paste a personal API key. Settings sends it once to Goblins OS and never displays it again.",
    );
    entry.add_css_class("gos-key-entry");
    field.append(&entry);

    let feedback = label(
        if configured {
            "A personal key is configured. Paste a new key to replace it."
        } else {
            "Your key is stored by the OS, never shown again, and never leaves this device except to call OpenAI."
        },
        &["gos-row-copy"],
    );

    let save = button("Save OpenAI key", &["gos-permission-action"]);
    set_accessible_label_description(
        &save,
        "Save OpenAI API key",
        "Stores the key in OS-owned secret storage through Goblins OS.",
    );
    {
        let entry = entry.clone();
        let feedback = feedback.clone();
        let hosted = hosted_btn.clone();
        let core_url = core_url.clone();
        save.connect_clicked(move |_| {
            let key = entry.text().to_string();
            if key.trim().is_empty() {
                feedback.set_text("Paste your OpenAI API key first.");
                return;
            }
            match set_openai_key(&core_url, key.trim()) {
                Ok(detail) => {
                    entry.set_text("");
                    feedback.set_text(&detail);
                    // The hosted engine is now selectable in this session.
                    hosted.set_sensitive(true);
                }
                Err(error) => {
                    feedback.set_text(
                        "Goblins OS could not store that key — check that it is a valid 'sk-' OpenAI key.",
                    );
                    eprintln!("settings_openai_key_error={error}");
                }
            }
        });
    }

    field.append(&save);
    field.append(&feedback);
    panel.append(&field);
}

/// The Models panel: the focused home for the engine that powers Goblins OS.
/// GPT-OSS runs on-device by default (the heart of the OS); bringing a personal
/// OpenAI API key is the optional way to use OpenAI's hosted models instead.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_models(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Goblin & Models",
        "GPT-OSS is the heart of Goblins OS — it runs on this device, private by default, and \
         builds the apps you ask for. Add your own OpenAI API key to use OpenAI's hosted models \
         instead; the choice is yours and the OS keeps any key owner-only.",
    );

    append_goblins_ai_settings(panel, state);
    append_models_summary(panel, state);
    append_privacy_settings(panel, state);
    append_openai_key_settings(panel, state);
    append_voice_settings(panel, state);

    panel.append(&label("Local models", &["gos-subsection-title"]));
    append_model_settings(panel, state);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_goblins_ai_settings(panel: &gtk4::Box, state: &SettingsState) {
    let Some(catalog) = state.ai_actions.as_ref() else {
        append_preference_group(
            panel,
            "Goblin",
            vec![health_row(
                "Action registry",
                "waiting",
                false,
                "Waiting for the Goblin action catalog from local OS services.",
            )],
        );
        return;
    };

    let entrypoints = ai_entrypoint_summary(catalog);
    append_preference_group(
        panel,
        "Goblin",
        vec![
            health_row(
                "Assistant engine",
                ai_engine_state_label(catalog),
                catalog.engine.ready,
                &catalog.engine.detail,
            ),
            health_row(
                "Action registry",
                &format!("{} actions", catalog.actions.len()),
                true,
                &format!(
                    "{} Registry {}.",
                    catalog.permission_model, catalog.registry_version
                ),
            ),
            health_row(
                "Entry points",
                "system-wide",
                true,
                &format!("Available from {entrypoints}."),
            ),
        ],
    );

    let rows = [
        "ask-goblins",
        "open-settings-panel",
        "change-safe-setting",
        "ask-selected-text",
        "summarize-screen",
        "ask-file-or-folder",
        "answer-notification",
        "troubleshoot-network-audio-display-storage",
        "build-app",
    ]
    .iter()
    .filter_map(|id| ai_action_by_id(catalog, id))
    .map(|action| {
        health_row(
            &action.title,
            ai_action_state_label(action),
            ai_action_state_ok(action),
            &ai_action_detail(action),
        )
    })
    .collect::<Vec<_>>();

    append_preference_group(panel, "Goblin actions", rows);
    append_goblins_ai_history(panel, state.ai_action_history.as_ref());
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_goblins_ai_history(panel: &gtk4::Box, history: Option<&AiActionHistory>) {
    let Some(history) = history else {
        append_preference_group(
            panel,
            "Recent Goblins AI actions",
            vec![health_row(
                "Action history",
                "checking",
                false,
                "Checking the OS action history. Goblins AI history stores metadata only.",
            )],
        );
        return;
    };

    if history.events.is_empty() {
        append_preference_group(
            panel,
            "Recent Goblins AI actions",
            vec![health_row(
                "Action history",
                "empty",
                true,
                &history.retention,
            )],
        );
        return;
    }

    let rows = history
        .events
        .iter()
        .take(5)
        .map(|event| {
            let title = format!(
                "{} · {}",
                event.title,
                readable_runtime_value(&event.entrypoint)
            );
            health_row(
                &title,
                ai_history_state_label(event),
                ai_history_state_ok(event),
                &ai_history_detail(event),
            )
        })
        .collect::<Vec<_>>();

    append_preference_group(panel, "Recent Goblins AI actions", rows);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_settings_ai_help(panel: &gtk4::Box, active_panel: SettingsPanel, state: &SettingsState) {
    use gtk4::prelude::*;

    let core_url = config_core_url(state);
    let readiness = settings_ai_help_readiness(state);
    let entry = gtk4::Entry::new();
    entry.add_css_class("gos-search-entry");
    entry.set_placeholder_text(Some("Ask about this setting or describe the problem"));
    entry.update_property(&[gtk4::accessible::Property::Label(
        "Ask Goblin about this Settings panel",
    )]);

    let ask = button("Ask Goblin", &["gos-primary-button"]);
    ask.set_sensitive(readiness.enabled);
    ask.update_property(&[gtk4::accessible::Property::Description(
        readiness.detail.as_str(),
    )]);

    let feedback = label(&readiness.detail, &["gos-row-copy"]);
    feedback.set_wrap(true);
    feedback.set_selectable(true);
    feedback.set_xalign(0.0);

    let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    controls.add_css_class("gos-inline-controls");
    entry.set_hexpand(true);
    controls.append(&entry);
    controls.append(&ask);

    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    row.add_css_class("gos-row");
    row.add_css_class("gos-system-row");
    row.append(&label(
        &format!("Ask Goblin about {}", active_panel.display_name()),
        &["gos-row-title"],
    ));
    row.append(&controls);
    row.append(&feedback);
    set_accessible_label_description(
        &row,
        "Ask Goblin about this Settings panel",
        readiness.detail.as_str(),
    );

    let panel_summary = settings_ai_panel_status_summary(active_panel, state);
    let topic = active_panel.display_name().to_string();
    let panel_id = active_panel.as_str().to_string();
    ask.connect_clicked(move |_| {
        let question = entry.text().to_string();
        feedback.set_text("Asking Goblin through the OS-owned Settings context route.");
        match ask_settings_context(&core_url, &panel_id, &topic, &question, &panel_summary) {
            Ok(answer) => feedback.set_text(&answer),
            Err(detail) => feedback.set_text(&detail),
        }
    });

    append_preference_group(panel, "Goblin help", vec![row]);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
struct SettingsAiHelpReadiness {
    enabled: bool,
    detail: String,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_ai_help_readiness(state: &SettingsState) -> SettingsAiHelpReadiness {
    let Some(catalog) = state.ai_actions.as_ref() else {
        return SettingsAiHelpReadiness {
            enabled: false,
            detail: "Goblin help appears after Settings reaches the OS action catalog.".to_string(),
        };
    };

    let action = ai_action_by_id(catalog, "explain-system-status")
        .or_else(|| ai_action_by_id(catalog, "troubleshoot-network-audio-display-storage"));
    match action {
        Some(action) if action.enabled => SettingsAiHelpReadiness {
            enabled: true,
            detail: "Ask a question about this panel. Settings sends only the panel name and a bounded OS status summary through local OS services.".to_string(),
        },
        Some(action) => SettingsAiHelpReadiness {
            enabled: false,
            // User-facing help caption: the plain reason only, never the internal
            // action taxonomy (context/permission/confirmation/entry-points).
            detail: format!("{} {}", action.detail.trim(), action.reason.trim())
                .trim()
                .to_string(),
        },
        None => SettingsAiHelpReadiness {
            enabled: false,
            detail: "Goblin help is waiting for the Settings troubleshooting action from local OS services.".to_string(),
        },
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn ai_history_state_label(event: &AiActionHistoryEvent) -> &str {
    match event.outcome.as_str() {
        "succeeded" => "done",
        "started" => "started",
        "permission-gated" => "permission",
        "confirmation-required" => "confirm",
        "blocked" => "blocked",
        "denied" => "denied",
        "failed" => "failed",
        _ => "recorded",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn ai_history_state_ok(event: &AiActionHistoryEvent) -> bool {
    matches!(event.outcome.as_str(), "succeeded" | "started")
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn ai_history_detail(event: &AiActionHistoryEvent) -> String {
    format!(
        "{} Permission: {}. Confirmation: {}.",
        event.detail,
        readable_runtime_value(&event.permission_control),
        readable_runtime_value(&event.confirmation)
    )
}

fn ai_action_by_id<'a>(catalog: &'a AiActionCatalog, id: &str) -> Option<&'a AiActionStatus> {
    catalog.actions.iter().find(|action| action.id == id)
}

fn ai_engine_state_label(catalog: &AiActionCatalog) -> &'static str {
    if catalog.engine.ready {
        "ready"
    } else {
        "waiting"
    }
}

fn ai_action_state_label(action: &AiActionStatus) -> &'static str {
    match action.state.as_str() {
        "ready" => "ready",
        "confirmation-required" => "confirm",
        "permission-gated" => "permission",
        "waiting-for-engine" => "waiting",
        "denied" => "denied",
        _ => "waiting",
    }
}

fn ai_action_state_ok(action: &AiActionStatus) -> bool {
    matches!(action.state.as_str(), "ready" | "confirmation-required")
}

fn ai_action_detail(action: &AiActionStatus) -> String {
    let context = action
        .contexts
        .iter()
        .map(|value| readable_runtime_value(value))
        .collect::<Vec<_>>()
        .join(", ");
    let entrypoints = action
        .entrypoints
        .iter()
        .map(|value| readable_runtime_value(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} {} Context: {}. Permission: {}. Confirmation: {}. Entry points: {}.",
        action.detail,
        action.reason,
        context,
        readable_runtime_value(&action.permission_control),
        readable_runtime_value(&action.confirmation),
        entrypoints
    )
}

fn ai_entrypoint_summary(catalog: &AiActionCatalog) -> String {
    let mut entrypoints = catalog
        .actions
        .iter()
        .flat_map(|action| action.entrypoints.iter().map(String::as_str))
        .collect::<Vec<_>>();
    entrypoints.sort_unstable();
    entrypoints.dedup();
    entrypoints
        .into_iter()
        .map(readable_runtime_value)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_models_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Model summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-model-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        active_engine_summary_spec(
            state.openai_key.as_ref(),
            state.codex.as_ref(),
            state.privacy.as_ref(),
            state.resident.as_ref(),
        ),
        local_model_summary_spec(state.local_models.as_ref()),
        openai_access_summary_spec(state.openai_key.as_ref(), state.codex.as_ref()),
        voice_model_summary_spec(state.voice.as_ref()),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, &item.state, item.ready, &item.detail);
        row.add_css_class("gos-model-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

/// The local-voice status. Voice is built from on-device Whisper + Piper, so it
/// works the same with GPT-OSS or a key and stays private; this row reports
/// whether it is ready and what to add.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_voice_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Voice", &["gos-subsection-title"]));
    match &state.voice {
        Some(voice) => {
            let title = if voice.available {
                format!("{} · voice ready", voice.wake_word())
            } else {
                format!("{} · add local voice", voice.wake_word())
            };
            let detail = voice_settings_detail(voice);
            panel.append(&system_row(&title, &detail));
        }
        None => panel.append(&system_row("Voice", "Waiting for voice capability.")),
    }
}

/// The OS color scheme: Light, Dark, or Auto. Saved in OS-owned state, applied
/// live to this window, and honored by every native surface — the OS is never
/// locked to one scheme. Auto follows the system's preference.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_appearance_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Appearance", &["gos-subsection-title"]));

    let Some(appearance) = &state.appearance else {
        panel.append(&system_row(
            "Color scheme",
            "Waiting for appearance status.",
        ));
        return;
    };

    panel.append(&system_row(
        "Color scheme",
        "Light, Dark, or Auto. Goblins OS saves this as the system appearance setting so native apps can follow the same choice.",
    ));

    if !appearance.color_scheme_available {
        panel.append(&system_row("Appearance control", &appearance.detail));
        return;
    }

    let current = normalized_appearance_theme(&appearance.theme);
    let core_url = config_core_url(state);
    let feedback = label(appearance_scheme_detail(current), &["gos-row-copy"]);
    panel.append(&feedback);

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    row.add_css_class("gos-segmented-control");
    for (theme, name) in [("light", "Light"), ("dark", "Dark"), ("auto", "Auto")] {
        let option = button(name, &["gos-segmented-option"]);
        // The three segments share the column's full width, ending flush on the
        // same right margin as the cards above and below.
        option.set_hexpand(true);
        set_accessible_label_description(
            &option,
            &format!("{name} appearance"),
            &format!("Set the standard desktop appearance preference to {name}."),
        );
        if theme == current {
            option.add_css_class("is-selected");
        }
        let core_url = core_url.clone();
        let feedback = feedback.clone();
        option.connect_clicked(
            move |option| match set_appearance_color_scheme(&core_url, theme) {
                Ok(outcome) => {
                    if let Some(parent) = option.parent() {
                        let mut child = parent.first_child();
                        while let Some(widget) = child {
                            widget.remove_css_class("is-selected");
                            child = widget.next_sibling();
                        }
                    }
                    option.add_css_class("is-selected");
                    feedback.set_text(&outcome);
                }
                Err(error) => feedback.set_text(&error),
            },
        );
        row.append(&option);
    }
    panel.append(&row);
}

/// The offline / private-mode control. When on, the AI is held to this device and
/// never reaches the internet; Goblins OS enforces it, this only flips the flag.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_privacy_settings(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Privacy", &["gos-subsection-title"]));

    let Some(privacy) = state.privacy.as_ref() else {
        panel.append(&system_row(
            "Private mode",
            privacy_control_waiting_detail(),
        ));
        return;
    };

    let offline = privacy.offline;
    let detail = privacy.detail.clone();

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
    row.add_css_class("gos-row");
    row.add_css_class("gos-switch-row");

    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    copy.set_hexpand(true);
    let title = label(privacy_state_label(offline), &["gos-row-title"]);
    let detail_label = label(&detail, &["gos-row-copy"]);
    copy.append(&title);
    copy.append(&detail_label);
    row.append(&copy);

    let toggle = gtk4::Switch::new();
    toggle.set_active(offline);
    toggle.set_valign(gtk4::Align::Center);
    toggle.set_tooltip_text(Some("Private mode"));
    set_accessible_label_description(&toggle, "Private mode", &detail);
    row.append(&toggle);

    let feedback = label(
        "Private mode keeps every prompt on this device. Hosted OpenAI models and model downloads need the internet, so they pause while it’s on.",
        &["gos-row-copy"],
    );
    let core_url = config_core_url(state);
    let current_offline = Rc::new(Cell::new(offline));
    let updating_switch = Rc::new(Cell::new(false));
    {
        let feedback = feedback.clone();
        let title = title.clone();
        let detail_label = detail_label.clone();
        let current_offline = current_offline.clone();
        let updating_switch = updating_switch.clone();
        toggle.connect_active_notify(move |toggle| {
            if updating_switch.get() {
                return;
            }
            let next_offline = toggle.is_active();
            if next_offline == current_offline.get() {
                return;
            }

            toggle.set_sensitive(false);
            match set_privacy(&core_url, next_offline) {
                Ok(detail) => {
                    current_offline.set(next_offline);
                    title.set_text(privacy_state_label(next_offline));
                    detail_label.set_text(&detail);
                    feedback.set_text(&detail);
                    toggle.update_property(&[gtk4::accessible::Property::Description(&detail)]);
                }
                Err(error) => {
                    let error_detail = setting_change_rejected_detail(&error.to_string());
                    feedback.set_text(&error_detail);
                    toggle
                        .update_property(&[gtk4::accessible::Property::Description(&error_detail)]);
                    updating_switch.set(true);
                    toggle.set_active(current_offline.get());
                    updating_switch.set(false);
                    eprintln!("settings_privacy_error={error}");
                }
            }
            toggle.set_sensitive(true);
        });
    }
    panel.append(&row);
    panel.append(&feedback);
}

fn privacy_control_waiting_detail() -> &'static str {
    "Disabled: waiting for privacy status."
}

fn privacy_state_label(offline: bool) -> &'static str {
    if offline {
        "Private mode · on"
    } else {
        "Private mode · off"
    }
}

struct PrivacySummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn privacy_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> PrivacySummarySpec {
    PrivacySummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn privacy_private_mode_summary_spec(privacy: Option<&PrivacyStatus>) -> PrivacySummarySpec {
    let Some(privacy) = privacy else {
        return privacy_summary_spec(
            "Private mode",
            "waiting",
            false,
            "Waiting for private-mode status.",
        );
    };

    privacy_summary_spec(
        "Private mode",
        if privacy.offline {
            "private"
        } else {
            "online-capable"
        },
        true,
        privacy.detail.as_str(),
    )
}

fn desktop_privacy_schema_summary_spec(privacy: Option<&PrivacyStatus>) -> PrivacySummarySpec {
    let Some(privacy) = privacy else {
        return privacy_summary_spec(
            "Privacy",
            "waiting",
            false,
            "Waiting for desktop privacy status.",
        );
    };
    let Some(desktop) = privacy.desktop.as_ref() else {
        return privacy_summary_spec(
            "Privacy",
            "waiting",
            false,
            "Waiting for desktop privacy controls.",
        );
    };

    let available = desktop.gsettings_available && desktop.schema_available;
    privacy_summary_spec(
        "Privacy",
        if available {
            "available"
        } else {
            "unavailable"
        },
        available,
        desktop.detail.as_str(),
    )
}

fn privacy_device_access_summary_spec(privacy: Option<&PrivacyStatus>) -> PrivacySummarySpec {
    let Some(privacy) = privacy else {
        return privacy_summary_spec(
            "Device access",
            "waiting",
            false,
            "Waiting for device privacy status.",
        );
    };
    let Some(desktop) = privacy.desktop.as_ref() else {
        return privacy_summary_spec(
            "Device access",
            "waiting",
            false,
            "Waiting for device privacy controls.",
        );
    };

    if !(desktop.gsettings_available && desktop.schema_available) {
        return privacy_summary_spec(
            "Device access",
            "unavailable",
            false,
            desktop.detail.as_str(),
        );
    }

    let known = [
        desktop.disable_microphone,
        desktop.disable_camera,
        desktop.disable_sound_output,
        desktop.usb_protection,
    ]
    .into_iter()
    .filter(Option::is_some)
    .count();
    if known == 0 {
        return privacy_summary_spec(
            "Device access",
            "unknown",
            false,
            "Privacy settings are available, but this session did not report device access controls.",
        );
    }

    let blocked = [
        desktop.disable_microphone,
        desktop.disable_camera,
        desktop.disable_sound_output,
    ]
    .into_iter()
    .filter(|value| *value == Some(true))
    .count();
    let state = if blocked > 0 {
        format!("{blocked} blocked")
    } else if desktop.usb_protection == Some(true) {
        "protected".to_string()
    } else {
        "allowed".to_string()
    };

    privacy_summary_spec(
        "Device access",
        state,
        true,
        format!(
            "Microphone {} · Camera {} · Sound {} · USB {}.",
            privacy_block_word(desktop.disable_microphone),
            privacy_block_word(desktop.disable_camera),
            privacy_block_word(desktop.disable_sound_output),
            privacy_usb_word(desktop.usb_protection)
        ),
    )
}

fn privacy_cleanup_summary_spec(privacy: Option<&PrivacyStatus>) -> PrivacySummarySpec {
    let Some(privacy) = privacy else {
        return privacy_summary_spec(
            "History and cleanup",
            "waiting",
            false,
            "Waiting for cleanup and history status.",
        );
    };
    let Some(desktop) = privacy.desktop.as_ref() else {
        return privacy_summary_spec(
            "History and cleanup",
            "waiting",
            false,
            "Waiting for cleanup and history controls.",
        );
    };

    if !(desktop.gsettings_available && desktop.schema_available) {
        return privacy_summary_spec(
            "History and cleanup",
            "unavailable",
            false,
            desktop.detail.as_str(),
        );
    }

    let known = [
        desktop.remember_recent_files,
        desktop.remember_app_usage,
        desktop.remove_old_trash_files,
        desktop.remove_old_temp_files,
    ]
    .into_iter()
    .filter(Option::is_some)
    .count()
        + usize::from(desktop.old_files_age_days.is_some());
    if known == 0 {
        return privacy_summary_spec(
            "History and cleanup",
            "unknown",
            false,
        "Privacy settings are available, but this session did not report cleanup or history controls.",
        );
    }

    let state = if desktop.remove_old_trash_files == Some(true)
        || desktop.remove_old_temp_files == Some(true)
    {
        "auto cleanup"
    } else if desktop.remember_recent_files == Some(false)
        || desktop.remember_app_usage == Some(false)
    {
        "reduced history"
    } else {
        "standard"
    };

    privacy_summary_spec(
        "History and cleanup",
        state,
        true,
        format!(
            "Recent files {} · App usage {} · Trash cleanup {} · Temp cleanup {} · Cleanup age {}.",
            option_on_off_word(desktop.remember_recent_files),
            option_on_off_word(desktop.remember_app_usage),
            option_on_off_word(desktop.remove_old_trash_files),
            option_on_off_word(desktop.remove_old_temp_files),
            privacy_cleanup_age_word(desktop.old_files_age_days)
        ),
    )
}

fn privacy_block_word(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "blocked",
        Some(false) => "allowed",
        None => "unknown",
    }
}

fn privacy_usb_word(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "protected",
        Some(false) => "off",
        None => "unknown",
    }
}

fn privacy_cleanup_age_word(value: Option<u32>) -> String {
    value
        .map(|days| days_label(f64::from(days)))
        .unwrap_or_else(|| "unknown".to_string())
}

fn overview_account_label(
    auth: Option<&OpenAIAuthStatus>,
    system: Option<&SettingsSystemStatus>,
) -> &'static str {
    if auth.is_some_and(|auth| auth.authenticated)
        || system.is_some_and(|system| system.identity.account_authenticated)
    {
        "signed in"
    } else if auth.is_some_and(|auth| auth.configured)
        || system.is_some_and(|system| system.identity.provider_configured)
    {
        "sign in"
    } else if auth.is_some() || system.is_some() {
        "local only"
    } else {
        "waiting"
    }
}

fn overview_account_ready(
    auth: Option<&OpenAIAuthStatus>,
    system: Option<&SettingsSystemStatus>,
) -> bool {
    matches!(
        overview_account_label(auth, system),
        "signed in" | "local only"
    )
}

fn overview_account_detail(
    auth: Option<&OpenAIAuthStatus>,
    system: Option<&SettingsSystemStatus>,
) -> String {
    if let Some(auth) = auth {
        if auth.authenticated {
            return "Signed in; OpenAI session storage is OS-owned.".to_string();
        }
        if auth.configured {
            return format!(
                "Provider configured; sign in creates the OS-owned session at {}.",
                auth.session_storage
            );
        }
        return "local-only; no OpenAI provider is configured. Session storage stays OS-owned."
            .to_string();
    }

    match system {
        Some(system) if system.identity.account_authenticated => {
            "Signed in; OpenAI session storage is OS-owned.".to_string()
        }
        Some(system) if system.identity.provider_configured => format!(
            "Provider configured; sign in creates the OS-owned session at {}.",
            system.identity.session_path
        ),
        Some(_) => "local-only; no OpenAI provider is configured. Session storage stays OS-owned."
            .to_string(),
        None => "Waiting for account status.".to_string(),
    }
}

fn overview_network_label(network: Option<&NetworkStatus>) -> &'static str {
    match network {
        Some(network) if network.online => "online",
        Some(network) if network.manager_available => "offline",
        Some(_) => "unavailable",
        None => "waiting",
    }
}

fn overview_network_ready(network: Option<&NetworkStatus>) -> bool {
    matches!(overview_network_label(network), "online")
}

fn overview_network_detail(network: Option<&NetworkStatus>) -> String {
    let Some(network) = network else {
        return "Waiting for networking status.".to_string();
    };

    match &network.active {
        Some(active) => format!(
            "{} via {} on {}; connectivity {}.",
            active.name, active.kind, active.device, network.connectivity
        ),
        None if !network.manager_available => {
            "Networking is not reachable in this session. Connectivity and active connection details are not ready.".to_string()
        }
        None => format!(
            "No active connection is reported. Connectivity {}; service state {}.",
            readable_runtime_value(&network.connectivity),
            readable_runtime_value(&network.state)
        ),
    }
}

fn overview_native_desktop_label(
    system: Option<&SettingsSystemStatus>,
    control_center_available: bool,
) -> &'static str {
    match device_settings_readiness(system, control_center_available) {
        DeviceSettingsReadiness::Ready => "ready",
        DeviceSettingsReadiness::IntegratedDesktopUnavailable => "not ready",
        DeviceSettingsReadiness::WaitingForSession => "waiting",
        DeviceSettingsReadiness::Unavailable => "not available",
    }
}

fn overview_native_desktop_ready(
    system: Option<&SettingsSystemStatus>,
    control_center_available: bool,
) -> bool {
    matches!(
        overview_native_desktop_label(system, control_center_available),
        "ready"
    )
}

struct NetworkSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn network_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> NetworkSummarySpec {
    NetworkSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn internet_network_summary_spec(network: Option<&NetworkStatus>) -> NetworkSummarySpec {
    let Some(network) = network else {
        return network_summary_spec(
            "Internet",
            "waiting",
            false,
            "Waiting for networking status.",
        );
    };

    let state = if network.online {
        "online"
    } else if !network.manager_available {
        "unavailable"
    } else if network.connectivity.trim().is_empty() {
        "offline"
    } else {
        network.connectivity.as_str()
    };

    network_summary_spec(
        "Internet",
        state,
        network.online,
        network_connectivity_summary_detail(network),
    )
}

fn active_connection_summary_spec(network: Option<&NetworkStatus>) -> NetworkSummarySpec {
    let Some(network) = network else {
        return network_summary_spec(
            "Active connection",
            "waiting",
            false,
            "Waiting for active connection details.",
        );
    };

    match &network.active {
        Some(active) => network_summary_spec(
            "Active connection",
            "connected",
            true,
            format!("{} via {} on {}.", active.name, active.kind, active.device),
        ),
        None => network_summary_spec(
            "Active connection",
            "none",
            false,
            if network.manager_available {
                "No active connection is reported by networking."
            } else {
                "Networking is not reachable, so Settings cannot display an active connection."
            },
        ),
    }
}

fn network_manager_summary_spec(network: Option<&NetworkStatus>) -> NetworkSummarySpec {
    let Some(network) = network else {
        return network_summary_spec(
            "System service",
            "waiting",
            false,
            "Waiting for network service status.",
        );
    };

    network_summary_spec(
        "System service",
        if network.manager_available {
            "available"
        } else {
            "unavailable"
        },
        network.manager_available,
        network_manager_summary_detail(network),
    )
}

fn proxy_network_summary_spec(network: Option<&NetworkStatus>) -> NetworkSummarySpec {
    let Some(network) = network else {
        return network_summary_spec("Proxy", "waiting", false, "Waiting for proxy settings.");
    };

    let Some(proxy) = network.proxy.as_ref() else {
        return network_summary_spec("Proxy", "waiting", false, "Waiting for proxy settings.");
    };

    if proxy.gsettings_available && proxy.schema_available {
        let mode = if proxy.mode_available {
            normalized_proxy_mode(&proxy.mode)
        } else {
            "read-only"
        };
        network_summary_spec("Proxy", mode, true, proxy.detail.as_str())
    } else {
        network_summary_spec("Proxy", "unavailable", false, proxy.detail.as_str())
    }
}

fn network_connectivity_summary_detail(network: &NetworkStatus) -> String {
    let detail = polished_network_detail(&network.detail);
    if network.online {
        return if detail.is_empty() {
            "Internet connectivity is available.".to_string()
        } else {
            detail
        };
    }

    if !network.manager_available {
        return append_sentence_detail(
            "Networking is not reachable in this session.",
            detail.as_str(),
        );
    }

    if raw_error_like(&network.detail) {
        return append_sentence_detail(
            "Connectivity has not been confirmed by networking.",
            detail.as_str(),
        );
    }

    if !detail.is_empty() {
        detail
    } else {
        format!(
            "Connectivity {}; service state {}.",
            readable_runtime_value(&network.connectivity),
            readable_runtime_value(&network.state)
        )
    }
}

fn network_manager_summary_detail(network: &NetworkStatus) -> String {
    let state = readable_runtime_value(&network.state);
    if network.manager_available {
        format!("Networking is available. Service state {state}.")
    } else {
        append_sentence_detail(
            "Networking is not reachable.",
            polished_network_detail(&network.detail).as_str(),
        )
    }
}

fn polished_network_detail(detail: &str) -> String {
    let raw = detail.trim();
    let detail = raw.strip_prefix("Error:").unwrap_or(raw).trim();
    let detail = detail.trim_end_matches('.');
    if detail.is_empty() {
        return String::new();
    }
    if let Some(summary) = network_failure_summary(detail) {
        return summary;
    }
    if raw_error_like(raw) {
        format!("Technical detail: {detail}.")
    } else {
        format!("{detail}.")
    }
}

fn network_failure_summary(detail: &str) -> Option<String> {
    let detail = detail.trim();
    if !detail.contains("Could not connect") {
        return None;
    }

    if detail.contains("No such file or directory") {
        return Some(
            "Networking could not connect. Detail: service socket was not found.".to_string(),
        );
    }

    Some("Networking could not connect.".to_string())
}

fn append_sentence_detail(summary: &str, detail: &str) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        summary.to_string()
    } else {
        format!("{summary} {detail}")
    }
}

fn raw_error_like(detail: &str) -> bool {
    let detail = detail.trim_start();
    detail.starts_with("Error:") || detail.contains("Could not ")
}

fn readable_runtime_value(value: &str) -> &str {
    // Humanize the internal runtime sentinels/slugs so no raw token (e.g.
    // "not-configured", "os-managed-runtime") reaches user copy.
    match value.trim() {
        "" => "unknown",
        "not-configured" | "unconfigured" => "not configured",
        "os-managed-runtime" => "OS-managed runtime",
        other => other,
    }
}

fn overview_native_settings_detail(
    system: Option<&SettingsSystemStatus>,
    control_center_available: bool,
) -> String {
    match device_settings_readiness(system, control_center_available) {
        DeviceSettingsReadiness::Ready => {
            "Manage display, sound, keyboard, power, accounts, apps, printers, and other device controls. OpenAI, policy, models, storage, and recovery stay here.".to_string()
        }
        DeviceSettingsReadiness::IntegratedDesktopUnavailable => {
            let _system =
                system.expect("session-unavailable readiness always includes session details");
            "Device controls open after the desktop finishes loading.".to_string()
        }
        DeviceSettingsReadiness::WaitingForSession => "Checking device controls.".to_string(),
        DeviceSettingsReadiness::Unavailable => {
            "Device controls are not supported on this device.".to_string()
        }
    }
}

fn overview_native_settings_accessibility(
    system: Option<&SettingsSystemStatus>,
    available: bool,
) -> String {
    match device_settings_readiness(system, available) {
        DeviceSettingsReadiness::Ready => "Manage device controls.".to_string(),
        DeviceSettingsReadiness::IntegratedDesktopUnavailable => {
            "Device controls open after the desktop finishes loading.".to_string()
        }
        DeviceSettingsReadiness::WaitingForSession => "Checking device controls.".to_string(),
        DeviceSettingsReadiness::Unavailable => {
            "Device controls are not supported on this device.".to_string()
        }
    }
}

fn overview_privacy_policy_label(
    privacy: Option<&PrivacyStatus>,
    policy: Option<&PolicyStatus>,
) -> &'static str {
    if policy.is_some_and(|policy| policy.locked) {
        "locked"
    } else if privacy.is_some_and(|privacy| privacy.offline) {
        "private"
    } else if policy.is_some() {
        "active"
    } else if privacy.is_some() {
        "privacy ready"
    } else {
        "waiting"
    }
}

fn overview_privacy_policy_ready(
    privacy: Option<&PrivacyStatus>,
    policy: Option<&PolicyStatus>,
) -> bool {
    !matches!(overview_privacy_policy_label(privacy, policy), "waiting")
}

fn overview_privacy_policy_detail(
    privacy: Option<&PrivacyStatus>,
    policy: Option<&PolicyStatus>,
) -> String {
    let privacy_detail = privacy.map_or_else(
        || "Privacy status is waiting for Goblins OS.".to_string(),
        |privacy| {
            if privacy.offline {
                "Private mode is on.".to_string()
            } else {
                "Private mode is off.".to_string()
            }
        },
    );
    let policy_detail = policy
        .map(|policy| {
            if policy.locked {
                format!("Policy profile {} is locked.", policy.profile)
            } else {
                format!("Policy profile {} is active.", policy.profile)
            }
        })
        .unwrap_or_else(|| "Policy status is waiting for Goblins OS.".to_string());

    format!("{privacy_detail} {policy_detail}")
}

#[derive(Debug, PartialEq, Eq)]
struct ModelOverviewCounts {
    installed: usize,
    installable: usize,
    blocked: usize,
    waiting: usize,
    total: usize,
}

fn local_model_overview_counts(models: &[LocalModelOption]) -> ModelOverviewCounts {
    let mut counts = ModelOverviewCounts {
        installed: 0,
        installable: 0,
        blocked: 0,
        waiting: 0,
        total: models.len(),
    };

    for model in models {
        if model.install.state == "installed" {
            counts.installed += 1;
        } else {
            match model.state.as_str() {
                "installable" => counts.installable += 1,
                "blocked" => counts.blocked += 1,
                _ => counts.waiting += 1,
            }
        }
    }

    counts
}

fn overview_models_label(catalog: Option<&LocalModelCatalog>) -> &'static str {
    let Some(catalog) = catalog else {
        return "waiting";
    };
    let counts = local_model_overview_counts(&catalog.models);
    if counts.installed > 0 {
        "installed"
    } else if counts.installable > 0 {
        "ready to download"
    } else if counts.blocked > 0 {
        "blocked"
    } else {
        "waiting"
    }
}

fn overview_models_ready(catalog: Option<&LocalModelCatalog>) -> bool {
    matches!(
        overview_models_label(catalog),
        "installed" | "ready to download"
    )
}

fn overview_models_detail(catalog: Option<&LocalModelCatalog>) -> String {
    let Some(catalog) = catalog else {
        return "Waiting for local model compatibility.".to_string();
    };

    let counts = local_model_overview_counts(&catalog.models);
    let blocker = catalog
        .models
        .iter()
        .find(|model| model.state == "blocked" && !model.reasons.is_empty())
        .and_then(|model| model.reasons.first())
        .map(|reason| overview_first_blocker_detail(reason))
        .unwrap_or_default();

    format!(
        "{} installed, {} ready, {} blocked. Engine {}.{}",
        counts.installed,
        counts.installable,
        counts.blocked,
        runtime_label(&catalog.hardware.runtime),
        blocker
    )
}

struct ModelSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn model_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> ModelSummarySpec {
    ModelSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn active_engine_summary_spec(
    key: Option<&OpenAiKeyStatus>,
    codex: Option<&CodexStatus>,
    privacy: Option<&PrivacyStatus>,
    resident: Option<&ResidentStatus>,
) -> ModelSummarySpec {
    let Some(key) = key else {
        return model_summary_spec(
            "Active engine",
            "waiting",
            false,
            "Waiting for model engine status.",
        );
    };

    let private_mode = privacy.is_some_and(|privacy| privacy.offline);
    let resident_detail = resident
        .map(resident_process_summary_detail)
        .unwrap_or_else(|| "Goblins AI runtime status is waiting.".to_string());

    match key.engine.as_str() {
        "openai-api" => model_summary_spec(
            "Active engine",
            "hosted",
            key.configured && !private_mode,
            format!(
                "{} {} {}",
                engine_active_copy("openai-api"),
                hosted_engine_readiness_detail(key, private_mode),
                resident_detail
            ),
        ),
        "codex" => {
            let codex_ready = codex.is_some_and(|codex| codex.installed && codex.authenticated);
            model_summary_spec(
                "Active engine",
                "codex",
                codex_ready && !private_mode,
                format!(
                    "{} {} {}",
                    engine_active_copy("codex"),
                    codex_engine_readiness_detail(codex, private_mode),
                    resident_detail
                ),
            )
        }
        _ => model_summary_spec(
            "Active engine",
            "on-device",
            resident.is_some_and(model_resident_ready),
            format!(
                "{} {}",
                engine_active_copy("local-gpt-oss"),
                resident_detail
            ),
        ),
    }
}

fn local_model_summary_spec(catalog: Option<&LocalModelCatalog>) -> ModelSummarySpec {
    let Some(catalog) = catalog else {
        return model_summary_spec(
            "Local models",
            "waiting",
            false,
            "Waiting for local model compatibility.",
        );
    };

    model_summary_spec(
        "Local models",
        overview_models_label(Some(catalog)),
        overview_models_ready(Some(catalog)),
        overview_models_detail(Some(catalog)),
    )
}

fn openai_access_summary_spec(
    key: Option<&OpenAiKeyStatus>,
    codex: Option<&CodexStatus>,
) -> ModelSummarySpec {
    if codex.is_some_and(|codex| codex.installed && codex.authenticated) {
        return model_summary_spec(
            "OpenAI access",
            "codex signed in",
            true,
            "OpenAI account access is owned by Codex; Settings never receives credentials.",
        );
    }

    if let Some(key) = key {
        if key.configured {
            return model_summary_spec(
                "OpenAI access",
                "api key saved",
                true,
                format!(
                    "Hosted model {} is available. The key is held in owner-only system storage and is never returned to Settings.",
                    key.model
                ),
            );
        }
    }

    if let Some(codex) = codex {
        if codex.installed {
            return model_summary_spec("OpenAI access", "sign in", false, codex.detail.as_str());
        }
        return model_summary_spec(
            "OpenAI access",
            "local only",
            true,
            format!(
                "{} GPT-OSS remains available without OpenAI credentials.",
                codex.detail
            ),
        );
    }

    model_summary_spec(
        "OpenAI access",
        "waiting",
        false,
        "Waiting for Codex and API-key status.",
    )
}

fn voice_model_summary_spec(voice: Option<&VoiceStatus>) -> ModelSummarySpec {
    match voice {
        Some(voice) => model_summary_spec(
            "Voice",
            if voice.available {
                "ready"
            } else {
                "add models"
            },
            voice.available,
            voice_settings_detail(voice),
        ),
        None => model_summary_spec(
            "Voice",
            "waiting",
            false,
            "Waiting for local voice capability.",
        ),
    }
}

fn hosted_engine_readiness_detail(key: &OpenAiKeyStatus, private_mode: bool) -> String {
    if private_mode {
        return "Private mode is on, so hosted OpenAI calls are paused until you turn it off."
            .to_string();
    }
    if key.configured {
        format!(
            "Hosted model {} can run through the OS-owned key store.",
            key.model
        )
    } else {
        "Add an OS-owned OpenAI API key before hosted models can run.".to_string()
    }
}

fn codex_engine_readiness_detail(codex: Option<&CodexStatus>, private_mode: bool) -> String {
    if private_mode {
        return "Private mode is on, so Codex/OpenAI account access is paused.".to_string();
    }
    match codex {
        Some(codex) if codex.installed && codex.authenticated => {
            "Codex is signed in and ready.".to_string()
        }
        Some(codex) => codex.detail.clone(),
        None => "Waiting for Codex account status.".to_string(),
    }
}

fn resident_process_summary_detail(resident: &ResidentStatus) -> String {
    let last_check = resident
        .process
        .heartbeat_age_secs
        .map(|age| format!("Last check updated {age}s ago"))
        .unwrap_or_else(|| "Last check is waiting".to_string());
    format!(
        "Goblins AI runtime is {}. Model engine {}. {}.",
        resident.process.state,
        readable_runtime_value(&resident.engine.selected),
        last_check
    )
}

fn model_resident_ready(resident: &ResidentStatus) -> bool {
    matches!(
        resident.process.state.as_str(),
        "active" | "ready" | "running" | "healthy"
    )
}

fn overview_first_blocker_detail(reason: &str) -> String {
    let summary = reason
        .split(';')
        .next()
        .unwrap_or(reason)
        .trim()
        .trim_end_matches('.');
    if summary.is_empty() {
        String::new()
    } else {
        format!(" First blocker: {summary}.")
    }
}

fn overview_storage_detail(
    hardware: Option<&HardwareStatus>,
    catalog: Option<&LocalModelCatalog>,
    system: Option<&SettingsSystemStatus>,
) -> String {
    let volume_detail = match hardware {
        Some(hardware) if !hardware.storage.is_empty() => {
            most_constrained_storage_volume(&hardware.storage)
                .map(|volume| {
                    format!(
                        "{}: {}GB free ({}).",
                        volume.mount_point,
                        volume.available_gb.min(volume.total_gb),
                        storage_capacity_percent_text(volume.total_gb, volume.available_gb)
                    )
                })
                .unwrap_or_else(|| "Mounted capacity is available.".to_string())
        }
        Some(_) => "No mounted volumes reported.".to_string(),
        None => "Mounted capacity is waiting for Goblins OS.".to_string(),
    };

    let model_detail = match catalog {
        Some(catalog) => catalog
            .hardware
            .model_dir_available_gb
            .map(|gb| format!(" Models: {gb}GB free."))
            .unwrap_or_else(|| " Models: free space unknown.".to_string()),
        None => match system {
            Some(_system) => " Model cache capacity is waiting for Goblins OS.".to_string(),
            None => " Model cache capacity is waiting.".to_string(),
        },
    };

    format!("{volume_detail}{model_detail}")
}

fn overview_recovery_label(recovery: Option<&RecoveryStatus>) -> &'static str {
    recovery
        .map(|recovery| recovery_summary_label(recovery_counts(&recovery.checks)))
        .unwrap_or("waiting")
}

fn overview_recovery_ready(recovery: Option<&RecoveryStatus>) -> bool {
    matches!(overview_recovery_label(recovery), "ready")
}

fn overview_recovery_detail(recovery: Option<&RecoveryStatus>) -> String {
    let Some(recovery) = recovery else {
        return "Waiting for recovery readiness.".to_string();
    };

    let counts = recovery_counts(&recovery.checks);
    if counts.total == 0 {
        "Waiting for recovery checks.".to_string()
    } else if counts.waiting == 0 {
        format!(
            "{}/{} recovery checks are ready.",
            counts.ready, counts.total
        )
    } else {
        format!(
            "{} of {} recovery checks need attention; {} ready.",
            counts.waiting, counts.total, counts.ready
        )
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_desktop_privacy_settings(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Desktop privacy", &["gos-subsection-title"]));

    let Some(desktop) = state
        .privacy
        .as_ref()
        .and_then(|privacy| privacy.desktop.as_ref())
    else {
        panel.append(&system_row(
            "Desktop privacy",
            "Waiting for desktop privacy controls.",
        ));
        return;
    };

    panel.append(&health_row(
        "Privacy settings",
        if desktop.gsettings_available && desktop.schema_available {
            "available"
        } else {
            "unavailable"
        },
        desktop.gsettings_available && desktop.schema_available,
        &desktop.detail,
    ));

    if !desktop.schema_available {
        return;
    }

    let core_url = config_core_url(state);
    let mut rows = 0;
    append_desktop_privacy_bool_row(
        panel,
        &core_url,
        "remember-recent-files",
        "Remember recent files",
        desktop.remember_recent_files,
        recent_files_detail,
        &mut rows,
    );
    append_desktop_privacy_bool_row(
        panel,
        &core_url,
        "remember-app-usage",
        "Remember app usage",
        desktop.remember_app_usage,
        app_usage_detail,
        &mut rows,
    );
    append_desktop_privacy_bool_row(
        panel,
        &core_url,
        "remove-old-trash-files",
        "Remove aged Trash items",
        desktop.remove_old_trash_files,
        cleanup_trash_detail,
        &mut rows,
    );
    append_desktop_privacy_bool_row(
        panel,
        &core_url,
        "remove-old-temp-files",
        "Remove aged temporary files",
        desktop.remove_old_temp_files,
        cleanup_temp_detail,
        &mut rows,
    );

    if let Some(age) = desktop.old_files_age_days {
        let core_url = core_url.clone();
        panel.append(&slider_row(
            SliderSpec {
                title: "Cleanup age",
                detail:
                    "Trash and temporary files become eligible for cleanup after this many days.",
                value: f64::from(normalized_old_files_age(age)),
                min: 1.0,
                max: 365.0,
                step: 1.0,
            },
            days_label,
            normalized_old_files_age_slider,
            move |value| set_desktop_privacy_number(&core_url, "old-files-age-days", value),
        ));
        rows += 1;
    }

    panel.append(&label("Device access", &["gos-subsection-title"]));
    let mut access_rows = 0;
    append_desktop_privacy_bool_row(
        panel,
        &core_url,
        "disable-microphone",
        "Block microphone access",
        desktop.disable_microphone,
        microphone_access_detail,
        &mut access_rows,
    );
    append_desktop_privacy_bool_row(
        panel,
        &core_url,
        "disable-camera",
        "Block camera access",
        desktop.disable_camera,
        camera_access_detail,
        &mut access_rows,
    );
    append_desktop_privacy_bool_row(
        panel,
        &core_url,
        "disable-sound-output",
        "Block sound output",
        desktop.disable_sound_output,
        sound_output_access_detail,
        &mut access_rows,
    );
    append_desktop_privacy_bool_row(
        panel,
        &core_url,
        "usb-protection",
        "Protect new USB devices",
        desktop.usb_protection,
        usb_protection_detail,
        &mut access_rows,
    );

    if rows == 0 && access_rows == 0 {
        panel.append(&system_row(
            "Desktop privacy controls",
            "Privacy settings are present, but the expected privacy controls are not available in this session.",
        ));
    } else if access_rows == 0 {
        panel.append(&system_row(
            "Device access controls",
            "This privacy status does not expose microphone, camera, sound-output, or USB protection controls.",
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_desktop_privacy_bool_row(
    panel: &gtk4::Box,
    core_url: &str,
    target: &'static str,
    title: &'static str,
    value: Option<bool>,
    detail_for_state: fn(bool) -> &'static str,
    rows: &mut usize,
) {
    if let Some(value) = value {
        let core_url = core_url.to_string();
        panel.append(&switch_row_dynamic(
            title,
            value,
            true,
            move |enabled| detail_for_state(enabled).to_string(),
            move |enabled| set_desktop_privacy_bool(&core_url, target, enabled),
        ));
        *rows += 1;
    }
}

fn engine_selection_success_copy(engine: &str) -> &'static str {
    match engine {
        "openai-api" => {
            "Active engine: OpenAI hosted models. Answers now come from OpenAI's API using your key."
        }
        "codex" => {
            "Active engine: your OpenAI account via Codex. Goblins OS works through OpenAI's own coding agent."
        }
        _ => {
            "Active engine: GPT-OSS on-device. Goblins OS is private and local again."
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecoveryCounts {
    ready: usize,
    waiting: usize,
    total: usize,
}

fn recovery_check_ready(check: &RecoveryCheck) -> bool {
    matches!(
        check.state.as_str(),
        "ready" | "active" | "available" | "completed" | "ok" | "healthy"
    )
}

fn recovery_counts(checks: &[RecoveryCheck]) -> RecoveryCounts {
    let ready = checks
        .iter()
        .filter(|check| recovery_check_ready(check))
        .count();
    let total = checks.len();
    RecoveryCounts {
        ready,
        waiting: total.saturating_sub(ready),
        total,
    }
}

fn recovery_summary_label(counts: RecoveryCounts) -> &'static str {
    if counts.total == 0 {
        "waiting"
    } else if counts.waiting == 0 {
        "ready"
    } else {
        "needs attention"
    }
}

fn recovery_source_summary(status: &RecoveryStatus) -> String {
    generated_status_sentence(&status.generated_at)
}

fn generated_timestamp(generated_at: &str) -> Option<&str> {
    let generated = generated_at.trim();
    if generated.is_empty() || generated.starts_with("SystemTime") {
        None
    } else {
        Some(generated)
    }
}

fn generated_status_sentence(generated_at: &str) -> String {
    match generated_timestamp(generated_at) {
        Some(generated) => format!("Status updated {generated}."),
        None => "Status is available.".to_string(),
    }
}

fn recovery_summary_detail(status: &RecoveryStatus) -> String {
    let counts = recovery_counts(&status.checks);
    if counts.total == 0 {
        return format!(
            "Waiting for recovery checks. {}",
            recovery_source_summary(status)
        );
    }

    if counts.waiting == 0 {
        format!(
            "{}/{} recovery checks are ready. {}",
            counts.ready,
            counts.total,
            recovery_source_summary(status)
        )
    } else {
        format!(
            "{}/{} recovery checks are ready; {} still need attention. {}",
            counts.ready,
            counts.total,
            counts.waiting,
            recovery_source_summary(status)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecoverySummarySpec {
    title: &'static str,
    state: &'static str,
    ready: bool,
    detail: String,
}

fn recovery_overall_summary_spec(status: Option<&RecoveryStatus>) -> RecoverySummarySpec {
    let Some(status) = status else {
        return RecoverySummarySpec {
            title: "Recovery readiness",
            state: "waiting",
            ready: false,
            detail: "Waiting for recovery state.".to_string(),
        };
    };

    let counts = recovery_counts(&status.checks);
    RecoverySummarySpec {
        title: "Recovery readiness",
        state: recovery_summary_label(counts),
        ready: counts.total > 0 && counts.waiting == 0,
        detail: recovery_summary_detail(status),
    }
}

fn recovery_actions_detail(status: Option<&RecoveryStatus>) -> String {
    let Some(status) = status else {
        return "Disabled: waiting for recovery status.".to_string();
    };

    let counts = recovery_counts(&status.checks);
    if counts.total > 0 && counts.waiting == 0 {
        return "Read-only for now. Recovery checks are ready, but repair, rollback, and reset actions stay disabled until secure recovery actions are available.".to_string();
    }

    let blockers = status
        .checks
        .iter()
        .filter(|check| !recovery_check_ready(check))
        .take(4)
        .map(|check| check.label.as_str())
        .collect::<Vec<_>>();

    if blockers.is_empty() {
        "Disabled: no recovery checks were reported through Goblins OS.".to_string()
    } else {
        format!(
            "Disabled: {}. Recovery actions stay disabled until all checks are ready and secure recovery actions are available.",
            blockers.join(", ")
        )
    }
}

fn recovery_check_group(check_id: &str) -> &'static str {
    match check_id {
        "bootc-tooling" | "boot-image" | "os-packaging-verifier" => "Image recovery",
        "system-services" => "Service recovery",
        "model-cache" | "installer-state" | "session-gate" | "policy-state" | "resident-state"
        | "secret-storage" => "State storage",
        id if id.ends_with("-service") => "Service recovery",
        _ => "System facilities",
    }
}

fn recovery_group_counts(checks: &[RecoveryCheck], group: &'static str) -> RecoveryCounts {
    let mut ready: usize = 0;
    let mut total: usize = 0;
    for check in checks
        .iter()
        .filter(|check| recovery_check_group(&check.id) == group)
    {
        total += 1;
        if recovery_check_ready(check) {
            ready += 1;
        }
    }

    RecoveryCounts {
        ready,
        waiting: total.saturating_sub(ready),
        total,
    }
}

fn recovery_group_summary_detail(status: &RecoveryStatus, group: &'static str) -> String {
    let counts = recovery_group_counts(&status.checks, group);
    let group_label = group.to_ascii_lowercase();
    if counts.total == 0 {
        format!("Waiting for {group_label} checks from {}.", status.source)
    } else if counts.waiting == 0 {
        format!(
            "{}/{} {group_label} checks are ready.",
            counts.ready, counts.total
        )
    } else {
        format!(
            "{} of {} {group_label} checks need attention; {} ready.",
            counts.waiting, counts.total, counts.ready
        )
    }
}

fn recovery_group_summary_spec(
    status: Option<&RecoveryStatus>,
    title: &'static str,
    group: &'static str,
) -> RecoverySummarySpec {
    let Some(status) = status else {
        return RecoverySummarySpec {
            title,
            state: "waiting",
            ready: false,
            detail: format!("Waiting for {} checks.", group.to_ascii_lowercase()),
        };
    };

    let counts = recovery_group_counts(&status.checks, group);
    RecoverySummarySpec {
        title,
        state: recovery_summary_label(counts),
        ready: counts.total > 0 && counts.waiting == 0,
        detail: recovery_group_summary_detail(status, group),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_recovery(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    append_panel_header(
        panel,
        "Recovery",
        "System image, service, private-state, and protected-resource readiness for recovery flows.",
    );
    append_recovery_summary(panel, state);

    if let Some(status) = &state.system_services {
        panel.append(&label("Service health", &["gos-subsection-title"]));
        panel.append(&health_row(
            "Service manager",
            if status.manager_available {
                "available"
            } else {
                "waiting"
            },
            status.manager_available,
            "The supervisor that starts and watches over Goblins OS services is available.",
        ));
        for service in &status.services {
            let running = service.state == service.expected_state;
            // One calm, plain-language line per service, with the live state shown
            // as a colored pill.
            // `binary_present` is Option<bool>: None means the binary is platform-
            // managed (e.g. gdm, NetworkManager) and not something Goblins OS ships.
            let detail = if !service.unit_file_present {
                "Required service support is not included in this build.".to_string()
            } else if service.binary_present == Some(false) {
                "Required service support is incomplete on this system.".to_string()
            } else if running {
                "Present and running.".to_string()
            } else {
                format!("Present · expected to be {}.", service.expected_state)
            };
            panel.append(&health_row(
                &service.label,
                &service.state,
                running,
                &detail,
            ));
        }
    }

    match &state.recovery {
        Some(recovery) => {
            let mut last_group: Option<&'static str> = None;
            for check in &recovery.checks {
                let group = recovery_check_group(&check.id);
                if last_group != Some(group) {
                    panel.append(&label(group, &["gos-subsection-title"]));
                    last_group = Some(group);
                }
                panel.append(&health_row(
                    &check.label,
                    &check.state,
                    recovery_check_ready(check),
                    &check.detail,
                ));
            }
        }
        None => panel.append(&system_row(
            "Recovery checks",
            "Waiting for recovery state.",
        )),
    }

    panel.append(&label("Recovery actions", &["gos-subsection-title"]));
    panel.append(&system_row(
        "Repair, rollback, and reset",
        &recovery_actions_detail(state.recovery.as_ref()),
    ));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_recovery_summary(panel: &gtk4::Box, state: &SettingsState) {
    use gtk4::prelude::*;

    panel.append(&label("Recovery summary", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-recovery-summary-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    for (index, item) in [
        recovery_overall_summary_spec(state.recovery.as_ref()),
        recovery_group_summary_spec(state.recovery.as_ref(), "Image recovery", "Image recovery"),
        recovery_group_summary_spec(
            state.recovery.as_ref(),
            "Service recovery",
            "Service recovery",
        ),
        recovery_group_summary_spec(state.recovery.as_ref(), "State storage", "State storage"),
    ]
    .into_iter()
    .enumerate()
    {
        let row = health_row(item.title, item.state, item.ready, &item.detail);
        row.add_css_class("gos-recovery-summary-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

/// A health row: a title, a status pill carrying the live state word, and one
/// concise plain-language line. Normal states are visually quiet so Settings
/// reads like a control surface, not a wall of debug badges.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn health_row(title: &str, state: &str, ok: bool, detail: &str) -> gtk4::Box {
    use gtk4::prelude::*;

    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    row.add_css_class("gos-row");
    row.add_css_class("gos-health-row");
    let display_state = settings_status_display_label(state);
    let row_label = format!("{title}: {display_state}");
    let display_detail = settings_detail_display_copy(detail);
    set_accessible_label_description(&row, &row_label, &display_detail);

    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let title_label = label(title, &["gos-row-title"]);
    title_label.set_hexpand(true);
    head.append(&title_label);
    if ok && settings_status_state_is_quiet(state) {
        head.append(&settings_status_value_label(state));
    } else {
        head.append(&settings_status_pill(state, ok));
    }
    row.append(&head);

    let copy = label(&display_detail, &["gos-row-copy"]);
    copy.set_wrap(true);
    copy.set_xalign(0.0);
    row.append(&copy);
    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_status_pill(state: &str, ok: bool) -> gtk4::Label {
    let display_state = settings_status_display_label(state);
    let pill = status_pill(&display_state, ok);
    if ok && settings_status_state_is_quiet(state) {
        pill.add_css_class("gos-status-quiet");
    }
    pill
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_status_value_label(state: &str) -> gtk4::Label {
    use gtk4::prelude::*;

    let display_state = settings_status_display_label(state);
    let value = label(&display_state, &["gos-row-value", "gos-status-value"]);
    value.set_wrap(false);
    value.set_valign(gtk4::Align::Center);
    set_accessible_label_description(&value, &display_state, &display_state);
    value
}

fn settings_status_display_label(state: &str) -> String {
    let trimmed = state.trim();
    let normalized = trimmed.to_ascii_lowercase();
    match normalized.as_str() {
        "" => "Waiting".to_string(),
        "unavailable" => "Not ready".to_string(),
        "inactive-or-unavailable" => "Inactive".to_string(),
        "not available" => "Not supported".to_string(),
        "not-requested" => "Ready to download".to_string(),
        "missing" => "Not installed".to_string(),
        "failed" => "Needs attention".to_string(),
        "blocked" => "Blocked".to_string(),
        "unknown" => "Unknown".to_string(),
        "waiting" => "Waiting".to_string(),
        "waiting-for-engine" => "Waiting for engine".to_string(),
        "waiting-for-manifest" => "Waiting for provider manifest".to_string(),
        "partial" => "Partially ready".to_string(),
        "ready" => "Ready".to_string(),
        "adapter-ready" => "Ready".to_string(),
        "available" => "Available".to_string(),
        "adapter-waiting" => "No adapter".to_string(),
        "device-waiting" => "No device".to_string(),
        "service-waiting" => "Starting".to_string(),
        "provider-ready" => "Provider ready".to_string(),
        "signed-in" => "Signed in".to_string(),
        "local-only" => "Local only".to_string(),
        "permission-gated" => "Needs permission".to_string(),
        "confirmation-required" => "Needs confirmation".to_string(),
        "online" => "Online".to_string(),
        "offline" => "Offline".to_string(),
        _ if trimmed.ends_with('%') => trimmed.to_string(),
        _ => trimmed
            .split(['-', '_', ' '])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => {
                        let mut word = first.to_uppercase().collect::<String>();
                        word.push_str(&chars.as_str().to_ascii_lowercase());
                        word
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn settings_detail_display_copy(detail: &str) -> String {
    let mut text = detail.trim().to_string();
    let old_runtime_name = ["Codex", "resident"].join(" ");
    let old_runtime_name_lower = ["codex", "resident"].join(" ");
    for (from, to) in [
        ("GNOME Control Center is", "Device controls are"),
        ("gnome-control-center is", "device controls are"),
        ("GNOME Control Center", "Device controls"),
        ("gnome-control-center", "device controls"),
        ("NetworkManager", "Network service"),
        ("WirePlumber", "Audio routing"),
        ("PipeWire", "audio service"),
        ("BlueZ", "Bluetooth service"),
        ("bluetoothctl", "Bluetooth controls"),
        ("GSettings", "desktop preferences"),
        ("gsettings", "desktop preferences"),
        ("gdbus", "desktop bridge"),
        ("xrandr", "display fallback"),
        ("systemd", "system services"),
        ("Relay routes", "Assistant routes"),
        ("relay routes", "assistant routes"),
        ("Cloud relay", "Cloud assistant route"),
        ("cloud relay", "cloud assistant route"),
        ("Local relay", "Local assistant route"),
        ("local relay", "local assistant route"),
        ("Not configured", "Not set up"),
        ("not configured", "not set up"),
        ("Unconfigured", "Not set up"),
        ("unconfigured", "not set up"),
        ("Are not available yet", "Are not ready yet"),
        ("are not available yet", "are not ready yet"),
        ("Is not available yet", "Is not ready yet"),
        ("is not available yet", "is not ready yet"),
        ("Not available yet", "Not ready yet"),
        ("not available yet", "not ready yet"),
        ("Stays unavailable", "Stays disabled"),
        ("stays unavailable", "stays disabled"),
        ("Stay unavailable", "Stay disabled"),
        ("stay unavailable", "stay disabled"),
        ("Are not available", "Are not ready"),
        ("are not available", "are not ready"),
        ("Is not available", "Is not ready"),
        ("is not available", "is not ready"),
        ("Not available", "Not ready"),
        ("not available", "not ready"),
        ("Are unavailable", "Are not ready"),
        ("are unavailable", "are not ready"),
        ("Is unavailable", "Is not ready"),
        ("is unavailable", "is not ready"),
        ("Unavailable", "Not ready"),
        ("unavailable", "not ready"),
    ] {
        text = text.replace(from, to);
    }
    for from in [
        ["Resident", "process"].join(" "),
        ["resident", "process"].join(" "),
    ] {
        text = text.replace(&from, "Goblins AI runtime");
    }
    text = text.replace(&old_runtime_name, "Goblins AI runtime");
    text = text.replace(&old_runtime_name_lower, "Goblins AI runtime");
    text
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_status_state_is_quiet(state: &str) -> bool {
    let normalized = state.trim().to_ascii_lowercase();
    // A trailing-% reading (text size, scale, volume) is a descriptive value, not a
    // health state, so it rests in the calm neutral pill instead of reading as "good".
    if normalized.ends_with('%') {
        return true;
    }
    // One status -> pill-variant map for the whole app: descriptive/neutral facts
    // (a chosen mode, a font, a layout, a configured-but-idle state) read as a calm
    // gray pill; only genuinely affirmative health (on, connected, granted, …) stays
    // green; problems (waiting/unknown/unavailable/blocked) stay explicit. Matching is
    // case-insensitive so "Inter" and "inter" never disagree.
    matches!(
        normalized.as_str(),
        "active"
            | "auto"
            | "available"
            | "configured"
            | "full motion"
            | "goblins theme"
            | "inter"
            | "installed"
            | "left-aligned"
            | "local only"
            | "local-only"
            | "menu bar"
            | "night light off"
            | "none"
            | "off"
            | "not installed"
            | "not-requested"
            | "online"
            | "personal"
            | "privacy ready"
            | "ready"
            | "adapter-ready"
            | "ready to download"
            | "server-side"
            | "signed in"
            | "signed-in"
            | "provider-ready"
            | "unlocked"
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_panel_header(panel: &gtk4::Box, title: &str, detail: &str) {
    use gtk4::prelude::*;

    panel.append(&label(title, &["gos-section-title"]));
    panel.append(&label(detail, &["gos-panel-intro"]));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_facility_status(
    panel: &gtk4::Box,
    state: &SettingsState,
    id: &str,
    title: &str,
    fallback: &str,
) {
    use gtk4::prelude::*;

    match facility_by_id(state, id) {
        Some(facility) => {
            let detail = facility_user_detail(facility);
            panel.append(&health_row(
                title,
                facility_state_label(&facility.state),
                facility_state_is_ready(&facility.state),
                &detail,
            ));
        }
        None => panel.append(&system_row(title, fallback)),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn facility_by_id<'a>(state: &'a SettingsState, id: &str) -> Option<&'a SystemFacility> {
    state
        .privacy
        .as_ref()
        .and_then(|privacy| privacy.facilities.iter().find(|facility| facility.id == id))
        .or_else(|| {
            state.hardware.as_ref().and_then(|hardware| {
                hardware
                    .facilities
                    .iter()
                    .find(|facility| facility.id == id)
            })
        })
}

fn facility_user_detail(facility: &SystemFacility) -> String {
    // User-facing facility cards show the plain-language status only. The raw
    // probe evidence (e.g. "/dev/input:missing", "bootc:present") is internal
    // verification data, not user copy, so it is never surfaced here.
    facility.detail.clone()
}

fn trim_terminal_period(value: &str) -> &str {
    value.trim_end().strip_suffix('.').unwrap_or(value)
}

fn setting_change_rejected_detail(error: &str) -> String {
    let error = trim_terminal_period(error);
    if error.trim().is_empty() {
        "Could not apply the setting. The previous value was restored.".to_string()
    } else {
        let display_error = settings_detail_display_copy(error);
        format!(
            "Could not apply the setting: {}. The previous value was restored.",
            trim_terminal_period(&display_error)
        )
    }
}

fn facility_state_label(state: &str) -> &'static str {
    if facility_state_is_ready(state) {
        return "Available";
    }

    match state {
        "waiting" | "inactive" | "pending" => "Waiting",
        "unavailable" => "Not ready",
        "missing" => "Not installed",
        "failed" => "Needs attention",
        _ => "Unknown",
    }
}

fn facility_state_is_ready(state: &str) -> bool {
    matches!(
        state,
        "ready" | "active" | "available" | "completed" | "ok" | "healthy"
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_motion_preference(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Motion", &["gos-subsection-title"]));
    let Some(accessibility) = &state.accessibility else {
        panel.append(&system_row(
            "Motion",
            "Waiting for accessibility preferences.",
        ));
        return;
    };
    let interface = &accessibility.interface;
    if !interface.schema_available {
        panel.append(&system_row("Motion", &interface.detail));
        return;
    }
    let Some(reduce_motion) = interface.reduce_motion else {
        panel.append(&system_row(
            "Reduce motion",
            "The animation preference is not available in this session.",
        ));
        return;
    };

    let core_url = config_core_url(state);
    panel.append(&switch_row_dynamic(
        "Reduce motion",
        reduce_motion,
        true,
        |reduce_motion| motion_preference_detail(reduce_motion).to_string(),
        move |reduce_motion| set_accessibility_bool(&core_url, "reduce-motion", reduce_motion),
    ));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_text_scale_preference(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Text size", &["gos-subsection-title"]));
    let Some(accessibility) = &state.accessibility else {
        panel.append(&system_row(
            "Text size",
            "Waiting for accessibility preferences.",
        ));
        return;
    };
    let interface = &accessibility.interface;
    if !interface.schema_available {
        panel.append(&system_row("Text size", &interface.detail));
        return;
    }
    let Some(scale) = interface.text_scale else {
        panel.append(&system_row(
            "Text size",
            "The text scaling preference is not available in this session.",
        ));
        return;
    };

    let core_url = config_core_url(state);
    panel.append(&slider_row(
        SliderSpec {
            title: "Text size",
            detail:
                "Adjust the standard desktop text scale used by Goblins OS and system utilities.",
            value: normalized_text_scale(scale),
            min: 0.85,
            max: 1.35,
            step: 0.05,
        },
        text_scale_percent,
        normalized_text_scale,
        move |next_scale| set_accessibility_number(&core_url, "text-scale", next_scale),
    ));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_assistive_technology_settings(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Assistive access", &["gos-subsection-title"]));
    let Some(accessibility) = &state.accessibility else {
        panel.append(&system_row(
            "Assistive technologies",
            "Waiting for accessibility preferences.",
        ));
        return;
    };

    panel.append(&health_row(
        "Accessibility settings",
        if accessibility.gsettings_available {
            "available"
        } else {
            "unavailable"
        },
        accessibility.gsettings_available,
        &accessibility.detail,
    ));

    let assistive = &accessibility.assistive;
    if !assistive.schema_available {
        panel.append(&system_row("Assistive technologies", &assistive.detail));
        return;
    }

    append_accessibility_bool_row(
        panel,
        state,
        "screen-reader",
        "Screen reader",
        assistive.screen_reader,
        screen_reader_detail,
    );
    append_accessibility_bool_row(
        panel,
        state,
        "screen-keyboard",
        "On-screen keyboard",
        assistive.screen_keyboard,
        screen_keyboard_detail,
    );
    append_accessibility_bool_row(
        panel,
        state,
        "magnifier",
        "Magnifier",
        assistive.magnifier,
        magnifier_detail,
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_night_light_settings(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Display comfort", &["gos-subsection-title"]));
    let Some(accessibility) = &state.accessibility else {
        panel.append(&system_row(
            "Night Light",
            "Waiting for display-comfort preferences.",
        ));
        return;
    };
    let display = &accessibility.display_comfort;
    if !display.schema_available {
        panel.append(&system_row("Night Light", &display.detail));
        return;
    }

    append_accessibility_bool_row(
        panel,
        state,
        "night-light",
        "Night Light",
        display.night_light_enabled,
        night_light_detail,
    );
    append_accessibility_bool_row(
        panel,
        state,
        "night-light-automatic-schedule",
        "Automatic schedule",
        display.schedule_automatic,
        night_light_schedule_detail,
    );

    if let Some(temperature) = display.temperature {
        let core_url = config_core_url(state);
        let temperature = normalized_night_light_temperature(temperature);
        panel.append(&slider_row(
            SliderSpec {
                title: "Color temperature",
                detail: "Lower values make the display warmer while Night Light is active.",
                value: f64::from(temperature),
                min: 1000.0,
                max: 10000.0,
                step: 100.0,
            },
            night_light_temperature_label,
            |value| f64::from(normalized_night_light_temperature(value.round() as u32)),
            move |value| set_accessibility_number(&core_url, "night-light-temperature", value),
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_accessibility_bool_row(
    panel: &gtk4::Box,
    state: &SettingsState,
    target: &'static str,
    title: &'static str,
    value: Option<bool>,
    detail_for_state: fn(bool) -> &'static str,
) {
    if let Some(value) = value {
        let core_url = config_core_url(state);
        panel.append(&switch_row_dynamic(
            title,
            value,
            true,
            move |enabled| detail_for_state(enabled).to_string(),
            move |enabled| set_accessibility_bool(&core_url, target, enabled),
        ));
    } else {
        panel.append(&system_row(
            title,
            "This preference is not available in the current desktop session.",
        ));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_keyboard_preferences(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("Typing", &["gos-subsection-title"]));
    let Some(input) = &state.input else {
        panel.append(&system_row(
            "Typing preferences",
            "Waiting for input preferences.",
        ));
        return;
    };

    panel.append(&input_settings_health_row(input));
    let keyboard = &input.keyboard;
    if !keyboard.schema_available {
        panel.append(&system_row("Typing preferences", &keyboard.detail));
        return;
    }

    let core_url = config_core_url(state);
    append_input_bool_preference(
        panel,
        &core_url,
        "keyboard-repeat",
        "Key repeat",
        keyboard.repeat,
        key_repeat_detail,
        "The keyboard repeat preference is not reported by this session.",
    );

    if let Some(delay) = keyboard.delay_ms {
        let core_url = core_url.clone();
        panel.append(&slider_row(
            SliderSpec {
                title: "Repeat delay",
                detail: "How long a key is held before repeating starts.",
                value: f64::from(normalized_keyboard_delay(delay)),
                min: 150.0,
                max: 1000.0,
                step: 25.0,
            },
            milliseconds_label,
            normalized_keyboard_delay_slider,
            move |value| set_input_number(&core_url, "keyboard-delay-ms", value),
        ));
    } else {
        panel.append(&system_row(
            "Repeat delay",
            "The keyboard repeat delay preference is not reported by this session.",
        ));
    }

    if let Some(interval) = keyboard.repeat_interval_ms {
        let core_url = core_url.clone();
        panel.append(&slider_row(
            SliderSpec {
                title: "Repeat interval",
                detail: "Lower values repeat keys more quickly across the desktop.",
                value: f64::from(normalized_keyboard_repeat_interval(interval)),
                min: 15.0,
                max: 120.0,
                step: 5.0,
            },
            milliseconds_label,
            normalized_keyboard_repeat_interval_slider,
            move |value| set_input_number(&core_url, "keyboard-repeat-interval-ms", value),
        ));
    } else {
        panel.append(&system_row(
            "Repeat interval",
            "The keyboard repeat interval preference is not reported by this session.",
        ));
    }

    append_input_bool_preference(
        panel,
        &core_url,
        "keyboard-remember-numlock-state",
        "Remember Num Lock",
        keyboard.remember_numlock_state,
        remember_numlock_detail,
        "The Num Lock restore preference is not reported by this session.",
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_pointer_preferences(panel: &gtk4::Box, state: &SettingsState) {
    let Some(input) = &state.input else {
        panel.append(&label("Mouse", &["gos-subsection-title"]));
        panel.append(&system_row(
            "Mouse preferences",
            "Waiting for input preferences.",
        ));
        panel.append(&label("Trackpad", &["gos-subsection-title"]));
        panel.append(&system_row(
            "Trackpad preferences",
            "Waiting for input preferences.",
        ));
        return;
    };

    panel.append(&input_settings_health_row(input));
    append_mouse_preferences(panel, state, input);
    append_touchpad_preferences(panel, state, input);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_mouse_preferences(panel: &gtk4::Box, state: &SettingsState, input: &InputStatus) {
    panel.append(&label("Mouse", &["gos-subsection-title"]));
    let mouse = &input.mouse;
    if !mouse.schema_available {
        panel.append(&system_row("Mouse preferences", &mouse.detail));
        return;
    }

    let core_url = config_core_url(state);
    append_input_speed_preference(
        panel,
        &core_url,
        "mouse-speed",
        "Tracking speed",
        mouse.speed,
        "Adjust pointer speed through the system pointer setting.",
        "The mouse tracking speed preference is not reported by this session.",
    );
    append_input_bool_preference(
        panel,
        &core_url,
        "mouse-natural-scroll",
        "Natural scrolling",
        mouse.natural_scroll,
        mouse_natural_scroll_detail,
        "The mouse natural scrolling preference is not reported by this session.",
    );
    append_input_bool_preference(
        panel,
        &core_url,
        "mouse-left-handed",
        "Primary button on right",
        mouse.left_handed,
        primary_button_detail,
        "The mouse primary button preference is not reported by this session.",
    );
    append_input_bool_preference(
        panel,
        &core_url,
        "mouse-middle-click-emulation",
        "Middle-click emulation",
        mouse.middle_click_emulation,
        middle_click_detail,
        "The middle-click emulation preference is not reported by this session.",
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_touchpad_preferences(panel: &gtk4::Box, state: &SettingsState, input: &InputStatus) {
    panel.append(&label("Trackpad", &["gos-subsection-title"]));
    let touchpad = &input.touchpad;
    if !touchpad.schema_available {
        panel.append(&system_row("Trackpad preferences", &touchpad.detail));
        return;
    }

    let core_url = config_core_url(state);
    append_input_speed_preference(
        panel,
        &core_url,
        "touchpad-speed",
        "Tracking speed",
        touchpad.speed,
        "Adjust trackpad pointer speed through the system touchpad setting.",
        "The trackpad tracking speed preference is not reported by this session.",
    );
    append_input_bool_preference(
        panel,
        &core_url,
        "touchpad-tap-to-click",
        "Tap to click",
        touchpad.tap_to_click,
        tap_to_click_detail,
        "The tap-to-click preference is not reported by this session.",
    );
    append_input_bool_preference(
        panel,
        &core_url,
        "touchpad-natural-scroll",
        "Natural scrolling",
        touchpad.natural_scroll,
        touchpad_natural_scroll_detail,
        "The trackpad natural scrolling preference is not reported by this session.",
    );
    append_input_bool_preference(
        panel,
        &core_url,
        "touchpad-two-finger-scrolling",
        "Two-finger scrolling",
        touchpad.two_finger_scrolling_enabled,
        two_finger_scroll_detail,
        "The two-finger scrolling preference is not reported by this session.",
    );
    append_input_bool_preference(
        panel,
        &core_url,
        "touchpad-disable-while-typing",
        "Ignore trackpad while typing",
        touchpad.disable_while_typing,
        disable_trackpad_while_typing_detail,
        "The ignore-while-typing preference is not reported by this session.",
    );
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn input_settings_health_row(input: &InputStatus) -> gtk4::Box {
    health_row(
        "Input settings",
        if input.gsettings_available {
            "available"
        } else {
            "unavailable"
        },
        input.gsettings_available,
        &input.detail,
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_input_bool_preference(
    panel: &gtk4::Box,
    core_url: &str,
    target: &'static str,
    title: &'static str,
    value: Option<bool>,
    detail_for_state: fn(bool) -> &'static str,
    unavailable_detail: &'static str,
) {
    if let Some(value) = value {
        let core_url = core_url.to_string();
        panel.append(&switch_row_dynamic(
            title,
            value,
            true,
            move |enabled| detail_for_state(enabled).to_string(),
            move |enabled| set_input_bool(&core_url, target, enabled),
        ));
    } else {
        panel.append(&system_row(title, unavailable_detail));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_input_speed_preference(
    panel: &gtk4::Box,
    core_url: &str,
    target: &'static str,
    title: &'static str,
    value: Option<f64>,
    detail: &'static str,
    unavailable_detail: &'static str,
) {
    if let Some(speed) = value {
        let core_url = core_url.to_string();
        panel.append(&slider_row(
            SliderSpec {
                title,
                detail,
                value: normalized_unit_speed(speed),
                min: -1.0,
                max: 1.0,
                step: 0.05,
            },
            pointer_speed_label,
            normalized_unit_speed,
            move |value| set_input_number(&core_url, target, value),
        ));
    } else {
        panel.append(&system_row(title, unavailable_detail));
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_sound_preferences(panel: &gtk4::Box, state: &SettingsState) {
    panel.append(&label("System sounds", &["gos-subsection-title"]));
    let Some(sound) = state.audio.as_ref().and_then(|audio| audio.sound.as_ref()) else {
        panel.append(&system_row(
            "System sounds",
            "Waiting for desktop sound preferences.",
        ));
        return;
    };

    panel.append(&health_row(
        "Sound settings",
        if sound.gsettings_available && sound.schema_available {
            "available"
        } else {
            "unavailable"
        },
        sound.gsettings_available && sound.schema_available,
        &sound.detail,
    ));

    if !sound.schema_available {
        return;
    }

    append_sound_bool_row(
        panel,
        state,
        "event-sounds",
        "Interface sounds",
        sound.event_sounds,
        interface_sounds_detail,
    );

    append_sound_bool_row(
        panel,
        state,
        "input-feedback-sounds",
        "Input feedback sounds",
        sound.input_feedback_sounds,
        input_feedback_sounds_detail,
    );

    append_sound_bool_row(
        panel,
        state,
        "volume-boost",
        "Allow volume above 100%",
        sound.volume_boost,
        volume_boost_detail,
    );

    panel.append(&system_row(
        "Sound theme",
        &sound_theme_detail(sound.theme_name.as_deref()),
    ));
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_sound_bool_row(
    panel: &gtk4::Box,
    state: &SettingsState,
    target: &'static str,
    title: &'static str,
    value: Option<bool>,
    detail_for_state: fn(bool) -> &'static str,
) {
    if let Some(value) = value {
        let core_url = config_core_url(state);
        panel.append(&switch_row_dynamic(
            title,
            value,
            true,
            move |enabled| detail_for_state(enabled).to_string(),
            move |enabled| set_sound_preference_bool(&core_url, target, enabled),
        ));
    } else {
        panel.append(&system_row(
            title,
            "This sound preference is not available in the current desktop session.",
        ));
    }
}

fn motion_preference_detail(reduce_motion: bool) -> &'static str {
    if reduce_motion {
        "Desktop animations are reduced. State changes stay direct and calm."
    } else {
        "Desktop animations are enabled for standard Goblins OS transitions."
    }
}

fn normalized_background_picture_option(option: &str) -> &'static str {
    match option {
        "none" => "none",
        "wallpaper" => "wallpaper",
        "centered" => "centered",
        "scaled" => "scaled",
        "stretched" => "stretched",
        "zoom" => "zoom",
        "spanned" => "spanned",
        _ => "zoom",
    }
}

fn background_picture_option_detail(option: &str) -> &'static str {
    match normalized_background_picture_option(option) {
        "none" => "No image is drawn; the desktop uses the configured color.",
        "wallpaper" => "Tiles the image across the desktop at its original size.",
        "centered" => "Centers the image at its original size without stretching it.",
        "scaled" => "Fits the whole image inside the desktop without cropping.",
        "stretched" => "Stretches the image to fill the desktop; proportions may change.",
        "spanned" => "Spans the image across multiple monitors as one desktop.",
        _ => "Fills the desktop while preserving proportions; image edges may crop.",
    }
}

fn normalized_background_shading(shading: &str) -> &'static str {
    match shading {
        "solid" => "solid",
        "horizontal" => "horizontal",
        "vertical" => "vertical",
        _ => "solid",
    }
}

fn background_shading_detail(shading: &str) -> &'static str {
    match normalized_background_shading(shading) {
        "horizontal" => "Blends the configured desktop colors from left to right.",
        "vertical" => "Blends the configured desktop colors from top to bottom.",
        _ => "Uses the primary desktop color behind the wallpaper.",
    }
}

fn wallpaper_color_detail(color: Option<&str>, unavailable: &str) -> String {
    match color {
        Some(color) if color.trim().is_empty() => "No desktop color is set.".to_string(),
        Some(color) => color.to_string(),
        None => unavailable.to_string(),
    }
}

fn wallpaper_uri_detail(uri: Option<&str>, unavailable: &str) -> String {
    match uri {
        Some(uri) if uri.trim().is_empty() => "No wallpaper image is set.".to_string(),
        Some(_) => "Wallpaper image is set on this device.".to_string(),
        None => unavailable.to_string(),
    }
}

struct WallpaperSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn wallpaper_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> WallpaperSummarySpec {
    WallpaperSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn wallpaper_status_summary_spec(wallpaper: Option<&WallpaperStatus>) -> WallpaperSummarySpec {
    let Some(wallpaper) = wallpaper else {
        return wallpaper_summary_spec(
            "Wallpaper",
            "waiting",
            false,
            "Waiting for wallpaper status.",
        );
    };

    if wallpaper.gsettings_available && wallpaper.schema_available {
        wallpaper_summary_spec("Wallpaper", "available", true, wallpaper.detail.as_str())
    } else {
        wallpaper_summary_spec("Wallpaper", "unavailable", false, wallpaper.detail.as_str())
    }
}

fn wallpaper_light_image_summary_spec(wallpaper: Option<&WallpaperStatus>) -> WallpaperSummarySpec {
    wallpaper_image_summary_spec(
        "Light image",
        wallpaper,
        |wallpaper| wallpaper.picture_uri.as_deref(),
        "Light wallpaper preference is not available in this session.",
    )
}

fn wallpaper_dark_image_summary_spec(wallpaper: Option<&WallpaperStatus>) -> WallpaperSummarySpec {
    wallpaper_image_summary_spec(
        "Dark image",
        wallpaper,
        |wallpaper| wallpaper.picture_uri_dark.as_deref(),
        "Dark wallpaper preference is not available in this session.",
    )
}

fn wallpaper_image_summary_spec(
    title: &'static str,
    wallpaper: Option<&WallpaperStatus>,
    uri_for_wallpaper: fn(&WallpaperStatus) -> Option<&str>,
    unavailable: &str,
) -> WallpaperSummarySpec {
    let Some(wallpaper) = wallpaper else {
        return wallpaper_summary_spec(title, "waiting", false, "Waiting for wallpaper status.");
    };

    if !(wallpaper.gsettings_available && wallpaper.schema_available) {
        return wallpaper_summary_spec(title, "unavailable", false, wallpaper.detail.as_str());
    }

    let uri = uri_for_wallpaper(wallpaper);
    wallpaper_summary_spec(
        title,
        wallpaper_uri_summary_state(uri),
        uri.is_some(),
        wallpaper_uri_detail(uri, unavailable),
    )
}

fn wallpaper_uri_summary_state(uri: Option<&str>) -> &'static str {
    match uri {
        Some(uri) if uri.trim().is_empty() => "none",
        Some(_) => "set",
        None => "unknown",
    }
}

fn wallpaper_placement_summary_spec(wallpaper: Option<&WallpaperStatus>) -> WallpaperSummarySpec {
    let Some(wallpaper) = wallpaper else {
        return wallpaper_summary_spec(
            "Placement",
            "waiting",
            false,
            "Waiting for wallpaper placement status.",
        );
    };

    if !(wallpaper.gsettings_available && wallpaper.schema_available) {
        return wallpaper_summary_spec(
            "Placement",
            "unavailable",
            false,
            wallpaper.detail.as_str(),
        );
    }
    if !wallpaper.picture_options_available {
        return wallpaper_summary_spec(
            "Placement",
            "unavailable",
            false,
            "Wallpaper placement is not available in this session.",
        );
    }

    let current = normalized_background_picture_option(&wallpaper.picture_options);
    wallpaper_summary_spec(
        "Placement",
        current,
        true,
        background_picture_option_detail(current),
    )
}

fn normalized_proxy_mode(mode: &str) -> &'static str {
    match mode {
        "none" => "none",
        "auto" => "auto",
        "manual" => "manual",
        _ => "none",
    }
}

fn proxy_mode_detail(mode: &str) -> &'static str {
    match normalized_proxy_mode(mode) {
        "auto" => {
            "Uses the automatic configuration URL below when the desktop supports proxy lookup."
        }
        "manual" => "Uses the manual HTTP, HTTPS, FTP, and SOCKS proxy endpoints below.",
        _ => "Direct network connections are used; no desktop proxy is configured.",
    }
}

fn proxy_auto_config_detail(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        "No automatic proxy configuration URL is set.".to_string()
    } else {
        format!("{url} · used when proxy mode is Automatic.")
    }
}

fn proxy_ignore_hosts_detail(hosts: &[String]) -> String {
    if hosts.is_empty() {
        return "No bypass hosts are configured.".to_string();
    }

    let shown = hosts.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
    if hosts.len() > 4 {
        format!("{shown}, and {} more", hosts.len() - 4)
    } else {
        shown
    }
}

fn proxy_endpoint_detail(host: Option<String>, port: Option<i32>) -> String {
    let host = host.unwrap_or_default();
    let host = host.trim();
    let port = port.unwrap_or(0);
    if host.is_empty() || port <= 0 {
        "Not configured.".to_string()
    } else {
        format!("{host}:{port}")
    }
}

fn recent_files_detail(enabled: bool) -> &'static str {
    if enabled {
        "Applications can keep a recent-files list for faster reopening."
    } else {
        "Applications should not keep a recent-files list in this desktop session."
    }
}

fn app_usage_detail(enabled: bool) -> &'static str {
    if enabled {
        "The desktop can remember application usage for launchers and suggestions."
    } else {
        "Application usage should not be monitored or recorded by the desktop."
    }
}

fn cleanup_trash_detail(enabled: bool) -> &'static str {
    if enabled {
        "Trash items older than the cleanup age are removed automatically."
    } else {
        "Trash is kept until you empty it or another cleanup tool removes it."
    }
}

fn cleanup_temp_detail(enabled: bool) -> &'static str {
    if enabled {
        "Temporary files older than the cleanup age are removed automatically."
    } else {
        "Temporary files are not removed by the desktop privacy cleanup setting."
    }
}

fn microphone_access_detail(blocked: bool) -> &'static str {
    if blocked {
        "Applications should not use the microphone while this desktop privacy setting is on."
    } else {
        "Applications may request microphone access through the desktop session."
    }
}

fn camera_access_detail(blocked: bool) -> &'static str {
    if blocked {
        "Applications should not use the camera while this desktop privacy setting is on."
    } else {
        "Applications may request camera access through the desktop session."
    }
}

fn sound_output_access_detail(blocked: bool) -> &'static str {
    if blocked {
        "Applications should not produce sound while this desktop privacy setting is on."
    } else {
        "Applications may play sound through the desktop session."
    }
}

fn usb_protection_detail(enabled: bool) -> &'static str {
    if enabled {
        "New USB devices are protected when the desktop and USBGuard support the policy."
    } else {
        "The desktop USB protection preference is off."
    }
}

fn screen_reader_detail(enabled: bool) -> &'static str {
    if enabled {
        "Screen reader support is enabled for the desktop session."
    } else {
        "Screen reader support is off until you turn it on for this session."
    }
}

fn screen_keyboard_detail(enabled: bool) -> &'static str {
    if enabled {
        "The on-screen keyboard can appear for text entry when the desktop needs it."
    } else {
        "The on-screen keyboard stays hidden unless another accessibility tool enables it."
    }
}

fn magnifier_detail(enabled: bool) -> &'static str {
    if enabled {
        "Screen magnification is enabled for the desktop session."
    } else {
        "Screen magnification is off. Text size still follows the setting below."
    }
}

fn night_light_detail(enabled: bool) -> &'static str {
    if enabled {
        "Night Light is on. The display shifts warmer when the schedule says it should."
    } else {
        "Night Light is off. The display keeps its normal color temperature."
    }
}

fn night_light_schedule_detail(automatic: bool) -> &'static str {
    if automatic {
        "Uses the desktop location and time zone to schedule warmer color automatically."
    } else {
        "Uses the manual Night Light schedule stored by the desktop session."
    }
}

fn normalized_night_light_temperature(temperature: u32) -> u32 {
    round_to_step(temperature.clamp(1000, 10000), 100)
}

fn night_light_temperature_label(value: f64) -> String {
    let temperature = normalized_night_light_temperature(value.round() as u32);
    format!("{temperature} K")
}

struct DisplaySummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn display_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> DisplaySummarySpec {
    DisplaySummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn display_session_summary_spec(
    displays: Option<&DisplaysStatus>,
    hardware: Option<&HardwareStatus>,
) -> DisplaySummarySpec {
    if let Some(displays) = displays {
        return display_summary_spec(
            "Session",
            displays.session_type.as_str(),
            display_has_session_handle(displays),
            display_session_summary_detail(displays),
        );
    }

    if let Some(hardware) = hardware {
        return display_summary_spec(
            "Session",
            hardware.platform.session_type.as_str(),
            !hardware.platform.session_type.trim().is_empty()
                && hardware.platform.session_type != "unconfigured",
            format!(
                "{} · {} · {} · {}",
                hardware.platform.desktop,
                hardware.platform.session_type,
                hardware.platform.current_desktop,
                hardware.source
            ),
        );
    }

    display_summary_spec(
        "Session",
        "waiting",
        false,
        "Waiting for display session status.",
    )
}

fn display_query_summary_spec(displays: Option<&DisplaysStatus>) -> DisplaySummarySpec {
    let Some(displays) = displays else {
        return display_summary_spec(
            "Display query",
            "waiting",
            false,
            "Waiting for display query status.",
        );
    };

    display_summary_spec(
        "Display query",
        display_query_state(displays),
        displays.mutter_display_config_available || !displays.outputs.is_empty(),
        format!("{} · {}", displays.detail, display_handles_detail(displays)),
    )
}

fn display_outputs_summary_spec(displays: Option<&DisplaysStatus>) -> DisplaySummarySpec {
    let Some(displays) = displays else {
        return display_summary_spec(
            "Detected displays",
            "waiting",
            false,
            "Waiting for display outputs.",
        );
    };

    let connected = displays
        .outputs
        .iter()
        .filter(|output| output.connected)
        .count();
    let state = match connected {
        0 => "none".to_string(),
        1 => "1 display".to_string(),
        count => format!("{count} displays"),
    };

    display_summary_spec(
        "Detected displays",
        state,
        connected > 0,
        display_outputs_summary_detail(&displays.outputs),
    )
}

fn display_comfort_summary_spec(accessibility: Option<&AccessibilityStatus>) -> DisplaySummarySpec {
    let Some(accessibility) = accessibility else {
        return display_summary_spec(
            "Display comfort",
            "waiting",
            false,
            "Waiting for display-comfort preferences.",
        );
    };
    let display = &accessibility.display_comfort;
    if !display.schema_available {
        return display_summary_spec(
            "Display comfort",
            "unavailable",
            false,
            display.detail.as_str(),
        );
    }

    match display.night_light_enabled {
        Some(enabled) => display_summary_spec(
            "Display comfort",
            if enabled {
                "night light on"
            } else {
                "night light off"
            },
            true,
            display_comfort_summary_detail(display),
        ),
        None => display_summary_spec(
            "Display comfort",
            "unknown",
            false,
            "Night Light preference is not reported in this desktop session.",
        ),
    }
}

fn display_has_session_handle(displays: &DisplaysStatus) -> bool {
    displays
        .wayland_display
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || displays
            .x11_display
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

fn display_query_state(displays: &DisplaysStatus) -> &'static str {
    if displays.mutter_display_config_available {
        "desktop bridge"
    } else if !displays.outputs.is_empty() {
        "fallback"
    } else {
        "limited"
    }
}

fn display_session_summary_detail(displays: &DisplaysStatus) -> String {
    if displays.outputs.is_empty() {
        "Display status is available, but no individual outputs were reported.".to_string()
    } else {
        "Display status is available for this session.".to_string()
    }
}

fn display_outputs_summary_detail(outputs: &[DisplayOutputStatus]) -> String {
    if outputs.is_empty() {
        return "No individual display outputs were reported by the display system.".to_string();
    }

    let connected = outputs.iter().filter(|output| output.connected).count();
    let disconnected = outputs.len().saturating_sub(connected);
    let primary = outputs
        .iter()
        .find(|output| output.primary)
        .map(|output| output.name.as_str())
        .unwrap_or("not reported");
    let first_mode = outputs
        .iter()
        .find_map(|output| output.current_mode.as_deref())
        .unwrap_or("mode not reported");
    format!(
        "{connected} connected · {disconnected} disconnected · Primary {primary} · {first_mode}"
    )
}

fn display_comfort_summary_detail(display: &DisplayComfortStatus) -> String {
    let night_light = option_on_off_word(display.night_light_enabled);
    let schedule = display
        .schedule_automatic
        .map(|automatic| if automatic { "automatic" } else { "manual" })
        .unwrap_or("unknown schedule");
    let temperature = display
        .temperature
        .map(|value| format!("{} K", normalized_night_light_temperature(value)))
        .unwrap_or_else(|| "temperature unavailable".to_string());

    format!("Night Light {night_light} · {schedule} · {temperature}")
}

fn display_handles_detail(displays: &DisplaysStatus) -> String {
    let wayland = displays
        .wayland_display
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("not reported");
    let x11 = displays
        .x11_display
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("not reported");
    format!(
        "Graphics session: Wayland {wayland} · X11 {x11}. Display service {} · display query {}.",
        display_availability_word(displays.gdbus_available),
        display_availability_word(displays.xrandr_available)
    )
}

fn display_availability_word(available: bool) -> &'static str {
    if available {
        "ready"
    } else {
        "not ready"
    }
}

fn display_output_title(output: &DisplayOutputStatus) -> String {
    if output.primary {
        format!("{} · primary", output.name)
    } else {
        output.name.clone()
    }
}

fn display_output_detail(output: &DisplayOutputStatus) -> String {
    let mut detail = output.detail.clone();
    if let Some(mode) = output
        .current_mode
        .as_deref()
        .filter(|mode| !mode.trim().is_empty())
    {
        detail.push_str(&format!(" Mode {mode}."));
    }
    if let Some(position) = output
        .position
        .as_deref()
        .filter(|position| !position.trim().is_empty())
    {
        detail.push_str(&format!(" Position {position}."));
    }
    if !output.connected {
        detail.push_str(" Settings will not expose controls for disconnected outputs.");
    }
    detail
}

fn normalized_text_scale(scale: f64) -> f64 {
    if !scale.is_finite() {
        return 1.0;
    }
    ((scale.clamp(0.85, 1.35) * 20.0).round() / 20.0 * 100.0).round() / 100.0
}

fn text_scale_percent(scale: f64) -> String {
    format!("{:.0}%", normalized_text_scale(scale) * 100.0)
}

fn key_repeat_detail(enabled: bool) -> &'static str {
    if enabled {
        "Holding a key repeats it after the configured delay."
    } else {
        "Holding a key enters one character until repeat is enabled again."
    }
}

fn remember_numlock_detail(enabled: bool) -> &'static str {
    if enabled {
        "The desktop restores the last Num Lock state when the session starts."
    } else {
        "The desktop does not restore Num Lock state automatically."
    }
}

fn mouse_natural_scroll_detail(enabled: bool) -> &'static str {
    if enabled {
        "Mouse wheel movement follows natural content direction."
    } else {
        "Mouse wheel movement follows traditional scroll direction."
    }
}

fn touchpad_natural_scroll_detail(enabled: bool) -> &'static str {
    if enabled {
        "Trackpad gestures move content in the same direction as your fingers."
    } else {
        "Trackpad gestures use traditional scroll direction."
    }
}

fn primary_button_detail(right_primary: bool) -> &'static str {
    if right_primary {
        "The right mouse button is the primary click."
    } else {
        "The left mouse button is the primary click."
    }
}

fn middle_click_detail(enabled: bool) -> &'static str {
    if enabled {
        "Pressing the left and right mouse buttons together sends a middle click."
    } else {
        "Middle click is sent only by a dedicated middle button or device gesture."
    }
}

fn tap_to_click_detail(enabled: bool) -> &'static str {
    if enabled {
        "Tapping the trackpad surface clicks without pressing down."
    } else {
        "Trackpad clicks require a physical press."
    }
}

fn two_finger_scroll_detail(enabled: bool) -> &'static str {
    if enabled {
        "Two-finger gestures scroll when the touchpad driver supports them."
    } else {
        "Two-finger scrolling is disabled for the touchpad."
    }
}

fn disable_trackpad_while_typing_detail(enabled: bool) -> &'static str {
    if enabled {
        "The trackpad is ignored while typing to reduce accidental pointer movement."
    } else {
        "The trackpad remains active while typing."
    }
}

struct InputSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn input_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> InputSummarySpec {
    InputSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn input_source_summary_spec(input: Option<&InputStatus>) -> InputSummarySpec {
    let Some(input) = input else {
        return input_summary_spec("Input", "waiting", false, "Waiting for input preferences.");
    };

    if input.gsettings_available {
        input_summary_spec("Input", "available", true, input.detail.as_str())
    } else {
        input_summary_spec("Input", "unavailable", false, input.detail.as_str())
    }
}

fn keyboard_repeat_summary_spec(input: Option<&InputStatus>) -> InputSummarySpec {
    let keyboard =
        match keyboard_summary_status(input, "Key repeat", "Waiting for keyboard preferences.") {
            Ok(keyboard) => keyboard,
            Err(summary) => return summary,
        };

    match keyboard.repeat {
        Some(enabled) => input_summary_spec(
            "Key repeat",
            option_on_off_word(Some(enabled)),
            true,
            key_repeat_detail(enabled),
        ),
        None => input_summary_spec(
            "Key repeat",
            "unknown",
            false,
            "The keyboard repeat preference is not reported by this session.",
        ),
    }
}

fn keyboard_delay_summary_spec(input: Option<&InputStatus>) -> InputSummarySpec {
    let keyboard = match keyboard_summary_status(
        input,
        "Repeat delay",
        "Waiting for keyboard repeat delay.",
    ) {
        Ok(keyboard) => keyboard,
        Err(summary) => return summary,
    };

    match keyboard.delay_ms {
        Some(delay) => {
            let delay = normalized_keyboard_delay(delay);
            input_summary_spec(
                "Repeat delay",
                milliseconds_label(f64::from(delay)),
                true,
                format!("Holding a key for {delay} ms starts repeat when key repeat is enabled."),
            )
        }
        None => input_summary_spec(
            "Repeat delay",
            "unknown",
            false,
            "The keyboard repeat delay preference is not reported by this session.",
        ),
    }
}

fn keyboard_interval_summary_spec(input: Option<&InputStatus>) -> InputSummarySpec {
    let keyboard = match keyboard_summary_status(
        input,
        "Repeat interval",
        "Waiting for keyboard repeat interval.",
    ) {
        Ok(keyboard) => keyboard,
        Err(summary) => return summary,
    };

    match keyboard.repeat_interval_ms {
        Some(interval) => {
            let interval = normalized_keyboard_repeat_interval(interval);
            input_summary_spec(
                "Repeat interval",
                milliseconds_label(f64::from(interval)),
                true,
                format!("Repeated key events are spaced by {interval} ms across the desktop."),
            )
        }
        None => input_summary_spec(
            "Repeat interval",
            "unknown",
            false,
            "The keyboard repeat interval preference is not reported by this session.",
        ),
    }
}

fn keyboard_summary_status<'a>(
    input: Option<&'a InputStatus>,
    title: &'static str,
    waiting_detail: &'static str,
) -> Result<&'a KeyboardInputStatus, InputSummarySpec> {
    let Some(input) = input else {
        return Err(input_summary_spec(title, "waiting", false, waiting_detail));
    };
    if !input.gsettings_available {
        return Err(input_summary_spec(
            title,
            "unavailable",
            false,
            input.detail.as_str(),
        ));
    }

    let keyboard = &input.keyboard;
    if !keyboard.schema_available {
        return Err(input_summary_spec(
            title,
            "unavailable",
            false,
            keyboard.detail.as_str(),
        ));
    }

    Ok(keyboard)
}

fn mouse_speed_summary_spec(input: Option<&InputStatus>) -> InputSummarySpec {
    let mouse = match mouse_summary_status(input) {
        Ok(mouse) => mouse,
        Err(summary) => return summary,
    };

    match mouse.speed {
        Some(speed) => input_summary_spec(
            "Mouse speed",
            pointer_speed_label(speed),
            true,
            "Mouse tracking speed is backed by the system pointer setting.",
        ),
        None => input_summary_spec(
            "Mouse speed",
            "unknown",
            false,
            "The mouse tracking speed preference is not reported by this session.",
        ),
    }
}

fn touchpad_speed_summary_spec(input: Option<&InputStatus>) -> InputSummarySpec {
    let touchpad =
        match touchpad_summary_status(input, "Trackpad speed", "Waiting for trackpad preferences.")
        {
            Ok(touchpad) => touchpad,
            Err(summary) => return summary,
        };

    match touchpad.speed {
        Some(speed) => input_summary_spec(
            "Trackpad speed",
            pointer_speed_label(speed),
            true,
            "Trackpad tracking speed is backed by the system touchpad setting.",
        ),
        None => input_summary_spec(
            "Trackpad speed",
            "unknown",
            false,
            "The trackpad tracking speed preference is not reported by this session.",
        ),
    }
}

fn touchpad_tap_summary_spec(input: Option<&InputStatus>) -> InputSummarySpec {
    let touchpad = match touchpad_summary_status(
        input,
        "Tap to click",
        "Waiting for trackpad tap preferences.",
    ) {
        Ok(touchpad) => touchpad,
        Err(summary) => return summary,
    };

    match touchpad.tap_to_click {
        Some(enabled) => input_summary_spec(
            "Tap to click",
            option_on_off_word(Some(enabled)),
            true,
            tap_to_click_detail(enabled),
        ),
        None => input_summary_spec(
            "Tap to click",
            "unknown",
            false,
            "The tap-to-click preference is not reported by this session.",
        ),
    }
}

fn mouse_summary_status(
    input: Option<&InputStatus>,
) -> Result<&MouseInputStatus, InputSummarySpec> {
    let Some(input) = input else {
        return Err(input_summary_spec(
            "Mouse speed",
            "waiting",
            false,
            "Waiting for mouse preferences.",
        ));
    };
    if !input.gsettings_available {
        return Err(input_summary_spec(
            "Mouse speed",
            "unavailable",
            false,
            input.detail.as_str(),
        ));
    }

    let mouse = &input.mouse;
    if !mouse.schema_available {
        return Err(input_summary_spec(
            "Mouse speed",
            "unavailable",
            false,
            mouse.detail.as_str(),
        ));
    }

    Ok(mouse)
}

fn touchpad_summary_status<'a>(
    input: Option<&'a InputStatus>,
    title: &'static str,
    waiting_detail: &'static str,
) -> Result<&'a TouchpadInputStatus, InputSummarySpec> {
    let Some(input) = input else {
        return Err(input_summary_spec(title, "waiting", false, waiting_detail));
    };
    if !input.gsettings_available {
        return Err(input_summary_spec(
            title,
            "unavailable",
            false,
            input.detail.as_str(),
        ));
    }

    let touchpad = &input.touchpad;
    if !touchpad.schema_available {
        return Err(input_summary_spec(
            title,
            "unavailable",
            false,
            touchpad.detail.as_str(),
        ));
    }

    Ok(touchpad)
}

fn interface_sounds_detail(enabled: bool) -> &'static str {
    if enabled {
        "Alerts and interface events can play the configured desktop sound theme."
    } else {
        "Alerts and interface events stay visually indicated without desktop sounds."
    }
}

fn input_feedback_sounds_detail(enabled: bool) -> &'static str {
    if enabled {
        "Typing and input feedback sounds may play when supported by the desktop session."
    } else {
        "Typing and input feedback sounds are muted across the desktop session."
    }
}

fn volume_boost_detail(enabled: bool) -> &'static str {
    if enabled {
        "Output volume controls may exceed 100% when the audio stack supports amplification."
    } else {
        "Output volume is capped at the normal 100% range to avoid accidental amplification."
    }
}

fn sound_theme_detail(theme: Option<&str>) -> String {
    match theme {
        Some(theme) if !theme.trim().is_empty() => format!("{theme} · read-only here"),
        Some(_) => "No desktop sound theme is configured.".to_string(),
        None => "Sound theme preference is not available in this session.".to_string(),
    }
}

struct SoundSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn sound_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> SoundSummarySpec {
    SoundSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn audio_service_summary_spec(audio: Option<&AudioStatus>) -> SoundSummarySpec {
    let Some(audio) = audio else {
        return sound_summary_spec(
            "Audio service",
            "waiting",
            false,
            "Waiting for audio service status.",
        );
    };

    let endpoints_available = audio.output.available || audio.input.available;
    let state = if !audio.wireplumber_available {
        "unavailable"
    } else if endpoints_available {
        "available"
    } else {
        "limited"
    };

    sound_summary_spec(
        "Audio service",
        state,
        audio.wireplumber_available && endpoints_available,
        polished_audio_service_detail(audio),
    )
}

fn polished_audio_service_detail(audio: &AudioStatus) -> String {
    let detail = audio.detail.trim();
    if detail.is_empty() {
        return if audio.wireplumber_available {
            "Audio routing status is available.".to_string()
        } else {
            "Audio routing support is not ready.".to_string()
        };
    }

    let lower = detail.to_ascii_lowercase();
    if lower.contains("wireplumber control tooling is not ready")
        || lower.contains("wireplumber control tooling is not installed")
    {
        return "Audio routing support is not ready in this build.".to_string();
    }

    detail
        .replace("WirePlumber", "Audio routing")
        .replace("PipeWire", "audio routing")
}

fn audio_endpoint_summary_spec(
    title: &'static str,
    target: &'static str,
    endpoint: Option<&AudioEndpointStatus>,
) -> SoundSummarySpec {
    let Some(endpoint) = endpoint else {
        return sound_summary_spec(
            title,
            "waiting",
            false,
            format!("Waiting for {} status.", audio_target_kind(target)),
        );
    };

    let state = if !endpoint.available {
        "unavailable".to_string()
    } else if endpoint.muted == Some(true) {
        "muted".to_string()
    } else {
        endpoint
            .volume_percent
            .map(|volume| format!("{volume}%"))
            .unwrap_or_else(|| "available".to_string())
    };

    sound_summary_spec(
        title,
        state,
        endpoint.available,
        audio_endpoint_summary_detail(target, endpoint),
    )
}

fn audio_endpoint_summary_detail(target: &str, endpoint: &AudioEndpointStatus) -> String {
    if !endpoint.available {
        return audio_endpoint_unavailable_detail(target, endpoint);
    }

    let current = endpoint
        .default_device_id
        .as_deref()
        .and_then(|device_id| {
            endpoint
                .devices
                .iter()
                .find(|device| device.id == device_id)
        })
        .or_else(|| endpoint.devices.iter().find(|device| device.active));

    match current {
        Some(device) => format!(
            "{} · Default {}: {}.",
            endpoint.detail,
            audio_target_kind(target),
            device.name
        ),
        None if endpoint.devices.is_empty() => format!(
            "{} · No selectable {} devices were returned.",
            endpoint.detail,
            audio_target_kind(target)
        ),
        None => format!(
            "{} · {} selectable {} devices, but no default was marked.",
            endpoint.detail,
            endpoint.devices.len(),
            audio_target_kind(target)
        ),
    }
}

fn audio_endpoint_unavailable_detail(target: &str, endpoint: &AudioEndpointStatus) -> String {
    let target_kind = audio_target_kind(target);
    let detail = endpoint.detail.trim();
    let lower = detail.to_ascii_lowercase();

    if lower.contains("could not connect to pipewire") {
        return format!(
            "Audio routing is not reachable in this session, so {target_kind} devices and volume controls are not ready."
        );
    }
    if lower.contains("wireplumber control tooling is not ready")
        || lower.contains("wireplumber control tooling is not installed")
    {
        return format!(
            "Audio routing support is not ready in this build, so {target_kind} devices and volume controls stay disabled."
        );
    }
    if lower.contains("did not report") {
        return format!(
            "Audio routing did not report a readable {target_kind} endpoint in this session."
        );
    }
    if detail.is_empty() {
        return format!(
            "The Goblins OS did not report a readable {target_kind} endpoint in this session."
        );
    }

    let concise = detail
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("E ") && !line.starts_with("W "))
        .unwrap_or(detail)
        .replace('\n', " ")
        .replace("WirePlumber", "Audio routing")
        .replace("PipeWire", "audio routing");
    format!("{target_kind} status is not ready: {concise}")
}

fn sound_preferences_summary_spec(audio: Option<&AudioStatus>) -> SoundSummarySpec {
    let Some(sound) = audio.and_then(|audio| audio.sound.as_ref()) else {
        return sound_summary_spec(
            "System sounds",
            "waiting",
            false,
            "Waiting for desktop sound preferences.",
        );
    };

    if !(sound.gsettings_available && sound.schema_available) {
        return sound_summary_spec("System sounds", "unavailable", false, sound.detail.as_str());
    }

    sound_summary_spec(
        "System sounds",
        "available",
        true,
        format!(
            "Interface sounds {} · Input feedback {} · Volume boost {} · {}",
            option_on_off_word(sound.event_sounds),
            option_on_off_word(sound.input_feedback_sounds),
            option_on_off_word(sound.volume_boost),
            sound_theme_summary_detail(sound.theme_name.as_deref())
        ),
    )
}

fn sound_theme_summary_detail(theme: Option<&str>) -> String {
    match theme {
        Some(theme) if !theme.trim().is_empty() => format!("Theme {theme}"),
        Some(_) => "No sound theme configured".to_string(),
        None => "Theme not ready".to_string(),
    }
}

fn option_on_off_word(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "unknown",
    }
}

fn normalized_audio_volume(volume: f64) -> f64 {
    if !volume.is_finite() {
        return 0.0;
    }
    volume.clamp(0.0, 150.0).round()
}

fn audio_volume_label(volume: f64) -> String {
    format!("{:.0}%", normalized_audio_volume(volume))
}

fn audio_endpoint_title(target: &str) -> &'static str {
    match target {
        "input" => "Input volume",
        _ => "Output volume",
    }
}

fn audio_device_choice_title(target: &str) -> &'static str {
    match target {
        "input" => "Input device",
        _ => "Output device",
    }
}

fn audio_device_unavailable_detail(target: &str, endpoint: &AudioEndpointStatus) -> String {
    if endpoint.available {
        format!(
            "Audio routing reports the default {}, but no selectable {} devices were returned.",
            audio_target_kind(target),
            audio_target_kind(target)
        )
    } else {
        format!(
            "{} Device selection stays disabled until audio routing reports selectable {} devices.",
            audio_endpoint_unavailable_detail(target, endpoint),
            audio_target_kind(target)
        )
    }
}

fn audio_single_device_detail(target: &str, device: &AudioDeviceStatus) -> String {
    let suffix = if device.active {
        "It is the current default."
    } else {
        "Audio routing did not mark it as the default in this session."
    };
    format!(
        "{} is the only reported {} device. {}",
        device.name,
        audio_target_kind(target),
        suffix
    )
}

fn audio_device_choice_detail(
    target: &str,
    devices: &[AudioDeviceStatus],
    device_id: &str,
) -> String {
    let device = devices
        .iter()
        .find(|device| device.id == device_id)
        .map(|device| device.name.as_str())
        .unwrap_or("Selected device");
    format!(
        "{} is the default {}. Changes are saved through Goblins OS audio routing.",
        device,
        audio_target_kind(target)
    )
}

fn audio_target_kind(target: &str) -> &'static str {
    match target {
        "input" => "input",
        _ => "output",
    }
}

fn audio_volume_title(target: &str) -> &'static str {
    match target {
        "input" => "Input volume",
        _ => "Output volume",
    }
}

fn audio_volume_detail(target: &str) -> &'static str {
    match target {
        "input" => "Adjusts the default input gain through Goblins OS.",
        _ => "Adjusts the default output volume through Goblins OS.",
    }
}

fn audio_mute_title(target: &str) -> &'static str {
    match target {
        "input" => "Mute input",
        _ => "Mute output",
    }
}

fn audio_mute_detail(target: &str, muted: bool) -> &'static str {
    match (target, muted) {
        ("input", true) => "The default microphone/input is muted.",
        ("input", false) => "The default microphone/input can capture audio.",
        (_, true) => "The default speakers/output are muted.",
        _ => "The default speakers/output can play audio.",
    }
}

fn appearance_scheme_detail(theme: &str) -> &'static str {
    match theme {
        "light" => "Light appearance is active across Goblins OS and apps that follow the system setting.",
        "dark" => "Dark appearance is active across Goblins OS and apps that follow the system setting.",
        _ => "Auto appearance is active. Goblins OS follows the system appearance setting when it changes.",
    }
}

fn normalized_appearance_theme(theme: &str) -> &'static str {
    match theme {
        "light" | "prefer-light" => "light",
        "dark" | "prefer-dark" => "dark",
        _ => "auto",
    }
}

struct AppearanceSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn appearance_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> AppearanceSummarySpec {
    AppearanceSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn appearance_scheme_summary_spec(appearance: Option<&AppearanceStatus>) -> AppearanceSummarySpec {
    let Some(appearance) = appearance else {
        return appearance_summary_spec(
            "Color scheme",
            "waiting",
            false,
            "Waiting for appearance status.",
        );
    };
    if !appearance.color_scheme_available {
        return appearance_summary_spec(
            "Color scheme",
            "unavailable",
            false,
            appearance.detail.as_str(),
        );
    }

    let current = normalized_appearance_theme(&appearance.theme);
    // Name the system value with the same friendly label as the pill/selector
    // (Auto/Light/Dark), never the raw gsettings token (default/prefer-*).
    let scheme_label = match appearance.color_scheme.as_str() {
        "prefer-dark" => "Dark",
        "prefer-light" => "Light",
        _ => "Auto",
    };
    appearance_summary_spec(
        "Color scheme",
        current,
        true,
        format!(
            "{} Current system value is {}.",
            appearance_scheme_detail(current),
            scheme_label
        ),
    )
}

fn appearance_typography_summary_spec() -> AppearanceSummarySpec {
    appearance_summary_spec(
        "Typography",
        "Inter",
        true,
        "Inter 11 is the system font across Goblins OS and included desktop utilities.",
    )
}

fn appearance_motion_summary_spec(
    accessibility: Option<&AccessibilityStatus>,
) -> AppearanceSummarySpec {
    let Some(accessibility) = accessibility else {
        return appearance_summary_spec(
            "Motion",
            "waiting",
            false,
            "Waiting for motion preference status.",
        );
    };
    let interface = &accessibility.interface;
    if !interface.schema_available {
        return appearance_summary_spec("Motion", "unavailable", false, interface.detail.as_str());
    }

    match interface.reduce_motion {
        Some(true) => {
            appearance_summary_spec("Motion", "reduced", true, motion_preference_detail(true))
        }
        Some(false) => appearance_summary_spec(
            "Motion",
            "full motion",
            true,
            motion_preference_detail(false),
        ),
        None => appearance_summary_spec(
            "Motion",
            "unknown",
            false,
            "The animation preference is not available in this session.",
        ),
    }
}

fn appearance_text_size_summary_spec(
    accessibility: Option<&AccessibilityStatus>,
) -> AppearanceSummarySpec {
    let Some(accessibility) = accessibility else {
        return appearance_summary_spec(
            "Text size",
            "waiting",
            false,
            "Waiting for text scaling status.",
        );
    };
    let interface = &accessibility.interface;
    if !interface.schema_available {
        return appearance_summary_spec(
            "Text size",
            "unavailable",
            false,
            interface.detail.as_str(),
        );
    }

    match interface.text_scale {
        Some(scale) => {
            let percent = text_scale_percent(scale);
            appearance_summary_spec(
                "Text size",
                percent.clone(),
                true,
                format!("Desktop text scale is {percent}; Goblins OS and system utilities follow the same setting."),
            )
        }
        None => appearance_summary_spec(
            "Text size",
            "unknown",
            false,
            "The text scaling preference is not available in this session.",
        ),
    }
}

struct AccessibilitySummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn accessibility_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> AccessibilitySummarySpec {
    AccessibilitySummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn accessibility_text_size_summary_spec(
    accessibility: Option<&AccessibilityStatus>,
) -> AccessibilitySummarySpec {
    let Some(accessibility) = accessibility else {
        return accessibility_summary_spec(
            "Text size",
            "waiting",
            false,
            "Waiting for text scaling status.",
        );
    };
    let interface = &accessibility.interface;
    if !accessibility.gsettings_available || !interface.schema_available {
        return accessibility_summary_spec(
            "Text size",
            "unavailable",
            false,
            interface.detail.as_str(),
        );
    }

    match interface.text_scale {
        Some(scale) => {
            let percent = text_scale_percent(scale);
            accessibility_summary_spec(
                "Text size",
                percent.clone(),
                true,
                format!(
                    "Desktop text scale is {percent}; Goblins OS and system utilities follow it."
                ),
            )
        }
        None => accessibility_summary_spec(
            "Text size",
            "unknown",
            false,
            "The text scaling preference is not available in this session.",
        ),
    }
}

fn accessibility_motion_summary_spec(
    accessibility: Option<&AccessibilityStatus>,
) -> AccessibilitySummarySpec {
    let Some(accessibility) = accessibility else {
        return accessibility_summary_spec(
            "Motion",
            "waiting",
            false,
            "Waiting for motion preference status.",
        );
    };
    let interface = &accessibility.interface;
    if !accessibility.gsettings_available || !interface.schema_available {
        return accessibility_summary_spec(
            "Motion",
            "unavailable",
            false,
            interface.detail.as_str(),
        );
    }

    match interface.reduce_motion {
        Some(true) => {
            accessibility_summary_spec("Motion", "reduced", true, motion_preference_detail(true))
        }
        Some(false) => accessibility_summary_spec(
            "Motion",
            "full motion",
            true,
            motion_preference_detail(false),
        ),
        None => accessibility_summary_spec(
            "Motion",
            "unknown",
            false,
            "The animation preference is not available in this session.",
        ),
    }
}

fn assistive_access_summary_spec(
    accessibility: Option<&AccessibilityStatus>,
) -> AccessibilitySummarySpec {
    let Some(accessibility) = accessibility else {
        return accessibility_summary_spec(
            "Assistive access",
            "waiting",
            false,
            "Waiting for assistive technology status.",
        );
    };
    let assistive = &accessibility.assistive;
    if !accessibility.gsettings_available || !assistive.schema_available {
        return accessibility_summary_spec(
            "Assistive access",
            "unavailable",
            false,
            assistive.detail.as_str(),
        );
    }

    let known = [
        assistive.screen_reader,
        assistive.screen_keyboard,
        assistive.magnifier,
    ]
    .into_iter()
    .filter(Option::is_some)
    .count();
    if known == 0 {
        return accessibility_summary_spec(
            "Assistive access",
            "unknown",
            false,
            "Accessibility settings are available, but this session did not report assistive technology keys.",
        );
    }

    let enabled = [
        assistive.screen_reader,
        assistive.screen_keyboard,
        assistive.magnifier,
    ]
    .into_iter()
    .filter(|value| *value == Some(true))
    .count();
    let state = if enabled == 0 {
        "off".to_string()
    } else {
        format!("{enabled} enabled")
    };

    accessibility_summary_spec(
        "Assistive access",
        state,
        true,
        format!(
            "Screen reader {} · Keyboard {} · Magnifier {}.",
            option_on_off_word(assistive.screen_reader),
            option_on_off_word(assistive.screen_keyboard),
            option_on_off_word(assistive.magnifier)
        ),
    )
}

fn accessibility_display_comfort_summary_spec(
    accessibility: Option<&AccessibilityStatus>,
) -> AccessibilitySummarySpec {
    let Some(accessibility) = accessibility else {
        return accessibility_summary_spec(
            "Display comfort",
            "waiting",
            false,
            "Waiting for display-comfort status.",
        );
    };
    let display = &accessibility.display_comfort;
    if !accessibility.gsettings_available || !display.schema_available {
        return accessibility_summary_spec(
            "Display comfort",
            "unavailable",
            false,
            display.detail.as_str(),
        );
    }

    match display.night_light_enabled {
        Some(enabled) => accessibility_summary_spec(
            "Display comfort",
            if enabled {
                "night light on"
            } else {
                "night light off"
            },
            true,
            display_comfort_summary_detail(display),
        ),
        None => accessibility_summary_spec(
            "Display comfort",
            "unknown",
            false,
            "Night Light preference is not reported in this desktop session.",
        ),
    }
}

struct NotificationSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn notification_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> NotificationSummarySpec {
    NotificationSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn notifications_delivery_summary_spec(
    notifications: Option<&NotificationsStatus>,
) -> NotificationSummarySpec {
    let Some(notifications) = notifications else {
        return notification_summary_spec(
            "Delivery",
            "waiting",
            false,
            "Waiting for notification preferences.",
        );
    };

    notification_summary_spec(
        "Delivery",
        if notifications.gsettings_available && notifications.schema_available {
            "available"
        } else {
            "unavailable"
        },
        notifications.gsettings_available && notifications.schema_available,
        notifications.detail.as_str(),
    )
}

fn notification_banners_summary_spec(
    notifications: Option<&NotificationsStatus>,
) -> NotificationSummarySpec {
    let Some(notifications) = notifications else {
        return notification_summary_spec(
            "Banners",
            "waiting",
            false,
            "Waiting for banner preference status.",
        );
    };
    if !notifications.schema_available {
        return notification_summary_spec(
            "Banners",
            "unavailable",
            false,
            notifications.detail.as_str(),
        );
    }

    notification_summary_spec(
        "Banners",
        notification_bool_summary_state(notifications.show_banners),
        notifications.show_banners.is_some(),
        notifications
            .show_banners
            .map(notification_banners_detail)
            .unwrap_or("Banner notification preference is not reported by this desktop session."),
    )
}

fn notification_lock_screen_summary_spec(
    notifications: Option<&NotificationsStatus>,
) -> NotificationSummarySpec {
    let Some(notifications) = notifications else {
        return notification_summary_spec(
            "Lock screen",
            "waiting",
            false,
            "Waiting for lock-screen notification status.",
        );
    };
    if !notifications.schema_available {
        return notification_summary_spec(
            "Lock screen",
            "unavailable",
            false,
            notifications.detail.as_str(),
        );
    }

    notification_summary_spec(
        "Lock screen",
        notification_bool_summary_state(notifications.show_in_lock_screen),
        notifications.show_in_lock_screen.is_some(),
        notifications
            .show_in_lock_screen
            .map(lock_screen_notifications_detail)
            .unwrap_or(
                "Lock-screen notification preference is not reported by this desktop session.",
            ),
    )
}

fn notification_app_registry_summary_spec(
    notifications: Option<&NotificationsStatus>,
) -> NotificationSummarySpec {
    let Some(notifications) = notifications else {
        return notification_summary_spec(
            "Per-app entries",
            "waiting",
            false,
            "Waiting for per-application notification entries.",
        );
    };
    if !notifications.schema_available {
        return notification_summary_spec(
            "Per-app entries",
            "unavailable",
            false,
            notifications.detail.as_str(),
        );
    }
    if !notifications.application_schema_available {
        return notification_summary_spec(
            "Per-app entries",
            "unavailable",
            false,
            "Per-application notification settings are not available in this session.",
        );
    }

    notification_summary_spec(
        "Per-app entries",
        notification_app_count_label(notifications.application_children.len()),
        true,
        notification_app_children_detail(&notifications.application_children),
    )
}

fn notification_bool_summary_state(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "unknown",
    }
}

fn notification_app_count_label(count: usize) -> String {
    match count {
        0 => "none".to_string(),
        1 => "1 app".to_string(),
        count => format!("{count} apps"),
    }
}

fn notification_banners_detail(enabled: bool) -> &'static str {
    if enabled {
        "Apps can interrupt the desktop with notification banners when the shell receives them."
    } else {
        "Apps can still record notifications, but banners will not interrupt the desktop."
    }
}

fn lock_screen_notifications_detail(enabled: bool) -> &'static str {
    if enabled {
        "Notifications may appear while the session is locked, subject to each app's own notification policy."
    } else {
        "Notifications stay hidden from the lock screen until you unlock the session."
    }
}

fn notification_app_children_detail(children: &[String]) -> String {
    match children {
        [] => "No per-app notification entries are registered in this session yet.".to_string(),
        [one] => format!("1 per-app notification entry is registered: {one}."),
        _ => {
            let preview = children
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            if children.len() > 4 {
                format!(
                    "{} per-app notification entries are registered: {}, and {} more.",
                    children.len(),
                    preview,
                    children.len() - 4
                )
            } else {
                format!(
                    "{} per-app notification entries are registered: {}.",
                    children.len(),
                    preview
                )
            }
        }
    }
}

fn notification_app_enable_detail(enabled: bool) -> &'static str {
    if enabled {
        "This application can deliver notifications when global delivery allows it."
    } else {
        "This application is muted in the desktop notification registry."
    }
}

fn notification_app_banner_detail(enabled: bool) -> &'static str {
    if enabled {
        "Banners can appear for this application when notifications arrive."
    } else {
        "Notifications can still be recorded, but banners are hidden for this application."
    }
}

fn notification_app_sound_detail(enabled: bool) -> &'static str {
    if enabled {
        "Notifications from this application may play sound alerts."
    } else {
        "Notifications from this application stay silent."
    }
}

fn notification_app_lock_screen_detail(enabled: bool) -> &'static str {
    if enabled {
        "Notifications from this application may appear while the session is locked."
    } else {
        "Notifications from this application stay hidden while the session is locked."
    }
}

fn notification_app_lock_screen_details_detail(enabled: bool) -> &'static str {
    if enabled {
        "Notification summaries and bodies may be visible before unlock."
    } else {
        "Lock-screen notifications avoid exposing summaries and message bodies."
    }
}

fn notification_app_expand_detail(enabled: bool) -> &'static str {
    if enabled {
        "Banners from this application open expanded when the shell supports it."
    } else {
        "Banners from this application use the shell's normal compact presentation."
    }
}

fn model_cache_capacity_label(available_gb: Option<u64>) -> &'static str {
    match available_gb {
        Some(gb) if gb <= 5 => "critical",
        Some(gb) if gb <= 20 => "low space",
        Some(_) => "available",
        None => "unknown",
    }
}

fn model_cache_capacity_ready(available_gb: Option<u64>) -> bool {
    matches!(model_cache_capacity_label(available_gb), "available")
}

fn model_cache_capacity_detail(catalog: &LocalModelCatalog) -> String {
    let capacity = catalog
        .hardware
        .model_dir_available_gb
        .map(|gb| format!("{gb}GB free"))
        .unwrap_or_else(|| "free space unknown".to_string());

    format!(
        "{} in model cache · Engine: {}. {}",
        capacity,
        runtime_label(&catalog.hardware.runtime),
        catalog.install_policy
    )
}

fn storage_overall_pressure_label(
    hardware: Option<&HardwareStatus>,
    catalog: Option<&LocalModelCatalog>,
) -> &'static str {
    let mut rank = 0;

    if let Some(hardware) = hardware {
        for volume in &hardware.storage {
            rank = rank.max(storage_pressure_rank(storage_pressure_label(
                volume.total_gb,
                volume.available_gb,
            )));
        }
    }

    if let Some(catalog) = catalog {
        rank = rank.max(storage_pressure_rank(model_cache_capacity_label(
            catalog.hardware.model_dir_available_gb,
        )));
    }

    storage_pressure_label_for_rank(rank)
}

fn storage_overall_pressure_ready(label: &str) -> bool {
    matches!(label, "available")
}

fn storage_overall_pressure_detail(
    hardware: Option<&HardwareStatus>,
    catalog: Option<&LocalModelCatalog>,
    system: Option<&SettingsSystemStatus>,
) -> String {
    let mut parts = Vec::new();

    match hardware {
        Some(hardware) if !hardware.storage.is_empty() => {
            if let Some(volume) = most_constrained_storage_volume(&hardware.storage) {
                parts.push(format!(
                    "Most constrained volume: {} · {}.",
                    storage_volume_title(volume),
                    storage_volume_detail(volume)
                ));
                parts.push(format!(
                    "Usage: {}.",
                    storage_capacity_percent_text(volume.total_gb, volume.available_gb)
                ));
            }
        }
        Some(_) => parts.push("No mounted volumes were reported by Goblins OS.".to_string()),
        None => parts.push("Mounted-volume capacity is waiting for Goblins OS.".to_string()),
    }

    match catalog {
        Some(catalog) => {
            let capacity = catalog
                .hardware
                .model_dir_available_gb
                .map(|gb| format!("{gb}GB free"))
                .unwrap_or_else(|| "free space unknown".to_string());
            parts.push(format!("Model cache has {capacity}."));
        }
        None => match system {
            Some(_system) => {
                parts.push("Model-cache capacity is waiting for Goblins OS.".to_string())
            }
            None => parts.push("Model-cache capacity is waiting for Goblins OS.".to_string()),
        },
    }

    parts.join(" ")
}

fn storage_pressure_plan_detail(
    pressure: &str,
    disk_usage_available: bool,
    disks_available: bool,
    cleanup_controls_available: bool,
    model_cache_reported: bool,
) -> String {
    let lead = match pressure {
        "critical" => "Free space now before downloads, updates, installs, or large file work.",
        "low space" => "Review storage before starting downloads, updates, installs, or local model work.",
        "available" => "No storage pressure is reported. Keep the review tools available for large downloads and disk changes.",
        _ => "Waiting for capacity data. Keep storage changes paused until Goblins OS reports mounted-volume and model-cache space.",
    };
    let disk_usage = if disk_usage_available {
        "Open Disk Usage Analyzer to inspect folders, mounted volumes, and where space is used."
    } else {
        "Disk Usage Analyzer is included in the full Goblins OS image; this session cannot open it yet."
    };
    let disks = if disks_available {
        "Open Disks to inspect drives, partitions, filesystems, and SMART data before changing storage."
    } else {
        "Disks is included in the full Goblins OS image; this session cannot open it yet."
    };
    let cleanup = if cleanup_controls_available {
        "Use the automatic Trash and temporary-file cleanup controls below when you want automatic removal of aged files."
    } else {
        "Automatic Trash and temporary-file cleanup controls are not available in this session."
    };
    let cache = if model_cache_reported {
        "Review model-cache capacity before starting local model downloads."
    } else {
        "Model-cache capacity is still waiting for Goblins OS."
    };

    format!("{lead} {disk_usage} {cleanup} {cache} {disks}")
}

fn storage_cleanup_controls_available(privacy: Option<&PrivacyStatus>) -> bool {
    let Some(desktop) = privacy.and_then(|privacy| privacy.desktop.as_ref()) else {
        return false;
    };

    desktop.gsettings_available
        && desktop.schema_available
        && (desktop.remove_old_trash_files.is_some()
            || desktop.remove_old_temp_files.is_some()
            || desktop.old_files_age_days.is_some())
}

fn most_constrained_storage_volume(volumes: &[StorageVolume]) -> Option<&StorageVolume> {
    volumes.iter().max_by(|left, right| {
        let left_rank =
            storage_pressure_rank(storage_pressure_label(left.total_gb, left.available_gb));
        let right_rank =
            storage_pressure_rank(storage_pressure_label(right.total_gb, right.available_gb));

        left_rank.cmp(&right_rank).then_with(|| {
            storage_used_fraction(left.total_gb, left.available_gb)
                .partial_cmp(&storage_used_fraction(right.total_gb, right.available_gb))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    })
}

fn storage_pressure_rank(label: &str) -> u8 {
    match label {
        "critical" => 3,
        "low space" => 2,
        "available" => 1,
        _ => 0,
    }
}

fn storage_pressure_label_for_rank(rank: u8) -> &'static str {
    match rank {
        3 => "critical",
        2 => "low space",
        1 => "available",
        _ => "unknown",
    }
}

fn storage_pressure_label(total_gb: u64, available_gb: u64) -> &'static str {
    if total_gb == 0 {
        return "unknown";
    }

    let used_fraction = storage_used_fraction(total_gb, available_gb);
    if available_gb <= 5 || used_fraction >= 0.95 {
        "critical"
    } else if available_gb <= 20 || used_fraction >= 0.85 {
        "low space"
    } else {
        "available"
    }
}

fn storage_pressure_ready(total_gb: u64, available_gb: u64) -> bool {
    matches!(storage_pressure_label(total_gb, available_gb), "available")
}

fn storage_used_gb(total_gb: u64, available_gb: u64) -> u64 {
    total_gb.saturating_sub(available_gb.min(total_gb))
}

fn storage_used_fraction(total_gb: u64, available_gb: u64) -> f64 {
    if total_gb == 0 {
        return 0.0;
    }

    (storage_used_gb(total_gb, available_gb) as f64 / total_gb as f64).clamp(0.0, 1.0)
}

fn storage_capacity_percent_text(total_gb: u64, available_gb: u64) -> String {
    format!(
        "{:.0}% used",
        storage_used_fraction(total_gb, available_gb) * 100.0
    )
}

fn storage_capacity_detail(total_gb: u64, available_gb: u64) -> String {
    format!(
        "{}GB used · {}GB free of {}GB",
        storage_used_gb(total_gb, available_gb),
        available_gb.min(total_gb),
        total_gb
    )
}

fn storage_volume_title(volume: &StorageVolume) -> String {
    if volume.mount_point == "/" {
        "System volume".to_string()
    } else if volume.mount_point.contains("/models") || volume.id.contains("model") {
        "Model cache volume".to_string()
    } else {
        "Mounted volume".to_string()
    }
}

fn storage_volume_detail(volume: &StorageVolume) -> String {
    storage_capacity_detail(volume.total_gb, volume.available_gb)
}

fn local_model_ready(model: &LocalModelOption) -> bool {
    matches!(model.install.state.as_str(), "installed")
        || (model.state == "installable" && model.install.state == "not-requested")
}

fn local_model_state_label(model: &LocalModelOption) -> &'static str {
    match model.install.state.as_str() {
        "installed" => "installed",
        "queued" => "queued",
        "downloading" => "downloading",
        "waiting-for-manifest" => "waiting for provider manifest",
        "failed" => "failed",
        _ => match model.state.as_str() {
            "installable" => "ready to download",
            "waiting" => "waiting",
            "blocked" => "blocked",
            _ => "unavailable",
        },
    }
}

fn local_model_action_label(model: &LocalModelOption) -> Option<&'static str> {
    if model.state != "installable" {
        return None;
    }

    match model.install.state.as_str() {
        "not-requested" => Some("Download with consent"),
        "failed" => Some("Retry download"),
        _ => None,
    }
}

fn local_model_install_state_copy(state: &str) -> String {
    match state.trim().to_ascii_lowercase().as_str() {
        "" => "waiting".to_string(),
        "not-requested" => "ready to download".to_string(),
        "waiting-for-manifest" => "waiting for provider manifest".to_string(),
        "failed" => "needs attention".to_string(),
        known @ ("installed" | "queued" | "downloading" | "blocked" | "waiting") => {
            known.to_string()
        }
        _ => settings_status_display_label(state).to_ascii_lowercase(),
    }
}

fn local_model_requirements(model: &LocalModelOption) -> String {
    let gpu = model
        .minimum_gpu_vram_gb
        .map(|vram| format!("{vram}GB VRAM"))
        .unwrap_or_else(|| "no dedicated GPU requirement".to_string());
    let image_policy = if model.weights_in_os_image {
        "weights included in the OS image"
    } else if model.download_required {
        "weights download outside the immutable OS image"
    } else {
        "no weight download required"
    };
    format!(
        "Requires {}GB RAM, {}, and about {}GB free. {}. {}",
        model.minimum_ram_gb,
        gpu,
        model.minimum_free_storage_gb,
        image_policy,
        model.disk_requirement
    )
}

fn local_model_detail(model: &LocalModelOption) -> String {
    let reasons = if model.reasons.is_empty() {
        "No blockers reported.".to_string()
    } else {
        model.reasons.join(" ")
    };
    let consent = match (
        model.install.consent_required,
        model.install.consent_recorded,
    ) {
        (true, true) => "Consent recorded.",
        (true, false) => "Download requires your consent.",
        (false, _) => "No separate consent step required.",
    };
    let manifest = if model.install.manifest_required {
        "Provider manifest required before download."
    } else {
        "Provider manifest present or not required."
    };
    let verification = if model.install.verification_required {
        "SHA-256 verification required."
    } else {
        "No separate verification step required."
    };
    let resumable = if model.install.resumable {
        "Downloads can resume if interrupted."
    } else {
        "Downloads may need to restart if interrupted."
    };
    let install_detail = settings_detail_display_copy(&model.install.detail);
    format!(
        "{} · {} · Status: {}. {} {} {} {} {} {}",
        model.role,
        model.source,
        local_model_install_state_copy(&model.install.state),
        consent,
        manifest,
        verification,
        resumable,
        install_detail,
        reasons
    )
}

fn local_model_download_disclosure(model: &LocalModelOption) -> &'static str {
    if model.install.manifest_required {
        "Records consent now; download waits until a provider manifest with HTTPS URLs, byte counts, and SHA-256 digests is present."
    } else {
        "Records consent and lets Goblins OS queue a resumable, SHA-256 verified model download outside the immutable OS image."
    }
}

fn local_model_install_message(outcome: &LocalModelInstallOutcome) -> String {
    let state = if outcome.ok {
        local_model_install_state_copy(&outcome.state)
    } else {
        "rejected".to_string()
    };
    format!(
        "{} · {} · Status: {}",
        outcome.model_id,
        settings_detail_display_copy(&outcome.detail),
        state
    )
}

struct BluetoothSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn bluetooth_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> BluetoothSummarySpec {
    BluetoothSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn bluetooth_service_summary_spec(bluetooth: Option<&BluetoothStatus>) -> BluetoothSummarySpec {
    let Some(bluetooth) = bluetooth else {
        return bluetooth_summary_spec(
            "Bluetooth",
            "waiting",
            false,
            "Waiting for Bluetooth status.",
        );
    };

    let state = if bluetooth.service_active {
        "active"
    } else if bluetooth.bluez_available {
        "installed"
    } else {
        "missing"
    };

    bluetooth_summary_spec(
        "Bluetooth",
        state,
        bluetooth.service_active,
        bluetooth_service_detail(bluetooth),
    )
}

fn bluetooth_service_detail(bluetooth: &BluetoothStatus) -> String {
    if bluetooth.service_active {
        "Bluetooth is on and ready to manage nearby devices.".to_string()
    } else if bluetooth.bluez_available {
        "Bluetooth support is present but not running.".to_string()
    } else {
        "Bluetooth support is not ready on this device.".to_string()
    }
}

fn bluetooth_adapter_summary_spec(bluetooth: Option<&BluetoothStatus>) -> BluetoothSummarySpec {
    let Some(bluetooth) = bluetooth else {
        return bluetooth_summary_spec("Adapter", "waiting", false, "Waiting for adapter details.");
    };

    match &bluetooth.adapter {
        Some(adapter) => bluetooth_summary_spec(
            "Adapter",
            "default",
            true,
            bluetooth_adapter_detail(adapter),
        ),
        None if bluetooth.adapter_present => bluetooth_summary_spec(
            "Adapter",
            "present",
            true,
            "A Bluetooth adapter is present, but Goblins OS did not report its identity.",
        ),
        None => bluetooth_summary_spec(
            "Adapter",
            "none",
            false,
            "No Bluetooth adapter is connected.",
        ),
    }
}

fn bluetooth_power_summary_spec(bluetooth: Option<&BluetoothStatus>) -> BluetoothSummarySpec {
    let Some(bluetooth) = bluetooth else {
        return bluetooth_summary_spec(
            "Power",
            "waiting",
            false,
            "Waiting for adapter power state.",
        );
    };

    match bluetooth.powered {
        Some(powered) => bluetooth_summary_spec(
            "Power",
            if powered { "on" } else { "off" },
            powered,
            bluetooth_power_detail(powered),
        ),
        None => bluetooth_summary_spec(
            "Power",
            "unknown",
            false,
            "Bluetooth has not reported the current power state.",
        ),
    }
}

fn bluetooth_visibility_summary_spec(bluetooth: Option<&BluetoothStatus>) -> BluetoothSummarySpec {
    let Some(bluetooth) = bluetooth else {
        return bluetooth_summary_spec(
            "Visibility",
            "waiting",
            false,
            "Waiting for Bluetooth discovery and pairing state.",
        );
    };

    let state = match (bluetooth.discoverable, bluetooth.pairable) {
        (Some(true), Some(true)) => "discoverable",
        (Some(true), _) => "visible",
        (Some(false), Some(true)) => "pairable",
        (Some(false), Some(false)) => "hidden",
        _ => "unknown",
    };
    bluetooth_summary_spec(
        "Visibility",
        state,
        bluetooth.discoverable.is_some() || bluetooth.pairable.is_some(),
        bluetooth_adapter_state_detail(
            bluetooth.powered,
            bluetooth.discoverable,
            bluetooth.pairable,
        ),
    )
}

fn bluetooth_adapter_detail(adapter: &BluetoothAdapterStatus) -> String {
    match (&adapter.alias, &adapter.name) {
        (Some(alias), Some(name)) if alias != name => {
            format!("{alias} · {name} · {}", adapter.address)
        }
        (Some(alias), _) => format!("{alias} · {}", adapter.address),
        (_, Some(name)) => format!("{name} · {}", adapter.address),
        _ => adapter.address.clone(),
    }
}

fn bluetooth_adapter_state_detail(
    powered: Option<bool>,
    discoverable: Option<bool>,
    pairable: Option<bool>,
) -> String {
    format!(
        "Power {} · Discovery {} · Pairing {}",
        option_bool_word(powered),
        option_bool_word(discoverable),
        option_bool_word(pairable)
    )
}

fn bluetooth_power_label(powered: bool) -> &'static str {
    if powered {
        "Bluetooth power · on"
    } else {
        "Bluetooth power · off"
    }
}

fn bluetooth_power_detail(powered: bool) -> &'static str {
    if powered {
        "Bluetooth is on. Pairing and device management will appear when secure device actions are available."
    } else {
        "The adapter is powered off. Nearby devices and existing Bluetooth connections are not shown."
    }
}

fn openai_account_detail(auth: &OpenAIAuthStatus) -> String {
    if auth.authenticated {
        format!(
            "{} signed in. Session is held in OS-owned storage.",
            auth.provider
        )
    } else if auth.configured {
        format!(
            "{} configured. Sign in to create an OS-owned session.",
            auth.provider
        )
    } else {
        "No supported provider is configured. Session storage remains OS-owned.".to_string()
    }
}

fn openai_account_summary_spec(
    auth: Option<&OpenAIAuthStatus>,
    system: Option<&SettingsSystemStatus>,
) -> AccountSummarySpec {
    account_summary_spec(
        "OpenAI account",
        overview_account_label(auth, system),
        overview_account_ready(auth, system),
        overview_account_detail(auth, system),
    )
}

fn codex_account_summary_spec(codex: Option<&CodexStatus>) -> AccountSummarySpec {
    match codex {
        Some(codex) if codex.authenticated => account_summary_spec(
            "Codex",
            "signed in",
            true,
            "OpenAI account access is owned by Codex; Settings never receives credentials.",
        ),
        Some(codex) if codex.installed => {
            account_summary_spec("Codex", "sign in", false, &codex.detail)
        }
        Some(codex) => account_summary_spec("Codex", "not included", false, &codex.detail),
        None => account_summary_spec(
            "Codex",
            "waiting",
            false,
            "Waiting for Codex account status.",
        ),
    }
}

fn local_account_summary_spec(system: Option<&SettingsSystemStatus>) -> AccountSummarySpec {
    match system.and_then(|system| system.local_account.as_ref()) {
        Some(account) => account_summary_spec(
            "Local account",
            account.account_type.as_str(),
            true,
            local_account_summary_detail(account),
        ),
        None if system.is_some() => account_summary_spec(
            "Local account",
            "waiting",
            false,
            "Waiting for local account details.",
        ),
        None => account_summary_spec(
            "Local account",
            "waiting",
            false,
            "Waiting for system identity.",
        ),
    }
}

fn local_account_summary_detail(account: &LocalAccountSummary) -> String {
    let display = if account.display_name == account.username {
        account.username.clone()
    } else {
        format!("{} ({})", account.display_name, account.username)
    };
    format!("{} on {}. Home {}", display, account.hostname, account.home)
}

fn local_account_identity_detail(account: &LocalAccountSummary) -> String {
    let user = if account.display_name == account.username {
        account.username.clone()
    } else {
        format!("{} · {}", account.display_name, account.username)
    };
    match (account.uid, account.gid) {
        (Some(uid), Some(gid)) => format!("{user} · uid {uid} · gid {gid}"),
        _ => format!("{user} · uid/gid not reported"),
    }
}

fn local_account_type_detail(account: &LocalAccountSummary) -> String {
    if account.admin_groups.is_empty() {
        format!(
            "{}. Administrator rights are not reported by wheel, sudo, or admin group membership.",
            account.account_type
        )
    } else {
        format!(
            "{} via {}.",
            account.account_type,
            account.admin_groups.join(", ")
        )
    }
}

fn bootc_image_configured(image: &str) -> bool {
    let image = image.trim();
    !image.is_empty() && image != "unconfigured"
}

fn bootc_image_status_label(image: &str) -> &'static str {
    if bootc_image_configured(image) {
        "configured"
    } else {
        "unconfigured"
    }
}

fn bootc_image_detail(system: &SettingsSystemStatus) -> String {
    if bootc_image_configured(&system.services.bootc_image) {
        "Goblins OS has an update image configured for this device.".to_string()
    } else {
        "Goblins OS can run from the installed image, but update image details are not available yet."
            .to_string()
    }
}

fn bootc_update_actions_detail(system: &SettingsSystemStatus) -> String {
    let mut blockers = Vec::new();
    if !bootc_image_configured(&system.services.bootc_image) {
        blockers.push("update image details are not ready");
    }
    if !system.services.bootc_available {
        blockers.push("the update engine is not ready");
    }
    if !system.services.systemctl_available {
        blockers.push("system health checks are not ready");
    }
    if !system.services.network_manager_available {
        blockers.push("networking is not ready");
    }

    if blockers.is_empty() {
        "Read-only for now. Check, apply, and rollback will appear when secure update actions are available.".to_string()
    } else {
        format!(
            "Disabled: {}. Update actions will appear when these checks and secure update actions are available.",
            blockers.join(", ")
        )
    }
}

struct UpdatesAboutSummarySpec {
    title: &'static str,
    state: String,
    ready: bool,
    detail: String,
}

fn updates_about_summary_spec(
    title: &'static str,
    state: impl Into<String>,
    ready: bool,
    detail: impl Into<String>,
) -> UpdatesAboutSummarySpec {
    UpdatesAboutSummarySpec {
        title,
        state: state.into(),
        ready,
        detail: detail.into(),
    }
}

fn boot_image_summary_spec(system: Option<&SettingsSystemStatus>) -> UpdatesAboutSummarySpec {
    match system {
        Some(system) => updates_about_summary_spec(
            "System image",
            bootc_image_status_label(&system.services.bootc_image),
            bootc_image_configured(&system.services.bootc_image),
            bootc_image_detail(system),
        ),
        None => updates_about_summary_spec(
            "System image",
            "waiting",
            false,
            "Waiting for OS image identity.",
        ),
    }
}

fn update_readiness_summary_spec(system: Option<&SettingsSystemStatus>) -> UpdatesAboutSummarySpec {
    let Some(system) = system else {
        return updates_about_summary_spec(
            "Update readiness",
            "waiting",
            false,
            "Waiting for update, system health, and network status.",
        );
    };

    let ready = update_prerequisites_ready(system);
    updates_about_summary_spec(
        "Update readiness",
        if ready { "ready" } else { "blocked" },
        ready,
        update_readiness_detail(system),
    )
}

fn desktop_session_summary_spec(system: Option<&SettingsSystemStatus>) -> UpdatesAboutSummarySpec {
    let Some(system) = system else {
        return updates_about_summary_spec(
            "Desktop",
            "waiting",
            false,
            "Waiting for desktop status.",
        );
    };

    let ready = configured_runtime_value(&system.session.desktop)
        && configured_runtime_value(&system.session.gui_platform)
        && configured_runtime_value(&system.session.shell_mode);
    updates_about_summary_spec(
        "Desktop",
        if ready { "configured" } else { "unconfigured" },
        ready,
        desktop_session_detail(system),
    )
}

fn device_identity_summary_spec(system: Option<&SettingsSystemStatus>) -> UpdatesAboutSummarySpec {
    let Some(system) = system else {
        return updates_about_summary_spec(
            "Device identity",
            "waiting",
            false,
            "Waiting for local account and device identity.",
        );
    };

    match &system.local_account {
        Some(account) => {
            let ready = local_account_identity_ready(account);
            updates_about_summary_spec(
                "Device identity",
                if ready {
                    account.account_type.as_str()
                } else {
                    "unknown"
                },
                ready,
                if ready {
                    format!(
                        "{} on {} · {} account.",
                        account.display_name, account.hostname, account.account_type
                    )
                } else {
                    "Local account and device identity are still being read.".to_string()
                },
            )
        }
        None => updates_about_summary_spec(
            "Device identity",
            "read-only",
            false,
            "Device identity is not available yet.",
        ),
    }
}

fn local_account_identity_ready(account: &LocalAccountSummary) -> bool {
    configured_runtime_value(&account.username)
        && configured_runtime_value(&account.display_name)
        && configured_runtime_value(&account.hostname)
        && !account.account_type.eq_ignore_ascii_case("unknown")
}

fn update_prerequisites_ready(system: &SettingsSystemStatus) -> bool {
    bootc_image_configured(&system.services.bootc_image)
        && system.services.bootc_available
        && system.services.systemctl_available
        && system.services.network_manager_available
}

fn update_readiness_detail(system: &SettingsSystemStatus) -> String {
    if update_prerequisites_ready(system) {
        "The update system is ready. Check, apply, and rollback will appear when secure update actions are available.".to_string()
    } else {
        bootc_update_actions_detail(system)
    }
}

fn desktop_session_detail(system: &SettingsSystemStatus) -> String {
    let ready = [
        system.session.desktop.as_str(),
        system.session.gui_platform.as_str(),
        system.session.shell_mode.as_str(),
    ]
    .into_iter()
    .all(|value| {
        let value = value.trim();
        !value.is_empty() && value != "unconfigured"
    });

    if ready {
        "Goblins OS desktop is active.".to_string()
    } else {
        "Desktop status is not available yet.".to_string()
    }
}

fn normalized_unit_speed(speed: f64) -> f64 {
    if !speed.is_finite() {
        return 0.0;
    }
    ((speed.clamp(-1.0, 1.0) * 20.0).round() / 20.0 * 100.0).round() / 100.0
}

fn pointer_speed_label(speed: f64) -> String {
    let speed = normalized_unit_speed(speed);
    if speed.abs() < 0.001 {
        "Default".to_string()
    } else if speed > 0.0 {
        format!("Faster {:.0}%", speed * 100.0)
    } else {
        format!("Slower {:.0}%", speed.abs() * 100.0)
    }
}

fn normalized_keyboard_delay(delay: u32) -> u32 {
    round_to_step(delay.clamp(150, 1000), 25)
}

fn normalized_keyboard_repeat_interval(interval: u32) -> u32 {
    round_to_step(interval.clamp(15, 120), 5)
}

fn normalized_keyboard_delay_slider(value: f64) -> f64 {
    f64::from(normalized_keyboard_delay(value.round() as u32))
}

fn normalized_keyboard_repeat_interval_slider(value: f64) -> f64 {
    f64::from(normalized_keyboard_repeat_interval(value.round() as u32))
}

fn round_to_step(value: u32, step: u32) -> u32 {
    ((value + (step / 2)) / step) * step
}

fn milliseconds_label(value: f64) -> String {
    format!("{} ms", value.round() as u32)
}

fn normalized_old_files_age(days: u32) -> u32 {
    days.clamp(1, 365)
}

fn normalized_old_files_age_slider(value: f64) -> f64 {
    f64::from(normalized_old_files_age(value.round() as u32))
}

fn days_label(value: f64) -> String {
    let days = normalized_old_files_age(value.round() as u32);
    if days == 1 {
        "1 day".to_string()
    } else {
        format!("{days} days")
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
struct SliderSpec<'a> {
    title: &'a str,
    detail: &'a str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
struct ChoiceOption<'a> {
    id: &'a str,
    label: &'a str,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const BACKGROUND_PICTURE_OPTIONS: &[ChoiceOption<'static>] = &[
    ChoiceOption {
        id: "zoom",
        label: "Fill",
    },
    ChoiceOption {
        id: "scaled",
        label: "Fit",
    },
    ChoiceOption {
        id: "centered",
        label: "Center",
    },
    ChoiceOption {
        id: "stretched",
        label: "Stretch",
    },
    ChoiceOption {
        id: "wallpaper",
        label: "Tile",
    },
    ChoiceOption {
        id: "spanned",
        label: "Span",
    },
    ChoiceOption {
        id: "none",
        label: "None",
    },
];

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const BACKGROUND_SHADING_OPTIONS: &[ChoiceOption<'static>] = &[
    ChoiceOption {
        id: "solid",
        label: "Solid",
    },
    ChoiceOption {
        id: "horizontal",
        label: "Horizontal",
    },
    ChoiceOption {
        id: "vertical",
        label: "Vertical",
    },
];

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const PROXY_MODE_OPTIONS: &[ChoiceOption<'static>] = &[
    ChoiceOption {
        id: "none",
        label: "Off",
    },
    ChoiceOption {
        id: "auto",
        label: "Automatic",
    },
    ChoiceOption {
        id: "manual",
        label: "Manual",
    },
];

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn slider_row(
    spec: SliderSpec<'_>,
    format_value: impl Fn(f64) -> String + 'static,
    normalize: impl Fn(f64) -> f64 + 'static,
    on_change: impl Fn(f64) -> Result<String, String> + 'static,
) -> gtk4::Box {
    use gtk4::prelude::*;

    let value = normalize(spec.value);
    let title = spec.title.to_string();
    let detail_text = settings_detail_display_copy(spec.detail);
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 18);
    row.add_css_class("gos-row");
    row.add_css_class("gos-slider-row");
    row.set_valign(gtk4::Align::Center);
    set_accessible_label_description(&row, &title, &detail_text);

    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    copy.set_hexpand(true);
    let title_label = label(spec.title, &["gos-row-title"]);
    copy.append(&title_label);
    let detail_label = label(&detail_text, &["gos-row-copy"]);
    copy.append(&detail_label);

    let control = gtk4::Box::new(gtk4::Orientation::Vertical, 5);
    control.add_css_class("gos-slider-control");
    control.set_width_request(320);
    let value_label = label(&format_value(value), &["gos-row-value"]);
    value_label.set_xalign(1.0);
    value_label.set_wrap(false);
    control.append(&value_label);

    let slider =
        gtk4::Scale::with_range(gtk4::Orientation::Horizontal, spec.min, spec.max, spec.step);
    slider.set_draw_value(false);
    slider.set_hexpand(true);
    slider.set_value(value);
    slider.set_tooltip_text(Some(spec.title));
    let value_text = format_value(value);
    slider.update_property(&[
        gtk4::accessible::Property::Label(spec.title),
        gtk4::accessible::Property::Description(&detail_text),
        gtk4::accessible::Property::ValueMin(spec.min),
        gtk4::accessible::Property::ValueMax(spec.max),
        gtk4::accessible::Property::ValueNow(value),
        gtk4::accessible::Property::ValueText(&value_text),
    ]);
    control.append(&slider);

    row.append(&copy);
    row.append(&control);

    let current_value = Rc::new(Cell::new(value));
    let updating_slider = Rc::new(Cell::new(false));
    {
        let row_accessibility = row.clone();
        let value_label = value_label.clone();
        let detail_label = detail_label.clone();
        let current_value = current_value.clone();
        let updating_slider = updating_slider.clone();
        let detail_text = detail_text.clone();
        slider.connect_value_changed(move |slider| {
            if updating_slider.get() {
                return;
            }
            let next_value = normalize(slider.value());
            if (next_value - current_value.get()).abs() < 0.001 {
                return;
            }

            slider.set_sensitive(false);
            match on_change(next_value) {
                Ok(success_detail) => {
                    current_value.set(next_value);
                    let value_text = format_value(next_value);
                    let next_detail = if success_detail.trim().is_empty() {
                        detail_text.clone()
                    } else {
                        settings_detail_display_copy(&success_detail)
                    };
                    value_label.set_text(&value_text);
                    detail_label.set_text(&next_detail);
                    set_accessible_label_description(&row_accessibility, &title, &next_detail);
                    slider.update_property(&[
                        gtk4::accessible::Property::ValueNow(next_value),
                        gtk4::accessible::Property::ValueText(&value_text),
                        gtk4::accessible::Property::Description(&next_detail),
                    ]);
                }
                Err(error) => {
                    let error_detail =
                        settings_detail_display_copy(&setting_change_rejected_detail(&error));
                    detail_label.set_text(&error_detail);
                    set_accessible_label_description(&row_accessibility, &title, &error_detail);
                    slider
                        .update_property(&[gtk4::accessible::Property::Description(&error_detail)]);
                    eprintln!("settings_control_change_rejected title={title:?} error={error:?}");
                    updating_slider.set(true);
                    slider.set_value(current_value.get());
                    updating_slider.set(false);
                }
            }
            slider.set_sensitive(true);
        });
    }

    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn choice_row(
    title: &str,
    value: &str,
    options: &[ChoiceOption<'_>],
    detail_for_value: impl Fn(&str) -> String + 'static,
    on_change: impl Fn(&str) -> Result<String, String> + 'static,
) -> gtk4::Box {
    use gtk4::prelude::*;

    let current_value = value.to_string();
    let initial_detail = settings_detail_display_copy(&detail_for_value(&current_value));
    let title_text = title.to_string();
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
    row.add_css_class("gos-row");
    row.add_css_class("gos-choice-row");
    set_accessible_label_description(&row, title, &initial_detail);

    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    copy.set_hexpand(true);
    copy.append(&label(title, &["gos-row-title"]));
    let detail = label(&initial_detail, &["gos-row-copy"]);
    copy.append(&detail);
    row.append(&copy);

    let control = gtk4::ComboBoxText::new();
    control.add_css_class("gos-choice-control");
    control.set_valign(gtk4::Align::Center);
    control.set_width_request(188);
    control.set_tooltip_text(Some(title));
    for option in options {
        control.append(Some(option.id), option.label);
    }
    let _ = control.set_active_id(Some(&current_value));
    set_accessible_label_description(&control, title, &initial_detail);

    let current_value = Rc::new(RefCell::new(current_value));
    let updating_control = Rc::new(Cell::new(false));
    {
        let row_accessibility = row.clone();
        let current_value = current_value.clone();
        let updating_control = updating_control.clone();
        let detail = detail.clone();
        control.connect_changed(move |control| {
            if updating_control.get() {
                return;
            }
            let Some(next_id) = control.active_id() else {
                return;
            };
            let next_value = next_id.to_string();
            if current_value.borrow().as_str() == next_value.as_str() {
                return;
            }

            control.set_sensitive(false);
            match on_change(&next_value) {
                Ok(success_detail) => {
                    *current_value.borrow_mut() = next_value.clone();
                    let next_detail = if success_detail.trim().is_empty() {
                        settings_detail_display_copy(&detail_for_value(&next_value))
                    } else {
                        settings_detail_display_copy(&success_detail)
                    };
                    detail.set_text(&next_detail);
                    set_accessible_label_description(&row_accessibility, &title_text, &next_detail);
                    control
                        .update_property(&[gtk4::accessible::Property::Description(&next_detail)]);
                }
                Err(error) => {
                    let error_detail =
                        settings_detail_display_copy(&setting_change_rejected_detail(&error));
                    detail.set_text(&error_detail);
                    set_accessible_label_description(
                        &row_accessibility,
                        &title_text,
                        &error_detail,
                    );
                    control
                        .update_property(&[gtk4::accessible::Property::Description(&error_detail)]);
                    eprintln!(
                        "settings_control_change_rejected title={title_text:?} error={error:?}"
                    );
                    let previous_value = current_value.borrow().clone();
                    updating_control.set(true);
                    let _ = control.set_active_id(Some(&previous_value));
                    updating_control.set(false);
                }
            }
            control.set_sensitive(true);
        });
    }

    row.append(&control);
    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn switch_row_dynamic(
    title: &str,
    active: bool,
    sensitive: bool,
    detail_for_state: impl Fn(bool) -> String + 'static,
    on_change: impl Fn(bool) -> Result<String, String> + 'static,
) -> gtk4::Box {
    use gtk4::prelude::*;

    let title_text = title.to_string();
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
    row.add_css_class("gos-row");
    row.add_css_class("gos-switch-row");

    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    copy.set_hexpand(true);
    copy.append(&label(title, &["gos-row-title"]));
    let initial_detail = settings_detail_display_copy(&detail_for_state(active));
    set_accessible_label_description(&row, title, &initial_detail);
    let detail = label(&initial_detail, &["gos-row-copy"]);
    copy.append(&detail);
    row.append(&copy);

    let toggle = gtk4::Switch::new();
    toggle.set_active(active);
    toggle.set_sensitive(sensitive);
    toggle.set_valign(gtk4::Align::Center);
    toggle.set_tooltip_text(Some(title));
    set_accessible_label_description(&toggle, title, &initial_detail);
    let current_active = Rc::new(Cell::new(active));
    let updating_switch = Rc::new(Cell::new(false));
    {
        let row_accessibility = row.clone();
        let current_active = current_active.clone();
        let updating_switch = updating_switch.clone();
        let detail = detail.clone();
        toggle.connect_active_notify(move |toggle| {
            if updating_switch.get() {
                return;
            }
            let next_active = toggle.is_active();
            if next_active == current_active.get() {
                return;
            }

            toggle.set_sensitive(false);
            match on_change(next_active) {
                Ok(success_detail) => {
                    current_active.set(next_active);
                    let next_detail = if success_detail.trim().is_empty() {
                        settings_detail_display_copy(&detail_for_state(next_active))
                    } else {
                        settings_detail_display_copy(&success_detail)
                    };
                    detail.set_text(&next_detail);
                    set_accessible_label_description(&row_accessibility, &title_text, &next_detail);
                    toggle
                        .update_property(&[gtk4::accessible::Property::Description(&next_detail)]);
                }
                Err(error) => {
                    let error_detail =
                        settings_detail_display_copy(&setting_change_rejected_detail(&error));
                    detail.set_text(&error_detail);
                    set_accessible_label_description(
                        &row_accessibility,
                        &title_text,
                        &error_detail,
                    );
                    toggle
                        .update_property(&[gtk4::accessible::Property::Description(&error_detail)]);
                    eprintln!(
                        "settings_control_change_rejected title={title_text:?} error={error:?}"
                    );
                    updating_switch.set(true);
                    toggle.set_active(current_active.get());
                    updating_switch.set(false);
                }
            }
            toggle.set_sensitive(true);
        });
    }
    row.append(&toggle);
    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn ready_word(available: bool) -> &'static str {
    if available {
        "ready"
    } else {
        "not ready"
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn system_row(title: &str, detail: &str) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    row.add_css_class("gos-row");
    row.add_css_class("gos-system-row");
    let display_detail = settings_detail_display_copy(detail);
    set_accessible_label_description(&row, title, &display_detail);
    row.append(&label(title, &["gos-row-title"]));
    row.append(&label(&display_detail, &["gos-row-copy"]));
    row
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_preference_group(panel: &gtk4::Box, title: &str, rows: Vec<gtk4::Box>) {
    use gtk4::prelude::*;

    if rows.is_empty() {
        return;
    }

    panel.append(&label(title, &["gos-subsection-title"]));
    let group = gtk4::Grid::new();
    group.add_css_class("gos-preference-group");
    group.set_column_homogeneous(false);
    group.set_row_spacing(0);
    group.set_column_spacing(0);
    group.set_hexpand(true);

    for (index, row) in rows.into_iter().enumerate() {
        row.set_hexpand(true);
        group.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&group);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_context_grid(
    settings_panel: SettingsPanel,
    readiness: DeviceSettingsReadiness,
) -> gtk4::Grid {
    use gtk4::prelude::*;

    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-device-context-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);

    let summary_detail = device_settings_summary_detail(settings_panel, readiness);
    let summary = device_settings_context_tile(
        "Control access",
        &summary_detail,
        Some((
            device_settings_status_label(readiness),
            readiness.is_ready(),
        )),
    );
    grid.attach(&summary, 0, 0, 1, 1);

    grid
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_summary_detail(
    settings_panel: SettingsPanel,
    readiness: DeviceSettingsReadiness,
) -> String {
    let panel_name = settings_panel.display_name();
    match readiness {
        DeviceSettingsReadiness::Ready => {
            format!("{panel_name} controls open from this Settings pane.")
        }
        DeviceSettingsReadiness::IntegratedDesktopUnavailable => {
            format!("{panel_name} controls open after the desktop finishes loading.")
        }
        DeviceSettingsReadiness::WaitingForSession => {
            format!("Checking {panel_name} controls.")
        }
        DeviceSettingsReadiness::Unavailable => {
            format!("{panel_name} controls are not supported on this device.")
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_context_tile(
    title: &str,
    detail: &str,
    status: Option<(&'static str, bool)>,
) -> gtk4::Box {
    use gtk4::prelude::*;

    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    row.add_css_class("gos-row");
    row.add_css_class("gos-device-context-tile");
    row.set_hexpand(true);
    let display_detail = settings_detail_display_copy(detail);
    set_accessible_label_description(&row, title, &display_detail);

    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let title_label = label(title, &["gos-row-title"]);
    title_label.set_hexpand(true);
    title_label.set_xalign(0.0);
    head.append(&title_label);
    if let Some((text, ready)) = status {
        let pill = settings_status_pill(text, ready);
        pill.set_halign(gtk4::Align::End);
        head.append(&pill);
    }
    row.append(&head);
    row.append(&label(&display_detail, &["gos-row-copy"]));
    row
}

#[derive(Clone, Copy)]
struct DeviceSettingsCapability {
    title: &'static str,
    detail: &'static str,
}

const APPLICATION_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Default apps",
        detail: "Default handlers cover web, mail, calendar, music, video, photos, and file links.",
    },
    DeviceSettingsCapability {
        title: "App permissions",
        detail: "Application permissions open the right privacy controls for each app.",
    },
    DeviceSettingsCapability {
        title: "Installed apps",
        detail:
            "Software handles installed application review, removal, software sources, and updates.",
    },
    DeviceSettingsCapability {
        title: "Launch behavior",
        detail:
            "Startup, background, and file-opening behavior follow the running desktop session.",
    },
];

// DesktopDock and MenuBarControlCenter now have bespoke read-only builders
// (build_desktop_dock / build_menu_bar_control_center) that report live shell
// status, so the old static capability tables were removed rather than left as
// dead code.

const NETWORK_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Wi-Fi",
        detail:
            "Saved networks, passwords, connection security, and roaming preferences stay private.",
    },
    DeviceSettingsCapability {
        title: "Known networks",
        detail:
            "Forget, reconnect, metered-network, auto-connect, IPv4, IPv6, DNS, and route details are available.",
    },
    DeviceSettingsCapability {
        title: "Proxy",
        detail:
            "Proxy mode, PAC URLs, and manual proxy settings follow the current network configuration.",
    },
    DeviceSettingsCapability {
        title: "Captive portals",
        detail:
            "Portal detection and connection repair remain part of Network settings.",
    },
];

const NETWORK_SERVICE_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Wired Ethernet",
        detail: "Wired connection profiles include device identity, IPv4, IPv6, DNS, and routes.",
    },
    DeviceSettingsCapability {
        title: "VPN",
        detail: "VPN providers, imported profiles, authentication, and connection state are available.",
    },
    DeviceSettingsCapability {
        title: "Advanced profiles",
        detail: "Connection priority, metered state, MAC handling, and advanced network fields are available.",
    },
    DeviceSettingsCapability {
        title: "Connection recovery",
        detail: "Reconnect, forget, and profile repair actions are available from connection details.",
    },
];

const BLUETOOTH_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Adapter power",
        detail: "Power, visibility, and radio availability are shown before device actions appear.",
    },
    DeviceSettingsCapability {
        title: "Pairing",
        detail: "Pair, trust, connect, disconnect, forget, and passkey actions are available when supported.",
    },
    DeviceSettingsCapability {
        title: "Devices",
        detail: "Device class, battery, connection status, and supported features are shown for connected hardware.",
    },
    DeviceSettingsCapability {
        title: "Hardware limits",
        detail: "Sessions without an adapter show that state before offering device actions.",
    },
];

const MOBILE_BROADBAND_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Cellular modem",
        detail: "Modem services handle discovery, SIM status, carrier registration, and data connection state.",
    },
    DeviceSettingsCapability {
        title: "APN settings",
        detail: "Access-point names, provider profiles, and data-authentication settings use modem services.",
    },
    DeviceSettingsCapability {
        title: "Roaming",
        detail: "Roaming and mobile-data choices stay with system services.",
    },
    DeviceSettingsCapability {
        title: "Hardware limits",
        detail: "If no modem exists, Settings shows that state before offering controls.",
    },
];

const SHARING_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Device name",
        detail: "Sharing services use the visible device name for discovery and network services.",
    },
    DeviceSettingsCapability {
        title: "Remote desktop",
        detail: "Remote desktop, screen sharing, authentication, and service availability use sharing services.",
    },
    DeviceSettingsCapability {
        title: "File sharing",
        detail: "Personal file sharing and related service controls are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Media sharing",
        detail: "Shared media libraries and service visibility are exposed only when the desktop supports them.",
    },
];

const DISPLAY_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Resolution",
        detail: "Resolution, refresh rate, rotation, fractional scale, and monitor arrangement stay with Display settings.",
    },
    DeviceSettingsCapability {
        title: "Night Light",
        detail: "Color temperature, scheduling, and sunset/sunrise behavior stay with the desktop display service.",
    },
    DeviceSettingsCapability {
        title: "Multiple displays",
        detail: "Mirror, join, primary-display, and per-monitor scale choices are managed as display settings.",
    },
    DeviceSettingsCapability {
        title: "Hardware evidence",
        detail: "Only connected screens reported by the device appear as configurable.",
    },
];

const COLOR_MANAGEMENT_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Display profiles",
        detail: "Color services handle ICC profile selection for displays and color-managed output.",
    },
    DeviceSettingsCapability {
        title: "Device profiles",
        detail: "Printers, scanners, cameras, and other color-capable devices keep their color profiles.",
    },
    DeviceSettingsCapability {
        title: "Calibration",
        detail: "Calibration tools and profile import actions are available when the color service supports them.",
    },
    DeviceSettingsCapability {
        title: "Profile evidence",
        detail: "Installed and active profiles come from the running color service.",
    },
];

const SOUND_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Output",
        detail: "Audio services handle output device selection, ports, device volumes, balance, and mute state.",
    },
    DeviceSettingsCapability {
        title: "Input",
        detail: "Microphone devices, input gain, mute state, and live input levels stay system-backed.",
    },
    DeviceSettingsCapability {
        title: "Alerts",
        detail: "Alert sound, event sounds, and system sound behavior remain desktop-managed.",
    },
    DeviceSettingsCapability {
        title: "Routing",
        detail: "Per-device and hardware routing choices follow system audio routing instead of a duplicate mixer.",
    },
];

const KEYBOARD_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Shortcuts",
        detail:
            "Input services handle global shortcuts, custom shortcuts, compose key, and special-key behavior.",
    },
    DeviceSettingsCapability {
        title: "Input sources",
        detail:
            "Keyboard layouts, input methods, language switching, and source ordering are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Typing",
        detail: "Repeat, cursor blink, and text-entry defaults remain standard desktop settings.",
    },
    DeviceSettingsCapability {
        title: "Hardware",
        detail:
            "Keyboard-device availability and special hardware keys come from the running session.",
    },
];

const MOUSE_TRACKPAD_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Pointer",
        detail: "Input services handle pointer speed, acceleration, primary button, and device-specific pointer behavior.",
    },
    DeviceSettingsCapability {
        title: "Trackpad",
        detail: "Tap-to-click, natural scrolling, edge scrolling, and click method follow device input settings.",
    },
    DeviceSettingsCapability {
        title: "Gestures",
        detail: "Desktop gestures stay session-managed so touchpad behavior matches the live session.",
    },
    DeviceSettingsCapability {
        title: "Device discovery",
        detail: "Only connected mouse, touchpad, and pointing devices expose hardware-specific options.",
    },
];

const DRAWING_TABLET_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Stylus mapping",
        detail: "Input services handle tablet-to-display mapping, stylus assignment, and orientation.",
    },
    DeviceSettingsCapability {
        title: "Pressure",
        detail: "Pressure curves and pen behavior stay with the system tablet stack.",
    },
    DeviceSettingsCapability {
        title: "Buttons",
        detail: "Tablet buttons, stylus buttons, and pad shortcuts use tablet input settings.",
    },
    DeviceSettingsCapability {
        title: "Hardware discovery",
        detail: "If no tablet is present, Settings shows that state instead of presenting inactive controls.",
    },
];

const ACCESSIBILITY_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Vision",
        detail: "Screen reader, zoom, contrast, cursor size, and visual assistance stay system-backed.",
    },
    DeviceSettingsCapability {
        title: "Hearing",
        detail: "Visual alerts, mono audio, and related hearing options use accessibility settings.",
    },
    DeviceSettingsCapability {
        title: "Typing and pointing",
        detail: "Sticky keys, slow keys, bounce keys, on-screen keyboard, and click assistance are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Assistive technology",
        detail: "Session accessibility support remains owned by the desktop accessibility stack.",
    },
];

const DESKTOP_WALLPAPER_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Wallpaper picker",
        detail: "The background library, file picker, and image selection flow are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Light and dark",
        detail: "Separate light/dark backgrounds and system-theme matching stay desktop-managed.",
    },
    DeviceSettingsCapability {
        title: "Placement",
        detail: "Zoom, centered, scaled, spanned, and wallpaper-placement behavior follow wallpaper settings.",
    },
    DeviceSettingsCapability {
        title: "Desktop identity",
        detail: "Wallpaper choices stay consistent with the running desktop session.",
    },
];

const NOTIFICATIONS_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Per-app alerts",
        detail: "Per-application banners, sounds, and notification visibility are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Lock screen",
        detail: "Lock-screen notification exposure stays with the desktop privacy and notification stack.",
    },
    DeviceSettingsCapability {
        title: "Do Not Disturb",
        detail: "Quiet mode and banner interruption behavior remain session-managed.",
    },
    DeviceSettingsCapability {
        title: "Notification history",
        detail: "Notification retention and clear behavior follow the running desktop session.",
    },
];

const LOCK_SCREEN_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Screen lock",
        detail: "Screen locking, blanking, and sign-in recovery stay tied to desktop privacy and power settings.",
    },
    DeviceSettingsCapability {
        title: "Lock-screen notifications",
        detail: "Notification visibility on the lock screen follows notification privacy preferences.",
    },
    DeviceSettingsCapability {
        title: "Privacy",
        detail: "Sensitive previews and account details stay hidden unless the protected setting allows them.",
    },
    DeviceSettingsCapability {
        title: "Recovery",
        detail: "Locked-session recovery uses OS-owned session state and does not expose credentials.",
    },
];

const SEARCH_INDEXING_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Search providers",
        detail: "Desktop provider visibility and search-result sources are available in settings.",
    },
    DeviceSettingsCapability {
        title: "File indexing",
        detail: "Indexed folders, excluded folders, and result visibility follow search settings.",
    },
    DeviceSettingsCapability {
        title: "Privacy",
        detail:
            "Search history and provider exposure are handled by the desktop privacy/search stack.",
    },
    DeviceSettingsCapability {
        title: "Result behavior",
        detail: "Search ranking and provider availability stay tied to the running desktop.",
    },
];

const MULTITASKING_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Workspaces",
        detail: "Workspace behavior, dynamic workspace creation, and multi-monitor workspace behavior are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Hot corner",
        detail: "Hot-corner behavior is controlled by the running desktop session.",
    },
    DeviceSettingsCapability {
        title: "Window switching",
        detail: "App switching, window switching, and related keyboard behavior follow desktop settings.",
    },
    DeviceSettingsCapability {
        title: "Window focus",
        detail: "Focus and workspace conventions stay with the window system.",
    },
];

const POWER_BATTERY_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Power mode",
        detail: "Power services provide balanced, power-saver, and performance-mode availability from the real hardware stack.",
    },
    DeviceSettingsCapability {
        title: "Battery",
        detail: "Battery health, charge state, and remaining-time estimates come from system power services.",
    },
    DeviceSettingsCapability {
        title: "Suspend",
        detail: "Automatic suspend, screen blanking, and lid behavior follow power settings.",
    },
    DeviceSettingsCapability {
        title: "Hardware limits",
        detail: "Desktop, laptop, and VM sessions expose only the power controls the device actually supports.",
    },
];

const PRINTERS_SCANNERS_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Printers",
        detail: "Printer discovery, add/remove flows, defaults, and driver-backed configuration are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Queues",
        detail: "Print queues, pause/resume, job cancellation, and device errors stay with print tools.",
    },
    DeviceSettingsCapability {
        title: "Scanners",
        detail: "Scanner discovery and supported scan devices remain tied to the system hardware stack.",
    },
    DeviceSettingsCapability {
        title: "Administration",
        detail: "Privileged printer administration opens only through approved system prompts.",
    },
];

const DATE_TIME_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Clock",
        detail:
            "Clock display, automatic time, and network-time status follow date and time services.",
    },
    DeviceSettingsCapability {
        title: "Time zone",
        detail:
            "Time-zone selection and location-based time settings stay with system date controls.",
    },
    DeviceSettingsCapability {
        title: "Calendar",
        detail: "Calendar and clock format behavior follows region and date settings.",
    },
    DeviceSettingsCapability {
        title: "Hardware clock",
        detail: "Privileged hardware-clock changes open only through approved system controls.",
    },
];

const LANGUAGE_REGION_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Language",
        detail: "Display language and fallback language choices follow region and language services.",
    },
    DeviceSettingsCapability {
        title: "Formats",
        detail: "Dates, numbers, measurement units, and calendar formats stay with region settings.",
    },
    DeviceSettingsCapability {
        title: "Input language",
        detail: "Keyboard input sources remain linked to language and keyboard settings.",
    },
    DeviceSettingsCapability {
        title: "Restart boundary",
        detail: "Changes that require sign-out or restart are handled by the system controls that own them.",
    },
];

const USERS_ACCOUNTS_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Local users",
        detail: "Local user creation, deletion, account type, and avatar changes are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Passwords",
        detail: "Password changes, account unlock, and administrator prompts use secure account actions.",
    },
    DeviceSettingsCapability {
        title: "Administrators",
        detail: "Administrator rights and local account privileges follow device policy.",
    },
    DeviceSettingsCapability {
        title: "Device identity",
        detail: "Computer name and visible local identity stay with device settings.",
    },
];

const ONLINE_ACCOUNTS_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Account providers",
        detail: "Account services provide supported providers and their authentication flows.",
    },
    DeviceSettingsCapability {
        title: "Integrated services",
        detail:
            "Mail, calendar, contacts, files, and other desktop integrations stay provider-managed.",
    },
    DeviceSettingsCapability {
        title: "Token storage",
        detail: "Provider credentials and refresh tokens stay in the account service keyring.",
    },
    DeviceSettingsCapability {
        title: "Account removal",
        detail: "Removal, re-authentication, and per-service enablement are handled by account controls.",
    },
];

const PRIVACY_PERMISSIONS_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Device access",
        detail: "Camera, microphone, location, and USB/privacy-device access stay system permission surfaces.",
    },
    DeviceSettingsCapability {
        title: "File history",
        detail: "Recent files, trash, temporary files, and history retention remain desktop privacy settings.",
    },
    DeviceSettingsCapability {
        title: "Screen access",
        detail: "Screen sharing, screenshots, and screen access use protected permission prompts.",
    },
    DeviceSettingsCapability {
        title: "Secrets",
        detail: "Keyring and credential behavior stay outside the Settings UI unless a real protected route exists.",
    },
];

// Security now has a bespoke read-only builder (build_security) backed by the
// live policy + hardware facility data, so its static capability table was
// removed rather than left as dead code.

const WELLBEING_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "Screen time",
        detail: "Screen-time summaries appear when the desktop session reports them.",
    },
    DeviceSettingsCapability {
        title: "Break reminders",
        detail:
            "Rest reminders, focus breaks, and attention-health prompts follow wellbeing settings.",
    },
    DeviceSettingsCapability {
        title: "Usage controls",
        detail:
            "Per-app or session usage controls appear only when the desktop exposes real support.",
    },
    DeviceSettingsCapability {
        title: "Privacy",
        detail: "Wellbeing data stays desktop-owned and appears only when the desktop reports it.",
    },
];

const UPDATES_ABOUT_CAPABILITIES: [DeviceSettingsCapability; 4] = [
    DeviceSettingsCapability {
        title: "About",
        detail: "About, device name, and OS details are available in settings.",
    },
    DeviceSettingsCapability {
        title: "Software",
        detail: "Installed application and package update flows stay with Software when available.",
    },
    DeviceSettingsCapability {
        title: "Hardware",
        detail: "Processor, memory, graphics, disk, and machine identity come from system-reported facts.",
    },
    DeviceSettingsCapability {
        title: "OS updates",
        detail: "Update checks, apply, and rollback appear only when the device can perform them safely.",
    },
];

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_device_native_coverage(panel: &gtk4::Box, settings_panel: SettingsPanel) {
    use gtk4::prelude::*;

    let capabilities = device_settings_capabilities(settings_panel);
    if capabilities.is_empty() {
        return;
    }

    panel.append(&label("Settings covered", &["gos-subsection-title"]));
    let grid = gtk4::Grid::new();
    grid.add_css_class("gos-device-capability-grid");
    grid.add_css_class("gos-preference-group");
    grid.set_column_homogeneous(false);
    grid.set_row_spacing(0);
    grid.set_column_spacing(0);
    grid.set_hexpand(true);
    let coverage_detail = device_settings_configuration_source(settings_panel);
    set_accessible_label_description(&grid, "Settings covered", &coverage_detail);

    for (index, capability) in capabilities.iter().enumerate() {
        let row = system_row(capability.title, capability.detail);
        row.add_css_class("gos-device-capability-tile");
        row.set_hexpand(true);
        grid.attach(&row, 0, index as i32, 1, 1);
    }

    panel.append(&grid);
}

fn device_settings_capabilities(
    settings_panel: SettingsPanel,
) -> &'static [DeviceSettingsCapability] {
    match settings_panel {
        SettingsPanel::Applications => &APPLICATION_CAPABILITIES,
        SettingsPanel::Network => &NETWORK_CAPABILITIES,
        SettingsPanel::NetworkServices => &NETWORK_SERVICE_CAPABILITIES,
        SettingsPanel::Bluetooth => &BLUETOOTH_CAPABILITIES,
        SettingsPanel::MobileBroadband => &MOBILE_BROADBAND_CAPABILITIES,
        SettingsPanel::Sharing => &SHARING_CAPABILITIES,
        SettingsPanel::Displays => &DISPLAY_CAPABILITIES,
        SettingsPanel::ColorManagement => &COLOR_MANAGEMENT_CAPABILITIES,
        SettingsPanel::Sound => &SOUND_CAPABILITIES,
        SettingsPanel::Keyboard => &KEYBOARD_CAPABILITIES,
        SettingsPanel::MouseTrackpad => &MOUSE_TRACKPAD_CAPABILITIES,
        SettingsPanel::DrawingTablet => &DRAWING_TABLET_CAPABILITIES,
        SettingsPanel::Accessibility => &ACCESSIBILITY_CAPABILITIES,
        SettingsPanel::DesktopWallpaper => &DESKTOP_WALLPAPER_CAPABILITIES,
        SettingsPanel::Notifications => &NOTIFICATIONS_CAPABILITIES,
        SettingsPanel::SearchIndexing => &SEARCH_INDEXING_CAPABILITIES,
        SettingsPanel::Multitasking => &MULTITASKING_CAPABILITIES,
        SettingsPanel::PowerBattery => &POWER_BATTERY_CAPABILITIES,
        SettingsPanel::PrintersScanners => &PRINTERS_SCANNERS_CAPABILITIES,
        SettingsPanel::UsersAccounts => &USERS_ACCOUNTS_CAPABILITIES,
        SettingsPanel::OnlineAccounts => &ONLINE_ACCOUNTS_CAPABILITIES,
        SettingsPanel::PrivacyPermissions => &PRIVACY_PERMISSIONS_CAPABILITIES,
        SettingsPanel::Wellbeing => &WELLBEING_CAPABILITIES,
        SettingsPanel::UpdatesAbout => &UPDATES_ABOUT_CAPABILITIES,
        SettingsPanel::LockScreen => &LOCK_SCREEN_CAPABILITIES,
        SettingsPanel::DateTime => &DATE_TIME_CAPABILITIES,
        SettingsPanel::LanguageRegion => &LANGUAGE_REGION_CAPABILITIES,
        _ => &[],
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_configuration_source(settings_panel: SettingsPanel) -> String {
    format!(
        "{} controls use built-in device services. {}",
        settings_panel.display_name(),
        device_settings_capability_scope(settings_panel)
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_capability_scope(settings_panel: SettingsPanel) -> &'static str {
    match settings_panel {
        SettingsPanel::Applications => {
            "Default apps, application permissions, and installed app behavior stay consistent across Goblins OS."
        }
        SettingsPanel::Network => {
            "Wi-Fi, known networks, passwords, connection repair, and proxy behavior are managed from Network settings."
        }
        SettingsPanel::NetworkServices => {
            "Wired, VPN, and advanced connection controls are managed from Network settings."
        }
        SettingsPanel::Bluetooth => {
            "Bluetooth adapter, pairing, trust, and connected-device controls stay in Bluetooth settings."
        }
        SettingsPanel::MobileBroadband => {
            "Cellular modem, SIM, and WWAN connection controls use modem services."
        }
        SettingsPanel::Sharing => {
            "Remote desktop, file sharing, and device sharing controls use sharing services."
        }
        SettingsPanel::Displays => {
            "Resolution, scaling, arrangement, refresh rate, and Night Light follow display services."
        }
        SettingsPanel::ColorManagement => {
            "Display and device color profiles come from color services."
        }
        SettingsPanel::Sound => {
            "Audio input, output, alert, and hardware routing controls follow audio services."
        }
        SettingsPanel::Keyboard => {
            "Keyboard shortcuts, input sources, and advanced typing controls follow input settings."
        }
        SettingsPanel::MouseTrackpad => {
            "Mouse, trackpad, pointer, scroll, and gesture controls follow input settings."
        }
        SettingsPanel::DrawingTablet => {
            "Pen, stylus, tablet buttons, and mapping controls follow tablet settings."
        }
        SettingsPanel::Accessibility => {
            "Assistive technology, visual, hearing, typing, and pointing controls follow accessibility settings."
        }
        SettingsPanel::DesktopWallpaper => {
            "Wallpaper picking, light/dark backgrounds, and placement controls follow wallpaper settings."
        }
        SettingsPanel::Notifications => {
            "Per-app notification, lock-screen, alert, and quiet-mode controls follow notification settings."
        }
        SettingsPanel::SearchIndexing => {
            "Desktop search providers, indexing, and result visibility follow search settings."
        }
        SettingsPanel::Multitasking => {
            "Workspace, hot corner, and window switching preferences follow desktop settings."
        }
        SettingsPanel::PowerBattery => {
            "Power mode, battery, suspend, and energy settings follow power services."
        }
        SettingsPanel::PrintersScanners => {
            "Printer, scanner, and device controls use print services."
        }
        SettingsPanel::UsersAccounts => {
            "Local users, passwords, administrator rights, and device identity stay in account settings."
        }
        SettingsPanel::OnlineAccounts => {
            "Cloud and internet account providers stay in account settings."
        }
        SettingsPanel::PrivacyPermissions => {
            "Camera, microphone, location, file history, and portal permissions use protected privacy prompts."
        }
        SettingsPanel::Wellbeing => {
            "Screen time, break reminders, and attention-health preferences follow wellbeing settings."
        }
        SettingsPanel::UpdatesAbout => {
            "System identity, About details, software tools, and update actions follow device policy."
        }
        _ => "This category uses the relevant Goblins OS service.",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_status_label(readiness: DeviceSettingsReadiness) -> &'static str {
    match readiness {
        DeviceSettingsReadiness::Ready => "ready",
        DeviceSettingsReadiness::IntegratedDesktopUnavailable => "not ready",
        DeviceSettingsReadiness::WaitingForSession => "waiting",
        DeviceSettingsReadiness::Unavailable => "not supported",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_action_label(readiness: DeviceSettingsReadiness) -> &'static str {
    match readiness {
        DeviceSettingsReadiness::Ready => "Open",
        DeviceSettingsReadiness::IntegratedDesktopUnavailable => "Open",
        DeviceSettingsReadiness::WaitingForSession => "Open",
        DeviceSettingsReadiness::Unavailable => "Not Supported",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_integrated_detail(settings_panel: SettingsPanel) -> String {
    format!(
        "Open detailed {} controls for this device. Status shown here comes from Goblins OS.",
        settings_panel.display_name()
    )
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_handoff_detail(
    settings_panel: SettingsPanel,
    readiness: DeviceSettingsReadiness,
) -> String {
    let panel_name = settings_panel.display_name();

    match readiness {
        DeviceSettingsReadiness::Ready => {
            format!("Open detailed {panel_name} controls for this device.")
        }
        DeviceSettingsReadiness::IntegratedDesktopUnavailable => {
            format!("{panel_name} controls open after the desktop finishes loading.")
        }
        DeviceSettingsReadiness::WaitingForSession => {
            format!("Checking {panel_name} controls.")
        }
        DeviceSettingsReadiness::Unavailable => {
            format!("{panel_name} controls are not supported on this device.")
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn device_settings_handoff_accessibility(
    settings_panel: SettingsPanel,
    readiness: DeviceSettingsReadiness,
) -> String {
    let panel_name = settings_panel.display_name();

    match readiness {
        DeviceSettingsReadiness::Ready => {
            format!("Manage {panel_name} controls.")
        }
        DeviceSettingsReadiness::IntegratedDesktopUnavailable => {
            format!("{panel_name} controls open after the desktop finishes loading.")
        }
        DeviceSettingsReadiness::WaitingForSession => "Checking device controls.".to_string(),
        DeviceSettingsReadiness::Unavailable => {
            "Device controls are not supported on this device.".to_string()
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_device_settings_handoff(
    panel: &gtk4::Box,
    settings_panel: SettingsPanel,
    title: &str,
    ready_detail: &str,
    system: Option<&SettingsSystemStatus>,
) {
    use gtk4::prelude::*;

    let Some(target) = settings_panel.gnome_control_center_panel() else {
        return;
    };
    let available = gnome_control_center_available();
    let readiness = device_settings_readiness(system, available);
    let ready = readiness.is_ready();
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
    row.add_css_class("gos-row");
    row.add_css_class("gos-switch-row");
    row.add_css_class("gos-device-handoff-row");
    let handoff_detail = if readiness.is_ready() {
        ready_detail.to_string()
    } else {
        device_settings_handoff_detail(settings_panel, readiness)
    };
    let accessibility = device_settings_handoff_accessibility(settings_panel, readiness);
    set_accessible_label_description(&row, title, &accessibility);

    let copy = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    copy.set_hexpand(true);
    copy.append(&label(title, &["gos-row-title"]));
    let detail_label = label(&handoff_detail, &["gos-row-copy"]);
    copy.append(&detail_label);
    row.append(&copy);

    let controls = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    controls.add_css_class("gos-handoff-controls");
    controls.set_valign(gtk4::Align::Center);
    controls.set_halign(gtk4::Align::End);
    if !ready {
        let status = settings_status_pill(device_settings_status_label(readiness), false);
        status.set_halign(gtk4::Align::End);
        controls.append(&status);
    }

    let action = button(
        device_settings_action_label(readiness),
        &["gos-permission-action", "gos-device-handoff-action"],
    );
    action.set_sensitive(ready);
    action.set_valign(gtk4::Align::Center);
    set_accessible_label_description(&action, title, &accessibility);
    if ready {
        let detail_label = detail_label.clone();
        let panel_name = settings_panel.display_name();
        action.connect_clicked(move |_| match launch_gnome_control_center_panel(target) {
            Ok(()) => detail_label.set_text(&format!("Opening {panel_name} controls.")),
            Err(error) => {
                detail_label.set_text(&format!(
                    "{panel_name} controls could not be opened from this session."
                ));
                eprintln!("settings_gnome_control_center_launch_error={error}");
            }
        });
    }
    controls.append(&action);
    row.append(&controls);
    panel.append(&row);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn gnome_control_center_available() -> bool {
    executable_in_path(GNOME_CONTROL_CENTER)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn executable_in_path(command: &str) -> bool {
    if command.contains('/') {
        return executable_file(std::path::Path::new(command));
    }
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|path| executable_file(&path.join(command)))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn executable_file(path: &std::path::Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn launch_gnome_control_center_panel(panel: &str) -> std::io::Result<()> {
    let mut command = gnome_control_center_command();
    command.arg(panel);
    command.spawn().map(|_| ())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn launch_gnome_control_center() -> std::io::Result<()> {
    gnome_control_center_command().spawn().map(|_| ())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn gnome_control_center_command() -> std::process::Command {
    let mut command = std::process::Command::new(GNOME_CONTROL_CENTER);
    if env::var_os("XDG_CURRENT_DESKTOP").is_none() {
        command.env("XDG_CURRENT_DESKTOP", "GNOME");
    }
    command
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_accessible_label_description<W>(widget: &W, label: &str, description: &str)
where
    W: gtk4::glib::object::IsA<gtk4::Accessible>,
{
    use gtk4::prelude::*;

    widget.update_property(&[
        gtk4::accessible::Property::Label(label),
        gtk4::accessible::Property::Description(description),
    ]);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PolicyControlCounts {
    total: usize,
    allowed: usize,
    gated: usize,
    denied: usize,
    granted: usize,
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn policy_control_counts(controls: &[PolicyControl]) -> PolicyControlCounts {
    let mut counts = PolicyControlCounts {
        total: controls.len(),
        allowed: 0,
        gated: 0,
        denied: 0,
        granted: 0,
    };

    for control in controls {
        match control.state.as_str() {
            "allowed" => counts.allowed += 1,
            "denied" => counts.denied += 1,
            "permission-gated" => counts.gated += 1,
            _ => {}
        }
        if control.grant.is_some() {
            counts.granted += 1;
        }
    }

    counts
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn policy_control_counts_label(counts: PolicyControlCounts) -> String {
    if counts.total == 0 {
        "waiting".to_string()
    } else if counts.gated > 0 {
        format!("{} gated", counts.gated)
    } else if counts.denied > 0 {
        format!("{} denied", counts.denied)
    } else {
        "allowed".to_string()
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn policy_profile_display_name(profile: &str) -> &'static str {
    match profile {
        "consumer" => "Consumer",
        "business" => "Business",
        "enterprise" => "Enterprise",
        "local-only" => "Local-only",
        _ => "Custom",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn policy_data_boundary_label(profile: &str) -> &'static str {
    match profile {
        "local-only" => "local only",
        "enterprise" => "admin managed",
        "business" => "team",
        "consumer" => "personal",
        _ => "custom",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn policy_control_status(control: &PolicyControl) -> String {
    match &control.grant {
        Some(grant) => format!("{} · granted {}", control.state, grant.granted_at),
        None => control.state.clone(),
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn policy_control_detail(control: &PolicyControl) -> String {
    match &control.grant {
        Some(grant) => format!("{} · {}", control.detail, grant.acknowledgement),
        None => control.detail.clone(),
    }
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

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_native_settings(_config: SettingsConfig, _state: SettingsState) -> SettingsResult<()> {
    println!("settings_state=unavailable");
    println!("settings_reason=build_requires_linux_native_desktop_feature");
    Ok(())
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
    let response = http_get_response(base_url, path)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    serde_json::from_slice(&response.body).map_err(|_| CoreFetchError::Decode)
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

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn grant_policy_permission(
    base_url: &str,
    control_id: &str,
    acknowledgement: &str,
) -> Result<String, CoreFetchError> {
    let body = serde_json::json!({
        "control_id": control_id,
        "acknowledgement": acknowledgement,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/policy/permissions/grant", &body)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(format!("Goblins OS permission granted for {control_id}."))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn permission_acknowledgement(control_id: &str, profile: &str) -> String {
    format!("GRANT GOBLINS OS PERMISSION {control_id} FOR {profile}")
}

/// The core relay URL as reported by the running Goblins OS, falling back to the
/// local default. The `append_*` panel helpers only receive `state`, so the
/// authoritative URL comes from the system status the core itself published.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn config_core_url(state: &SettingsState) -> String {
    state
        .system
        .as_ref()
        .map(|system| system.session.core_url.clone())
        .unwrap_or_else(|| DEFAULT_CORE_URL.to_string())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn settings_ai_panel_status_summary(panel: SettingsPanel, state: &SettingsState) -> String {
    let mut parts = vec![
        format!("Panel: {}", panel.display_name()),
        format!("Panel summary: {}", panel.summary()),
        format!(
            "Owner: {}",
            panel
                .gnome_control_center_panel()
                .map(|target| format!("Goblins OS routes deeper controls to {target}."))
                .unwrap_or_else(|| "Goblins OS owns this panel directly.".to_string())
        ),
        format!(
            "Local services: {}",
            if state.core_ready {
                "connected"
            } else {
                "waiting for OS services"
            }
        ),
    ];

    if let Some(catalog) = state.ai_actions.as_ref() {
        parts.push(format!("AI engine: {}", catalog.engine.detail));
    }

    match panel {
        SettingsPanel::Network | SettingsPanel::NetworkServices => {
            if let Some(network) = state.network.as_ref() {
                parts.push(format!(
                    "Network: manager={}, online={}, connectivity={}, state={}, detail={}",
                    network.manager_available,
                    network.online,
                    network.connectivity,
                    network.state,
                    network.detail
                ));
                if let Some(active) = network.active.as_ref() {
                    parts.push(format!(
                        "Active connection: {} over {} on {}",
                        active.name, active.kind, active.device
                    ));
                }
            }
        }
        SettingsPanel::Sound => {
            if let Some(audio) = state.audio.as_ref() {
                parts.push(format!(
                    "Audio: wireplumber={}, output={}, output_volume={:?}, output_muted={:?}, input={}, detail={}",
                    audio.wireplumber_available,
                    audio.output.available,
                    audio.output.volume_percent,
                    audio.output.muted,
                    audio.input.available,
                    audio.detail
                ));
            }
        }
        SettingsPanel::Displays => {
            if let Some(displays) = state.displays.as_ref() {
                parts.push(format!(
                    "Displays: session={}, display_config={}, outputs={}, detail={}",
                    displays.session_type,
                    displays.mutter_display_config_available,
                    displays.outputs.len(),
                    displays.detail
                ));
                for output in displays.outputs.iter().take(3) {
                    parts.push(format!(
                        "Display output: {} connected={} primary={} mode={:?}",
                        output.name, output.connected, output.primary, output.current_mode
                    ));
                }
            }
        }
        SettingsPanel::Bluetooth => {
            if let Some(bluetooth) = state.bluetooth.as_ref() {
                parts.push(format!(
                    "Bluetooth: bluez={}, service={}, adapter={}, powered={:?}",
                    bluetooth.bluez_available,
                    bluetooth.service_active,
                    bluetooth.adapter_present,
                    bluetooth.powered
                ));
            }
        }
        SettingsPanel::PrivacyPermissions | SettingsPanel::Policy | SettingsPanel::Models => {
            if let Some(privacy) = state.privacy.as_ref() {
                parts.push(format!(
                    "Private mode: offline={}, detail={}",
                    privacy.offline, privacy.detail
                ));
            }
            if let Some(policy) = state.policy.as_ref() {
                parts.push(format!(
                    "Policy: profile={}, data boundary={}",
                    policy.profile, policy.data_boundary
                ));
            }
        }
        SettingsPanel::Storage | SettingsPanel::UpdatesAbout => {
            if let Some(image) = state.system_image.as_ref() {
                parts.push(format!(
                    "Boot image: available={}, rollback={}, staged={}, detail={}",
                    image.available, image.rollback_available, image.staged_available, image.detail
                ));
            }
        }
        SettingsPanel::Accessibility => {
            if let Some(accessibility) = state.accessibility.as_ref() {
                parts.push(format!("Accessibility: {}", accessibility.detail));
            }
        }
        _ => {}
    }

    bounded_status_summary(&parts.join(" "))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn bounded_status_summary(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(1200)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn ask_settings_context(
    base_url: &str,
    panel: &str,
    topic: &str,
    question: &str,
    status_summary: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "panel": panel,
        "topic": topic,
        "question": question.trim(),
        "status_summary": status_summary,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/ai/settings-context", &body)
        .map_err(|error| format!("Goblins OS could not reach the Settings AI helper: {error}."))?;
    let outcome: SettingsContextAiResponse =
        serde_json::from_slice(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(outcome.text)
    } else {
        Err(outcome.text)
    }
}

/// Hand the user's personal OpenAI API key to Goblins OS for owner-only
/// storage. Consent is implicit in pressing Save in this OS-owned panel; the key
/// travels once over the loopback relay and is never echoed back to the GUI.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_openai_key(base_url: &str, api_key: &str) -> Result<String, CoreFetchError> {
    let body = serde_json::json!({
        "api_key": api_key,
        "consent": true,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/models/openai-key", &body)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok("Saved. Goblins OS now holds your OpenAI key owner-only — pick \"OpenAI hosted\" above to use it.".to_string())
}

/// Select which engine powers the Goblins AI runtime: the on-device GPT-OSS heart, or the
/// user's hosted OpenAI models. The core persists the choice in OS-owned state
/// and rejects the hosted engine when no key is stored, so the GUI never honors
/// a switch the OS cannot back.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_engine(base_url: &str, engine: &str) -> Result<String, CoreFetchError> {
    let body = serde_json::json!({ "engine": engine }).to_string();
    let response = http_post_json_response(base_url, "/v1/models/engine", &body)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(engine_selection_success_copy(engine).to_string())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_local_model(base_url: &str, model_id: &str) -> Result<String, CoreFetchError> {
    let body = serde_json::json!({
        "model_id": model_id,
        "consent": true,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/local-models/install", &body)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    let outcome: LocalModelInstallOutcome =
        serde_json::from_slice(&response.body).map_err(|_| CoreFetchError::Decode)?;
    Ok(local_model_install_message(&outcome))
}

/// Flip the OS-owned offline / private-mode flag. The core enforces the egress
/// gate; this only records the user's choice.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_privacy(base_url: &str, offline: bool) -> Result<String, CoreFetchError> {
    let body = serde_json::json!({ "offline": offline }).to_string();
    let response = http_post_json_response(base_url, "/v1/privacy", &body)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(if offline {
        "Private mode is on. The AI stays on this device — nothing leaves the machine.".to_string()
    } else {
        "Private mode is off. Goblins OS can reach the internet again when you ask it to."
            .to_string()
    })
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn connect_wifi(base_url: &str, ssid: &str, password: Option<&str>) -> Result<String, String> {
    let body = serde_json::json!({
        "ssid": ssid,
        "password": password.map(str::trim).filter(|password| !password.is_empty()),
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/network/wifi/connect", &body)
        .map_err(|error| format!("Goblins OS could not reach the Wi-Fi manager: {error}."))?;
    let outcome = wifi_connect_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn wifi_connect_outcome(body: &[u8]) -> Result<WifiConnectOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_proxy_mode(base_url: &str, mode: &str) -> Result<String, String> {
    let body = serde_json::json!({ "mode": mode }).to_string();
    let response = http_post_json_response(base_url, "/v1/network/proxy/mode", &body)
        .map_err(|error| format!("Goblins OS could not reach the proxy manager: {error}."))?;
    let outcome = proxy_mode_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(format!(
            "{} · {}",
            settings_detail_display_copy(&outcome.text),
            proxy_mode_detail(&outcome.mode)
        ))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn proxy_mode_outcome(body: &[u8]) -> Result<ProxyModeOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_bluetooth_power(base_url: &str, powered: bool) -> Result<String, String> {
    let body = serde_json::json!({ "powered": powered }).to_string();
    let response = http_post_json_response(base_url, "/v1/bluetooth/power", &body)
        .map_err(|error| format!("Goblins OS could not reach the Bluetooth manager: {error}."))?;
    let outcome = bluetooth_power_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn bluetooth_power_outcome(body: &[u8]) -> Result<BluetoothPowerOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_audio_volume(base_url: &str, target: &str, volume_percent: u8) -> Result<String, String> {
    let body = serde_json::json!({
        "target": target,
        "volume_percent": volume_percent,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/audio/volume", &body)
        .map_err(|error| format!("Goblins OS could not reach the audio manager: {error}."))?;
    let outcome = audio_control_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_audio_mute(base_url: &str, target: &str, muted: bool) -> Result<String, String> {
    let body = serde_json::json!({
        "target": target,
        "muted": muted,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/audio/mute", &body)
        .map_err(|error| format!("Goblins OS could not reach the audio manager: {error}."))?;
    let outcome = audio_control_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_audio_default_device(
    base_url: &str,
    target: &str,
    device_id: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "target": target,
        "device_id": device_id,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/audio/default-device", &body)
        .map_err(|error| format!("Goblins OS could not reach the audio manager: {error}."))?;
    let outcome = audio_control_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn audio_control_outcome(body: &[u8]) -> Result<AudioControlOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_sound_preference_bool(base_url: &str, target: &str, value: bool) -> Result<String, String> {
    let body = serde_json::json!({
        "target": target,
        "value": value,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/audio/preference", &body)
        .map_err(|error| format!("Goblins OS could not reach sound preferences: {error}."))?;
    let outcome = sound_preference_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn sound_preference_outcome(body: &[u8]) -> Result<SoundPreferenceOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_notification_preference_bool(
    base_url: &str,
    target: &str,
    child: Option<&str>,
    value: bool,
) -> Result<String, String> {
    let body = serde_json::json!({
        "target": target,
        "child": child,
        "value": value,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/notifications/preference", &body)
        .map_err(|error| {
            format!("Goblins OS could not reach notification preferences: {error}.")
        })?;
    let outcome =
        notification_preference_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn notification_preference_outcome(
    body: &[u8],
) -> Result<NotificationPreferenceOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_appearance_color_scheme(base_url: &str, theme: &str) -> Result<String, String> {
    let body = serde_json::json!({ "scheme": theme }).to_string();
    let response = http_post_json_response(base_url, "/v1/appearance/color-scheme", &body)
        .map_err(|error| format!("Goblins OS could not reach the appearance manager: {error}."))?;
    let outcome = appearance_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(format!(
            "{} · {}",
            settings_detail_display_copy(&outcome.text),
            appearance_scheme_detail(normalized_appearance_theme(&outcome.theme))
        ))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn appearance_outcome(body: &[u8]) -> Result<AppearanceOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_wallpaper_placement(base_url: &str, placement: &str) -> Result<String, String> {
    let body = serde_json::json!({ "placement": placement }).to_string();
    let response = http_post_json_response(base_url, "/v1/appearance/wallpaper-placement", &body)
        .map_err(|error| {
        format!("Goblins OS could not reach the wallpaper manager: {error}.")
    })?;
    let outcome = wallpaper_placement_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(format!(
            "{} · {}",
            settings_detail_display_copy(&outcome.text),
            background_picture_option_detail(&outcome.placement)
        ))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn wallpaper_placement_outcome(body: &[u8]) -> Result<WallpaperPlacementOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_wallpaper_shading(base_url: &str, shading: &str) -> Result<String, String> {
    let body = serde_json::json!({ "shading": shading }).to_string();
    let response = http_post_json_response(base_url, "/v1/appearance/wallpaper-shading", &body)
        .map_err(|error| {
            format!("Goblins OS could not reach the wallpaper color manager: {error}.")
        })?;
    let outcome = wallpaper_shading_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(format!(
            "{} · {}",
            settings_detail_display_copy(&outcome.text),
            background_shading_detail(&outcome.shading)
        ))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn wallpaper_shading_outcome(body: &[u8]) -> Result<WallpaperShadingOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_desktop_privacy_bool(base_url: &str, target: &str, value: bool) -> Result<String, String> {
    set_desktop_privacy_preference(base_url, target, serde_json::json!(value))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_desktop_privacy_number(base_url: &str, target: &str, value: f64) -> Result<String, String> {
    set_desktop_privacy_preference(base_url, target, serde_json::json!(value))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_desktop_privacy_preference(
    base_url: &str,
    target: &str,
    value: serde_json::Value,
) -> Result<String, String> {
    let body = serde_json::json!({
        "target": target,
        "value": value,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/privacy/desktop", &body)
        .map_err(|error| format!("Goblins OS could not reach the privacy manager: {error}."))?;
    let outcome = desktop_privacy_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn desktop_privacy_outcome(body: &[u8]) -> Result<DesktopPrivacyOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_accessibility_bool(base_url: &str, target: &str, value: bool) -> Result<String, String> {
    set_accessibility_preference(base_url, target, serde_json::json!(value))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_accessibility_number(base_url: &str, target: &str, value: f64) -> Result<String, String> {
    set_accessibility_preference(base_url, target, serde_json::json!(value))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_accessibility_preference(
    base_url: &str,
    target: &str,
    value: serde_json::Value,
) -> Result<String, String> {
    let body = serde_json::json!({
        "target": target,
        "value": value,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/accessibility/preference", &body)
        .map_err(|error| format!("Goblins OS could not reach accessibility settings: {error}."))?;
    let outcome =
        accessibility_preference_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn accessibility_preference_outcome(
    body: &[u8],
) -> Result<AccessibilityPreferenceOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_input_bool(base_url: &str, target: &str, value: bool) -> Result<String, String> {
    set_input_preference(base_url, target, serde_json::json!(value))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_input_number(base_url: &str, target: &str, value: f64) -> Result<String, String> {
    set_input_preference(base_url, target, serde_json::json!(value))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn set_input_preference(
    base_url: &str,
    target: &str,
    value: serde_json::Value,
) -> Result<String, String> {
    let body = serde_json::json!({
        "target": target,
        "value": value,
    })
    .to_string();
    let response = http_post_json_response(base_url, "/v1/input/preference", &body)
        .map_err(|error| format!("Goblins OS could not reach the input manager: {error}."))?;
    let outcome = input_preference_outcome(&response.body).map_err(|error| error.to_string())?;

    if (200..=299).contains(&response.status) && outcome.ok {
        Ok(settings_detail_display_copy(&outcome.text))
    } else {
        Err(settings_detail_display_copy(&outcome.text))
    }
}

fn input_preference_outcome(body: &[u8]) -> Result<InputPreferenceOutcome, CoreFetchError> {
    serde_json::from_slice(body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn wifi_requires_password(network: &WifiNetwork) -> bool {
    !network.security.trim().is_empty()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn wifi_security_label(security: &str) -> String {
    let security = security.trim();
    if security.is_empty() {
        "open network".to_string()
    } else {
        format!("{security} protected")
    }
}

fn engine_active_copy(engine: &str) -> &'static str {
    match engine {
        "openai-api" => {
            "Active engine: OpenAI hosted models — answers come from OpenAI's API using your key."
        }
        "codex" => {
            "Active engine: your OpenAI account via Codex — Goblins OS works through OpenAI's own coding agent."
        }
        _ => {
            "Active engine: GPT-OSS on-device — the heart of Goblins OS, private and local by default."
        }
    }
}

/// The Codex sign-in row beneath the engine selector. When Codex is ready but
/// not signed in, it offers a single button that starts `codex login`.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn append_codex_settings(panel: &gtk4::Box, state: &SettingsState, feedback: &gtk4::Label) -> bool {
    use gtk4::prelude::*;

    match &state.codex {
        Some(codex) if codex.authenticated => {
            panel.append(&system_row("Codex · signed in", &codex.detail));
            false
        }
        Some(codex) if codex.installed => {
            panel.append(&system_row("Codex · sign in", &codex.detail));
            let signin = button(
                "Sign in with your OpenAI account",
                &["gos-permission-action"],
            );
            set_accessible_label_description(
                &signin,
                "Sign in to Codex with your OpenAI account",
                "Starts codex login. Credentials remain owned by Codex.",
            );
            let feedback = feedback.clone();
            signin.connect_clicked(move |_| match codex_login() {
                Ok(()) => feedback.set_text(
                    "Opening Codex sign-in — finish in the browser, then reopen Settings.",
                ),
                Err(error) => {
                    feedback.set_text("Goblins OS could not start Codex sign-in.");
                    eprintln!("settings_codex_login_error={error}");
                }
            });
            panel.append(&signin);
            true
        }
        Some(codex) => {
            panel.append(&system_row("Codex · not included", &codex.detail));
            false
        }
        None => false,
    }
}

/// Start `codex login` (browser-based ChatGPT sign-in). The umask makes Codex's
/// `auth.json` group-readable so Goblins OS — a different system user — can use
/// the account; Goblins OS itself never reads the credentials. CODEX_HOME is set
/// OS-wide so the GUI and core agree on where the sign-in lives.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn codex_login() -> std::io::Result<()> {
    std::process::Command::new("sh")
        .arg("-c")
        .arg("umask 0007; exec codex login")
        .spawn()
        .map(|_| ())
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

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn http_post_json_response(
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
        .set_read_timeout(Some(Duration::from_millis(1200)))
        .map_err(|_| CoreFetchError::Transport)?;
    stream
        .set_write_timeout(Some(Duration::from_millis(900)))
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

fn parse_http_response(response: &[u8]) -> Result<HttpResponse, CoreFetchError> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(CoreFetchError::Malformed)?;
    let headers =
        std::str::from_utf8(&response[..header_end]).map_err(|_| CoreFetchError::Malformed)?;
    let mut header_lines = headers.lines();
    let status = header_lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or(CoreFetchError::Malformed)?;
    let headers = header_lines
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

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn runtime_label(runtime: &RuntimeReport) -> String {
    let mut names = Vec::new();

    if let Some(selected) = &runtime.selected {
        names.push(readable_runtime_value(selected).to_string());
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

fn option_bool_word(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const GOBLINS_OS_SETTINGS_CSS: &str = r#"
window.gos-settings-window {
  background: transparent;
}

.gos-settings-root,
.gos-settings-root label,
.gos-settings-root button,
.gos-settings-root entry,
.gos-settings-root entry > text,
.gos-settings-root text,
.gos-settings-root combobox,
.gos-settings-root popover,
.gos-settings-root popover label {
  font-family: "Inter", "Inter Display", "Noto Sans", sans-serif;
  letter-spacing: 0;
}

.gos-settings-root {
  margin: 16px;
  padding: 0;
  border: 1px solid @gos_hairline;
  border-radius: 16px;
  background: @gos_surface_muted;
  box-shadow: 0 1px 0 alpha(@gos_panel_sheen, 0.64) inset,
              0 26px 72px @gos_shadow_window;
}

.gos-settings-root .gos-settings-top {
  min-height: 50px;
  padding: 0 18px;
  border: none;
  border-bottom: 1px solid @gos_hairline;
  border-radius: 16px 16px 0 0;
  background: alpha(@gos_material_ultra_thick, 0.92);
  box-shadow: none;
}

.gos-settings-root .gos-settings-body {
  margin-top: 0;
  background: @gos_surface_muted;
}

.gos-settings-root .gos-side-panel {
  min-width: 244px;
  padding: 12px 10px 12px;
  border: none;
  border-right: 1px solid @gos_hairline;
  border-radius: 0 0 0 16px;
  background: linear-gradient(180deg, alpha(@gos_material_thick, 0.72), alpha(@gos_material_regular, 0.52));
  box-shadow: none;
}

.gos-settings-root .gos-main-panel {
  min-width: 584px;
  padding: 24px 32px 28px;
  border: none;
  border-radius: 0 0 16px 0;
  background: @gos_surface_muted;
  box-shadow: none;
}

.gos-settings-root .gos-main-scroll,
.gos-settings-root .gos-main-scroll viewport,
.gos-settings-root .gos-side-scroll,
.gos-settings-root .gos-side-scroll viewport {
  border: none;
  background: transparent;
}

.gos-settings-root .gos-side-scroll scrollbar.vertical {
  min-width: 6px;
  margin: 6px 0;
}

.gos-settings-root .gos-side-scroll scrollbar.vertical slider {
  min-height: 44px;
  border-radius: 999px;
  background: @gos_label_tertiary;
}

.gos-brand {
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0;
}

/* Item titles read a clear step below section headers (weight, not just size). */
.gos-row-title {
  font-size: 14px;
  font-weight: 500;
  letter-spacing: 0;
}

.gos-muted,
.gos-row-copy,
.gos-panel-intro {
  color: @gos_label_secondary;
}

.gos-panel-intro {
  margin-bottom: 6px;
  font-size: 14px;
  line-height: 19px;
}

.gos-settings-root .gos-kicker {
  color: @gos_label_secondary;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0;
  text-transform: none;
}

.gos-section-title {
  font-size: 28px;
  font-weight: 600;
  letter-spacing: 0;
}

.gos-subsection-title {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0;
  margin-top: 14px;
  margin-bottom: 2px;
}

.gos-search-entry {
  min-height: 33px;
  margin: 0 2px 8px;
  border-radius: 10px;
  border: 1px solid transparent;
  background: alpha(@gos_surface, 0.66);
  color: @gos_ink;
  box-shadow: 0 1px 0 alpha(@gos_panel_sheen, 0.32) inset,
              0 0 0 1px alpha(@gos_hairline, 0.70) inset;
}

.gos-search-entry:focus-within {
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset;
}

.gos-search-status {
  min-height: 18px;
  margin: -3px 10px 4px;
  color: @gos_label_secondary;
  font-size: 12px;
  font-weight: 500;
}

.gos-settings-root button:focus,
.gos-settings-root switch:focus,
.gos-settings-root scale:focus,
.gos-settings-root combobox:focus {
  outline: none;
}

.gos-settings-root button:disabled {
  opacity: 1;
  color: @gos_label_tertiary;
}

.gos-nav-group {
  margin-top: 10px;
  margin-bottom: 3px;
  padding-left: 12px;
  color: @gos_label_tertiary;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0;
  text-transform: none;
}

.gos-side-nav-list {
  padding: 0 3px 12px;
}

.gos-settings-root .gos-side-nav {
  min-height: 30px;
  padding: 0 7px;
  border-radius: 8px;
  border: 1px solid transparent;
  box-shadow: none;
  background: transparent;
  color: @gos_label_secondary;
  font-size: 14px;
  font-weight: 500;
}

.gos-side-row-content {
  min-height: 30px;
}

/* Colored rounded category tiles — the macOS-kit "system settings" signature.
   A white glyph on a saturated, lightly-glossed tile; the tint class resolves to
   a @gos_tint_* token so light and dark both stay vivid. */
.gos-side-icon-well {
  min-width: 26px;
  min-height: 26px;
  border-radius: 7px;
  background: @gos_tint_graphite;
  box-shadow: inset 0 1px 0 alpha(@gos_material_sheen, 0.45),
              0 0.5px 1.5px @gos_shadow_raise;
}

.gos-side-icon-well.gos-tint-blue     { background: @gos_tint_blue; }
.gos-side-icon-well.gos-tint-teal     { background: @gos_tint_teal; }
.gos-side-icon-well.gos-tint-indigo   { background: @gos_tint_indigo; }
.gos-side-icon-well.gos-tint-purple   { background: @gos_tint_purple; }
.gos-side-icon-well.gos-tint-pink     { background: @gos_tint_pink; }
.gos-side-icon-well.gos-tint-red      { background: @gos_tint_red; }
.gos-side-icon-well.gos-tint-orange   { background: @gos_tint_orange; }
.gos-side-icon-well.gos-tint-yellow   { background: @gos_tint_yellow; }
.gos-side-icon-well.gos-tint-green    { background: @gos_tint_green; }
.gos-side-icon-well.gos-tint-graphite { background: @gos_tint_graphite; }

.gos-side-icon {
  margin: 5px;
  -gtk-icon-size: 16px;
  color: @gos_on_tint;
}

.gos-side-nav-label {
  color: inherit;
}

.gos-side-text-stack {
  min-height: 20px;
}

.gos-side-match-label {
  margin-top: -1px;
  color: @gos_label_secondary;
  font-size: 11px;
  font-weight: 500;
}

/* Hover and selection stay neutral so the colored tile carries the only hue —
   two competing accents would read busy. Selection is a calm filled pill. */
.gos-settings-root .gos-side-nav:hover {
  background: @gos_fill_quaternary;
  color: @gos_ink;
}

.gos-settings-root .gos-side-nav:focus {
  color: @gos_ink;
  background: @gos_fill_quaternary;
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              inset 0 0 0 1px @gos_primary_border;
}

.gos-settings-root .gos-side-nav.is-current {
  border-color: transparent;
  background: @gos_fill_primary;
  color: @gos_ink;
  font-weight: 600;
}

.gos-settings-root .gos-side-nav.is-current:hover {
  background: @gos_fill_primary;
}

.gos-settings-root .gos-side-nav.is-current .gos-side-icon-well {
  box-shadow: inset 0 1px 0 alpha(@gos_material_sheen, 0.55),
              0 0.5px 1.5px @gos_shadow_raise,
              0 0 0 1px alpha(@gos_on_tint, 0.10);
}

.gos-settings-root .gos-side-nav.is-current:focus {
  background: @gos_fill_primary;
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              inset 0 0 0 1px @gos_primary_border;
}

.gos-settings-root .gos-row,
.gos-settings-root .gos-system-row {
  padding: 8px 14px;
  border: 1px solid alpha(@gos_hairline, 0.78);
  border-radius: 10px;
  background: alpha(@gos_surface, 0.82);
  box-shadow: 0 1px 0 alpha(@gos_panel_sheen, 0.28) inset;
}

.gos-settings-root .gos-health-row {
  min-height: 54px;
}

.gos-settings-root .gos-slider-row {
  min-height: 62px;
}

.gos-settings-root .gos-choice-row {
  min-height: 58px;
}

.gos-settings-root .gos-choice-control {
  min-height: 34px;
}

.gos-settings-root .gos-choice-control button {
  min-height: 32px;
  padding: 4px 12px;
  border-radius: 8px;
  border: 1px solid @gos_hairline;
  background: alpha(@gos_surface, 0.84);
  color: @gos_ink;
  font-weight: 500;
}

.gos-settings-root .gos-choice-control button:hover {
  border-color: @gos_hairline_strong;
  background: @gos_surface_sunken;
}

.gos-settings-root .gos-choice-control button:focus {
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset;
}

.gos-settings-root .gos-row:hover,
.gos-settings-root .gos-system-row:hover {
  border-color: @gos_hairline_strong;
  background: alpha(@gos_surface, 0.94);
  box-shadow: 0 1px 0 alpha(@gos_panel_sheen, 0.34) inset,
              0 2px 8px @gos_shadow_raise;
}

.gos-settings-root .gos-system-row {
  padding: 9px 4px;
  border-color: transparent;
  border-radius: 0;
  background: transparent;
  box-shadow: none;
}

.gos-settings-root .gos-system-row:hover {
  border-color: transparent;
  background: transparent;
  box-shadow: none;
}

.gos-settings-root .gos-preference-group .gos-system-row {
  padding: 8px 14px;
}

.gos-settings-root .gos-row:focus-within,
.gos-settings-root .gos-system-row:focus-within {
  border-color: @gos_primary_border;
  background: @gos_surface;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset,
              0 1px 0 alpha(@gos_panel_sheen, 0.52) inset;
}

.gos-settings-root .gos-preference-group {
  margin-top: 0;
  border: 1px solid alpha(@gos_hairline, 0.78);
  border-radius: 12px;
  background: alpha(@gos_surface, 0.82);
  box-shadow: 0 1px 0 alpha(@gos_panel_sheen, 0.28) inset;
}

.gos-settings-root .gos-preference-group .gos-row {
  border-radius: 0;
  border-width: 0;
  border-bottom: 1px solid @gos_separator;
  background: transparent;
  box-shadow: none;
}

.gos-settings-root .gos-preference-group .gos-row:first-child {
  border-radius: 12px 12px 0 0;
}

.gos-settings-root .gos-preference-group .gos-row:last-child {
  border-bottom-width: 0;
  border-radius: 0 0 12px 12px;
}

.gos-settings-root .gos-preference-group .gos-row:first-child:last-child {
  border-radius: 12px;
}

.gos-settings-root .gos-preference-group .gos-row:hover {
  border-color: @gos_separator;
  background: alpha(@gos_surface, 0.50);
  box-shadow: none;
}

/* Inset focus ring so the highlight never clips to a square corner on a
   mid-group row (those rows have radius 0). Inset traces the row's real edge. */
.gos-settings-root .gos-preference-group .gos-row:focus-within {
  border-color: @gos_separator;
  background: alpha(@gos_surface, 0.70);
  box-shadow: inset 0 0 0 2px @gos_focus;
}

.gos-settings-root .gos-handoff-summary {
  min-height: 76px;
  padding: 12px 16px;
  border-color: @gos_hairline_strong;
  background: linear-gradient(180deg, alpha(@gos_surface, 0.92), alpha(@gos_surface_sunken, 0.66));
}

.gos-settings-root .gos-device-handoff-row {
  min-height: 66px;
  padding: 10px 15px;
  border-color: alpha(@gos_hairline_strong, 0.82);
  background: alpha(@gos_surface, 0.86);
}

.gos-settings-root .gos-device-context-grid {
  margin-top: 0;
}

.gos-settings-root .gos-device-context-tile {
  min-height: 54px;
  padding: 9px 14px;
  border-color: alpha(@gos_hairline, 0.78);
  background: alpha(@gos_surface, 0.82);
}

.gos-settings-root .gos-device-context-source {
  background: alpha(@gos_surface_sunken, 0.72);
}

.gos-settings-root .gos-device-capability-grid {
  margin-top: 0;
}

.gos-settings-root .gos-device-capability-tile {
  min-height: 52px;
  padding: 8px 14px;
  background: alpha(@gos_surface, 0.80);
}

.gos-handoff-controls {
  min-width: 94px;
}

.gos-device-handoff-action {
  min-width: 86px;
  min-height: 36px;
  padding: 4px 16px;
}

.gos-device-handoff-action:disabled {
  opacity: 1;
  color: @gos_label_secondary;
  border-color: alpha(@gos_hairline_strong, 0.84);
  background: alpha(@gos_fill_secondary, 0.72);
}

.gos-settings-root .gos-overview-summary-grid {
  margin-top: 0;
}

.gos-settings-root .gos-overview-summary-grid,
.gos-settings-root .gos-account-summary-grid,
.gos-settings-root .gos-appearance-summary-grid,
.gos-settings-root .gos-wallpaper-summary-grid,
.gos-settings-root .gos-input-summary-grid,
.gos-settings-root .gos-accessibility-summary-grid,
.gos-settings-root .gos-network-summary-grid,
.gos-settings-root .gos-privacy-summary-grid,
.gos-settings-root .gos-model-summary-grid,
.gos-settings-root .gos-policy-summary-grid,
.gos-settings-root .gos-developer-summary-grid,
.gos-settings-root .gos-updates-summary-grid,
.gos-settings-root .gos-recovery-summary-grid,
.gos-settings-root .gos-bluetooth-summary-grid,
.gos-settings-root .gos-display-summary-grid,
.gos-settings-root .gos-sound-summary-grid,
.gos-settings-root .gos-notifications-summary-grid,
.gos-settings-root .gos-storage-summary-grid {
  margin-top: 0;
}

.gos-settings-root .gos-overview-native-settings {
  min-height: 76px;
}

.gos-settings-root .gos-overview-summary-tile {
  min-height: 52px;
  padding: 8px 14px;
}

.gos-settings-root .gos-account-summary-tile,
.gos-settings-root .gos-appearance-summary-tile,
.gos-settings-root .gos-wallpaper-summary-tile,
.gos-settings-root .gos-input-summary-tile,
.gos-settings-root .gos-accessibility-summary-tile,
.gos-settings-root .gos-network-summary-tile,
.gos-settings-root .gos-privacy-summary-tile,
.gos-settings-root .gos-model-summary-tile,
.gos-settings-root .gos-policy-summary-tile,
.gos-settings-root .gos-developer-summary-tile,
.gos-settings-root .gos-updates-summary-tile,
.gos-settings-root .gos-recovery-summary-tile,
.gos-settings-root .gos-bluetooth-summary-tile,
.gos-settings-root .gos-display-summary-tile,
.gos-settings-root .gos-sound-summary-tile,
.gos-settings-root .gos-notifications-summary-tile,
.gos-settings-root .gos-storage-summary-tile {
  min-height: 54px;
  padding: 8px 14px;
}

/* The consolidated summary group — one class for every grouped status card,
   so new panels reach for this instead of minting another per-panel grid. */
.gos-settings-root .gos-summary-grid {
  margin-top: 0;
}

.gos-settings-root .gos-summary-tile {
  min-height: 54px;
  padding: 8px 14px;
}

.gos-settings-root .gos-side-panel .gos-system-row {
  margin-top: 14px;
  padding: 11px 12px;
  background: @gos_fill_tertiary;
  border-color: transparent;
}

.gos-settings-root .gos-side-panel .gos-search-empty {
  margin-top: 10px;
}

.gos-permission-action {
  min-height: 36px;
  border-radius: 10px;
  color: @gos_ink;
  border: 1px solid @gos_hairline_strong;
  background: alpha(@gos_surface, 0.94);
  font-weight: 600;
}

.gos-permission-action:hover {
  border-color: @gos_primary_border;
  background: @gos_surface_sunken;
}

.gos-permission-action:focus {
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset;
}

.gos-engine-choice {
  margin-top: 0;
  margin-bottom: 0;
  padding: 2px;
  border: 1px solid @gos_hairline;
  border-radius: 12px;
  background: @gos_surface_muted;
}

.gos-preference-choice {
  margin-top: 0;
  margin-bottom: 0;
  padding: 2px;
  border: 1px solid @gos_hairline;
  border-radius: 12px;
  background: @gos_surface_muted;
}

.gos-segmented-control {
  margin-top: 0;
  margin-bottom: 0;
  padding: 2px;
  border: 1px solid @gos_hairline;
  border-radius: 11px;
  background: @gos_surface_muted;
}

.gos-segmented-option {
  min-height: 30px;
  min-width: 96px;
  padding: 4px 14px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: transparent;
  box-shadow: none;
  color: @gos_label_secondary;
  font-size: 13px;
  font-weight: 500;
}

.gos-segmented-option:hover {
  color: @gos_ink;
  background: @gos_fill_tertiary;
}

.gos-segmented-option:focus {
  color: @gos_ink;
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset;
}

.gos-segmented-option.is-selected {
  color: @gos_ink;
  border-color: @gos_hairline_strong;
  background: @gos_surface;
  box-shadow: 0 1px 0 alpha(@gos_panel_sheen, 0.56) inset,
              0 2px 6px @gos_shadow_panel;
}

.gos-segmented-option.is-selected:focus {
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset,
              0 1px 0 alpha(@gos_panel_sheen, 0.56) inset;
}

.gos-engine-option {
  min-height: 32px;
  min-width: 188px;
  padding: 4px 14px;
  border-radius: 9px;
  font-weight: 600;
  color: @gos_ink;
  border: 1px solid transparent;
  background: transparent;
  box-shadow: none;
}

.gos-engine-option:disabled {
  opacity: 1;
  color: @gos_label_tertiary;
  border-color: @gos_hairline;
  background: @gos_fill_tertiary;
}

.gos-engine-option:hover {
  background: @gos_fill_tertiary;
}

.gos-engine-option:focus {
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset;
}

.gos-engine-active {
  color: @gos_on_primary;
  border-color: @gos_primary_border;
  background: linear-gradient(180deg, @gos_primary_top, @gos_primary_bottom);
  box-shadow: 0 1px 0 alpha(@gos_panel_sheen, 0.48) inset,
              0 2px 8px @gos_shadow_panel;
}

.gos-engine-advanced {
  min-width: 0;
  min-height: 34px;
  padding: 4px 16px;
  font-weight: 600;
  font-size: 13px;
  color: @gos_ink_muted;
  border: 1px solid @gos_hairline;
  background: transparent;
}

.gos-engine-advanced:disabled {
  opacity: 1;
  color: @gos_label_tertiary;
  border-color: @gos_hairline;
  background: @gos_fill_tertiary;
}

.gos-engine-advanced:hover {
  color: @gos_ink;
  background: @gos_fill_tertiary;
}

.gos-engine-advanced:focus {
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset;
}

.gos-key-field {
  margin-top: 6px;
  padding: 14px;
  border-radius: 12px;
  border: 1px solid @gos_hairline;
  background: @gos_surface_muted;
}

.gos-key-entry {
  min-height: 38px;
  border-radius: 9px;
  border: 1px solid @gos_hairline_strong;
  background: @gos_surface;
  color: @gos_ink;
  padding: 4px 12px;
  font-size: 14px;
}

.gos-key-entry > text {
  color: @gos_ink;
  caret-color: @gos_ink;
}

.gos-key-entry:focus-within {
  border-color: @gos_ink_faint;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset;
}

.gos-wifi-connect {
  min-width: 104px;
  min-height: 36px;
  padding: 4px 16px;
}

.gos-model-row {
  min-height: 112px;
}

.gos-model-requirements {
  padding-top: 2px;
}

.gos-model-action {
  min-width: 172px;
  min-height: 38px;
  padding: 4px 16px;
}

.gos-switch-row {
  min-height: 56px;
}

.gos-switch-row switch {
  min-width: 48px;
}

.gos-switch-row switch:focus {
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset;
}

.gos-slider-row scale {
  min-height: 34px;
}

.gos-slider-row scale trough {
  min-height: 4px;
  border-radius: 999px;
  background: @gos_fill_secondary;
}

.gos-slider-row scale highlight {
  min-height: 4px;
  border-radius: 999px;
  background: @gos_primary_bottom;
}

.gos-slider-row scale slider {
  min-width: 18px;
  min-height: 18px;
  margin: -7px 0;
  border-radius: 999px;
  border: 1px solid @gos_hairline_strong;
  background: @gos_on_primary;
  box-shadow: 0 1px 0 alpha(@gos_panel_sheen, 0.54) inset,
              0 2px 7px @gos_shadow_panel;
}

.gos-slider-row scale slider:hover,
.gos-slider-row scale slider:focus {
  border-color: @gos_primary_border;
  box-shadow: 0 0 0 3px @gos_focus,
              0 0 0 1px @gos_primary_border inset,
              0 2px 7px @gos_shadow_panel;
}

.gos-row-value {
  color: @gos_label_secondary;
  font-size: 13px;
  font-weight: 500;
  /* Tabular figures so versions, percentages, counts, and volumes never wobble
     as their digits change — the macOS standard for any data readout. */
  font-feature-settings: "tnum" 1;
  font-variant-numeric: tabular-nums;
}

.gos-status-value {
  margin-left: 12px;
}

.gos-storage-row {
  min-height: 86px;
}

.gos-storage-meter {
  min-height: 8px;
}

.gos-storage-meter trough {
  min-height: 7px;
  border-radius: 999px;
  background: @gos_fill_secondary;
}

.gos-storage-meter progress {
  min-height: 7px;
  border-radius: 999px;
  background: linear-gradient(90deg, @gos_primary_top, @gos_primary_bottom);
}

.gos-settings-root .gos-status-pill {
  padding: 4px 9px;
  border: 1px solid transparent;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0;
}

.gos-settings-root .gos-ready {
  color: @gos_ready;
  background: @gos_ready_soft;
  border-color: alpha(@gos_ready, 0.34);
}

.gos-settings-root .gos-status-quiet {
  color: @gos_label_secondary;
  background: @gos_fill_secondary;
  border-color: alpha(@gos_hairline, 0.92);
}

.gos-settings-root .gos-waiting {
  color: @gos_waiting;
  background: alpha(@gos_waiting, 0.12);
  border-color: alpha(@gos_waiting, 0.36);
}
"#;

#[cfg(test)]
fn test_local_model(state: &str, install_state: &str) -> LocalModelOption {
    LocalModelOption {
        id: "gpt-oss-20b".to_string(),
        name: "gpt-oss-20b".to_string(),
        role: "Local/private reasoning".to_string(),
        source: "openai/gpt-oss-20b".to_string(),
        weights_in_os_image: false,
        download_required: true,
        minimum_ram_gb: 16,
        minimum_gpu_vram_gb: None,
        minimum_free_storage_gb: 24,
        disk_requirement: "About 16GB of verified weights.".to_string(),
        state: state.to_string(),
        reasons: Vec::new(),
        install: LocalModelInstall {
            state: install_state.to_string(),
            consent_required: true,
            consent_recorded: false,
            manifest_required: false,
            verification_required: true,
            resumable: true,
            state_path: "/var/lib/goblins-os/models/state/gpt-oss-20b.json".to_string(),
            target_dir: "/var/lib/goblins-os/models/gpt-oss-20b".to_string(),
            manifest_path: "/var/lib/goblins-os/models/manifests/gpt-oss-20b.json".to_string(),
            detail: "No download has been requested.".to_string(),
        },
    }
}

#[cfg(test)]
fn test_local_model_catalog(model_dir_available_gb: Option<u64>) -> LocalModelCatalog {
    LocalModelCatalog {
        install_policy: "Downloads require explicit consent.".to_string(),
        hardware: HardwareReport {
            ram_gb: 32,
            gpu_vram_gb: Some(8),
            model_dir: "/var/lib/goblins-os/models".to_string(),
            model_dir_available_gb,
            runtime: RuntimeReport {
                selected: Some("llama.cpp".to_string()),
                ollama: true,
                vllm: false,
                lm_studio: false,
            },
        },
        models: vec![test_local_model("installable", "not-requested")],
    }
}

#[cfg(test)]
fn test_openai_key_status(configured: bool, engine: &str) -> OpenAiKeyStatus {
    OpenAiKeyStatus {
        configured,
        model: "gpt-5.5".to_string(),
        engine_selected: engine != "local-gpt-oss",
        engine: engine.to_string(),
        storage: "/var/lib/goblins-os/secrets/openai/api-key".to_string(),
    }
}

#[cfg(test)]
fn test_codex_status(installed: bool, authenticated: bool) -> CodexStatus {
    CodexStatus {
        installed,
        authenticated,
        detail: if !installed {
            "Codex account support is not included in this build.".to_string()
        } else if authenticated {
            "Codex is signed in and ready.".to_string()
        } else {
            "Codex account support is ready but not signed in.".to_string()
        },
    }
}

#[cfg(test)]
fn test_resident_status(
    process_state: &str,
    selected_engine: &str,
    heartbeat_age_secs: Option<u64>,
) -> ResidentStatus {
    ResidentStatus {
        generated_at: "test-generated-at".to_string(),
        source: "goblins-os-core".to_string(),
        state_path: "/var/lib/goblins-os/resident/resident.json".to_string(),
        process: ResidentProcess {
            state: process_state.to_string(),
            mode: "persistent".to_string(),
            heartbeat_age_secs,
            detail: "Goblins AI runtime status is reported by Goblins OS.".to_string(),
        },
        engine: ResidentEngine {
            selected: selected_engine.to_string(),
            cloud_relay_configured: false,
            local_relay_configured: true,
            relay_contract: "POST JSON {message:string} -> {text:string}".to_string(),
        },
        capabilities: vec![ResidentCapability {
            label: "Conversation".to_string(),
            state: process_state.to_string(),
            detail: "Conversation capability follows the Goblins AI runtime.".to_string(),
        }],
    }
}

#[cfg(test)]
fn test_voice_status(available: bool) -> VoiceStatus {
    VoiceStatus {
        available,
        wake_word: "Goblin".to_string(),
        wake_phrases: vec!["Goblin".to_string(), "Hey Goblin".to_string()],
        wake_listening: Some(VoiceCapability {
            ready: false,
            detail: "Press the voice button, then say Goblin. Background wake listening is not ready until the local wake-word listener is available.".to_string(),
        }),
        detail: if available {
            "Goblin voice is ready with local Whisper and Piper models.".to_string()
        } else {
            "Goblin voice runs on local Whisper and Piper models. Add the missing voice components."
                .to_string()
        },
    }
}

#[cfg(test)]
fn test_policy_status(profile: &str, locked: bool) -> PolicyStatus {
    PolicyStatus {
        generated_at: "test-generated-at".to_string(),
        source: "goblins-os-core".to_string(),
        state_path: "/var/lib/goblins-os/policy/profile.json".to_string(),
        permission_path: "/var/lib/goblins-os/policy/permissions.json".to_string(),
        profile: profile.to_string(),
        locked,
        data_boundary: format!("{profile} policy keeps data boundaries explicit."),
        secret_boundary: "Secrets stay in OS-owned services or server-side relays.".to_string(),
        controls: vec![
            PolicyControl {
                id: "cloud-openai".to_string(),
                label: "OpenAI cloud services".to_string(),
                state: "denied".to_string(),
                profile_state: "denied".to_string(),
                grant: None,
                detail: "Cloud OpenAI services are blocked by policy.".to_string(),
            },
            PolicyControl {
                id: "local-models".to_string(),
                label: "Local gpt-oss models".to_string(),
                state: "allowed".to_string(),
                profile_state: "allowed".to_string(),
                grant: None,
                detail: "Allowed by the active Goblins OS policy profile.".to_string(),
            },
            PolicyControl {
                id: "app-builder".to_string(),
                label: "Build Studio".to_string(),
                state: "permission-gated".to_string(),
                profile_state: "permission-gated".to_string(),
                grant: None,
                detail: "Build Studio app creation requires OS sandbox and policy review."
                    .to_string(),
            },
            PolicyControl {
                id: "computer-use".to_string(),
                label: "Computer Use".to_string(),
                state: "allowed".to_string(),
                profile_state: "permission-gated".to_string(),
                grant: Some(PolicyPermissionGrant {
                    granted_at: "test-granted-at".to_string(),
                    acknowledgement: "GRANT GOBLINS OS PERMISSION computer-use FOR local-only"
                        .to_string(),
                }),
                detail: "Allowed by an explicit Goblins OS permission grant.".to_string(),
            },
        ],
    }
}

#[cfg(test)]
fn test_storage_hardware(storage: Vec<StorageVolume>) -> HardwareStatus {
    HardwareStatus {
        generated_at: "test-generated-at".to_string(),
        source: "goblins-os-core".to_string(),
        platform: PlatformStatus {
            os: "linux".to_string(),
            desktop: "goblins".to_string(),
            session_type: "wayland".to_string(),
            current_desktop: "GNOME".to_string(),
        },
        memory: MemoryStatus {
            total_gb: 32,
            available_gb: 24,
        },
        storage,
        facilities: Vec::new(),
    }
}

#[cfg(test)]
fn test_privacy_status(
    offline: bool,
    gsettings_available: bool,
    schema_available: bool,
) -> PrivacyStatus {
    let available = gsettings_available && schema_available;
    PrivacyStatus {
        offline,
        detail: if offline {
            "Private mode is on. Local GPT-OSS handles prompts on this device.".to_string()
        } else {
            "Private mode is off. Hosted engines can connect when selected.".to_string()
        },
        desktop: Some(DesktopPrivacyStatus {
            gsettings_available,
            schema_available,
            remember_recent_files: Some(false),
            remember_app_usage: Some(false),
            remove_old_trash_files: Some(true),
            remove_old_temp_files: Some(false),
            old_files_age_days: Some(30),
            disable_microphone: Some(true),
            disable_camera: Some(false),
            disable_sound_output: Some(true),
            usb_protection: Some(true),
            detail: if available {
                "Privacy controls are ready for this desktop.".to_string()
            } else if !gsettings_available {
                "Desktop preference support is unavailable, so privacy controls are unavailable in this runtime."
                   .to_string()
            } else {
                "The standard privacy preferences are not available in this runtime.".to_string()
            },
        }),
        facilities: Vec::new(),
    }
}

#[cfg(test)]
fn test_accessibility_status(
    gsettings_available: bool,
    interface_schema_available: bool,
    assistive_schema_available: bool,
    display_comfort_schema_available: bool,
) -> AccessibilityStatus {
    let unavailable_detail =
        "Desktop preference support is unavailable, so accessibility controls are unavailable here.";
    AccessibilityStatus {
        source: "goblins-os-core".to_string(),
        gsettings_available,
        interface: InterfaceAccessibilityStatus {
            schema_available: interface_schema_available,
            reduce_motion: Some(true),
            text_scale: Some(1.15),
            detail: if gsettings_available && interface_schema_available {
                "Interface accessibility controls are ready.".to_string()
            } else {
                unavailable_detail.to_string()
            },
        },
        assistive: AssistiveTechnologyStatus {
            schema_available: assistive_schema_available,
            screen_reader: Some(true),
            screen_keyboard: Some(false),
            magnifier: Some(false),
            detail: if gsettings_available && assistive_schema_available {
                "Assistive technology controls are ready.".to_string()
            } else {
                unavailable_detail.to_string()
            },
        },
        display_comfort: DisplayComfortStatus {
            schema_available: display_comfort_schema_available,
            night_light_enabled: Some(true),
            schedule_automatic: Some(true),
            temperature: Some(4490),
            detail: if gsettings_available && display_comfort_schema_available {
                "Display comfort controls are ready.".to_string()
            } else {
                unavailable_detail.to_string()
            },
        },
        detail: if gsettings_available {
            "Accessibility preferences are ready for this desktop.".to_string()
        } else {
            "Desktop preference support is unavailable, so accessibility preferences are read-only in this runtime."
               .to_string()
        },
    }
}

#[cfg(test)]
fn test_input_status(
    gsettings_available: bool,
    keyboard_schema_available: bool,
    mouse_schema_available: bool,
    touchpad_schema_available: bool,
) -> InputStatus {
    let unavailable_detail = "Desktop preference support is unavailable, so keyboard, mouse, and trackpad preferences are unavailable in this runtime.";
    InputStatus {
        source: "goblins-os-core".to_string(),
        gsettings_available,
        keyboard: KeyboardInputStatus {
            schema_available: keyboard_schema_available,
            repeat: Some(true),
            delay_ms: Some(500),
            repeat_interval_ms: Some(30),
            remember_numlock_state: Some(false),
            detail: if gsettings_available && keyboard_schema_available {
                "Keyboard preferences are ready.".to_string()
            } else {
                unavailable_detail.to_string()
            },
        },
        mouse: MouseInputStatus {
            schema_available: mouse_schema_available,
            speed: Some(0.34),
            natural_scroll: Some(false),
            left_handed: Some(false),
            middle_click_emulation: Some(false),
            detail: if gsettings_available && mouse_schema_available {
                "Mouse preferences are ready.".to_string()
            } else {
                unavailable_detail.to_string()
            },
        },
        touchpad: TouchpadInputStatus {
            schema_available: touchpad_schema_available,
            speed: Some(-0.24),
            tap_to_click: Some(true),
            natural_scroll: Some(true),
            two_finger_scrolling_enabled: Some(true),
            disable_while_typing: Some(true),
            detail: if gsettings_available && touchpad_schema_available {
                "Trackpad preferences are ready.".to_string()
            } else {
                unavailable_detail.to_string()
            },
        },
        detail: if gsettings_available {
            "Keyboard, mouse, and trackpad preferences are ready for this desktop.".to_string()
        } else {
            unavailable_detail.to_string()
        },
    }
}

#[cfg(test)]
fn test_settings_system(
    bootc_image: &str,
    bootc_available: bool,
    systemctl_available: bool,
    network_manager_available: bool,
) -> SettingsSystemStatus {
    SettingsSystemStatus {
        generated_at: "test-generated-at".to_string(),
        source: "goblins-os-core".to_string(),
        session: SessionSettings {
            desktop: "goblins".to_string(),
            gui_platform: "gtk4".to_string(),
            shell_mode: "native".to_string(),
            core_url: "http://127.0.0.1:8787".to_string(),
        },
        identity: IdentitySettings {
            provider_configured: false,
            account_authenticated: false,
            session_path: "/var/lib/goblins-os/secrets/openai/session.json".to_string(),
        },
        local_account: Some(LocalAccountSummary {
            username: "joseph".to_string(),
            display_name: "Joseph Simo".to_string(),
            uid: Some(1000),
            gid: Some(1000),
            home: "/home/joseph".to_string(),
            shell: "/usr/bin/zsh".to_string(),
            hostname: "goblins-workstation".to_string(),
            account_type: "Administrator".to_string(),
            admin_groups: vec!["wheel".to_string()],
        }),
        storage: StorageSettings {
            model_dir: "/var/lib/goblins-os/models".to_string(),
            installer_state_dir: "/var/lib/goblins-os/installer".to_string(),
            session_state_dir: "/var/lib/goblins-os/session".to_string(),
            policy_state_dir: "/var/lib/goblins-os/policy".to_string(),
            resident_state_dir: "/var/lib/goblins-os/resident".to_string(),
            secrets_dir: "/var/lib/goblins-os/secrets/openai".to_string(),
        },
        services: ServiceSettings {
            bootc_image: bootc_image.to_string(),
            bootc_available,
            systemctl_available,
            network_manager_available,
        },
    }
}

#[cfg(test)]
fn test_system_services_status(
    manager_available: bool,
    service_states: &[&str],
) -> SystemServicesStatus {
    SystemServicesStatus {
        generated_at: "test-generated-at".to_string(),
        source: "goblins-os-core".to_string(),
        manager_available,
        unit_dir: "/usr/lib/systemd/system".to_string(),
        libexec_dir: "/usr/libexec/goblins-os".to_string(),
        services: service_states
            .iter()
            .enumerate()
            .map(|(index, state)| OsServiceStatus {
                id: format!("test-service-{index}"),
                label: format!("Test service {index}"),
                unit: format!("test-service-{index}.service"),
                binary: Some(format!("test-service-{index}")),
                expected_state: "active".to_string(),
                state: (*state).to_string(),
                unit_file: format!("/usr/lib/systemd/system/test-service-{index}.service"),
                unit_file_present: true,
                binary_path: Some(format!("/usr/libexec/goblins-os/test-service-{index}")),
                binary_present: Some(true),
                detail: format!("Service {index} reported {state}."),
            })
            .collect(),
    }
}

#[cfg(test)]
fn test_recovery_status(states: &[&str]) -> RecoveryStatus {
    let labels = [
        ("bootc-tooling", "System image"),
        ("system-services", "Recovery services"),
        ("secret-storage", "OpenAI secret storage"),
    ];

    RecoveryStatus {
        generated_at: "test-generated-at".to_string(),
        source: "goblins-os-core".to_string(),
        checks: labels
            .iter()
            .enumerate()
            .map(|(index, (id, label))| RecoveryCheck {
                id: (*id).to_string(),
                label: (*label).to_string(),
                state: states.get(index).copied().unwrap_or("waiting").to_string(),
                detail: format!("{label} detail."),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        accessibility_preference_outcome, app_usage_detail, appearance_outcome,
        appearance_scheme_detail, audio_control_outcome, audio_device_choice_detail,
        audio_device_unavailable_detail, audio_mute_detail, audio_mute_title, audio_target_kind,
        audio_volume_detail, audio_volume_label, audio_volume_title,
        background_picture_option_detail, background_shading_detail, bluetooth_adapter_detail,
        bluetooth_adapter_state_detail, bluetooth_power_detail, bluetooth_power_label,
        bluetooth_power_outcome, camera_access_detail, cleanup_temp_detail, cleanup_trash_detail,
        days_label, desktop_privacy_outcome, display_handles_detail, display_output_detail,
        display_output_title, engine_selection_success_copy, facility_state_is_ready,
        facility_state_label, facility_user_detail, input_feedback_sounds_detail,
        interface_sounds_detail, key_repeat_detail, local_account_identity_detail,
        local_account_type_detail, lock_screen_notifications_detail, magnifier_detail,
        microphone_access_detail, milliseconds_label, motion_preference_detail, night_light_detail,
        night_light_schedule_detail, night_light_temperature_label, normalized_appearance_theme,
        normalized_audio_volume, normalized_background_picture_option,
        normalized_background_shading, normalized_keyboard_delay,
        normalized_keyboard_repeat_interval, normalized_night_light_temperature,
        normalized_old_files_age, normalized_proxy_mode, normalized_text_scale,
        normalized_unit_speed, notification_app_children_detail, notification_app_enable_detail,
        notification_app_expand_detail, notification_app_lock_screen_detail,
        notification_app_lock_screen_details_detail, notification_app_sound_detail,
        notification_banners_detail, notification_preference_outcome, openai_account_detail,
        openai_login_destination_from_response, parse_http_endpoint, parse_http_response,
        pointer_speed_label, privacy_control_waiting_detail, privacy_state_label,
        proxy_auto_config_detail, proxy_endpoint_detail, proxy_ignore_hosts_detail,
        proxy_mode_detail, proxy_mode_outcome, recent_files_detail, screen_keyboard_detail,
        screen_reader_detail, sidebar_keyboard_target, sidebar_movement_from_key_name,
        sound_output_access_detail, sound_preference_outcome, sound_theme_detail,
        storage_capacity_detail, storage_capacity_percent_text, storage_used_fraction,
        storage_used_gb, text_scale_percent, usb_protection_detail, volume_boost_detail,
        wallpaper_color_detail, wallpaper_placement_outcome, wallpaper_shading_outcome,
        wallpaper_uri_detail, wifi_connect_outcome, AudioDeviceStatus, AudioEndpointStatus,
        BluetoothAdapterStatus, DisplayOutputStatus, DisplaysStatus, HttpEndpoint, HttpResponse,
        LocalAccountSummary, LocalModelInstallOutcome, OpenAIAuthStatus, SettingsPanel,
        SidebarMovement, SystemFacility,
    };

    #[test]
    fn parses_recovery_panel_argument() {
        assert!(matches!(
            SettingsPanel::from_args(["--panel=recovery".to_string()].into_iter()),
            SettingsPanel::Recovery
        ));
    }

    #[test]
    fn parses_policy_panel_argument() {
        assert!(matches!(
            SettingsPanel::from_args(["--panel=policy".to_string()].into_iter()),
            SettingsPanel::Policy
        ));
    }

    #[test]
    fn parses_expanded_panel_arguments_and_aliases() {
        assert!(matches!(
            SettingsPanel::from_args(["--panel=network".to_string()].into_iter()),
            SettingsPanel::Network
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=wifi".to_string()].into_iter()),
            SettingsPanel::Network
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=privacy".to_string()].into_iter()),
            SettingsPanel::PrivacyPermissions
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=night light".to_string()].into_iter()),
            SettingsPanel::Displays
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=screen reader".to_string()].into_iter()),
            SettingsPanel::Accessibility
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=updates".to_string()].into_iter()),
            SettingsPanel::UpdatesAbout
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=power".to_string()].into_iter()),
            SettingsPanel::PowerBattery
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=internet accounts".to_string()].into_iter()),
            SettingsPanel::OnlineAccounts
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=wacom".to_string()].into_iter()),
            SettingsPanel::DrawingTablet
        ));
    }

    #[test]
    fn exact_panel_slug_wins_over_earlier_alias() {
        assert!(matches!(
            SettingsPanel::from_args(["--panel=lock-screen".to_string()].into_iter()),
            SettingsPanel::LockScreen
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=lock screen".to_string()].into_iter()),
            SettingsPanel::LockScreen
        ));
        assert!(matches!(
            SettingsPanel::from_args(["--panel=alerts".to_string()].into_iter()),
            SettingsPanel::Notifications
        ));
    }

    #[test]
    fn expanded_settings_surface_has_expected_panel_count() {
        assert_eq!(SettingsPanel::ALL.len(), 38);
    }

    #[test]
    fn sidebar_metadata_covers_every_settings_panel() {
        for panel in SettingsPanel::ALL {
            assert!(!panel.sidebar_icon_name().is_empty());
            assert!(!panel.sidebar_owner_description().is_empty());
        }

        assert!(SettingsPanel::PowerBattery
            .sidebar_owner_description()
            .contains("built-in device controls"));
        assert!(SettingsPanel::Policy
            .sidebar_owner_description()
            .contains("OS-owned controls"));
        assert!(!SettingsPanel::Policy
            .sidebar_owner_description()
            .contains("core controls"));
        assert_eq!(
            SettingsPanel::Network.sidebar_icon_name(),
            "network-wireless-symbolic"
        );
        assert_eq!(
            SettingsPanel::Games.sidebar_icon_name(),
            "applications-games-symbolic"
        );
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn sidebar_groups_are_contiguous() {
        let mut closed_groups = std::collections::BTreeSet::new();
        let mut current_group = None;

        for panel in SettingsPanel::ALL {
            let group = panel.sidebar_group();
            if current_group == Some(group) {
                continue;
            }

            if let Some(previous) = current_group {
                closed_groups.insert(previous);
            }
            assert!(
                !closed_groups.contains(group),
                "sidebar group appears in multiple separated blocks: {group}"
            );
            current_group = Some(group);
        }
    }

    #[test]
    fn gnome_control_center_handoff_uses_fedora_panel_ids() {
        assert_eq!(
            SettingsPanel::Applications.gnome_control_center_panel(),
            Some("applications")
        );
        assert_eq!(
            SettingsPanel::Network.gnome_control_center_panel(),
            Some("wifi")
        );
        assert_eq!(
            SettingsPanel::NetworkServices.gnome_control_center_panel(),
            Some("network")
        );
        assert_eq!(
            SettingsPanel::Bluetooth.gnome_control_center_panel(),
            Some("bluetooth")
        );
        assert_eq!(
            SettingsPanel::MobileBroadband.gnome_control_center_panel(),
            Some("wwan")
        );
        assert_eq!(
            SettingsPanel::Sharing.gnome_control_center_panel(),
            Some("sharing")
        );
        assert_eq!(
            SettingsPanel::Displays.gnome_control_center_panel(),
            Some("display")
        );
        assert_eq!(
            SettingsPanel::ColorManagement.gnome_control_center_panel(),
            Some("color")
        );
        assert_eq!(
            SettingsPanel::Sound.gnome_control_center_panel(),
            Some("sound")
        );
        assert_eq!(
            SettingsPanel::Keyboard.gnome_control_center_panel(),
            Some("keyboard")
        );
        assert_eq!(
            SettingsPanel::MouseTrackpad.gnome_control_center_panel(),
            Some("mouse")
        );
        assert_eq!(
            SettingsPanel::DrawingTablet.gnome_control_center_panel(),
            Some("wacom")
        );
        assert_eq!(
            SettingsPanel::Accessibility.gnome_control_center_panel(),
            Some("universal-access")
        );
        assert_eq!(
            SettingsPanel::DesktopWallpaper.gnome_control_center_panel(),
            Some("background")
        );
        assert_eq!(
            SettingsPanel::Notifications.gnome_control_center_panel(),
            Some("notifications")
        );
        assert_eq!(
            SettingsPanel::SearchIndexing.gnome_control_center_panel(),
            Some("search")
        );
        assert_eq!(
            SettingsPanel::Multitasking.gnome_control_center_panel(),
            Some("multitasking")
        );
        assert_eq!(
            SettingsPanel::PowerBattery.gnome_control_center_panel(),
            Some("power")
        );
        assert_eq!(
            SettingsPanel::PrintersScanners.gnome_control_center_panel(),
            Some("printers")
        );
        assert_eq!(
            SettingsPanel::UsersAccounts.gnome_control_center_panel(),
            Some("system")
        );
        assert_eq!(
            SettingsPanel::OnlineAccounts.gnome_control_center_panel(),
            Some("online-accounts")
        );
        assert_eq!(
            SettingsPanel::PrivacyPermissions.gnome_control_center_panel(),
            Some("privacy")
        );
        assert_eq!(
            SettingsPanel::Wellbeing.gnome_control_center_panel(),
            Some("wellbeing")
        );
        assert_eq!(
            SettingsPanel::UpdatesAbout.gnome_control_center_panel(),
            Some("system")
        );
        assert_eq!(SettingsPanel::Models.gnome_control_center_panel(), None);
        assert_eq!(SettingsPanel::Policy.gnome_control_center_panel(), None);
        assert_eq!(SettingsPanel::Recovery.gnome_control_center_panel(), None);
    }

    #[test]
    fn every_fedora_gnome_control_center_panel_is_reachable() {
        let reachable: std::collections::BTreeSet<_> = SettingsPanel::ALL
            .iter()
            .filter_map(|panel| panel.gnome_control_center_panel())
            .collect();

        for expected in [
            "applications",
            "background",
            "bluetooth",
            "color",
            "datetime",
            "display",
            "keyboard",
            "mouse",
            "multitasking",
            "network",
            "wifi",
            "notifications",
            "online-accounts",
            "power",
            "printers",
            "privacy",
            "region",
            "search",
            "sharing",
            "sound",
            "system",
            "universal-access",
            "wacom",
            "wellbeing",
            "wwan",
        ] {
            assert!(
                reachable.contains(expected),
                "missing device settings panel: {expected}"
            );
        }
        assert_eq!(reachable.len(), 25);
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn device_settings_handoff_copy_names_real_owner_and_availability() {
        let ready = super::device_settings_summary_detail(
            SettingsPanel::PowerBattery,
            super::DeviceSettingsReadiness::Ready,
        );
        assert!(ready.contains("Power & Battery"));
        assert!(ready.contains("Power & Battery controls open"));
        assert!(!ready.contains('`'));

        let session_unavailable = super::device_settings_summary_detail(
            SettingsPanel::PrintersScanners,
            super::DeviceSettingsReadiness::IntegratedDesktopUnavailable,
        );
        assert!(session_unavailable.contains("Printers & Scanners"));
        assert!(session_unavailable.contains("open after the desktop finishes loading"));
        assert!(!session_unavailable.contains("full Goblins OS desktop session"));
        assert!(!session_unavailable.contains(super::GNOME_CONTROL_CENTER));
        let dependency_leak = format!("{} {}", "needs", "GN".to_string() + "OME");
        assert!(!session_unavailable.contains(&dependency_leak));

        let waiting = super::device_settings_summary_detail(
            SettingsPanel::OnlineAccounts,
            super::DeviceSettingsReadiness::WaitingForSession,
        );
        assert!(waiting.contains("Online Accounts"));
        assert!(waiting.contains("Checking Online Accounts controls"));
        assert!(!waiting.contains('`'));

        let unavailable = super::device_settings_summary_detail(
            SettingsPanel::PrintersScanners,
            super::DeviceSettingsReadiness::Unavailable,
        );
        assert!(unavailable.contains("Printers & Scanners"));
        assert!(unavailable.contains("not supported on this device"));
        assert!(!unavailable.contains(super::GNOME_CONTROL_CENTER));

        assert_eq!(
            super::device_settings_status_label(super::DeviceSettingsReadiness::Ready),
            "ready"
        );
        assert_eq!(
            super::device_settings_status_label(
                super::DeviceSettingsReadiness::IntegratedDesktopUnavailable
            ),
            "not ready"
        );
        assert_eq!(
            super::device_settings_status_label(super::DeviceSettingsReadiness::WaitingForSession),
            "waiting"
        );
        assert_eq!(
            super::device_settings_status_label(super::DeviceSettingsReadiness::Unavailable),
            "not supported"
        );
        assert_eq!(
            super::device_settings_action_label(super::DeviceSettingsReadiness::Ready),
            "Open"
        );
        assert_eq!(
            super::device_settings_action_label(
                super::DeviceSettingsReadiness::IntegratedDesktopUnavailable
            ),
            "Open"
        );
        assert_eq!(super::native_handoff_status_label(true), "ready");
        assert_eq!(super::native_handoff_status_label(false), "not included");
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn device_settings_configuration_source_keeps_controls_truthful() {
        let source = super::device_settings_configuration_source(SettingsPanel::OnlineAccounts);
        assert!(source.contains("Online Accounts"));
        assert!(source.contains("controls use built-in device services"));
        assert!(source.contains("account settings"));
        let system_managed = ["system", "managed"].join("-");
        let dependency_leak = format!("{} {}", "needs", "GN".to_string() + "OME");
        assert!(!source.contains(&system_managed));
        assert!(!source.contains(&dependency_leak));
        assert!(!source.contains("fake controls"));
        assert!(!source.contains('`'));

        let scope = super::device_settings_capability_scope(SettingsPanel::DrawingTablet);
        assert!(scope.contains("Pen"));
        assert!(scope.contains("tablet settings"));
        assert!(!scope.contains(&system_managed));

        let power_capabilities = super::device_settings_capabilities(SettingsPanel::PowerBattery);
        assert_eq!(power_capabilities.len(), 4);
        assert!(power_capabilities
            .iter()
            .any(|capability| capability.title == "Power mode"));

        let online_account_capabilities =
            super::device_settings_capabilities(SettingsPanel::OnlineAccounts);
        assert!(online_account_capabilities
            .iter()
            .any(|capability| capability.title == "Account providers"));

        let wellbeing_capabilities = super::device_settings_capabilities(SettingsPanel::Wellbeing);
        assert!(wellbeing_capabilities
            .iter()
            .any(|capability| capability.title == "Break reminders"));

        for panel in SettingsPanel::ALL {
            for capability in super::device_settings_capabilities(panel) {
                let detail = capability.detail.to_ascii_lowercase();
                assert!(
                    !detail.contains("fake")
                        && !detail.contains("sample")
                        && !detail.contains("mock"),
                    "{} capability '{}' leaks placeholder copy: {}",
                    panel.as_str(),
                    capability.title,
                    capability.detail
                );
            }
            if panel.gnome_control_center_panel().is_some() {
                assert_eq!(
                    super::device_settings_capabilities(panel).len(),
                    4,
                    "{} should describe system coverage",
                    panel.as_str()
                );
            }
        }

        assert!(super::device_settings_capabilities(SettingsPanel::Models).is_empty());

        let applications_handoff = super::device_native_handoff_spec(SettingsPanel::Applications)
            .expect("Applications should hand off app management to Software");
        assert_eq!(applications_handoff.title, "Software");
        assert_eq!(applications_handoff.app_label, "Software");
        assert_eq!(applications_handoff.command, super::GNOME_SOFTWARE);
        assert!(applications_handoff
            .purpose
            .contains("installed applications"));
        assert!(super::native_app_handoff_detail(
            applications_handoff.app_label,
            applications_handoff.purpose,
            true
        )
        .contains("Software lets you"));
        assert!(super::device_native_handoff_spec(SettingsPanel::PowerBattery).is_none());
    }

    #[test]
    fn settings_copy_avoids_debug_panel_language() {
        let source = include_str!("main.rs");
        let build_leak = ["Read-only", " in this build"].join("");
        assert!(!source.contains(&build_leak));
        assert!(source.contains("System health"));
        assert!(source.contains("Local services"));
        assert!(source.contains("through local OS services"));
        assert!(source.contains("Goblin wake word"));
        assert!(source.contains("Ask Goblin"));
        for forbidden in [
            ["needs ", "gnome"].join(""),
            ["shell", "-managed"].join(""),
            ["shell", "-provided"].join(""),
            ["shell ", "chrome"].join(""),
            ["desktop ", "chrome"].join(""),
            ["core", " health"].join(""),
            ["core", " status"].join(""),
            ["core", ": "].join(""),
            ["goblins os ", "core"].join(""),
            ["the os ", "core"].join(""),
        ] {
            assert!(
                !source.to_ascii_lowercase().contains(&forbidden),
                "Settings copy must not expose implementation wording: {forbidden}"
            );
        }
        for app_name in [
            "Disk Usage Analyzer",
            "Disks",
            "Software",
            "System Monitor",
            "Logs",
        ] {
            let launcher_title = format!("\"Open {app_name}\"");
            assert!(
                !source.contains(&launcher_title),
                "{app_name} should be titled as an integrated tool row"
            );
        }
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn device_handoff_copy_names_target_panel_and_disabled_reason() {
        let ready = super::device_settings_handoff_detail(
            SettingsPanel::Network,
            super::DeviceSettingsReadiness::Ready,
        );
        assert!(ready.contains("Open detailed Network controls"));
        assert!(!ready.contains("Open settings"));
        assert!(!ready.contains("fake controls"));
        assert!(!ready.contains('`'));

        let integrated = super::device_settings_integrated_detail(SettingsPanel::Network);
        assert!(integrated.contains("Open detailed Network controls"));
        assert!(integrated.contains("Status shown here comes from Goblins OS"));

        let session_unavailable = super::device_settings_handoff_detail(
            SettingsPanel::PowerBattery,
            super::DeviceSettingsReadiness::IntegratedDesktopUnavailable,
        );
        assert!(session_unavailable.contains("open after the desktop finishes loading"));
        assert!(!session_unavailable.contains("full Goblins OS desktop session"));
        assert!(session_unavailable.contains("Power & Battery"));
        assert!(!session_unavailable.contains(super::GNOME_CONTROL_CENTER));
        let dependency_leak = format!("{} {}", "needs", "GN".to_string() + "OME");
        assert!(!session_unavailable.contains(&dependency_leak));

        let waiting = super::device_settings_handoff_detail(
            SettingsPanel::PowerBattery,
            super::DeviceSettingsReadiness::WaitingForSession,
        );
        assert!(waiting.contains("Checking Power & Battery controls"));
        assert!(waiting.contains("Power & Battery"));

        let unavailable = super::device_settings_handoff_detail(
            SettingsPanel::PowerBattery,
            super::DeviceSettingsReadiness::Unavailable,
        );
        assert!(unavailable.contains("not supported on this device"));
        assert!(unavailable.contains("Power & Battery"));
        assert!(!unavailable.contains(super::GNOME_CONTROL_CENTER));

        assert_eq!(
            super::device_settings_handoff_accessibility(
                SettingsPanel::Network,
                super::DeviceSettingsReadiness::Ready
            ),
            "Manage Network controls."
        );
        assert_eq!(
            super::device_settings_handoff_accessibility(
                SettingsPanel::Network,
                super::DeviceSettingsReadiness::IntegratedDesktopUnavailable
            ),
            "Network controls open after the desktop finishes loading."
        );
        assert_eq!(
            super::device_settings_handoff_accessibility(
                SettingsPanel::Network,
                super::DeviceSettingsReadiness::Unavailable
            ),
            "Device controls are not supported on this device."
        );
    }

    #[test]
    fn settings_default_window_size_preserves_first_screen_status() {
        assert_eq!(super::SETTINGS_DEFAULT_WIDTH, 1055);
        assert_eq!(super::SETTINGS_DEFAULT_HEIGHT, 840);
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn settings_css_pins_inter_typography() {
        let css = super::GOBLINS_OS_SETTINGS_CSS;
        assert!(
            css.contains("font-family: \"Inter\", \"Inter Display\", \"Noto Sans\", sans-serif")
        );
        assert!(css.contains(".gos-settings-root label"));
        assert!(
            !css.contains(".gos-settings-root.gos-"),
            "Settings child selectors must descend from the root; same-widget root selectors miss child widgets"
        );
        assert!(css.contains(".gos-device-handoff-row"));
        assert!(css.contains(".gos-settings-root .gos-kicker {\n  color: @gos_label_secondary;"));
        let settings_kicker_block = css
            .split(".gos-settings-root .gos-kicker {")
            .nth(1)
            .and_then(|block| block.split(".gos-section-title").next())
            .expect("settings kicker CSS block is present");
        assert!(settings_kicker_block.contains("text-transform: none;"));
        assert!(!settings_kicker_block.contains("uppercase"));
        let nav_group_block = css
            .split(".gos-nav-group {")
            .nth(1)
            .and_then(|block| block.split(".gos-settings-root .gos-side-nav").next())
            .expect("settings nav group CSS block is present");
        assert!(nav_group_block.contains("font-size: 11px;"));
        assert!(nav_group_block.contains("text-transform: none;"));
        assert!(!nav_group_block.contains("uppercase"));
        assert!(css.contains(".gos-side-nav-list {\n  padding: 0 3px 12px;"));
        // Colored category tiles (the macOS-kit "system settings" signature):
        // the tint classes are wired and the glyph reads white on the tile.
        assert!(css.contains(".gos-side-icon-well.gos-tint-blue"));
        assert!(css.contains(".gos-side-icon-well.gos-tint-graphite"));
        assert!(css.contains("color: @gos_on_tint;"));
        // Selection stays a calm neutral fill so the tile carries the only hue.
        assert!(css.contains(".gos-settings-root .gos-side-nav.is-current .gos-side-icon-well"));
        assert!(css.contains(".gos-settings-root .gos-overview-native-settings"));
        assert!(css.contains(".gos-settings-root {\n  margin: 16px;\n  padding: 0;\n  border: 1px solid @gos_hairline;\n  border-radius: 16px;"));
        assert!(css.contains(".gos-settings-root .gos-settings-top {\n  min-height: 50px;"));
        assert!(css.contains(".gos-settings-root .gos-device-context-grid"));
        assert!(css.contains(".gos-settings-root .gos-device-context-tile"));
        assert!(css.contains(".gos-settings-root .gos-device-context-source"));
        assert!(css.contains(".gos-settings-root .gos-device-capability-grid"));
        assert!(css.contains(".gos-settings-root .gos-device-capability-tile"));
        assert!(css.contains(".gos-settings-root .gos-preference-group {"));
        assert!(css.contains(".gos-settings-root .gos-preference-group .gos-row {"));
        assert!(css.contains(".gos-settings-root .gos-preference-group .gos-row:first-child"));
        assert!(css.contains(".gos-settings-root .gos-preference-group .gos-row:last-child"));
        assert!(css.contains(".gos-handoff-controls"));
        assert!(css.contains(".gos-device-handoff-action:disabled"));
        assert!(css.contains(".gos-settings-root button:disabled"));
        assert!(css.contains("opacity: 1;"));
        assert!(css.contains("border-color: alpha(@gos_hairline_strong, 0.84);"));
        assert!(css.contains("0 0 0 1px @gos_primary_border inset"));
        assert!(css.contains(".gos-settings-root .gos-side-nav:focus"));
        assert!(!css.contains("margin-left: auto"));
        assert!(css.contains(
            ".gos-settings-root .gos-main-panel {\n  min-width: 584px;\n  padding: 24px 32px 28px;"
        ));
        assert!(css.contains(".gos-settings-root .gos-side-panel {\n  min-width: 244px;"));
        assert!(css.contains(".gos-settings-root .gos-side-scroll scrollbar.vertical slider"));
        assert!(css.contains(".gos-settings-root .gos-side-panel .gos-search-empty"));
        assert!(css.contains(".gos-search-entry {\n  min-height: 33px;"));
        assert!(css.contains(".gos-settings-root .gos-side-nav {\n  min-height: 30px;"));
        assert!(css.contains(".gos-settings-root .gos-health-row {\n  min-height: 54px;"));
        assert!(
            css.contains(
                "background: linear-gradient(180deg, alpha(@gos_surface, 0.92), alpha(@gos_surface_sunken, 0.66));"
            )
        );
        assert!(css.contains(".gos-settings-root .gos-status-pill {\n  padding: 4px 9px;"));
        assert!(css.contains(".gos-settings-root .gos-ready {\n  color: @gos_ready;"));
        assert!(css.contains("background: @gos_ready_soft;"));
        assert!(css.contains("border-color: alpha(@gos_ready, 0.34);"));
        assert!(
            css.contains(".gos-settings-root .gos-status-quiet {\n  color: @gos_label_secondary;")
        );
        assert!(css.contains("background: @gos_fill_secondary;"));
        assert!(css.contains(".gos-status-value {\n  margin-left: 12px;"));
        assert!(css.contains(".gos-settings-root .gos-waiting {\n  color: @gos_waiting;"));
        assert!(css.contains("background: alpha(@gos_waiting, 0.12);"));
        assert!(!css.contains(".gos-status-pill {\n  padding: 6px 10px;"));
        let status_pill_block = css
            .split(".gos-settings-root .gos-status-pill {")
            .nth(1)
            .and_then(|block| block.split(".gos-settings-root .gos-ready").next())
            .expect("settings status pill CSS block is present");
        assert!(!status_pill_block.contains("text-transform"));
        assert!(css.contains("border-radius: 999px;"));
        assert!(css.contains(".gos-device-handoff-action {\n  min-width: 86px;"));
        assert!(!css.contains(".gos-device-handoff-action {\n  min-width: 172px;"));
        assert!(!css.contains("padding: 26px 30px 30px;"));
        assert!(!css.contains("font-weight: 700"));
        assert!(!css.contains("letter-spacing: -"));
        assert!(!css.contains("letter-spacing: 1"));
        assert!(css.contains("line-height: 19px;"));
        assert!(!css.contains("line-height: 1.35;"));
        for forbidden in ["SFPro", "SF Pro", "San Francisco"] {
            assert!(
                !css.contains(forbidden),
                "settings CSS must not request Apple font naming: {forbidden}"
            );
        }
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn ordinary_settings_statuses_are_visually_quiet() {
        for state in [
            "active",
            "auto",
            "available",
            "configured",
            "full motion",
            "Inter",
            "left-aligned",
            "local only",
            "none",
            "off",
            "not installed",
            "online",
            "personal",
            "ready",
            "ready to download",
            "signed in",
            "125%",
        ] {
            assert!(
                super::settings_status_state_is_quiet(state),
                "{state} should not read like an alert badge"
            );
        }

        for state in ["blocked", "offline", "unavailable", "unknown", "waiting"] {
            assert!(
                !super::settings_status_state_is_quiet(state),
                "{state} should remain visually explicit"
            );
        }
    }

    #[test]
    fn settings_status_pill_labels_are_user_facing_not_backend_words() {
        assert_eq!(
            super::settings_status_display_label("unavailable"),
            "Not ready"
        );
        assert_eq!(
            super::settings_status_display_label("inactive-or-unavailable"),
            "Inactive"
        );
        assert_eq!(
            super::settings_status_display_label("not available"),
            "Not supported"
        );
        assert_eq!(
            super::settings_status_display_label("service-waiting"),
            "Starting"
        );
        assert_eq!(
            super::settings_status_display_label("adapter-ready"),
            "Ready"
        );
        assert_eq!(
            super::settings_status_display_label("adapter-waiting"),
            "No adapter"
        );
        assert_eq!(
            super::settings_status_display_label("device-waiting"),
            "No device"
        );
        assert_eq!(
            super::settings_status_display_label("waiting-for-manifest"),
            "Waiting for provider manifest"
        );
        assert_eq!(
            super::settings_status_display_label("permission-gated"),
            "Needs permission"
        );
        for raw_state in [
            "service-waiting",
            "adapter-ready",
            "adapter-waiting",
            "device-waiting",
            "waiting-for-manifest",
            "permission-gated",
        ] {
            let label = super::settings_status_display_label(raw_state);
            assert!(!label.contains('-'), "{raw_state} leaked as {label}");
        }
        assert_eq!(super::settings_status_display_label("125%"), "125%");
    }

    #[test]
    fn settings_detail_copy_polishes_backend_availability_language() {
        assert_eq!(
            super::settings_detail_display_copy("Bluetooth support is unavailable."),
            "Bluetooth support is not ready."
        );
        assert_eq!(
            super::settings_detail_display_copy(
                "Recovery actions stay unavailable until checks are ready."
            ),
            "Recovery actions stay disabled until checks are ready."
        );
        assert_eq!(
            super::settings_detail_display_copy(
                "Device connections are unavailable until Bluetooth is on."
            ),
            "Device connections are not ready until Bluetooth is on."
        );
        assert_eq!(
            super::settings_detail_display_copy("Proxy mode is not available in this session."),
            "Proxy mode is not ready in this session."
        );
        assert_eq!(
            super::settings_detail_display_copy("Theme unavailable"),
            "Theme not ready"
        );
        assert_eq!(
            super::settings_detail_display_copy("NetworkManager and GSettings are unavailable."),
            "Network service and desktop preferences are not ready."
        );
        assert_eq!(
            super::settings_detail_display_copy("WirePlumber, PipeWire, gdbus, and xrandr failed."),
            "Audio routing, audio service, desktop bridge, and display fallback failed."
        );
        assert_eq!(
            super::settings_detail_display_copy("gnome-control-center is not available."),
            "device controls are not ready."
        );
        assert_eq!(
            super::settings_detail_display_copy("Relay routes are not configured yet."),
            "Assistant routes are not set up yet."
        );
        assert_eq!(
            super::settings_detail_display_copy(&format!(
                "{} is waiting; local relay is not configured.",
                ["Codex", "resident"].join(" ")
            )),
            "Goblins AI runtime is waiting; local assistant route is not set up."
        );
        assert_eq!(
            super::settings_detail_display_copy("Desktop status is not available yet."),
            "Desktop status is not ready yet."
        );
        assert_eq!(
            super::settings_detail_display_copy("Checking privacy controls."),
            "Checking privacy controls."
        );
    }

    #[test]
    fn sidebar_scroll_target_keeps_selected_row_in_context() {
        let target = super::sidebar_scroll_target(520.0, 554.0, 0.0, 420.0, 0.0, 1000.0)
            .expect("offscreen row should request scroll");
        let old_bottom_pin = 554.0 - 420.0;
        assert!(target > old_bottom_pin + 200.0);
        assert!(520.0 >= target + 64.0);
        assert!(554.0 <= target + 420.0 - 8.0);

        assert!(super::sidebar_scroll_target(180.0, 214.0, 100.0, 420.0, 0.0, 1000.0).is_none());

        let clamped = super::sidebar_scroll_target(40.0, 74.0, 220.0, 420.0, 0.0, 1000.0).unwrap();
        assert_eq!(clamped, 0.0);
    }

    #[test]
    fn sidebar_keyboard_navigation_targets_visible_rows() {
        let visible = [
            SettingsPanel::Overview,
            SettingsPanel::Appearance,
            SettingsPanel::Network,
        ];

        assert!(matches!(
            sidebar_keyboard_target(
                &visible,
                SettingsPanel::Appearance,
                SidebarMovement::Previous
            ),
            Some(SettingsPanel::Overview)
        ));
        assert!(matches!(
            sidebar_keyboard_target(&visible, SettingsPanel::Appearance, SidebarMovement::Next),
            Some(SettingsPanel::Network)
        ));
        assert!(matches!(
            sidebar_keyboard_target(&visible, SettingsPanel::Network, SidebarMovement::Next),
            Some(SettingsPanel::Network)
        ));
        assert!(matches!(
            sidebar_keyboard_target(&visible, SettingsPanel::Network, SidebarMovement::First),
            Some(SettingsPanel::Overview)
        ));
        assert!(matches!(
            sidebar_keyboard_target(&visible, SettingsPanel::Overview, SidebarMovement::Last),
            Some(SettingsPanel::Network)
        ));
        assert!(matches!(
            sidebar_keyboard_target(&visible, SettingsPanel::Developer, SidebarMovement::Next),
            Some(SettingsPanel::Appearance)
        ));
        assert!(
            sidebar_keyboard_target(&[], SettingsPanel::Overview, SidebarMovement::Next).is_none()
        );
    }

    #[test]
    fn sidebar_keyboard_navigation_maps_standard_keys() {
        assert!(matches!(
            sidebar_movement_from_key_name("up"),
            Some(SidebarMovement::Previous)
        ));
        assert!(matches!(
            sidebar_movement_from_key_name("down"),
            Some(SidebarMovement::Next)
        ));
        assert!(matches!(
            sidebar_movement_from_key_name("home"),
            Some(SidebarMovement::First)
        ));
        assert!(matches!(
            sidebar_movement_from_key_name("end"),
            Some(SidebarMovement::Last)
        ));
        assert!(sidebar_movement_from_key_name("f").is_none());
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn sidebar_accessible_descriptions_include_active_search_match() {
        let query = super::settings_search_query("wi fi");
        let filtered =
            super::sidebar_accessible_description_for_search(SettingsPanel::Network, false, &query);
        assert!(filtered.contains("Connectivity, active connection, and Wi-Fi management status."));
        assert!(filtered.contains("Goblins OS shows live status here"));
        assert!(filtered.contains("Matches: Wi-Fi networks"));

        let selected =
            super::sidebar_accessible_description_for_search(SettingsPanel::Network, true, &query);
        assert!(selected.starts_with("Current category."));
        assert!(selected.contains("Matches: Wi-Fi networks"));

        let ordinary =
            super::sidebar_accessible_description_for_search(SettingsPanel::Network, false, "");
        assert!(!ordinary.contains("Matches:"));

        let unrelated =
            super::sidebar_accessible_description_for_search(SettingsPanel::Sound, false, &query);
        assert!(!unrelated.contains("Matches:"));
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn settings_search_matches_aliases_and_panel_copy() {
        let wifi_query = super::settings_search_query("wi fi");
        let private_query = super::settings_search_query("private mode");
        let update_query = super::settings_search_query("boot image");
        let screen_reader_query = super::settings_search_query("screen reader");
        let screen_time_query = super::settings_search_query("screen time");
        let diagnostics_query = super::settings_search_query("developer logs");
        let openai_key_query = super::settings_search_query("openai api key");
        let volume_boost_query = super::settings_search_query("volume boost");
        let wallpaper_query = super::settings_search_query("wallpaper placement");
        let repeat_delay_query = super::settings_search_query("repeat delay");
        let num_lock_query = super::settings_search_query("remember num lock");
        let magnifier_query = super::settings_search_query("magnifier");
        let color_temperature_query = super::settings_search_query("color temperature");
        let sticky_keys_query = super::settings_search_query("sticky keys");
        let file_indexing_query = super::settings_search_query("file indexing");
        let games_query = super::settings_search_query("gamescope mangohud");

        assert!(super::settings_search_matches(
            SettingsPanel::Network,
            &wifi_query
        ));
        assert!(!super::settings_search_matches(
            SettingsPanel::Notifications,
            &wifi_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::PrivacyPermissions,
            &private_query
        ));
        assert!(!super::settings_search_matches(
            SettingsPanel::UsersAccounts,
            &private_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::UpdatesAbout,
            &update_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Accessibility,
            &screen_reader_query
        ));
        assert!(!super::settings_search_matches(
            SettingsPanel::Bluetooth,
            &private_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Wellbeing,
            &screen_time_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Developer,
            &diagnostics_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Models,
            &openai_key_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Sound,
            &volume_boost_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::DesktopWallpaper,
            &wallpaper_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Keyboard,
            &repeat_delay_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Keyboard,
            &num_lock_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Accessibility,
            &magnifier_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Displays,
            &color_temperature_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Accessibility,
            &sticky_keys_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::SearchIndexing,
            &file_indexing_query
        ));
        assert!(super::settings_search_matches(
            SettingsPanel::Games,
            &games_query
        ));
        assert!(!super::settings_search_matches(
            SettingsPanel::Policy,
            &openai_key_query
        ));
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn settings_search_previews_matching_settings() {
        let night_light_query = super::settings_search_query("night light");
        assert_eq!(
            super::settings_search_preview(SettingsPanel::Displays, &night_light_query).as_deref(),
            Some("Matches: Night Light, Automatic schedule, Color temperature")
        );
        assert!(super::settings_search_preview(SettingsPanel::Sound, &night_light_query).is_none());

        let storage_query = super::settings_search_query("storage cleanup");
        assert_eq!(
            super::settings_search_preview(SettingsPanel::Storage, &storage_query).as_deref(),
            Some("Matches: Cleanup")
        );
        assert_eq!(
            super::settings_search_matched_setting_count(&storage_query),
            1
        );

        let schedule_query = super::settings_search_query("automatic schedule");
        assert_eq!(
            super::settings_search_preview(SettingsPanel::Displays, &schedule_query).as_deref(),
            Some("Matches: Automatic schedule")
        );
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn settings_search_terms_do_not_use_apple_product_vocabulary() {
        let indexed = super::SETTINGS_SEARCH_ITEMS
            .iter()
            .flat_map(|item| std::iter::once(item.title).chain(item.terms.iter().copied()))
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();

        for forbidden in ["spotlight", "voiceover", "siri", "icloud"] {
            assert!(
                !indexed.contains(forbidden),
                "search terms must stay Goblins OS/Fedora-native: {forbidden}"
            );
        }
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn settings_search_reports_result_counts_and_enter_target() {
        let empty_query = super::settings_search_query("");
        assert_eq!(
            super::settings_search_visible_panels(&empty_query).len(),
            SettingsPanel::ALL.len()
        );
        assert!(super::settings_search_first_match(&empty_query).is_none());
        assert!(
            super::settings_search_status_text(&empty_query, SettingsPanel::ALL.len()).is_none()
        );
        assert_eq!(
            super::settings_search_accessible_description(&empty_query, SettingsPanel::ALL.len()),
            "Search settings, controls, and categories. Type to narrow results."
        );

        let wifi_query = super::settings_search_query("wi fi");
        let wifi_results = super::settings_search_visible_panels(&wifi_query);
        assert_eq!(wifi_results.len(), 2);
        assert!(wifi_results.contains(&SettingsPanel::MenuBarControlCenter));
        assert!(wifi_results.contains(&SettingsPanel::Network));
        assert!(matches!(
            super::settings_search_first_match(&wifi_query),
            Some(SettingsPanel::Network)
        ));
        assert_eq!(
            super::settings_search_status_text(&wifi_query, wifi_results.len()).as_deref(),
            Some("3 matching settings in 2 categories")
        );
        assert_eq!(
            super::settings_search_accessible_description(&wifi_query, wifi_results.len()),
            "Search settings, controls, and categories. 3 matching settings in 2 categories. Press Enter to open the first visible result. Press Escape to clear search."
        );

        let missing_query = super::settings_search_query("definitely missing category");
        let missing_results = super::settings_search_visible_panels(&missing_query);
        assert!(missing_results.is_empty());
        assert!(super::settings_search_first_match(&missing_query).is_none());
        assert_eq!(
            super::settings_search_status_text(&missing_query, missing_results.len()).as_deref(),
            Some("No matching settings")
        );
        assert_eq!(
            super::settings_search_accessible_description(&missing_query, missing_results.len()),
            "Search settings, controls, and categories. No matching settings. Press Escape to clear search."
        );
    }

    #[test]
    fn parses_default_panel_argument() {
        assert!(matches!(
            SettingsPanel::from_args(std::iter::empty()),
            SettingsPanel::Overview
        ));
    }

    #[test]
    fn parses_local_core_endpoint() {
        assert_eq!(
            parse_http_endpoint("http://127.0.0.1:8787"),
            Some(HttpEndpoint {
                host: "127.0.0.1".to_string(),
                port: 8787,
            })
        );
    }

    #[test]
    fn parses_core_json_response() {
        let response = parse_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            super::header_value(&response.headers, "content-type"),
            Some("application/json")
        );
        assert_eq!(response.body, br#"{"ok":true}"#);
    }

    #[test]
    fn parses_openai_account_login_redirect() {
        let response = parse_http_response(
            b"HTTP/1.1 302 Found\r\nLocation: https://auth.openai.example/start\r\n\r\n",
        )
        .unwrap();

        assert_eq!(
            openai_login_destination_from_response(&response),
            Ok("https://auth.openai.example/start".to_string())
        );
        assert_eq!(
            openai_login_destination_from_response(&HttpResponse {
                status: 302,
                headers: Vec::new(),
                body: Vec::new(),
            }),
            Err(super::CoreFetchError::Malformed)
        );
    }

    #[test]
    fn local_account_copy_reports_core_sourced_identity_without_fake_admin_state() {
        let account = LocalAccountSummary {
            username: "joseph".to_string(),
            display_name: "Joseph Simo".to_string(),
            uid: Some(1000),
            gid: Some(1000),
            home: "/home/joseph".to_string(),
            shell: "/usr/bin/zsh".to_string(),
            hostname: "goblins-workstation".to_string(),
            account_type: "Administrator".to_string(),
            admin_groups: vec!["wheel".to_string()],
        };

        assert_eq!(account.username, "joseph");
        assert_eq!(account.display_name, "Joseph Simo");
        assert_eq!(account.uid, Some(1000));
        assert_eq!(account.gid, Some(1000));
        assert_eq!(account.home, "/home/joseph");
        assert_eq!(account.shell, "/usr/bin/zsh");
        assert_eq!(account.hostname, "goblins-workstation");
        assert_eq!(account.account_type, "Administrator");
        assert_eq!(account.admin_groups, vec!["wheel"]);
        assert_eq!(
            local_account_identity_detail(&account),
            "Joseph Simo · joseph · uid 1000 · gid 1000"
        );
        assert_eq!(
            local_account_type_detail(&account),
            "Administrator via wheel."
        );
        assert_eq!(
            super::local_account_summary_detail(&account),
            "Joseph Simo (joseph) on goblins-workstation. Home /home/joseph"
        );
    }

    #[test]
    fn local_account_copy_falls_back_truthfully_when_core_lacks_ids() {
        let account = LocalAccountSummary {
            username: "unknown".to_string(),
            display_name: "unknown".to_string(),
            uid: None,
            gid: None,
            home: "/home/session-user".to_string(),
            shell: "Unknown login shell".to_string(),
            hostname: "Unknown computer".to_string(),
            account_type: "Unknown".to_string(),
            admin_groups: Vec::new(),
        };

        assert_eq!(account.username, "unknown");
        assert_eq!(account.display_name, "unknown");
        assert_eq!(account.uid, None);
        assert_eq!(account.gid, None);
        assert_eq!(account.home, "/home/session-user");
        assert_eq!(account.shell, "Unknown login shell");
        assert_eq!(account.hostname, "Unknown computer");
        assert_eq!(account.account_type, "Unknown");
        assert!(local_account_identity_detail(&account).contains("uid/gid not reported"));
        assert!(local_account_type_detail(&account).contains("not reported"));
    }

    #[test]
    fn account_summary_tiles_do_not_invent_signin_or_admin_state() {
        let system = super::test_settings_system("localhost/goblins-os:test", true, true, true);
        let local = super::local_account_summary_spec(Some(&system));
        assert_eq!(local.title, "Local account");
        assert_eq!(local.state, "Administrator");
        assert!(local.ready);
        assert!(local.detail.contains("Joseph Simo"));
        assert!(local.detail.contains("/home/joseph"));

        let openai = super::openai_account_summary_spec(None, Some(&system));
        assert_eq!(openai.title, "OpenAI account");
        assert_eq!(openai.state, "local only");
        assert!(openai.ready);
        assert!(openai.detail.contains("local-only"));

        let codex = super::codex_account_summary_spec(Some(&super::CodexStatus {
            installed: true,
            authenticated: false,
            detail: "Codex account support is ready but not signed in.".to_string(),
        }));
        assert_eq!(codex.title, "Codex");
        assert_eq!(codex.state, "sign in");
        assert!(!codex.ready);
        assert!(codex.detail.contains("not signed in"));
    }

    #[test]
    fn privacy_state_label_tracks_current_state() {
        assert_eq!(privacy_state_label(false), "Private mode · off");
        assert_eq!(privacy_state_label(true), "Private mode · on");
        assert!(privacy_control_waiting_detail().starts_with("Disabled:"));
        assert!(privacy_control_waiting_detail().contains("privacy status"));
    }

    #[test]
    fn facility_state_copy_is_reader_friendly() {
        let facility = SystemFacility {
            id: "desktop-portals".to_string(),
            label: "Desktop portals".to_string(),
            state: "ready".to_string(),
            detail: "Portals provide native app integration.".to_string(),
            evidence: vec![
                "xdg-desktop-portal:present".to_string(),
                "xdg-desktop-portal.service:active".to_string(),
            ],
        };

        assert_eq!(facility_state_label("ready"), "Available");
        assert_eq!(facility_state_label("waiting"), "Waiting");
        assert_eq!(facility_state_label("unavailable"), "Not ready");
        assert_eq!(facility_state_label("missing"), "Not installed");
        assert_eq!(facility_state_label("failed"), "Needs attention");
        assert!(facility_state_is_ready("ready"));
        assert!(!facility_state_is_ready("waiting"));
        let detail = facility_user_detail(&facility);
        assert!(detail.contains("Portals provide native app integration"));
        // Raw probe evidence is internal and must never reach user copy.
        assert!(!detail.contains("Evidence:"));
        assert!(!detail.contains("xdg-desktop-portal:present"));
    }

    #[test]
    fn desktop_privacy_details_are_truthful_and_bounded() {
        assert!(recent_files_detail(true).contains("recent-files list"));
        assert!(recent_files_detail(false).contains("should not keep"));
        assert!(app_usage_detail(true).contains("remember application usage"));
        assert!(app_usage_detail(false).contains("should not be monitored"));
        assert!(cleanup_trash_detail(true).contains("older than the cleanup age"));
        assert!(cleanup_trash_detail(false).contains("kept until"));
        assert!(cleanup_temp_detail(true).contains("Temporary files"));
        assert!(cleanup_temp_detail(false).contains("not removed"));
        assert!(microphone_access_detail(true).contains("should not use the microphone"));
        assert!(microphone_access_detail(false).contains("may request microphone"));
        assert!(camera_access_detail(true).contains("should not use the camera"));
        assert!(sound_output_access_detail(true).contains("should not produce sound"));
        assert!(usb_protection_detail(true).contains("USBGuard"));
        assert!(usb_protection_detail(false).contains("off"));
        assert_eq!(normalized_old_files_age(0), 1);
        assert_eq!(normalized_old_files_age(30), 30);
        assert_eq!(normalized_old_files_age(500), 365);
        assert_eq!(days_label(1.0), "1 day");
        assert_eq!(days_label(30.4), "30 days");
    }

    #[test]
    fn privacy_summary_tiles_use_private_mode_and_desktop_state() {
        let privacy = super::test_privacy_status(true, true, true);

        let private = super::privacy_private_mode_summary_spec(Some(&privacy));
        assert_eq!(private.title, "Private mode");
        assert_eq!(private.state, "private");
        assert!(private.ready);
        assert!(private.detail.contains("Local GPT-OSS"));

        let schema = super::desktop_privacy_schema_summary_spec(Some(&privacy));
        assert_eq!(schema.state, "available");
        assert!(schema.ready);
        assert!(schema.detail.contains("ready for this desktop"));

        let device = super::privacy_device_access_summary_spec(Some(&privacy));
        assert_eq!(device.state, "2 blocked");
        assert!(device.ready);
        assert!(device.detail.contains("Microphone blocked"));
        assert!(device.detail.contains("Camera allowed"));
        assert!(device.detail.contains("USB protected"));

        let cleanup = super::privacy_cleanup_summary_spec(Some(&privacy));
        assert_eq!(cleanup.state, "auto cleanup");
        assert!(cleanup.ready);
        assert!(cleanup.detail.contains("Recent files off"));
        assert!(cleanup.detail.contains("Trash cleanup on"));
        assert!(cleanup.detail.contains("30 days"));
    }

    #[test]
    fn privacy_summary_tiles_report_waiting_unavailable_and_unknown_truthfully() {
        let waiting = super::privacy_private_mode_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);

        let unavailable = super::test_privacy_status(false, false, false);
        let schema = super::desktop_privacy_schema_summary_spec(Some(&unavailable));
        assert_eq!(schema.state, "unavailable");
        assert!(!schema.ready);
        assert!(schema.detail.contains("unavailable"));

        let device = super::privacy_device_access_summary_spec(Some(&unavailable));
        assert_eq!(device.state, "unavailable");
        assert!(!device.ready);

        let mut partial = super::test_privacy_status(false, true, true);
        if let Some(desktop) = partial.desktop.as_mut() {
            desktop.remember_recent_files = None;
            desktop.remember_app_usage = None;
            desktop.remove_old_trash_files = None;
            desktop.remove_old_temp_files = None;
            desktop.old_files_age_days = None;
            desktop.disable_microphone = None;
            desktop.disable_camera = None;
            desktop.disable_sound_output = None;
            desktop.usb_protection = None;
        }

        let device = super::privacy_device_access_summary_spec(Some(&partial));
        assert_eq!(device.state, "unknown");
        assert!(!device.ready);
        assert!(device
            .detail
            .contains("did not report device access controls"));

        let cleanup = super::privacy_cleanup_summary_spec(Some(&partial));
        assert_eq!(cleanup.state, "unknown");
        assert!(!cleanup.ready);
        assert!(cleanup
            .detail
            .contains("did not report cleanup or history controls"));
    }

    #[test]
    fn parses_desktop_privacy_outcome_without_hiding_core_text() {
        let outcome = desktop_privacy_outcome(
            br#"{"ok":false,"target":"disable-camera","text":"Camera preference unavailable"}"#,
        )
        .unwrap();

        assert!(!outcome.ok);
        assert_eq!(outcome.target, "disable-camera");
        assert_eq!(outcome.text, "Camera preference unavailable");
    }

    #[test]
    fn parses_accessibility_preference_outcome_without_hiding_core_text() {
        let outcome = accessibility_preference_outcome(
            br#"{"ok":true,"target":"reduce-motion","text":"Desktop animations are reduced."}"#,
        )
        .unwrap();

        assert!(outcome.ok);
        assert_eq!(outcome.target, "reduce-motion");
        assert_eq!(outcome.text, "Desktop animations are reduced.");
    }

    #[test]
    fn motion_preference_detail_tracks_reduce_motion_state() {
        assert!(motion_preference_detail(true).contains("reduced"));
        assert!(motion_preference_detail(false).contains("enabled"));
        assert!(screen_reader_detail(true).contains("enabled"));
        assert!(screen_reader_detail(false).contains("off"));
        assert!(screen_keyboard_detail(true).contains("on-screen keyboard"));
        assert!(screen_keyboard_detail(false).contains("hidden"));
        assert!(magnifier_detail(true).contains("enabled"));
        assert!(magnifier_detail(false).contains("off"));
    }

    #[test]
    fn appearance_scheme_detail_tracks_selected_mode() {
        assert!(appearance_scheme_detail("light").contains("Light appearance"));
        assert!(appearance_scheme_detail("dark").contains("Dark appearance"));
        assert!(appearance_scheme_detail("auto").contains("Auto appearance"));
        assert!(appearance_scheme_detail("unknown").contains("Auto appearance"));
        assert_eq!(normalized_appearance_theme("prefer-light"), "light");
        assert_eq!(normalized_appearance_theme("prefer-dark"), "dark");
        assert_eq!(normalized_appearance_theme("default"), "auto");
    }

    #[test]
    fn parses_appearance_outcome_without_hiding_core_text() {
        let outcome = appearance_outcome(
            br#"{"ok":true,"color_scheme":"prefer-dark","theme":"dark","text":"Dark appearance is active."}"#,
        )
       .unwrap();

        assert!(outcome.ok);
        assert_eq!(outcome.color_scheme, "prefer-dark");
        assert_eq!(outcome.theme, "dark");
        assert_eq!(outcome.text, "Dark appearance is active.");
    }

    #[test]
    fn background_picture_options_are_stable_and_truthful() {
        assert_eq!(normalized_background_picture_option("zoom"), "zoom");
        assert_eq!(normalized_background_picture_option("scaled"), "scaled");
        assert_eq!(normalized_background_picture_option("unknown"), "zoom");
        assert!(background_picture_option_detail("zoom").contains("Fills"));
        assert!(background_picture_option_detail("scaled").contains("Fits"));
        assert!(background_picture_option_detail("centered").contains("Centers"));
        assert!(background_picture_option_detail("wallpaper").contains("Tiles"));
        assert!(background_picture_option_detail("spanned").contains("multiple monitors"));
        assert!(background_picture_option_detail("none").contains("No image"));
    }

    #[test]
    fn background_shading_options_are_stable_and_truthful() {
        assert_eq!(normalized_background_shading("solid"), "solid");
        assert_eq!(normalized_background_shading("horizontal"), "horizontal");
        assert_eq!(normalized_background_shading("vertical"), "vertical");
        assert_eq!(normalized_background_shading("radial"), "solid");
        assert!(background_shading_detail("solid").contains("primary"));
        assert!(background_shading_detail("horizontal").contains("left to right"));
        assert!(background_shading_detail("vertical").contains("top to bottom"));
    }

    #[test]
    fn wallpaper_color_copy_distinguishes_empty_and_unavailable() {
        assert_eq!(
            wallpaper_color_detail(Some("#241d2f"), "Unavailable"),
            "#241d2f"
        );
        assert_eq!(
            wallpaper_color_detail(Some(""), "Unavailable"),
            "No desktop color is set."
        );
        assert_eq!(wallpaper_color_detail(None, "Unavailable"), "Unavailable");
    }

    #[test]
    fn wallpaper_uri_copy_distinguishes_empty_and_unavailable() {
        assert_eq!(
            wallpaper_uri_detail(Some("file:///wallpapers/goblins.jpg"), "Unavailable"),
            "Wallpaper image is set on this device."
        );
        assert_eq!(
            wallpaper_uri_detail(Some(""), "Unavailable"),
            "No wallpaper image is set."
        );
        assert_eq!(wallpaper_uri_detail(None, "Unavailable"), "Unavailable");
    }

    #[test]
    fn parses_wallpaper_placement_outcome_without_hiding_core_text() {
        let outcome = wallpaper_placement_outcome(
            br#"{"ok":true,"placement":"scaled","text":"Wallpaper fits the display."}"#,
        )
        .unwrap();

        assert!(outcome.ok);
        assert_eq!(outcome.placement, "scaled");
        assert_eq!(outcome.text, "Wallpaper fits the display.");
    }

    #[test]
    fn parses_wallpaper_shading_outcome_without_hiding_core_text() {
        let outcome = wallpaper_shading_outcome(
            br#"{"ok":true,"shading":"vertical","text":"Wallpaper color blends vertically."}"#,
        )
        .unwrap();

        assert!(outcome.ok);
        assert_eq!(outcome.shading, "vertical");
        assert_eq!(outcome.text, "Wallpaper color blends vertically.");
    }

    #[test]
    fn wallpaper_summary_tiles_use_gnome_background_state_without_inventing_images() {
        let wallpaper = super::WallpaperStatus {
            gsettings_available: true,
            schema_available: true,
            picture_uri: Some("file:///wallpapers/light.jpg".to_string()),
            picture_uri_dark: Some("file:///wallpapers/dark.jpg".to_string()),
            picture_options_available: true,
            picture_options: "zoom".to_string(),
            primary_color: Some("#241d2f".to_string()),
            secondary_color: Some("#111111".to_string()),
            color_shading_type_available: true,
            color_shading_type: "solid".to_string(),
            detail: "Wallpaper preferences are ready for this desktop.".to_string(),
        };

        let source = super::wallpaper_status_summary_spec(Some(&wallpaper));
        assert_eq!(source.state, "available");
        assert!(source.ready);
        assert!(source.detail.contains("ready for this desktop"));

        let light = super::wallpaper_light_image_summary_spec(Some(&wallpaper));
        assert_eq!(light.state, "set");
        assert!(light.ready);
        assert_eq!(light.detail, "Wallpaper image is set on this device.");

        let dark = super::wallpaper_dark_image_summary_spec(Some(&wallpaper));
        assert_eq!(dark.state, "set");
        assert!(dark.ready);
        assert_eq!(dark.detail, "Wallpaper image is set on this device.");

        let placement = super::wallpaper_placement_summary_spec(Some(&wallpaper));
        assert_eq!(placement.state, "zoom");
        assert!(placement.ready);
        assert!(placement.detail.contains("Fills"));
    }

    #[test]
    fn wallpaper_summary_tiles_report_waiting_and_unavailable_truthfully() {
        let waiting = super::wallpaper_status_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);
        assert!(waiting.detail.contains("Waiting for wallpaper status"));

        let unavailable = super::WallpaperStatus {
            gsettings_available: false,
            schema_available: false,
            picture_uri: None,
            picture_uri_dark: None,
            picture_options_available: false,
            picture_options: "zoom".to_string(),
            primary_color: None,
            secondary_color: None,
            color_shading_type_available: false,
            color_shading_type: "solid".to_string(),
            detail: "Desktop preference support is unavailable.".to_string(),
        };
        let source = super::wallpaper_status_summary_spec(Some(&unavailable));
        assert_eq!(source.state, "unavailable");
        assert!(!source.ready);
        assert!(source.detail.contains("unavailable"));

        let light = super::wallpaper_light_image_summary_spec(Some(&unavailable));
        assert_eq!(light.state, "unavailable");
        assert!(!light.ready);
        assert!(light.detail.contains("unavailable"));

        let partial = super::WallpaperStatus {
            gsettings_available: true,
            schema_available: true,
            picture_uri: None,
            picture_uri_dark: Some(String::new()),
            picture_options_available: false,
            picture_options: "zoom".to_string(),
            primary_color: Some("#241d2f".to_string()),
            secondary_color: Some("#111111".to_string()),
            color_shading_type_available: true,
            color_shading_type: "solid".to_string(),
            detail: "Wallpaper preferences are ready for this desktop.".to_string(),
        };
        let light = super::wallpaper_light_image_summary_spec(Some(&partial));
        assert_eq!(light.state, "unknown");
        assert!(!light.ready);

        let dark = super::wallpaper_dark_image_summary_spec(Some(&partial));
        assert_eq!(dark.state, "none");
        assert!(dark.ready);
        assert!(dark.detail.contains("No wallpaper image"));

        let placement = super::wallpaper_placement_summary_spec(Some(&partial));
        assert_eq!(placement.state, "unavailable");
        assert!(!placement.ready);
        assert!(placement.detail.contains("placement is not available"));
    }

    #[test]
    fn proxy_helpers_keep_mode_and_endpoint_copy_truthful() {
        assert_eq!(normalized_proxy_mode("none"), "none");
        assert_eq!(normalized_proxy_mode("auto"), "auto");
        assert_eq!(normalized_proxy_mode("manual"), "manual");
        assert_eq!(normalized_proxy_mode("invalid"), "none");
        assert!(proxy_mode_detail("none").contains("Direct network"));
        assert!(proxy_mode_detail("auto").contains("automatic configuration URL"));
        assert!(proxy_mode_detail("manual").contains("manual HTTP"));
        assert_eq!(
            proxy_auto_config_detail(""),
            "No automatic proxy configuration URL is set."
        );
        assert!(proxy_auto_config_detail("https://proxy.example/proxy.pac")
            .contains("used when proxy mode is Automatic"));
        assert_eq!(
            proxy_endpoint_detail(Some("proxy.local".to_string()), Some(8080)),
            "proxy.local:8080"
        );
        assert_eq!(
            proxy_endpoint_detail(Some("".to_string()), Some(8080)),
            "Not configured."
        );
        assert_eq!(
            proxy_endpoint_detail(Some("proxy.local".to_string()), Some(0)),
            "Not configured."
        );
        assert_eq!(
            proxy_ignore_hosts_detail(&[]),
            "No bypass hosts are configured."
        );
        assert_eq!(
            proxy_ignore_hosts_detail(&[
                "localhost".to_string(),
                "127.0.0.0/8".to_string(),
                "::1".to_string(),
                "*.example.test".to_string(),
                "10.0.0.0/8".to_string(),
            ]),
            "localhost, 127.0.0.0/8, ::1, *.example.test, and 1 more"
        );
    }

    #[test]
    fn network_summary_tiles_use_networkmanager_state_without_inventing_success() {
        let online = super::NetworkStatus {
            source: "networkmanager".to_string(),
            manager_available: true,
            online: true,
            connectivity: "full".to_string(),
            state: "connected".to_string(),
            active: Some(super::ActiveConnection {
                name: "Studio Wi-Fi".to_string(),
                kind: "wifi".to_string(),
                device: "wlp0s20f3".to_string(),
            }),
            proxy: Some(super::ProxyStatus {
                gsettings_available: true,
                schema_available: true,
                mode_available: true,
                mode: "none".to_string(),
                autoconfig_url: None,
                ignore_hosts: Vec::new(),
                http: super::ProxyEndpoint {
                    host: None,
                    port: None,
                },
                https: super::ProxyEndpoint {
                    host: None,
                    port: None,
                },
                ftp: super::ProxyEndpoint {
                    host: None,
                    port: None,
                },
                socks: super::ProxyEndpoint {
                    host: None,
                    port: None,
                },
                detail: "Proxy settings are ready for this desktop.".to_string(),
            }),
            detail: "Full internet connectivity is available.".to_string(),
        };

        let internet = super::internet_network_summary_spec(Some(&online));
        assert_eq!(internet.state, "online");
        assert!(internet.ready);
        assert!(internet.detail.contains("Full internet"));

        let active = super::active_connection_summary_spec(Some(&online));
        assert_eq!(active.state, "connected");
        assert!(active.ready);
        assert!(active.detail.contains("Studio Wi-Fi via wifi on wlp0s20f3"));

        let manager = super::network_manager_summary_spec(Some(&online));
        assert_eq!(manager.state, "available");
        assert!(manager.ready);
        assert!(manager.detail.contains("state connected"));

        let proxy = super::proxy_network_summary_spec(Some(&online));
        assert_eq!(proxy.state, "none");
        assert!(proxy.ready);
        assert!(proxy.detail.contains("Proxy settings"));
    }

    #[test]
    fn network_summary_tiles_report_waiting_and_unavailable_truthfully() {
        let waiting = super::internet_network_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);
        assert!(waiting.detail.contains("Waiting for networking"));

        let unavailable = super::NetworkStatus {
            source: "networkmanager".to_string(),
            manager_available: false,
            online: false,
            connectivity: String::new(),
            state: "unavailable".to_string(),
            active: None,
            proxy: None,
            detail: "The network service is unavailable in this session.".to_string(),
        };

        let internet = super::internet_network_summary_spec(Some(&unavailable));
        assert_eq!(internet.state, "unavailable");
        assert!(!internet.ready);
        assert!(internet.detail.contains("Networking is not reachable"));
        assert!(internet.detail.contains("unavailable"));
        assert!(!internet.detail.contains("Error:"));

        let active = super::active_connection_summary_spec(Some(&unavailable));
        assert_eq!(active.state, "none");
        assert!(!active.ready);
        assert!(active
            .detail
            .contains("Settings cannot display an active connection"));

        let manager = super::network_manager_summary_spec(Some(&unavailable));
        assert_eq!(manager.state, "unavailable");
        assert!(!manager.ready);
        assert!(manager.detail.contains("Networking is not reachable"));

        let proxy = super::proxy_network_summary_spec(Some(&unavailable));
        assert_eq!(proxy.state, "waiting");
        assert!(!proxy.ready);
        assert!(proxy.detail.contains("Waiting for proxy settings"));
    }

    #[test]
    fn network_summary_polishes_core_errors_without_faking_success() {
        let errored = super::NetworkStatus {
            source: "goblins-os-core".to_string(),
            manager_available: true,
            online: false,
            connectivity: "unknown".to_string(),
            state: "unknown".to_string(),
            active: None,
            proxy: None,
            detail: "Error: Could not create NMClient object: Could not connect: No such file or directory.".to_string(),
        };

        let internet = super::internet_network_summary_spec(Some(&errored));
        assert_eq!(internet.state, "unknown");
        assert!(!internet.ready);
        assert!(internet
            .detail
            .contains("Connectivity has not been confirmed by networking."));
        assert!(internet.detail.contains("Networking could not connect"));
        assert!(internet.detail.contains("service socket was not found"));
        assert!(!internet.detail.contains("Could not create NMClient object"));
        assert!(!internet.detail.contains("Error:"));

        let overview = super::overview_network_detail(Some(&errored));
        assert!(overview.contains("No active connection is reported"));
        assert!(overview.contains("Connectivity unknown"));

        let manager = super::network_manager_summary_spec(Some(&errored));
        assert_eq!(manager.state, "available");
        assert!(manager.ready);
        assert!(manager.detail.contains("Service state unknown"));
        assert!(!manager.detail.contains("goblins-os-core"));
    }

    #[test]
    fn parses_proxy_mode_outcome_without_hiding_core_text() {
        let outcome =
            proxy_mode_outcome(br#"{"ok":true,"mode":"auto","text":"Automatic proxy active"}"#)
                .unwrap();

        assert!(outcome.ok);
        assert_eq!(outcome.mode, "auto");
        assert_eq!(outcome.text, "Automatic proxy active");
    }

    #[test]
    fn setting_change_rejection_copy_is_visible_and_restores_previous_value() {
        assert_eq!(
            super::setting_change_rejected_detail("Bluetooth unavailable."),
            "Could not apply the setting: Bluetooth not ready. The previous value was restored."
        );
        assert_eq!(
            super::setting_change_rejected_detail("   "),
            "Could not apply the setting. The previous value was restored."
        );
    }

    #[test]
    fn text_scale_helpers_clamp_round_and_format() {
        assert_eq!(normalized_text_scale(0.1), 0.85);
        assert_eq!(normalized_text_scale(1.123), 1.1);
        assert_eq!(normalized_text_scale(9.0), 1.35);
        assert_eq!(text_scale_percent(1.16), "115%");
        assert_eq!(text_scale_percent(f64::NAN), "100%");
    }

    #[test]
    fn accessibility_summary_tiles_use_gnome_state() {
        let accessibility = super::test_accessibility_status(true, true, true, true);

        let text = super::accessibility_text_size_summary_spec(Some(&accessibility));
        assert_eq!(text.title, "Text size");
        assert_eq!(text.state, "115%");
        assert!(text.ready);
        assert!(text.detail.contains("system utilities"));

        let motion = super::accessibility_motion_summary_spec(Some(&accessibility));
        assert_eq!(motion.state, "reduced");
        assert!(motion.ready);
        assert!(motion.detail.contains("reduced"));

        let assistive = super::assistive_access_summary_spec(Some(&accessibility));
        assert_eq!(assistive.state, "1 enabled");
        assert!(assistive.ready);
        assert!(assistive.detail.contains("Screen reader on"));
        assert!(assistive.detail.contains("Keyboard off"));
        assert!(assistive.detail.contains("Magnifier off"));

        let comfort = super::accessibility_display_comfort_summary_spec(Some(&accessibility));
        assert_eq!(comfort.state, "night light on");
        assert!(comfort.ready);
        assert!(comfort.detail.contains("Night Light on"));
        assert!(comfort.detail.contains("automatic"));
        assert!(comfort.detail.contains("4500 K"));
    }

    #[test]
    fn accessibility_summary_tiles_report_waiting_unavailable_and_unknown_truthfully() {
        let waiting = super::accessibility_text_size_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);

        let unavailable = super::test_accessibility_status(false, false, false, false);
        let text = super::accessibility_text_size_summary_spec(Some(&unavailable));
        assert_eq!(text.state, "unavailable");
        assert!(!text.ready);
        assert!(text.detail.contains("unavailable"));

        let assistive = super::assistive_access_summary_spec(Some(&unavailable));
        assert_eq!(assistive.state, "unavailable");
        assert!(!assistive.ready);

        let mut partial = super::test_accessibility_status(true, true, true, true);
        partial.interface.reduce_motion = None;
        partial.interface.text_scale = None;
        partial.assistive.screen_reader = None;
        partial.assistive.screen_keyboard = None;
        partial.assistive.magnifier = None;
        partial.display_comfort.night_light_enabled = None;

        let motion = super::accessibility_motion_summary_spec(Some(&partial));
        assert_eq!(motion.state, "unknown");
        assert!(!motion.ready);

        let assistive = super::assistive_access_summary_spec(Some(&partial));
        assert_eq!(assistive.state, "unknown");
        assert!(!assistive.ready);
        assert!(assistive
            .detail
            .contains("did not report assistive technology keys"));

        let comfort = super::accessibility_display_comfort_summary_spec(Some(&partial));
        assert_eq!(comfort.state, "unknown");
        assert!(!comfort.ready);
        assert!(comfort
            .detail
            .contains("Night Light preference is not reported"));
    }

    #[test]
    fn appearance_summary_tiles_use_desktop_state_and_inter_policy() {
        let appearance = super::AppearanceStatus {
            source: "goblins-os-core".to_string(),
            gsettings_available: true,
            color_scheme_available: true,
            color_scheme: "prefer-dark".to_string(),
            theme: "dark".to_string(),
            wallpaper: None,
            detail: "Appearance preference is ready.".to_string(),
        };
        let accessibility = super::AccessibilityStatus {
            source: "goblins-os-core".to_string(),
            gsettings_available: true,
            interface: super::InterfaceAccessibilityStatus {
                schema_available: true,
                reduce_motion: Some(false),
                text_scale: Some(1.16),
                detail: "Interface settings are available.".to_string(),
            },
            assistive: super::AssistiveTechnologyStatus {
                schema_available: true,
                screen_reader: Some(false),
                screen_keyboard: Some(false),
                magnifier: Some(false),
                detail: "Assistive technology settings are available.".to_string(),
            },
            display_comfort: super::DisplayComfortStatus {
                schema_available: true,
                night_light_enabled: Some(false),
                schedule_automatic: Some(true),
                temperature: Some(4500),
                detail: "Display comfort settings are available.".to_string(),
            },
            detail: "Accessibility settings are available.".to_string(),
        };

        let scheme = super::appearance_scheme_summary_spec(Some(&appearance));
        assert_eq!(scheme.state, "dark");
        assert!(scheme.ready);
        // The system value reads as the friendly label (Dark), never the raw
        // gsettings token (prefer-dark).
        assert!(scheme.detail.contains("system value is Dark"));
        assert!(!scheme.detail.contains("prefer-dark"));
        assert!(!scheme.detail.contains("goblins-os-core"));
        assert!(scheme.detail.contains("system value"));

        let typography = super::appearance_typography_summary_spec();
        assert_eq!(typography.state, "Inter");
        assert!(typography.ready);
        assert!(typography.detail.contains("Inter 11"));
        assert!(!typography.detail.contains("Apple"));

        let motion = super::appearance_motion_summary_spec(Some(&accessibility));
        assert_eq!(motion.state, "full motion");
        assert!(motion.ready);
        assert!(motion.detail.contains("enabled"));

        let text_size = super::appearance_text_size_summary_spec(Some(&accessibility));
        assert_eq!(text_size.state, "115%");
        assert!(text_size.ready);
        assert!(text_size.detail.contains("115%"));
    }

    #[test]
    fn appearance_summary_tiles_report_waiting_and_unavailable_truthfully() {
        let waiting = super::appearance_scheme_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);
        assert!(waiting.detail.contains("Waiting for appearance status"));

        let appearance = super::AppearanceStatus {
            source: "goblins-os-core".to_string(),
            gsettings_available: false,
            color_scheme_available: false,
            color_scheme: "default".to_string(),
            theme: "auto".to_string(),
            wallpaper: None,
            detail: "Desktop preference support is unavailable.".to_string(),
        };
        let scheme = super::appearance_scheme_summary_spec(Some(&appearance));
        assert_eq!(scheme.state, "unavailable");
        assert!(!scheme.ready);
        assert!(scheme.detail.contains("unavailable"));

        let inaccessible = super::AccessibilityStatus {
            source: "goblins-os-core".to_string(),
            gsettings_available: true,
            interface: super::InterfaceAccessibilityStatus {
                schema_available: false,
                reduce_motion: None,
                text_scale: None,
                detail: "Interface accessibility schema is unavailable.".to_string(),
            },
            assistive: super::AssistiveTechnologyStatus {
                schema_available: false,
                screen_reader: None,
                screen_keyboard: None,
                magnifier: None,
                detail: "Assistive technology settings are unavailable.".to_string(),
            },
            display_comfort: super::DisplayComfortStatus {
                schema_available: false,
                night_light_enabled: None,
                schedule_automatic: None,
                temperature: None,
                detail: "Display comfort settings are unavailable.".to_string(),
            },
            detail: "Accessibility settings are incomplete.".to_string(),
        };

        let motion = super::appearance_motion_summary_spec(Some(&inaccessible));
        assert_eq!(motion.state, "unavailable");
        assert!(!motion.ready);
        assert!(motion.detail.contains("schema is unavailable"));

        let text_size = super::appearance_text_size_summary_spec(Some(&inaccessible));
        assert_eq!(text_size.state, "unavailable");
        assert!(!text_size.ready);
        assert!(text_size.detail.contains("schema is unavailable"));
    }

    #[test]
    fn input_setting_helpers_clamp_round_and_format() {
        assert!(key_repeat_detail(true).contains("repeats"));
        assert!(key_repeat_detail(false).contains("one character"));
        assert_eq!(normalized_unit_speed(f64::NAN), 0.0);
        assert_eq!(normalized_unit_speed(-2.0), -1.0);
        assert_eq!(normalized_unit_speed(0.13), 0.15);
        assert_eq!(pointer_speed_label(0.0), "Default");
        assert_eq!(pointer_speed_label(0.34), "Faster 35%");
        assert_eq!(pointer_speed_label(-0.24), "Slower 25%");
        assert_eq!(normalized_keyboard_delay(1), 150);
        assert_eq!(normalized_keyboard_delay(764), 775);
        assert_eq!(normalized_keyboard_delay(4000), 1000);
        assert_eq!(normalized_keyboard_repeat_interval(1), 15);
        assert_eq!(normalized_keyboard_repeat_interval(27), 25);
        assert_eq!(normalized_keyboard_repeat_interval(999), 120);
        assert_eq!(milliseconds_label(27.4), "27 ms");
    }

    #[test]
    fn keyboard_summary_tiles_use_desktop_input_state() {
        let input = super::test_input_status(true, true, true, true);

        let source = super::input_source_summary_spec(Some(&input));
        assert_eq!(source.state, "available");
        assert!(source.ready);
        assert!(source.detail.contains("ready for this desktop"));
        assert!(!source.detail.contains("goblins-os-core"));

        let repeat = super::keyboard_repeat_summary_spec(Some(&input));
        assert_eq!(repeat.state, "on");
        assert!(repeat.ready);
        assert!(repeat.detail.contains("repeats"));

        let delay = super::keyboard_delay_summary_spec(Some(&input));
        assert_eq!(delay.state, "500 ms");
        assert!(delay.ready);
        assert!(delay.detail.contains("500 ms"));

        let interval = super::keyboard_interval_summary_spec(Some(&input));
        assert_eq!(interval.state, "30 ms");
        assert!(interval.ready);
        assert!(interval.detail.contains("30 ms"));
    }

    #[test]
    fn pointer_summary_tiles_use_mouse_and_touchpad_state() {
        let input = super::test_input_status(true, true, true, true);

        let mouse = super::mouse_speed_summary_spec(Some(&input));
        assert_eq!(mouse.state, "Faster 35%");
        assert!(mouse.ready);
        assert!(mouse.detail.contains("system pointer setting"));

        let touchpad = super::touchpad_speed_summary_spec(Some(&input));
        assert_eq!(touchpad.state, "Slower 25%");
        assert!(touchpad.ready);
        assert!(touchpad.detail.contains("system touchpad setting"));

        let tap = super::touchpad_tap_summary_spec(Some(&input));
        assert_eq!(tap.state, "on");
        assert!(tap.ready);
        assert!(tap.detail.contains("clicks without pressing"));
    }

    #[test]
    fn input_summary_tiles_report_waiting_unavailable_and_unknown_truthfully() {
        let waiting = super::input_source_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);

        let unavailable = super::test_input_status(false, false, false, false);
        let source = super::input_source_summary_spec(Some(&unavailable));
        assert_eq!(source.state, "unavailable");
        assert!(!source.ready);
        assert!(source.detail.contains("unavailable"));

        let repeat = super::keyboard_repeat_summary_spec(Some(&unavailable));
        assert_eq!(repeat.state, "unavailable");
        assert!(!repeat.ready);
        assert!(repeat.detail.contains("unavailable"));

        let mut partial = super::test_input_status(true, true, true, true);
        partial.keyboard.repeat = None;
        partial.keyboard.delay_ms = None;
        partial.touchpad.tap_to_click = None;

        let repeat = super::keyboard_repeat_summary_spec(Some(&partial));
        assert_eq!(repeat.state, "unknown");
        assert!(!repeat.ready);

        let delay = super::keyboard_delay_summary_spec(Some(&partial));
        assert_eq!(delay.state, "unknown");
        assert!(!delay.ready);

        let tap = super::touchpad_tap_summary_spec(Some(&partial));
        assert_eq!(tap.state, "unknown");
        assert!(!tap.ready);
    }

    #[test]
    fn night_light_helpers_track_state_and_temperature_bounds() {
        assert!(night_light_detail(true).contains("on"));
        assert!(night_light_detail(false).contains("off"));
        assert!(night_light_schedule_detail(true).contains("automatically"));
        assert!(night_light_schedule_detail(false).contains("manual"));
        assert_eq!(normalized_night_light_temperature(1), 1000);
        assert_eq!(normalized_night_light_temperature(3449), 3400);
        assert_eq!(normalized_night_light_temperature(3450), 3500);
        assert_eq!(normalized_night_light_temperature(20000), 10000);
        assert_eq!(night_light_temperature_label(3499.0), "3500 K");
    }

    #[test]
    fn display_helpers_report_core_query_state_without_fake_controls() {
        let displays = DisplaysStatus {
            session_type: "wayland".to_string(),
            wayland_display: Some("wayland-0".to_string()),
            x11_display: None,
            gdbus_available: true,
            mutter_display_config_available: true,
            xrandr_available: false,
            outputs: Vec::new(),
            detail: "Display configuration is reachable.".to_string(),
        };
        assert!(display_handles_detail(&displays).contains("Wayland wayland-0"));
        assert!(display_handles_detail(&displays).contains("Display service ready"));
        assert!(display_handles_detail(&displays).contains("display query not ready"));

        let output = DisplayOutputStatus {
            name: "eDP-1".to_string(),
            connected: true,
            primary: true,
            current_mode: Some("2560x1440".to_string()),
            position: Some("+0+0".to_string()),
            detail: "eDP-1 is the primary display at 2560x1440.".to_string(),
        };
        assert_eq!(display_output_title(&output), "eDP-1 · primary");
        assert!(display_output_detail(&output).contains("Mode 2560x1440"));
        assert!(display_output_detail(&output).contains("Position +0+0"));

        let disconnected = DisplayOutputStatus {
            name: "HDMI-1".to_string(),
            connected: false,
            primary: false,
            current_mode: None,
            position: None,
            detail: "HDMI-1 is disconnected.".to_string(),
        };
        assert!(display_output_detail(&disconnected).contains("disconnected outputs"));

        let session = super::display_session_summary_spec(Some(&displays), None);
        assert_eq!(session.state, "wayland");
        assert!(session.ready);
        assert!(session.detail.contains("Display status is available"));
        assert!(!session.detail.contains("Wayland"));

        let query = super::display_query_summary_spec(Some(&displays));
        assert_eq!(query.state, "desktop bridge");
        assert!(query.ready);
        assert!(query.detail.contains("Display configuration"));

        let no_outputs = super::display_outputs_summary_spec(Some(&displays));
        assert_eq!(no_outputs.state, "none");
        assert!(!no_outputs.ready);
        assert!(no_outputs.detail.contains("No individual display outputs"));

        let mut displays_with_outputs = displays.clone();
        displays_with_outputs.outputs = vec![output, disconnected];
        let outputs = super::display_outputs_summary_spec(Some(&displays_with_outputs));
        assert_eq!(outputs.state, "1 display");
        assert!(outputs.ready);
        assert!(outputs.detail.contains("1 connected"));
        assert!(outputs.detail.contains("1 disconnected"));
        assert!(outputs.detail.contains("Primary eDP-1"));
        assert!(outputs.detail.contains("2560x1440"));
    }

    #[test]
    fn display_summary_tiles_report_waiting_and_comfort_state_truthfully() {
        let waiting = super::display_query_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);
        assert!(waiting.detail.contains("Waiting for display query"));

        let limited = DisplaysStatus {
            session_type: "x11".to_string(),
            wayland_display: None,
            x11_display: Some(":0".to_string()),
            gdbus_available: true,
            mutter_display_config_available: false,
            xrandr_available: true,
            outputs: Vec::new(),
            detail: "A desktop display handle is visible, but monitor configuration could not be queried from this session.".to_string(),
        };
        let query = super::display_query_summary_spec(Some(&limited));
        assert_eq!(query.state, "limited");
        assert!(!query.ready);

        let accessibility = super::AccessibilityStatus {
            source: "goblins-os-core".to_string(),
            gsettings_available: true,
            interface: super::InterfaceAccessibilityStatus {
                schema_available: true,
                reduce_motion: Some(false),
                text_scale: Some(1.0),
                detail: "Interface settings are available.".to_string(),
            },
            assistive: super::AssistiveTechnologyStatus {
                schema_available: true,
                screen_reader: Some(false),
                screen_keyboard: Some(false),
                magnifier: Some(false),
                detail: "Assistive technology settings are available.".to_string(),
            },
            display_comfort: super::DisplayComfortStatus {
                schema_available: true,
                night_light_enabled: Some(true),
                schedule_automatic: Some(true),
                temperature: Some(4490),
                detail: "Display comfort settings are available.".to_string(),
            },
            detail: "Accessibility settings are available.".to_string(),
        };
        let comfort = super::display_comfort_summary_spec(Some(&accessibility));
        assert_eq!(comfort.state, "night light on");
        assert!(comfort.ready);
        assert!(comfort.detail.contains("Night Light on"));
        assert!(comfort.detail.contains("automatic"));
        assert!(comfort.detail.contains("4500 K"));
    }

    #[test]
    fn sound_setting_details_track_current_state() {
        assert!(interface_sounds_detail(true).contains("sound theme"));
        assert!(interface_sounds_detail(false).contains("without desktop sounds"));
        assert!(input_feedback_sounds_detail(true).contains("feedback sounds"));
        assert!(input_feedback_sounds_detail(false).contains("muted"));
        assert!(volume_boost_detail(true).contains("exceed 100%"));
        assert!(volume_boost_detail(false).contains("capped"));
        assert!(sound_theme_detail(Some("freedesktop")).contains("freedesktop"));
        assert!(sound_theme_detail(Some("")).contains("No desktop sound theme"));
        assert!(sound_theme_detail(None).contains("not available"));
        assert_eq!(normalized_audio_volume(f64::NAN), 0.0);
        assert_eq!(normalized_audio_volume(-1.0), 0.0);
        assert_eq!(normalized_audio_volume(72.4), 72.0);
        assert_eq!(normalized_audio_volume(200.0), 150.0);
        assert_eq!(audio_volume_label(62.0), "62%");
        assert_eq!(audio_volume_title("input"), "Input volume");
        assert_eq!(audio_volume_title("output"), "Output volume");
        assert!(audio_volume_detail("input").contains("input gain"));
        assert_eq!(audio_mute_title("input"), "Mute input");
        assert!(audio_mute_detail("output", true).contains("muted"));
        assert!(audio_mute_detail("input", false).contains("capture"));
        assert_eq!(audio_target_kind("input"), "input");
        assert_eq!(audio_target_kind("output"), "output");

        let devices = vec![
            AudioDeviceStatus {
                id: "55".to_string(),
                name: "Built-in Audio".to_string(),
                active: true,
            },
            AudioDeviceStatus {
                id: "56".to_string(),
                name: "HDMI Display".to_string(),
                active: false,
            },
        ];
        assert!(audio_device_choice_detail("output", &devices, "56").contains("HDMI Display"));
        let unavailable = AudioEndpointStatus {
            available: false,
            volume_percent: None,
            muted: None,
            default_device_id: None,
            devices: Vec::new(),
            detail: "WirePlumber unavailable.".to_string(),
        };
        assert!(audio_device_unavailable_detail("input", &unavailable).contains("Audio routing"));

        let outcome = audio_control_outcome(
            br#"{"ok":true,"target":"output","text":"Output volume set to 62%.","volume_percent":62,"muted":null}"#,
        )
       .unwrap();
        assert!(outcome.ok);
        assert_eq!(outcome.text, "Output volume set to 62%.");

        let preference_outcome = sound_preference_outcome(
            br#"{"ok":true,"target":"volume-boost","text":"Output volume is capped at 100%."}"#,
        )
        .unwrap();
        assert!(preference_outcome.ok);
        assert_eq!(preference_outcome.target, "volume-boost");
        assert_eq!(preference_outcome.text, "Output volume is capped at 100%.");
    }

    #[test]
    fn sound_summary_tiles_use_audio_state_without_inventing_devices() {
        let audio = super::AudioStatus {
            source: "goblins-os-core".to_string(),
            wireplumber_available: true,
            output: AudioEndpointStatus {
                available: true,
                volume_percent: Some(62),
                muted: Some(false),
                default_device_id: Some("56".to_string()),
                devices: vec![
                    AudioDeviceStatus {
                        id: "55".to_string(),
                        name: "Built-in Audio".to_string(),
                        active: false,
                    },
                    AudioDeviceStatus {
                        id: "56".to_string(),
                        name: "HDMI Display".to_string(),
                        active: true,
                    },
                ],
                detail: "Default output volume is 62%.".to_string(),
            },
            input: AudioEndpointStatus {
                available: true,
                volume_percent: Some(40),
                muted: Some(true),
                default_device_id: Some("88".to_string()),
                devices: vec![AudioDeviceStatus {
                    id: "88".to_string(),
                    name: "Studio Microphone".to_string(),
                    active: true,
                }],
                detail: "Default input volume is 40% and muted.".to_string(),
            },
            sound: Some(super::SoundPreferencesStatus {
                gsettings_available: true,
                schema_available: true,
                event_sounds: Some(true),
                input_feedback_sounds: Some(false),
                volume_boost: Some(true),
                theme_name: Some("freedesktop".to_string()),
                detail: "Sound preferences are ready for this desktop.".to_string(),
            }),
            detail: "WirePlumber is available and default audio endpoints were reported."
                .to_string(),
        };

        let service = super::audio_service_summary_spec(Some(&audio));
        assert_eq!(service.state, "available");
        assert!(service.ready);
        assert!(service.detail.contains("Audio routing is available"));

        let output = super::audio_endpoint_summary_spec("Output", "output", Some(&audio.output));
        assert_eq!(output.state, "62%");
        assert!(output.ready);
        assert!(output.detail.contains("HDMI Display"));

        let input = super::audio_endpoint_summary_spec("Input", "input", Some(&audio.input));
        assert_eq!(input.state, "muted");
        assert!(input.ready);
        assert!(input.detail.contains("Studio Microphone"));

        let sound = super::sound_preferences_summary_spec(Some(&audio));
        assert_eq!(sound.state, "available");
        assert!(sound.ready);
        assert!(sound.detail.contains("Interface sounds on"));
        assert!(sound.detail.contains("Input feedback off"));
        assert!(sound.detail.contains("freedesktop"));
    }

    #[test]
    fn sound_summary_tiles_report_unavailable_and_waiting_truthfully() {
        let waiting = super::audio_service_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);
        assert!(waiting.detail.contains("Waiting for audio service"));

        let pipewire_error_endpoint = AudioEndpointStatus {
            available: false,
            volume_percent: None,
            muted: None,
            default_device_id: None,
            devices: Vec::new(),
            detail: "E module-rt: Failed to connect to session bus\nCould not connect to PipeWire"
                .to_string(),
        };
        let limited_audio = super::AudioStatus {
            source: "goblins-os-core".to_string(),
            wireplumber_available: true,
            output: pipewire_error_endpoint.clone(),
            input: pipewire_error_endpoint.clone(),
            sound: None,
            detail: "WirePlumber is present, but no default output or input is reported."
                .to_string(),
        };
        let limited = super::audio_service_summary_spec(Some(&limited_audio));
        assert_eq!(limited.state, "limited");
        assert!(!limited.ready);
        let concise_output =
            super::audio_endpoint_summary_spec("Output", "output", Some(&limited_audio.output));
        assert!(concise_output
            .detail
            .contains("Audio routing is not reachable"));
        assert!(!concise_output.detail.contains("module-rt"));

        let unavailable_endpoint = AudioEndpointStatus {
            available: false,
            volume_percent: None,
            muted: None,
            default_device_id: None,
            devices: Vec::new(),
            detail: "WirePlumber control tooling is not ready in this build.".to_string(),
        };
        let audio = super::AudioStatus {
            source: "goblins-os-core".to_string(),
            wireplumber_available: false,
            output: unavailable_endpoint.clone(),
            input: unavailable_endpoint,
            sound: Some(super::SoundPreferencesStatus {
                gsettings_available: false,
                schema_available: false,
                event_sounds: None,
                input_feedback_sounds: None,
                volume_boost: None,
                theme_name: None,
                detail: "Desktop preference support is unavailable.".to_string(),
            }),
            detail: "WirePlumber control tooling is not ready in this build.".to_string(),
        };

        let service = super::audio_service_summary_spec(Some(&audio));
        assert_eq!(service.state, "unavailable");
        assert!(!service.ready);
        assert!(service.detail.contains("not ready"));

        let output = super::audio_endpoint_summary_spec("Output", "output", Some(&audio.output));
        assert_eq!(output.state, "unavailable");
        assert!(!output.ready);
        assert!(output.detail.contains("not ready"));

        let sound = super::sound_preferences_summary_spec(Some(&audio));
        assert_eq!(sound.state, "unavailable");
        assert!(!sound.ready);
        assert!(sound.detail.contains("unavailable"));
    }

    #[test]
    fn notification_setting_details_track_current_state() {
        assert!(notification_banners_detail(true).contains("banners"));
        assert!(notification_banners_detail(false).contains("will not interrupt"));
        assert!(lock_screen_notifications_detail(true).contains("locked"));
        assert!(lock_screen_notifications_detail(false).contains("hidden"));
        assert!(notification_app_children_detail(&[]).contains("No per-app"));
        assert!(
            notification_app_children_detail(&["org.gnome.Console".to_string()])
                .contains("1 per-app")
        );
        assert!(notification_app_children_detail(&[
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
            "four".to_string(),
            "five".to_string(),
        ])
        .contains("and 1 more"));
    }

    #[test]
    fn notification_application_helpers_keep_per_app_controls_truthful() {
        assert!(notification_app_enable_detail(true).contains("can deliver"));
        assert!(notification_app_enable_detail(false).contains("muted"));
        assert!(notification_app_sound_detail(false).contains("silent"));
        assert!(notification_app_lock_screen_detail(false).contains("hidden"));
        assert!(notification_app_lock_screen_details_detail(true).contains("summaries"));
        assert!(notification_app_expand_detail(false).contains("compact"));

        let outcome = notification_preference_outcome(
            br#"{"ok":true,"target":"application-sound-alerts","text":"Notifications from this application stay silent."}"#,
        )
       .unwrap();
        assert!(outcome.ok);
        assert_eq!(outcome.target, "application-sound-alerts");
        assert_eq!(
            outcome.text,
            "Notifications from this application stay silent."
        );
    }

    #[test]
    fn notification_summary_tiles_use_gnome_state_without_inventing_entries() {
        let notifications = super::NotificationsStatus {
            source: "goblins-os-core".to_string(),
            gsettings_available: true,
            schema_available: true,
            application_schema_available: true,
            show_banners: Some(true),
            show_in_lock_screen: Some(false),
            application_children: vec![
                "org-gnome-Console".to_string(),
                "com-example-Mail".to_string(),
            ],
            applications: vec![super::NotificationApplicationStatus {
                child: "org-gnome-Console".to_string(),
                label: "Console".to_string(),
                enable: Some(true),
                show_banners: Some(true),
                enable_sound_alerts: Some(false),
                show_in_lock_screen: Some(false),
                details_in_lock_screen: Some(false),
                force_expanded: Some(false),
                detail: "Console maps to org.gnome.Console.".to_string(),
            }],
            detail: "Notification preferences are ready for this desktop.".to_string(),
        };

        let delivery = super::notifications_delivery_summary_spec(Some(&notifications));
        assert_eq!(delivery.state, "available");
        assert!(delivery.ready);
        assert!(delivery.detail.contains("Notification preferences"));

        let banners = super::notification_banners_summary_spec(Some(&notifications));
        assert_eq!(banners.state, "on");
        assert!(banners.ready);
        assert!(banners.detail.contains("banners"));

        let lock_screen = super::notification_lock_screen_summary_spec(Some(&notifications));
        assert_eq!(lock_screen.state, "off");
        assert!(lock_screen.ready);
        assert!(lock_screen.detail.contains("hidden"));

        let apps = super::notification_app_registry_summary_spec(Some(&notifications));
        assert_eq!(apps.state, "2 apps");
        assert!(apps.ready);
        assert!(apps.detail.contains("org-gnome-Console"));
        assert!(apps.detail.contains("com-example-Mail"));
    }

    #[test]
    fn notification_summary_tiles_report_waiting_and_unavailable_truthfully() {
        let waiting = super::notifications_delivery_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);
        assert!(waiting
            .detail
            .contains("Waiting for notification preferences"));

        let unavailable = super::NotificationsStatus {
            source: "goblins-os-core".to_string(),
            gsettings_available: false,
            schema_available: false,
            application_schema_available: false,
            show_banners: None,
            show_in_lock_screen: None,
            application_children: Vec::new(),
            applications: Vec::new(),
            detail: "Desktop preference support is unavailable.".to_string(),
        };
        let delivery = super::notifications_delivery_summary_spec(Some(&unavailable));
        assert_eq!(delivery.state, "unavailable");
        assert!(!delivery.ready);

        let banners = super::notification_banners_summary_spec(Some(&unavailable));
        assert_eq!(banners.state, "unavailable");
        assert!(!banners.ready);
        assert!(banners.detail.contains("unavailable"));

        let partial = super::NotificationsStatus {
            source: "goblins-os-core".to_string(),
            gsettings_available: true,
            schema_available: true,
            application_schema_available: false,
            show_banners: None,
            show_in_lock_screen: None,
            application_children: Vec::new(),
            applications: Vec::new(),
            detail: "Notification preferences are ready for this desktop.".to_string(),
        };

        let lock_screen = super::notification_lock_screen_summary_spec(Some(&partial));
        assert_eq!(lock_screen.state, "unknown");
        assert!(!lock_screen.ready);
        assert!(lock_screen.detail.contains("not reported"));

        let apps = super::notification_app_registry_summary_spec(Some(&partial));
        assert_eq!(apps.state, "unavailable");
        assert!(!apps.ready);
        assert!(apps
            .detail
            .contains("Per-application notification settings"));
    }

    #[test]
    fn storage_capacity_helpers_are_bounded_and_human_readable() {
        assert_eq!(storage_used_gb(500, 125), 375);
        assert_eq!(
            storage_capacity_detail(500, 125),
            "375GB used · 125GB free of 500GB"
        );
        assert_eq!(storage_capacity_percent_text(500, 125), "75% used");
        assert_eq!(storage_used_gb(500, 700), 0);
        assert_eq!(
            storage_capacity_detail(500, 700),
            "0GB used · 500GB free of 500GB"
        );
        assert_eq!(storage_capacity_percent_text(0, 0), "0% used");
        assert!((storage_used_fraction(500, 125) - 0.75).abs() < f64::EPSILON);
        assert_eq!(super::storage_pressure_label(500, 125), "available");
        assert!(super::storage_pressure_ready(500, 125));
        assert_eq!(super::storage_pressure_label(500, 50), "low space");
        assert!(!super::storage_pressure_ready(500, 50));
        assert_eq!(super::storage_pressure_label(500, 4), "critical");
        assert_eq!(super::storage_pressure_label(0, 0), "unknown");
        assert_eq!(super::model_cache_capacity_label(Some(24)), "available");
        assert_eq!(super::model_cache_capacity_label(Some(20)), "low space");
        assert_eq!(super::model_cache_capacity_label(Some(5)), "critical");
        assert_eq!(super::model_cache_capacity_label(None), "unknown");
        assert!(super::model_cache_capacity_ready(Some(24)));
        assert!(!super::model_cache_capacity_ready(Some(20)));

        let catalog = super::test_local_model_catalog(Some(24));
        let detail = super::model_cache_capacity_detail(&catalog);
        assert!(detail.contains("model cache"));
        assert!(!detail.contains("/var/lib/goblins-os/models"));
        assert!(detail.contains("24GB free"));
        assert!(detail.contains("Engine:"));
        assert!(detail.contains("Ollama"));
        assert!(detail.contains("Downloads require explicit consent"));
        let system_volume = super::StorageVolume {
            id: "system-root".to_string(),
            mount_point: "/".to_string(),
            total_gb: 500,
            available_gb: 125,
        };
        assert_eq!(super::storage_volume_title(&system_volume), "System volume");
        assert_eq!(
            super::storage_volume_detail(&system_volume),
            "375GB used · 125GB free of 500GB"
        );
        assert!(super::native_app_handoff_detail(
            "Disk Usage Analyzer",
            "inspect folders, mounted volumes, and where disk space is being used",
            true
        )
        .contains("Disk Usage Analyzer lets you"));
        assert!(super::native_app_handoff_detail(
            "Disks",
            "inspect drives, partitions, filesystems, and SMART data",
            true
        )
        .contains("Disks lets you"));
        assert!(super::native_app_handoff_detail(
            "Disks",
            "inspect drives, partitions, filesystems, and SMART data",
            false
        )
        .contains("included in the full Goblins OS image"));
        assert!(super::native_app_handoff_accessibility("Disks", false)
            .contains("not included in this build"));
    }

    #[test]
    fn storage_pressure_summary_uses_real_capacity_without_guessing_cleanup() {
        let hardware = super::test_storage_hardware(vec![
            super::StorageVolume {
                id: "system-root".to_string(),
                mount_point: "/".to_string(),
                total_gb: 500,
                available_gb: 48,
            },
            super::StorageVolume {
                id: "model-store".to_string(),
                mount_point: "/var/lib/goblins-os/models".to_string(),
                total_gb: 500,
                available_gb: 125,
            },
        ]);
        let catalog = super::test_local_model_catalog(Some(24));

        assert_eq!(
            super::storage_overall_pressure_label(Some(&hardware), Some(&catalog)),
            "low space"
        );
        assert!(!super::storage_overall_pressure_ready("low space"));

        let detail = super::storage_overall_pressure_detail(Some(&hardware), Some(&catalog), None);
        assert!(detail.contains("Most constrained volume: System volume"));
        assert!(detail.contains("452GB used · 48GB free of 500GB"));
        assert!(!detail.contains("system-root"));
        assert!(!detail.contains("mounted at /"));
        assert!(detail.contains("90% used"));
        assert!(detail.contains("Model cache has 24GB free"));
        assert!(!detail.contains("/var/lib/goblins-os/models"));
        assert!(!detail.contains("clean"));
    }

    #[test]
    fn storage_pressure_summary_reports_unknown_when_core_data_is_missing() {
        assert_eq!(super::storage_overall_pressure_label(None, None), "unknown");

        let system = super::test_settings_system("localhost/goblins-os:test", true, true, true);
        let detail = super::storage_overall_pressure_detail(None, None, Some(&system));
        assert!(detail.contains("Mounted-volume capacity is waiting"));
        assert!(detail.contains("Model-cache capacity is waiting"));
    }

    #[test]
    fn storage_pressure_plan_gives_truthful_gnome_handoffs() {
        let ready = super::storage_pressure_plan_detail("critical", true, true, true, true);
        let open_disk_usage = ["Open", "Disk Usage Analyzer"].join(" ");
        let open_disks = ["Open", "Disks"].join(" ");

        assert!(ready.contains("Free space now"));
        assert!(ready.contains(&open_disk_usage));
        assert!(ready.contains("automatic Trash and temporary-file cleanup controls"));
        assert!(!ready.contains("old Trash"));
        assert!(ready.contains("Review model-cache capacity"));
        assert!(ready.contains(&open_disks));

        let waiting = super::storage_pressure_plan_detail("unknown", false, false, false, false);
        assert!(waiting.contains("included in the full Goblins OS image"));
        assert!(waiting
            .contains("Automatic Trash and temporary-file cleanup controls are not available"));
        assert!(!waiting.contains("old Trash"));
        assert!(waiting.contains("Model-cache capacity is still waiting"));

        let forbidden_desktop_copy = ["needs", "GNOME"].join(" ");
        assert!(!ready.contains(&forbidden_desktop_copy));
        assert!(!waiting.contains(&forbidden_desktop_copy));

        let available = super::test_privacy_status(true, true, true);
        assert!(super::storage_cleanup_controls_available(Some(&available)));

        let unavailable = super::test_privacy_status(true, false, false);
        assert!(!super::storage_cleanup_controls_available(Some(
            &unavailable
        )));
        assert!(!super::storage_cleanup_controls_available(None));
    }

    #[test]
    fn storage_summary_tiles_report_pressure_cache_mounts_and_state_truthfully() {
        let hardware = super::test_storage_hardware(vec![
            super::StorageVolume {
                id: "system-root".to_string(),
                mount_point: "/".to_string(),
                total_gb: 500,
                available_gb: 48,
            },
            super::StorageVolume {
                id: "model-store".to_string(),
                mount_point: "/var/lib/goblins-os/models".to_string(),
                total_gb: 500,
                available_gb: 125,
            },
        ]);
        let catalog = super::test_local_model_catalog(Some(24));
        let system = super::test_settings_system("localhost/goblins-os:test", true, true, true);

        let pressure =
            super::storage_pressure_summary_spec(Some(&hardware), Some(&catalog), Some(&system));
        assert_eq!(pressure.title, "Storage pressure");
        assert_eq!(pressure.state, "low space");
        assert!(!pressure.ready);
        assert!(pressure.detail.contains("System volume"));
        assert!(!pressure.detail.contains("system-root"));
        assert!(!pressure.detail.contains("/var/lib/goblins-os/models"));
        assert!(pressure.detail.contains("Model cache has"));

        let cache = super::model_cache_summary_spec(Some(&catalog), Some(&system));
        assert_eq!(cache.title, "Model cache");
        assert_eq!(cache.state, "available");
        assert!(cache.ready);
        assert!(cache.detail.contains("24GB free"));

        let mounts = super::mounted_storage_summary_spec(Some(&hardware));
        assert_eq!(mounts.title, "Mounted volumes");
        assert_eq!(mounts.state, "low space");
        assert!(!mounts.ready);
        assert!(mounts.detail.contains("2 mounted volumes reported"));
        assert!(mounts.detail.contains("Most constrained: System volume"));
        assert!(mounts.detail.contains("90% used"));

        let state = super::os_state_storage_summary_spec(Some(&system));
        assert_eq!(state.title, "Private system state");
        assert_eq!(state.state, "configured");
        assert!(state.ready);
        assert!(state.detail.contains("Secrets stay OS-owned"));
        assert!(!state.detail.contains("/var/lib"));

        let boundary_detail = super::private_system_storage_boundary_detail(&state);
        assert!(boundary_detail.contains("assistant runtime"));
        assert!(boundary_detail.contains("secret vault"));
        assert!(boundary_detail.contains("Secrets are never displayed"));
        assert!(!boundary_detail.contains("/var/lib"));

        let waiting_cache = super::model_cache_summary_spec(None, Some(&system));
        assert_eq!(waiting_cache.state, "waiting");
        assert!(!waiting_cache.ready);
        assert!(!waiting_cache.detail.contains("/var/lib/goblins-os/models"));

        let waiting_mounts = super::mounted_storage_summary_spec(None);
        assert_eq!(waiting_mounts.state, "waiting");
        assert!(!waiting_mounts.ready);

        let waiting_state = super::os_state_storage_summary_spec(None);
        assert_eq!(waiting_state.state, "waiting");
        assert!(!waiting_state.ready);
    }

    #[test]
    fn storage_summary_flags_missing_os_owned_paths() {
        let mut system = super::test_settings_system("localhost/goblins-os:test", true, true, true);
        system.storage.resident_state_dir.clear();

        let state = super::os_state_storage_summary_spec(Some(&system));
        assert_eq!(state.state, "incomplete");
        assert!(!state.ready);
        assert!(state.detail.contains("assistant runtime"));
        assert!(!state.detail.contains("resident"));
        assert_eq!(
            super::private_system_storage_boundary_detail(&state),
            state.detail
        );

        let missing = super::missing_storage_path_names(&system.storage);
        assert_eq!(missing, vec!["assistant runtime"]);
    }

    #[test]
    fn storage_pressure_summary_promotes_critical_pressure() {
        let hardware = super::test_storage_hardware(vec![super::StorageVolume {
            id: "system-root".to_string(),
            mount_point: "/".to_string(),
            total_gb: 500,
            available_gb: 4,
        }]);
        let catalog = super::test_local_model_catalog(Some(414));

        assert_eq!(
            super::storage_overall_pressure_label(Some(&hardware), Some(&catalog)),
            "critical"
        );
        assert_eq!(
            super::most_constrained_storage_volume(&hardware.storage)
                .map(|volume| volume.mount_point.as_str()),
            Some("/")
        );
    }

    #[test]
    fn overview_summary_keeps_account_and_models_truthful() {
        let local_only_system =
            super::test_settings_system("localhost/goblins-os:test", true, true, true);
        assert_eq!(
            super::overview_account_label(None, Some(&local_only_system)),
            "local only"
        );
        assert!(super::overview_account_ready(
            None,
            Some(&local_only_system)
        ));
        assert!(
            super::overview_account_detail(None, Some(&local_only_system)).contains("local-only")
        );

        let mut catalog = super::test_local_model_catalog(Some(414));
        catalog.models = vec![
            super::test_local_model("blocked", "not-requested"),
            super::test_local_model("blocked", "not-requested"),
        ];
        catalog.models[0]
            .reasons
            .push("Requires at least 16GB RAM; detected 8GB.".to_string());

        assert_eq!(super::overview_models_label(Some(&catalog)), "blocked");
        assert!(!super::overview_models_ready(Some(&catalog)));
        let counts = super::local_model_overview_counts(&catalog.models);
        assert_eq!(
            counts,
            super::ModelOverviewCounts {
                installed: 0,
                installable: 0,
                blocked: 2,
                waiting: 0,
                total: 2,
            }
        );
        let detail = super::overview_models_detail(Some(&catalog));
        assert!(detail.contains("0 installed"));
        assert!(detail.contains("2 blocked"));
        assert!(detail.contains("First blocker"));
    }

    #[test]
    fn model_summary_tiles_use_engine_models_access_and_voice_state() {
        let key = super::test_openai_key_status(false, "local-gpt-oss");
        let privacy = super::test_privacy_status(false, true, true);
        let resident = super::test_resident_status("active", "local-gpt-oss", Some(4));
        let catalog = super::test_local_model_catalog(Some(414));
        let codex = super::test_codex_status(true, false);
        let voice = super::test_voice_status(true);

        let engine = super::active_engine_summary_spec(
            Some(&key),
            Some(&codex),
            Some(&privacy),
            Some(&resident),
        );
        assert_eq!(engine.state, "on-device");
        assert!(engine.ready);
        assert!(engine.detail.contains("GPT-OSS"));
        assert!(engine.detail.contains("Goblins AI runtime is active"));
        assert!(engine.detail.contains("Last check updated 4s ago"));
        assert!(!engine.detail.contains(&["Codex", "resident"].join(" ")));
        assert!(!engine.detail.contains("Heartbeat"));

        let waiting_resident = super::test_resident_status("waiting", "local-gpt-oss", None);
        let waiting_engine = super::active_engine_summary_spec(
            Some(&key),
            Some(&codex),
            Some(&privacy),
            Some(&waiting_resident),
        );
        assert_eq!(waiting_engine.state, "on-device");
        assert!(!waiting_engine.ready);
        assert!(waiting_engine
            .detail
            .contains("Goblins AI runtime is waiting"));

        let models = super::local_model_summary_spec(Some(&catalog));
        assert_eq!(models.state, "ready to download");
        assert!(models.ready);
        assert!(models.detail.contains("Engine llama.cpp, Ollama"));

        let access = super::openai_access_summary_spec(Some(&key), Some(&codex));
        assert_eq!(access.state, "sign in");
        assert!(!access.ready);
        assert!(access.detail.contains("not signed in"));

        let voice = super::voice_model_summary_spec(Some(&voice));
        assert_eq!(voice.state, "ready");
        assert!(voice.ready);
        assert!(voice.detail.contains("local"));
        assert!(voice.detail.contains("Wake word: Goblin"));
        assert!(voice.detail.contains("Hey Goblin"));
        assert!(voice
            .detail
            .contains("Background wake listening is not ready"));
    }

    #[test]
    fn voice_settings_copy_names_goblin_without_passive_wake_claim() {
        let voice = super::test_voice_status(false);
        let detail = super::voice_settings_detail(&voice);

        assert!(detail.contains("Wake word: Goblin"));
        assert!(detail.contains("Hey Goblin"));
        assert!(detail.contains("Background wake listening is not ready"));
        assert!(!detail.to_ascii_lowercase().contains("siri"));
        assert!(!detail.to_ascii_lowercase().contains("always listening"));
    }

    #[test]
    fn model_summary_tiles_report_private_mode_and_cloud_readiness_truthfully() {
        let hosted_key = super::test_openai_key_status(true, "openai-api");
        let private = super::test_privacy_status(true, true, true);
        let resident = super::test_resident_status("waiting", "openai-api", None);

        let hosted = super::active_engine_summary_spec(
            Some(&hosted_key),
            None,
            Some(&private),
            Some(&resident),
        );
        assert_eq!(hosted.state, "hosted");
        assert!(!hosted.ready);
        assert!(hosted.detail.contains("Private mode is on"));

        let public = super::test_privacy_status(false, true, true);
        let hosted = super::active_engine_summary_spec(
            Some(&hosted_key),
            None,
            Some(&public),
            Some(&resident),
        );
        assert_eq!(hosted.state, "hosted");
        assert!(hosted.ready);
        assert!(hosted.detail.contains("OS-owned key store"));

        let codex_key = super::test_openai_key_status(false, "codex");
        let codex = super::test_codex_status(true, true);
        let codex_engine = super::active_engine_summary_spec(
            Some(&codex_key),
            Some(&codex),
            Some(&public),
            Some(&resident),
        );
        assert_eq!(codex_engine.state, "codex");
        assert!(codex_engine.ready);
        assert!(codex_engine.detail.contains("Codex is signed in and ready"));

        let access = super::openai_access_summary_spec(Some(&hosted_key), Some(&codex));
        assert_eq!(access.state, "codex signed in");
        assert!(access.ready);

        let waiting = super::active_engine_summary_spec(None, None, None, None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn policy_summary_counts_profile_and_boundaries_from_core_state() {
        let policy = super::test_policy_status("local-only", true);
        let counts = super::policy_control_counts(&policy.controls);

        assert_eq!(
            counts,
            super::PolicyControlCounts {
                total: 4,
                allowed: 2,
                gated: 1,
                denied: 1,
                granted: 1,
            }
        );
        assert_eq!(super::policy_control_counts_label(counts), "1 gated");
        assert_eq!(
            super::policy_profile_display_name(&policy.profile),
            "Local-only"
        );
        assert_eq!(
            super::policy_data_boundary_label(&policy.profile),
            "local only"
        );
        assert!(policy.data_boundary.contains("local-only"));
        assert!(policy.secret_boundary.contains("server-side"));
        assert!(super::policy_profile_summary_detail(&policy).contains("test-generated-at"));

        let mut debug_generated_at = policy.clone();
        debug_generated_at.generated_at = "SystemTime { tv_sec: 1, tv_nsec: 2 }".to_string();
        let detail = super::policy_profile_summary_detail(&debug_generated_at);
        assert!(!detail.contains("SystemTime"));
        assert!(detail.contains("policy state is available"));

        let granted = policy
            .controls
            .iter()
            .find(|control| control.grant.is_some())
            .expect("test policy includes granted control");
        assert!(super::policy_control_status(granted).contains("granted"));
        assert!(super::policy_control_detail(granted).contains("GRANT GOBLINS"));
    }

    #[test]
    fn developer_summary_reports_core_desktop_services_and_resident_truthfully() {
        let config = super::SettingsConfig {
            core_url: "http://127.0.0.1:8787".to_string(),
            core_wait: std::time::Duration::from_secs(1),
            panel: SettingsPanel::Developer,
        };
        let core = super::developer_core_summary_spec(true, &config);
        assert_eq!(core.state, "ready");
        assert!(core.ready);
        assert!(core.detail.contains("local diagnostics"));
        assert!(!core.detail.contains("127.0.0.1:8787"));

        let system = super::test_settings_system("localhost/goblins-os:latest", true, true, true);
        let hardware = super::test_storage_hardware(Vec::new());
        let desktop = super::developer_desktop_summary_spec(Some(&system), Some(&hardware));
        assert_eq!(desktop.state, "configured");
        assert!(desktop.ready);
        assert!(desktop.detail.contains("Goblins OS desktop is active"));
        assert!(desktop.detail.contains("Memory"));

        let mut unconfigured_hardware = hardware.clone();
        unconfigured_hardware.platform.current_desktop = "unconfigured".to_string();
        let unconfigured_desktop =
            super::developer_desktop_summary_spec(Some(&system), Some(&unconfigured_hardware));
        assert_eq!(unconfigured_desktop.state, "waiting");
        assert!(!unconfigured_desktop.ready);

        let services = super::test_system_services_status(true, &["active", "failed", "active"]);
        let service_counts = super::developer_service_counts(&services.services);
        assert_eq!(
            service_counts,
            super::DeveloperServiceCounts {
                ready: 2,
                blocked: 1,
                total: 3,
            }
        );
        assert_eq!(
            super::developer_service_counts_label(services.manager_available, service_counts),
            "1 need attention"
        );
        let service_summary = super::developer_services_summary_spec(Some(&services));
        assert!(!service_summary.ready);
        assert!(service_summary.detail.contains("2 of 3 services"));
        assert!(service_summary.detail.contains("test-generated-at"));
        let mut debug_services = services.clone();
        debug_services.generated_at = "SystemTime { tv_sec: 1, tv_nsec: 2 }".to_string();
        let service_summary = super::developer_services_summary_spec(Some(&debug_services));
        assert!(!service_summary.detail.contains("SystemTime"));
        assert!(service_summary.detail.contains("Status is available."));
        assert_eq!(
            super::diagnostic_status_row_title("Network", "failed"),
            "Network · Needs attention"
        );

        let mut pathy_service = services.services[0].clone();
        pathy_service.detail =
            "systemd looked at /usr/lib/systemd/system/test.service and /usr/libexec/goblins-os/test."
                .to_string();
        let service_detail = super::diagnostic_service_detail(&pathy_service);
        assert!(service_detail.contains("system services"));
        assert!(service_detail.contains("system service files"));
        assert!(service_detail.contains("OS service tools"));
        assert!(!service_detail.contains("/usr/lib"));

        let resident = super::test_resident_status("waiting", "local-gpt-oss", None);
        let resident_summary = super::developer_resident_summary_spec(Some(&resident));
        assert_eq!(resident_summary.title, "Goblins AI runtime");
        assert_eq!(resident_summary.state, "waiting");
        assert!(!resident_summary.ready);
        assert!(resident_summary
            .detail
            .contains("Goblins AI runtime is waiting"));
        assert!(!resident_summary.detail.contains("State path"));
        assert!(!resident_summary
            .detail
            .contains(&["Codex", "resident"].join(" ")));
        let mut pathy_capability = resident.capabilities[0].clone();
        pathy_capability.detail =
            "Waiting for http://127.0.0.1:8787 and /var/lib/goblins-os/resident.".to_string();
        let capability_detail = super::diagnostic_capability_detail(&pathy_capability);
        assert!(capability_detail.contains("local diagnostics"));
        assert!(capability_detail.contains("private OS storage"));
        assert!(!capability_detail.contains("127.0.0.1"));
        assert!(!capability_detail.contains("/var/lib"));

        let facility = super::SystemFacility {
            id: "display-compositor".to_string(),
            label: "Display and compositor".to_string(),
            state: "waiting".to_string(),
            detail: "GSettings and xrandr status are unavailable.".to_string(),
            evidence: Vec::new(),
        };
        let facility_detail = super::diagnostic_facility_detail(&facility);
        assert!(facility_detail.contains("desktop preferences"));
        assert!(facility_detail.contains("display fallback"));
        assert!(!facility_detail.contains("GSettings"));
        assert!(!facility_detail.contains("xrandr"));

        assert!(
            super::native_app_handoff_detail("Logs", "review system and service logs", true)
                .contains("Logs lets you")
        );
        assert!(
            super::native_app_handoff_detail("Logs", "review system and service logs", false)
                .contains("included in the full Goblins OS image")
        );
        assert!(super::native_app_handoff_accessibility("Logs", false)
            .contains("not included in this build"));
    }

    #[test]
    fn overview_summary_reports_waiting_without_inventing_state() {
        assert_eq!(super::overview_network_label(None), "waiting");
        assert!(!super::overview_network_ready(None));
        assert!(super::overview_network_detail(None).contains("Waiting"));
        assert_eq!(super::overview_privacy_policy_label(None, None), "waiting");
        assert!(!super::overview_privacy_policy_ready(None, None));
        assert!(super::overview_privacy_policy_detail(None, None).contains("waiting"));
        assert_eq!(super::overview_recovery_label(None), "waiting");
        assert!(!super::overview_recovery_ready(None));
        assert!(super::overview_recovery_detail(None).contains("Waiting"));
    }

    #[test]
    fn overview_native_desktop_summary_uses_integrated_device_controls() {
        let mut system =
            super::test_settings_system("localhost/goblins-os:latest", true, true, true);
        system.session.desktop = "gnome-native-desktop".to_string();
        system.session.gui_platform = "gnome-session".to_string();
        system.session.shell_mode = "native-desktop".to_string();

        assert!(system.session_has_integrated_device_settings());
        assert_eq!(
            super::device_settings_readiness(Some(&system), true),
            super::DeviceSettingsReadiness::Ready
        );
        assert_eq!(
            super::overview_native_desktop_label(Some(&system), true),
            "ready"
        );
        assert!(super::overview_native_desktop_ready(Some(&system), true));

        assert_eq!(
            super::overview_native_desktop_label(Some(&system), false),
            "not available"
        );
        assert_eq!(
            super::device_settings_readiness(Some(&system), false),
            super::DeviceSettingsReadiness::Unavailable
        );
        assert!(!super::overview_native_desktop_ready(Some(&system), false));

        let mut mismatch = system.clone();
        mismatch.session.gui_platform = "gtk4".to_string();
        assert!(!mismatch.session_has_integrated_device_settings());
        assert_eq!(
            super::device_settings_readiness(Some(&mismatch), true),
            super::DeviceSettingsReadiness::IntegratedDesktopUnavailable
        );
        assert_eq!(
            super::overview_native_desktop_label(Some(&mismatch), true),
            "not ready"
        );
        assert!(!super::overview_native_desktop_ready(Some(&mismatch), true));

        let mut unreported = mismatch.clone();
        unreported.session.desktop = "unconfigured".to_string();
        unreported.session.shell_mode = "unconfigured".to_string();

        assert_eq!(
            super::device_settings_readiness(None, true),
            super::DeviceSettingsReadiness::WaitingForSession
        );
        assert_eq!(super::overview_native_desktop_label(None, true), "waiting");
        assert!(!super::overview_native_desktop_ready(None, true));

        let ready_detail = super::overview_native_settings_detail(Some(&system), true);
        assert!(ready_detail.contains("Manage display"));
        assert!(ready_detail.contains("display"));
        assert!(ready_detail.contains("OpenAI, policy, models"));
        assert!(!ready_detail.contains("Open settings"));
        assert!(!ready_detail.contains("fake controls"));

        let unavailable = super::overview_native_settings_detail(Some(&system), false);
        assert!(unavailable.contains("not supported on this device"));
        assert!(unavailable.contains("Device controls"));
        assert!(!unavailable.contains("fake controls"));

        let waiting = super::overview_native_settings_detail(None, true);
        assert!(waiting.contains("Checking device controls"));

        let mismatch_detail = super::overview_native_settings_detail(Some(&mismatch), true);
        assert!(mismatch_detail.contains("open after the desktop finishes loading"));
        assert!(!mismatch_detail.contains("full Goblins OS desktop session"));
        assert!(!mismatch_detail.contains("platform gtk4"));
        assert!(!mismatch_detail.contains("session mismatch"));
        assert!(!mismatch_detail.contains(" / "));

        let unreported_detail = super::overview_native_settings_detail(Some(&unreported), true);
        assert!(unreported_detail.contains("open after the desktop finishes loading"));
        assert!(!unreported_detail.contains("full Goblins OS desktop session"));
        assert!(!unreported_detail.contains("desktop not reported"));
        assert!(!unreported_detail.contains("shell not reported"));

        assert_eq!(
            super::overview_native_settings_accessibility(Some(&system), true),
            "Manage device controls."
        );
        assert_eq!(
            super::overview_native_settings_accessibility(Some(&mismatch), true),
            "Device controls open after the desktop finishes loading."
        );
        assert_eq!(
            super::overview_native_settings_accessibility(None, true),
            "Checking device controls."
        );
        assert_eq!(
            super::overview_native_settings_accessibility(Some(&system), false),
            "Device controls are not supported on this device."
        );
    }

    #[test]
    fn local_model_helpers_keep_download_consent_truthful() {
        let mut model = super::test_local_model("installable", "not-requested");
        assert_eq!(super::local_model_state_label(&model), "ready to download");
        assert!(super::local_model_ready(&model));
        assert_eq!(
            super::local_model_action_label(&model),
            Some("Download with consent")
        );
        assert!(super::local_model_download_disclosure(&model).contains("Records consent"));
        assert!(super::local_model_requirements(&model).contains("16GB RAM"));
        assert!(super::local_model_detail(&model).contains("No blockers reported"));

        model.install.state = "waiting-for-manifest".to_string();
        model.install.manifest_required = true;
        assert_eq!(
            super::local_model_state_label(&model),
            "waiting for provider manifest"
        );
        assert!(!super::local_model_ready(&model));
        assert_eq!(super::local_model_action_label(&model), None);
        assert!(super::local_model_download_disclosure(&model).contains("download waits"));
        let waiting_detail = super::local_model_detail(&model);
        assert!(waiting_detail.contains("Status: waiting for provider manifest."));
        assert!(waiting_detail.contains("Download requires your consent."));
        assert!(waiting_detail.contains("Provider manifest required before download."));
        assert!(waiting_detail.contains("SHA-256 verification required."));
        assert!(waiting_detail.contains("Downloads can resume if interrupted."));
        assert!(!waiting_detail.contains("waiting-for-manifest"));
        assert!(!waiting_detail.contains("consent required true"));
        assert!(!waiting_detail.contains("verification true"));

        model.state = "blocked".to_string();
        model.install.state = "not-requested".to_string();
        model.reasons = vec!["Requires more storage.".to_string()];
        assert_eq!(super::local_model_state_label(&model), "blocked");
        assert_eq!(super::local_model_action_label(&model), None);
        assert!(super::local_model_detail(&model).contains("Requires more storage."));
    }

    #[test]
    fn local_model_install_message_hides_core_paths() {
        let outcome = LocalModelInstallOutcome {
            ok: true,
            model_id: "gpt-oss-20b".to_string(),
            state: "waiting-for-manifest".to_string(),
            detail: "Consent recorded.".to_string(),
        };

        let message = super::local_model_install_message(&outcome);
        assert!(message.contains("gpt-oss-20b"));
        assert!(message.contains("Consent recorded."));
        assert!(message.contains("Status: waiting for provider manifest"));
        assert!(!message.contains("waiting-for-manifest"));
        assert!(!message.contains("/var/lib/goblins-os/models/gpt-oss-20b"));
        assert!(!message.contains("state file"));
    }

    #[test]
    fn bluetooth_setting_details_are_human_readable() {
        assert_eq!(
            bluetooth_adapter_detail(&BluetoothAdapterStatus {
                name: Some("goblins".to_string()),
                alias: Some("Goblins Workstation".to_string()),
                address: "00:11:22:33:44:55".to_string(),
            }),
            "Goblins Workstation · goblins · 00:11:22:33:44:55"
        );
        assert_eq!(
            bluetooth_adapter_state_detail(Some(true), Some(false), None),
            "Power true · Discovery false · Pairing unknown"
        );
        assert_eq!(bluetooth_power_label(true), "Bluetooth power · on");
        assert_eq!(bluetooth_power_label(false), "Bluetooth power · off");
        assert!(bluetooth_power_detail(true).contains("Bluetooth is on"));
        assert!(bluetooth_power_detail(false).contains("powered off"));
    }

    #[test]
    fn bluetooth_summary_tiles_use_core_state_without_inventing_devices() {
        let bluetooth = super::BluetoothStatus {
            source: "goblins-os-core".to_string(),
            bluez_available: true,
            service_active: true,
            adapter_present: true,
            powered: Some(true),
            discoverable: Some(false),
            pairable: Some(true),
            adapter: Some(BluetoothAdapterStatus {
                name: Some("hci0".to_string()),
                alias: Some("Goblins Workstation".to_string()),
                address: "00:11:22:33:44:55".to_string(),
            }),
            detail: "Bluetooth is active and available.".to_string(),
        };

        let service = super::bluetooth_service_summary_spec(Some(&bluetooth));
        assert_eq!(service.state, "active");
        assert!(service.ready);
        assert!(service.detail.contains("Bluetooth is on"));

        let adapter = super::bluetooth_adapter_summary_spec(Some(&bluetooth));
        assert_eq!(adapter.state, "default");
        assert!(adapter.ready);
        assert!(adapter.detail.contains("Goblins Workstation"));

        let power = super::bluetooth_power_summary_spec(Some(&bluetooth));
        assert_eq!(power.state, "on");
        assert!(power.ready);
        assert!(power.detail.contains("Bluetooth is on"));

        let visibility = super::bluetooth_visibility_summary_spec(Some(&bluetooth));
        assert_eq!(visibility.state, "pairable");
        assert!(visibility.ready);
        assert!(visibility.detail.contains("Discovery false"));
    }

    #[test]
    fn bluetooth_summary_tiles_report_missing_and_waiting_truthfully() {
        let waiting = super::bluetooth_service_summary_spec(None);
        assert_eq!(waiting.state, "waiting");
        assert!(!waiting.ready);
        assert!(waiting.detail.contains("Waiting for Bluetooth status"));

        let missing = super::BluetoothStatus {
            source: "goblins-os-core".to_string(),
            bluez_available: false,
            service_active: false,
            adapter_present: false,
            powered: None,
            discoverable: None,
            pairable: None,
            adapter: None,
            detail: "Bluetooth support is unavailable.".to_string(),
        };

        let service = super::bluetooth_service_summary_spec(Some(&missing));
        assert_eq!(service.state, "missing");
        assert!(!service.ready);
        assert!(service.detail.contains("not ready"));

        let adapter = super::bluetooth_adapter_summary_spec(Some(&missing));
        assert_eq!(adapter.state, "none");
        assert!(!adapter.ready);
        assert!(adapter.detail.contains("No Bluetooth adapter"));

        let power = super::bluetooth_power_summary_spec(Some(&missing));
        assert_eq!(power.state, "unknown");
        assert!(!power.ready);
        assert!(power.detail.contains("has not reported"));

        let visibility = super::bluetooth_visibility_summary_spec(Some(&missing));
        assert_eq!(visibility.state, "unknown");
        assert!(!visibility.ready);
        assert!(visibility.detail.contains("Discovery unknown"));
    }

    #[test]
    fn parses_bluetooth_power_outcome_without_hiding_core_text() {
        let outcome = bluetooth_power_outcome(
            br#"{"ok":false,"powered":true,"text":"Bluetooth unavailable"}"#,
        )
        .unwrap();
        assert!(!outcome.ok);
        assert!(outcome.powered);
        assert_eq!(outcome.text, "Bluetooth unavailable");
    }

    #[test]
    fn openai_account_copy_tracks_real_auth_state() {
        let storage = "/var/lib/goblins-os/secrets/openai/session.json".to_string();
        let unconfigured = OpenAIAuthStatus {
            configured: false,
            authenticated: false,
            provider: "unconfigured".to_string(),
            session_storage: storage.clone(),
            message: "No provider.".to_string(),
        };
        assert!(openai_account_detail(&unconfigured).contains("No supported provider"));

        let configured = OpenAIAuthStatus {
            configured: true,
            authenticated: false,
            provider: "openai-oidc".to_string(),
            session_storage: storage.clone(),
            message: "Provider configured.".to_string(),
        };
        assert!(openai_account_detail(&configured).contains("Sign in"));

        let signed_in = OpenAIAuthStatus {
            configured: true,
            authenticated: true,
            provider: "openai-oidc".to_string(),
            session_storage: storage,
            message: "Signed in.".to_string(),
        };
        assert!(openai_account_detail(&signed_in).contains("signed in"));
    }

    #[test]
    fn updates_about_copy_keeps_bootc_actions_truthful() {
        let ready = super::test_settings_system("localhost/goblins-os:latest", true, true, true);
        assert!(super::bootc_image_configured(&ready.services.bootc_image));
        assert_eq!(
            super::bootc_image_status_label(&ready.services.bootc_image),
            "configured"
        );
        assert!(super::bootc_image_detail(&ready).contains("update image configured"));
        assert!(super::bootc_update_actions_detail(&ready).contains("Read-only for now"));
        assert!(super::bootc_update_actions_detail(&ready).contains("secure update actions"));

        let blocked = super::test_settings_system("unconfigured", false, false, false);
        assert!(!super::bootc_image_configured(
            &blocked.services.bootc_image
        ));
        assert_eq!(
            super::bootc_image_status_label(&blocked.services.bootc_image),
            "unconfigured"
        );
        assert!(super::bootc_image_detail(&blocked).contains("update image details"));
        let detail = super::bootc_update_actions_detail(&blocked);
        assert!(detail.contains("Disabled"));
        assert!(detail.contains("update image details are not ready"));
        assert!(detail.contains("the update engine is not ready"));
        assert!(detail.contains("system health checks are not ready"));
        assert!(detail.contains("networking is not ready"));

        assert_eq!(super::GNOME_SOFTWARE, "gnome-software");
        assert!(super::native_app_handoff_detail(
            "Software",
            "review application and OS update surfaces",
            true
        )
        .contains("Software lets you"));
        assert!(super::native_app_handoff_detail(
            "Software",
            "review application and OS update surfaces",
            false
        )
        .contains("included in the full Goblins OS image"));
        assert!(super::native_app_handoff_accessibility("Software", false)
            .contains("not included in this build"));
    }

    #[test]
    fn updates_about_summary_reports_image_update_session_and_identity() {
        let ready = super::test_settings_system("localhost/goblins-os:latest", true, true, true);

        let image = super::boot_image_summary_spec(Some(&ready));
        assert_eq!(image.state, "configured");
        assert!(image.ready);
        assert!(image.detail.contains("update image configured"));

        let update = super::update_readiness_summary_spec(Some(&ready));
        assert_eq!(update.state, "ready");
        assert!(update.ready);
        assert!(update.detail.contains("update system is ready"));
        assert!(update.detail.contains("secure update actions"));

        let session = super::desktop_session_summary_spec(Some(&ready));
        assert_eq!(session.state, "configured");
        assert!(session.ready);
        assert!(session.detail.contains("Goblins OS desktop is active"));

        let identity = super::device_identity_summary_spec(Some(&ready));
        assert_eq!(identity.state, "Administrator");
        assert!(identity.ready);
        assert!(identity.detail.contains("goblins-workstation"));

        let blocked = super::test_settings_system("unconfigured", false, false, false);
        let image = super::boot_image_summary_spec(Some(&blocked));
        assert_eq!(image.state, "unconfigured");
        assert!(!image.ready);
        let update = super::update_readiness_summary_spec(Some(&blocked));
        assert_eq!(update.state, "blocked");
        assert!(!update.ready);
        assert!(update.detail.contains("Disabled"));

        let mut unconfigured_session = ready.clone();
        unconfigured_session.session.desktop = "unconfigured".to_string();
        let session = super::desktop_session_summary_spec(Some(&unconfigured_session));
        assert_eq!(session.state, "unconfigured");
        assert!(!session.ready);

        let mut missing_identity = ready.clone();
        missing_identity.local_account = None;
        let identity = super::device_identity_summary_spec(Some(&missing_identity));
        assert_eq!(identity.state, "read-only");
        assert!(!identity.ready);

        let mut unknown_identity = ready.clone();
        let account = unknown_identity
            .local_account
            .as_mut()
            .expect("test system includes local account");
        account.username = "unknown".to_string();
        account.display_name = "unknown".to_string();
        account.account_type = "Unknown".to_string();
        let identity = super::device_identity_summary_spec(Some(&unknown_identity));
        assert_eq!(identity.state, "unknown");
        assert!(!identity.ready);

        let mut debug_generated_at = ready.clone();
        debug_generated_at.generated_at = "SystemTime { tv_sec: 1, tv_nsec: 2 }".to_string();
        let image = super::boot_image_summary_spec(Some(&debug_generated_at));
        assert!(!image.detail.contains("SystemTime"));
        assert!(!image.detail.contains("source goblins-os-core"));
        let identity = super::device_identity_summary_spec(Some(&debug_generated_at));
        assert!(!identity.detail.contains("SystemTime"));
        assert!(!identity.detail.contains("source goblins-os-core"));
    }

    #[test]
    fn recovery_copy_groups_checks_and_keeps_actions_disabled_truthfully() {
        let ready = super::test_recovery_status(&["ready", "ready", "ready"]);
        let ready_counts = super::recovery_counts(&ready.checks);
        assert_eq!(
            ready_counts,
            super::RecoveryCounts {
                ready: 3,
                waiting: 0,
                total: 3,
            }
        );
        assert_eq!(super::recovery_summary_label(ready_counts), "ready");
        assert!(super::recovery_summary_detail(&ready).contains("3/3 recovery checks are ready"));
        assert!(super::recovery_actions_detail(Some(&ready)).contains("Read-only for now"));
        assert!(super::recovery_actions_detail(Some(&ready)).contains("secure recovery actions"));
        let mut debug_generated_at = ready.clone();
        debug_generated_at.generated_at = "SystemTime { tv_sec: 1, tv_nsec: 2 }".to_string();
        let generated_detail = super::recovery_summary_detail(&debug_generated_at);
        assert!(!generated_detail.contains("SystemTime"));
        assert!(generated_detail.contains("Status is available."));
        assert_eq!(
            super::recovery_check_group("bootc-tooling"),
            "Image recovery"
        );
        assert_eq!(
            super::recovery_check_group("os-core-service"),
            "Service recovery"
        );
        assert_eq!(
            super::recovery_check_group("secret-storage"),
            "State storage"
        );

        let blocked = super::test_recovery_status(&["ready", "waiting", "waiting"]);
        let blocked_counts = super::recovery_counts(&blocked.checks);
        assert_eq!(
            super::recovery_summary_label(blocked_counts),
            "needs attention"
        );
        assert!(super::recovery_summary_detail(&blocked).contains("2 still need attention"));
        let detail = super::recovery_actions_detail(Some(&blocked));
        assert!(detail.contains("Disabled"));
        assert!(detail.contains("Recovery services"));
        assert!(detail.contains("OpenAI secret storage"));
        assert!(super::recovery_actions_detail(None).contains("waiting for recovery status"));
    }

    #[test]
    fn recovery_summary_tiles_group_image_service_and_state_storage_truthfully() {
        let ready = super::test_recovery_status(&["ready", "ready", "ready"]);
        let overall = super::recovery_overall_summary_spec(Some(&ready));
        assert_eq!(overall.title, "Recovery readiness");
        assert_eq!(overall.state, "ready");
        assert!(overall.ready);

        let image =
            super::recovery_group_summary_spec(Some(&ready), "Image recovery", "Image recovery");
        assert_eq!(image.state, "ready");
        assert!(image.ready);
        assert!(image.detail.contains("1/1 image recovery checks are ready"));

        let blocked = super::test_recovery_status(&["ready", "waiting", "waiting"]);
        let service = super::recovery_group_summary_spec(
            Some(&blocked),
            "Service recovery",
            "Service recovery",
        );
        assert_eq!(service.state, "needs attention");
        assert!(!service.ready);
        assert!(service
            .detail
            .contains("1 of 1 service recovery checks need attention"));

        let state_storage =
            super::recovery_group_summary_spec(Some(&blocked), "State storage", "State storage");
        assert_eq!(state_storage.state, "needs attention");
        assert!(!state_storage.ready);
        assert_eq!(
            super::recovery_group_counts(&blocked.checks, "State storage"),
            super::RecoveryCounts {
                ready: 0,
                waiting: 1,
                total: 1,
            }
        );

        let system_facilities = super::recovery_group_summary_spec(
            Some(&blocked),
            "System facilities",
            "System facilities",
        );
        assert_eq!(system_facilities.state, "waiting");
        assert!(!system_facilities.ready);
        assert!(system_facilities
            .detail
            .contains("Waiting for system facilities checks"));

        let missing = super::recovery_overall_summary_spec(None);
        assert_eq!(missing.state, "waiting");
        assert!(!missing.ready);
        assert!(missing.detail.contains("Waiting for recovery state"));
    }

    #[test]
    fn engine_success_copy_matches_selected_engine() {
        assert!(engine_selection_success_copy("local-gpt-oss").contains("GPT-OSS"));
        assert!(engine_selection_success_copy("codex").contains("Codex"));
        assert!(engine_selection_success_copy("openai-api").contains("hosted models"));
    }

    #[test]
    fn wifi_connect_outcome_keeps_core_text() {
        let outcome = wifi_connect_outcome(
            br#"{"ok":false,"ssid":"Cafe","text":"Goblins OS could not join that network."}"#,
        )
        .unwrap();

        assert!(!outcome.ok);
        assert_eq!(
            outcome.text,
            "Goblins OS could not join that network.".to_string()
        );
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn wifi_security_labels_and_password_need_are_truthful() {
        let open = super::WifiNetwork {
            ssid: "Cafe".to_string(),
            signal: 72,
            security: String::new(),
            in_use: false,
        };
        let secure = super::WifiNetwork {
            ssid: "Studio".to_string(),
            signal: 94,
            security: "WPA2".to_string(),
            in_use: false,
        };

        assert!(!super::wifi_requires_password(&open));
        assert_eq!(super::wifi_security_label(&open.security), "open network");
        assert!(super::wifi_requires_password(&secure));
        assert_eq!(
            super::wifi_security_label(&secure.security),
            "WPA2 protected"
        );
    }
}
