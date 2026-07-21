#[cfg(not(unix))]
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::FileExt;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

use saphyr::{LoadableYamlNode, Yaml};
use sha2::{Digest, Sha256};
use syn::visit::Visit;

const REVIEWED_GITHUB_ACTION_PINS: &[(&str, &str)] = &[
    (
        "actions/checkout",
        "9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0",
    ),
    (
        "actions/upload-artifact",
        "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
    ),
    ("actions/attest", "f7c74d28b9d84cb8768d0b8ca14a4bac6ef463e6"),
    (
        "actions/download-artifact",
        "3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
    ),
    (
        "docker/setup-buildx-action",
        "bb05f3f5519dd87d3ba754cc423b652a5edd6d2c",
    ),
    (
        "docker/build-push-action",
        "53b7df96c91f9c12dcc8a07bcb9ccacbed38856a",
    ),
];

const DEPRECATED_GITHUB_ACTION_PINS: &[&str] = &[
    "34e114876b0b11c390a56381ad16ebd13914f8d5",
    "ea165f8d65b6e75b540449e92b4886f43607fa02",
    "d3f86a106a0bac45b974a628896c90dbdf5c8093",
    "8d2750c68a42422c14e847fe6c8ac0403b4cbd6f",
];

const BINARIES: &[&str] = &[
    "goblins-os-control-center",
    "goblins-os-core",
    "goblins-os-dictate",
    "goblins-os-file-builder",
    "goblins-os-focus-tick",
    "goblins-os-installer",
    "goblins-os-launcher",
    "goblins-os-login",
    "goblins-os-markup",
    "goblins-os-open",
    "goblins-os-resident",
    "goblins-os-screenshot-context",
    "goblins-os-session-bridge",
    "goblins-os-settings",
    "goblins-os-shell",
    "goblins-os-today",
    "goblins-os-verify",
    "goblins-os-visual-lookup",
    "goblins-os-voice-control",
];

const DESKTOP_CAPABILITY_BINARIES: &[&str] = &[
    "goblins-os-control-center",
    "goblins-os-dictate",
    "goblins-os-file-builder",
    "goblins-os-focus-tick",
    "goblins-os-installer",
    "goblins-os-launcher",
    "goblins-os-login",
    "goblins-os-markup",
    "goblins-os-open",
    "goblins-os-screenshot-context",
    "goblins-os-settings",
    "goblins-os-shell",
    "goblins-os-today",
    "goblins-os-visual-lookup",
    "goblins-os-voice-control",
];

const SYSTEMD_UNITS: &[&str] = &[
    "goblins-os-core.service",
    "goblins-os-resident.service",
    "goblins-os-model-cache.service",
];

const SYSTEMD_SYSTEM_DROPINS: &[&str] =
    &["systemd-remount-fs.service.d/10-goblins-os-composefs.conf"];

const SYSTEMD_USER_UNITS: &[&str] = &[
    "gnome-session@goblins-os.target.d/goblins-os.session.conf",
    "org.goblins.OS.Shell.target",
    "org.goblins.OS.Shell.service",
    "org.goblins.OS.SessionBridge.service",
];

const APPLICATIONS: &[&str] = &[
    "org.goblins.OS.OpenAI.Agents.desktop",
    "org.goblins.OS.OpenAI.ChatGPT.desktop",
    "org.goblins.OS.OpenAI.Codex.desktop",
    "org.goblins.OS.OpenAI.Images.desktop",
    "org.goblins.OS.OpenAI.Platform.desktop",
    "org.goblins.OS.OpenAI.Responses.desktop",
    "org.goblins.OS.OpenAI.VoiceRealtime.desktop",
    "org.goblins.OS.Policy.desktop",
    "org.goblins.OS.Recovery.desktop",
    "org.goblins.OS.Settings.desktop",
    "org.goblins.OS.Shell.desktop",
    "org.goblins.OS.Today.desktop",
    "org.goblins.OS.VisualLookup.desktop",
];

const AUTOSTART: &[&str] = &["org.goblins.OS.Installer.desktop"];

const DCONF_FILES: &[&str] = &[
    "00-goblins-os-first-run",
    "10-goblins-os-desktop",
    "20-goblins-os-today",
    "30-captions",
    "40-sound-recognition",
];

const GLIB_SCHEMA_FILES: &[&str] = &[
    "org.goblins.SoundRecognition.gschema.xml",
    "org.goblins.os.a11y.switch-control.gschema.xml",
    "org.goblins.os.focus.gschema.xml",
    "org.goblins.os.today.gschema.xml",
    "org.goblins.shell.extensions.wm.gschema.xml",
    "org.goblins.shell.extensions.captions.gschema.xml",
];

const GNOME_SHELL_EXTENSION_FILES: &[&str] = &[
    "goblins-switch@goblins.os/metadata.json",
    "goblins-switch@goblins.os/extension.js",
    "goblins-switch@goblins.os/stylesheet.css",
    "goblins-wm@goblins.os/metadata.json",
    "goblins-wm@goblins.os/extension.js",
    "goblins-wm@goblins.os/stylesheet.css",
    "goblins-wm@goblins.os/schemas/org.goblins.shell.extensions.wm.gschema.xml",
];

const ICON_THEME_FILES: &[&str] = &[
    "index.theme",
    "scalable/actions/document-build-symbolic.svg",
    "scalable/actions/goblins-engine-symbolic.svg",
    "scalable/actions/preferences-desktop-appearance-symbolic.svg",
    "scalable/apps/org.gnome.Console.svg",
    "scalable/apps/org.gnome.Nautilus.svg",
    "scalable/apps/org.goblins.OS.Installer.svg",
    "scalable/apps/org.goblins.OS.OpenAI.Codex.svg",
    "scalable/apps/org.goblins.OS.Settings.svg",
    "scalable/status/audio-volume-high-symbolic.svg",
    "scalable/status/display-brightness-symbolic.svg",
    "scalable/status/network-wireless-symbolic.svg",
];

const NAUTILUS_SCRIPTS: &[&str] = &[
    "scripts/Build an app to open this",
    "scripts/Ask Goblin about this",
];

const NATIVE_DESIGN_APPS: &[&str] = &[
    "goblins-os-control-center",
    "goblins-os-installer",
    "goblins-os-launcher",
    "goblins-os-login",
    "goblins-os-settings",
    "goblins-os-shell",
    "goblins-os-today",
];

const SETTINGS_RENDER_SCREENSHOTS: &[&str] = &[
    "03-settings.png",
    "05-settings-models.png",
    "11-settings-dark.png",
    "19-settings-policy.png",
    "20-settings-recovery.png",
    "23-settings-models-dark.png",
    "33-settings-policy-dark.png",
    "34-settings-recovery-dark.png",
    "46-settings-appearance.png",
    "47-settings-network.png",
    "48-settings-bluetooth.png",
    "49-settings-displays.png",
    "50-settings-sound.png",
    "51-settings-keyboard.png",
    "52-settings-mouse-trackpad.png",
    "53-settings-accessibility.png",
    "54-settings-desktop-wallpaper.png",
    "55-settings-notifications.png",
    "110-settings-lock-screen.png",
    "56-settings-users-accounts.png",
    "57-settings-privacy-permissions.png",
    "58-settings-storage.png",
    "59-settings-updates-about.png",
    "60-settings-developer.png",
    "76-settings-applications.png",
    "77-settings-wired-vpn.png",
    "78-settings-mobile-broadband.png",
    "79-settings-sharing.png",
    "80-settings-color.png",
    "81-settings-drawing-tablet.png",
    "82-settings-search.png",
    "83-settings-multitasking.png",
    "84-settings-power-battery.png",
    "116-settings-games.png",
    "85-settings-printers-scanners.png",
    "111-settings-date-time.png",
    "112-settings-language-region.png",
    "86-settings-online-accounts.png",
    "87-settings-wellbeing.png",
    "104-settings-security.png",
    "105-settings-desktop-dock.png",
    "106-settings-menu-bar.png",
    "61-settings-appearance-dark.png",
    "62-settings-network-dark.png",
    "63-settings-bluetooth-dark.png",
    "64-settings-displays-dark.png",
    "65-settings-sound-dark.png",
    "66-settings-keyboard-dark.png",
    "67-settings-mouse-trackpad-dark.png",
    "68-settings-accessibility-dark.png",
    "69-settings-desktop-wallpaper-dark.png",
    "70-settings-notifications-dark.png",
    "113-settings-lock-screen-dark.png",
    "71-settings-users-accounts-dark.png",
    "72-settings-privacy-permissions-dark.png",
    "73-settings-storage-dark.png",
    "74-settings-updates-about-dark.png",
    "75-settings-developer-dark.png",
    "88-settings-applications-dark.png",
    "89-settings-wired-vpn-dark.png",
    "90-settings-mobile-broadband-dark.png",
    "91-settings-sharing-dark.png",
    "92-settings-color-dark.png",
    "93-settings-drawing-tablet-dark.png",
    "94-settings-search-dark.png",
    "95-settings-multitasking-dark.png",
    "96-settings-power-battery-dark.png",
    "117-settings-games-dark.png",
    "97-settings-printers-scanners-dark.png",
    "114-settings-date-time-dark.png",
    "115-settings-language-region-dark.png",
    "98-settings-online-accounts-dark.png",
    "99-settings-wellbeing-dark.png",
    "107-settings-security-dark.png",
    "108-settings-desktop-dock-dark.png",
    "109-settings-menu-bar-dark.png",
];

const SETTINGS_INTERACTION_SCREENSHOTS: &[&str] = &[
    "100-settings-search-wifi-filter.png",
    "101-settings-search-enter-network.png",
    "102-settings-search-no-results.png",
    "103-settings-search-cleared.png",
    "118-settings-firewall-before.png",
    "119-settings-firewall-toggle-failed.png",
];

const POLISH_INTERACTION_SCREENSHOTS: &[&str] = &[
    "124-settings-models-advanced-collapsed.png",
    "125-settings-models-advanced-expanded.png",
    "126-settings-models-engine-offline-error.png",
    "127-studio-engine-menu.png",
    "128-studio-engine-offline-error.png",
    "129-first-app-grant-required.png",
    "130-first-app-policy-granted.png",
    "131-first-app-offline-error.png",
    "132-first-app-policy-blocked.png",
    "133-setup-accessibility-reduced-motion.png",
    "134-install-progress-reduced-motion-a.png",
    "135-install-progress-reduced-motion-b.png",
    "136-settings-models-advanced-expanded-dark.png",
    "137-studio-engine-menu-dark.png",
    "138-first-boot-codex-offline.png",
];
const POLISH_INTERACTION_PROOF: &str = "139-polish-interactions-proof.json";

const GAMING_PROOF_SCREENSHOTS: &[&str] = &[
    "19-vulkan-vkcube.png",
    "20-gamemode-active.png",
    "21-gamescope-session.png",
    "22-mangohud-overlay.png",
    "23-controller-detection.png",
    "24-audio-output.png",
];

const INSTALL_STORAGE_PROOF_SCREENSHOTS: &[&str] = &[
    "25-install-destination.png",
    "26-install-storage-summary.png",
    "27-dual-boot-preserve-existing-os.png",
    "28-bootloader-efi-summary.png",
];

const SUPPORTED_RELEASE_ARCHES: &[&str] = &["aarch64", "x86_64"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Source,
    Installed,
    Stage,
    ReleaseEvidence,
    WorkflowActions,
}

struct Config {
    mode: Mode,
    root: PathBuf,
    source: PathBuf,
    binaries: PathBuf,
    quiet: bool,
    release_arch: Option<String>,
    candidate_commit: Option<String>,
    image_ref: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CheckState {
    Ready,
    Blocked,
}

struct Check {
    id: String,
    state: CheckState,
    detail: String,
}

#[derive(Debug, PartialEq, Eq)]
enum VerifyError {
    Usage,
    UnknownArgument(String),
    MissingValue(String),
    InvalidArchitecture(String),
    InvalidCandidateCommit(String),
    InvalidImageRef(String),
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("goblins-os-verify: {error}");
            std::process::exit(64);
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let config = Config::from_env()?;
    if config.mode == Mode::ReleaseEvidence {
        let arch = config
            .release_arch
            .as_deref()
            .ok_or("release evidence requires --arch")?;
        let candidate_commit = config
            .candidate_commit
            .as_deref()
            .ok_or("release evidence requires --candidate-commit")?;
        let image_ref = config
            .image_ref
            .as_deref()
            .ok_or("release evidence requires --image-ref")?;
        let manifest = write_release_evidence(
            &config.source,
            arch,
            candidate_commit,
            image_ref,
            &config.root,
        )?;
        if !config.quiet {
            println!(
                "goblins_os_release_evidence arch={} output={} manifest={}",
                arch,
                config.root.display(),
                manifest.display()
            );
        }
        return Ok(());
    }

    let checks = match config.mode {
        Mode::Source => source_checks(&config.root),
        Mode::Installed => installed_checks(&config.root),
        Mode::Stage => {
            stage_install(&config.source, &config.binaries, &config.root).map_err(|error| {
                format!("staging into {} failed: {error}", config.root.display())
            })?;
            if !config.quiet {
                println!(
                    "goblins_os_verify_staged source={} binaries={} destdir={}",
                    config.source.display(),
                    config.binaries.display(),
                    config.root.display()
                );
            }
            installed_checks(&config.root)
        }
        Mode::ReleaseEvidence => unreachable!("release evidence mode returns before checks run"),
        Mode::WorkflowActions => vec![
            reviewed_github_action_pins_check(&config.root),
            deprecated_github_action_pins_absent_check(&config.root),
        ],
    };
    let blocked = checks
        .iter()
        .filter(|check| check.state == CheckState::Blocked)
        .count();

    if !config.quiet {
        println!(
            "goblins_os_verify mode={} root={}",
            config.mode.as_str(),
            config.root.display()
        );
        for check in &checks {
            println!("{} {} {}", check.state.as_str(), check.id, check.detail);
        }
        println!(
            "goblins_os_verify_result total={} blocked={blocked}",
            checks.len()
        );
    }

    if blocked == 0 {
        Ok(())
    } else {
        Err(format!("{blocked} packaging check failed").into())
    }
}

impl Config {
    fn from_env() -> Result<Self, VerifyError> {
        let mut args = env::args().skip(1);
        let mut source_root = None;
        let mut installed_root = None;
        let mut stage_root = None;
        let mut binaries = None;
        let mut release_evidence_output = None;
        let mut release_arch = None;
        let mut candidate_commit = None;
        let mut image_ref = None;
        let mut workflow_action_pins_root = None;
        let mut quiet = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--source-root" => {
                    source_root = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    ));
                }
                "--installed-root" => {
                    installed_root = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    ));
                }
                "--stage" => {
                    stage_root = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    ));
                }
                "--binaries" => {
                    binaries = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    ));
                }
                "--release-evidence" => {
                    release_evidence_output = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    ));
                }
                "--arch" => {
                    release_arch = Some(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    );
                }
                "--candidate-commit" => {
                    candidate_commit = Some(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    );
                }
                "--image-ref" => {
                    image_ref = Some(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    );
                }
                "--workflow-action-pins" => {
                    workflow_action_pins_root = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| VerifyError::MissingValue(arg.clone()))?,
                    ));
                }
                "--quiet" => quiet = true,
                "--help" | "-h" => return Err(VerifyError::Usage),
                _ => return Err(VerifyError::UnknownArgument(arg)),
            }
        }

        if let Some(root) = workflow_action_pins_root {
            if source_root.is_some()
                || installed_root.is_some()
                || stage_root.is_some()
                || binaries.is_some()
                || release_evidence_output.is_some()
                || release_arch.is_some()
                || candidate_commit.is_some()
                || image_ref.is_some()
            {
                return Err(VerifyError::Usage);
            }
            return Ok(Self {
                mode: Mode::WorkflowActions,
                source: root.clone(),
                binaries: root.join("target/release"),
                root,
                quiet,
                release_arch: None,
                candidate_commit: None,
                image_ref: None,
            });
        }

        if let Some(output) = release_evidence_output {
            if installed_root.is_some() || stage_root.is_some() || binaries.is_some() {
                return Err(VerifyError::Usage);
            }
            let arch = release_arch.ok_or(VerifyError::Usage)?;
            if !SUPPORTED_RELEASE_ARCHES.contains(&arch.as_str()) {
                return Err(VerifyError::InvalidArchitecture(arch));
            }
            let candidate_commit = candidate_commit.ok_or(VerifyError::Usage)?;
            if !candidate_commit_is_valid(&candidate_commit) {
                return Err(VerifyError::InvalidCandidateCommit(candidate_commit));
            }
            let image_ref = image_ref.ok_or(VerifyError::Usage)?;
            if !image_ref_is_valid(&image_ref) {
                return Err(VerifyError::InvalidImageRef(image_ref));
            }
            let source = match source_root {
                Some(root) => root,
                None => env::current_dir().map_err(|_| VerifyError::Usage)?,
            };
            return Ok(Self {
                mode: Mode::ReleaseEvidence,
                root: output,
                source: source.clone(),
                binaries: source.join("target/release"),
                quiet,
                release_arch: Some(arch),
                candidate_commit: Some(candidate_commit.to_ascii_lowercase()),
                image_ref: Some(image_ref),
            });
        }

        if let Some(destdir) = stage_root {
            if source_root.is_some() || installed_root.is_some() {
                return Err(VerifyError::Usage);
            }
            let source = env::current_dir().map_err(|_| VerifyError::Usage)?;
            let binaries = binaries.unwrap_or_else(|| source.join("target/release"));
            return Ok(Self {
                mode: Mode::Stage,
                root: destdir,
                source,
                binaries,
                quiet,
                release_arch: None,
                candidate_commit: None,
                image_ref: None,
            });
        }

        let default_binaries = |root: &PathBuf| root.join("target/release");
        match (source_root, installed_root) {
            (Some(root), None) => Ok(Self {
                mode: Mode::Source,
                source: root.clone(),
                binaries: default_binaries(&root),
                root,
                quiet,
                release_arch: None,
                candidate_commit: None,
                image_ref: None,
            }),
            (None, Some(root)) => Ok(Self {
                mode: Mode::Installed,
                source: root.clone(),
                binaries: default_binaries(&root),
                root,
                quiet,
                release_arch: None,
                candidate_commit: None,
                image_ref: None,
            }),
            (None, None) => {
                let current = env::current_dir().map_err(|_| VerifyError::Usage)?;
                if current.join("os/bootc/Containerfile").is_file() {
                    Ok(Self {
                        mode: Mode::Source,
                        source: current.clone(),
                        binaries: default_binaries(&current),
                        root: current,
                        quiet,
                        release_arch: None,
                        candidate_commit: None,
                        image_ref: None,
                    })
                } else {
                    Ok(Self {
                        mode: Mode::Installed,
                        source: PathBuf::from("/"),
                        binaries: PathBuf::from("/usr/libexec/goblins-os"),
                        root: PathBuf::from("/"),
                        quiet,
                        release_arch: None,
                        candidate_commit: None,
                        image_ref: None,
                    })
                }
            }
            (Some(_), Some(_)) => Err(VerifyError::Usage),
        }
    }
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Installed => "installed",
            Self::Stage => "stage",
            Self::ReleaseEvidence => "release-evidence",
            Self::WorkflowActions => "workflow-actions",
        }
    }
}

impl CheckState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
        }
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => formatter.write_str(
                "usage: goblins-os-verify [--source-root <path> | --installed-root <path> | --stage <destdir> [--binaries <dir>] | --release-evidence <output-dir> --arch <aarch64|x86_64> --candidate-commit <40-hex-commit> --image-ref <container-image-ref> | --workflow-action-pins <source-root>] [--quiet]",
            ),
            Self::UnknownArgument(arg) => write!(formatter, "unknown argument {arg}"),
            Self::MissingValue(arg) => write!(formatter, "missing value for {arg}"),
            Self::InvalidArchitecture(arch) => write!(
                formatter,
                "unsupported architecture {arch}; expected aarch64 or x86_64"
            ),
            Self::InvalidCandidateCommit(commit) => write!(
                formatter,
                "invalid candidate commit {commit}; expected exactly 40 hexadecimal characters"
            ),
            Self::InvalidImageRef(image_ref) => write!(
                formatter,
                "invalid image ref {image_ref}; expected a nonempty container image reference without whitespace"
            ),
        }
    }
}

impl Error for VerifyError {}

fn source_checks(root: &Path) -> Vec<Check> {
    let mut checks = vec![
        file_check(root, "Cargo.toml"),
        file_check(root, "Cargo.lock"),
        file_check(root, "LICENSE"),
        file_check(root, "crates/goblins-os-ai/src/lib.rs"),
        file_check(root, "crates/goblins-os-design/src/lib.rs"),
        file_check(root, "os/bootc/Containerfile"),
        file_check(root, "os/etc/goblins-os/environment"),
        file_check(root, "os/etc/goblins-os/openai-secrets.env"),
        file_check(root, "os/bootc-install/00-goblins-os.toml"),
        file_check(root, "os/session/goblins-os.desktop"),
        file_check(root, "os/session/goblins-os-session"),
        file_check(root, "os/gnome-session/goblins-os.session"),
        file_check(root, "os/gtk-4.0/gtk.css"),
        file_check(root, "os/sounds/GoblinsOS/index.theme"),
    ];

    for script in NAUTILUS_SCRIPTS {
        checks.push(file_check(root, &format!("os/nautilus/{script}")));
    }

    for dconf in DCONF_FILES {
        checks.push(file_check(root, &format!("os/dconf/db/local.d/{dconf}")));
    }
    for schema in GLIB_SCHEMA_FILES {
        checks.push(file_check(root, &format!("os/glib-schemas/{schema}")));
    }
    for extension_file in GNOME_SHELL_EXTENSION_FILES {
        checks.push(file_check(
            root,
            &format!("os/gnome-shell-extensions/{extension_file}"),
        ));
    }
    for icon in ICON_THEME_FILES {
        checks.push(file_check(root, &format!("os/icons/GoblinsOS/{icon}")));
    }
    for unit in SYSTEMD_UNITS {
        checks.push(file_check(root, &format!("os/systemd/{unit}")));
    }
    for dropin in SYSTEMD_SYSTEM_DROPINS {
        checks.push(file_check(root, &format!("os/systemd-system/{dropin}")));
    }
    for unit in SYSTEMD_USER_UNITS {
        checks.push(file_check(root, &format!("os/systemd-user/{unit}")));
    }
    for app in APPLICATIONS {
        checks.push(file_check(root, &format!("os/applications/{app}")));
    }
    checks.push(file_check(root, "os/applications/mimeapps.list"));
    checks.push(contains_check(
        root.join("os/applications/mimeapps.list"),
        "preview-pdf-defaults-to-papers",
        "application/pdf=org.gnome.Papers.desktop",
    ));
    checks.push(contains_check(
        root.join("os/applications/mimeapps.list"),
        "preview-png-defaults-to-loupe",
        "image/png=org.gnome.Loupe.desktop",
    ));
    checks.push(contains_check(
        root.join("os/applications/mimeapps.list"),
        "preview-jpeg-defaults-to-loupe",
        "image/jpeg=org.gnome.Loupe.desktop",
    ));
    for app in AUTOSTART {
        checks.push(file_check(root, &format!("os/autostart/{app}")));
    }
    for binary in BINARIES {
        checks.push(workspace_member_check(root, binary));
        checks.push(container_copy_check(root, binary));
    }

    checks.push(container_contains_check(
        root,
        "fedora-bootc-base",
        "FROM quay.io/fedora/fedora-bootc:44",
    ));
    checks.push(container_contains_check(
        root,
        "bootc-lint",
        "bootc container lint",
    ));
    checks.push(container_contains_check(
        root,
        "bootc-package-install-disables-weak-deps",
        "--setopt=install_weak_deps=False",
    ));
    checks.push(container_contains_check(
        root,
        "bootc-package-install-pins-normal-kernel-modules-extra",
        "kernel-modules-extra-${kernel_release}",
    ));
    checks.push(container_absent_check(
        root,
        "bootc-source-does-not-install-debug-kernel",
        "kernel-debug",
    ));
    // The OpenAI account credential (CODEX_HOME/auth.json) must be reachable only
    // by the goblins-os service user: CODEX_HOME is created owner-only 0700, and
    // the desktop login user is NOT placed in the goblins-os service group.
    checks.push(container_contains_check(
        root,
        "codex-home-owner-only-0700",
        "-m 0700 /var/lib/goblins-os/codex",
    ));
    checks.push(container_absent_check(
        root,
        "codex-login-user-not-in-service-group",
        "usermod -aG goblins-os goblin",
    ));
    checks.push(container_contains_check(
        root,
        "native-desktop-features",
        "goblins-os-shell/native-desktop",
    ));
    checks.push(container_contains_check(
        root,
        "native-desktop-ui-feature",
        "goblins-os-ui/native-desktop",
    ));
    checks.push(session_contains_check(
        root,
        "gnome-session",
        "gnome-session --session=goblins-os",
    ));
    checks.push(session_contains_check(
        root,
        "dbus-session",
        "dbus-run-session",
    ));
    checks.push(container_contains_check(
        root,
        "gnome-session-systemd-user-install",
        "COPY os/systemd-user/ /usr/lib/systemd/user/",
    ));
    checks.push(container_contains_check(
        root,
        "gnome-control-center-settings",
        "gnome-control-center",
    ));
    checks.push(container_contains_check(
        root,
        "gnome-disk-usage-analyzer",
        "baobab",
    ));
    checks.push(container_contains_check(
        root,
        "gnome-disk-utility",
        "gnome-disk-utility",
    ));
    checks.push(container_contains_check(
        root,
        "gnome-logs-diagnostics",
        "gnome-logs",
    ));
    checks.push(container_contains_check(
        root,
        "gnome-software-updates",
        "gnome-software",
    ));
    checks.push(container_contains_check(
        root,
        "gnome-software-rpm-ostree-updates",
        "gnome-software-rpm-ostree",
    ));
    checks.push(container_contains_check(
        root,
        "gnome-system-monitor-diagnostics",
        "gnome-system-monitor",
    ));
    checks.push(container_contains_check(
        root,
        "desktop-wifi-support",
        "NetworkManager-wifi",
    ));
    checks.push(container_contains_check(
        root,
        "desktop-bluetooth-support",
        "gnome-bluetooth",
    ));
    checks.push(container_contains_check(
        root,
        "desktop-power-profile-support",
        "tuned-ppd",
    ));
    checks.push(container_contains_check(
        root,
        "desktop-gpu-switching-support",
        "switcheroo-control",
    ));
    checks.push(container_contains_check(
        root,
        "desktop-mesa-dri-support",
        "mesa-dri-drivers",
    ));
    checks.push(container_contains_check(
        root,
        "systemd-system-dropins-install",
        "COPY os/systemd-system/ /usr/lib/systemd/system/",
    ));
    checks.push(contains_check(
        root.join("os/systemd-system/systemd-remount-fs.service.d/10-goblins-os-composefs.conf"),
        "ostree-composefs-remount-skip",
        "ConditionKernelCommandLine=!ostree",
    ));
    checks.push(contains_check(
        root.join("os/gnome-session/goblins-os.session"),
        "gnome-session-settings-daemon-components",
        "org.gnome.SettingsDaemon.Power",
    ));
    checks.push(absent_check(
        root.join("os/gnome-session/goblins-os.session"),
        "gnome-session-shell-not-required-component",
        "org.goblins.OS.Shell",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "gnome-session-systemd-gnome-shell-service",
        "Requires=org.gnome.Shell@user.service",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "gnome-session-systemd-shell-service-wanted",
        "Wants=org.goblins.OS.Shell.service",
    ));
    checks.push(absent_check(
        root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "gnome-session-systemd-shell-target-not-required",
        "Requires=org.goblins.OS.Shell.target",
    ));
    checks.push(absent_check(
        root.join("os/systemd-user/org.goblins.OS.Shell.target"),
        "goblins-shell-target-no-initialized-requisite",
        "Requisite=gnome-session-initialized.target",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "gnome-session-systemd-settings-daemon-targets",
        "Wants=org.gnome.SettingsDaemon.Power.target",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/org.goblins.OS.Shell.service"),
        "goblins-shell-user-service-exec",
        "ExecStart=/usr/libexec/goblins-os/goblins-os-shell",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/org.goblins.OS.Shell.service"),
        "goblins-shell-user-service-session-partof",
        "PartOf=gnome-session-initialized.target",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/org.goblins.OS.SessionBridge.service"),
        "session-bridge-user-service-exec",
        "ExecStart=/usr/libexec/goblins-os/goblins-os-session-bridge",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "session-bridge-wanted-by-gnome-session",
        "Wants=org.goblins.OS.SessionBridge.service",
    ));
    checks.push(contains_check(
        root.join("os/systemd/goblins-os-core.service"),
        "core-service-joins-session-bridge-group-only",
        "SupplementaryGroups=goblins-session-bridge",
    ));
    checks.push(contains_check(
        root.join("os/systemd/goblins-os-core.service"),
        "core-service-restart-policy-always",
        "Restart=always",
    ));
    checks.push(contains_check(
        root.join("os/systemd/goblins-os-core.service"),
        "core-service-start-limit-disabled",
        "StartLimitIntervalSec=0",
    ));
    checks.push(contains_check(
        root.join("os/systemd/goblins-os-core.service"),
        "core-service-owns-policy-state-directory",
        "StateDirectory=goblins-os/models goblins-os/ai goblins-os/policy",
    ));
    checks.push(contains_check(
        root.join("os/systemd/goblins-os-core.service"),
        "core-service-state-directory-mode-private",
        "StateDirectoryMode=0750",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/org.goblins.OS.SessionBridge.service"),
        "session-bridge-service-restart-policy-always",
        "Restart=always",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/org.goblins.OS.SessionBridge.service"),
        "session-bridge-service-start-limit-disabled",
        "StartLimitIntervalSec=0",
    ));
    checks.push(contains_check(
        root.join("os/bootc/Containerfile"),
        "bootc-creates-session-bridge-group",
        "groupadd --system goblins-session-bridge",
    ));
    checks.push(contains_check(
        root.join("os/bootc/Containerfile"),
        "bootc-image-oci-title-label",
        r#"org.opencontainers.image.title="Goblins OS""#,
    ));
    checks.push(contains_check(
        root.join("os/bootc/Containerfile"),
        "bootc-image-oci-source-label",
        r#"org.opencontainers.image.source="https://github.com/Joe-Simo/goblins-os""#,
    ));
    checks.push(contains_check(
        root.join("os/bootc/Containerfile"),
        "bootc-image-oci-license-label",
        r#"org.opencontainers.image.licenses="AGPL-3.0-or-later""#,
    ));
    checks.push(contains_check(
        root.join("os/bootc/Containerfile"),
        "bootc-image-oci-site-label",
        r#"org.opencontainers.image.url="https://goblinsos.com""#,
    ));
    checks.push(contains_check(
        root.join("os/bootc/Containerfile"),
        "bootc-self-tests-session-bridge",
        "goblins-os-session-bridge --self-test",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-session-bridge/src/main.rs"),
        "session-bridge-rejects-arbitrary-gsettings-schemas",
        "non-allowlisted schema was accepted",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-session-bridge/src/main.rs"),
        "session-bridge-accepts-permission-store-delete-op",
        "PermissionStoreDelete",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-session-bridge/src/main.rs"),
        "session-bridge-limits-permission-store-delete-tables",
        "PermissionStore deletes are limited to app-keyed tables",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-core/src/displays.rs"),
        "display-config-uses-session-bridge-before-direct-gdbus",
        "display_config_get_current_state",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-session-bridge/src/main.rs"),
        "session-bridge-accepts-display-config-apply-op",
        "DisplayConfigApplyMonitors",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-session-bridge/src/main.rs"),
        "session-bridge-limits-display-config-layouts",
        "validate_display_config_logical_monitors",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-core/src/session_bridge.rs"),
        "core-session-bridge-client-uses-unix-socket",
        "UnixStream::connect",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-core/src/session_bridge.rs"),
        "core-session-bridge-client-supports-permission-store-delete",
        "permission_store_delete_permission",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-core/src/session_bridge.rs"),
        "core-session-bridge-client-supports-display-config-apply",
        "display_config_apply_monitors",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-core/src/session_bridge.rs"),
        "core-session-bridge-default-socket-is-group-traversable",
        "/run/goblins-os-session/session-bridge.sock",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-session-bridge/src/main.rs"),
        "session-bridge-default-socket-is-group-traversable",
        "/run/goblins-os-session/session-bridge.sock",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/org.goblins.OS.SessionBridge.service"),
        "session-bridge-user-service-uses-group-traversable-socket",
        "GOBLINS_OS_SESSION_BRIDGE_SOCKET=/run/goblins-os-session/session-bridge.sock",
    ));
    checks.push(contains_check(
        root.join("os/tmpfiles/goblins-os-session.conf"),
        "session-bridge-runtime-dir-group-traversable",
        "d /run/goblins-os-session 0770 goblin goblins-session-bridge -",
    ));
    checks.push(contains_check(
        root.join("os/bootc/Containerfile"),
        "container-installs-session-bridge-tmpfiles",
        "COPY os/tmpfiles/goblins-os-session.conf /usr/lib/tmpfiles.d/goblins-os-session.conf",
    ));
    checks.push(contains_check(
        root.join("os/bootc/Containerfile"),
        "container-verifies-session-bridge-tmpfiles",
        "grep -Fq 'd /run/goblins-os-session 0770 goblin goblins-session-bridge -' /usr/lib/tmpfiles.d/goblins-os-session.conf",
    ));
    checks.push(container_order_check(
        root,
        "container-copies-session-bridge-tmpfiles-before-verifier",
        "COPY os/tmpfiles/goblins-os-session.conf /usr/lib/tmpfiles.d/goblins-os-session.conf",
        "test -f /usr/lib/tmpfiles.d/goblins-os-session.conf",
    ));
    checks.push(absent_check(
        root.join("os/etc/goblins-os/environment"),
        "environment-does-not-publish-browser-access-to-private-core-routes",
        "GOBLINS_OS_CORE_URL=",
    ));
    checks.push(contains_check(
        root.join("os/etc/goblins-os/environment"),
        "environment-session-bridge-socket-is-stable",
        "GOBLINS_OS_SESSION_BRIDGE_SOCKET=/run/goblins-os-session/session-bridge.sock",
    ));
    checks.push(contains_check(
        root.join("os/etc/goblins-os/environment"),
        "environment-primary-core-port-is-goblins-native",
        "GOBLINS_OS_CORE_PORT=8787",
    ));
    checks.push(absent_check(
        root.join("os/etc/goblins-os/environment"),
        "environment-does-not-ship-legacy-core-url",
        "OPENAI_OS_CORE_URL=",
    ));
    checks.push(absent_check(
        root.join("os/etc/goblins-os/environment"),
        "environment-does-not-ship-legacy-core-port",
        "OPENAI_OS_CORE_PORT=",
    ));
    checks.push(absent_check(
        root.join("os/etc/goblins-os/environment"),
        "environment-does-not-advertise-openai-os-prefix",
        "OPENAI_OS_",
    ));
    checks.push(absent_check(
        root.join("os/session/goblins-os-session"),
        "session-does-not-export-a-browser-core-url",
        "GOBLINS_OS_CORE_URL",
    ));
    checks.push(absent_check(
        root.join("os/session/goblins-os-session"),
        "session-does-not-export-legacy-core-url",
        "export OPENAI_OS_CORE_URL=",
    ));
    checks.push(absent_check(
        root.join("os/systemd-user/org.goblins.OS.Shell.service"),
        "shell-service-does-not-export-a-browser-core-url",
        "Environment=GOBLINS_OS_CORE_URL",
    ));
    checks.push(absent_check(
        root.join("os/systemd-user/org.goblins.OS.Shell.service"),
        "shell-service-does-not-directly-export-legacy-core-url",
        "Environment=OPENAI_OS_CORE_URL",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-core/src/main.rs"),
        "core-port-prefers-goblins-env-name",
        "std::env::var(\"GOBLINS_OS_CORE_PORT\")",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-core/src/main.rs"),
        "core-port-keeps-openai-env-compatibility",
        "std::env::var(\"OPENAI_OS_CORE_PORT\")",
    ));
    for (id, path) in [
        (
            "installer-does-not-accept-core-url-overrides",
            "crates/goblins-os-installer/src/main.rs",
        ),
        (
            "login-does-not-accept-core-url-overrides",
            "crates/goblins-os-login/src/main.rs",
        ),
        (
            "shell-does-not-accept-core-url-overrides",
            "crates/goblins-os-shell/src/main.rs",
        ),
        (
            "settings-does-not-accept-core-url-overrides",
            "crates/goblins-os-settings/src/main.rs",
        ),
        (
            "launcher-does-not-accept-core-url-overrides",
            "crates/goblins-os-launcher/src/main.rs",
        ),
        (
            "control-center-does-not-accept-core-url-overrides",
            "crates/goblins-os-control-center/src/main.rs",
        ),
        (
            "open-helper-does-not-accept-core-url-overrides",
            "crates/goblins-os-open/src/main.rs",
        ),
        (
            "file-builder-does-not-accept-core-url-overrides",
            "crates/goblins-os-file-builder/src/main.rs",
        ),
        (
            "resident-does-not-accept-core-url-overrides",
            "crates/goblins-os-resident/src/main.rs",
        ),
    ] {
        checks.push(absent_check(root.join(path), id, "GOBLINS_OS_CORE_URL"));
    }
    checks.extend(core_capability_boundary_checks(root));
    checks.push(contains_check(
        root.join("crates/goblins-os-installer/src/main.rs"),
        "installer-proof-page-override-bypasses-completed-firstboot-exit",
        "should_exit_after_first_boot(first_boot_completed, installer_page_override_requested())",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-installer/src/main.rs"),
        "installer-proof-page-override-uses-goblins-page-env",
        "GOBLINS_OS_INSTALLER_PAGE",
    ));
    checks.push(contains_check(
        root.join("os/applications/org.goblins.OS.Shell.desktop"),
        "goblins-shell-hidden-under-systemd",
        "X-GNOME-HiddenUnderSystemd=true",
    ));
    checks.extend(desktop_exec_checks(root));
    checks.push(contains_check(
        root.join("os/applications/org.goblins.OS.Settings.desktop"),
        "settings-desktop-categories",
        "Categories=System;Settings;",
    ));
    checks.push(contains_check(
        root.join("os/applications/org.goblins.OS.Settings.desktop"),
        "settings-desktop-keywords",
        "Keywords=Settings;Preferences;System;",
    ));
    checks.push(contains_check(
        root.join("os/applications/org.goblins.OS.Settings.desktop"),
        "settings-desktop-startup-notify",
        "StartupNotify=true",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-settings/src/main.rs"),
        "settings-fetches-encryption-posture-status",
        "/v1/security/encryption",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-settings/src/main.rs"),
        "settings-renders-encryption-posture-row",
        "append_security_encryption_status",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-settings/src/main.rs"),
        "settings-keeps-encryption-enrollment-read-only",
        "Recovery-key minting and TPM enrollment remain installer and hardware-gated",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-settings/src/main.rs"),
        "settings-fetches-snapshots-status",
        "/v1/snapshots/status",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-settings/src/main.rs"),
        "settings-renders-storage-snapshots-row",
        "append_storage_snapshots_status",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-settings/src/main.rs"),
        "settings-renders-recovery-snapshots-row",
        "append_recovery_snapshots_status",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-settings/src/main.rs"),
        "settings-keeps-snapshot-restore-read-only",
        "Restore remains CI/qemu-gated",
    ));
    checks.extend(systemd_hardening_checks(root));
    checks.push(bootc_install_config_check(root));
    checks.extend(goblins_ai_contract_checks(root));
    checks.extend(native_design_system_checks(root));
    checks.extend(release_readiness_checks(root));
    checks.extend(secret_hygiene_checks(root));
    checks.extend(dual_arch_release_checks(root));
    checks.extend(installer_readiness_checks(root));
    checks.extend(gaming_readiness_checks(root));
    checks.push(forbidden_source_drift_check(root));

    // A clean install must be able to log into the graphical session: a human
    // login user, GDM autologin, and the Goblins OS session as the default.
    checks.push(file_check(root, "os/gdm/custom.conf"));
    checks.push(file_check(root, "os/accountsservice/goblin"));
    checks.push(container_contains_check(
        root,
        "human-login-user",
        "useradd --uid 1000 --create-home --home-dir /var/home/goblin",
    ));
    checks.push(container_contains_check(
        root,
        "human-login-user-invalid-nonlocked-password-hash",
        "usermod --password '*' goblin",
    ));
    checks.push(container_contains_check(
        root,
        "gdm-autologin-config",
        "COPY os/gdm/custom.conf /etc/gdm/custom.conf",
    ));
    checks.push(container_contains_check(
        root,
        "desktop-defaults-to-graphical-target",
        "systemctl set-default graphical.target",
    ));
    checks.push(container_contains_check(
        root,
        "image-default-target-pinned-in-usr",
        "ln -sfn graphical.target /usr/lib/systemd/system/default.target",
    ));
    checks.push(container_contains_check(
        root,
        "display-manager-alias-pinned-in-usr",
        "ln -sfn gdm.service /usr/lib/systemd/system/display-manager.service",
    ));
    checks.push(container_contains_check(
        root,
        "display-manager-alias-pinned-in-etc",
        "ln -sfn /usr/lib/systemd/system/gdm.service /etc/systemd/system/display-manager.service",
    ));
    checks.push(container_contains_check(
        root,
        "accountsservice-default-session",
        "COPY os/accountsservice/goblin /var/lib/AccountsService/users/goblin",
    ));
    checks.push(container_contains_check(
        root,
        "dconf-defaults-install",
        "COPY os/dconf/ /etc/dconf/",
    ));
    checks.push(container_contains_check(
        root,
        "gtk4-stock-theme-install",
        "COPY os/gtk-4.0/ /etc/gtk-4.0/",
    ));
    checks.push(container_contains_check(
        root,
        "nautilus-context-action-install",
        "COPY os/nautilus/ /usr/share/goblins-os/nautilus/",
    ));
    checks.push(container_contains_check(
        root,
        "nautilus-context-action-user-seed",
        "/var/home/goblin/.local/share/nautilus/scripts/Build an app to open this",
    ));
    checks.push(container_contains_check(
        root,
        "nautilus-ask-goblin-context-action-user-seed",
        "/var/home/goblin/.local/share/nautilus/scripts/Ask Goblin about this",
    ));
    checks.push(container_contains_check(
        root,
        "sound-theme-build-generation",
        "system-ready.wav",
    ));
    checks.push(container_contains_check(
        root,
        "sound-theme-install",
        "COPY os/sounds/GoblinsOS/index.theme /usr/share/sounds/GoblinsOS/index.theme",
    ));
    checks.push(container_contains_check(
        root,
        "dconf-update",
        "dconf update",
    ));
    checks.push(contains_check(
        root.join("os/gdm/custom.conf"),
        "gdm-autologin-switch-enabled",
        "AutomaticLoginEnable=True",
    ));
    checks.push(contains_check(
        root.join("os/gdm/custom.conf"),
        "gdm-autologin-user",
        "AutomaticLogin=goblin",
    ));
    checks.push(contains_check(
        root.join("os/gdm/custom.conf"),
        "gdm-timed-login-switch-enabled",
        "TimedLoginEnable=True",
    ));
    checks.push(contains_check(
        root.join("os/gdm/custom.conf"),
        "gdm-timed-login-user",
        "TimedLogin=goblin",
    ));
    checks.push(contains_check(
        root.join("os/gdm/custom.conf"),
        "gdm-timed-login-zero-delay",
        "TimedLoginDelay=0",
    ));
    checks.push(container_contains_check(
        root,
        "gdm-autologin-user-shadow-hash-is-invalid-not-locked",
        r#"test "$(getent shadow goblin | cut -d: -f2)" = "*""#,
    ));
    checks.push(contains_check(
        root.join("os/accountsservice/goblin"),
        "default-session-goblins-os",
        "Session=goblins-os",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/00-goblins-os-first-run"),
        "gnome-welcome-dialog-disabled",
        "welcome-dialog-last-shown-version='999999999'",
    ));
    checks.extend(first_boot_idle_policy_checks(
        root.join("os/dconf/db/local.d/00-goblins-os-first-run"),
        "",
    ));
    checks.push(container_absent_check(
        root,
        "login-gate-not-autostarted-before-firstboot",
        "COPY os/autostart/org.goblins.OS.Login.desktop /etc/xdg/autostart/org.goblins.OS.Login.desktop",
    ));
    checks.push(absent_check(
        root.join("os/session/goblins-os-session"),
        "session-does-not-force-gtk-simple-input-method",
        "GTK_IM_MODULE",
    ));
    checks.push(absent_check(
        root.join("os/session/goblins-os-session"),
        "session-does-not-force-qt-simple-input-method",
        "QT_IM_MODULE",
    ));
    checks.push(absent_check(
        root.join("os/session/goblins-os-session"),
        "session-does-not-force-xim-disabled",
        "XMODIFIERS",
    ));
    checks.push(session_contains_check(
        root,
        "session-keeps-ibus-sync-mode-conservative",
        "IBUS_ENABLE_SYNC_MODE",
    ));
    checks.push(session_contains_check(
        root,
        "session-imports-display-env-into-systemd-user",
        "systemctl --user import-environment",
    ));
    checks.push(session_contains_check(
        root,
        "session-updates-dbus-activation-display-env",
        "dbus-update-activation-environment --systemd",
    ));
    checks.push(session_contains_check(
        root,
        "session-imports-wayland-display-for-user-services",
        "WAYLAND_DISPLAY",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
        "super-space-launcher",
        "binding='<Super>space'",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
        "super-space-launcher-input-handoff",
        "command='/usr/libexec/goblins-os/goblins-os-launcher --super-space'",
    ));
    checks.push(contains_check(
        root.join("crates/goblins-os-launcher/src/main.rs"),
        "launcher-super-space-input-handoff",
        "/v1/input/switch-next",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
        "goblins-sound-theme",
        "theme-name='GoblinsOS'",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
        "mutter-snap-assist",
        "edge-tiling=true",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
        "goblins-wm-enabled-dconf",
        "goblins-wm@goblins.os",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-modes/goblins-os.json"),
        "goblins-wm-enabled-shell-mode",
        "\"goblins-wm@goblins.os\"",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
        "goblins-switch-enabled-dconf",
        "goblins-switch@goblins.os",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-modes/goblins-os.json"),
        "goblins-switch-enabled-shell-mode",
        "\"goblins-switch@goblins.os\"",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
        "goblins-switch-dconf-default-off",
        "[org/goblins/os/a11y/switch-control]",
    ));
    checks.push(container_contains_check(
        root,
        "goblins-wm-schema-compiled",
        "glib-compile-schemas /usr/share/gnome-shell/extensions/goblins-wm@goblins.os/schemas",
    ));
    checks.push(contains_check(
        root.join("os/glib-schemas/org.goblins.shell.extensions.wm.gschema.xml"),
        "goblins-wm-system-schema-for-core-shortcut-writes",
        "window-hud",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-real-window-clones",
        "new Clutter.Clone",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-mutter-keybindings",
        "Main.wm.addKeybinding",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-spaces-api",
        "append_new_workspace",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-space-strip-user-facing-label",
        "spaceStripLabel",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-space-strip-empty-copy",
        "Space ${index + 1} - Empty",
    ));
    checks.push(absent_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-space-strip-no-raw-zero-label",
        "Space ${index + 1}  ${counts.get(index) || 0}",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-snap-api",
        "move_resize_frame",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-render-hook",
        "globalThis.goblinsWindowManager",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-touch-events",
        "touch-event",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-touch-swipe-spaces",
        "TOUCH_SWIPE_MIN",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-dock@goblins.os/extension.js"),
        "goblins-dock-items-accessible-name",
        "accessible_name: `Open ${appName}`",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-dock@goblins.os/stylesheet.css"),
        "goblins-dock-focus-state",
        ".goblins-dock-item:focus",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-cards-accessible-name",
        "accessible_name: `Activate ${entry.title}`",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-symbol-controls-accessible-name",
        "accessible_name: 'Move to previous space'",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/schemas/org.goblins.shell.extensions.wm.gschema.xml"),
        "goblins-wm-corner-snap-shortcuts",
        "snap-top-left",
    ));
    checks.push(contains_check(
        root.join("os/gtk-4.0/gtk.css"),
        "gtk4-libadwaita-accent",
        "@define-color accent_bg_color",
    ));
    checks.push(contains_check(
        root.join("os/themes/GoblinsOS/index.theme"),
        "metatheme-icon-theme-goblins",
        "IconTheme=GoblinsOS",
    ));
    checks.push(contains_check(
        root.join("os/nautilus/scripts/Build an app to open this"),
        "nautilus-build-file-helper",
        "goblins-os-file-builder",
    ));
    checks.push(contains_check(
        root.join("os/nautilus/scripts/Ask Goblin about this"),
        "nautilus-ask-goblin-file-helper",
        "goblins-os-file-builder --ask",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-setup-appearance",
        "40-setup-appearance.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-launcher-dark",
        "38-launcher-dark.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-launcher-query-prefill-only",
        "GOBLINS_OS_RENDER_QUERY",
    ));
    checks.push(absent_check(
        root.join("os/bootc/render-screens.sh"),
        "render-no-legacy-shell-demo-env",
        "GOBLINS_OS_SHELL_DEMO",
    ));
    checks.push(absent_check(
        root.join("os/bootc/render-screens.sh"),
        "render-no-legacy-launcher-demo-env",
        "GOBLINS_OS_LAUNCHER_DEMO",
    ));
    checks.push(absent_check(
        root.join("crates/goblins-os-shell/src/main.rs"),
        "shell-no-legacy-demo-env",
        "GOBLINS_OS_SHELL_DEMO",
    ));
    checks.push(absent_check(
        root.join("crates/goblins-os-launcher/src/main.rs"),
        "launcher-no-legacy-demo-env",
        "GOBLINS_OS_LAUNCHER_DEMO",
    ));
    checks.push(absent_check(
        root.join("crates/goblins-os-launcher/src/main.rs"),
        "launcher-no-seeded-render-apps",
        "Render/design proof: seed",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-settings-scope-selector",
        "GOBLINS_OS_RENDER_SCOPE:-all",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-desktop-user-runtime-directory",
        "install -d -m 0700 -o goblin -g goblin \"$XDG_RUNTIME_DIR\"",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-desktop-clients-consume-setgid-capability-as-human-user",
        "setpriv --reuid=goblin --regid=goblin --init-groups --",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-desktop-clients-use-hardened-launcher",
        "run_desktop_app /usr/libexec/goblins-os/",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-chrome-focused-scope",
        "capture_chrome_surface",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-chrome-focused-control-center-dark-proof",
        "39-control-center-dark.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-chrome-focused-control-center-focus-light-proof",
        "37b-control-center-focus.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-chrome-focused-control-center-focus-dark-proof",
        "39b-control-center-focus-dark.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-settings-scope-function",
        "capture_settings_surface",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-settings-interaction-scope",
        "settings-interactions",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-settings-search-interaction",
        "capture_settings_search_interaction",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-screens.sh"),
        "render-settings-firewall-toggle-interaction",
        "capture_settings_firewall_toggle_interaction",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render.suffix.Dockerfile"),
        "render-settings-scope-build-arg",
        "ARG GOBLINS_OS_RENDER_SCOPE=all",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render.suffix.Dockerfile"),
        "render-settings-scope-build-env",
        "GOBLINS_OS_RENDER_SCOPE=\"$GOBLINS_OS_RENDER_SCOPE\"",
    ));
    checks.push(contains_check(
        root.join("os/bootc/selftest.suffix.Dockerfile"),
        "selftest-suffix-copy-chmod-layer",
        "COPY --chmod=0755 os/bootc/run-selftest.sh",
    ));
    checks.push(absent_check(
        root.join("os/bootc/selftest.suffix.Dockerfile"),
        "selftest-suffix-no-extra-chmod-run-layer",
        "RUN chmod +x /usr/local/bin/run-selftest.sh",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render.suffix.Dockerfile"),
        "render-suffix-copy-chmod-layer",
        "COPY --chmod=0755 os/bootc/render-screens.sh",
    ));
    checks.push(absent_check(
        root.join("os/bootc/render.suffix.Dockerfile"),
        "render-suffix-no-extra-chmod-run-layer",
        "RUN chmod +x /usr/local/bin/render-screens.sh",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.suffix.Dockerfile"),
        "desktop-render-suffix-copy-chmod-layer",
        "COPY --chmod=0755 os/bootc/render-desktop.sh",
    ));
    checks.push(absent_check(
        root.join("os/bootc/render-desktop.suffix.Dockerfile"),
        "desktop-render-suffix-no-extra-chmod-run-layer",
        "RUN chmod +x /usr/local/bin/render-desktop.sh",
    ));
    checks.push(file_check(root, "os/bootc/verify.suffix.Dockerfile"));
    checks.push(reviewed_github_action_pins_check(root));
    checks.push(deprecated_github_action_pins_absent_check(root));
    checks.push(contains_check(
        root.join("os/hardware-gate/verify-shipping-status.sh"),
        "shipping-status-runs-structural-workflow-action-pin-check",
        "--workflow-action-pins \"$ROOT\" --quiet",
    ));
    checks.push(contains_check(
        root.join("os/bootc/verify.suffix.Dockerfile"),
        "image-verify-suffix-target",
        "FROM goblins-os AS verify",
    ));
    checks.push(contains_check(
        root.join("os/bootc/verify.suffix.Dockerfile"),
        "image-verify-suffix-blocked-zero",
        "grep -q 'blocked=0'",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-cacheonly-output",
        "outputs: type=cacheonly",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-uses-buildx-builder",
        "docker/setup-buildx-action@bb05f3f5519dd87d3ba754cc423b652a5edd6d2c",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-buildkit-gha-cache-scope",
        "type=gha,scope=goblins-os-bootc-${{ matrix.arch }}",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-buildkit-gha-cache-nonblocking",
        "mode=max,ignore-error=true",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-renders-settings-interactions-scope",
        "GOBLINS_OS_RENDER_SCOPE=settings-interactions",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-uploads-settings-interactions-artifact",
        "goblins-os-settings-interactions-${{ matrix.arch }}",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-renders-polish-interactions-scope",
        "GOBLINS_OS_RENDER_SCOPE=polish-interactions",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-uploads-polish-interactions-artifact",
        "goblins-os-polish-interactions-${{ matrix.arch }}",
    ));
    checks.push(absent_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-no-daemon-export-tag",
        "-t goblins-os:${{ matrix.arch }}",
    ));
    checks.push(absent_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-no-daemon-run",
        "docker run --rm goblins-os:${{ matrix.arch }}",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-explicit-push-marker",
        "contains(github.event.head_commit.message, '[image]')",
    ));
    checks.push(contains_check(
        root.join(".github/workflows/build.yml"),
        "image-workflow-push-marker-guard",
        "github.event_name == 'push' && contains(github.event.head_commit.message, '[image]')",
    ));
    checks.extend(settings_render_screenshot_checks(root));
    checks.extend(settings_interaction_screenshot_checks(root));
    checks.extend(polish_interaction_screenshot_checks(root));
    checks.push(file_check(root, "os/bootc/render-desktop.sh"));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-composited-desktop-shell",
        "51-desktop-shell-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-shell-eval-interface",
        "org.gnome.Shell.Eval",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-shell-eval-hard-fail",
        "|| return 1",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-propagates-overlay-failure",
        "exit \"$RENDER_FAILED\"",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-core-state-is-not-readable-by-desktop-user",
        "runuser -u goblin -- test -r \"$GOBLINS_OS_INSTALLER_STATE/first-boot.json\"",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-consumes-capability-sockets-only",
        "Native clients consume only their fixed capability sockets",
    ));
    checks.push(absent_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-user-never-rewrites-core-first-boot-state",
        "if [ ! -f \"$GOBLINS_OS_INSTALLER_STATE/first-boot.json\" ]",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-rejects-failed-screenshot-response",
        "screenshot D-Bus call returned failure",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-rejects-empty-screenshots",
        "if [ ! -s \"$OUT/$name\" ]",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-decodes-and-sizes-pngs",
        "magick identify -format '%m %wx%h'",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-requires-complete-28-frame-proof",
        "if [ \"$VALID_PNGS\" -ne 28 ]",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.suffix.Dockerfile"),
        "render-desktop-installs-png-decoder",
        "ImageMagick",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-workspace-overview",
        "52-wm-workspace-overview-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-focused-app-windows",
        "52b-wm-focused-app-windows-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-hot-corner",
        "52c-wm-hot-corner-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-workspaces",
        "53-wm-workspaces-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-switcher",
        "54-wm-switcher-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-snap-assist",
        "55-wm-snap-assist-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-snap-assist-hook",
        "showSnapAssistDemo",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-snap-assist-requires-two-real-windows",
        "renderWindowCount() >= 2",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "render-wm-snap-assist-requires-mapped-window-actors",
        "entry.actor.is_mapped()",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-hud",
        "56-wm-hud-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-switch-control-point",
        "57-switch-control-point-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-switch-control-hook",
        "showPointScanDemo",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-menubar-input-source-chip",
        "59-menubar-input-source-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-menubar-input-source-seeds-two-sources",
        "[('xkb', 'us'), ('xkb', 'gb')]",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-menubar-input-source-seeds-current-index",
        "gsettings set org.gnome.desktop.input-sources current 1",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-menubar-focus-chip",
        "59b-menubar-focus-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-menubar-focus-seeds-mode",
        r#"[{"id":"work","name":"Deep Work"}]"#,
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-menubar-focus-seeds-active-mode",
        "gsettings set org.goblins.os.focus active-mode work",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.suffix.Dockerfile"),
        "desktop-render-target",
        "desktop-screenshots",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.suffix.Dockerfile"),
        "desktop-render-docker-example",
        "DOCKER_BUILDKIT=1 docker build",
    ));
    checks.push(absent_check(
        root.join("os/bootc/render-desktop.suffix.Dockerfile"),
        "desktop-render-no-podman-example",
        "podman build",
    ));
    checks.push(contains_check(
        root.join("os/themes/GoblinsOS/gnome-shell/gnome-shell.css"),
        "overview-search-themed",
        ".overview .search-entry",
    ));
    checks.push(contains_check(
        root.join("os/themes/GoblinsOS/gnome-shell/gnome-shell.css"),
        "snap-assist-themed",
        ".tile-preview",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"),
        "goblins-wm-overlay-themed",
        ".goblins-wm-overlay.light",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"),
        "goblins-wm-snap-preview-themed",
        ".goblins-wm-snap-preview",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"),
        "goblins-wm-touch-targets",
        "min-height: 44px",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"),
        "goblins-wm-window-card-focus-state",
        ".goblins-wm-window-card:focus",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"),
        "goblins-wm-hud-button-focus-state",
        ".goblins-wm-hud-button:focus",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"),
        "goblins-wm-light-hint-contrast",
        "-st-hint-text-color",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-motion-token-overlay",
        "const OVERLAY_FADE_MS = 180;",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-motion-token-snap-preview",
        "const SNAP_PREVIEW_FADE_MS = 140;",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-reduced-motion-native-setting",
        "get_boolean('enable-animations')",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-reduced-motion-no-forced-fade",
        "actor.set_opacity(255);",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-overlay-uses-motion-token",
        "_fadeIn(this._overlay, OVERLAY_FADE_MS);",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
        "goblins-wm-snap-uses-motion-token",
        "_fadeIn(this._snapPreview, SNAP_PREVIEW_FADE_MS);",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-uses-system-schema",
        "const SCHEMA_ID = 'org.goblins.os.a11y.switch-control';",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-atspi-discovery",
        "import('gi://Atspi')",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-point-fallback",
        "This window has no scannable controls - using point scan.",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-escape-disable",
        "Clutter.KEY_Escape",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-qemu-input-honesty",
        "Secure pointer control is not available yet.",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-point-status-stays-concise",
        "Selection is paused until secure pointer control is available.",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-render-hook",
        "showPointScanDemo",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-disabled-actor-invariant",
        "renderProofInactive",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-disabled-focus-invariant",
        "global.stage.set_key_focus(null);",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-render-proof-rejects-retained-focus",
        "!this._panel?.has_key_focus()",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-switch-control-disabled-baseline",
        "assert_switch_control_inactive",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-bounds-shell-dbus-calls",
        "gdbus call --timeout 5",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-cleans-every-scheme-exit",
        "run_render_scheme()",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-scheme-wrapper-calls-cleanup",
        "cleanup_scheme",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-bounds-shell-teardown",
        "kill -KILL \"$SHELL_PID\"",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-fails-on-capture-error",
        "shoot \"50-desktop-$suffix.png\" || return 1",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-desktop-fails-on-artifact-mode-error",
        "could not make screenshot exportable",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/stylesheet.css"),
        "goblins-switch-inter",
        "font-family: \"Inter\"",
    ));

    // The session must OWN GNOME via a custom shell mode (no stock chrome),
    // not present stock GNOME.
    checks.push(file_check(root, "os/gnome-shell-modes/goblins-os.json"));
    checks.push(container_contains_check(
        root,
        "gnome-shell-mode-install",
        "COPY os/gnome-shell-modes/goblins-os.json /usr/share/gnome-shell/modes/goblins-os.json",
    ));
    checks.push(session_contains_check(
        root,
        "session-owns-gnome-shell-mode",
        "GNOME_SHELL_SESSION_MODE",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-modes/goblins-os.json"),
        "shell-mode-no-stock-overview",
        "\"hasOverview\": false",
    ));
    checks
}

fn installed_checks(root: &Path) -> Vec<Check> {
    let mut checks = Vec::new();
    for binary in BINARIES {
        checks.push(file_check(
            root,
            &format!("usr/libexec/goblins-os/{binary}"),
        ));
    }
    checks.push(path_mode_check(
        root,
        "usr/libexec/goblins-os/ui",
        0o755,
        "installed-desktop-payload-directory-mode",
    ));
    checks.push(path_owner_check(
        root,
        "usr/libexec/goblins-os/ui",
        0,
        0,
        "installed-desktop-payload-directory-root-owner",
    ));
    for binary in DESKTOP_CAPABILITY_BINARIES {
        let entrypoint = format!("usr/libexec/goblins-os/{binary}");
        let payload = format!("usr/libexec/goblins-os/ui/{binary}");
        let client = binary
            .strip_prefix("goblins-os-")
            .expect("desktop capability binary prefix");
        checks.push(path_mode_check(
            root,
            &entrypoint,
            0o2755,
            &format!("installed-{client}-entrypoint-setgid-mode"),
        ));
        checks.push(installed_named_owner_check(
            root,
            &entrypoint,
            "root",
            &format!("goblins-core-{client}"),
            &format!("installed-{client}-entrypoint-capability-owner"),
        ));
        checks.push(file_check(root, &payload));
        checks.push(path_mode_check(
            root,
            &payload,
            0o755,
            &format!("installed-{client}-payload-regular-mode"),
        ));
        checks.push(path_owner_check(
            root,
            &payload,
            0,
            0,
            &format!("installed-{client}-payload-root-owner"),
        ));
    }
    for unit in SYSTEMD_UNITS {
        checks.push(file_check(root, &format!("usr/lib/systemd/system/{unit}")));
    }
    for dropin in SYSTEMD_SYSTEM_DROPINS {
        checks.push(file_check(
            root,
            &format!("usr/lib/systemd/system/{dropin}"),
        ));
    }
    for unit in SYSTEMD_USER_UNITS {
        checks.push(file_check(root, &format!("usr/lib/systemd/user/{unit}")));
    }
    for app in APPLICATIONS {
        checks.push(file_check(root, &format!("usr/share/applications/{app}")));
    }
    for app in AUTOSTART {
        checks.push(file_check(root, &format!("etc/xdg/autostart/{app}")));
    }
    for extension_file in GNOME_SHELL_EXTENSION_FILES {
        checks.push(file_check(
            root,
            &format!("usr/share/gnome-shell/extensions/{extension_file}"),
        ));
    }
    checks.push(file_check(
        root,
        "usr/share/wayland-sessions/goblins-os.desktop",
    ));
    checks.push(file_check(
        root,
        "usr/share/gnome-session/sessions/goblins-os.session",
    ));
    checks.push(contains_check(
        root.join("usr/share/gnome-session/sessions/goblins-os.session"),
        "installed-gnome-session-settings-daemon-components",
        "org.gnome.SettingsDaemon.Power",
    ));
    checks.push(contains_check(
        root.join(
            "usr/lib/systemd/system/systemd-remount-fs.service.d/10-goblins-os-composefs.conf",
        ),
        "installed-ostree-composefs-remount-skip",
        "ConditionKernelCommandLine=!ostree",
    ));
    checks.push(absent_check(
        root.join("usr/share/gnome-session/sessions/goblins-os.session"),
        "installed-gnome-session-shell-not-required-component",
        "org.goblins.OS.Shell",
    ));
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "installed-gnome-session-systemd-gnome-shell-service",
        "Requires=org.gnome.Shell@user.service",
    ));
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "installed-gnome-session-systemd-shell-service-wanted",
        "Wants=org.goblins.OS.Shell.service",
    ));
    checks.push(absent_check(
        root.join("usr/lib/systemd/user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "installed-gnome-session-systemd-shell-target-not-required",
        "Requires=org.goblins.OS.Shell.target",
    ));
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/org.goblins.OS.Shell.service"),
        "installed-goblins-shell-user-service-exec",
        "ExecStart=/usr/libexec/goblins-os/goblins-os-shell",
    ));
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/org.goblins.OS.Shell.service"),
        "installed-goblins-shell-user-service-session-partof",
        "PartOf=gnome-session-initialized.target",
    ));
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/org.goblins.OS.SessionBridge.service"),
        "installed-session-bridge-user-service-exec",
        "ExecStart=/usr/libexec/goblins-os/goblins-os-session-bridge",
    ));
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "installed-session-bridge-wanted-by-gnome-session",
        "Wants=org.goblins.OS.SessionBridge.service",
    ));
    checks.push(file_check(root, "usr/lib/bootc/install/00-goblins-os.toml"));
    checks.push(file_check(root, "etc/goblins-os/environment"));
    checks.push(file_check(root, "etc/goblins-os/openai-secrets.env"));
    checks.push(contains_check(
        root.join("usr/lib/systemd/system/goblins-os-core.service"),
        "installed-core-service-loads-openai-systemd-credential",
        "LoadCredential=openai-secrets.env:/etc/goblins-os/openai-secrets.env",
    ));
    checks.push(absent_check(
        root.join("usr/lib/systemd/system/goblins-os-core.service"),
        "installed-core-service-does-not-export-openai-secret-environment",
        "EnvironmentFile=-/etc/goblins-os/openai-secrets.env",
    ));
    checks.push(core_service_writable_paths_check(
        root.join("usr/lib/systemd/system/goblins-os-core.service"),
        "installed-core-service-writable-paths-are-exact",
    ));
    checks.push(path_mode_check(
        root,
        "etc/goblins-os/openai-secrets.env",
        0o600,
        "installed-openai-secret-file-mode-0600",
    ));
    checks.push(path_owner_check(
        root,
        "etc/goblins-os/openai-secrets.env",
        0,
        0,
        "installed-openai-secret-file-owner-root",
    ));
    checks.push(file_has_no_active_secrets_check(
        root,
        "etc/goblins-os/openai-secrets.env",
        "installed-openai-secret-file-empty",
    ));
    checks.push(file_check(root, "usr/bin/bootc"));
    checks.push(file_check(root, "usr/bin/flatpak"));
    checks.push(file_check(root, "usr/bin/vulkaninfo"));
    checks.push(file_check(root, "usr/bin/vkcube"));
    checks.push(file_check(root, "usr/bin/gamescope"));
    checks.push(file_check(root, "usr/bin/gamemoderun"));
    checks.push(file_check(root, "usr/bin/mangohud"));
    checks.push(file_check(root, "usr/bin/vainfo"));
    checks.push(file_check(root, "usr/bin/vdpauinfo"));
    checks.push(file_check(root, "usr/bin/papers"));
    checks.push(file_check(root, "usr/bin/loupe"));
    checks.push(file_check(root, "usr/bin/authselect"));
    checks.push(file_check(root, "usr/sbin/fprintd-list"));
    checks.push(file_check(root, "usr/sbin/fprintd-enroll"));
    checks.push(file_check(root, "usr/sbin/fprintd-delete"));
    checks.push(file_check(root, "usr/sbin/fprintd-verify"));
    checks.push(file_check(root, "usr/lib64/security/pam_fprintd.so"));
    checks.push(file_check(
        root,
        "usr/share/dbus-1/system-services/net.reactivated.Fprint.service",
    ));
    checks.push(file_check(
        root,
        "usr/share/applications/org.gnome.Papers.desktop",
    ));
    checks.push(file_check(
        root,
        "usr/share/applications/org.gnome.Loupe.desktop",
    ));
    checks.push(file_check(root, "usr/share/applications/mimeapps.list"));
    checks.push(contains_check(
        root.join("usr/share/applications/mimeapps.list"),
        "installed-preview-pdf-defaults-to-papers",
        "application/pdf=org.gnome.Papers.desktop",
    ));
    checks.push(contains_check(
        root.join("usr/share/applications/mimeapps.list"),
        "installed-preview-png-defaults-to-loupe",
        "image/png=org.gnome.Loupe.desktop",
    ));
    checks.push(contains_check(
        root.join("usr/share/applications/mimeapps.list"),
        "installed-preview-jpeg-defaults-to-loupe",
        "image/jpeg=org.gnome.Loupe.desktop",
    ));
    checks.push(file_check(root, "usr/bin/pw-cli"));
    checks.push(file_check(root, "usr/bin/pw-play"));
    checks.push(file_check(root, "usr/bin/pw-record"));
    checks.push(file_check(root, "usr/bin/pw-dump"));
    checks.push(file_check(root, "usr/bin/wpctl"));
    checks.push(file_check(root, "usr/bin/evtest"));
    checks.push(file_check(root, "usr/bin/lsusb"));
    checks.push(path_absent_check(
        root,
        "usr/bin/steam",
        "installed-steam-binary-absent",
    ));
    checks.push(path_absent_check(
        root,
        "usr/share/applications/steam.desktop",
        "installed-steam-desktop-entry-absent",
    ));
    checks.push(path_absent_check(
        root,
        "usr/lib/udev/rules.d/60-steam-input.rules",
        "installed-steam-devices-rules-absent",
    ));
    checks.push(file_check(
        root,
        "etc/dconf/db/local.d/00-goblins-os-first-run",
    ));
    checks.push(file_check(
        root,
        "etc/dconf/db/local.d/10-goblins-os-desktop",
    ));
    checks.push(file_check(root, "etc/dconf/db/local.d/30-captions"));
    checks.push(file_check(
        root,
        "etc/dconf/db/local.d/40-sound-recognition",
    ));
    for schema in GLIB_SCHEMA_FILES {
        checks.push(file_check(
            root,
            &format!("usr/share/glib-2.0/schemas/{schema}"),
        ));
    }
    checks.push(file_check(root, "etc/gtk-4.0/gtk.css"));
    checks.push(file_check(root, "usr/share/sounds/GoblinsOS/index.theme"));
    for script in NAUTILUS_SCRIPTS {
        checks.push(file_check(
            root,
            &format!("usr/share/goblins-os/nautilus/{script}"),
        ));
        checks.push(file_check(
            root,
            &format!("var/home/goblin/.local/share/nautilus/{script}"),
        ));
    }
    for icon in ICON_THEME_FILES {
        checks.push(file_check(
            root,
            &format!("usr/share/icons/GoblinsOS/{icon}"),
        ));
    }
    checks.push(contains_check(
        root.join("etc/dconf/db/local.d/00-goblins-os-first-run"),
        "installed-gnome-welcome-dialog-disabled",
        "welcome-dialog-last-shown-version='999999999'",
    ));
    checks.extend(first_boot_idle_policy_checks(
        root.join("etc/dconf/db/local.d/00-goblins-os-first-run"),
        "installed-",
    ));
    checks.push(installed_session_check(root));
    checks.push(installed_secret_dir_check(root));
    checks.push(secret_dir_not_session_readable_check(
        root,
        "var/lib/goblins-os/secrets/openai",
        "installed-secret-storage-owner",
    ));
    let state_root = "var/lib/goblins-os";
    checks.push(path_mode_check(
        root,
        state_root,
        0o710,
        "installed-state-root-traverse-mode",
    ));
    checks.push(installed_named_owner_check(
        root,
        state_root,
        "goblins-os",
        "goblins-core-resident",
        "installed-state-root-owner",
    ));
    for relative in [
        "var/lib/goblins-os/models",
        "var/lib/goblins-os/apps",
        "var/lib/goblins-os/installer",
        "var/lib/goblins-os/session",
        "var/lib/goblins-os/policy",
    ] {
        checks.push(installed_state_dir_check(root, relative));
        checks.push(installed_state_dir_owner_check(
            root,
            relative,
            "goblins-os",
            &format!("{relative}-owner"),
        ));
    }
    let resident_state = "var/lib/goblins-os/resident";
    checks.push(installed_state_dir_check(root, resident_state));
    checks.push(path_mode_check(
        root,
        resident_state,
        0o750,
        "installed-resident-state-mode",
    ));
    checks.push(installed_state_dir_owner_check(
        root,
        resident_state,
        "goblins-resident",
        &format!("{resident_state}-owner"),
    ));
    checks.push(file_check(root, "etc/gdm/custom.conf"));
    checks.push(contains_check(
        root.join("etc/gdm/custom.conf"),
        "installed-gdm-autologin",
        "AutomaticLogin=goblin",
    ));
    checks.push(file_check(root, "etc/systemd/system/default.target"));
    checks.push(symlink_target_check(
        root,
        "etc/systemd/system/default.target",
        "installed-graphical-default-target",
        "graphical.target",
    ));
    checks.push(file_check(root, "usr/lib/systemd/system/default.target"));
    checks.push(symlink_target_check(
        root,
        "usr/lib/systemd/system/default.target",
        "installed-image-graphical-default-target",
        "graphical.target",
    ));
    checks.push(file_check(
        root,
        "etc/systemd/system/display-manager.service",
    ));
    checks.push(symlink_target_check(
        root,
        "etc/systemd/system/display-manager.service",
        "installed-etc-display-manager-is-gdm",
        "gdm.service",
    ));
    checks.push(file_check(
        root,
        "usr/lib/systemd/system/display-manager.service",
    ));
    checks.push(symlink_target_check(
        root,
        "usr/lib/systemd/system/display-manager.service",
        "installed-image-display-manager-is-gdm",
        "gdm.service",
    ));
    checks.push(file_check(root, "var/lib/AccountsService/users/goblin"));
    checks.push(contains_check(
        root.join("var/lib/AccountsService/users/goblin"),
        "installed-default-session",
        "Session=goblins-os",
    ));
    checks.push(file_check(
        root,
        "usr/share/gnome-shell/modes/goblins-os.json",
    ));
    checks.push(contains_check(
        root.join("usr/share/gnome-shell/modes/goblins-os.json"),
        "installed-shell-mode",
        "\"parentMode\"",
    ));
    checks.push(contains_check(
        root.join("etc/dconf/db/local.d/10-goblins-os-desktop"),
        "installed-goblins-wm-enabled-dconf",
        "goblins-wm@goblins.os",
    ));
    checks.push(contains_check(
        root.join("usr/share/gnome-shell/modes/goblins-os.json"),
        "installed-goblins-wm-enabled-shell-mode",
        "\"goblins-wm@goblins.os\"",
    ));
    checks.push(contains_check(
        root.join("etc/dconf/db/local.d/10-goblins-os-desktop"),
        "installed-goblins-switch-enabled-dconf",
        "goblins-switch@goblins.os",
    ));
    checks.push(contains_check(
        root.join("usr/share/gnome-shell/modes/goblins-os.json"),
        "installed-goblins-switch-enabled-shell-mode",
        "\"goblins-switch@goblins.os\"",
    ));
    checks
}

const STATE_DIRS: &[&str] = &[
    "var/lib/goblins-os/resident",
    "var/lib/goblins-os/installer",
    "var/lib/goblins-os/session",
    "var/lib/goblins-os/policy",
    "var/lib/goblins-os/models",
    "var/lib/goblins-os/apps",
];

/// The source-to-installed file map, mirroring the bootc Containerfile so the
/// staged tree matches what the image build produces. Returns
/// (absolute source path, installed-relative destination) pairs.
fn install_files(source: &Path, binaries: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();

    for binary in BINARIES {
        files.push((
            binaries.join(binary),
            format!("usr/libexec/goblins-os/{binary}"),
        ));
    }
    files.push((
        source.join("os/session/goblins-os-session"),
        "usr/libexec/goblins-os/goblins-os-session".to_string(),
    ));
    for unit in SYSTEMD_UNITS {
        files.push((
            source.join(format!("os/systemd/{unit}")),
            format!("usr/lib/systemd/system/{unit}"),
        ));
    }
    for dropin in SYSTEMD_SYSTEM_DROPINS {
        files.push((
            source.join(format!("os/systemd-system/{dropin}")),
            format!("usr/lib/systemd/system/{dropin}"),
        ));
    }
    for unit in SYSTEMD_USER_UNITS {
        files.push((
            source.join(format!("os/systemd-user/{unit}")),
            format!("usr/lib/systemd/user/{unit}"),
        ));
    }
    files.push((
        source.join("os/session/goblins-os.desktop"),
        "usr/share/wayland-sessions/goblins-os.desktop".to_string(),
    ));
    files.push((
        source.join("os/gnome-session/goblins-os.session"),
        "usr/share/gnome-session/sessions/goblins-os.session".to_string(),
    ));
    for app in APPLICATIONS {
        files.push((
            source.join(format!("os/applications/{app}")),
            format!("usr/share/applications/{app}"),
        ));
    }
    for app in AUTOSTART {
        files.push((
            source.join(format!("os/autostart/{app}")),
            format!("etc/xdg/autostart/{app}"),
        ));
    }
    files.push((
        source.join("os/bootc-install/00-goblins-os.toml"),
        "usr/lib/bootc/install/00-goblins-os.toml".to_string(),
    ));
    files.push((
        source.join("os/etc/goblins-os/environment"),
        "etc/goblins-os/environment".to_string(),
    ));
    files.push((
        source.join("os/etc/goblins-os/openai-secrets.env"),
        "etc/goblins-os/openai-secrets.env".to_string(),
    ));
    for dconf in DCONF_FILES {
        files.push((
            source.join(format!("os/dconf/db/local.d/{dconf}")),
            format!("etc/dconf/db/local.d/{dconf}"),
        ));
    }
    for schema in GLIB_SCHEMA_FILES {
        files.push((
            source.join(format!("os/glib-schemas/{schema}")),
            format!("usr/share/glib-2.0/schemas/{schema}"),
        ));
    }
    for extension_file in GNOME_SHELL_EXTENSION_FILES {
        files.push((
            source.join(format!("os/gnome-shell-extensions/{extension_file}")),
            format!("usr/share/gnome-shell/extensions/{extension_file}"),
        ));
    }
    files.push((
        source.join("os/gdm/custom.conf"),
        "etc/gdm/custom.conf".to_string(),
    ));
    files.push((
        source.join("os/accountsservice/goblin"),
        "var/lib/AccountsService/users/goblin".to_string(),
    ));
    files.push((
        source.join("os/gnome-shell-modes/goblins-os.json"),
        "usr/share/gnome-shell/modes/goblins-os.json".to_string(),
    ));
    files.push((
        source.join("os/gtk-4.0/gtk.css"),
        "etc/gtk-4.0/gtk.css".to_string(),
    ));
    files.push((
        source.join("os/sounds/GoblinsOS/index.theme"),
        "usr/share/sounds/GoblinsOS/index.theme".to_string(),
    ));
    for script in NAUTILUS_SCRIPTS {
        files.push((
            source.join(format!("os/nautilus/{script}")),
            format!("usr/share/goblins-os/nautilus/{script}"),
        ));
        files.push((
            source.join(format!("os/nautilus/{script}")),
            format!("var/home/goblin/.local/share/nautilus/{script}"),
        ));
    }
    for icon in ICON_THEME_FILES {
        files.push((
            source.join(format!("os/icons/GoblinsOS/{icon}")),
            format!("usr/share/icons/GoblinsOS/{icon}"),
        ));
    }

    files
}

/// Materialize the full installed OS tree into `destdir`, the same layout the
/// bootc image produces, so the installed-mode contract can validate it.
fn stage_install(source: &Path, binaries: &Path, destdir: &Path) -> std::io::Result<()> {
    for (src, relative) in install_files(source, binaries) {
        let dst = destdir.join(&relative);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&src, &dst).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("copy {} -> {relative}: {error}", src.display()),
            )
        })?;
    }

    #[cfg(unix)]
    {
        for binary in BINARIES {
            set_mode(
                &destdir.join(format!("usr/libexec/goblins-os/{binary}")),
                0o755,
            )?;
        }
        set_mode(
            &destdir.join("usr/libexec/goblins-os/goblins-os-session"),
            0o755,
        )?;
        for script in NAUTILUS_SCRIPTS {
            set_mode(
                &destdir.join(format!("var/home/goblin/.local/share/nautilus/{script}")),
                0o755,
            )?;
        }
        set_mode(&destdir.join("etc/goblins-os/openai-secrets.env"), 0o600)?;
    }

    for dir in STATE_DIRS {
        fs::create_dir_all(destdir.join(dir))?;
    }
    let secrets = destdir.join("var/lib/goblins-os/secrets/openai");
    fs::create_dir_all(&secrets)?;
    #[cfg(unix)]
    set_mode(&secrets, 0o700)?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoLockPackage {
    name: String,
    version: String,
    source: String,
    checksum: String,
}

fn candidate_commit_is_valid(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn image_ref_is_valid(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_whitespace)
}

fn image_ref_is_digest_pinned(value: &str) -> bool {
    let Some((name, digest)) = value.rsplit_once("@sha256:") else {
        return false;
    };
    image_ref_is_valid(value)
        && !name.is_empty()
        && !name.contains('@')
        && digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_release_evidence(
    source: &Path,
    arch: &str,
    candidate_commit: &str,
    image_ref: &str,
    output: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    if !SUPPORTED_RELEASE_ARCHES.contains(&arch) {
        return Err(format!("unsupported architecture {arch}").into());
    }
    if !candidate_commit_is_valid(candidate_commit) {
        return Err("candidate commit must be exactly 40 hexadecimal characters".into());
    }
    if !image_ref_is_valid(image_ref) {
        return Err("image ref must be nonempty and contain no whitespace".into());
    }

    fs::create_dir_all(output)?;
    for generated_name in [
        "release-evidence-manifest.json",
        "cargo-lock-packages.tsv",
        "rpm-packages.command",
        "rpm-packages.tsv",
        "rpm-packages.not-generated.txt",
    ] {
        let generated_path = output.join(generated_name);
        if generated_path.exists() || generated_path.is_symlink() {
            fs::remove_file(generated_path)?;
        }
    }
    let cargo_lock = fs::read_to_string(source.join("Cargo.lock"))?;
    let mut packages = cargo_lock_packages(&cargo_lock);
    if packages.is_empty() {
        return Err("Cargo.lock did not contain any package entries".into());
    }
    packages.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.version.cmp(&right.version))
            .then_with(|| left.source.cmp(&right.source))
    });

    let cargo_packages_path = output.join("cargo-lock-packages.tsv");
    fs::write(&cargo_packages_path, cargo_lock_packages_tsv(&packages))?;
    let cargo_packages_sha256 = sha256_path(&cargo_packages_path)?;
    fs::write(output.join("rpm-packages.command"), rpm_packages_command())?;
    let rpm_status = write_rpm_packages_if_available(output)?;
    let rpm_packages_path = output.join("rpm-packages.tsv");
    let rpm_packages_sha256 = if rpm_packages_path.is_file() {
        Some(sha256_path(&rpm_packages_path)?)
    } else {
        None
    };
    let manifest = output.join("release-evidence-manifest.json");
    fs::write(
        &manifest,
        release_evidence_manifest(
            arch,
            &candidate_commit.to_ascii_lowercase(),
            image_ref,
            packages.len(),
            &cargo_packages_sha256,
            rpm_packages_sha256.as_deref(),
            &rpm_status,
        ),
    )?;
    Ok(manifest)
}

fn cargo_lock_packages(lock: &str) -> Vec<CargoLockPackage> {
    let mut packages = Vec::new();
    let mut current: Option<CargoLockPackage> = None;

    for line in lock.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if let Some(package) = current.take() {
                if !package.name.is_empty() && !package.version.is_empty() {
                    packages.push(package);
                }
            }
            current = Some(CargoLockPackage {
                name: String::new(),
                version: String::new(),
                source: String::new(),
                checksum: String::new(),
            });
            continue;
        }

        let Some(package) = current.as_mut() else {
            continue;
        };
        if let Some(value) = lock_string_value(trimmed, "name") {
            package.name = value;
        } else if let Some(value) = lock_string_value(trimmed, "version") {
            package.version = value;
        } else if let Some(value) = lock_string_value(trimmed, "source") {
            package.source = value;
        } else if let Some(value) = lock_string_value(trimmed, "checksum") {
            package.checksum = value;
        }
    }

    if let Some(package) = current {
        if !package.name.is_empty() && !package.version.is_empty() {
            packages.push(package);
        }
    }

    packages
}

fn lock_string_value(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"");
    line.strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix('"'))
        .map(ToOwned::to_owned)
}

fn cargo_lock_packages_tsv(packages: &[CargoLockPackage]) -> String {
    let mut lines = vec!["name\tversion\tsource\tchecksum".to_string()];
    for package in packages {
        lines.push(format!(
            "{}\t{}\t{}\t{}",
            tsv_field(&package.name),
            tsv_field(&package.version),
            tsv_field(&package.source),
            tsv_field(&package.checksum)
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn tsv_field(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
}

fn sha256_path(path: &Path) -> Result<String, Box<dyn Error>> {
    let digest = Sha256::digest(fs::read(path)?);
    Ok(format!("{digest:x}"))
}

fn rpm_packages_command() -> &'static str {
    "#!/usr/bin/env sh\nset -eu\ntmp=\"${TMPDIR:-/tmp}/goblins-os-rpm-packages.$$\"\ntrap 'rm -f \"$tmp\"' EXIT\nrpm -qa --qf '%{NAME}\\t%{VERSION}-%{RELEASE}\\t%{ARCH}\\t%{LICENSE}\\n' | LC_ALL=C sort > \"$tmp\"\n{\n  printf 'name\\tversion_release\\tarch\\tlicense\\n'\n  cat \"$tmp\"\n} > rpm-packages.tsv\nrm -f rpm-packages.not-generated.txt\n"
}

fn write_rpm_packages_if_available(output: &Path) -> Result<String, Box<dyn Error>> {
    let query = "%{NAME}\t%{VERSION}-%{RELEASE}\t%{ARCH}\t%{LICENSE}\n";
    match Command::new("rpm").args(["-qa", "--qf", query]).output() {
        Ok(result) if result.status.success() => {
            let text = String::from_utf8_lossy(&result.stdout);
            let mut lines = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            lines.sort();
            let mut tsv = String::from("name\tversion_release\tarch\tlicense\n");
            tsv.push_str(&lines.join("\n"));
            tsv.push('\n');
            fs::write(output.join("rpm-packages.tsv"), tsv)?;
            let stale = output.join("rpm-packages.not-generated.txt");
            if stale.exists() {
                fs::remove_file(stale)?;
            }
            Ok("generated from rpm database".to_string())
        }
        Ok(result) => {
            let stale = output.join("rpm-packages.tsv");
            if stale.exists() {
                fs::remove_file(stale)?;
            }
            let stderr = String::from_utf8_lossy(&result.stderr);
            let status = format!(
                "not generated: rpm query exited with status {}",
                result.status
            );
            fs::write(
                output.join("rpm-packages.not-generated.txt"),
                format!(
                    "{status}\nRun this evidence mode inside the built Goblins OS image or run rpm-packages.command there.\n{}\n",
                    stderr.trim()
                ),
            )?;
            Ok(status)
        }
        Err(error) => {
            let stale = output.join("rpm-packages.tsv");
            if stale.exists() {
                fs::remove_file(stale)?;
            }
            let status = "not generated: rpm command unavailable on this host".to_string();
            fs::write(
                output.join("rpm-packages.not-generated.txt"),
                format!(
                    "{status}\nRun this evidence mode inside the built Goblins OS image or run rpm-packages.command there.\n{error}\n"
                ),
            )?;
            Ok(status)
        }
    }
}

fn release_evidence_manifest(
    arch: &str,
    candidate_commit: &str,
    image_ref: &str,
    cargo_package_count: usize,
    cargo_packages_sha256: &str,
    rpm_packages_sha256: Option<&str>,
    rpm_status: &str,
) -> String {
    let rpm_packages_sha256 = rpm_packages_sha256
        .map(|value| format!("\"{}\"", json_escape(value)))
        .unwrap_or_else(|| "null".to_string());
    format!(
        concat!(
            "{{\n",
            "  \"schema\": \"goblins-os-release-evidence-v4\",\n",
            "  \"architecture\": \"{}\",\n",
            "  \"candidate_commit\": \"{}\",\n",
            "  \"image_ref\": \"{}\",\n",
            "  \"image_digest_pinned\": {},\n",
            "  \"cargo_lock\": \"Cargo.lock\",\n",
            "  \"cargo_package_count\": {},\n",
            "  \"cargo_packages_tsv\": \"cargo-lock-packages.tsv\",\n",
            "  \"cargo_packages_sha256\": \"{}\",\n",
            "  \"rpm_packages_tsv\": \"rpm-packages.tsv\",\n",
            "  \"rpm_packages_sha256\": {},\n",
            "  \"rpm_command_file\": \"rpm-packages.command\",\n",
            "  \"rpm_status\": \"{}\",\n",
            "  \"asset_provenance\": \"os/release/asset-provenance.toml\",\n",
            "  \"third_party_notices\": \"os/release/third-party-notices.toml\",\n",
            "  \"trademark_posture\": \"os/release/trademark-posture.toml\",\n",
            "  \"source_tree_manifest\": \"os/release/source-tree-manifest.toml\"\n",
            "}}\n"
        ),
        json_escape(arch),
        json_escape(candidate_commit),
        json_escape(image_ref),
        image_ref_is_digest_pinned(image_ref),
        cargo_package_count,
        json_escape(cargo_packages_sha256),
        rpm_packages_sha256,
        json_escape(rpm_status)
    )
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            value if value.is_control() => escaped.push_str(&format!("\\u{:04x}", value as u32)),
            value => escaped.push(value),
        }
    }
    escaped
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

fn file_check(root: &Path, relative: &str) -> Check {
    let path = root.join(relative);
    if path.is_file() {
        ready(relative, &format!("found {}", path.display()))
    } else {
        blocked(relative, &format!("missing {}", path.display()))
    }
}

#[cfg(unix)]
fn path_mode_check(root: &Path, relative: &str, expected: u32, id: &str) -> Check {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join(relative);
    match fs::metadata(&path) {
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o7777;
            if mode == expected {
                ready(id, &format!("{} mode is {:03o}", path.display(), expected))
            } else {
                blocked(
                    id,
                    &format!(
                        "{} mode is {:03o}; expected {:03o}",
                        path.display(),
                        mode,
                        expected
                    ),
                )
            }
        }
        Err(_) => blocked(id, &format!("missing {}", path.display())),
    }
}

#[cfg(not(unix))]
fn path_mode_check(_root: &Path, relative: &str, _expected: u32, id: &str) -> Check {
    blocked(
        id,
        &format!("mode check for {relative} cannot be verified on a non-Unix host"),
    )
}

#[cfg(unix)]
fn path_owner_check(
    root: &Path,
    relative: &str,
    expected_uid: u32,
    expected_gid: u32,
    id: &str,
) -> Check {
    use std::os::unix::fs::MetadataExt;

    let path = root.join(relative);
    match fs::metadata(&path) {
        Ok(metadata) => {
            let uid = metadata.uid();
            let gid = metadata.gid();
            if uid == expected_uid && gid == expected_gid {
                ready(id, &format!("{} owned by uid {uid}:{gid}", path.display()))
            } else {
                blocked(
                    id,
                    &format!(
                        "{} owned by uid {uid}:{gid}; expected {expected_uid}:{expected_gid}",
                        path.display()
                    ),
                )
            }
        }
        Err(_) => blocked(id, &format!("missing {}", path.display())),
    }
}

#[cfg(not(unix))]
fn path_owner_check(
    _root: &Path,
    relative: &str,
    _expected_uid: u32,
    _expected_gid: u32,
    id: &str,
) -> Check {
    blocked(
        id,
        &format!("owner check for {relative} cannot be verified on a non-Unix host"),
    )
}

/// Assert that a secret directory is NOT readable by the auto-login desktop
/// session user. The directory under /var/lib/goblins-os is owned by the
/// dynamically-allocated `goblins-os --system` account, so we do not hardcode
/// root; instead we encode the actual security property: the directory must not
/// be owned by the desktop user (uid 1000 / `goblin`) and must expose no group
/// or other read/execute bits, so the GUI session can never traverse or read it.
#[cfg(unix)]
fn secret_dir_not_session_readable_check(root: &Path, relative: &str, id: &str) -> Check {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    const DESKTOP_USER_UID: u32 = 1000;

    let path = root.join(relative);
    match fs::metadata(&path) {
        Ok(metadata) => {
            let uid = metadata.uid();
            let group_other_bits = metadata.permissions().mode() & 0o077;
            if uid == DESKTOP_USER_UID {
                blocked(
                    id,
                    &format!(
                        "{} is owned by the desktop session user (uid {uid})",
                        path.display()
                    ),
                )
            } else if group_other_bits != 0 {
                blocked(
                    id,
                    &format!(
                        "{} grants group/other access ({:03o})",
                        path.display(),
                        group_other_bits
                    ),
                )
            } else {
                ready(
                    id,
                    &format!(
                        "{} is owned by uid {uid} with no group/other access",
                        path.display()
                    ),
                )
            }
        }
        Err(_) => blocked(id, &format!("missing {}", path.display())),
    }
}

#[cfg(not(unix))]
fn secret_dir_not_session_readable_check(_root: &Path, relative: &str, id: &str) -> Check {
    blocked(
        id,
        &format!("secret-dir ownership for {relative} cannot be verified on a non-Unix host"),
    )
}

fn file_has_no_active_secrets_check(root: &Path, relative: &str, id: &str) -> Check {
    let path = root.join(relative);
    let text = read_to_string(&path);
    if text.lines().any(is_suspicious_secret_line) {
        blocked(
            id,
            &format!("{} contains active secret material", path.display()),
        )
    } else {
        ready(
            id,
            &format!("{} has no active secret assignments", path.display()),
        )
    }
}

fn path_absent_check(root: &Path, relative: &str, id: &str) -> Check {
    let path = root.join(relative);
    if path.exists() {
        blocked(id, &format!("{} exists", path.display()))
    } else {
        ready(id, &format!("{} is absent", path.display()))
    }
}

fn workspace_member_check(root: &Path, binary: &str) -> Check {
    let member = match binary {
        "goblins-os-dictate" | "goblins-os-focus-tick" | "goblins-os-voice-control" => {
            "crates/goblins-os-session-tools".to_string()
        }
        _ => {
            let crate_name = binary.strip_prefix("goblins-os-").unwrap_or(binary);
            format!("crates/goblins-os-{crate_name}")
        }
    };
    let text = read_to_string(root.join("Cargo.toml"));
    if text.contains(&member) {
        ready(
            &format!("workspace-{binary}"),
            &format!("workspace includes {member}"),
        )
    } else {
        blocked(
            &format!("workspace-{binary}"),
            &format!("workspace is missing {member}"),
        )
    }
}

fn container_copy_check(root: &Path, binary: &str) -> Check {
    let path = root.join("os/bootc/Containerfile");
    let text = read_to_string(&path);
    let list_line = format!("{binary} \\");
    let terminal_list_line = format!("{binary}; do");
    let staged_install_listed = text
        .lines()
        .map(str::trim)
        .any(|line| line == list_line || line == terminal_list_line);
    let stages_binaries = text.contains("/out/usr/libexec/goblins-os")
        && text.contains("install -m 0755 \"/src/target/release/${binary}\" \"/out/usr/libexec/goblins-os/${binary}\"");
    let copies_staged_tree = text.contains("COPY --from=rust-build /out/ /");

    if staged_install_listed && stages_binaries && copies_staged_tree {
        ready(
            &format!("container-copy-{binary}"),
            &format!("Containerfile stages and copies {binary} into /usr/libexec/goblins-os"),
        )
    } else {
        blocked(
            &format!("container-copy-{binary}"),
            &format!("Containerfile does not stage and copy {binary} into /usr/libexec/goblins-os"),
        )
    }
}

fn container_contains_check(root: &Path, id: &str, needle: &str) -> Check {
    contains_check(root.join("os/bootc/Containerfile"), id, needle)
}

fn container_order_check(root: &Path, id: &str, first: &str, second: &str) -> Check {
    ordered_contains_check(root.join("os/bootc/Containerfile"), id, first, second)
}

fn container_absent_check(root: &Path, id: &str, needle: &str) -> Check {
    absent_check(root.join("os/bootc/Containerfile"), id, needle)
}

fn container_package_not_installed_check(root: &Path, id: &str, package: &str) -> Check {
    let path = root.join("os/bootc/Containerfile");
    let text = read_to_string(&path);
    let package_with_slash = format!("{package} \\");
    let package_with_space = format!("{package} ");
    let installed = text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == package
            || trimmed == package_with_slash
            || trimmed.starts_with(&package_with_space)
    });

    if installed {
        blocked(
            id,
            &format!("{} installs package {}", path.display(), package),
        )
    } else {
        ready(
            id,
            &format!("{} does not install package {}", path.display(), package),
        )
    }
}

fn container_package_lockstep_check(root: &Path, id: &str, package: &str) -> Check {
    let path = root.join("os/bootc/Containerfile");
    let text = read_to_string(&path);
    let install_segment = text.split("&& rpm -q").next().unwrap_or_default();
    let rpm_segment = text
        .split_once("&& rpm -q")
        .map(|(_, rest)| rest.split("&& command -v").next().unwrap_or_default())
        .unwrap_or_default();
    let installed = segment_has_package(install_segment, package);
    let rpm_asserted = segment_has_package(rpm_segment, package);

    match (installed, rpm_asserted) {
        (true, true) => ready(
            id,
            &format!(
                "{} installs and rpm-asserts package {}",
                path.display(),
                package
            ),
        ),
        (false, true) => blocked(
            id,
            &format!(
                "{} rpm-asserts but does not install {}",
                path.display(),
                package
            ),
        ),
        (true, false) => blocked(
            id,
            &format!(
                "{} installs but does not rpm-assert {}",
                path.display(),
                package
            ),
        ),
        (false, false) => blocked(
            id,
            &format!("{} is missing package {}", path.display(), package),
        ),
    }
}

fn segment_has_package(segment: &str, package: &str) -> bool {
    segment
        .split_whitespace()
        .map(|token| token.trim_end_matches('\\'))
        .any(|token| token == package)
}

fn session_contains_check(root: &Path, id: &str, needle: &str) -> Check {
    contains_check(root.join("os/session/goblins-os-session"), id, needle)
}

fn contains_check(path: PathBuf, id: &str, needle: &str) -> Check {
    let text = read_to_string(&path);
    if text.contains(needle) {
        ready(id, &format!("{} contains {}", path.display(), needle))
    } else {
        blocked(id, &format!("{} is missing {}", path.display(), needle))
    }
}

fn ordered_contains_check(path: PathBuf, id: &str, first: &str, second: &str) -> Check {
    let text = read_to_string(&path);
    let Some(first_index) = text.find(first) else {
        return blocked(id, &format!("{} is missing {}", path.display(), first));
    };
    let Some(second_index) = text.find(second) else {
        return blocked(id, &format!("{} is missing {}", path.display(), second));
    };

    if first_index < second_index {
        ready(
            id,
            &format!("{} orders {} before {}", path.display(), first, second),
        )
    } else {
        blocked(
            id,
            &format!(
                "{} orders {} after {}; expected before",
                path.display(),
                first,
                second
            ),
        )
    }
}

fn symlink_target_check(root: &Path, relative: &str, id: &str, expected_target: &str) -> Check {
    let path = root.join(relative);
    match fs::read_link(&path) {
        Ok(target) if target.to_string_lossy().contains(expected_target) => ready(
            id,
            &format!("{} points to {}", path.display(), target.display()),
        ),
        Ok(target) => blocked(
            id,
            &format!(
                "{} points to {}; expected {}",
                path.display(),
                target.display(),
                expected_target
            ),
        ),
        Err(error) => blocked(
            id,
            &format!("{} is not a readable symlink: {error}", path.display()),
        ),
    }
}

fn shell_function_contains_check(
    path: PathBuf,
    function_name: &str,
    id: &str,
    needle: &str,
) -> Check {
    let text = read_to_string(&path);
    let marker = format!("{function_name}() {{");
    let Some(start) = text.find(&marker) else {
        return blocked(
            id,
            &format!(
                "{} is missing shell function {function_name}",
                path.display()
            ),
        );
    };
    let body = text[start..]
        .split_once("\n}\n")
        .map(|(body, _)| body)
        .unwrap_or(&text[start..]);
    if body.contains(needle) {
        ready(
            id,
            &format!("{} {function_name} contains {}", path.display(), needle),
        )
    } else {
        blocked(
            id,
            &format!("{} {function_name} is missing {}", path.display(), needle),
        )
    }
}

/// Like `contains_check`, but collapses runs of whitespace in the file to a
/// single space before searching, so the needle survives any benign re-wrap or
/// reflow of the source document. Used for prose checks where the asserted
/// adjacency (e.g. `0600 root:root`) must hold but the line breaks may move.
fn whitespace_normalized_contains_check(path: PathBuf, id: &str, needle: &str) -> Check {
    let text = read_to_string(&path);
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.contains(needle) {
        ready(id, &format!("{} contains {}", path.display(), needle))
    } else {
        blocked(id, &format!("{} is missing {}", path.display(), needle))
    }
}

fn absent_check(path: PathBuf, id: &str, needle: &str) -> Check {
    let text = read_to_string(&path);
    if text.contains(needle) {
        blocked(id, &format!("{} still contains {}", path.display(), needle))
    } else {
        ready(
            id,
            &format!("{} does not contain {}", path.display(), needle),
        )
    }
}

fn github_workflow_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    let directory = root.join(".github/workflows");
    let entries = fs::read_dir(&directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "cannot read an entry under {}: {error}",
                directory.display()
            )
        })?;
        let path = entry.path();
        if path.is_file()
            && matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("yml" | "yaml")
            )
        {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        Err(format!(
            "{} contains no YAML workflows",
            directory.display()
        ))
    } else {
        Ok(paths)
    }
}

fn reviewed_github_action_pins_check(root: &Path) -> Check {
    let paths = match github_workflow_paths(root) {
        Ok(paths) => paths,
        Err(error) => return blocked("github-action-pins-reviewed-node24", &error),
    };
    let mut seen = BTreeMap::<String, usize>::new();
    let mut problems = Vec::new();

    for path in paths {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                problems.push(format!("cannot read {}: {error}", path.display()));
                continue;
            }
        };
        let documents = match Yaml::load_from_str(&text) {
            Ok(documents) if documents.len() == 1 => documents,
            Ok(documents) => {
                problems.push(format!(
                    "{} must contain exactly one YAML document, found {}",
                    path.display(),
                    documents.len()
                ));
                continue;
            }
            Err(error) => {
                problems.push(format!("cannot parse {} as YAML: {error}", path.display()));
                continue;
            }
        };
        let mut references = Vec::new();
        collect_workflow_uses_references(&documents[0], &mut references, &mut problems, &path);
        for reference in references {
            if reference.starts_with("./") {
                continue;
            }
            let Some((action, pin)) = reference.split_once('@') else {
                problems.push(format!(
                    "{} has an unreviewed or unpinned action reference {reference}",
                    path.display()
                ));
                continue;
            };
            match REVIEWED_GITHUB_ACTION_PINS
                .iter()
                .find(|(reviewed_action, _)| *reviewed_action == action)
            {
                Some((_, reviewed_pin)) if *reviewed_pin == pin => {
                    *seen.entry(action.to_string()).or_insert(0) += 1;
                }
                Some((_, reviewed_pin)) => problems.push(format!(
                    "{} pins {action}@{pin}; expected {action}@{reviewed_pin}",
                    path.display()
                )),
                None => problems.push(format!(
                    "{} uses unreviewed external action {reference}",
                    path.display()
                )),
            }
        }
    }

    for (action, pin) in REVIEWED_GITHUB_ACTION_PINS {
        if !seen.contains_key(*action) {
            problems.push(format!("release workflows are missing {action}@{pin}"));
        }
    }

    if problems.is_empty() {
        let counts = REVIEWED_GITHUB_ACTION_PINS
            .iter()
            .map(|(action, _)| format!("{action}={}", seen.get(*action).copied().unwrap_or(0)))
            .collect::<Vec<_>>()
            .join(", ");
        ready(
            "github-action-pins-reviewed-node24",
            &format!("all external workflow actions use the reviewed immutable pins ({counts})"),
        )
    } else {
        blocked("github-action-pins-reviewed-node24", &problems.join("; "))
    }
}

fn collect_workflow_uses_references(
    node: &Yaml<'_>,
    references: &mut Vec<String>,
    problems: &mut Vec<String>,
    path: &Path,
) {
    match node {
        Yaml::Mapping(mapping) => {
            for (key, value) in mapping {
                let Some(key) = key.as_str() else {
                    problems.push(format!(
                        "{} has a non-literal YAML mapping key; workflow keys must be explicit strings",
                        path.display()
                    ));
                    collect_workflow_uses_references(value, references, problems, path);
                    continue;
                };
                if key == "uses" {
                    match value.as_str() {
                        Some(reference) if !reference.trim().is_empty() => {
                            references.push(reference.trim().to_string());
                        }
                        _ => problems.push(format!(
                            "{} has a non-literal or empty uses value; aliases and computed action references are not allowed",
                            path.display()
                        )),
                    }
                }
                collect_workflow_uses_references(value, references, problems, path);
            }
        }
        Yaml::Sequence(sequence) => {
            for value in sequence {
                collect_workflow_uses_references(value, references, problems, path);
            }
        }
        Yaml::Tagged(_, value) => {
            collect_workflow_uses_references(value, references, problems, path);
        }
        Yaml::Alias(_) => problems.push(format!(
            "{} uses a YAML alias; workflow action references must be explicit literals",
            path.display()
        )),
        Yaml::BadValue => {
            problems.push(format!("{} contains an invalid YAML value", path.display()))
        }
        _ => {}
    }
}

fn deprecated_github_action_pins_absent_check(root: &Path) -> Check {
    let paths = match github_workflow_paths(root) {
        Ok(paths) => paths,
        Err(error) => return blocked("github-action-pins-no-deprecated-node20", &error),
    };
    let mut hits = Vec::new();
    for path in paths {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                hits.push(format!("cannot read {}: {error}", path.display()));
                continue;
            }
        };
        for pin in DEPRECATED_GITHUB_ACTION_PINS {
            if text.contains(pin) {
                hits.push(format!("{} contains deprecated pin {pin}", path.display()));
            }
        }
    }
    if hits.is_empty() {
        ready(
            "github-action-pins-no-deprecated-node20",
            "release workflows contain none of the retired Node 20 action pins",
        )
    } else {
        blocked("github-action-pins-no-deprecated-node20", &hits.join("; "))
    }
}

fn source_secret_scan_check(root: &Path) -> Check {
    match source_secret_scan_hits(root) {
        Ok(hits) if hits.is_empty() => ready(
            "source-secret-scan",
            "no live provider keys or active secret assignments found in source package",
        ),
        Ok(hits) => blocked(
            "source-secret-scan",
            &format!("possible live secrets found: {}", hits.join(", ")),
        ),
        Err(error) => blocked("source-secret-scan", &error),
    }
}

const SECRET_SCAN_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

fn source_secret_scan_hits(root: &Path) -> Result<Vec<String>, String> {
    let mut command = Command::new("rg");
    command
        .current_dir(root)
        .arg("--line-number")
        .arg("--no-heading")
        .arg("--with-filename")
        .arg("--no-messages")
        .arg("--hidden")
        .arg("-I")
        .arg("--max-filesize")
        .arg(format!("{}K", SECRET_SCAN_MAX_FILE_BYTES / 1024))
        .arg("--glob")
        .arg("!.git/**")
        .arg("--glob")
        .arg("!.claude/**")
        .arg("--glob")
        .arg("!**/node_modules/**")
        .arg("--glob")
        .arg("!**/.next/**")
        .arg("--glob")
        .arg("!**/.vercel/**")
        .arg("--glob")
        .arg("!.ci-target/**")
        .arg("--glob")
        .arg("!.ci-target-amd64/**")
        .arg("--glob")
        .arg("!target/**")
        .arg("--glob")
        .arg("!artifacts/**")
        .arg("--glob")
        .arg("!libpod/**")
        .arg("--glob")
        .arg("!os/signoff-proofs/**")
        .arg("--glob")
        .arg("!os/screenshots/**")
        .arg("--glob")
        .arg("!os/iso/output/**")
        .arg("-e")
        .arg(
            r"OPENAI_API_KEY|AI_GATEWAY_API_KEY|OPENAI_ACCOUNT_CLIENT_SECRET|sk-proj-[A-Za-z0-9_-]{24,}|sk-[A-Za-z0-9_-]{29,}|^[[:space:]]*(export[[:space:]]+)?[A-Za-z0-9_]*(KEY|SECRET|TOKEN)[[:space:]]*=",
        )
        .arg(".");

    let output = command
        .output()
        .map_err(|error| format!("could not run rg source secret scan: {error}"))?;
    if !output.status.success() {
        if output.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("rg source secret scan failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let hits = stdout
        .lines()
        .filter_map(rg_secret_scan_hit)
        .collect::<Vec<_>>();
    Ok(hits)
}

fn rg_secret_scan_hit(line: &str) -> Option<String> {
    let mut parts = line.splitn(3, ':');
    let path = parts.next()?;
    let line_number = parts.next()?;
    let text = parts.next()?;
    if is_suspicious_secret_line(text) {
        Some(format!("{path}:{line_number}"))
    } else {
        None
    }
}

fn read_bounded_text_file(path: &Path, byte_len: u64) -> Option<String> {
    let len = usize::try_from(byte_len).ok()?;
    let file = fs::File::open(path).ok()?;
    #[cfg(unix)]
    {
        let mut bytes = vec![0_u8; len];
        let mut offset = 0_usize;
        while offset < len {
            let read = file.read_at(&mut bytes[offset..], offset as u64).ok()?;
            if read == 0 {
                bytes.truncate(offset);
                break;
            }
            offset += read;
        }
        String::from_utf8(bytes).ok()
    }
    #[cfg(not(unix))]
    let mut file = file;
    #[cfg(not(unix))]
    {
        let mut bytes = Vec::with_capacity(len);
        file.take(byte_len).read_to_end(&mut bytes).ok()?;
        return String::from_utf8(bytes).ok();
    }
}

#[cfg(test)]
fn should_skip_secret_scan_path(relative: &Path) -> bool {
    let path = relative.to_string_lossy();
    path == ".git"
        || path.starts_with(".git/")
        || path == ".claude"
        || path.starts_with(".claude/")
        || path == ".ci-target"
        || path.starts_with(".ci-target/")
        || path == ".ci-target-amd64"
        || path.starts_with(".ci-target-amd64/")
        || path == "target"
        || path.starts_with("target/")
        || path == "artifacts"
        || path.starts_with("artifacts/")
        || path == "libpod"
        || path.starts_with("libpod/")
        || path == "os/signoff-proofs"
        || path.starts_with("os/signoff-proofs/")
        || path == "os/screenshots"
        || path.starts_with("os/screenshots/")
        || path == "os/iso/output"
        || path.starts_with("os/iso/output")
}

fn is_suspicious_secret_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with("//")
        || trimmed.starts_with('*')
    {
        return false;
    }

    let upper = trimmed.to_ascii_uppercase();
    let active_secret_assignment = [
        "OPENAI_API_KEY",
        "AI_GATEWAY_API_KEY",
        "OPENAI_ACCOUNT_CLIENT_SECRET",
    ]
    .iter()
    .any(|name| {
        upper.starts_with(&format!("{name}="))
            || upper.starts_with(&format!("EXPORT {name}="))
            || upper.contains(&format!(" {name}="))
    });
    if active_secret_assignment && !trimmed.contains('<') {
        return true;
    }

    if is_populated_key_secret_token_assignment(trimmed) {
        return true;
    }

    contains_realish_openai_key(trimmed)
}

/// Line-anchored mirror of the Containerfile guard
/// `^[[:space:]]*[A-Za-z0-9_]*(KEY|SECRET|TOKEN)[[:space:]]*=`: flag a line whose
/// leading identifier (optionally preceded by `export `) matches
/// `^[A-Za-z0-9_]*(KEY|SECRET|TOKEN)$` immediately followed by `=`, with a
/// non-empty RHS that is neither a `<placeholder>` nor an allowed dummy. Anchored
/// to line start (never a mid-line `.contains`) so the whole-repo source scan does
/// not produce false positives.
fn is_populated_key_secret_token_assignment(trimmed: &str) -> bool {
    let candidate = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let Some((name, value)) = candidate.split_once('=') else {
        return false;
    };
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return false;
    }
    let upper = name.to_ascii_uppercase();
    if !(upper.ends_with("KEY") || upper.ends_with("SECRET") || upper.ends_with("TOKEN")) {
        return false;
    }
    if value.is_empty() || value.starts_with('<') {
        return false;
    }
    !is_allowed_dummy_secret(value)
}

fn contains_realish_openai_key(line: &str) -> bool {
    for prefix in ["sk-proj-", "sk-"] {
        for (index, _) in line.match_indices(prefix) {
            if index > 0 {
                let previous = line[..index].chars().next_back().unwrap_or_default();
                if previous.is_ascii_alphanumeric() || previous == '-' || previous == '_' {
                    continue;
                }
            }
            let token = line[index..]
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
                .collect::<String>();
            if token.len() >= 32 && !is_allowed_dummy_secret(&token) {
                return true;
            }
        }
    }
    false
}

fn is_allowed_dummy_secret(token: &str) -> bool {
    // Only shield tokens that are structurally non-random: a real key will never
    // contain the full 26-letter sequential alphabet run. English-word
    // substrings ("example", "placeholder", ...) are intentionally NOT exempted,
    // because a genuine leaked key can contain such a fragment.
    token
        .to_ascii_lowercase()
        .contains("abcdefghijklmnopqrstuvwxyz")
}

fn desktop_exec_checks(root: &Path) -> Vec<Check> {
    let mut checks = Vec::new();
    for relative in APPLICATIONS
        .iter()
        .map(|name| format!("os/applications/{name}"))
        .chain(AUTOSTART.iter().map(|name| format!("os/autostart/{name}")))
    {
        let path = root.join(&relative);
        let text = read_to_string(&path);
        let exec = desktop_field(&text, "Exec");
        if exec.is_some_and(|value| value.starts_with("/usr/libexec/goblins-os/")) {
            checks.push(ready(
                &format!("desktop-exec-{relative}"),
                &format!("{relative} uses OS-owned launcher path"),
            ));
        } else {
            checks.push(blocked(
                &format!("desktop-exec-{relative}"),
                &format!("{relative} must launch through /usr/libexec/goblins-os"),
            ));
        }
    }
    checks
}

#[derive(Clone, Copy)]
struct NativeCoreClient {
    slug: &'static str,
    binary: &'static str,
    entrypoint: &'static str,
    kind: &'static str,
    setgid: bool,
}

const NATIVE_CORE_CLIENTS: [NativeCoreClient; 16] = [
    NativeCoreClient {
        slug: "control-center",
        binary: "goblins-os-control-center",
        entrypoint: "crates/goblins-os-control-center/src/main.rs",
        kind: "ControlCenter",
        setgid: true,
    },
    NativeCoreClient {
        slug: "dictate",
        binary: "goblins-os-dictate",
        entrypoint: "crates/goblins-os-session-tools/src/bin/goblins-os-dictate.rs",
        kind: "Dictate",
        setgid: true,
    },
    NativeCoreClient {
        slug: "file-builder",
        binary: "goblins-os-file-builder",
        entrypoint: "crates/goblins-os-file-builder/src/main.rs",
        kind: "FileBuilder",
        setgid: true,
    },
    NativeCoreClient {
        slug: "focus-tick",
        binary: "goblins-os-focus-tick",
        entrypoint: "crates/goblins-os-session-tools/src/bin/goblins-os-focus-tick.rs",
        kind: "FocusTick",
        setgid: true,
    },
    NativeCoreClient {
        slug: "installer",
        binary: "goblins-os-installer",
        entrypoint: "crates/goblins-os-installer/src/main.rs",
        kind: "Installer",
        setgid: true,
    },
    NativeCoreClient {
        slug: "launcher",
        binary: "goblins-os-launcher",
        entrypoint: "crates/goblins-os-launcher/src/main.rs",
        kind: "Launcher",
        setgid: true,
    },
    NativeCoreClient {
        slug: "login",
        binary: "goblins-os-login",
        entrypoint: "crates/goblins-os-login/src/main.rs",
        kind: "Login",
        setgid: true,
    },
    NativeCoreClient {
        slug: "markup",
        binary: "goblins-os-markup",
        entrypoint: "crates/goblins-os-markup/src/main.rs",
        kind: "Markup",
        setgid: true,
    },
    NativeCoreClient {
        slug: "open",
        binary: "goblins-os-open",
        entrypoint: "crates/goblins-os-open/src/main.rs",
        kind: "Open",
        setgid: true,
    },
    NativeCoreClient {
        slug: "resident",
        binary: "goblins-os-resident",
        entrypoint: "crates/goblins-os-resident/src/main.rs",
        kind: "Resident",
        setgid: false,
    },
    NativeCoreClient {
        slug: "screenshot-context",
        binary: "goblins-os-screenshot-context",
        entrypoint: "crates/goblins-os-screenshot-context/src/main.rs",
        kind: "ScreenshotContext",
        setgid: true,
    },
    NativeCoreClient {
        slug: "settings",
        binary: "goblins-os-settings",
        entrypoint: "crates/goblins-os-settings/src/main.rs",
        kind: "Settings",
        setgid: true,
    },
    NativeCoreClient {
        slug: "shell",
        binary: "goblins-os-shell",
        entrypoint: "crates/goblins-os-shell/src/main.rs",
        kind: "Shell",
        setgid: true,
    },
    NativeCoreClient {
        slug: "today",
        binary: "goblins-os-today",
        entrypoint: "crates/goblins-os-today/src/main.rs",
        kind: "Today",
        setgid: true,
    },
    NativeCoreClient {
        slug: "visual-lookup",
        binary: "goblins-os-visual-lookup",
        entrypoint: "crates/goblins-os-visual-lookup/src/main.rs",
        kind: "VisualLookup",
        setgid: true,
    },
    NativeCoreClient {
        slug: "voice-control",
        binary: "goblins-os-voice-control",
        entrypoint: "crates/goblins-os-session-tools/src/bin/goblins-os-voice-control.rs",
        kind: "VoiceControl",
        setgid: true,
    },
];

const NATIVE_CORE_CLIENT_CRATES: [&str; 14] = [
    "crates/goblins-os-control-center",
    "crates/goblins-os-file-builder",
    "crates/goblins-os-installer",
    "crates/goblins-os-launcher",
    "crates/goblins-os-login",
    "crates/goblins-os-markup",
    "crates/goblins-os-open",
    "crates/goblins-os-resident",
    "crates/goblins-os-screenshot-context",
    "crates/goblins-os-session-tools",
    "crates/goblins-os-settings",
    "crates/goblins-os-shell",
    "crates/goblins-os-today",
    "crates/goblins-os-visual-lookup",
];

fn cfg_predicate_implies_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|predicates| predicates.iter().any(cfg_predicate_implies_test)),
        syn::Meta::List(list) if list.path.is_ident("any") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|predicates| predicates.iter().all(cfg_predicate_implies_test)),
        _ => false,
    }
}

fn attributes_are_test_only(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .parse_args::<syn::Meta>()
                .is_ok_and(|predicate| cfg_predicate_implies_test(&predicate))
    })
}

fn item_is_test_only(item: &syn::Item) -> bool {
    let attributes = match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        syn::Item::Verbatim(_) => return false,
        _ => return false,
    };
    attributes_are_test_only(attributes)
}

fn impl_item_is_test_only(item: &syn::ImplItem) -> bool {
    let attributes = match item {
        syn::ImplItem::Const(item) => &item.attrs,
        syn::ImplItem::Fn(item) => &item.attrs,
        syn::ImplItem::Type(item) => &item.attrs,
        syn::ImplItem::Macro(item) => &item.attrs,
        syn::ImplItem::Verbatim(_) => return false,
        _ => return false,
    };
    attributes_are_test_only(attributes)
}

fn trait_item_is_test_only(item: &syn::TraitItem) -> bool {
    let attributes = match item {
        syn::TraitItem::Const(item) => &item.attrs,
        syn::TraitItem::Fn(item) => &item.attrs,
        syn::TraitItem::Type(item) => &item.attrs,
        syn::TraitItem::Macro(item) => &item.attrs,
        syn::TraitItem::Verbatim(_) => return false,
        _ => return false,
    };
    attributes_are_test_only(attributes)
}

fn foreign_item_is_test_only(item: &syn::ForeignItem) -> bool {
    let attributes = match item {
        syn::ForeignItem::Fn(item) => &item.attrs,
        syn::ForeignItem::Static(item) => &item.attrs,
        syn::ForeignItem::Type(item) => &item.attrs,
        syn::ForeignItem::Macro(item) => &item.attrs,
        syn::ForeignItem::Verbatim(_) => return false,
        _ => return false,
    };
    attributes_are_test_only(attributes)
}

fn initialize_kind_from_call(call: &syn::ExprCall) -> Option<String> {
    let syn::Expr::Path(function) = call.func.as_ref() else {
        return None;
    };
    if !function.path.segments.last()?.ident.eq("initialize") || call.args.len() != 1 {
        return None;
    }
    let syn::Expr::Path(argument) = call.args.first()? else {
        return None;
    };
    let segments = argument.path.segments.iter().collect::<Vec<_>>();
    if segments.len() < 2 || !segments[segments.len() - 2].ident.eq("ClientKind") {
        return None;
    }
    Some(segments.last()?.ident.to_string())
}

fn leading_initialize_kind(expression: &syn::Expr) -> Option<String> {
    match expression {
        syn::Expr::Call(call) => initialize_kind_from_call(call),
        syn::Expr::Group(group) => leading_initialize_kind(&group.expr),
        syn::Expr::Match(expression) => leading_initialize_kind(&expression.expr),
        syn::Expr::Paren(expression) => leading_initialize_kind(&expression.expr),
        syn::Expr::Try(expression) => leading_initialize_kind(&expression.expr),
        _ => None,
    }
}

fn first_executable_initialization(function: &syn::ItemFn) -> Option<String> {
    let statement = function
        .block
        .stmts
        .iter()
        .find(|statement| !matches!(statement, syn::Stmt::Item(_)))?;
    match statement {
        syn::Stmt::Local(local) => local
            .init
            .as_ref()
            .and_then(|initializer| leading_initialize_kind(&initializer.expr)),
        syn::Stmt::Expr(expression, _) => leading_initialize_kind(expression),
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => None,
    }
}

fn collect_use_names(tree: &syn::UseTree, names: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Name(name) => {
            names.insert(name.ident.to_string());
        }
        syn::UseTree::Rename(rename) => {
            names.insert(rename.ident.to_string());
        }
        syn::UseTree::Path(path) => collect_use_names(&path.tree, names),
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_names(tree, names);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

fn imports_shared_core_initializer(syntax: &syn::File) -> bool {
    let mut names = BTreeSet::new();
    for item in &syntax.items {
        let syn::Item::Use(item) = item else {
            continue;
        };
        let syn::UseTree::Path(root) = &item.tree else {
            continue;
        };
        if root.ident == "goblins_os_core_client" {
            collect_use_names(&root.tree, &mut names);
        }
    }
    names.contains("initialize") && names.contains("ClientKind")
}

fn first_main_initialization_check(path: PathBuf, slug: &str, expected_kind: &str) -> Check {
    let id = format!("core-capability-{slug}-initializes-first-with-exact-kind");
    let source = read_to_string(&path);
    let syntax = match syn::parse_file(&source) {
        Ok(syntax) => syntax,
        Err(error) => {
            return blocked(
                &id,
                &format!("{} is not valid Rust: {error}", path.display()),
            );
        }
    };
    if !imports_shared_core_initializer(&syntax) {
        return blocked(
            &id,
            &format!(
                "{} must import initialize and ClientKind from goblins_os_core_client",
                path.display()
            ),
        );
    }
    let mains = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(function)
                if function.sig.ident == "main" && !attributes_are_test_only(&function.attrs) =>
            {
                Some(function)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if mains.len() != 1 {
        return blocked(
            &id,
            &format!(
                "{} must contain exactly one production main function; found {}",
                path.display(),
                mains.len()
            ),
        );
    }
    let actual = first_executable_initialization(mains[0]);
    if actual.as_deref() == Some(expected_kind) {
        ready(
            &id,
            &format!(
                "{} first executable main statement consumes ClientKind::{expected_kind}",
                path.display()
            ),
        )
    } else {
        blocked(
            &id,
            &format!(
                "{} first executable main statement must begin with initialize(ClientKind::{expected_kind}); found {}",
                path.display(),
                actual.as_deref().unwrap_or("no leading initialize call")
            ),
        )
    }
}

fn forbidden_client_token(value: &str) -> Option<&'static str> {
    if value.contains("GOBLINS_OS_CORE_") || value.contains("OPENAI_OS_CORE_") {
        return Some("core environment override");
    }
    let lower = value.to_ascii_lowercase();
    let compact = lower
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    if compact.contains("tcpstream") {
        return Some("raw TcpStream transport");
    }
    if compact.contains("coreurl") {
        return Some("core URL override");
    }
    if compact.contains("coresocket") {
        return Some("core socket override");
    }
    if compact.contains("coreport") {
        return Some("core port override");
    }
    if lower.contains("/run/goblins-os-core") || lower.contains("control.sock") {
        return Some("raw core socket path");
    }
    if lower.contains("127.0.0.1:8787") || lower.contains("localhost:8787") {
        return Some("raw core loopback address");
    }
    None
}

#[derive(Default)]
struct ForbiddenClientTokenVisitor {
    hits: BTreeSet<String>,
}

impl ForbiddenClientTokenVisitor {
    fn inspect(&mut self, value: &str) {
        if let Some(reason) = forbidden_client_token(value) {
            self.hits.insert(reason.to_string());
        }
    }

    fn inspect_token_stream(&mut self, tokens: proc_macro2::TokenStream) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Group(group) => self.inspect_token_stream(group.stream()),
                proc_macro2::TokenTree::Ident(identifier) => {
                    self.inspect(&identifier.to_string());
                }
                proc_macro2::TokenTree::Literal(literal) => self.inspect(&literal.to_string()),
                proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for ForbiddenClientTokenVisitor {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !item_is_test_only(item) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if !impl_item_is_test_only(item) {
            syn::visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        if !trait_item_is_test_only(item) {
            syn::visit::visit_trait_item(self, item);
        }
    }

    fn visit_foreign_item(&mut self, item: &'ast syn::ForeignItem) {
        if !foreign_item_is_test_only(item) {
            syn::visit::visit_foreign_item(self, item);
        }
    }

    fn visit_ident(&mut self, identifier: &'ast syn::Ident) {
        self.inspect(&identifier.to_string());
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.inspect(&literal.value());
    }

    fn visit_lit_byte_str(&mut self, literal: &'ast syn::LitByteStr) {
        if let Ok(value) = String::from_utf8(literal.value()) {
            self.inspect(&value);
        }
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.inspect_token_stream(item.tokens.clone());
        syn::visit::visit_macro(self, item);
    }
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not read {}: {error}", directory.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("could not inspect {}: {error}", directory.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn client_rust_transport_check(path: PathBuf, id_suffix: &str) -> Check {
    let id = format!("core-capability-production-source-{id_suffix}-uses-shared-client-only");
    let source = read_to_string(&path);
    let syntax = match syn::parse_file(&source) {
        Ok(syntax) => syntax,
        Err(error) => {
            return blocked(
                &id,
                &format!("{} is not valid Rust: {error}", path.display()),
            );
        }
    };
    let mut visitor = ForbiddenClientTokenVisitor::default();
    visitor.visit_file(&syntax);
    if visitor.hits.is_empty() {
        ready(
            &id,
            &format!(
                "{} contains no production raw core transport or override token",
                path.display()
            ),
        )
    } else {
        blocked(
            &id,
            &format!(
                "{} contains forbidden production token classes: {}",
                path.display(),
                visitor.hits.into_iter().collect::<Vec<_>>().join(", ")
            ),
        )
    }
}

fn client_manifest_transport_check(path: PathBuf, id_suffix: &str) -> Check {
    let id = format!("core-capability-manifest-{id_suffix}-cannot-bypass-shared-client");
    let source = read_to_string(&path);
    let hits = source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            forbidden_client_token(line)
                .or_else(|| {
                    line.contains("test-transport")
                        .then_some("test transport feature")
                })
                .map(|reason| format!("line {}: {reason}", index + 1))
        })
        .collect::<Vec<_>>();
    let shared_dependency = "goblins-os-core-client = { path = \"../goblins-os-core-client\" }";
    let dependency_count = source
        .lines()
        .filter(|line| line.trim() == shared_dependency)
        .count();
    if hits.is_empty() && dependency_count == 1 {
        ready(
            &id,
            &format!(
                "{} has one exact shared-client dependency and no transport bypass dependency or feature",
                path.display()
            ),
        )
    } else {
        blocked(
            &id,
            &format!(
                "{} must have one exact {shared_dependency} dependency (found {dependency_count}) and no bypass token; findings: {}",
                path.display(),
                if hits.is_empty() { "none".to_string() } else { hits.join(", ") }
            ),
        )
    }
}

fn enum_variant_inventory(source: &str, enum_name: &str) -> Result<Vec<String>, String> {
    let syntax = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut enums = syntax.items.iter().filter_map(|item| match item {
        syn::Item::Enum(item) if item.ident == enum_name => Some(item),
        _ => None,
    });
    let item = enums
        .next()
        .ok_or_else(|| format!("missing enum {enum_name}"))?;
    if enums.next().is_some() {
        return Err(format!("multiple enum {enum_name} declarations"));
    }
    Ok(item
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect())
}

fn client_array_inventory(source: &str, const_name: &str) -> Result<Vec<String>, String> {
    let syntax = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut constants = syntax.items.iter().filter_map(|item| match item {
        syn::Item::Const(item) if item.ident == const_name => Some(item),
        _ => None,
    });
    let item = constants
        .next()
        .ok_or_else(|| format!("missing const {const_name}"))?;
    if constants.next().is_some() {
        return Err(format!("multiple const {const_name} declarations"));
    }
    let syn::Expr::Array(array) = item.expr.as_ref() else {
        return Err(format!("{const_name} must be an array literal"));
    };
    array
        .elems
        .iter()
        .map(|element| match element {
            syn::Expr::Path(path) => path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .ok_or_else(|| format!("{const_name} contains an empty path")),
            _ => Err(format!("{const_name} contains a non-path entry")),
        })
        .collect()
}

fn client_slug_inventory(
    source: &str,
    type_name: &str,
    method_name: &str,
) -> Result<BTreeMap<String, String>, String> {
    let syntax = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut methods = syntax.items.iter().filter_map(|item| {
        let syn::Item::Impl(item) = item else {
            return None;
        };
        let syn::Type::Path(self_type) = item.self_ty.as_ref() else {
            return None;
        };
        if self_type.path.segments.last()?.ident != type_name {
            return None;
        }
        item.items.iter().find_map(|item| match item {
            syn::ImplItem::Fn(function) if function.sig.ident == method_name => Some(function),
            _ => None,
        })
    });
    let method = methods
        .next()
        .ok_or_else(|| format!("missing {type_name}::{method_name}"))?;
    if methods.next().is_some() {
        return Err(format!("multiple {type_name}::{method_name} methods"));
    }
    let match_expression = method
        .block
        .stmts
        .iter()
        .find_map(|statement| match statement {
            syn::Stmt::Expr(syn::Expr::Match(expression), _) => Some(expression),
            _ => None,
        });
    let expression = match_expression
        .ok_or_else(|| format!("{type_name}::{method_name} must use a direct match"))?;
    let mut result = BTreeMap::new();
    for arm in &expression.arms {
        if arm.guard.is_some() {
            return Err(format!("{type_name}::{method_name} has a guarded arm"));
        }
        let syn::Pat::Path(pattern) = &arm.pat else {
            return Err(format!("{type_name}::{method_name} has a non-path arm"));
        };
        let variant = pattern
            .path
            .segments
            .last()
            .ok_or_else(|| format!("{type_name}::{method_name} has an empty pattern"))?
            .ident
            .to_string();
        let syn::Expr::Lit(expression) = arm.body.as_ref() else {
            return Err(format!(
                "{type_name}::{method_name} has a non-literal value"
            ));
        };
        let syn::Lit::Str(slug) = &expression.lit else {
            return Err(format!("{type_name}::{method_name} has a non-string value"));
        };
        if result.insert(variant.clone(), slug.value()).is_some() {
            return Err(format!("{type_name}::{method_name} repeats {variant}"));
        }
    }
    Ok(result)
}

fn client_path_binding_inventory(
    source: &str,
    type_name: &str,
    method_name: &str,
) -> Result<BTreeMap<String, String>, String> {
    let syntax = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut methods = syntax.items.iter().filter_map(|item| {
        let syn::Item::Impl(item) = item else {
            return None;
        };
        let syn::Type::Path(self_type) = item.self_ty.as_ref() else {
            return None;
        };
        if self_type.path.segments.last()?.ident != type_name {
            return None;
        }
        item.items.iter().find_map(|item| match item {
            syn::ImplItem::Fn(function) if function.sig.ident == method_name => Some(function),
            _ => None,
        })
    });
    let method = methods
        .next()
        .ok_or_else(|| format!("missing {type_name}::{method_name}"))?;
    if methods.next().is_some() {
        return Err(format!("multiple {type_name}::{method_name} methods"));
    }
    let expression = method
        .block
        .stmts
        .iter()
        .find_map(|statement| match statement {
            syn::Stmt::Expr(syn::Expr::Match(expression), _) => Some(expression),
            _ => None,
        })
        .ok_or_else(|| format!("{type_name}::{method_name} must use a direct match"))?;
    let mut result = BTreeMap::new();
    for arm in &expression.arms {
        if arm.guard.is_some() {
            return Err(format!("{type_name}::{method_name} has a guarded arm"));
        }
        let syn::Pat::Path(pattern) = &arm.pat else {
            return Err(format!("{type_name}::{method_name} has a non-path arm"));
        };
        let variant = pattern
            .path
            .segments
            .last()
            .ok_or_else(|| format!("{type_name}::{method_name} has an empty pattern"))?
            .ident
            .to_string();
        let syn::Expr::Path(binding) = arm.body.as_ref() else {
            return Err(format!("{type_name}::{method_name} has a non-path value"));
        };
        let binding = binding
            .path
            .segments
            .last()
            .ok_or_else(|| format!("{type_name}::{method_name} has an empty value"))?
            .ident
            .to_string();
        if result.insert(variant.clone(), binding).is_some() {
            return Err(format!("{type_name}::{method_name} repeats {variant}"));
        }
    }
    Ok(result)
}

struct PermissionEntry {
    method: syn::Ident,
    path: syn::LitStr,
}

impl syn::parse::Parse for PermissionEntry {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let method = content.parse()?;
        content.parse::<syn::Token![,]>()?;
        let path = content.parse()?;
        Ok(Self { method, path })
    }
}

struct PermissionEntries(syn::punctuated::Punctuated<PermissionEntry, syn::Token![,]>);

impl syn::parse::Parse for PermissionEntries {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self(syn::punctuated::Punctuated::parse_terminated(input)?))
    }
}

fn permission_inventory(source: &str, const_name: &str) -> Result<Vec<(String, String)>, String> {
    let syntax = syn::parse_file(source).map_err(|error| error.to_string())?;
    let item = syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(item) if item.ident == const_name => Some(item),
            _ => None,
        })
        .ok_or_else(|| format!("missing const {const_name}"))?;
    let syn::Expr::Macro(expression) = item.expr.as_ref() else {
        return Err(format!("{const_name} must be a permissions macro"));
    };
    if !expression.mac.path.is_ident("permissions") {
        return Err(format!("{const_name} must use permissions!"));
    }
    let entries = syn::parse2::<PermissionEntries>(expression.mac.tokens.clone())
        .map_err(|error| error.to_string())?;
    Ok(entries
        .0
        .into_iter()
        .map(|entry| (entry.method.to_string(), entry.path.value()))
        .collect())
}

fn exact_inventory_check(
    id: &str,
    label: &str,
    actual: Result<Vec<String>, String>,
    expected: Vec<String>,
) -> Check {
    let actual = match actual {
        Ok(actual) => actual,
        Err(error) => return blocked(id, &format!("could not parse {label}: {error}")),
    };
    let actual_set = actual.iter().cloned().collect::<BTreeSet<_>>();
    let expected_set = expected.iter().cloned().collect::<BTreeSet<_>>();
    if actual.len() == actual_set.len()
        && expected.len() == expected_set.len()
        && actual_set == expected_set
    {
        ready(
            id,
            &format!("{label} exactly covers {} unique entries", expected.len()),
        )
    } else {
        blocked(
            id,
            &format!("{label} must be one-to-one; expected {expected_set:?}, found {actual:?}"),
        )
    }
}

fn shell_loop_words(source: &str, marker: &str) -> Result<Vec<String>, String> {
    let start = source
        .find(marker)
        .ok_or_else(|| format!("missing shell loop marker {marker}"))?
        + marker.len();
    let remainder = &source[start..];
    let end = remainder
        .find("; do")
        .ok_or_else(|| format!("unterminated shell loop after {marker}"))?;
    Ok(remainder[..end]
        .split_whitespace()
        .map(|word| word.trim_matches(['\\', '\'', '"']))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect())
}

fn capability_groupadd_targets(source: &str) -> Vec<String> {
    let words = source
        .split_whitespace()
        .map(|word| word.trim_matches(['\\', '\'', '"', ';']))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let mut targets = Vec::new();
    for (index, word) in words.iter().enumerate() {
        if *word != "groupadd" {
            continue;
        }
        if let Some(target) = words[index + 1..]
            .iter()
            .take_while(|candidate| !candidate.contains("&&"))
            .find(|candidate| !candidate.starts_with('-'))
        {
            if target.starts_with("goblins-core-") {
                targets.push((*target).to_string());
            }
        }
    }
    targets
}

fn tmpfiles_capability_entries(source: &str) -> Result<Vec<String>, String> {
    let root_entries = source
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("d /run/goblins-os-core "))
        .collect::<Vec<_>>();
    if root_entries != ["d /run/goblins-os-core 0755 root root -"] {
        return Err(format!(
            "capability root must have one exact tmpfiles entry; found {root_entries:?}"
        ));
    }
    let mut slugs = Vec::new();
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("d /run/goblins-os-core/"))
    {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 6 || fields[0] != "d" {
            return Err(format!("malformed capability tmpfiles entry {line:?}"));
        }
        let slug = fields[1]
            .strip_prefix("/run/goblins-os-core/")
            .ok_or_else(|| format!("invalid capability path in {line:?}"))?;
        let expected_group = format!("goblins-core-{slug}");
        if fields[2] != "2750"
            || fields[3] != "goblins-os"
            || fields[4] != expected_group
            || fields[5] != "-"
        {
            return Err(format!("unsafe capability tmpfiles entry {line:?}"));
        }
        slugs.push(slug.to_string());
    }
    Ok(slugs)
}

fn service_unit_values(source: &str) -> BTreeMap<String, Vec<String>> {
    let mut service = false;
    let mut result = BTreeMap::<String, Vec<String>>::new();
    for line in source.lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            service = line == "[Service]";
            continue;
        }
        if !service || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            result
                .entry(key.trim().to_string())
                .or_default()
                .push(value.trim().to_string());
        }
    }
    result
}

fn exact_service_value(values: &BTreeMap<String, Vec<String>>, key: &str, value: &str) -> bool {
    values
        .get(key)
        .is_some_and(|actual| actual.len() == 1 && actual[0] == value)
}

const CORE_SERVICE_READ_WRITE_PATHS: &str = "/run/goblins-os-core /var/lib/goblins-os/installer /var/lib/goblins-os/session /var/lib/goblins-os/policy /var/lib/goblins-os/ai /var/lib/goblins-os/models /var/lib/goblins-os/voice/work /var/lib/goblins-os/secrets/openai /var/lib/goblins-os/apps /var/lib/goblins-os/codex";

/// Keep the core's writable systemd namespace narrow and reviewable. Requiring
/// one canonical directive rejects omissions, additive reset directives, and
/// broad parent paths such as /run, /var/lib/goblins-os, or /.
fn core_service_writable_paths_are_exact(source: &str) -> bool {
    let service = service_unit_values(source);
    let expected_line = format!("ReadWritePaths={CORE_SERVICE_READ_WRITE_PATHS}");
    let read_write_lines = source
        .lines()
        .filter(|line| line.starts_with("ReadWritePaths="))
        .collect::<Vec<_>>();

    exact_service_value(&service, "ProtectSystem", "strict")
        && exact_service_value(&service, "UMask", "0077")
        && exact_service_value(&service, "ReadWritePaths", CORE_SERVICE_READ_WRITE_PATHS)
        && read_write_lines == [expected_line.as_str()]
}

fn core_service_writable_paths_check(path: PathBuf, id: &str) -> Check {
    let source = read_to_string(&path);
    if core_service_writable_paths_are_exact(&source) {
        ready(
            id,
            "core service has one exact narrow writable-path allowlist under ProtectSystem=strict",
        )
    } else {
        blocked(
            id,
            &format!(
                "{} must contain ProtectSystem=strict, UMask=0077, and exactly one canonical ReadWritePaths={}",
                path.display(),
                CORE_SERVICE_READ_WRITE_PATHS
            ),
        )
    }
}

/// Pin the native private-control boundary as a release contract. Desktop
/// processes receive one setgid capability, connect to one fixed AF_UNIX
/// listener before parsing UI state, permanently drop that group, and keep the
/// single descriptor until exit. The dedicated resident service starts and
/// remains in its one narrow primary group. Loopback TCP remains browser-safe
/// and exposes only health plus the exact OAuth callback.
fn core_capability_boundary_checks(root: &Path) -> Vec<Check> {
    let control_plane = root.join("crates/goblins-os-core/src/control_plane.rs");
    let core_main = root.join("crates/goblins-os-core/src/main.rs");
    let client = root.join("crates/goblins-os-core-client/src/lib.rs");
    let client_manifest = root.join("crates/goblins-os-core-client/Cargo.toml");
    let container = root.join("os/bootc/Containerfile");
    let tmpfiles = root.join("os/tmpfiles/goblins-os-core.conf");

    let mut checks = vec![
        file_check(root, "crates/goblins-os-core-client/Cargo.toml"),
        file_check(root, "crates/goblins-os-core-client/src/lib.rs"),
        file_check(root, "crates/goblins-os-core/src/control_plane.rs"),
        file_check(root, "os/tmpfiles/goblins-os-core.conf"),
        contains_check(
            control_plane.clone(),
            "core-capability-root-is-fixed",
            "const PRODUCTION_ROOT: &str = \"/run/goblins-os-core\";",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-directories-require-2750",
            "const REQUIRED_DIRECTORY_MODE: u32 = 0o2750;",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-sockets-require-0660",
            "const REQUIRED_SOCKET_MODE: u32 = 0o660;",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-server-reads-kernel-peer-credentials",
            "libc::SO_PEERCRED",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-server-reads-kernel-peer-groups",
            "libc::SO_PEERGROUPS",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-server-authorizes-every-accepted-stream",
            "axum::serve(CapabilityListener::new(client, group_id, listener), router)",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-server-allows-only-primary-or-supplementary-group",
            "self.primary_group_id == required_group_id",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-server-tests-no-uid-bypass",
            "fn peer_group_authorization_never_bypasses_for_shared_or_root_uid()",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-all-sockets-bind-before-serving",
            "fn bind_production_sockets()",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-groups-must-be-unique",
            "unique_groups.len() != sockets.len()",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-directory-owner-is-core-euid",
            "metadata.uid() != effective_uid()",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-stale-socket-replacement-is-validated",
            "fn remove_stale_socket",
        ),
        contains_check(
            control_plane.clone(),
            "core-capability-routes-are-exact-method-and-path-pairs",
            "permission.method == method.as_str() && permission.path == path",
        ),
        contains_check(
            control_plane.clone(),
            "core-tcp-surface-is-get-only",
            "request.method() == Method::GET",
        ),
        contains_check(
            control_plane.clone(),
            "core-tcp-surface-is-health-and-oauth-only",
            "matches!(request.uri().path(), \"/health\" | \"/v1/auth/openai/callback\")",
        ),
        contains_check(
            core_main.clone(),
            "core-tcp-router-applies-exact-surface-filter",
            "control_plane::tcp_surface_router",
        ),
        contains_check(
            core_main.clone(),
            "core-private-router-serves-capability-sockets",
            "control_plane::serve(listener, tcp_router(), private_router(), shutdown_signal())",
        ),
        absent_check(
            client_manifest.clone(),
            "core-client-test-transport-is-not-default",
            "default = [\"test-transport\"]",
        ),
        contains_check(
            client_manifest,
            "core-client-default-feature-set-is-empty",
            "default = []",
        ),
        contains_check(
            client.clone(),
            "core-client-socket-path-is-fixed",
            "PathBuf::from(\"/run/goblins-os-core\")",
        ),
        ordered_contains_check(
            client.clone(),
            "core-client-sanitizes-before-linux-initialization",
            "sanitize_environment();",
            "initialize_linux(kind)",
        ),
        contains_check(
            client.clone(),
            "core-client-clears-all-goblins-core-overrides",
            "name.starts_with(\"GOBLINS_OS_CORE_\")",
        ),
        contains_check(
            client.clone(),
            "core-client-clears-all-legacy-core-overrides",
            "name.starts_with(\"OPENAI_OS_CORE_\")",
        ),
        ordered_contains_check(
            client.clone(),
            "core-client-disables-dumps-before-socket-retry",
            "disable_dumpability().map_err(Error::Initialization)?;",
            "connect_initial_socket(&socket_path)",
        ),
        contains_check(
            client.clone(),
            "core-client-uses-unix-domain-sockets",
            "Socket::new(Domain::UNIX, Type::STREAM, None)",
        ),
        contains_check(
            client.clone(),
            "core-client-descriptor-is-close-on-exec",
            "socket.set_cloexec(true)?;",
        ),
        contains_check(
            client.clone(),
            "core-client-desktop-payload-root-is-fixed",
            "const DESKTOP_PAYLOAD_ROOT: &str = \"/usr/libexec/goblins-os/ui\";",
        ),
        contains_check(
            client.clone(),
            "core-client-transfer-descriptor-is-fixed",
            "const TRANSFERRED_CAPABILITY_FD: libc::c_int = 3;",
        ),
        contains_check(
            client.clone(),
            "core-client-reexec-closes-unrelated-descriptors",
            "libc::CLOSE_RANGE_CLOEXEC",
        ),
        contains_check(
            client.clone(),
            "core-client-payload-rejects-at-secure",
            "desktop payload must start from a non-privileged exec",
        ),
        contains_check(
            client.clone(),
            "core-client-payload-verifies-fixed-socket-peer-path",
            "peer.as_pathname() != Some(socket_path)",
        ),
        contains_check(
            root.join("crates/goblins-os-core-client/tests/native_capability.rs"),
            "core-client-native-test-proves-at-secure-cleared",
            "regular desktop payload retained AT_SECURE",
        ),
        contains_check(
            client.clone(),
            "core-client-verifies-peer-credentials",
            "libc::SO_PEERCRED",
        ),
        ordered_contains_check(
            client.clone(),
            "core-client-desktop-only-permanently-drops-effective-and-saved-group",
            "if !kind.requires_no_new_privs() {",
            "libc::setresgid(real_gid, real_gid, real_gid)",
        ),
        contains_check(
            client.clone(),
            "core-client-closes-capability-in-forked-child",
            "libc::pthread_atfork(None, None, Some(close_core_fd_in_child))",
        ),
        contains_check(
            client.clone(),
            "core-client-marks-broken-connection-terminal",
            "state.broken = true;",
        ),
        contains_check(
            client.clone(),
            "core-client-tests-no-reconnect-contract",
            "fn broken_connection_never_reconnects()",
        ),
        contains_check(
            client.clone(),
            "core-client-enforces-nondumpable-process",
            "libc::PR_SET_DUMPABLE, 0",
        ),
        contains_check(
            client.clone(),
            "core-client-no-new-privileges-is-resident-only",
            "matches!(self, Self::Resident)",
        ),
        contains_check(
            client.clone(),
            "core-client-resident-enables-no-new-privileges",
            "libc::PR_SET_NO_NEW_PRIVS, 1",
        ),
        contains_check(
            container.clone(),
            "container-creates-distinct-core-capability-groups",
            "groupadd --system \"goblins-core-${client}\"",
        ),
        contains_check(
            container.clone(),
            "container-installs-desktop-capabilities-as-setgid",
            "chmod 2755 \"/usr/libexec/goblins-os/${binary}\"",
        ),
        contains_check(
            container.clone(),
            "container-installs-root-owned-regular-desktop-payloads",
            "install -m 0755 -o root -g root \"/usr/libexec/goblins-os/${binary}\" \"/usr/libexec/goblins-os/ui/${binary}\"",
        ),
        contains_check(
            container.clone(),
            "container-verifies-regular-desktop-payload-owner-and-mode",
            "root:root:755",
        ),
        contains_check(
            container.clone(),
            "container-refuses-desktop-user-capability-membership",
            "! id -nG goblin | tr ' ' '\\n' | grep -Eq '^goblins-core-'",
        ),
        contains_check(
            container.clone(),
            "container-resident-binary-is-not-setgid",
            "root:goblins-core-resident:755",
        ),
        contains_check(
            container.clone(),
            "container-provides-release-proof-group-executor",
            "command -v setpriv",
        ),
        contains_check(
            root.join("os/systemd/goblins-os-resident.service"),
            "resident-runs-as-dedicated-capability-user",
            "User=goblins-resident",
        ),
        contains_check(
            root.join("os/systemd/goblins-os-resident.service"),
            "resident-runs-with-exact-capability-group",
            "Group=goblins-core-resident",
        ),
        absent_check(
            root.join("os/systemd/goblins-os-resident.service"),
            "resident-has-no-ambient-capability-escalation",
            "AmbientCapabilities=",
        ),
        absent_check(
            root.join("os/systemd/goblins-os-resident.service"),
            "resident-has-no-capability-bounding-override",
            "CapabilityBoundingSet=",
        ),
        absent_check(
            root.join("os/systemd/goblins-os-resident.service"),
            "resident-does-not-request-cap-setgid",
            "CAP_SETGID",
        ),
        contains_check(
            root.join("os/systemd/goblins-os-core.service"),
            "core-loads-provider-secrets-as-systemd-credential",
            "LoadCredential=openai-secrets.env:/etc/goblins-os/openai-secrets.env",
        ),
        absent_check(
            root.join("os/systemd/goblins-os-core.service"),
            "core-does-not-source-provider-secrets-as-environment",
            "EnvironmentFile=-/etc/goblins-os/openai-secrets.env",
        ),
        core_service_writable_paths_check(
            root.join("os/systemd/goblins-os-core.service"),
            "core-service-writable-paths-are-exact",
        ),
    ];

    for (id, script) in [
        ("render-screens", "os/bootc/render-screens.sh"),
        ("render-desktop", "os/bootc/render-desktop.sh"),
        ("installed-selftest", "os/bootc/run-selftest.sh"),
        (
            "runtime-model-gate",
            "os/runtime-gate/build-an-app-live-model.sh",
        ),
        (
            "firstboot-unlock",
            "os/hardware-gate/capture-harness/firstboot-unlock.sh",
        ),
        (
            "hardware-core-proof",
            "os/hardware-gate/capture-harness/core-proof-operation.sh",
        ),
    ] {
        checks.push(contains_check(
            root.join(script),
            &format!("{id}-uses-exact-release-proof-peer-group"),
            "setpriv --regid=goblins-core-release-proof --clear-groups --",
        ));
    }

    for client_spec in NATIVE_CORE_CLIENTS {
        checks.push(first_main_initialization_check(
            root.join(client_spec.entrypoint),
            client_spec.slug,
            client_spec.kind,
        ));
    }

    for crate_dir in NATIVE_CORE_CLIENT_CRATES {
        let crate_path = root.join(crate_dir);
        let manifest = crate_path.join("Cargo.toml");
        let id_suffix = crate_dir
            .trim_start_matches("crates/")
            .replace(['/', '.'], "-");
        checks.push(client_manifest_transport_check(manifest, &id_suffix));

        let mut rust_files = Vec::new();
        match collect_rust_files(&crate_path.join("src"), &mut rust_files) {
            Ok(()) => {
                rust_files.sort();
                if rust_files.is_empty() {
                    checks.push(blocked(
                        &format!("core-capability-{id_suffix}-source-inventory"),
                        &format!("{} contains no Rust source files", crate_path.display()),
                    ));
                }
                for path in rust_files {
                    let relative = path.strip_prefix(root).unwrap_or(&path);
                    let source_id = stable_id(&relative.to_string_lossy());
                    checks.push(client_rust_transport_check(path, &source_id));
                }
            }
            Err(error) => checks.push(blocked(
                &format!("core-capability-{id_suffix}-source-inventory"),
                &error,
            )),
        }
    }

    let expected_client_kinds = NATIVE_CORE_CLIENTS
        .iter()
        .map(|client_spec| client_spec.kind.to_string())
        .collect::<Vec<_>>();
    let mut expected_server_kinds = expected_client_kinds.clone();
    expected_server_kinds.push("ReleaseProof".to_string());
    checks.push(exact_inventory_check(
        "core-client-kind-inventory-is-one-to-one",
        "shared client ClientKind enum",
        enum_variant_inventory(&read_to_string(&client), "ClientKind"),
        expected_client_kinds.clone(),
    ));
    checks.push(exact_inventory_check(
        "core-server-capability-kind-inventory-is-one-to-one",
        "core control-plane ClientKind enum",
        enum_variant_inventory(&read_to_string(&control_plane), "ClientKind"),
        expected_server_kinds.clone(),
    ));
    checks.push(exact_inventory_check(
        "core-server-bound-socket-inventory-is-one-to-one",
        "core control-plane ALL_CLIENTS array",
        client_array_inventory(&read_to_string(&control_plane), "ALL_CLIENTS"),
        expected_server_kinds,
    ));

    let expected_client_slugs = NATIVE_CORE_CLIENTS
        .iter()
        .map(|client_spec| (client_spec.kind.to_string(), client_spec.slug.to_string()))
        .collect::<BTreeMap<_, _>>();
    let mut expected_server_slugs = expected_client_slugs.clone();
    expected_server_slugs.insert("ReleaseProof".to_string(), "release-proof".to_string());
    for (id, label, actual, expected) in [
        (
            "core-client-kind-to-socket-slug-map-is-exact",
            "shared client ClientKind::slug",
            client_slug_inventory(&read_to_string(&client), "ClientKind", "slug"),
            expected_client_slugs,
        ),
        (
            "core-server-kind-to-socket-slug-map-is-exact",
            "core control-plane ClientKind::id",
            client_slug_inventory(&read_to_string(&control_plane), "ClientKind", "id"),
            expected_server_slugs,
        ),
    ] {
        match actual {
            Ok(actual) if actual == expected => checks.push(ready(
                id,
                &format!("{label} exactly maps {} capabilities", expected.len()),
            )),
            Ok(actual) => checks.push(blocked(
                id,
                &format!("{label} must be one-to-one; expected {expected:?}, found {actual:?}"),
            )),
            Err(error) => checks.push(blocked(id, &format!("could not parse {label}: {error}"))),
        }
    }

    let expected_socket_slugs = NATIVE_CORE_CLIENTS
        .iter()
        .map(|client_spec| client_spec.slug.to_string())
        .chain(std::iter::once("release-proof".to_string()))
        .collect::<Vec<_>>();
    checks.push(exact_inventory_check(
        "core-tmpfiles-capability-directory-inventory-is-one-to-one",
        "tmpfiles capability directory inventory",
        tmpfiles_capability_entries(&read_to_string(&tmpfiles)),
        expected_socket_slugs.clone(),
    ));
    checks.push(exact_inventory_check(
        "container-core-capability-group-inventory-is-one-to-one",
        "Containerfile capability group loop",
        shell_loop_words(&read_to_string(&container), "RUN for client in"),
        expected_socket_slugs,
    ));
    checks.push(exact_inventory_check(
        "container-creates-no-capability-groups-outside-canonical-loop",
        "Containerfile capability groupadd targets",
        Ok(capability_groupadd_targets(&read_to_string(&container))),
        vec!["goblins-core-${client}".to_string()],
    ));
    let expected_setgid_bindings = NATIVE_CORE_CLIENTS
        .iter()
        .filter(|client_spec| client_spec.setgid)
        .map(|client_spec| format!("{}:{}", client_spec.slug, client_spec.binary))
        .collect::<Vec<_>>();
    checks.push(exact_inventory_check(
        "container-setgid-capability-binding-inventory-is-one-to-one",
        "Containerfile setgid binding loop",
        shell_loop_words(&read_to_string(&container), "for binding in"),
        expected_setgid_bindings,
    ));
    let container_text = read_to_string(&container);
    if container_text
        .matches("chmod 2755 \"/usr/libexec/goblins-os/${binary}\"")
        .count()
        == 1
        && container_text
            .matches("chown \"root:goblins-core-${client}\" \"/usr/libexec/goblins-os/${binary}\"")
            .count()
            == 1
        && container_text
            .matches("install -m 0755 -o root -g root \"/usr/libexec/goblins-os/${binary}\" \"/usr/libexec/goblins-os/ui/${binary}\"")
            .count()
            == 1
    {
        checks.push(ready(
            "container-setgid-mode-and-owner-apply-only-to-canonical-binding-loop",
            "Containerfile has one canonical setgid entrypoint and regular-payload implementation for the exact binding inventory",
        ));
    } else {
        checks.push(blocked(
            "container-setgid-mode-and-owner-apply-only-to-canonical-binding-loop",
            "Containerfile must have exactly one canonical setgid entrypoint and regular-payload implementation",
        ));
    }

    let control_plane_source = read_to_string(&control_plane);
    match client_path_binding_inventory(&control_plane_source, "ClientKind", "permissions") {
        Ok(bindings)
            if bindings.get("Resident").map(String::as_str) == Some("RESIDENT_PERMISSIONS") =>
        {
            checks.push(ready(
                "resident-kind-is-bound-to-resident-permission-manifest",
                "ClientKind::Resident resolves to RESIDENT_PERMISSIONS",
            ));
        }
        Ok(bindings) => checks.push(blocked(
            "resident-kind-is-bound-to-resident-permission-manifest",
            &format!(
                "ClientKind::Resident must resolve to RESIDENT_PERMISSIONS; found {:?}",
                bindings.get("Resident")
            ),
        )),
        Err(error) => checks.push(blocked(
            "resident-kind-is-bound-to-resident-permission-manifest",
            &format!("could not parse ClientKind::permissions: {error}"),
        )),
    }
    let resident_permissions = permission_inventory(&control_plane_source, "RESIDENT_PERMISSIONS");
    match resident_permissions {
        Ok(actual) if actual == [("POST".to_string(), "/v1/codex/resident".to_string())] => {
            checks.push(ready(
                "resident-capability-permission-is-one-exact-post-route",
                "Resident permits only POST /v1/codex/resident",
            ));
        }
        Ok(actual) => checks.push(blocked(
            "resident-capability-permission-is-one-exact-post-route",
            &format!("Resident must permit only POST /v1/codex/resident; found {actual:?}"),
        )),
        Err(error) => checks.push(blocked(
            "resident-capability-permission-is-one-exact-post-route",
            &format!("could not parse Resident permissions: {error}"),
        )),
    }

    let container_source = container_text;
    let normalized_container = container_source
        .split_whitespace()
        .filter(|word| *word != "\\")
        .collect::<Vec<_>>()
        .join(" ");
    let resident_account = "useradd --system --no-user-group --gid goblins-core-resident --home-dir /var/lib/goblins-os/resident --shell /usr/sbin/nologin goblins-resident";
    if normalized_container.matches(resident_account).count() == 1 {
        checks.push(ready(
            "resident-account-has-one-dedicated-nologin-primary-capability-group",
            "Containerfile creates one nologin goblins-resident account with goblins-core-resident as its primary group",
        ));
    } else {
        checks.push(blocked(
            "resident-account-has-one-dedicated-nologin-primary-capability-group",
            "Containerfile must create exactly one nologin goblins-resident account whose primary group is goblins-core-resident",
        ));
    }
    let resident_state_layout = "install -d -m 0710 -o goblins-os -g goblins-core-resident /var/lib/goblins-os && install -d -m 0750 -o goblins-resident -g goblins-core-resident /var/lib/goblins-os/resident";
    if normalized_container.matches(resident_state_layout).count() == 1 {
        checks.push(ready(
            "resident-state-layout-has-narrow-parent-traverse-and-private-leaf",
            "Containerfile grants only the Resident capability group traversal to a private Resident-owned state directory",
        ));
    } else {
        checks.push(blocked(
            "resident-state-layout-has-narrow-parent-traverse-and-private-leaf",
            "Containerfile must install /var/lib/goblins-os as goblins-os:goblins-core-resident mode 0710 and its resident leaf as goblins-resident:goblins-core-resident mode 0750",
        ));
    }
    if !normalized_container.split("&&").any(|command| {
        command.contains("goblins-resident")
            && ["usermod", "gpasswd", "adduser"]
                .iter()
                .any(|membership_tool| command.contains(membership_tool))
    }) {
        checks.push(ready(
            "resident-account-has-no-supplementary-core-capability-group",
            "Containerfile never adds goblins-resident to a supplementary group",
        ));
    } else {
        checks.push(blocked(
            "resident-account-has-no-supplementary-core-capability-group",
            "goblins-resident must not receive supplementary group membership",
        ));
    }
    for (id, needle) in [
        (
            "resident-binary-owner-and-mode-are-exact",
            "test \"$(stat -c '%U:%G:%a' /usr/libexec/goblins-os/goblins-os-resident)\" = \"root:goblins-core-resident:755\"",
        ),
        (
            "resident-binary-mode-is-explicitly-not-setgid",
            "chmod 0755 /usr/libexec/goblins-os/goblins-os-resident",
        ),
    ] {
        if normalized_container.contains(needle) {
            checks.push(ready(id, &format!("Containerfile contains {needle}")));
        } else {
            checks.push(blocked(id, &format!("Containerfile missing {needle}")));
        }
    }

    let resident_unit_path = root.join("os/systemd/goblins-os-resident.service");
    let resident_unit_source = read_to_string(&resident_unit_path);
    let resident_service = service_unit_values(&resident_unit_source);
    let required_resident_service_values = [
        ("Type", "simple"),
        ("User", "goblins-resident"),
        ("Group", "goblins-core-resident"),
        ("ExecStart", "/usr/libexec/goblins-os/goblins-os-resident"),
        ("UMask", "0027"),
        ("NoNewPrivileges", "yes"),
        ("PrivateTmp", "yes"),
        ("ProtectSystem", "strict"),
        ("ProtectHome", "yes"),
        ("ReadWritePaths", "/var/lib/goblins-os/resident"),
        ("RestrictSUIDSGID", "yes"),
        ("LockPersonality", "yes"),
        ("MemoryDenyWriteExecute", "yes"),
        ("SystemCallArchitectures", "native"),
    ];
    let invalid_service_values = required_resident_service_values
        .iter()
        .filter(|(key, value)| !exact_service_value(&resident_service, key, value))
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    let forbidden_capability_keys = [
        "AmbientCapabilities",
        "CapabilityBoundingSet",
        "SupplementaryGroups",
    ]
    .into_iter()
    .filter(|key| resident_service.contains_key(*key))
    .collect::<Vec<_>>();
    if invalid_service_values.is_empty()
        && forbidden_capability_keys.is_empty()
        && !resident_unit_source.contains("CAP_SETGID")
    {
        checks.push(ready(
            "resident-systemd-sandbox-and-identity-are-exact",
            "resident service keeps its exact dedicated identity and full NNP/seccomp hardening without transient capabilities",
        ));
    } else {
        checks.push(blocked(
            "resident-systemd-sandbox-and-identity-are-exact",
            &format!(
                "resident service drift: invalid required values {invalid_service_values:?}; forbidden capability keys {forbidden_capability_keys:?}; CAP_SETGID present={} ",
                resident_unit_source.contains("CAP_SETGID")
            ),
        ));
    }

    checks
}

fn systemd_hardening_checks(root: &Path) -> Vec<Check> {
    let mut checks = Vec::new();
    for unit in SYSTEMD_UNITS {
        let relative = format!("os/systemd/{unit}");
        let path = root.join(&relative);
        let text = read_to_string(&path);
        for needle in [
            "NoNewPrivileges=yes",
            "ProtectSystem=strict",
            "PrivateTmp=yes",
            "SystemCallArchitectures=native",
        ] {
            let id = format!("systemd-{unit}-{needle}");
            if text.contains(needle) {
                checks.push(ready(&id, &format!("{relative} contains {needle}")));
            } else {
                checks.push(blocked(&id, &format!("{relative} missing {needle}")));
            }
        }
    }
    checks
}

fn bootc_install_config_check(root: &Path) -> Check {
    contains_check(
        root.join("os/bootc-install/00-goblins-os.toml"),
        "bootc-install-root-xfs",
        "type = \"xfs\"",
    )
}

fn installer_readiness_checks(root: &Path) -> Vec<Check> {
    let mut checks = vec![
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-leaves-destination-interactive",
            "Installation Destination",
        ),
        kickstart_command_absent_check(root, "clearpart"),
        kickstart_command_absent_check(root, "autopart"),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-no-clearpart",
            "no clearpart",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-no-autopart",
            "autopart here",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-dual-boot-manual-storage",
            "manual storage for dual boot with Windows, macOS, Linux, or another OS",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-dual-boot-reclaim-space",
            "Custom/manual storage or Reclaim Space",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-full-storage-button",
            "Open advanced storage",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-preserves-existing-os-partitions",
            "preserve existing Windows, macOS/APFS, Linux,\n# other OS, recovery, and EFI partitions",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-efi-bootloader-summary",
            "Bootloader, EFI System Partition, and root formatting",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-required-final-summary-target",
            "target disk\n# or free-space assignment",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-required-final-summary-preserves",
            "preserved Windows, macOS/APFS, Linux, other OS, recovery, EFI, vendor, and data",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-required-final-summary-bootloader",
            "bootloader/EFI target",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-dual-boot-installer-path",
            "Dual boot with Windows, macOS,\nLinux, or another OS uses advanced storage",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-dual-boot-reclaim-space-path",
            "Custom/manual storage or Reclaim\nSpace",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-dual-boot-free-space-target",
            "unallocated free space or a dedicated disk",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-dual-boot-apfs-preservation",
            "Windows, macOS/APFS, Linux, other OS, recovery, and EFI partitions",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-dual-boot-assistant",
            "Dual-boot assistant",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-dual-boot-safe-route",
            "Install beside an existing OS",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-full-storage-button",
            "Open advanced storage",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-dual-boot-desktop-entry",
            "Install Goblins OS Beside Another OS",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-dual-boot-decision-map",
            "Dual-boot decision map",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-pre-write-plan",
            "Before writing to disk",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-blank-disk-simple-flow",
            "blank-disk, whole-disk erase only",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-boot-step-full-storage-installer",
            "choose the disk/storage layout in advanced storage",
        ),
        absent_check(
            root.join("SHIP.md"),
            "ship-documents-no-anaconda-dual-boot-path",
            "uses Anaconda Installation Destination/manual storage",
        ),
        absent_check(
            root.join("SHIP.md"),
            "ship-documents-no-anaconda-manual-storage-handoff",
            "to Anaconda manual storage",
        ),
        absent_check(
            root.join("SHIP.md"),
            "ship-documents-no-anaconda-custom-storage",
            "stay in Anaconda manual storage",
        ),
        absent_check(
            root.join("SHIP.md"),
            "ship-documents-no-anaconda-boot-step",
            "choose the disk/storage layout in Anaconda",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-full-storage-interactive-destination",
            "LEFT to advanced storage and its Installation\n# Destination",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-routes-existing-systems-to-manual-storage",
            "routes the user back here so advanced\n# storage can",
        ),
        contains_check(
            root.join("os/iso/config.toml"),
            "iso-installer-storage-engine-first-party-copy",
            "handled by advanced\n# storage and the image builder",
        ),
        absent_check(
            root.join("os/iso/config.toml"),
            "iso-installer-no-visible-in-anaconda-copy",
            "visible in Anaconda",
        ),
        absent_check(
            root.join("os/iso/config.toml"),
            "iso-installer-no-anaconda-storage-path-copy",
            "Anaconda storage path",
        ),
        absent_check(
            root.join("os/iso/config.toml"),
            "iso-installer-no-anaconda-routing-copy",
            "routes the user back here so Anaconda can",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-guidance",
            "dual_boot_guidance",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-preservation",
            "dual_boot_preservation",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-handoff",
            "dual_boot_handoff",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-safe-route-contract",
            "dual_boot_safe_route",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-safe-route-type",
            "DualBootSafeRoute",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-safe-route-title",
            "Install beside an existing OS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-safe-route-desktop-entry",
            "Install Goblins OS Beside Another OS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-safe-route-final-review",
            "every filesystem that will be formatted",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-assistant-contract",
            "dual_boot_choices",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-readiness-contract",
            "dual_boot_readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-readiness-type",
            "DualBootReadinessItem",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-choice-type",
            "DualBootChoice",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-preflight",
            "dual_boot_preflight",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-backup-first",
            "Back up first",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-apfs-preservation",
            "macOS/APFS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-reclaim-space",
            "Custom/manual storage or Reclaim Space",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-keep-current-os",
            "Keep your current OS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-only-free-space",
            "select only unallocated free space",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-route-target-free-space",
            "Choose only unallocated free space",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-data-partitions",
            "data partitions preserved",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-guide-contract",
            "dual_boot_guide",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-decision-map-contract",
            "dual_boot_decision_map",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-decision-type",
            "DualBootDecision",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-full-storage-installer-contract",
            "full_storage_installer",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-full-storage-installer-command",
            "/usr/libexec/goblins-os/goblins-os-full-installer",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-full-storage-installer-desktop",
            "org.goblins.OS.FullInstaller.desktop",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-full-storage-manual-storage-copy",
            "advanced storage",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-full-storage-engine-copy",
            "installer",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-disk-installer-copy",
            "Goblins OS disk installer",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-prepared-copy-no-disk-changed",
            "Install plan prepared. No disk has been changed",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-ready-disk-install-copy",
            "Ready for guarded disk install preparation",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-command-model-is-user-facing",
            "Goblins OS disk install",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-quick-start-contract",
            "dual_boot_quick_start",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-quick-start-heading",
            "Install beside another OS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-quick-start-preserve-format-bootloader",
            "Confirm preserve, format, and bootloader",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-quick-start-test-boot-paths",
            "Test every boot path",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-anaconda",
            "Anaconda",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-bootc-anaconda",
            "bootc/Anaconda",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-bootc-installer",
            "bootc installer",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-guarded-bootc-prep",
            "Ready for guarded bootc install preparation",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-install-command-prepared",
            "Install command prepared",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-bootc-install-started",
            "bootc install was started by the Goblins OS core",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-core-spawn-bootc",
            "core may spawn bootc install",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-could-not-spawn-bootc",
            "could not spawn bootc install",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-iso-installation-destination",
            "ISO Installation Destination",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-iso-manual-storage",
            "ISO manual storage",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-manual-storage-from-iso",
            "manual storage from the ISO",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-copy-no-raw-use-installation-destination",
            "Use Installation Destination",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-decision-windows",
            "Windows beside Goblins OS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-decision-macos",
            "macOS beside Goblins OS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-decision-linux",
            "Linux beside Goblins OS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-decision-separate-disk",
            "Separate disk",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-choice-windows",
            "Keep Windows",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-choice-bitlocker",
            "suspend BitLocker",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-choice-macos",
            "Keep macOS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-choice-linux",
            "Keep Linux",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-choice-other-os-data",
            "Keep another OS or data",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-choice-dedicated-disk",
            "Use a dedicated disk",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-readiness-windows",
            "Windows readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-readiness-macos",
            "macOS readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-readiness-linux",
            "Linux readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-readiness-other-os",
            "Other OS or data readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-dual-boot-readiness-dedicated-disk",
            "Dedicated disk readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-windows-disk-management",
            "Disk Management",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-macos-disk-utility",
            "Disk Utility",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-bootloader-efi-step",
            "Bootloader and EFI",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-startup-menu-step",
            "Startup menu",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-firmware-boot-picker-guidance",
            "firmware startup menu or boot picker",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-final-storage-review-step",
            "Final storage review",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-required-final-summary-step",
            "Required final summary",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-required-final-summary-target",
            "target disk or free-space assignment",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-required-final-summary-formats",
            "every filesystem that will be formatted",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-required-final-summary-preserves",
            "every Windows, macOS/APFS, Linux, other OS, recovery, EFI, vendor, and data partition that will be preserved",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-bootloader-guidance",
            "Bootloader and EFI setup",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-simple-install-scope",
            "simple_install_scope",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-simple-install-blank-disk",
            "blank internal disk",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-simple-install-root-format",
            "formats the new Goblins OS root filesystem",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-bootloader-recovery",
            "bootloader_recovery",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-firmware-boot-options",
            "firmware boot options",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-storage-review-checklist",
            "storage_review_checklist",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-storage-review-item",
            "StorageReviewItem",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-recommended-path-options",
            "install_path_options",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-pre-write-plan-contract",
            "pre_write_install_plan",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-pre-write-plan-type",
            "InstallPlanItem",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-pre-write-plan-gpt",
            "fresh GPT layout",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-pre-write-plan-bootloader-efi",
            "bootloader/EFI target",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-pre-write-plan-root-xfs",
            "xfs root",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-pre-write-plan-custom-formatting",
            "TPM2 LUKS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-keep-current-os-option",
            "Keep my current OS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-replace-blank-disk-option",
            "Replace one blank disk",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-advanced-storage-option",
            "Advanced storage",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-formatting-guidance",
            "formatting_guidance",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-advanced-storage-guidance",
            "advanced_storage_guidance",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-custom-formatting-options",
            "ext4, btrfs, separate /home",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-simple-api-xfs-only-contract",
            "simple_install_filesystem",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-simple-api-rejects-custom-filesystems",
            "only writes an xfs root",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-simple-api-direct-block-only-contract",
            "simple_install_block_setup",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-simple-api-routes-tpm2-luks-to-full-storage",
            "TPM2 LUKS belongs in advanced storage",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-simple-api-wipe-guard-contract",
            "simple_install_wipe",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-simple-api-rejects-non-wipe-layouts",
            "always uses the wipe guard",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-policy-tpm2-luks-guidance",
            "TPM2 LUKS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-partition-reporting",
            "partitions: Vec<String>",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-existing-system-reporting",
            "existing_systems: Vec<ExistingSystem>",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-existing-os-blocks-simple-flow",
            "the simple flow only installs to a blank disk",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-ineligible-detail-names-simple-erase-flow",
            "The simple erase flow will not install",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-ineligible-detail-opens-full-storage",
            "open advanced storage",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-ineligible-detail-uses-only-free-space",
            "select only unallocated free space",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-windows-partitions",
            "\"Windows\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-macos-apfs-partitions",
            "\"macOS/APFS\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-linux-partitions",
            "\"Linux\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-efi-partitions",
            "\"EFI boot\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-bitlocker",
            "bitlocker",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-microsoft-reserved",
            "e3c9e316-0b5c-4db8-817d-f92df00215ae",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-apple-hfs-guid",
            "48465300-0000-11aa-aa11-00306543ecac",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-linux-f2fs",
            "f2fs",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-detects-linux-bcachefs",
            "bcachefs",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-tests-dual-boot-classification",
            "scans_sys_block_and_routes_existing_operating_systems_to_manual_storage",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-tests-windows-ntfs",
            "TYPE=ntfs",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-tests-macos-apfs",
            "TYPE=apfs",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-tests-linux-luks",
            "TYPE=crypto_LUKS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "install-target-tests-unknown-data-preservation",
            "TYPE=zfs_member",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-keep-other-os-guidance",
            "Keep an existing OS",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-choose-install-path",
            "Choose install path",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-full-storage-first-choice",
            "Keeping another OS or data?",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-starts-with-full-storage",
            "start with advanced storage",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-choose-os-first",
            "choose the OS you are keeping first",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-unsure-preserve-current-os",
            "Unsure? Keep your current OS",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-before-disk-writes",
            "before any disk writes",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-blank-disk-path-choice",
            "Replace one blank disk",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-title-case-install-step",
            "Step · Install",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-title-case-confirm-step",
            "Step 3 of 3 · Confirm",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-title-case-confirmation-heading",
            "Required Confirmation",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "installer-ui-kicker-no-uppercase-transform",
            ".gos-onboarding-kicker",
        ),
        absent_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "installer-ui-kicker-no-wide-tracking",
            "letter-spacing: 2.2px",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-no-uppercase-step-labels",
            "STEP ·",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-no-uppercase-final-step",
            "FINAL STEP",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-no-uppercase-required-confirmation",
            "REQUIRED CONFIRMATION",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-install-path-options-summary",
            "install_path_options_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-recommended-install-paths",
            "Recommended install paths",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-install-path-options-renderer",
            "append_install_path_options",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-pre-write-plan-summary",
            "pre_write_install_plan_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-pre-write-plan-renderer",
            "append_pre_write_install_plan",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-pre-write-plan-label",
            "Before writing to disk",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-pre-write-plan-simple-dual-boot-boundary",
            "dual boot and custom formatting stay in advanced storage",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-other-os-preservation",
            "Windows, macOS, Linux, another OS, recovery, and EFI partitions",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-preflight",
            "Before dual boot",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-best-path-for-dual-boot",
            "Best dual-boot path",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-reclaim-space",
            "Custom/manual storage or Reclaim Space",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-handoff-contract",
            "dual_boot_handoff",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-handoff-row",
            "Keep your current OS",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-safe-route-summary",
            "dual_boot_safe_route_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-safe-route-renderer",
            "append_dual_boot_safe_route",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-safe-route-title",
            "Install beside an existing OS",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-safe-route-launch-error",
            "installer_dual_boot_safe_route_launch_error",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-assistant-summary",
            "dual_boot_choices_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-readiness-summary",
            "dual_boot_readiness_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-readiness-renderer",
            "append_dual_boot_readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-readiness-label",
            "Dual-boot readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-readiness-checklist",
            "Use this checklist before writing storage changes",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-quick-start-renderer",
            "append_dual_boot_quick_start",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-quick-start-summary",
            "dual_boot_quick_start_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-quick-start-label",
            "Dual-boot quick start",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-quick-start-final-summary",
            "final preserve, format, and bootloader summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-assistant-renderer",
            "append_dual_boot_choices",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-decision-map-renderer",
            "append_dual_boot_decision_map",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-decision-map-summary",
            "dual_boot_decision_map_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-decision-map-label",
            "Dual-boot decision map",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-decision-map-best-for",
            "Best for:",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-full-storage-installer-renderer",
            "append_full_storage_installer_handoff",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-full-storage-installer-button",
            "Open advanced storage",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-full-storage-installer-launcher",
            "launch_full_storage_installer",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-full-storage-installer-no-shell",
            "StorageInstallerCommand",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-detected-systems-action-row",
            "Detected systems are actions",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-launcher",
            "append_dual_boot_launcher",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-install-beside-another-os",
            "Install beside another OS",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-choice-question",
            "What are you keeping?",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-choice-launch-error",
            "installer_dual_boot_choice_launch_error",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "installer-ui-dual-boot-choice-styling",
            ".gos-dual-boot-choice",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-detected-disk-preservation-prompt",
            "Open advanced storage from detected disk",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-detected-disk-preservation-checklist",
            "Preservation checklist:",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-detected-disk-preservation-recovery-keys",
            "Back up and save recovery keys",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-detected-disk-preservation-os-specific-prep",
            "detected_system_preparation_hint",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-detected-disk-preservation-final-boot-check",
            "test every preserved system from the firmware boot picker",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-detected-disk-opens-full-storage",
            "installer_detected_disk_full_storage_launch_error",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-preservation-row-stays-off-erase-confirmation",
            "row.set_sensitive(target.eligible || preservation_handoff)",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-app-copy-is-product-facing",
            "Goblins OS builds it with your selected engine",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-app-grant-is-explicit",
            "Allow and build first app",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-app-grants-policy-before-build",
            "grant_setup_app_builder_permission",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-app-shows-live-progress",
            "Building with the selected engine…",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-boot-discloses-codex-egress",
            "Requests leave this device for OpenAI",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-boot-defaults-to-device-engine",
            "Start with GPT-OSS on this device",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-boot-codex-has-accessible-label",
            "Use OpenAI account through Codex",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-app-build-has-accessible-description",
            "Allow app creation and build first app",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-reduced-motion-removes-transitions",
            "installer_transition_duration(false), 0",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-models-leads-with-engine-choice",
            "Goblins AI engine",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-models-collapses-advanced-diagnostics",
            "Advanced · Actions and activity",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-models-advanced-diagnostics-default-closed",
            "disclosure.set_expanded(false)",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-models-advanced-has-accessible-description",
            "Advanced Goblins AI actions and activity",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "studio-engine-choice-is-explicit-menu",
            "let picker = gtk::MenuButton::new()",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "studio-engine-menu-has-accessible-label",
            "Choose Goblins AI engine",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "studio-send-button-has-accessible-name",
            "gtk::accessible::Property::Label(\"Send build request\")",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "studio-engine-menu-has-keyboard-focus-ring",
            ".gos-studio-engine-option:focus:focus-visible",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-app-copy-hides-backend-plumbing",
            "local build daemon",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-first-app-copy-hides-loopback-plumbing",
            "loopback service",
        ),
        container_contains_check(root, "installer-image-anaconda-live", "anaconda-live"),
        container_contains_check(
            root,
            "installer-image-full-storage-helper",
            "goblins-os-full-installer",
        ),
        container_contains_check(
            root,
            "installer-image-full-storage-desktop-entry",
            "org.goblins.OS.FullInstaller.desktop",
        ),
        container_contains_check(
            root,
            "installer-image-full-storage-desktop-validation",
            "desktop-file-validate /usr/share/applications/org.goblins.OS.FullInstaller.desktop",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-assistant-label",
            "Dual-boot assistant",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-assistant-pick-os",
            "Pick the operating system you are keeping",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-preserve-path",
            "Preserve path: keep",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-only-free-space",
            "select only unallocated free space",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-blocked-detail-helper",
            "install_blocked_detail",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-no-eligible-copy",
            "No eligible blank disks were found",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-guided-dual-boot-renderer",
            "append_dual_boot_guide",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-dual-boot-guide-label",
            "Dual-boot guide",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-keep-another-os-review",
            "Keep another OS",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-partition-summary",
            "partition_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-existing-system-summary",
            "existing_system_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-detected-systems-row",
            "Detected systems",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-preservation-details-row",
            "Preservation details",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-whole-disk-only-copy",
            "Disks with existing OS, recovery, EFI, or data partitions are routed to manual storage.",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-efi-erase-warning",
            "including any Windows, macOS, Linux, other OS, recovery, and EFI partitions",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-formatting-guidance",
            "Formatting",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-simple-install-scope",
            "Simple install scope",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-erase-scope",
            "Erase scope",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-startup-recovery",
            "Startup recovery",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-after-reboot",
            "After reboot",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-storage-review-renderer",
            "append_storage_review_checklist",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-storage-review-label",
            "Storage review checklist",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-ui-advanced-storage-guidance",
            "Advanced storage",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-dual-boot-reclaim-space-proof",
            "Custom/manual storage or Reclaim Space",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-full-storage-installer-proof",
            "Open advanced storage",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-full-storage-installer-destination-proof",
            "advanced storage Installation Destination",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-full-storage-installer-manual-summary-proof",
            "advanced storage summary",
        ),
        absent_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-no-anaconda-install-destination-copy",
            "Anaconda Installation Destination",
        ),
        absent_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-no-anaconda-manual-summary-copy",
            "Anaconda manual storage summary",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-dual-boot-desktop-entry-proof",
            "Install Goblins OS Beside Another OS",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-native-installer-blank-disk-proof",
            "simple flow proceeds only for a blank disk",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-dual-boot-proof-handoff",
            "preserved Windows/macOS/APFS/Linux/other OS/recovery/EFI partitions",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-full-storage-installer-proof",
            "Open advanced storage",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-dual-boot-desktop-entry-proof",
            "Install Goblins OS Beside Another OS",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-existing-partition",
            "nvme0n1p1/partition",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-existing-system-uevent",
            "PARTNAME=EFI System Partition",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-windows-partition",
            "TYPE=ntfs",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-macos-apfs-partition",
            "TYPE=apfs",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-linux-luks-partition",
            "TYPE=crypto_LUKS",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-data-partition",
            "TYPE=zfs_member",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-dual-boot-handoff-note",
            "Open advanced storage handoff",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-dual-boot-handoff-screenshot",
            "27-dual-boot-preserve-existing-os.png",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-blank-disk-for-safe-whole-disk-flow",
            "Goblins Blank SSD",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-sys-block-fixture-path",
            "goblins-os-render-sys-block",
        ),
        absent_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-no-fake-sys-block-name",
            "fake-sysblock",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-focused-scope",
            "GOBLINS_OS_RENDER_SCOPE=$RENDER_SCOPE (expected all, chrome, installer, settings, settings-interactions, or polish-interactions)",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "installer-render-focused-confirm-dark-proof",
            "17-install-confirm-dark.png",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-review-splits-dense-safety-copy",
            "review_detail_lines",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "installer-review-scan-line-style",
            ".gos-install-row-line",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "overlay-chrome-visible-focus-states",
            ".gos-cc-tile:focus:focus-visible",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "signoff-install-storage-field",
            "Install storage/bootloader/dual-boot checked",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-install-storage-field",
            "Install storage/bootloader/dual-boot checked: yes",
        ),
    ];

    for screenshot in INSTALL_STORAGE_PROOF_SCREENSHOTS {
        checks.push(contains_check(
            root.join("os/hardware-gate/runbook.md"),
            &format!("runbook-install-storage-proof-{screenshot}"),
            screenshot,
        ));
        checks.push(contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            &format!("shipping-status-install-storage-proof-{screenshot}"),
            screenshot,
        ));
        checks.push(contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            &format!("close-signoff-install-storage-proof-{screenshot}"),
            screenshot,
        ));
    }

    checks
}

fn kickstart_command_absent_check(root: &Path, command: &str) -> Check {
    let path = root.join("os/iso/config.toml");
    let text = read_to_string(&path);
    let active = text.lines().any(|line| {
        let trimmed = line.trim_start();
        !trimmed.starts_with('#') && trimmed.starts_with(command)
    });
    if active {
        blocked(
            &format!("iso-installer-no-active-{command}"),
            &format!(
                "{} contains active kickstart command {command}",
                path.display()
            ),
        )
    } else {
        ready(
            &format!("iso-installer-no-active-{command}"),
            &format!(
                "{} has no active kickstart {command} command",
                path.display()
            ),
        )
    }
}

fn first_boot_idle_policy_checks(path: PathBuf, id_prefix: &str) -> Vec<Check> {
    [
        ("gnome-session-idle-disabled", "idle-delay=uint32 0"),
        (
            "gnome-screensaver-idle-disabled",
            "idle-activation-enabled=false",
        ),
        ("gnome-screensaver-lock-disabled", "lock-enabled=false"),
        ("gnome-screensaver-lock-delay-zero", "lock-delay=uint32 0"),
        ("gnome-lock-screen-disabled", "disable-lock-screen=true"),
        ("gnome-power-idle-dim-disabled", "idle-dim=false"),
        (
            "gnome-power-ac-sleep-disabled",
            "sleep-inactive-ac-type='nothing'",
        ),
        ("gnome-power-ac-timeout-zero", "sleep-inactive-ac-timeout=0"),
        (
            "gnome-power-battery-sleep-disabled",
            "sleep-inactive-battery-type='nothing'",
        ),
        (
            "gnome-power-battery-timeout-zero",
            "sleep-inactive-battery-timeout=0",
        ),
    ]
    .into_iter()
    .map(|(id, needle)| contains_check(path.clone(), &format!("{id_prefix}{id}"), needle))
    .collect()
}

fn settings_render_screenshot_checks(root: &Path) -> Vec<Check> {
    let render_script = root.join("os/bootc/render-screens.sh");
    SETTINGS_RENDER_SCREENSHOTS
        .iter()
        .map(|screenshot| {
            contains_check(
                render_script.clone(),
                &format!("render-settings-screenshot-{screenshot}"),
                screenshot,
            )
        })
        .collect()
}

fn settings_interaction_screenshot_checks(root: &Path) -> Vec<Check> {
    let render_script = root.join("os/bootc/render-screens.sh");
    SETTINGS_INTERACTION_SCREENSHOTS
        .iter()
        .map(|screenshot| {
            contains_check(
                render_script.clone(),
                &format!("render-settings-interaction-screenshot-{screenshot}"),
                screenshot,
            )
        })
        .collect()
}

fn polish_interaction_screenshot_checks(root: &Path) -> Vec<Check> {
    let render_script = root.join("os/bootc/render-screens.sh");
    let mut checks = POLISH_INTERACTION_SCREENSHOTS
        .iter()
        .map(|screenshot| {
            contains_check(
                render_script.clone(),
                &format!("render-polish-interaction-screenshot-{screenshot}"),
                screenshot,
            )
        })
        .collect::<Vec<_>>();
    checks.extend([
        contains_check(
            render_script.clone(),
            "render-polish-interaction-machine-proof",
            POLISH_INTERACTION_PROOF,
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-isolates-policy-state",
            "GOBLINS_OS_POLICY_STATE=\"$GOBLINS_OS_RENDER_STATE_DIR/policy\"",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-uses-real-policy-grant-route",
            "/v1/policy/permissions/grant",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-stops-real-core-for-offline-proof",
            "offline_driver\": \"stop and restart the real goblins-os-core process",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-requires-zero-motion-difference",
            "reduced_motion_zero_difference",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-counts-exact-changed-pixels",
            "round(mean*w*h)",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-error-differs-from-closed-studio",
            "studio_offline_error_vs_closed_changed_pixels",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-finds-native-transient-surfaces",
            "xdotool search --onlyvisible --pid",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-reconstructs-transient-alpha",
            "color 0,0 floodfill",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-uses-real-dark-theme",
            "export GOBLINS_OS_THEME=dark",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-gates-offline-codex-on-real-status",
            "supported_by_real_render_state",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-uses-real-codex-status-route",
            "/v1/codex/status",
        ),
        contains_check(
            render_script.clone(),
            "render-polish-interaction-uses-real-network-status-route",
            "/v1/network/status",
        ),
        contains_check(
            render_script,
            "render-polish-interaction-does-not-claim-codex-auth",
            "authenticated_codex_claim\": False",
        ),
    ]);
    checks
}

fn native_design_system_checks(root: &Path) -> Vec<Check> {
    let mut checks = vec![
        contains_check(
            root.join("Cargo.toml"),
            "workspace-native-design-system",
            "crates/goblins-os-design",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "native-design-shared-css",
            "GOBLINS_NATIVE_CSS",
        ),
        absent_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "native-design-no-legacy-openai-css-name",
            "OPENAI_NATIVE_CSS",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "native-design-css-composer",
            "pub fn native_css",
        ),
        contains_check(
            root.join("Cargo.toml"),
            "workspace-native-ui-theming",
            "crates/goblins-os-ui",
        ),
        contains_check(
            root.join("crates/goblins-os-ui/Cargo.toml"),
            "native-ui-depends-on-design",
            "goblins-os-design",
        ),
        contains_check(
            root.join("crates/goblins-os-ui/Cargo.toml"),
            "native-ui-feature-gates-gtk",
            "native-desktop",
        ),
        contains_check(
            root.join("crates/goblins-os-ui/src/lib.rs"),
            "native-ui-loads-shared-css",
            "goblins_os_design::native_css",
        ),
        contains_check(
            root.join("crates/goblins-os-ui/src/lib.rs"),
            "native-ui-exports-init-theming",
            "pub fn init_theming",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "native-settings-default-width-preserves-sidebar",
            "const SETTINGS_DEFAULT_WIDTH: i32 = 1055",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "native-settings-sidebar-scrollbar-visible",
            ".gos-settings-root .gos-side-scroll scrollbar.vertical slider",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "native-settings-sidebar-scrollbar-not-overlay-only",
            "nav_scroll.set_overlay_scrolling(false)",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "native-launcher-entry-selection-themed",
            ".gos-launcher-entry > text selection",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "native-launcher-selected-row-uses-material",
            "background: @gos_material_active;",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "native-control-center-active-tile-uses-material",
            ".gos-cc-tile.is-on {\n  background: @gos_material_regular;",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "native-control-center-active-segment-uses-material",
            ".gos-cc-seg.is-active {\n  color: @gos_ink;\n  background: @gos_material_active;",
        ),
        contains_check(
            root.join("crates/goblins-os-design/src/lib.rs"),
            "native-selection-regression-test",
            "launcher_and_control_center_selection_stays_material_not_primary_flood",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "native-workspace-overview-body-reserves-footer-space",
            "style_class: 'goblins-wm-body', y_expand: true",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "native-workspace-overview-workspaces-strip-footer-aligned",
            "y_align: Clutter.ActorAlign.END",
        ),
    ];

    for app in NATIVE_DESIGN_APPS {
        checks.push(contains_check(
            root.join(format!("crates/{app}/Cargo.toml")),
            &format!("native-ui-dependency-{app}"),
            "goblins-os-ui",
        ));
        checks.push(contains_check(
            root.join(format!("crates/{app}/Cargo.toml")),
            &format!("native-ui-feature-{app}"),
            "goblins-os-ui/native-desktop",
        ));
        checks.push(contains_check(
            root.join(format!("crates/{app}/src/main.rs")),
            &format!("native-ui-theming-loader-{app}"),
            "goblins_os_ui::init_theming",
        ));
    }

    checks
}

fn release_readiness_checks(root: &Path) -> Vec<Check> {
    vec![
        file_check(root, "os/release/source-tree-manifest.toml"),
        file_check(root, "os/release/asset-provenance.toml"),
        file_check(root, "os/release/third-party-notices.toml"),
        file_check(root, "os/release/trademark-posture.toml"),
        file_check(root, "os/release/architectures.toml"),
        file_check(root, "os/release/release-readiness-delta.toml"),
        file_check(root, "os/release/hydrate-release-artifacts.sh"),
        contains_check(
            root.join("Cargo.toml"),
            "workspace-agpl-license",
            "license = \"AGPL-3.0-or-later\"",
        ),
        contains_check(
            root.join("LICENSE"),
            "license-is-agpl-3",
            "GNU AFFERO GENERAL PUBLIC LICENSE",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-generated-proofs",
            "os/signoff-proofs/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-artifacts-dir",
            "artifacts/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-screenshots",
            "os/screenshots/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-release-artifacts",
            "os/iso/output*/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-ci-workflows",
            ".github/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-gitignore-policy",
            ".gitignore",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-dockerignore-policy",
            ".dockerignore",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-goal-md",
            "GOAL.md",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-signoff-ledger",
            "os/signoff-notes.md",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-rust-session-tools",
            "crates/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-local-agent-state",
            ".claude/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-ci-target-dir",
            ".ci-target/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-ci-target-amd64-dir",
            ".ci-target-amd64/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-target-dir",
            "target/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-local-container-state",
            "libpod/",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-ds-store",
            ".DS_Store",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-shell-fragment-sn",
            "%sn *",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-classifies-shell-fragment-background",
            "-background",
        ),
        contains_check(
            root.join("os/release/hydrate-release-artifacts.sh"),
            "release-hydration-default-skips-large-iso",
            "GOBLINS_OS_DOWNLOAD_ISO",
        ),
        contains_check(
            root.join("os/release/hydrate-release-artifacts.sh"),
            "release-hydration-verifies-split-iso-parts",
            "goblins-os-$arch.iso.zst.parts.sha256",
        ),
        contains_check(
            root.join("os/release/hydrate-release-artifacts.sh"),
            "release-hydration-normalizes-runner-checksum-paths",
            "normalize_sha256_file_paths",
        ),
        contains_check(
            root.join("os/release/hydrate-release-artifacts.sh"),
            "release-hydration-decodes-long-window-zstd",
            "zstd -d --long=31 -f",
        ),
        contains_check(
            root.join("GO-LIVE.md"),
            "go-live-documents-release-artifact-hydration",
            "Published release metadata/SBOM can be hydrated",
        ),
        contains_check(
            root.join("GO-LIVE.md"),
            "go-live-documents-full-iso-hydration",
            "Full ISO release media can be hydrated from split GitHub release assets",
        ),
        contains_check(
            root.join("GO-LIVE.md"),
            "go-live-documents-x86-verification-iso-display-proof",
            "`x86_64` display-backed verification-ISO screenshot/runtime run is complete",
        ),
        contains_check(
            root.join("GO-LIVE.md"),
            "go-live-documents-public-release-iso-artifact-checks",
            "`x86_64` public release ISO artifacts are checked separately",
        ),
        file_check(root, ".github/workflows/aarch64-verification-iso.yml"),
        contains_check(
            root.join(".github/workflows/aarch64-verification-iso.yml"),
            "aarch64-verification-iso-manual-dispatch",
            "workflow_dispatch",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-verification-iso.yml"),
            "aarch64-verification-iso-native-arm-runner",
            "ubuntu-24.04-arm",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-verification-iso.yml"),
            "aarch64-verification-iso-uses-verify-config",
            "GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-verification-iso.yml"),
            "aarch64-verification-iso-short-lived-artifact",
            "retention-days: 7",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-aarch64-verification-iso-workflow",
            "gh workflow run aarch64-verification-iso.yml",
        ),
        source_manifest_top_level_coverage_check(root),
        contains_check(
            root.join(".gitignore"),
            "gitignore-target-dir",
            "target",
        ),
        contains_check(
            root.join(".gitignore"),
            "gitignore-artifacts-dir",
            "artifacts/",
        ),
        contains_check(
            root.join(".gitignore"),
            "gitignore-screenshots-dir",
            "os/screenshots/",
        ),
        contains_check(
            root.join(".gitignore"),
            "gitignore-local-agent-state",
            ".claude/",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-local-agent-state",
            ".claude",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-ci-target-dir",
            ".ci-target",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-ci-target-amd64-dir",
            ".ci-target-amd64",
        ),
        contains_check(
            root.join(".gitignore"),
            "gitignore-ds-store",
            ".DS_Store",
        ),
        contains_check(
            root.join(".gitignore"),
            "gitignore-shell-fragment-background",
            "-background",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-target-dir",
            "target",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-artifacts-dir",
            "artifacts",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-iso-output-artifacts",
            "os/iso/output*",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-screenshots-dir",
            "os/screenshots",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-signoff-proofs-dir",
            "os/signoff-proofs",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-ds-store",
            ".DS_Store",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-shell-fragment-sn",
            "%sn *",
        ),
        contains_check(
            root.join(".dockerignore"),
            "dockerignore-shell-fragment-background",
            "-background",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-goblins-primary-identity",
            "primary product identity",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-goblins-marks",
            "os/brand/Goblins-black-mark.svg",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-openai-raster-wordmark",
            "os/brand/OpenAI-black-wordmark.png",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-openai-raster-monoblossom",
            "os/brand/OpenAI-white-monoblossom.png",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-installer-svg-source",
            "os/brand/anaconda/sidebar-bg.svg",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "plymouth-uses-goblins-mark",
            "cp /usr/share/goblins-os/brand/anaconda/sidebar-logo.png",
        ),
        absent_check(
            root.join("os/bootc/Containerfile"),
            "plymouth-does-not-use-openai-provider-mark",
            "cp /usr/share/goblins-os/brand/OpenAI-white-monoblossom.png",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.Login.desktop"),
            "login-desktop-comment-is-goblins-native",
            "Comment=Native Goblins OS identity gate",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.Recovery.desktop"),
            "recovery-desktop-comment-is-goblins-native",
            "Comment=Native recovery checks for the boot image, services, models, and Goblins identity",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.Policy.desktop"),
            "policy-desktop-comment-is-goblins-native",
            "Comment=Native Goblins OS policy, enterprise controls, data boundaries, and permission gates",
        ),
        contains_check(
            root.join("os/plymouth/goblins-os/goblins-os.plymouth"),
            "plymouth-description-is-goblins-native",
            "Goblins OS boot splash — calm dark with the Goblins mark",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-brand-icons",
            "os/brand/icons/",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-generated-sounds",
            "os/sounds/GoblinsOS/",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-apple-assets-excluded",
            "Apple design sources are quality references only.",
        ),
        contains_check(
            root.join("os/release/asset-provenance.toml"),
            "asset-provenance-sf-symbols-excluded",
            "sf_symbols = \"Not used.\"",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-asset-provenance-completeness",
            "asset provenance covers OpenAI mark variants",
        ),
        contains_check(
            root.join("os/release/third-party-notices.toml"),
            "third-party-notices-rpm-sbom-path",
            "rpm -qa output",
        ),
        contains_check(
            root.join("os/release/third-party-notices.toml"),
            "third-party-notices-cargo-sbom-path",
            "Cargo.lock",
        ),
        contains_check(
            root.join("os/release/third-party-notices.toml"),
            "third-party-notices-gnome-package-coverage",
            "GNOME Shell, GTK, libadwaita/Adwaita assets",
        ),
        contains_check(
            root.join("os/release/third-party-notices.toml"),
            "third-party-notices-gnome-trademark-scope",
            "Goblins OS remains the product identity",
        ),
        contains_check(
            root.join("os/release/trademark-posture.toml"),
            "trademark-posture-goblins-primary",
            "Goblins OS remains the leading product identity",
        ),
        contains_check(
            root.join("os/release/trademark-posture.toml"),
            "trademark-posture-openai-provider-only",
            "Provider/integration reference only",
        ),
        contains_check(
            root.join("os/release/trademark-posture.toml"),
            "trademark-posture-fedora-base-only",
            "Base-platform reference only",
        ),
        contains_check(
            root.join("os/release/trademark-posture.toml"),
            "trademark-posture-gnome-factual-only",
            "Runtime, toolkit, and package reference only",
        ),
        contains_check(
            root.join("os/release/trademark-posture.toml"),
            "trademark-posture-apple-assets-blocked",
            "Do not ship Apple fonts, logos, symbols, wallpapers, screenshots, app screens, product images, SF Symbols, or copied Apple trade dress.",
        ),
        contains_check(
            root.join("crates/goblins-os-verify/src/main.rs"),
            "release-evidence-mode",
            "--release-evidence",
        ),
        contains_check(
            root.join("crates/goblins-os-verify/src/main.rs"),
            "release-evidence-records-trademark-posture",
            "trademark_posture",
        ),
        contains_check(
            root.join("os/release/third-party-notices.toml"),
            "third-party-notices-release-evidence-manifest",
            "release-evidence-manifest.json",
        ),
        contains_check(
            root.join("os/release/third-party-notices.toml"),
            "third-party-notices-cargo-package-tsv",
            "cargo-lock-packages.tsv",
        ),
        contains_check(
            root.join("os/release/third-party-notices.toml"),
            "third-party-notices-rpm-command-file",
            "rpm-packages.command",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-release-evidence-manifest",
            "release evidence manifest exists",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-rpm-sbom",
            "RPM SBOM package TSV exists",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-release-evidence",
            "Release evidence/SBOM checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-generates-release-evidence",
            "--release-evidence",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-generates-rpm-sbom-in-image",
            "generate_image_release_evidence",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-release-evidence-trademark-posture",
            "trademark_posture",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-regenerates-stale-release-evidence-manifest",
            "release_evidence_manifest_has_diligence_fields",
        ),
        contains_check(
            root.join("os/signoff-notes.md"),
            "signoff-template-records-release-evidence",
            "Release evidence/SBOM checked",
        ),
        contains_check(
            root.join("os/signoff-notes.md"),
            "signoff-template-uses-architecture-screenshot-root",
            "os/screenshots/hardware-gate/<arch>/YYYY-MM-DD/",
        ),
        contains_check(
            root.join("os/signoff-notes.md"),
            "signoff-template-requires-real-build-studio-run",
            "Real Build Studio run (real engine)",
        ),
        contains_check(
            root.join("os/signoff-notes.md"),
            "signoff-template-openai-key-is-relay-scoped",
            "BYO OpenAI relay",
        ),
        contains_check(
            root.join("os/signoff-notes.md"),
            "signoff-template-documents-current-docker-runner",
            "Current release runs use Docker on native Linux runners",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-records-release-evidence-path",
            "os/signoff-proofs/sbom/<arch>/",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-records-release-evidence-manifest-fields",
            "`trademark_posture`",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-records-rust-source-gates",
            "rust_source_gates_available",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-records-source-package-materialized",
            "source_package_materialized",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-source-root-is-portable",
            "root = \".\"",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-links-source-tree-manifest",
            "source_tree_manifest = \"os/release/source-tree-manifest.toml\"",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-records-dual-arch-rpm-sbom-proof",
            "dual_arch_rpm_sbom_present",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-blocks-on-native-linux-runner",
            "native_linux_release_runner_required",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-blocks-on-shippable-release-isos",
            "shippable_release_iso_artifacts_incomplete",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-blocks-on-display-proofs",
            "display_backed_architecture_proofs_missing",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-records-x86-rpm-sbom-proof",
            "x86_64_rpm_sbom_present",
        ),
        contains_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-blocks-on-complete-signoff",
            "complete_signoff_rows_missing",
        ),
        absent_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-no-stale-rust-missing-blocker",
            "rust_toolchain_missing",
        ),
        absent_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-no-stale-dataless-blocker",
            "source_files_dataless",
        ),
        absent_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-no-stale-disk-space-blocker",
            "disk_space_low",
        ),
        absent_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-no-stale-x86-rpm-sbom-blocker",
            "x86_64_rpm_sbom_missing",
        ),
        absent_check(
            root.join("os/release/release-readiness-delta.toml"),
            "release-readiness-delta-no-local-user-path",
            "/Users/",
        ),
        contains_check(
            root.join(".gitignore"),
            "gitignore-generated-proof-artifacts",
            "os/signoff-proofs/",
        ),
        contains_check(
            root.join(".gitignore"),
            "gitignore-iso-output-artifacts",
            "os/iso/output*/",
        ),
        contains_check(
            root.join(".gitignore"),
            "gitignore-accidental-shell-fragments",
            "%sn *",
        ),
    ]
}

fn secret_hygiene_checks(root: &Path) -> Vec<Check> {
    vec![
        contains_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "secret-template-documents-empty-image",
            "The immutable image ships it empty",
        ),
        contains_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "secret-template-server-side-relay-only",
            "native clients receive only readiness and plain-language storage labels",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "openai-secret-file-root-owned",
            "chown root:root /etc/goblins-os/openai-secrets.env",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "openai-secret-file-mode-0600",
            "chmod 0600 /etc/goblins-os/openai-secrets.env",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "openai-secret-dir-mode-0700",
            "install -d -m 0700 -o goblins-os -g goblins-os /var/lib/goblins-os/secrets/openai",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "openai-secret-file-mode-asserted",
            "test \"$(stat -c '%U:%G:%a' /etc/goblins-os/openai-secrets.env)\" = \"root:root:600\"",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "openai-secret-file-no-baked-secret",
            "(KEY|SECRET|TOKEN)[[:space:]]*=' /etc/goblins-os/openai-secrets.env",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "environment-file-no-baked-secret",
            "(KEY|SECRET|TOKEN)[[:space:]]*=' /etc/goblins-os/environment",
        ),
        contains_check(
            root.join("crates/goblins-os-verify/src/main.rs"),
            "installed-root-verifier-secret-file-mode",
            "installed-openai-secret-file-mode-0600",
        ),
        contains_check(
            root.join("crates/goblins-os-verify/src/main.rs"),
            "installed-root-verifier-secret-file-owner",
            "installed-openai-secret-file-owner-root",
        ),
        contains_check(
            root.join("crates/goblins-os-verify/src/main.rs"),
            "installed-root-verifier-secret-file-empty",
            "installed-openai-secret-file-empty",
        ),
        contains_check(
            root.join("crates/goblins-os-verify/src/main.rs"),
            "installed-root-verifier-secret-dir-mode",
            "var/lib/goblins-os/secrets/openai",
        ),
        whitespace_normalized_contains_check(
            root.join("SHIP.md"),
            "ship-documents-secret-file-permissions",
            "mode `0600 root:root`",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-source-secret-scan",
            "source_secret_scan",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-source-secret-scan-skips-claude-worktrees",
            "!.claude/**",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-source-secret-scan-skips-ci-target",
            "!.ci-target/**",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-source-secret-scan-skips-ci-target-amd64",
            "!.ci-target-amd64/**",
        ),
        contains_check(
            root.join("os/hardware-gate/secret-scan.sh"),
            "artifact-secret-scan-helper",
            "goblins_os_artifact_secret_scan",
        ),
        contains_check(
            root.join("os/hardware-gate/secret-scan.sh"),
            "artifact-secret-scan-text-only-globs",
            "*.sha256",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-artifact-secret-scan",
            "Generated artifact/evidence secret scan finds no live keys",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-artifact-secret-scan",
            "goblins_os_artifact_secret_scan",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-artifact-secret-scan",
            "goblins_os_artifact_secret_scan",
        ),
        contains_check(
            root.join("SHIP.md"),
            "ship-documents-artifact-secret-scan",
            "artifact/evidence secret scan",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-artifact-secret-scan",
            "artifact/evidence secret scan",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-openai-key-pattern-scan",
            "sk-proj-",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-active-secret-assignment-scan",
            "OPENAI_ACCOUNT_CLIENT_SECRET",
        ),
        container_package_lockstep_check(root, "source-secret-scan-ripgrep-packaged", "ripgrep"),
        container_contains_check(
            root,
            "source-secret-scan-rg-command-available",
            "command -v rg",
        ),
        source_secret_scan_check(root),
    ]
}

fn toml_required_string<'a>(table: &'a toml::Table, key: &str) -> Result<&'a str, String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("installer branding provenance requires string field {key}"))
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn exact_digest_ref(value: &str, repository: &str) -> bool {
    value
        .strip_prefix(&format!("{repository}@sha256:"))
        .is_some_and(|digest| lowercase_hex(digest, 64))
}

fn github_actions_run_url_is_canonical(value: &str) -> bool {
    value
        .strip_prefix("https://github.com/Joe-Simo/goblins-os/actions/runs/")
        .is_some_and(|run_id| {
            !run_id.is_empty() && run_id.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn iso_date_is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        && value[5..7]
            .parse::<u8>()
            .is_ok_and(|month| (1..=12).contains(&month))
        && value[8..10]
            .parse::<u8>()
            .is_ok_and(|day| (1..=31).contains(&day))
}

fn verify_installer_branding_tool_provenance(root: &Path) -> Result<String, String> {
    const TOOL_REPOSITORY: &str = "ghcr.io/joe-simo/goblins-os-installer-branding-tool";
    let provenance_path = root.join("os/release/installer-branding-tool.toml");
    let provenance_text = fs::read_to_string(&provenance_path)
        .map_err(|error| format!("cannot read {}: {error}", provenance_path.display()))?;
    let document = toml::from_str::<toml::Value>(&provenance_text)
        .map_err(|error| format!("invalid {}: {error}", provenance_path.display()))?;
    let table = document
        .as_table()
        .ok_or_else(|| "installer branding provenance root must be a table".to_string())?;
    if table.get("schema").and_then(toml::Value::as_integer) != Some(1) {
        return Err("installer branding provenance schema must be 1".to_string());
    }

    let image_ref = toml_required_string(table, "image_ref")?;
    if !exact_digest_ref(image_ref, TOOL_REPOSITORY) {
        return Err(
            "installer branding index must pin the canonical GHCR repository by digest".to_string(),
        );
    }
    let source_commit = toml_required_string(table, "source_commit")?;
    if !lowercase_hex(source_commit, 40) {
        return Err(
            "installer branding source_commit must be 40 lowercase hex characters".to_string(),
        );
    }
    let workflow_run = toml_required_string(table, "workflow_run")?;
    if !github_actions_run_url_is_canonical(workflow_run) {
        return Err(
            "installer branding workflow_run must be a canonical Goblins OS Actions run"
                .to_string(),
        );
    }
    if table
        .get("workflow_run_attempt")
        .and_then(toml::Value::as_integer)
        .is_none_or(|attempt| attempt <= 0)
    {
        return Err("installer branding workflow_run_attempt must be positive".to_string());
    }
    let base_image = toml_required_string(table, "base_image")?;
    if !image_ref_is_digest_pinned(base_image) {
        return Err("installer branding base_image must be digest pinned".to_string());
    }
    let public_pull_verified_on = toml_required_string(table, "public_pull_verified_on")?;
    if !iso_date_is_valid(public_pull_verified_on) {
        return Err("installer branding public_pull_verified_on must use YYYY-MM-DD".to_string());
    }
    if toml_required_string(table, "inventory_path_in_image")?
        != "/usr/share/goblins-os-installer-branding-tool/rpm-packages.tsv"
    {
        return Err("installer branding inventory path is not canonical".to_string());
    }

    let containerfile_path = root.join("os/iso/branding-tool.Containerfile");
    let recorded_containerfile_sha = toml_required_string(table, "containerfile_sha256")?;
    if !lowercase_hex(recorded_containerfile_sha, 64) {
        return Err("installer branding containerfile_sha256 must be lowercase SHA256".to_string());
    }
    let actual_containerfile_sha = sha256_path(&containerfile_path)
        .map_err(|error| format!("cannot hash {}: {error}", containerfile_path.display()))?;
    if actual_containerfile_sha != recorded_containerfile_sha {
        return Err(format!(
            "installer branding Containerfile hash drifted: expected {recorded_containerfile_sha}, got {actual_containerfile_sha}"
        ));
    }
    let containerfile = fs::read_to_string(&containerfile_path)
        .map_err(|error| format!("cannot read {}: {error}", containerfile_path.display()))?;
    let declared_base_image = containerfile
        .lines()
        .find_map(|line| line.strip_prefix("ARG FEDORA_IMAGE="))
        .ok_or_else(|| {
            "installer branding Containerfile lacks ARG FEDORA_IMAGE=<digest>".to_string()
        })?;
    if declared_base_image != base_image {
        return Err(
            "installer branding Containerfile base image differs from provenance".to_string(),
        );
    }
    let containerfile_lines = containerfile
        .lines()
        .map(str::trim)
        .collect::<BTreeSet<_>>();
    if !containerfile_lines.contains("diffutils \\") {
        return Err(
            "installer branding Containerfile must install diffutils as the cmp provider"
                .to_string(),
        );
    }
    if !containerfile_lines.contains("&& command -v cmp \\") {
        return Err(
            "installer branding Containerfile must assert the required cmp executable".to_string(),
        );
    }

    let branding_workflow_path = root.join(".github/workflows/branding-tool-image.yml");
    let branding_workflow = fs::read_to_string(&branding_workflow_path)
        .map_err(|error| format!("cannot read {}: {error}", branding_workflow_path.display()))?;
    let runtime_tool_check = concat!(
        "for required_tool in checkisomd5 cmp implantisomd5 magick mksquashfs ",
        "osirrox unsquashfs xorriso; do command -v \"$required_tool\" >/dev/null; done"
    );
    if !branding_workflow.contains(runtime_tool_check) {
        return Err(
            "installer branding workflow must verify the remaster runtime tool contract"
                .to_string(),
        );
    }

    let architectures = table
        .get("architectures")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "installer branding provenance requires architectures".to_string())?;
    let actual_arches = architectures
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected_arches = ["aarch64", "x86_64"].into_iter().collect::<BTreeSet<_>>();
    if actual_arches != expected_arches {
        return Err(
            "installer branding provenance must contain exactly aarch64 and x86_64".to_string(),
        );
    }
    let mut native_refs = BTreeSet::new();
    for arch in ["aarch64", "x86_64"] {
        let arch_table = architectures
            .get(arch)
            .and_then(toml::Value::as_table)
            .ok_or_else(|| format!("installer branding provenance lacks {arch} table"))?;
        let native_ref = toml_required_string(arch_table, "native_image_ref")?;
        if !exact_digest_ref(native_ref, TOOL_REPOSITORY) {
            return Err(format!(
                "installer branding {arch} image is not an exact GHCR digest"
            ));
        }
        native_refs.insert(native_ref);
        let inventory_sha = toml_required_string(arch_table, "rpm_inventory_sha256")?;
        if !lowercase_hex(inventory_sha, 64) {
            return Err(format!(
                "installer branding {arch} inventory hash is invalid"
            ));
        }
        if arch_table
            .get("rpm_package_count")
            .and_then(toml::Value::as_integer)
            .is_none_or(|count| count <= 0)
        {
            return Err(format!(
                "installer branding {arch} package count must be positive"
            ));
        }
    }
    if native_refs.len() != 2 {
        return Err(
            "installer branding native architecture image refs must be distinct".to_string(),
        );
    }

    let propagation = [
        (
            "os/iso/build-iso.sh",
            format!(
                "INSTALLER_BRANDING_IMAGE=\"${{GOBLINS_OS_INSTALLER_BRANDING_IMAGE:-{image_ref}}}\""
            ),
        ),
        (
            ".github/workflows/build.yml",
            format!("GOBLINS_OS_INSTALLER_BRANDING_IMAGE: {image_ref}"),
        ),
        (
            ".github/workflows/candidate-artifacts.yml",
            format!("GOBLINS_OS_INSTALLER_BRANDING_IMAGE: {image_ref}"),
        ),
        (
            ".github/workflows/hardware-gate-capture.yml",
            format!("GOBLINS_OS_INSTALLER_BRANDING_IMAGE: {image_ref}"),
        ),
        (
            ".github/workflows/aarch64-verification-iso.yml",
            format!("GOBLINS_OS_INSTALLER_BRANDING_IMAGE: {image_ref}"),
        ),
    ];
    for (relative, expected) in propagation {
        let content = fs::read_to_string(root.join(relative))
            .map_err(|error| format!("cannot read {relative}: {error}"))?;
        if !content.contains(&expected) {
            return Err(format!(
                "installer branding index is not propagated exactly to {relative}"
            ));
        }
    }

    Ok(format!(
        "reviewed installer branding index {image_ref} is hash-bound and propagated; source {source_commit}; run {workflow_run}"
    ))
}

fn installer_branding_tool_provenance_check(root: &Path) -> Check {
    match verify_installer_branding_tool_provenance(root) {
        Ok(detail) => ready("installer-branding-tool-provenance", &detail),
        Err(detail) => blocked("installer-branding-tool-provenance", &detail),
    }
}

fn dual_arch_release_checks(root: &Path) -> Vec<Check> {
    vec![
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64",
            "goblins-os-aarch64.iso",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-iso-path",
            "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-sha256",
            "goblins-os-aarch64.iso.sha256",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-manifest",
            "manifest-goblins-os-aarch64.json",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-hardware-proofs",
            "os/screenshots/hardware-gate/aarch64/<date>/",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-rpm-sbom",
            "os/signoff-proofs/sbom/aarch64/rpm-packages.tsv",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-qemu",
            "qemu-system-aarch64",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-qemu-machine",
            "virt,accel=kvm,gic-version=max",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-uefi-code",
            "AARCH64_UEFI_CODE",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-uefi-vars",
            "AARCH64_UEFI_VARS",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-aarch64-no-emulation-baseline",
            "do not use x86_64 emulation as baseline",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-x86-64",
            "goblins-os-x86_64.iso",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-x86-64-iso-path",
            "os/iso/output/x86_64/bootiso/goblins-os-x86_64.iso",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-x86-64-sha256",
            "goblins-os-x86_64.iso.sha256",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-x86-64-manifest",
            "manifest-goblins-os-x86_64.json",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-x86-64-hardware-proofs",
            "os/screenshots/hardware-gate/x86_64/<date>/",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-x86-64-rpm-sbom",
            "os/signoff-proofs/sbom/x86_64/rpm-packages.tsv",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-x86-64-qemu",
            "qemu-system-x86_64",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "release-architecture-x86-64-kvm",
            "qemu_accel = \"kvm\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-arch-selector",
            "GOBLINS_OS_ARCH",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-native-arch-fail-closed",
            "must be built on a native $ARCH container engine",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-arch-named-output",
            "goblins-os-$ARCH.iso",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-brands-anaconda-installer",
            "brand_installer \"$ISO_PATH\"",
        ),
        contains_check(
            root.join("os/iso/remaster-anaconda-branding.sh"),
            "anaconda-remaster-swaps-goblins-sidebar",
            "sidebar-bg.png",
        ),
        contains_check(
            root.join("os/iso/remaster-anaconda-branding.sh"),
            "anaconda-remaster-recolors-fedora-accent",
            "#0b0b0f",
        ),
        contains_check(
            root.join("os/iso/remaster-anaconda-branding.sh"),
            "anaconda-remaster-verifies-goblins-sidebar-assets",
            "cmp --silent \"$BRAND/sidebar-bg.png\" \"$PIX/sidebar-bg.png\"",
        ),
        contains_check(
            root.join("os/iso/remaster-anaconda-branding.sh"),
            "anaconda-remaster-rejects-legacy-fedora-accent",
            "installer stylesheet still contains the legacy Fedora accent",
        ),
        absent_check(
            root.join("os/iso/remaster-anaconda-branding.sh"),
            "anaconda-remaster-does-not-install-live-packages",
            "dnf -y install",
        ),
        contains_check(
            root.join("os/iso/remaster-anaconda-branding.sh"),
            "anaconda-remaster-verifies-embedded-media-checksum",
            "checkisomd5 --verbose",
        ),
        contains_check(
            root.join("os/iso/branding-tool.Containerfile"),
            "installer-branding-tool-records-rpm-inventory",
            "rpm-packages.tsv",
        ),
        contains_check(
            root.join(".github/workflows/branding-tool-image.yml"),
            "installer-branding-tool-builds-on-native-architectures",
            "ubuntu-24.04-arm",
        ),
        contains_check(
            root.join(".github/workflows/branding-tool-image.yml"),
            "installer-branding-tool-index-records-workflow-attempt",
            "workflow_run_attempt: $workflow_run_attempt",
        ),
        installer_branding_tool_provenance_check(root),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-installer-branding-tool-rotation",
            "Rotating the immutable installer-branding tool",
        ),
        contains_check(
            root.join("os/hardware-gate/release-evidence.sh"),
            "release-evidence-requires-hash-sealed-v4-schema",
            "goblins-os-release-evidence-v4",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-pins-privileged-bib-image",
            "bootc-image-builder@sha256:",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-forbids-release-auth-file-exposure",
            "shippable release media forbids GOBLINS_OS_BIB_AUTH_FILE",
        ),
        absent_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-does-not-mount-registry-token-into-builder",
            "GOBLINS_OS_BIB_AUTH_FILE",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-forbids-skipping-release-branding",
            "shippable release media cannot skip Goblins installer branding",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-requires-native-host-and-engine-for-release",
            "requires a native $ARCH host and container engine",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-records-release-tool-provenance",
            "\"installer_branding_image\": \"$INSTALLER_BRANDING_IMAGE\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-container-runtime-selector",
            "GOBLINS_OS_CONTAINER_RUNTIME",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-host-runtime-docker-only",
            "expected docker",
        ),
        absent_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-no-host-podman-option-copy",
            "docker or podman",
        ),
        absent_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-no-podman-sudo-env",
            "GOBLINS_OS_PODMAN_SUDO",
        ),
        absent_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-no-host-podman-builder",
            "run_podman_builder",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-local-registry-uses-internal-bridge",
            "--internal",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-labels-dedicated-registry-bridge",
            "--label org.goblins-os.purpose=installer-registry-handoff",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-declares-dedicated-egress-network",
            "GOBLINS_OS_DOCKER_EGRESS_NETWORK",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-labels-dedicated-egress-bridge",
            "--label org.goblins-os.purpose=installer-builder-egress",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-requires-distinct-managed-networks",
            "must name distinct Docker networks",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-rejects-built-in-network-names",
            "must name a user-defined Docker network",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-validates-non-internal-egress-network",
            "dedicated non-internal BIB egress bridge contract",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-validates-local-network-scope",
            "scope=\"$(docker network inspect --format '{{.Scope}}'",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-rejects-unexpected-egress-members",
            "refusing to share the builder egress boundary",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-local-registry-host-port-is-loopback-only",
            "-p \"127.0.0.1:$DOCKER_REGISTRY_PORT:5000\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-local-registry-uses-container-dns",
            "builder_image=\"$DOCKER_REGISTRY_NAME:5000/goblins-os:$ARCH\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-probes-local-registry-on-builder-network",
            "probe_docker_registry_from_builder_network",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-local-registry-route-uses-user-defined-egress",
            "--network \"name=$DOCKER_EGRESS_NETWORK,gw-priority=1\"",
        ),
        absent_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-never-mixes-built-in-bridge-with-user-defined-network",
            "--network \"name=bridge,gw-priority=1\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-requires-dual-network-capable-docker",
            "requires Docker 28 or newer on both client and server",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-validates-both-docker-versions",
            "docker_versions_support_dual_network",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-tests-dual-network-create",
            "if ! preflight_container_id=\"$(docker create",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-verifies-gateway-priority",
            ".GwPriority",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-requires-egress-priority-one",
            "|| [ \"$egress_priority\" != \"1\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-requires-registry-priority-zero",
            "|| [ \"$registry_priority\" != \"0\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-creates-user-defined-egress",
            "--label org.goblins-os.purpose=installer-network-preflight-egress",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-creates-internal-registry-network",
            "--label org.goblins-os.purpose=installer-network-preflight-registry",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-checks-both-internal-flags",
            "|| [ \"$egress_internal\" != \"false\" ] \\\n    || [ \"$registry_internal\" != \"true\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-checks-distinct-network-ids",
            "|| [ \"$egress_network_id\" = \"$registry_network_id\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-checks-egress-network-id",
            "|| [ \"$egress_network_id\" != \"$expected_egress_network_id\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-checks-registry-network-id",
            "|| [ \"$registry_network_id\" != \"$expected_registry_network_id\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-binds-egress-create-id",
            "|| [ \"$expected_egress_network_id\" != \"$preflight_egress_network_id\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-binds-registry-create-id",
            "|| [ \"$expected_registry_network_id\" != \"$preflight_registry_network_id\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-requires-local-scopes",
            "|| [ \"$egress_scope\" != \"local\" ] \\\n    || [ \"$registry_scope\" != \"local\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-proves-two-network-count",
            "if [ \"$network_count\" != \"2\" ]",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-verifies-container-cleanup",
            "preflight container cleanup did not complete",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-verifies-egress-cleanup",
            "preflight egress-network cleanup did not complete",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-verifies-registry-cleanup",
            "preflight registry-network cleanup did not complete",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-fails-closed-on-name-collision",
            "refusing to remove an object this invocation did not create",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-tracks-container-ownership",
            "preflight_container_created=1",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-tracks-egress-network-ownership",
            "preflight_egress_network_created=1",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-tracks-registry-network-ownership",
            "preflight_registry_network_created=1",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-cleans-exact-container-id",
            "bounded_docker_remove \"$preflight_container_id\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-cleans-exact-egress-id",
            "bounded_docker_network_remove \"$preflight_egress_network_id\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-cleans-exact-registry-id",
            "bounded_docker_network_remove \"$preflight_registry_network_id\"",
        ),
        ordered_contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-capability-preflight-precedes-local-image-build",
            "    require_docker_dual_network_capability\n",
            "DOCKER_BUILDKIT=1 docker build",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-reuses-existing-local-registry-image-scope",
            "LOCAL_REGISTRY_IMAGE=\"registry:2\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-attaches-to-internal-registry-network",
            "--network \"$DOCKER_REGISTRY_NETWORK\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-probe-reuses-builder-network-arguments",
            "probe_docker_registry_from_builder_network \"${bib_network_args[@]}\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-probe-applies-forwarded-network-arguments",
            "\"${network_args[@]}\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-validates-live-registry-network-endpoint",
            "lacks the exact live endpoint for $DOCKER_REGISTRY_NETWORK",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-requires-exact-post-start-registry-membership",
            "assert_dedicated_registry_network_membership true",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-requires-empty-dedicated-egress-membership",
            "assert_dedicated_egress_network_membership",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-post-build-network-checks-are-managed-route-only",
            "if [ \"$source_route\" = \"managed-registry\" ]; then\n    # --rm must restore both managed networks",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-pre-build-network-checks-are-managed-route-only",
            "if [ \"$source_route\" = \"managed-registry\" ]; then\n    # Recheck immediately before the privileged builder attaches",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-public-registry-route-is-network-neutral",
            "Public remote images intentionally use Docker's normal network only;\n      # they create and attach neither managed local network.",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-preloads-route-probe-image",
            "docker pull --platform \"$DOCKER_PLATFORM\" \"$BIB\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-route-probe-does-not-pull-inside-deadline",
            "docker run --rm --pull=never",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-bounds-local-registry-route-probe",
            "BIB registry route probe timed out after ${DOCKER_REGISTRY_PROBE_TIMEOUT_SECS}s",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-registry-probe-cleans-up-on-exit",
            "trap cleanup_registry_probe EXIT",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-registry-probe-cleanup-is-bounded",
            "bounded_stop_process",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-registry-probe-rechecks-final-deadline",
            "completed during that second instead of reporting a false timeout",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-validates-local-registry-network-membership",
            "refusing to expose the unauthenticated build registry to it",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-host-gateway-is-explicit-override-only",
            "host-gateway is intentionally available only for this explicit override",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-explicit-host-registry-adds-host-gateway",
            "bib_host_args=(--add-host=host.docker.internal:host-gateway)",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-rejects-container-loopback-source",
            "uses container loopback and cannot reach a host registry from BIB",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-rejects-unsupported-local-alias",
            "uses an unsupported local registry alias",
        ),
        contains_check(
            root.join("os/iso/manifest-provenance.sh"),
            "installer-local-ref-classifier-is-shared",
            "goblins_os_image_ref_is_local_only",
        ),
        contains_check(
            root.join("os/iso/manifest-provenance.sh"),
            "installer-local-ref-classifier-rejects-non-global-ip-literals",
            "address.is_global",
        ),
        contains_check(
            root.join("os/iso/manifest-provenance.sh"),
            "installer-local-ref-classifier-normalizes-trailing-dot-aliases",
            "normalized_host=\"${normalized_host%.}\"",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-regresses-local-ref-classification",
            "installer_local_ref_classifier_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-uses-shared-local-ref-classifier",
            "goblins_os_image_ref_is_local_only \"$actual_ref\"",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-uses-shared-local-ref-classifier",
            "goblins_os_image_ref_is_local_only \"$bib_image_ref\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-shippable-source-ref-env",
            "GOBLINS_OS_BIB_SOURCE_IMAGE",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-shippable-release-guard",
            "shippable release media cannot track local/test-only installer payload ref",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-skip-local-image-build-env",
            "GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-skip-local-image-build-requires-registry-source",
            "requires GOBLINS_OS_BIB_SOURCE_IMAGE",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-selects-explicit-config",
            "GOBLINS_OS_ISO_CONFIG",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-records-installer-config-in-manifest",
            "\"installer_config\": \"$CONFIG_LABEL\"",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-requires-digest-pinned-candidate-image",
            "candidate_image_ref",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-pulls-selected-candidate-image",
            "docker pull \"$GOBLINS_OS_CANDIDATE_IMAGE_REF\"",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-verifies-candidate-oci-revision",
            "org.opencontainers.image.revision",
        ),
        file_check(
            root,
            "os/hardware-gate/capture-harness/evidence_bundle.py",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/evidence_bundle.py"),
            "hardware-evidence-bundle-has-exact-v1-schema",
            "goblins-os-hardware-evidence-bundle-v1",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/evidence_bundle.py"),
            "hardware-evidence-bundle-requires-32nd-firstboot-shot",
            "05-first-boot-private-unlock.png",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/evidence_bundle.py"),
            "hardware-evidence-bundle-rejects-duplicate-json-keys",
            "reject_duplicate_keys",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/evidence_bundle.py"),
            "hardware-evidence-bundle-uses-no-follow-files",
            "O_NOFOLLOW",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/evidence_bundle.py"),
            "hardware-evidence-bundle-enforces-uniform-framebuffer",
            "evidence screenshots do not share one framebuffer size",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-recomputes-canonical-evidence-bundle",
            "evidence_bundle.py\" verify",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-recomputes-canonical-evidence-bundle",
            "evidence_bundle.py\" verify",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-validates-proof-run-directory",
            "--run-directory \"$SCREENSHOT_DIR\" \"$REPO_ROOT\" \"$ARCH\"",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-validates-exact-proof-manifest",
            "--manifest \"$manifest\" \"$arch\" \"$SELECTED_CANDIDATE_COMMIT\"",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "x86-hardware-evidence-artifact-name-is-attempt-bound",
            "hardware-gate-evidence-${{ inputs.candidate_commit }}-${{ matrix.arch }}-${{ inputs.run_date }}-attempt-${{ github.run_attempt }}",
        ),
        file_check(
            root,
            ".github/workflows/aarch64-local-display-attestation.yml",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-local-display-attestation.yml"),
            "aarch64-local-display-attestation-is-github-hosted",
            "runs-on: ubuntu-24.04",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-local-display-attestation.yml"),
            "aarch64-local-display-attestation-has-least-signing-permissions",
            "attestations: write",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-local-display-attestation.yml"),
            "aarch64-local-display-attestation-has-oidc-permission",
            "id-token: write",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-local-display-attestation.yml"),
            "aarch64-local-display-attestation-uses-reviewed-attest-action",
            "actions/attest@f7c74d28b9d84cb8768d0b8ca14a4bac6ef463e6",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-verifies-signed-local-display-seal",
            "gh attestation verify",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-verifies-signed-local-display-seal",
            "gh attestation verify",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-matches-local-display-artifact-bytes",
            "github_actions_artifact_file_matches",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-authenticates-local-display-workflow-run",
            "github_actions_run_is_successful",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-binds-local-display-signer-workflow",
            "--signer-workflow Joe-Simo/goblins-os/.github/workflows/aarch64-local-display-attestation.yml",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-rejects-self-hosted-local-display-attestation",
            "--deny-self-hosted-runners",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-matches-exact-github-evidence-files",
            "github_actions_artifact_file_matches",
        ),
        absent_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-does-not-publish-channel-images",
            "docker/build-push-action",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-cancels-superseded-runs",
            "cancel-in-progress: true",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-kvm-runner-access-prep",
            "sudo chmod a+rw /dev/kvm",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-kvm-runner-access-assertion",
            "test -r /dev/kvm && test -w /dev/kvm",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-installs-close-signoff-search-dependency",
            "ripgrep",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-uses-ripgrep-for-proof-contracts",
            "rg -q",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-skips-local-iso-image-build",
            "GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD=1",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-uses-verification-iso-config",
            "GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml",
        ),
        absent_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-no-daemon-export-build-tag",
            "docker build -f os/bootc/Containerfile -t localhost/goblins-os",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-requires-real-bootc-ref-for-display-proof",
            "Display-backed shipping proof requires GOBLINS_OS_BIB_SOURCE_IMAGE",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-passes-shippable-source-to-iso-builder",
            "GOBLINS_OS_BIB_SOURCE_IMAGE=\"$BIB_SOURCE_IMAGE\"",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-passes-shippable-release-flag",
            "GOBLINS_OS_SHIPPABLE_RELEASE=\"$SHIPPABLE_RELEASE\"",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-display-proof-requires-shippable-manifest",
            "ISO manifest shippable release mode",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-display-proof-rejects-local-bib-manifest",
            "refusing display-backed release proof",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-rpm-evidence-mounts-real-output-dir",
            "-v \"$evidence_abs:/out\"",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-artifact-only-does-not-create-screenshot-root",
            "Screenshot target: not created for artifact-only Docker run",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-screenshot-dir-created-only-for-qemu-or-closeoff",
            r#"[[ "$RUN_QEMU" == "1" || "$RUN_CLOSEOFF" == "1" ]]"#,
        ),
        absent_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-no-stale-evidence-mount-var",
            "evidence_mount",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-release-image-ref",
            "RELEASE_IMAGE=<registry>/<namespace>/goblins-os@sha256:<64-hex-digest>",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-rejects-docker-local-release-manifest",
            "\"installer_payload_source_local_only\": false",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-screenshot-proof-manifest",
            "proof-manifest.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-firewall-live-toggle-proof",
            "firewall-live-toggle-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-session-enable-proof",
            "text-shortcuts-session-enable-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-classifies-textshortcuts-preflight-as-non-live",
            "non-live build-time behavior",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-metadata-preflight",
            "candidate metadata probe",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-overlay-intent-preflight",
            "--overlay-intent-self-test",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-bubble-frame-preflight",
            "--candidate-bubble-frame-self-test",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-bubble-layout-preflight",
            "--candidate-bubble-layout-self-test",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-bubble-render-intent-preflight",
            "--candidate-bubble-render-intent-self-test",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-rejects-textshortcuts-synthetic-diagnostic-as-release-evidence",
            "not evidence of the production popup",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-live-ibus-runtime-render-proof",
            "text-shortcuts-live-ibus-runtime-render-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-bubble-render-screenshot",
            "31-text-shortcuts-candidate-bubble-render.png",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-live-ibus-runtime-render-screenshot",
            "32-text-shortcuts-live-ibus-runtime-render.png",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-keyboard-shortcuts-roundtrip-proof",
            "keyboard-shortcuts-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-input-sources-roundtrip-proof",
            "input-sources-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-multi-display-apply-proof",
            "multi-display-apply-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-multi-display-apply-manifest-key",
            "multi_display_apply_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-focus-arm-roundtrip-proof",
            "focus-arm-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-app-privacy-revoke-proof",
            "app-privacy-revoke-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-preview-open-render-proof",
            "preview-open-render-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-audio-output-proof",
            "audio-output-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-audio-output-manifest-key",
            "audio_output_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-preview-open-render-screenshots",
            "29-preview-pdf-open.png",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-preview-open-render-image-screenshot",
            "30-preview-image-open.png",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-native-renderer-honesty",
            "synthetic_overlay=false",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-password-refusal-proof",
            "\"password_refusal\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-passthrough-proof",
            "\"passthrough_actual\": \"hello.\"",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-final-runtime-proof",
            "\"focused_field_callback\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-core-readiness-live",
            "\"core_readiness_flip\": \"live\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-harness-rejects-stale-gdm-screenshot-set",
            "stable_frame_hash",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-harness-crops-top-bar-for-stale-screenshot-guard",
            "cropping the top bar",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-harness-stable-frame-hash-macos-fallback",
            "macOS sips",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-harness-distinct-guard-ignores-debug-frames",
            "! -name '_debug-*'",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-waits-for-unique-required-frame",
            "REQUIRED_FRAME_SETTLE_SECONDS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-fail-closes-on-duplicate-frame",
            "framebuffer stayed duplicate",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-bounds-total-ready-signal-window",
            "GOS_CAPTURE_TOTAL_TIMEOUT_SECONDS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-bounds-ready-signal-inactivity",
            "GOS_CAPTURE_INACTIVITY_TIMEOUT_SECONDS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-resets-timeout-on-progress",
            "last_progress = time.monotonic()",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-reports-missing-ready-shots",
            "EXPECTED_READY_SHOTS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-reports-time-since-progress",
            "seconds_since_progress",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-launches-nonunique-proof-windows",
            "GOBLINS_OS_CAPTURE_NON_UNIQUE=1",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-bounds-proof-window-settle",
            "GOS_SHOT_SETTLE_SECONDS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-bounds-ready-signals",
            "GOS_READY_SIGNAL_TIMEOUT_SECONDS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-bounds-shot-helpers",
            "GOS_SHOT_HELPER_TIMEOUT_SECONDS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-forwards-installer-core-wait",
            "GOBLINS_OS_INSTALLER_CORE_WAIT_SECS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-routes-installer-shots-through-fixture-core",
            "installer_shot()",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-bounds-installer-capture-core-wait",
            "GOS_INSTALLER_CAPTURE_CORE_WAIT_SECS",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-page-override-explicitly-supports-welcome",
            "Some(\"welcome\") => \"welcome\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-logs-bounded-helper-timeouts",
            "GOBLINS_HWGATE_BOUNDED_COMMAND_TIMED_OUT",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-kills-stale-proof-window-processes",
            "pkill -x \"$base\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-kills-long-proof-window-commands",
            "pkill -f -- \"$bin\"",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-does-not-launch-proof-windows-on-nested-dbus",
            "dbus-run-session -- \"$@\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-firewall-live-toggle-proof",
            "/proof/firewall-live-toggle",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-firewall-proof-carries-toggle-response-text",
            "enable_text=$(proof_query_value",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-defuses-switch-control-before-ordinary-shots",
            "switch_control_off",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-switch-control-off-uses-gsettings",
            "gsettings set org.goblins.os.a11y.switch-control enabled false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-switch-control-off-uses-shell-hide-hook",
            "goblinsSwitchControl.hide",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-disables-switch-overlay-extension",
            "gnome-extensions disable goblins-switch@goblins.os",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
            "switch-control-hide-stops-overlay-immediately",
            "this._stopScanner();",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-capture-proof-instances-are-nonunique",
            "GOBLINS_OS_CAPTURE_NON_UNIQUE",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-capture-proof-uses-gtk-nonunique-flag",
            "ApplicationFlags::NON_UNIQUE",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-capture-proof-instances-are-nonunique",
            "GOBLINS_OS_CAPTURE_NON_UNIQUE",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-capture-proof-uses-gtk-nonunique-flag",
            "ApplicationFlags::NON_UNIQUE",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-capture-proof-instances-are-nonunique",
            "GOBLINS_OS_CAPTURE_NON_UNIQUE",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-capture-proof-uses-gtk-nonunique-flag",
            "ApplicationFlags::NON_UNIQUE",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-session-proof",
            "/proof/text-shortcuts-session-enable",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-does-not-post-retired-textshortcuts-live-keystroke-proof",
            "/proof/text-shortcuts-live-keystroke",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-candidate-metadata-proof",
            "/proof/text-shortcuts-candidate-metadata",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-overlay-intent-proof",
            "/proof/text-shortcuts-overlay-intent",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-candidate-bubble-frame-proof",
            "/proof/text-shortcuts-candidate-bubble-frame",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-candidate-bubble-layout-proof",
            "/proof/text-shortcuts-candidate-bubble-layout",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-candidate-bubble-render-intent-proof",
            "/proof/text-shortcuts-candidate-bubble-render-intent",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-candidate-bubble-render-proof",
            "/proof/text-shortcuts-candidate-bubble-render",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-live-ibus-runtime-render-proof",
            "/proof/text-shortcuts-live-ibus-runtime-render",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-drives-textshortcuts-live-runtime-proof-app",
            "--text-shortcuts-proof normal",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-propagates-textshortcuts-live-proof-ledger-env",
            "systemctl --user set-environment GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-focuses-textshortcuts-field-before-live-runtime-input",
            "host_focus_text_shortcuts_field runtime-render-focus",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-captures-textshortcuts-candidate-bubble-render-screenshot",
            "31-text-shortcuts-candidate-bubble-render",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-references-textshortcuts-live-ibus-runtime-render-screenshot",
            "32-text-shortcuts-live-ibus-runtime-render.png",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-keyboard-shortcuts-roundtrip-proof",
            "/proof/keyboard-shortcuts-roundtrip",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-input-sources-roundtrip-proof",
            "/proof/input-sources-roundtrip",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-focus-arm-roundtrip-proof",
            "/proof/focus-arm-roundtrip",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-app-privacy-revoke-proof",
            "/proof/app-privacy-revoke",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-audio-output-proof",
            "/proof/audio-output",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-waits-for-audio-settings-title",
            "Goblins OS Settings - Sound",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-bounds-audio-title-wait-attempts",
            r#"GOS_SHOT_WINDOW_WAIT_ATTEMPTS="${GOS_AUDIO_SHOT_WINDOW_WAIT_ATTEMPTS:-8}""#,
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-bounds-audio-title-helper-timeout",
            r#"GOS_SHOT_HELPER_TIMEOUT_SECONDS="${GOS_AUDIO_SHOT_HELPER_TIMEOUT_SECONDS:-1}""#,
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-proof-buffers-one-second-tone",
            "one_second = bytearray()",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-proof-reuses-tone-buffer",
            "for _ in range(seconds):",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-status-uses-finite-release-proof-operation",
            "core_proof_request audio-status \"$status_file\" || true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-harness-audio-status-operation-is-one-exact-route",
            "audio-status) request audio-status GET /v1/audio/status ;;",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-harness-release-proof-request-has-connect-timeout",
            "--connect-timeout 2",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-harness-release-proof-request-has-total-timeout",
            "--max-time \"$timeout\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-status-attempts-are-bounded",
            "GOS_AUDIO_STATUS_ATTEMPTS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-wav-generation-is-bounded",
            "GOBLINS_HWGATE_AUDIO_WAV_GENERATION_TIMED_OUT",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-proof-records-wav-generation-state",
            "wav_generated=$wav_generated",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-failure-probes-core-service-state",
            "core_probe_http",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-waits-on-present-ledger",
            "GOBLINS_OS_CAPTURE_PRESENT_LEDGER",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-writes-capture-present-ledger",
            "GOBLINS_OS_CAPTURE_PRESENT_LEDGER",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-audio-proof-records-core-restarts",
            "core_restarts=",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-accepts-wpctl-op",
            "Wpctl",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-validates-wpctl-allowlist",
            "validate_wpctl_args",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-bounds-wpctl-probe",
            "WirePlumber did not answer before the session bridge audio timeout.",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-allowlists-sound-gsettings",
            "org.gnome.desktop.sound",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-allowlists-recursive-sound-gsettings-read",
            "\"list-recursively\"",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-bounds-gsettings-probe",
            "gsettings did not answer before the session bridge preference timeout.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-client-supports-wpctl",
            "pub(crate) fn wpctl",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-client-bounds-socket-io",
            "BRIDGE_IO_TIMEOUT",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-status-uses-session-bridge-wpctl-first",
            "session_bridge::wpctl(args)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-status-uses-device-snapshot-for-endpoint-readiness",
            "audio_endpoint_ready_without_volume_detail",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-status-parses-inline-wpctl-volume",
            "parse_wpctl_volume(suffix)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-sound-preferences-use-session-bridge-gsettings-first",
            "session_bridge::gsettings(args)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-sound-preferences-use-single-recursive-snapshot",
            "[\"list-recursively\", SOUND_SCHEMA]",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-sound-preferences-parse-recursive-snapshot",
            "parse_sound_schema_snapshot",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-status-defers-sound-preference-reads",
            "Audio device readiness does not wait for desktop sound preferences. Changing system sounds still checks the current session.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-status-uses-default-volume-readiness",
            "audio_endpoint_default_volume_status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-status-reads-default-output-volume",
            r#"wpctl(&["get-volume", target.wpctl_id()])"#,
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-vm-attaches-dummy-audio-backend",
            "-audiodev none,id=audio0",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-vm-attaches-hda-output-controller",
            "ich9-intel-hda",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-vm-attaches-hda-output-codec",
            "hda-output,audiodev=audio0",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-wpctl-timeout-is-configurable",
            "GOBLINS_OS_WPCTL_TIMEOUT_MS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-wpctl-timeout-is-clamped",
            "clamp_wpctl_timeout_ms",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-status-uses-one-device-snapshot",
            "audio_device_snapshot",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-wpctl-probe-is-bounded",
            "bounded_session_command_output",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/bounded.rs"),
            "core-bounded-command-runner-kills-at-bound",
            "try_wait()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-wpctl-timeout-copy-is-truthful",
            "WirePlumber did not answer before the audio status timeout.",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-app-privacy-seeds-permission-store",
            "PermissionStore.SetPermission",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-app-privacy-set-permission-plain-fallback",
            "plain_permissions",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-seeds-permissionstore-flatpak-db-directory",
            "/var/home/goblin/.local/share/flatpak/db",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-creates-permissionstore-flatpak-db-directory",
            ".local/share/flatpak/db",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-reports-permissionstore-db-directory-failure",
            "permission-db-dir",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-app-privacy-reports-seed-attempt",
            "seed_attempt=",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-app-privacy-reports-seed-error",
            "seed_error=",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-waits-permission-store-bus-name",
            "wait_session_bus_name org.freedesktop.impl.portal.PermissionStore",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-app-privacy-revokes-through-permission-store",
            "PermissionStore.DeletePermission",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-app-privacy-reads-back-permission-store",
            "PermissionStore.GetPermission",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-app-privacy-restores-prior-state",
            "restore_prior_state=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-preview-open-render-proof",
            "/proof/preview-open-render",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-single-instances-session-orchestrator",
            "goblins-hwgate-orchestrator.lock",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-rejects-duplicate-session-orchestrator",
            "GOBLINS_HWGATE_ORCHESTRATOR_ALREADY_RUNNING",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-firewall-toggle-route",
            "/v1/firewall/enabled",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-textshortcuts-status-route",
            "/v1/text-shortcuts",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-keyboard-shortcut-binding-route",
            "/v1/keyboard/shortcuts/binding",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-keyboard-modifier-remap-route",
            "/v1/keyboard/modifier-remap",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-input-sources-route",
            "/v1/input/sources",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-input-switch-next-route",
            "/v1/input/switch-next",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-focus-status-route",
            "/v1/focus/status",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-focus-activate-route",
            "/v1/focus/activate",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-focus-deactivate-route",
            "/v1/focus/deactivate",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-harness-focus-mode-seed-is-one-exact-operation",
            "focus-mode-seed) request focus-mode-seed POST /v1/focus/mode '{\"id\":\"gate-work\",\"name\":\"Gate Work\"}' ;;",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-invokes-finite-focus-mode-seed-operation",
            "core_proof_request focus-mode-seed /tmp/gate-focus-mode-seed.json || true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-focus-active-mode-readback",
            "active_mode_gsettings_readback=gate-work",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-focus-banner-restore",
            "original_notification_banners_restored=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keeps-focus-mode-crud-claim-false",
            "mode_crud_claim=false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-preview-status-route",
            "/v1/preview/status",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-uses-preview-open-route",
            "/v1/preview/open",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-verifies-fedora-gnome-ibus-service",
            "TEXT_SHORTCUTS_IBUS_SERVICE=org.freedesktop.IBus.session.GNOME.service",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-refreshes-textshortcuts-ibus-cache",
            "gate-text-shortcuts-session-write-cache.log",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-seeds-textshortcuts-user-ibus-component",
            "ensure_textshortcuts_ibus_component",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-waits-for-textshortcuts-ibus-cli",
            "wait_ibus_cli_ready",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-waits-for-textshortcuts-ibus-bus-owner",
            "wait_ibus_bus_owned",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-reports-textshortcuts-user-component-seed",
            "user_component_seeded=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-reports-textshortcuts-engine-list-error",
            "list_error=",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-reports-textshortcuts-ibus-bus-owner",
            "bus_owner=",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-reports-textshortcuts-ibus-service-diagnostics",
            "service_diag=",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-reports-textshortcuts-ibus-daemon-process",
            "daemon_process=",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-reports-textshortcuts-ibus-session-env",
            "session_env=",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-restarts-textshortcuts-ibus-after-cache-refresh",
            "gate-text-shortcuts-session-ibus-restart.log",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-records-fedora-gnome-ibus-service-unit",
            "service_unit=$TEXT_SHORTCUTS_IBUS_SERVICE",
        ),
        contains_check(
            root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
            "session-wants-fedora-gnome-ibus-service",
            "Wants=org.freedesktop.IBus.session.GNOME.service",
        ),
        contains_check(
            root.join("os/systemd-user/org.goblins.OS.InputSourcesSeed.service"),
            "input-source-seed-runs-before-fedora-gnome-ibus",
            "Before=org.freedesktop.IBus.session.GNOME.service",
        ),
        path_absent_check(
            root,
            "os/systemd-user/org.goblins.OS.IBus.service",
            "custom-ibus-service-removed-to-use-fedora-gnome-service",
        ),
        absent_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-strips-custom-gtk-flag",
            "application.run_with_args(&[\"goblins-os-shell\", \"--text-shortcuts-proof\"]);",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-activates-goblins-ibus-engine",
            "ibus engine goblins-textshortcuts",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keeps-textshortcuts-runtime-claim-false",
            "runtime_ready_claim=false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-session-enable-proves-plumbing-without-live-readiness",
            "proof_scope=session-plumbing",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-session-enable-records-observed-runtime-readiness",
            "core_engine_available=$core_engine_available&core_runtime_loop_available=$core_runtime_loop&runtime_ready_claim=false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-drives-textshortcuts-with-qmp-keyboard",
            "host_type_text runtime-render-omw \"omw\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-clicks-textshortcuts-field-before-typing",
            "host_focus_text_shortcuts_field runtime-render-focus",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-dismisses-shell-overview-before-textshortcuts-typing",
            "dismiss_shell_overview text-shortcuts-live-runtime-render",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-focuses-textshortcuts-candidate-render-window",
            "host_focus_text_shortcuts_field candidate-render-focus",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-defaults-wayland-session-env",
            "export XDG_SESSION_TYPE=\"${XDG_SESSION_TYPE:-wayland}\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-defaults-wayland-display-env",
            "export WAYLAND_DISPLAY=\"${WAYLAND_DISPLAY:-wayland-0}\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-defaults-x11-display-env",
            "export DISPLAY=\"${DISPLAY:-:0}\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-declares-qmp-keyboard-driver",
            "TEXT_SHORTCUTS_INPUT_DRIVER=qmp-keyboard",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-handles-authenticated-qmp-keyboard-text-event",
            "event[\"input_kind\"] == \"text\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-handles-authenticated-qmp-keyboard-key-event",
            "event[\"input_kind\"] == \"key\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-handles-authenticated-qmp-pointer-event",
            "event[\"input_kind\"] == \"click\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-opens-textshortcuts-password-proof",
            "goblins-os-shell\" --text-shortcuts-proof password",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-opens-textshortcuts-passthrough-proof",
            "goblins-os-shell\" --text-shortcuts-proof passthrough",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-drives-textshortcuts-passthrough",
            "host_type_text runtime-passthrough-hello \"hello.\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-textshortcuts-passthrough-unchanged",
            "passthrough_actual=hello.",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-drives-textshortcuts-password-refusal",
            "host_type_text runtime-password-omw \"omw.\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-textshortcuts-password-refusal-in-live-proof",
            "password_refusal=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-textshortcuts-password-refusal",
            "password_refusal=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-opens-textshortcuts-candidate-proof",
            "goblins-os-shell\" --text-shortcuts-proof candidate",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-textshortcuts-candidate-replacement",
            "replacement=on my way",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keeps-textshortcuts-candidate-render-claim-false",
            "rendered_bubble_ready_claim=false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keeps-textshortcuts-live-overlay-claim-false",
            "live_overlay_claim=false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-runs-textshortcuts-candidate-bubble-frame-self-test",
            "--candidate-bubble-frame-self-test",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-runs-textshortcuts-candidate-bubble-render-intent-self-test",
            "--candidate-bubble-render-intent-self-test",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-textshortcuts-candidate-bubble-frame-style",
            "style_class=gos-text-shortcuts-candidate",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-textshortcuts-candidate-bubble-render-intent-count",
            "render_intent_count=8",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keeps-textshortcuts-candidate-bubble-render-claim-false",
            "rendered_bubble_ready_claim=false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keyboard-roundtrip-sets-window-hud",
            "shortcut_action=window-hud",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keyboard-roundtrip-uses-test-binding",
            "shortcut_binding=%3CSuper%3E%3CShift%3EH",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keyboard-roundtrip-reads-back-shortcut",
            "shortcut_gsettings_readback=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keyboard-roundtrip-remaps-caps",
            "modifier_gsettings_readback=ctrl:nocaps",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-keyboard-roundtrip-restores-defaults",
            "roundtrip_restored=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-input-sources-roundtrip-uses-test-sources",
            "test_sources=xkb-us,xkb-gb",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-input-sources-roundtrip-reads-back-sources",
            "sources_gsettings_readback=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-input-sources-roundtrip-switches-current",
            "switch_switched=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-input-sources-roundtrip-restores-defaults",
            "restore_sources=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-focus-roundtrip-uses-test-mode",
            "test_mode=gate-work",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-focus-roundtrip-reads-back-active-mode",
            "active_mode_gsettings_readback=gate-work",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-focus-roundtrip-suppresses-banners",
            "notification_banners_after_activate=false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-focus-roundtrip-restores-banners",
            "notification_banners_after_deactivate=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-focus-roundtrip-restores-original-state",
            "original_focus_state_restored=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-focus-roundtrip-keeps-per-app-claim-false",
            "per_app_breakthroughs_claim=false",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-preview-verifies-papers-default",
            "org.gnome.Papers.desktop",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-preview-verifies-loupe-default",
            "org.gnome.Loupe.desktop",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-preview-captures-pdf-screenshot",
            "sig 29-preview-pdf-open",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-preview-captures-image-screenshot",
            "sig 30-preview-image-open",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-preview-records-rendered-pdf-frame",
            "rendered_pdf_frame=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-preview-records-rendered-image-frame",
            "rendered_image_frame=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-preview-rejects-unsupported-file",
            "unsupported_rejected=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-requires-proof-signals",
            "require_proofs",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-firewall-proof-json",
            "firewall-live-toggle-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-session-proof-json",
            "text-shortcuts-session-enable-proof.json",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-does-not-require-retired-textshortcuts-live-keystroke-proof-json",
            "text-shortcuts-live-keystroke-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-candidate-metadata-proof-json",
            "text-shortcuts-candidate-metadata-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-overlay-intent-proof-json",
            "text-shortcuts-overlay-intent-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-candidate-bubble-frame-proof-json",
            "text-shortcuts-candidate-bubble-frame-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-candidate-bubble-layout-proof-json",
            "text-shortcuts-candidate-bubble-layout-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-candidate-bubble-render-intent-proof-json",
            "text-shortcuts-candidate-bubble-render-intent-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-candidate-bubble-render-proof-json",
            "text-shortcuts-candidate-bubble-render-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-live-ibus-runtime-render-proof-json",
            "text-shortcuts-live-ibus-runtime-render-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-keyboard-shortcuts-roundtrip-proof-json",
            "keyboard-shortcuts-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-input-sources-roundtrip-proof-json",
            "input-sources-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-focus-arm-roundtrip-proof-json",
            "focus-arm-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-app-privacy-revoke-proof-json",
            "app-privacy-revoke-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-preview-open-render-proof-json",
            "preview-open-render-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-audio-output-proof-json",
            "audio-output-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-firewall-proof",
            "HONESTY GUARD: missing or failing live firewall toggle proof",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-capture-run-dir-reset-check",
            "capture harness resets only the exact validated dated run dir",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-dir-reset-is-scoped-to-arch-date",
            "refusing to reset unexpected hardware-gate run dir",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-session-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts session-enable proof",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-does-not-guard-retired-textshortcuts-live-keystroke-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts live keystroke proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-metadata-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts candidate metadata proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-overlay-intent-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts overlay-intent proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-frame-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-frame proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-layout-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-layout proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-render-intent-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render-intent proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-render-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render screenshot proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-runtime-render-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts live IBus runtime/render proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-keyboard-shortcuts-roundtrip-proof",
            "HONESTY GUARD: missing or failing Keyboard shortcuts roundtrip proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-input-sources-roundtrip-proof",
            "HONESTY GUARD: missing or failing Input sources roundtrip proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-focus-arm-roundtrip-proof",
            "HONESTY GUARD: missing or failing Focus arm roundtrip proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-app-privacy-revoke-proof",
            "HONESTY GUARD: missing or failing App privacy revoke proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-preview-open-render-proof",
            "HONESTY GUARD: missing or failing Preview open/render proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-fixture-core-uses-root-owned-ephemeral-state-root",
            "FIXTURE_STATE=/run/goblins-hwgate-fixture-state",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-uses-ephemeral-policy-state",
            "Environment=GOBLINS_OS_POLICY_STATE=/run/goblins-hwgate-fixture-state/policy",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-uses-ephemeral-apps-state",
            "Environment=GOBLINS_OS_APPS_DIR=/run/goblins-hwgate-fixture-state/apps",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-uses-ephemeral-ai-state",
            "Environment=GOBLINS_OS_AI_STATE=/run/goblins-hwgate-fixture-state/ai",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-pins-local-model",
            "CAPTURE_LOCAL_MODEL=llama3.2:1b",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-passes-local-model",
            "Environment=GOBLINS_OS_LOCAL_MODEL=llama3.2:1b",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-starts-loopback-model-forwarder",
            "start_capture_model_loopback",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-forwards-qemu-host-model",
            "TARGET = (\"10.0.2.2\", 11434)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-uses-dedicated-loopback-port",
            "LISTEN = (\"127.0.0.1\", 41134)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-uses-loopback-runtime-url",
            "CAPTURE_MODEL_RUNTIME_URL=http://127.0.0.1:41134",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-uses-loopback-contract-relay-url",
            "CAPTURE_MODEL_RELAY_URL=http://127.0.0.1:41135/v1/resident",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-keeps-local-model-warm",
            "CAPTURE_MODEL_KEEP_ALIVE=30m",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-starts-contract-relay",
            "start_capture_model_contract_relay",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-contract-relay-listens-on-dedicated-port",
            "LISTEN = (\"127.0.0.1\", 41135)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-direct-model-uses-keepalive",
            "\\\"keep_alive\\\":$CAPTURE_MODEL_KEEP_ALIVE_JSON",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-passes-loopback-runtime-url",
            "Environment=GOBLINS_OS_LOCAL_RUNTIME_URL=http://127.0.0.1:41134",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-passes-local-contract-relay-url",
            "Environment=GOBLINS_OS_LOCAL_MODEL_RELAY=http://127.0.0.1:41135/v1/resident",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-passes-local-model-keepalive",
            "Environment=GOBLINS_OS_LOCAL_MODEL_KEEP_ALIVE=30m",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-proof-helper-uses-fixed-release-proof-socket",
            "CORE_SOCKET=/run/goblins-os-core/release-proof/control.sock",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-proof-helper-has-finite-fixture-start-operation",
            "fixture-start) fixture_start ;;",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-proof-helper-has-finite-fixture-restore-operation",
            "fixture-restore) fixture_restore ;;",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-has-finite-runtime",
            "RuntimeMaxSec=1800",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-proof-unit-has-exact-writable-roots",
            "ReadWritePaths=/run/goblins-hwgate-core-proof /run/goblins-hwgate-fixture-state /run/goblins-hwgate-fixture-block",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-proof-unit-hides-home-by-default",
            "ProtectHome=tmpfs",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-proof-unit-binds-only-text-shortcuts-state-read-only",
            "BindReadOnlyPaths=-/var/home/goblin/.config/goblins-os",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-resident-uses-dedicated-runtime-socket",
            "Environment=GOBLINS_OS_RESIDENT_SOCKET=/run/goblins-hwgate-fixture-state/resident/resident.sock",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-core-stop-hook-runs-with-root-identity",
            "ExecStopPost=-+/etc/goblins-os/hardware-gate/goblins-hwgate-core-proof-operation fixture-core-stopped",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-proof-helper-pins-dedicated-fixture-resident-socket",
            "FIXTURE_RESIDENT_SOCKET=$FIXTURE_STATE/resident/resident.sock",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-proof-helper-does-not-mutate-production-resident-runtime",
            "/run/goblins-os/resident.sock",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-proof-helper-preserves-fixture-state-mount-root",
            "find \"$FIXTURE_STATE\" -mindepth 1 -delete",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-proof-helper-preserves-fixture-block-mount-root",
            "find \"$FIXTURE_BLOCK\" -mindepth 1 -delete",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/core-proof-operation.sh"),
            "capture-proof-helper-has-no-legacy-8788-listener",
            "8788",
        ),
        absent_check(
            root.join("os/iso/verify-config.toml"),
            "capture-fixture-services-have-no-core-url-override",
            "GOBLINS_OS_CORE_URL",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-session-has-no-core-url-override",
            "GOBLINS_OS_CORE_URL",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-records-direct-model-diagnostic",
            "/tmp/model-direct.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-records-contract-relay-diagnostic",
            "/tmp/model-contract-direct.json",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-records-runtime-log-diagnostics",
            "core_log_tail=",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-fixture-core-records-contract-log-diagnostics",
            "contract_log_tail=",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-serves-the-pinned-capture-model",
            "docker exec goblins-proof-ollama ollama pull",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-verifies-capture-model-manifest",
            "GOBLINS_OS_PROOF_OLLAMA_MODEL_MANIFEST_SHA256",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-replacement",
            "\"candidate_replacement\": \"on my way\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-no-live-overlay-claim",
            "\"live_overlay_claim\": \"false\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-overlay-intent-counts",
            "\"show_count\": \"2\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-frame-counts",
            "\"show_frame_count\": \"2\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-frame-style",
            "\"style_class\": \"gos-text-shortcuts-candidate\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-layout-count",
            "\"layout_count\": \"4\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-layout-clamp",
            "\"right_edge_clamped\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-layout-flip",
            "\"bottom_edge_flipped\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-layout-collapse",
            "\"hidden_frame_collapses\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-render-intent-count",
            "\"render_intent_count\": \"8\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-render-intent-fail-open",
            "\"sink_failure_fail_open\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-render-screenshot",
            "\"screenshot\": \"31-text-shortcuts-candidate-bubble-render.png\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-candidate-bubble-render-surface",
            "\"rendered_candidate_surface\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-runtime-render-screenshot",
            "\"screenshot\": \"32-text-shortcuts-live-ibus-runtime-render.png\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-runtime-render-surface",
            "\"surface\": \"goblins-textshortcuts-live-ibus-runtime-render\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-focused-callback",
            "\"focused_field_callback\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-render-intent-key-release",
            "\"key_release_preserved_candidate\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-render-intent-runtime-failure-cleanup",
            "\"runtime_failure_cleanup\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-secure-parent",
            "\"desktop_parent_contract\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-bounded-read",
            "\"desktop_file_bounded_read\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-watcher-reload",
            "\"live_watcher_reload\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-post-keystroke-roundtrip",
            "\"post_keystroke_roundtrip\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-password-popup-absence",
            "\"password_popup_absent\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-normal-stage-ledger-scope",
            "\"normal_stage_ledger_scoped\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-cursor-location-callback",
            "\"cursor_location_callback\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-pre-boundary-commit-absence",
            "\"pre_boundary_commit_absent\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-boundary-ledger-scope",
            "\"boundary_stage_ledger_scoped\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-single-boundary-commit",
            "\"boundary_stage_commit_count\": \"1\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-normal-stage-commit",
            "\"normal_stage_commit\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-commit-operation",
            "\"ibus_commit_operation\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-focused-readback",
            "\"focused_entry_readback\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-delivery",
            "\"ibus_commit_delivered\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-boundary-popup-hide",
            "\"boundary_popup_action\": \"hide-candidate\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-boundary-popup-commit-reason",
            "\"boundary_popup_reason\": \"committed\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-publication",
            "\"native_ibus_candidate_published\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-current-generation",
            "\"native_popup_generation_current\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-record-ordinal",
            "\"native_popup_record_ordinal\": \"[1-9][0-9]*\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-record-current-at-capture",
            "\"native_popup_record_current_at_capture\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-show-action",
            "\"native_popup_action\": \"show-candidate\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-cursor-rect",
            "\"native_popup_has_cursor_rect\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-expected-replacement",
            "\"native_popup_expected_replacement\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-hint",
            "\"native_popup_hint_published\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-screenshot-acknowledgement",
            "\"screenshot_capture_ack\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-native-ibus-renderer",
            "\"renderer\": \"native-ibus-lookup-table\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-rejects-textshortcuts-synthetic-overlay",
            "\"synthetic_overlay\": \"false\"",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-rejects-obsolete-text-input-v3-claim",
            "text_input_v3_commit",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-rejects-obsolete-rendered-accept-bubble-claim",
            "rendered_accept_bubble",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-rejects-obsolete-text-input-v3-claim",
            "text_input_v3_commit",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-rejects-obsolete-rendered-accept-bubble-claim",
            "rendered_accept_bubble",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-scopes-normal-textshortcuts-ledger",
            "normal_ledger_file=/tmp/gate-text-shortcuts-normal-stage-events.jsonl",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-scopes-pre-boundary-textshortcuts-ledger",
            "pre_boundary_ledger_file=/tmp/gate-text-shortcuts-pre-boundary-events.jsonl",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-scopes-boundary-textshortcuts-ledger",
            "boundary_ledger_file=/tmp/gate-text-shortcuts-boundary-stage-events.jsonl",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-selects-chronological-latest-popup",
            "| last) as $latest_popup",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-rejects-generation-sorted-popup-selection",
            "max_by(.generation // -1)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-binds-popup-record-ordinal",
            "native_popup_record_ordinal",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-rechecks-captured-popup-record",
            "captured_ordinal",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-captures-boundary-ledger-offset",
            "boundary_ledger_start=\"$(wc -l < \"$ledger_file\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-proves-pre-boundary-commit-absence",
            "pre_boundary_commit_absent=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-proves-single-boundary-commit",
            "boundary_stage_commit_count=1",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-binds-commit-to-process-key-event",
            ".callback == \"process-key-event\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-requires-exactly-one-commit-operation",
            "($commit_operations | length) == 1",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-proves-committed-boundary-popup-hide",
            "boundary_popup_action=hide-candidate&boundary_popup_reason=committed",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-fail-closes-boundary-ledger",
            "stage=boundary-ledger",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-waits-for-host-screenshot-ack",
            "wait_capture_ack \"$1\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-validates-screenshot-ack-sha256",
            "CAPTURE_ACK_PNG_SHA256=\"$png_sha256\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-orchestrator-binds-screenshot-sha256-to-live-proof",
            "screenshot_sha256=$screenshot_sha256",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-requires-successful-png-conversion",
            "check=True",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/png_validation.py"),
            "capture-driver-validates-complete-png",
            "def validate_png(path, expected_dimensions=None):",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/png_validation.py"),
            "capture-driver-bounds-decoded-png",
            "MAX_DECODED_PNG_BYTES",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/png_validation.py"),
            "capture-driver-rejects-unknown-critical-png-chunks",
            "unknown critical chunk",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/png_validation.py"),
            "capture-driver-requires-single-complete-zlib-stream",
            "or not decompressor.eof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/png_validation.py"),
            "capture-driver-rejects-trailing-zlib-stream-data",
            "or decompressor.unused_data",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-acknowledges-png-sha256",
            "png-sha256={png_sha256}",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-atomically-acknowledges-written-frame",
            "os.replace(temporary_ack, ack)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-fully-decodes-png-proof",
            "png_validation.py",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-binds-textshortcuts-screenshot-sha256",
            "TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_SCREENSHOT_SHA256",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-manifest-records-textshortcuts-screenshot-sha256",
            "text_shortcuts_live_ibus_runtime_render_screenshot_sha256",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "signoff-binds-textshortcuts-screenshot-proof-manifest-and-file",
            "recorded_manifest_screenshot_sha",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-binds-textshortcuts-screenshot-proof-manifest-and-file",
            "recorded_manifest_screenshot_sha",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-core-readiness-deferred",
            "\"core_readiness_flip\": \"live\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-password-refusal-proof",
            "\"password_refusal\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-passthrough-proof",
            "\"passthrough_actual\": \"hello.\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-keyboard-shortcut-readback",
            "\"shortcut_gsettings_readback\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-keyboard-modifier-readback",
            "\"modifier_gsettings_readback\": \"ctrl:nocaps\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-keyboard-roundtrip-restored",
            "\"roundtrip_restored\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-input-sources-readback",
            "\"sources_gsettings_readback\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-input-sources-switch",
            "\"switch_switched\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-input-sources-restored",
            "\"restore_sources\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-focus-active-mode-readback",
            "\"active_mode_gsettings_readback\": \"gate-work\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-focus-banner-suppressed",
            "\"notification_banners_after_activate\": \"false\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-focus-original-state-restored",
            "\"original_focus_state_restored\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-focus-per-app-claim-false",
            "\"per_app_breakthroughs_claim\": \"false\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-preview-papers-default",
            "\"pdf_default\": \"org.gnome.Papers.desktop\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-preview-loupe-default",
            "\"image_default\": \"org.gnome.Loupe.desktop\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-preview-pdf-screenshot",
            "\"pdf_screenshot\": \"29-preview-pdf-open.png\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-preview-image-screenshot",
            "\"image_screenshot\": \"30-preview-image-open.png\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-preview-unsupported-rejection",
            "\"unsupported_rejected\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-passthrough-proof",
            "passthrough_actual",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-passthrough-proof",
            "passthrough_actual",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-password-refusal-proof",
            "password_refusal",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-password-refusal-proof",
            "password_refusal",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-prints-qemu-startup-diagnostics",
            "QEMU startup diagnostics",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-prints-qemu-and-serial-logs",
            "qemu.log",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-qmp-startup-error-includes-last-error",
            "last connection error",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-exports-serial-log-for-stage-diagnostics",
            "GOS_SERIALLOG",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-waits-for-iso-boot-menu-marker",
            "wait_serial_contains(\"ISO boot menu\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-treats-iso-boot-handoff-marker-as-optional",
            "observe_serial_contains(\"ISO boot handoff\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-continues-to-framebuffer-stages-after-missing-booting-marker",
            "continuing to framebuffer stages",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-skips-grub-timeout",
            "key(\"ret\")",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-bounds-install-post-timeout",
            "GOS_INSTALL_POST_TIMEOUT",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-tags-install-post-timeout-exit",
            "exit_code=INSTALL_POST_TIMEOUT_EXIT",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-bounds-pre-kickstart-retries",
            "GOS_CAPTURE_MAX_ATTEMPTS",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-retries-only-install-timeout-exit",
            "driver_rc\" -eq \"$INSTALL_TIMEOUT_RC",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-resets-vm-state-between-install-timeout-attempts",
            "prepare_vm_state \"$attempt\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-preserves-attempt-logs-before-retry",
            "copy_capture_logs \"attempt-$attempt\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-waits-for-automated-kickstart-progress-diagnostics",
            "Anaconda automated kickstart progress",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-runs-kickstart-noninteractively",
            "text --non-interactive",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-pins-scratch-vda",
            "ignoredisk --only-use=vda",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-clears-only-scratch-vda",
            "clearpart --all --initlabel --disklabel=gpt --drives=vda",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-sets-vda-boot-drive",
            "bootloader --location=mbr --boot-drive=vda",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-puts-root-on-scratch-vda",
            "part / --fstype=xfs --label=root --grow --size=1024 --ondisk=vda",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-emits-install-done-marker",
            "GOBLINS_VERIFY_INSTALL_DONE",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-emits-post-install-target-diagnostics",
            "GOBLINS_HWGATE_POST_DIAG_BEGIN",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-installs-firstboot-diagnostics-service",
            "goblins-hwgate-firstboot-diagnostics.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-installs-hwgate-helpers-under-etc",
            "/etc/goblins-os/hardware-gate",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-confirms-etc-hwgate-helpers-installed",
            "GOBLINS_HWGATE_ETC_HELPERS_INSTALLED",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-firstboot-diagnostics-directly-wanted-by-multi-user",
            "multi-user.target.wants/goblins-hwgate-firstboot-diagnostics.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-firstboot-diagnostics-directly-wanted-by-graphical",
            "graphical.target.wants/goblins-hwgate-firstboot-diagnostics.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-installs-session-orchestrator-service",
            "goblins-hwgate-session-orchestrator.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-has-autostart-fallback",
            "/etc/xdg/autostart/goblins-hwgate-session-orchestrator.desktop",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-autostart-execs-helper",
            "Exec=/etc/goblins-os/hardware-gate/goblins-hwgate-session-orchestrator",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-emits-started-marker",
            "GOBLINS_HWGATE_SESSION_ORCHESTRATOR_STARTED",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-emits-firstboot-download-marker",
            "GOBLINS_HWGATE_FIRSTBOOT_HELPER_DOWNLOADED",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-waits-for-published-script",
            "download_with_wait orchestrator.sh /tmp/gos-orchestrator 600",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-is-session-target-wanted",
            "99-goblins-hwgate-session-orchestrator.conf",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-is-enabled-for-user-default-target",
            "systemctl --global enable goblins-hwgate-session-orchestrator.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-user-service-has-default-target-install",
            "WantedBy=default.target",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-user-service-has-wayland-env",
            "Environment=WAYLAND_DISPLAY=wayland-0",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-user-service-has-display-env",
            "Environment=DISPLAY=:0",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-user-service-has-proof-timeout",
            "ExecStart=/etc/goblins-os/hardware-gate/goblins-hwgate-session-orchestrator\nTimeoutStartSec=3900",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-imports-dbus-activation-env",
            "dbus-update-activation-environment --systemd DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-imports-systemd-user-env",
            "systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-installs-session-orchestrator-system-starter",
            "goblins-hwgate-session-orchestrator-starter.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-orders-after-core",
            "After=display-manager.service gdm.service systemd-user-sessions.service goblins-os-core.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-starts-core",
            "Wants=goblins-os-core.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-has-bounded-timeout",
            "ExecStart=/etc/goblins-os/hardware-gate/goblins-hwgate-start-session-orchestrator\nTimeoutStartSec=360",
        ),
        absent_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-has-no-graphical-ordering-cycle",
            "After=graphical.target display-manager.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-directly-wanted-by-graphical",
            "graphical.target.wants/goblins-hwgate-session-orchestrator-starter.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-waits-for-user-bus",
            "for _ in $(seq 1 120); do",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-reports-user-bus-ready",
            "GOBLINS_HWGATE_SESSION_BUS_READY",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-session-orchestrator-starter-requests-user-service",
            "GOBLINS_HWGATE_SESSION_ORCHESTRATOR_START_REQUESTED",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-firstboot-diagnostics-prints-default-target",
            "default_target=$(systemctl get-default 2>&1)",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-firstboot-diagnostics-statuses-gdm",
            "systemctl --no-pager --full status graphical.target display-manager.service gdm.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-diagnostics-captures-gdm-config",
            "gdm_custom_conf_begin",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-diagnostics-captures-goblin-accountsservice",
            "accountsservice_goblin_begin",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-diagnostics-reports-shadow-state-without-hash",
            "goblin_shadow_state=invalid-star",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-firstboot-diagnostics-captures-gdm-journal",
            "journalctl -b --no-pager",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-firstboot-diagnostics-does-not-change-target",
            "WantedBy=multi-user.target graphical.target",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-firstboot-diagnostics-done-marker",
            "GOBLINS_HWGATE_DIAG_DONE",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-restores-graphical-target-after-text-install",
            "systemctl set-default graphical.target",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-asserts-restored-graphical-target",
            "readlink /etc/systemd/system/default.target | grep -Fq graphical.target",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-emits-graphical-target-restored-marker",
            "GOBLINS_HWGATE_GRAPHICAL_TARGET_RESTORED",
        ),
        absent_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-lets-bib-inject-ostreecontainer",
            "ostreecontainer --url",
        ),
        absent_check(
            root.join("os/iso/config.toml"),
            "release-iso-config-keeps-install-done-marker-out",
            "GOBLINS_VERIFY_INSTALL_DONE",
        ),
        absent_check(
            root.join("os/iso/verify-config.toml"),
            "verify-config-does-not-write-hwgate-helpers-to-image-owned-usr",
            "/usr/libexec/goblins-hwgate",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-does-not-pretend-oemdrv-overrides-embedded-osbuild-ks",
            "make-oemdrv.sh",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-allows-explicit-verification-iso-path",
            "GOBLINS_OS_CAPTURE_ISO",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-requires-verification-iso-marker",
            "GOBLINS_VERIFY_INSTALL_DONE",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-requires-hardware-gate-session-orchestrator",
            "goblins-hwgate-session-orchestrator",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-refuses-human-safe-release-iso",
            "public release ISO is intentionally human-safe",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-does-not-fake-anaconda-disk-selection-by-click",
            "Anaconda destination disk selected",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-uses-documented-qmp-absolute-axis-range",
            "ABS_MAX = 0x7fff",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-clamps-qmp-absolute-axis",
            "def abs_axis(value):",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/qmp-capture.py"),
            "manual-qmp-helper-uses-documented-qmp-absolute-axis-range",
            "ABS_MAX = 0x7fff",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/qmp-capture.py"),
            "manual-qmp-helper-clamps-qmp-absolute-axis",
            "def abs_axis(value):",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-assigns-stable-display-device-id",
            "virtio-gpu-pci,id=video0",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-exports-qmp-display-device-route",
            "GOS_QMP_DISPLAY_DEVICE=video0",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-routes-pointer-events-to-display-device",
            "device\": DISPLAY_DEVICE",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/qmp-capture.py"),
            "manual-qmp-helper-routes-pointer-events-to-display-device",
            "device\": DISPLAY_DEVICE",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-fail-closes-on-qmp-command-errors",
            "QMP command",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-saves-automated-kickstart-debug-frame",
            "Anaconda automated kickstart progress",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-requires-kickstart-install-post-marker",
            "\"kickstart install post\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-pins-kickstart-install-post-needle",
            "GOBLINS_VERIFY_INSTALL_DONE",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-waits-for-first-boot-desktop-diagnostics",
            "wait_stage(\"first boot desktop\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-observes-first-boot-hardware-diagnostics",
            "observe_serial_contains(\"first boot hardware diagnostics\", \"GOBLINS_HWGATE_DIAG_DONE\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-probes-graphical-vts",
            "def probe_graphical_vts():",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-probes-first-boot-vt2",
            "\"ctrl+alt+f2\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-probes-legacy-graphical-vt7",
            "\"ctrl+alt+f7\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-saves-first-boot-vt-debug-frame",
            "first boot vt f2",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-keeps-vt-probe-off-firstboot-happy-path",
            "first boot setup failed before helper callback; collecting VT diagnostics",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-completes-first-boot-through-root-release-proof-capability",
            "first boot setup: completing offline path through the root release-proof capability",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-does-not-use-alt-f2-command-injection",
            "key(\"alt+f2\")",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-requires-authenticated-firstboot-helper-download",
            "wait_helper_event(event_reader, \"firstboot-unlock.sh\", 180)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-reports-session-orchestrator-starter-marker",
            "session orchestrator starter",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-saves-post-first-boot-release-proof-unlock-debug-frame",
            "post first boot release-proof unlock",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-harness-serves-firstboot-unlock-helper",
            "firstboot-unlock.sh",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-harness-defers-orchestrator-until-driver-publishes",
            "GOS_ORCHESTRATOR_DEST",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-publishes-orchestrator-after-firstboot-callback",
            "publish_orchestrator()",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-requires-authenticated-orchestrator-download",
            "wait_helper_event(event_reader, \"orchestrator.sh\", 180)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-sets-private-mode",
            "/v1/privacy",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-completes-installer",
            "/v1/installer/complete",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-unlocks-session",
            "/v1/session/unlock",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-callbacks-to-host",
            "/ready/FIRSTBOOT_UNLOCK",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-callbacks-failure-to-host",
            "/failed/FIRSTBOOT_UNLOCK?stage=$CURRENT_STAGE&rc=$rc",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-emits-sanitized-stage-status",
            "status=fail curl_rc=$curl_rc http_status=$http_status",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-emits-safe-core-unit-state",
            "GOBLINS_HWGATE_CORE_UNIT_STATE property=$property value=$value",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-proves-production-core-unit",
            "prove_production_core_unit",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-production-unit-fragment",
            "CORE_UNIT_FRAGMENT=/usr/lib/systemd/system/goblins-os-core.service",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-active-running-main-pid",
            "--property=MainPID",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-active-unit-state",
            "[ \"$active\" = active ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-running-unit-substate",
            "[ \"$substate\" = running ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-live-main-pid",
            "[ \"$main_pid\" -gt 1 ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-inspects-core-unit-dropins",
            "--property=DropInPaths",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-exact-unit-fragment",
            "[ \"$fragment\" = \"$CORE_UNIT_FRAGMENT\" ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-allows-only-fedora-timeout-dropin",
            "[ \"$dropins\" = \"$CORE_TRUSTED_DROPIN\" ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-pins-fedora-timeout-dropin",
            "CORE_TRUSTED_DROPIN=/usr/lib/systemd/system/service.d/10-timeout-abort.conf",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-pins-fedora-timeout-dropin-sha256",
            "CORE_TRUSTED_DROPIN_SHA256=ae6b234f92bc22f1201a7572b59b454c9809f33c80d13f361b9674e1801acc37",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-trusted-dropin-owner-mode",
            "[ \"$dropin_owner_mode\" = root:root:644 ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-trusted-dropin-sha256",
            "[ \"$dropin_sha256\" = \"$CORE_TRUSTED_DROPIN_SHA256\" ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-hashes-trusted-dropin-bytes",
            "sha256sum \"$CORE_TRUSTED_DROPIN\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-reads-dropin-package-provenance",
            "rpm -qf --qf '%{NAME}' \"$CORE_TRUSTED_DROPIN\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-systemd-package-provenance",
            "[ \"$dropin_package\" = systemd ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-effective-timeout-abort-mode",
            "[ \"$timeout_stop_failure_mode\" = abort ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-effective-strict-protection",
            "[ \"$protect_system\" = strict ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-effective-exact-writable-paths",
            "[ \"$read_write_paths\" = \"$CORE_READ_WRITE_PATHS\" ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-binds-main-pid-to-installed-core-inode",
            "stat -Lc '%d:%i' \"/proc/$main_pid/exe\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-installed-core-inode-match",
            "[ \"$running_executable\" = \"$installed_executable\" ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-binds-capability-listeners-to-main-pid",
            "\"socket:[$socket_inode]\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-exact-capability-directory-count",
            "[ \"$entry_count\" = 17 ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-exact-capability-slug-count",
            "[ \"${#CORE_CAPABILITY_SLUGS[@]}\" = 17 ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-rejects-duplicate-capability-slugs",
            "[ -z \"${seen_slugs[$slug]+present}\" ]",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-capability-directory-owner-group-mode",
            "goblins-os:$expected_group:2750",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-capability-socket-owner-group-mode",
            "goblins-os:$expected_group:660",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-listening-unix-socket-row",
            "$4 == \"00010000\" && $5 == \"0001\" && $6 == \"01\" && $8 == path",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-proves-all-capability-listeners",
            "prove_production_capability_inventory \"$main_pid\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-checks-core-mount-namespace",
            "nsenter --target \"$main_pid\" --mount --",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "container-provides-firstboot-dropin-hasher",
            "command -v sha256sum",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "container-provides-firstboot-rpm-provenance-query",
            "command -v rpm",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-runtime-capability-root-writable",
            "mount_is_effectively_writable \"$main_pid\" /run/goblins-os-core",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-requires-voice-work-writable",
            "mount_is_effectively_writable \"$main_pid\" /var/lib/goblins-os/voice/work",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-emits-sanitized-production-unit-proof",
            "GOBLINS_HWGATE_CORE_PRODUCTION_UNIT status=pass identity=systemd-main-pid dropin=vendor-sha256 listeners=17 runtime_mount=rw voice_work_mount=rw",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-uses-fixed-shipped-root",
            "Path::new(DEFAULT_VOICE_DIR).join(\"work\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-uses-create-new",
            ".create_new(true)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-opens-every-directory-component-no-follow",
            "directory.open_dir_nofollow(name)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-opens-probe-no-follow",
            ".follow(FollowSymlinks::No)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-validates-exact-owner-and-mode",
            "metadata.mode() & 0o7777 != REQUIRED_VOICE_WORK_MODE",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-fsyncs-probe",
            "file.sync_all()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-unlinks-probe",
            "work.remove_file(name)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-fsyncs-held-directory",
            "sync_voice_work_directory(work)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "release-proof-voice-storage-rejects-symlinked-components-in-tests",
            "fn release_proof_rejects_final_and_intermediate_directory_symlinks()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/control_plane.rs"),
            "release-proof-capability-allows-only-fixed-voice-storage-probe",
            "(POST, \"/v1/release-proof/storage/voice\")",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-validates-voice-storage-response",
            ".ok == true and .storage == \"voice-work\" and .create_new == true and .write == true and .fsync == true and .unlink == true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-emits-sanitized-voice-storage-proof",
            "GOBLINS_HWGATE_FIRSTBOOT_STAGE stage=voice-storage status=pass curl_rc=0 http_status=200 create_new=true write=true fsync=true unlink=true",
        ),
        ordered_contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-proves-production-unit-before-state-mutation",
            "prove_production_core_unit\nprove_voice_storage\npost_json privacy",
            "post_json installer-complete",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-cannot-launch-a-manual-setpriv-core",
            "setpriv --reuid=goblins-os",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-cannot-use-fixture-core",
            "goblins-hwgate-fixture-core",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-does-not-upload-journal",
            "journalctl",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-requires-firstboot-release-proof-unlock-callback",
            "first boot release-proof unlock callback",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-fails-fast-on-authenticated-firstboot-failure-event",
            "event.get(\"kind\") == \"failed\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-fails-fast-on-firstboot-failure-serial",
            "failure_serial_marker = \"GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_FAILED\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-requires-firstboot-guest-completion-serial",
            "success_serial_marker = \"GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_DONE\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-requires-firstboot-authenticated-success-event",
            "success_event_seen = event.get(\"values\") == {\"status\": \"pass\"}",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-scopes-firstboot-events-to-current-attempt",
            "event_reader = IncrementalEventReader(EVENTS)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-scopes-firstboot-serial-to-current-attempt",
            "firstboot_serial_start_pos = safe_file_size(SERIALLOG, SERIAL_MAX_BYTES)",
        ),
        ordered_contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-prioritizes-firstboot-failure-before-success",
            "event.get(\"kind\") == \"failed\"",
            "if success_event_seen and success_serial_seen:",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-host-injects-private-bearer-token-via-fw-config",
            "-fw_cfg \"name=opt/goblins/capture-token,file=$TOKEN_FILE\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-host-starts-private-event-receiver",
            "python3 \"$HERE/drive-capture.py\" --event-receiver",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-receiver-authenticates-bearer-in-constant-time",
            "hmac.compare_digest(authorization, expected)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-receiver-restricts-clients-to-loopback",
            "ipaddress.ip_address(client_address[0]).is_loopback",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-event-reader-rejects-missing-or-reordered-events",
            "if event[\"sequence\"] != self.expected_sequence:",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-event-reader-bounds-retained-events",
            "if len(self.queued_events) + len(events) > EVENT_MAX_COUNT:",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-helper-wait-uses-selective-event-removal",
            "event_reader.take_matching(\"helper\", helper_name)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-event-reader-removes-only-selected-event",
            "return self.queued_events.pop(index)",
        ),
        ordered_contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-channel-selftest-retains-coalesced-firstboot-success",
            "wait_helper_event(coalesced_firstboot_reader, \"firstboot-unlock.sh\", 0.5)",
            "assert wait_firstboot_unlock_result(",
        ),
        ordered_contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-channel-selftest-retains-coalesced-orchestrator-events",
            "wait_helper_event(coalesced_orchestrator_reader, \"orchestrator.sh\", 0.5)",
            "remaining = coalesced_orchestrator_reader.poll()",
        ),
        ordered_contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-channel-selftest-preserves-orchestrator-event-order",
            "(2, \"ready\", \"ORCH_START\"),",
            "(3, \"ready\", \"06-onboarding\"),",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-bounds-existing-serial-log-before-firstboot",
            "safe_file_size(SERIALLOG, SERIAL_MAX_BYTES)",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-channel-has-adversarial-self-test",
            "def _capture_channel_self_test():",
        ),
        absent_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-host-does-not-run-a-generic-http-file-server",
            "http.server",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-uses-fixed-release-proof-socket",
            "CORE_PROOF_SOCKET=/run/goblins-os-core/release-proof/control.sock",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/firstboot-unlock.sh"),
            "firstboot-unlock-uses-unix-socket-transport",
            "curl --unix-socket \"$CORE_PROOF_SOCKET\"",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "firstboot-unlock-download-is-root-only",
            "download_with_wait firstboot-unlock.sh /run/goblins-hwgate-root/firstboot 15",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "core-proof-operation-download-is-bounded",
            "download_with_wait core-proof-operation.sh /run/goblins-hwgate-root/core-proof-operation 15",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "firstboot-unlock-root-starter-requests-core",
            "GOBLINS_HWGATE_CORE_START_REQUESTED",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "firstboot-unlock-root-starter-freshly-restarts-production-core",
            "systemctl restart goblins-os-core.service",
        ),
        contains_check(
            root.join("os/iso/verify-config.toml"),
            "firstboot-unlock-publishes-root-owned-marker",
            "install -m 0644 -o root -g root /dev/null /run/goblins-hwgate-firstboot-unlocked",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "installed-selftest-exercises-release-proof-firstboot-sequence",
            "verification first boot -> privacy=$firstboot_privacy_code installer=$firstboot_installer_code session=$firstboot_session_code",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "installed-selftest-persists-firstboot-privacy",
            "persisted_offline=$(cat \"$GOBLINS_OS_OFFLINE_PATH\"",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "installed-selftest-persists-firstboot-installer-mode",
            "persisted_installer_mode=$(jq -r '.mode // empty' \"$GOBLINS_OS_INSTALLER_STATE/first-boot.json\"",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "installed-selftest-persists-firstboot-session-mode",
            "persisted_session_mode=$(jq -r '.mode // empty' \"$GOBLINS_OS_SESSION_STATE/gate.json\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-prints-diagnostic-frame-samples",
            "diagnostic framebuffer samples",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-saves-debug-frame-artifacts",
            "_debug-",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-defaults-to-ci-kvm-supported-smp",
            "GOBLINS_OS_QEMU_CPUS:-2",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-passes-configured-qemu-smp",
            "-smp \"$QEMU_SMP\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-copies-failure-capture-logs",
            "_capture-logs",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-uploads-artifact-on-failure",
            "if: always()",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-uploads-only-current-screenshot-run",
            "os/screenshots/hardware-gate/${{ matrix.arch }}/${{ inputs.run_date }}/",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-firewall-proof",
            "firewall_live_toggle_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-textshortcuts-candidate-proof",
            "text_shortcuts_candidate_metadata_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-textshortcuts-overlay-intent-proof",
            "text_shortcuts_overlay_intent_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-textshortcuts-candidate-bubble-frame-proof",
            "text_shortcuts_candidate_bubble_frame_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-textshortcuts-candidate-bubble-layout-proof",
            "text_shortcuts_candidate_bubble_layout_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-textshortcuts-candidate-bubble-render-intent-proof",
            "text_shortcuts_candidate_bubble_render_intent_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-textshortcuts-candidate-bubble-render-proof",
            "text_shortcuts_candidate_bubble_render_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-textshortcuts-live-ibus-runtime-render-proof",
            "text_shortcuts_live_ibus_runtime_render_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-keyboard-shortcuts-roundtrip-proof",
            "keyboard_shortcuts_roundtrip_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-input-sources-roundtrip-proof",
            "input_sources_roundtrip_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-focus-arm-roundtrip-proof",
            "focus_arm_roundtrip_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-app-privacy-revoke-proof",
            "app_privacy_revoke_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-preview-open-render-proof",
            "preview_open_render_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-manifest-links-audio-output-proof",
            "audio_output_proof",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-rejects-legacy-non-arch-screenshot-roots",
            "Legacy/non-shipping screenshot roots",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-requires-arch-dated-screenshot-root",
            "os/screenshots/hardware-gate/<arch>/<YYYY-MM-DD>/",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-blocks-end-at-next-heading",
            "signoff_block_from_line",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-block-parser-stops-at-heading",
            "/^## / { exit }",
        ),
        absent_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-block-parser-no-fixed-tail-window",
            "start + 120",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-explicit-docker-platform",
            "GOBLINS_OS_DOCKER_PLATFORM",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-platform-recorded-in-manifest",
            "\"docker_platform\": \"$DOCKER_PLATFORM\"",
        ),
        contains_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-emulated-docker-rust-preflight",
            "emulation cannot run rustc",
        ),
        absent_check(
            root.join("os/iso/build-iso.sh"),
            "iso-builder-noninteractive-no-tty-flags",
            "--rm -it",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-shares-container-runtime-with-iso-builder",
            "GOBLINS_OS_CONTAINER_RUNTIME=\"$CONTAINER_RUNTIME\"",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-host-runtime-docker-only",
            "GOBLINS_OS_CONTAINER_RUNTIME must be docker",
        ),
        absent_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-no-host-podman-option-copy",
            "docker or podman",
        ),
        absent_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-no-podman-sudo-env",
            "GOBLINS_OS_PODMAN_SUDO",
        ),
        absent_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-no-sudo-podman-command",
            "sudo podman",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "installer-iso-workflow-builds-docker-image",
            "docker/build-push-action@53b7df96c91f9c12dcc8a07bcb9ccacbed38856a",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "installer-iso-workflow-loads-local-image",
            "load: true",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "installer-iso-workflow-generates-evidence-from-docker-image",
            "docker run --rm",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-x86-qemu",
            "qemu-system-x86_64",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-aarch64-qemu",
            "qemu-system-aarch64",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-native-kvm-required",
            "QEMU_ACCEL must be kvm",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-kvm-device-required",
            "/dev/kvm",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-min-free-space-required",
            "MIN_HOST_FREE_GB",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-repo-free-space-preflight",
            "Repository filesystem needs at least",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-vm-scratch-free-space-preflight",
            "VM scratch filesystem needs at least",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-container-runtime-health-timeout",
            "CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-container-runtime-health-preflight",
            "Checking $CONTAINER_RUNTIME health",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-container-runtime-hang-copy",
            "did not answer within",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-preflight-only-env",
            "PREFLIGHT_ONLY",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-preflight-only-pass-copy",
            "Preflight passed for native $ARCH release runner.",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-artifact-only-preflight-copy",
            "Docker artifact-only preflight passed for $ARCH on $HOST_ARCH; not release proof.",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-preflight-does-not-claim-proof",
            "No image, ISO, SBOM, screenshot, or signoff artifact was generated.",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-preflight-only-command",
            "PREFLIGHT_ONLY=1 GOBLINS_OS_ARCH",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-preflight-not-shipping-proof",
            "does not create shipping artifacts or satisfy proof by itself",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-aarch64-virt-kvm-machine",
            "virt,accel=$QEMU_ACCEL,gic-version=max",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-aarch64-pflash-code",
            "if=pflash,format=raw,readonly=on,file=$AARCH64_UEFI_CODE",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-aarch64-pflash-vars",
            "if=pflash,format=raw,file=$AARCH64_UEFI_VARS",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-aarch64-vars-template",
            "AARCH64_UEFI_VARS_TEMPLATE",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-linux-host-required",
            "External display-backed gate requires a native Linux host with Docker and QEMU",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-native-arch-required",
            "must be produced on a native $ARCH Linux runner",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-qemu-only-required-when-running-qemu",
            "REQUIRED_CMDS+=(\"$QEMU_BIN\" qemu-img)",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-artifact-only-mode-warns-proof-required",
            "RUN_QEMU=0: built and verified artifacts only. Shipping still requires a later display-backed VM run and screenshot proof.",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-artifact-only-does-not-create-screenshot-target",
            "Screenshot target: not created for artifact-only Docker run",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-arch-screenshot-path",
            "os/screenshots/hardware-gate/$ARCH/$DATE",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-iso-sha-verify-is-directory-relative",
            "sha_dir/$artifact",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-iso-sha-verify-rejects-path-escape",
            "artifact\" != */*",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-iso-sha-verify-compares-digest",
            "actual\" == \"$expected",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-release-evidence-dir",
            "RELEASE_EVIDENCE_DIR",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-generates-release-evidence",
            "--release-evidence /out",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-verifies-rpm-release-evidence",
            "rpm-packages.tsv",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-verifies-rpm-release-evidence-architecture",
            "rpm_sbom_arch_matches",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "hardware-gate-verifies-release-evidence-trademark-posture",
            "trademark_posture",
        ),
        contains_check(
            root.join("os/hardware-gate/rpm-sbom-arch.sh"),
            "hardware-gate-rpm-sbom-arch-validator",
            "expected_arch",
        ),
        contains_check(
            root.join("os/hardware-gate/rpm-sbom-arch.sh"),
            "hardware-gate-rpm-sbom-allows-noarch",
            "$3 != \"noarch\"",
        ),
        contains_check(
            root.join("os/hardware-gate/rpm-sbom-arch.sh"),
            "hardware-gate-rpm-sbom-allows-gpg-pubkey-none",
            "$1 == \"gpg-pubkey\" && $3 == \"(none)\"",
        ),
        contains_check(
            root.join("os/hardware-gate/rpm-sbom-arch.sh"),
            "hardware-gate-rpm-sbom-rejects-empty",
            "RPM SBOM has no package rows",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-verifies-rpm-sbom-architecture",
            "rpm_sbom_arch_matches",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-verifies-rpm-sbom-architecture",
            "rpm_sbom_arch_matches os/signoff-proofs/sbom/${{ matrix.arch }}/rpm-packages.tsv ${{ matrix.arch }}",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-verifies-release-evidence-trademark-posture",
            "release evidence manifest records trademark posture",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-verifies-rpm-sbom-architecture",
            "RPM SBOM package architectures match",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-sha256-verification-helper",
            "check_sha256_file",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-verifies-iso-sha256-file",
            "ISO SHA256 verifies",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-iso-manifest-records-artifact-name",
            "ISO manifest records ISO name",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-iso-manifest-records-sha-file",
            "ISO manifest records SHA file",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-rejects-local-installer-payload-ref",
            "installer payload tracks a local-only Docker/test registry",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-names-missing-screenshot-paths",
            "print_missing_screenshot_paths",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-reports-latest-incomplete-screenshot-run",
            "Latest incomplete",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-reports-expected-screenshot-proof-files",
            "Expected screenshot proof files",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-fully-validates-screenshot-png",
            "--check-png",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-screenshot-proof-manifest",
            "proof-manifest.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-separates-verification-proof-from-public-release-iso-artifact-checks",
            "print_verification_and_public_release_iso_detail",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-documents-public-release-iso-artifact-split",
            "public release ISO artifacts are checked separately",
        ),
        absent_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-does-not-require-public-release-iso-screenshot-alignment",
            "public release ISO-aligned screenshot run",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-firewall-live-toggle-proof",
            "firewall_live_toggle_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-session-proof",
            "text_shortcuts_session_enable_proof_passes",
        ),
        absent_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-does-not-require-retired-textshortcuts-live-keystroke-proof",
            "text_shortcuts_live_keystroke_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-candidate-metadata-proof",
            "text_shortcuts_candidate_metadata_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-overlay-intent-proof",
            "text_shortcuts_overlay_intent_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-candidate-bubble-frame-proof",
            "text_shortcuts_candidate_bubble_frame_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-candidate-bubble-layout-proof",
            "text_shortcuts_candidate_bubble_layout_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-candidate-bubble-render-intent-proof",
            "text_shortcuts_candidate_bubble_render_intent_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-candidate-bubble-render-proof",
            "text_shortcuts_candidate_bubble_render_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-live-ibus-runtime-render-proof",
            "text_shortcuts_live_ibus_runtime_render_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-keyboard-shortcuts-roundtrip-proof",
            "keyboard_shortcuts_roundtrip_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-input-sources-roundtrip-proof",
            "input_sources_roundtrip_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-multi-display-apply-proof",
            "multi_display_apply_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-focus-arm-roundtrip-proof",
            "focus_arm_roundtrip_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-app-privacy-revoke-proof",
            "app_privacy_revoke_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-preview-open-render-proof",
            "preview_open_render_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-audio-output-proof",
            "audio_output_proof_passes",
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "screenshot_run_is_complete",
            "shipping-status-complete-run-requires-keyboard-shortcuts-roundtrip-proof",
            r#"keyboard_shortcuts_roundtrip_proof_passes "$run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "screenshot_run_is_complete",
            "shipping-status-complete-run-requires-input-sources-roundtrip-proof",
            r#"input_sources_roundtrip_proof_passes "$run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "screenshot_run_is_complete",
            "shipping-status-complete-run-requires-multi-display-apply-proof",
            r#"multi_display_apply_proof_passes "$run_dir/$MULTI_DISPLAY_APPLY_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "screenshot_run_is_complete",
            "shipping-status-complete-run-requires-focus-arm-roundtrip-proof",
            r#"focus_arm_roundtrip_proof_passes "$run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "screenshot_run_is_complete",
            "shipping-status-complete-run-requires-app-privacy-revoke-proof",
            r#"app_privacy_revoke_proof_passes "$run_dir/$APP_PRIVACY_REVOKE_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "screenshot_run_is_complete",
            "shipping-status-complete-run-requires-preview-open-render-proof",
            r#"preview_open_render_proof_passes "$run_dir/$PREVIEW_OPEN_RENDER_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "screenshot_run_is_complete",
            "shipping-status-complete-run-requires-audio-output-proof",
            r#"audio_output_proof_passes "$run_dir/$AUDIO_OUTPUT_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "print_missing_screenshot_paths",
            "shipping-status-missing-list-includes-keyboard-shortcuts-roundtrip-proof",
            r#"echo "  $run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "print_missing_screenshot_paths",
            "shipping-status-missing-list-includes-input-sources-roundtrip-proof",
            r#"echo "  $run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "print_missing_screenshot_paths",
            "shipping-status-missing-list-includes-multi-display-apply-proof",
            r#"echo "  $run_dir/$MULTI_DISPLAY_APPLY_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "print_missing_screenshot_paths",
            "shipping-status-missing-list-includes-focus-arm-roundtrip-proof",
            r#"echo "  $run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "print_missing_screenshot_paths",
            "shipping-status-missing-list-includes-app-privacy-revoke-proof",
            r#"echo "  $run_dir/$APP_PRIVACY_REVOKE_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "print_missing_screenshot_paths",
            "shipping-status-missing-list-includes-preview-open-render-proof",
            r#"echo "  $run_dir/$PREVIEW_OPEN_RENDER_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "print_missing_screenshot_paths",
            "shipping-status-missing-list-includes-audio-output-proof",
            r#"echo "  $run_dir/$AUDIO_OUTPUT_PROOF""#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "print_arch_next_steps",
            "shipping-status-next-steps-lists-audio-output-proof",
            r#"os/screenshots/hardware-gate/$arch/<date>/$AUDIO_OUTPUT_PROOF"#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "signoff_block_required_proof_is_complete",
            "shipping-status-signoff-requires-preview-open-render-proof",
            r#"signoff_block_contains "$block" "^- Preview open/render checked: yes" || return 1"#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "signoff_block_required_proof_is_complete",
            "shipping-status-signoff-requires-audio-output-proof",
            r#"signoff_block_contains "$block" "^- Audio output checked: yes" || return 1"#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "signoff_block_required_proof_is_complete",
            "shipping-status-signoff-requires-focus-arm-roundtrip-proof",
            r#"signoff_block_contains "$block" "^- Focus arm roundtrip checked: yes" || return 1"#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "signoff_block_required_proof_is_complete",
            "shipping-status-signoff-requires-multi-display-apply-proof",
            r#"signoff_block_contains "$block" "^- Multi-display apply checked: yes" || return 1"#,
        ),
        shell_function_contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "signoff_block_required_proof_is_complete",
            "shipping-status-signoff-requires-app-privacy-revoke-proof",
            r#"signoff_block_contains "$block" "^- App privacy revoke checked: yes" || return 1"#,
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-firewall-proof-filename",
            "firewall-live-toggle-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-session-proof-filename",
            "text-shortcuts-session-enable-proof.json",
        ),
        absent_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-does-not-pin-retired-textshortcuts-live-keystroke-proof-filename",
            "text-shortcuts-live-keystroke-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-candidate-metadata-proof-filename",
            "text-shortcuts-candidate-metadata-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-candidate-bubble-frame-proof-filename",
            "text-shortcuts-candidate-bubble-frame-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-candidate-bubble-layout-proof-filename",
            "text-shortcuts-candidate-bubble-layout-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-candidate-bubble-render-intent-proof-filename",
            "text-shortcuts-candidate-bubble-render-intent-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-candidate-bubble-render-proof-filename",
            "text-shortcuts-candidate-bubble-render-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-live-ibus-runtime-render-proof-filename",
            "text-shortcuts-live-ibus-runtime-render-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-candidate-bubble-render-screenshot-filename",
            "31-text-shortcuts-candidate-bubble-render.png",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-live-ibus-runtime-render-screenshot-filename",
            "32-text-shortcuts-live-ibus-runtime-render.png",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-keyboard-shortcuts-roundtrip-proof-filename",
            "keyboard-shortcuts-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-input-sources-roundtrip-proof-filename",
            "input-sources-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-multi-display-apply-proof-filename",
            "multi-display-apply-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-focus-arm-roundtrip-proof-filename",
            "focus-arm-roundtrip-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-app-privacy-revoke-proof-filename",
            "app-privacy-revoke-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-preview-open-render-proof-filename",
            "preview-open-render-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-audio-output-proof-filename",
            "audio-output-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-preview-pdf-screenshot-filename",
            "29-preview-pdf-open.png",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-preview-image-screenshot-filename",
            "30-preview-image-open.png",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-preview-uses-fixed-string-xdg-open-check",
            r#"rg -Fq 'isolated_session_command(\"xdg-open\")'"#,
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-hotspot-uses-fixed-string-policy-check",
            r#"rg -Fq 'policy_state_for_control(\"settings-control\")'"#,
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-switch-uses-fixed-string-atspi-check",
            r#"rg -Fq \"import('gi://Atspi')\""#,
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-focus-uses-fixed-string-mode-check",
            "rg -Fq 'modes.find(entry => entry.id === activeMode)'",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-firewall-proof-disable-state",
            "\"disable_active\"",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-firewall-proof-enable-state",
            "\"enable_active\"",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-reports-legacy-screenshot-roots",
            "Legacy/non-shipping screenshot roots ignored by architecture proof gate",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-block-stops-at-next-heading",
            "NR == start { print; next } /^## / { exit }",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-prints-per-arch-next-command",
            "Next evidence command for $arch",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-next-command-explicitly-docker-runtime",
            "GOBLINS_OS_CONTAINER_RUNTIME=docker",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-next-command-explicitly-display-backed",
            "RUN_QEMU=1",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-next-command-explicitly-shippable",
            "GOBLINS_OS_SHIPPABLE_RELEASE=1",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-prints-artifact-only-preflight",
            "RUN_QEMU=0",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-checks-artifact-only-preflight-not-release-proof",
            "Docker artifact-only preflight passed for [$]ARCH on [$]HOST_ARCH; not release proof",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-prints-runtime-proof-env",
            "BUILT_ARTIFACT_PATH_URL=<real-built-app-path-or-url>",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-external-disk-preflight",
            "external gate fails low disk before build",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-container-runtime-health-preflight",
            "external gate checks container runtime health before build",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-host-free-space-preflight",
            "120 GiB free",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-docker-health-preflight",
            "docker info",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-arch-matrix",
            "ARCHES=(aarch64 x86_64)",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-per-arch-signoff-function",
            "signoff_run_for_arch_is_complete",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-per-arch-signoff-required",
            "$arch has complete signoff row",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-per-arch-signoff-requires-runtime-artifact",
            "built artifact path/URL",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-real-field-helper",
            "signoff_block_has_real_field",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-rejects-na-runtime-proof",
            "no live engine",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-requires-engine-source",
            "engine source",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-requires-motion-proof",
            "Motion/interactions checked: yes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-requires-preview-open-render-proof-row",
            "Preview open/render checked: yes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-requires-focus-arm-roundtrip-proof-row",
            "Focus arm roundtrip checked: yes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-requires-multi-display-apply-proof-row",
            "Multi-display apply checked: yes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-requires-app-privacy-revoke-proof-row",
            "App privacy revoke checked: yes",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-completion-consistency",
            "declares completion before required proof is present",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-completion-exact",
            "Current project completion status: complete$",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-runtime-proof-validator",
            "validate_runtime_proof_fields",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-rejects-placeholder-runtime-proof",
            "placeholders are not accepted",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-built-artifact-reference-validator",
            "built_artifact_reference_is_real",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-arch-screenshot-dir-validator",
            "screenshot_dir_matches_arch",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-arch-screenshot-dir-copy",
            "os/screenshots/hardware-gate/$ARCH/<date>",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-validates-screenshot-png-signature",
            "screenshot_file_is_valid_png",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-screenshot-proof-manifest",
            "screenshot_manifest_matches_iso",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-firewall-live-toggle-proof",
            "firewall_live_toggle_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-session-proof",
            "text_shortcuts_session_enable_proof_passes",
        ),
        absent_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-does-not-require-retired-textshortcuts-live-keystroke-proof",
            "text_shortcuts_live_keystroke_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-candidate-metadata-proof",
            "text_shortcuts_candidate_metadata_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-overlay-intent-proof",
            "text_shortcuts_overlay_intent_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-candidate-bubble-frame-proof",
            "text_shortcuts_candidate_bubble_frame_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-candidate-bubble-layout-proof",
            "text_shortcuts_candidate_bubble_layout_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-candidate-bubble-render-intent-proof",
            "text_shortcuts_candidate_bubble_render_intent_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-candidate-bubble-render-proof",
            "text_shortcuts_candidate_bubble_render_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-live-ibus-runtime-render-proof",
            "text_shortcuts_live_ibus_runtime_render_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-keyboard-shortcuts-roundtrip-proof",
            "keyboard_shortcuts_roundtrip_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-input-sources-roundtrip-proof",
            "input_sources_roundtrip_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-multi-display-apply-proof",
            "multi_display_apply_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-focus-arm-roundtrip-proof",
            "focus_arm_roundtrip_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-app-privacy-revoke-proof",
            "app_privacy_revoke_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-preview-open-render-proof",
            "preview_open_render_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-audio-output-proof",
            "audio_output_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-firewall-live-toggle-proof",
            "Firewall live toggle checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-session-proof",
            "Text Shortcuts session enablement checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-live-keystrokes-covered-by-runtime-render",
            "covered by $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-candidate-metadata-proof",
            "Text Shortcuts candidate metadata checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-overlay-intent-proof",
            "Text Shortcuts overlay intent checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-candidate-bubble-frame-proof",
            "Text Shortcuts candidate bubble frame checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-candidate-bubble-layout-proof",
            "Text Shortcuts candidate bubble layout checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-candidate-bubble-render-intent-proof",
            "Text Shortcuts candidate bubble render intent checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-candidate-bubble-render-proof",
            "Text Shortcuts candidate bubble render screenshot checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-textshortcuts-live-ibus-runtime-render-proof",
            "Text Shortcuts live IBus runtime/render checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-keyboard-shortcuts-roundtrip-proof",
            "Keyboard shortcuts roundtrip checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-input-sources-roundtrip-proof",
            "Input sources roundtrip checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-multi-display-apply-proof",
            "Multi-display apply checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-focus-arm-roundtrip-proof",
            "Focus arm roundtrip checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-app-privacy-revoke-proof",
            "App privacy revoke checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-preview-open-render-proof",
            "Preview open/render checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-audio-output-proof",
            "Audio output checked",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-preview-open-render-status-completes-project",
            r#"[[ "$PREVIEW_OPEN_RENDER_STATUS" == yes* ]]"#,
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-audio-output-status-completes-project",
            r#"[[ "$GAMING_AUDIO_OUTPUT_STATUS" == yes* ]]"#,
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-focus-arm-roundtrip-status-completes-project",
            r#"[[ "$FOCUS_ARM_ROUNDTRIP_STATUS" == yes* ]]"#,
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-multi-display-apply-status-completes-project",
            r#"[[ "$MULTI_DISPLAY_APPLY_STATUS" == yes* ]]"#,
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-app-privacy-revoke-status-completes-project",
            r#"[[ "$APP_PRIVACY_REVOKE_STATUS" == yes* ]]"#,
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-firewall-proof-disable-state",
            "\"disable_active\"",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-firewall-proof-enable-state",
            "\"enable_active\"",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-workflow-checks-fail-fast",
            "require_fixed",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-docker-assisted-signoff-copy",
            "Docker is required for assisted signoff testing",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-docker-image-check",
            "docker image inspect",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-docker-run",
            "docker run --rm",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-docker-build",
            "DOCKER_BUILDKIT=1 docker build",
        ),
        absent_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-no-podman-assisted-signoff",
            "podman",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-current-image-tag-contract",
            "goblins-os:${{ matrix.arch }}",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-close-signoff-docker-only",
            "close-signoff uses Docker for assisted signoff testing",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-exact-arch-iso-path",
            "expected_iso=\"os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso\"",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-close-signoff-runtime-validator",
            "close-signoff rejects placeholder runtime proof",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-signoff-arch-screenshot-dir-required",
            "os/screenshots/hardware-gate/$arch",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-aarch64-native-runner",
            "ubuntu-24.04-arm",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-x86-64-native-runner",
            "ubuntu-24.04",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-native-runner-arch-assertion",
            "Assert native runner architecture",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-native-runner-uname-check",
            r#"test "$(uname -m)" = "${{ matrix.expected_uname }}""#,
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-aarch64-expected-uname",
            "expected_uname: aarch64",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-x86-64-expected-uname",
            "expected_uname: x86_64",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-native-desktop-tests",
            r#"cargo test --workspace --features "$NATIVE_FEATURES""#,
        ),
        contains_check(
            root.join("os/bootc/gate.Dockerfile"),
            "local-gate-native-desktop-tests",
            "cargo test --workspace --features",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-release-evidence-generation",
            "--release-evidence /out",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-rpm-evidence-generation",
            "goblins_os_release_evidence_hashes_match",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-release-evidence-secret-scan",
            "goblins_os_artifact_secret_scan",
        ),
        contains_check(
            root.join(".github/workflows/build.yml"),
            "ci-release-evidence-upload-artifact",
            "goblins-os-release-evidence-${{ matrix.arch }}",
        ),
        contains_check(
            root.join(".github/workflows/release.yml"),
            "release-workflow-delegates-to-canonical-candidate-builder",
            "uses: ./.github/workflows/candidate-artifacts.yml",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-buildx-builder-action",
            "docker/setup-buildx-action@bb05f3f5519dd87d3ba754cc423b652a5edd6d2c",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-buildkit-gha-cache-scope",
            "type=gha,scope=goblins-os-bootc-${{ matrix.arch }}",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-uses-buildkit-registry-digest",
            "steps.build.outputs.digest",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-binds-bib-to-immutable-image",
            "GOBLINS_OS_BIB_SOURCE_IMAGE=\"$IMMUTABLE_IMAGE_REF\"",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-runs-exact-source-verifier",
            "--source-root /workspace",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-runs-exact-installed-root-verifier",
            "--installed-root /",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-runs-packaged-selftest",
            "--target selftest",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-verifies-exact-bib-payload-image",
            "bib_image_ref\" == \"$IMMUTABLE_IMAGE_REF",
        ),
        contains_check(
            root.join("os/iso/manifest-provenance.sh"),
            "bib-manifest-parser-bounds-json-embedded-image-token",
            "JSON-escaped kickstart payload",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-records-image-provenance",
            "--image-ref \"$IMMUTABLE_IMAGE_REF\"",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-requires-current-main",
            "is not the current origin/main commit",
        ),
        contains_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-uploads-lightweight-digest-metadata",
            "goblins-os-candidate-ref-${{ steps.candidate.outputs.commit }}-${{ matrix.arch }}",
        ),
        absent_check(
            root.join(".github/workflows/candidate-artifacts.yml"),
            "candidate-workflow-does-not-write-repository-contents",
            "contents: write",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-records-immutable-image-reference",
            "Image digest reference",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-proof-records-immutable-image-reference",
            "\"image_ref\":image_ref",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-proof-uses-exact-candidate-tooling",
            "Capture tooling checkout $SOURCE_HEAD does not match candidate $CANDIDATE_COMMIT.",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-proof-validates-safe-calendar-date",
            "RUN_DATE must be a real calendar date in YYYY-MM-DD form.",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-proof-verifies-iso-checksum",
            "Capture ISO checksum mismatch",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-proof-canonicalizes-external-evidence",
            "GOBLINS_OS_CAPTURE_RELEASE_EVIDENCE_DIR",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-proof-requires-selected-image-digest",
            "GOBLINS_OS_CAPTURE_EXPECTED_IMAGE_REF",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-verification-iso.yml"),
            "aarch64-native-packaging-gate-proof",
            "goblins-os-native-packaging-gate-v1",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-verification-iso.yml"),
            "aarch64-native-packaging-gate-records-workflow-attempt",
            "workflow_run_attempt: $workflow_run_attempt",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "aarch64-capture-propagates-native-gate-workflow-attempt",
            "GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_ATTEMPT",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "aarch64-signoff-records-native-gate-workflow-attempt",
            "Native packaging gate run attempt:",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-authenticates-native-gate-workflow-attempt",
            ".github/workflows/aarch64-verification-iso.yml",
        ),
        contains_check(
            root.join(".github/workflows/aarch64-verification-iso.yml"),
            "aarch64-hardware-consumer-reruns-exact-source-verifier",
            "goblins-os-aarch64-source-verify.log",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "x86-hardware-consumer-reruns-exact-source-verifier",
            "goblins-os-x86_64-source-verify.log",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "native-packaging-gate-records-source-verifier",
            "\"source_verifier\": \"pass\"",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-validates-native-packaging-gate",
            "native_packaging_gate_proof_passes",
        ),
        contains_check(
            root.join("os/hardware-gate/compose-signoff-rows.sh"),
            "dual-architecture-signoff-row-composition",
            "Current project completion status: complete",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-pins-ollama-runtime-image",
            "ollama/ollama@sha256:",
        ),
        absent_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-does-not-run-remote-ollama-installer",
            "ollama.com/install.sh",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "x86-hardware-capture-requires-complete-signoff",
            "GOBLINS_OS_CAPTURE_REQUIRE_COMPLETE=1",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-fail-closed-completion-mode",
            "requires a complete signoff row",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-pulls-exact-candidate-image",
            "Pulling and verifying exact candidate image",
        ),
        contains_check(
            root.join("os/hardware-gate/run-external-gate.sh"),
            "external-gate-verifies-exact-candidate-revision",
            "org.opencontainers.image.revision",
        ),
    ]
}

fn gaming_readiness_checks(root: &Path) -> Vec<Check> {
    let mut checks = vec![
        container_contains_check(root, "gaming-vulkan-mesa", "mesa-vulkan-drivers"),
        container_contains_check(root, "gaming-vulkan-tools", "vulkan-tools"),
        container_contains_check(root, "gaming-gamescope", "gamescope"),
        container_contains_check(root, "gaming-gamemode", "gamemode"),
        container_contains_check(root, "gaming-mangohud", "mangohud"),
        container_contains_check(root, "gaming-vaapi-tools", "libva-utils"),
        container_contains_check(root, "gaming-mesa-vaapi-drivers", "mesa-va-drivers"),
        container_contains_check(root, "gaming-vdpau-wrapper", "libvdpau"),
        container_contains_check(root, "gaming-vdpau-diagnostics", "vdpauinfo"),
        container_contains_check(root, "gaming-flatpak", "flatpak"),
        container_contains_check(root, "gaming-pipewire-alsa", "pipewire-alsa"),
        container_contains_check(root, "gaming-pipewire-pulseaudio", "pipewire-pulseaudio"),
        container_contains_check(root, "gaming-pipewire-utils", "pipewire-utils"),
        container_contains_check(root, "gaming-controller-udev", "joystick-support"),
        container_contains_check(root, "gaming-controller-evtest", "evtest"),
        container_contains_check(root, "gaming-controller-usbutils", "usbutils"),
        container_package_not_installed_check(root, "gaming-steam-not-installed", "steam"),
        container_package_not_installed_check(
            root,
            "gaming-steam-devices-not-installed",
            "steam-devices",
        ),
        container_contains_check(root, "gaming-steam-rpm-absence-guard", "! rpm -q steam"),
        container_contains_check(
            root,
            "gaming-steam-devices-rpm-absence-guard",
            "! rpm -q steam-devices",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-panel",
            "SettingsPanel::Games",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-vulkan-readiness",
            "Graphics acceleration is ready for games",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-vaapi-vdpau-readiness",
            "Hardware video decoding is available for games and media.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-flatpak-portals-readiness",
            "App installs and desktop integration are ready",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-native-architecture-readiness",
            "Game tools run natively on this device",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-non-steam-launchers",
            "Heroic",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-lutris-launcher",
            "Lutris",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-bottles-launcher",
            "Bottles",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-umu-proton-guidance",
            "UMU and Proton",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-user-initiated-launchers",
            "when you choose",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-launcher-arch-availability",
            "Availability is checked per architecture at install time.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-no-automatic-proton-downloads",
            "does not download Proton runtimes without user action",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-games-steam-absence-copy",
            "Steam is not part of the base system",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "gaming-release-package-contract",
            "Mesa Vulkan/VA-API, VDPAU wrapper",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "gaming-release-user-initiated-launchers",
            "user-initiated installs",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "gaming-release-steam-absence-policy",
            "Steam and steam-devices are intentionally absent",
        ),
        contains_check(
            root.join("os/release/architectures.toml"),
            "gaming-release-native-architecture-policy",
            "does not claim x86-only game runtimes work on Arm",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "render-settings-games-light",
            "116-settings-games.png",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "render-settings-games-dark",
            "117-settings-games-dark.png",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "signoff-gaming-readiness-field",
            "Gaming readiness checked",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-gaming-readiness-field",
            "Gaming readiness checked: yes",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-gaming-vaapi-diagnostic",
            "vainfo",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-gaming-controller-diagnostics",
            "evtest --query",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-gaming-pipewire-diagnostics",
            "wpctl status",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-gaming-audio-output-proof",
            "audio-output-proof.json",
        ),
    ];

    for screenshot in GAMING_PROOF_SCREENSHOTS {
        checks.push(contains_check(
            root.join("os/hardware-gate/runbook.md"),
            &format!("runbook-gaming-proof-{screenshot}"),
            screenshot,
        ));
        checks.push(contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            &format!("shipping-status-gaming-proof-{screenshot}"),
            screenshot,
        ));
        checks.push(contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            &format!("close-signoff-gaming-proof-{screenshot}"),
            screenshot,
        ));
    }

    checks
}

fn goblins_ai_contract_checks(root: &Path) -> Vec<Check> {
    let old_ask_goblins_placeholder = ["Ask", "Goblins or describe what you need"].join(" ");
    let old_voice_model_title = ["Voice", "model"].join(" ");
    let old_talk_to_os_button = ["Talk to", "Goblins OS"].join(" ");

    vec![
        contains_check(
            root.join("Cargo.toml"),
            "goblins-ai-workspace-member",
            "crates/goblins-os-ai",
        ),
        contains_check(
            root.join("Cargo.lock"),
            "goblins-ai-lock-package",
            "name = \"goblins-os-ai\"",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-registry-version",
            "REGISTRY_VERSION",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-ask-action",
            "ask-goblins",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-ask-action-title",
            "title: \"Ask Goblin\"",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-writing-action",
            "write-with-goblins",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-writing-action-title",
            "title: \"Write with Goblin\"",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-writing-action-kind",
            "AiActionKind::Write",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-screen-context-action",
            "summarize-screen",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-confirmed-settings-action",
            "change-safe-setting",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "goblins-ai-app-builder-action",
            "build-app",
        ),
        contains_check(
            root.join("crates/goblins-os-core/Cargo.toml"),
            "core-depends-on-goblins-ai",
            "goblins-os-ai",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-actions-route",
            "/v1/ai/actions",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-action-history-route",
            "/v1/ai/action-history",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-safe-setting-change-route",
            "/v1/ai/safe-setting-change",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-open-settings-panel-route",
            "/v1/ai/open-settings-panel",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-system-status-route",
            "/v1/ai/system-status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-file-context-route",
            "/v1/ai/file-context",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-settings-context-route",
            "/v1/ai/settings-context",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-selected-text-context-route",
            "/v1/ai/selected-text-context",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-writing-tools-route",
            "/v1/ai/write-selected-text",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-screen-context-route",
            "/v1/ai/screen-context",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-notification-context-route",
            "/v1/ai/notification-context",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-engine-waiting-state",
            "WaitingForEngine",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-permission-gated-state",
            "PermissionGated",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-history-retention",
            "MAX_HISTORY_EVENTS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-history-no-content-copy",
            "Prompts, replies, screen content, file content, notification text, and secrets are not stored.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-file-context-policy",
            "policy_state_for_control(\"file-context\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-file-context-no-content-read",
            "do not claim to have read file contents",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-settings-context-policy",
            "policy_state_for_control(\"system-troubleshooting\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-settings-context-metadata-boundary",
            "Use only this Settings metadata",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-settings-context-confirmation-boundary",
            "require explicit user confirmation",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-open-settings-panel-request",
            "OpenSettingsPanelRequest",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-open-settings-panel-resident-policy",
            "policy_state_for_control(\"resident-assistant\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-open-settings-panel-deterministic-map",
            "SETTINGS_PANEL_CANDIDATES",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-open-settings-panel-launch-argument",
            "launch_argument: format!(\"--panel={}\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-open-settings-panel-audit",
            "audit_open_settings_panel",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-open-settings-panel-tests",
            "settings_panel_router_maps_exact_and_natural_language_requests",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-system-status-request",
            "SystemStatusContextRequest",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-system-status-policy",
            "system_troubleshooting_policy",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-system-status-os-owned-snapshot",
            "Use only this OS-owned status snapshot",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-system-status-bounded-snapshot",
            "bounded_system_status_snapshot",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-system-status-audit-explain",
            "audit_ai_action(action_id, Some(\"troubleshooting\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-system-status-action-selection",
            "system_status_action_id",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-system-status-tests",
            "system_status_prompt_uses_only_os_owned_snapshot",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-safe-setting-change-request",
            "SafeSettingChangeRequest",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-safe-setting-change-settings-control-policy",
            "policy_state_for_control(\"settings-control\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-safe-setting-change-confirmation-required",
            "StatusCode::PRECONDITION_REQUIRED",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-safe-setting-change-allowlist",
            "appearance.color-scheme, accessibility.reduce-motion, or notifications.show-banners",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-safe-setting-change-audit",
            "audit_ai_action(\"change-safe-setting\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/appearance.rs"),
            "appearance-ai-safe-color-scheme-wrapper",
            "apply_ai_color_scheme",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/accessibility.rs"),
            "accessibility-ai-safe-reduce-motion-wrapper",
            "apply_ai_reduce_motion",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/notifications.rs"),
            "notifications-ai-safe-banners-wrapper",
            "apply_ai_notification_banners",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-selected-text-policy",
            "policy_state_for_control(\"screen-context\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-selected-text-boundary",
            "Use only the selected text and app/window metadata",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-writing-tools-boundary",
            "explicitly invoked writing assistance",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-writing-tools-ready-text",
            "Return ready-to-use text first",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-writing-tools-audit",
            "audit_ai_action(\"write-with-goblins\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-screen-context-boundary",
            "Use only the provided visible text, visual summary, and app/window metadata",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-screen-context-visual-summary",
            "visual_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-notification-context-policy",
            "policy_state_for_control(\"notification-context\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-notification-context-boundary",
            "Use only this invoked notification summary",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-notification-context-no-history",
            "do not claim to inspect notification history, other notifications, files, screenshots, secrets, hidden windows, or background app data",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-notification-context-confirmation",
            "require explicit user confirmation before the action",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-notification-context-audit",
            "audit_ai_action(\"answer-notification\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-notification-context-test",
            "notification_context_prompt_is_invoked_and_bounded_to_one_notification",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ai.rs"),
            "core-ai-context-no-hidden-content",
            "do not claim to inspect live pixels, files, notifications, secrets, hidden windows",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-audits-ask-goblins",
            "audit_ai_action(\"ask-goblins\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-runtime-route-status",
            ".route(\"/v1/ai/runtime/status\", get(ai_runtime_status))",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-runtime-route-message",
            ".route(\"/v1/ai/runtime\", post(ai_runtime))",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-ai-runtime-legacy-compatibility-route",
            ".route(\"/v1/codex/resident\", post(ai_runtime))",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-resident-copy-goblins-ai-message-bounds",
            "Goblins AI needs a message between 1 and 1000 characters.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-resident-copy-goblins-ai-runtime-checkin",
            "Goblins AI runtime checked in",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-resident-copy-goblins-ai-model-access",
            "Open Models to configure local or cloud model access.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-resident-copy-test-hides-backend-plumbing",
            "resident_user_copy_hides_backend_plumbing",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-resident-copy-no-codex-resident",
            concat!("Codex ", "resident"),
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-resident-copy-no-env-var-guidance",
            "Configure OPENAI_OS_LOCAL_MODEL_RELAY",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-resident-copy-no-relay-failed",
            "relay failed without exposing credentials",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-openai-hosted-uses-responses-api",
            "/v1/responses",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-openai-hosted-disables-response-storage",
            "\"store\": false",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-openai-hosted-no-chat-completions",
            concat!("/v1/chat/", "completions"),
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "core-openai-default-model-current-gpt56",
            "gpt-5.6",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-openai-gpt56-reasoning-is-explicitly-balanced",
            r#"payload["reasoning"] = serde_json::json!({ "effort": "medium" })"#,
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-openai-gpt56-payload-contract-is-tested",
            "gpt_5_6_responses_payload_is_private_and_explicitly_balanced",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-declares-openai-agents-sdk",
            "Official OpenAI Agents SDK",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-build-studio-selected-engine-only",
            "always uses the explicitly selected Goblins AI engine",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-agents-sdk-capabilities",
            "tools, handoffs, guardrails, tracing, and sandbox execution",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-no-legacy-openai-service-type",
            "pub struct OpenAIService",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-declares-chatkit",
            "ChatKit",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-build-studio",
            "Build Studio",
        ),
        contains_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "openai-secret-template-agents-sdk-relay-primary",
            "GOBLINS_OS_AGENTS_SDK_RELAY_URL",
        ),
        contains_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "openai-secret-template-separates-agents-sdk-from-build-studio",
            "never overrides Build Studio's selected engine",
        ),
        contains_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "openai-secret-template-chatkit-relay-primary",
            "GOBLINS_OS_CHATKIT_RELAY_URL",
        ),
        contains_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "openai-secret-template-resident-relay-primary",
            "GOBLINS_OS_RESIDENT_RELAY_URL",
        ),
        contains_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "openai-secret-template-local-model-relay-primary",
            "GOBLINS_OS_LOCAL_MODEL_RELAY",
        ),
        absent_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "openai-secret-template-does-not-publish-legacy-openai-os-env",
            "OPENAI_OS_",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-prefers-goblins-local-model-relay-env",
            "const LOCAL_MODEL_RELAY_ENV: &str = \"GOBLINS_OS_LOCAL_MODEL_RELAY\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-keeps-openai-local-model-relay-compat",
            "const LOCAL_MODEL_RELAY_LEGACY_ENV: &str = \"OPENAI_OS_LOCAL_MODEL_RELAY\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-local-model-supports-ollama-keepalive",
            "GOBLINS_OS_LOCAL_MODEL_KEEP_ALIVE",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-local-model-logs-runtime-rejection",
            "GOBLINS_OS_LOCAL_MODEL_RUNTIME_REJECTED",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-route-matrix-is-exhaustive",
            "route_matrix_is_exhaustive_and_never_crosses_provider_or_locality",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-cloud-policy-is-checked-at-execution",
            "relay.locality() == EngineLocality::Cloud && !hosted_execution_allowed()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-cloud-policy-is-authoritative",
            "policy_state_for_control(\"cloud-openai\") == PolicyControlState::Allowed",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-openai-api-base-is-https-without-query",
            "server_https_url(base) && uri.query().is_none()",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "resident-never-logs-model-response-body",
            "body_tail=",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-engine-selection-is-typed",
            "enum EngineSelection",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-engine-state-default-path-is-core-owned",
            "const DEFAULT_ENGINE_PATH: &str = \"/var/lib/goblins-os/ai/engine\";",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-engine-state-follows-core-ai-state-root",
            "env::var_os(\"GOBLINS_OS_AI_STATE\").map(|dir| PathBuf::from(dir).join(\"engine\"))",
        ),
        ordered_contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-engine-state-uses-exclusive-temp-before-atomic-rename",
            ".create_new(true)",
            "fs::rename(&tmp, path)",
        ),
        ordered_contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-engine-state-syncs-file-before-atomic-rename",
            "file.sync_all()?;",
            "fs::rename(&tmp, path)",
        ),
        ordered_contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-engine-state-syncs-parent-after-atomic-rename",
            "fs::rename(&tmp, path)",
            "fs::File::open(parent)?.sync_all()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-engine-state-atomic-roundtrip-is-tested",
            "engine_preference_round_trips_through_os_owned_state",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-status-uses-opaque-storage-label",
            "storage: PRIVATE_STORAGE_LABEL",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-byo-key-is-read-only-from-systemd-credential",
            "openai_credential(\"OPENAI_API_KEY\")",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-core-has-no-desktop-key-ingest-handler",
            "pub async fn set_openai_key",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/openai_key.rs"),
            "openai-core-has-no-mutable-byo-key-file-path",
            "GOBLINS_OS_OPENAI_KEY_PATH",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-child-clears-parent-environment",
            "command.env_clear()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-conversation-sandbox-is-read-only",
            "Self::Resident => \":read-only\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-builder-sandbox-is-workspace-write",
            "Self::Studio => \":workspace\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-ignores-user-execution-config",
            ".arg(\"--ignore-user-config\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-studio-denies-service-owned-state-and-credentials",
            "codex_policies_deny_os_credentials_without_shadowing_the_workspace",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-permission-profiles-have-real-behavior-proof",
            "installed_codex_enforces_both_permission_profiles",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-permission-profiles-reject-unknown-config",
            ".arg(\"--strict-config\")",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-does-not-shadow-permission-profiles-with-legacy-sandbox-mode",
            ".arg(\"--sandbox\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-studio-shell-inherits-no-parent-environment",
            r#".arg("shell_environment_policy.inherit=\"none\"")"#,
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-studio-does-not-load-project-instructions",
            "project_doc_max_bytes=0",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-does-not-accept-env-supplied-exec-flags",
            "GOBLINS_OS_CODEX_EXEC_FLAGS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/studio.rs"),
            "studio-workspace-components-open-without-following-symlinks",
            "open_dir_nofollow",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/studio.rs"),
            "studio-workspace-refuses-hardlinked-files",
            "metadata.nlink() != 1",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/studio.rs"),
            "studio-workspace-escape-regressions-are-tested",
            "workspace_links_cannot_read_list_or_overwrite_external_secrets",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "codex-status-uses-opaque-storage-label",
            "codex_home: PRIVATE_STORAGE_LABEL",
        ),
        contains_check(
            root.join("os/systemd/goblins-os-core.service"),
            "core-service-loads-openai-systemd-credential",
            "LoadCredential=openai-secrets.env:/etc/goblins-os/openai-secrets.env",
        ),
        absent_check(
            root.join("os/systemd/goblins-os-core.service"),
            "core-service-does-not-export-openai-secret-environment",
            "EnvironmentFile=-/etc/goblins-os/openai-secrets.env",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/credentials.rs"),
            "core-secret-reader-uses-systemd-credentials-directory",
            "env::var_os(\"CREDENTIALS_DIRECTORY\")",
        ),
        contains_check(
            root.join("os/etc/goblins-os/openai-secrets.env"),
            "openai-byo-key-template-is-server-side-only",
            "#OPENAI_API_KEY=<server-side-only-openai-api-key>",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/auth.rs"),
            "core-auth-reads-systemd-credential-values",
            "openai_credential(\"OPENAI_ACCOUNT_CLIENT_SECRET\")",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/auth.rs"),
            "core-auth-does-not-read-oauth-secret-from-environment",
            "std::env::var(\"OPENAI_ACCOUNT_CLIENT_SECRET\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/auth.rs"),
            "core-oauth-callback-requires-exact-loopback-host",
            "callback_host_matches_redirect(&headers, &config.redirect_uri)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/auth.rs"),
            "core-oauth-callback-host-contract-is-tested",
            "oauth_callback_host_must_match_the_registered_loopback_authority",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/auth.rs"),
            "core-oauth-lifecycle-serializes-signout-and-session-commit",
            "struct AuthLifecycle",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/auth.rs"),
            "core-oauth-inflight-results-require-current-generation",
            "persist_auth_session_if_current",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/auth.rs"),
            "core-oauth-signout-race-has-regression-test",
            "sign_out_generation_cancels_every_in_flight_auth_flow",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-relay-reads-systemd-credential-values",
            "openai_credential(\"AI_GATEWAY_API_KEY\")",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/resident.rs"),
            "core-relay-does-not-read-gateway-secret-from-environment",
            "env::var(\"AI_GATEWAY_API_KEY\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/bounded.rs"),
            "core-bounded-children-clear-parent-environment",
            "command.env_clear();",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/bounded.rs"),
            "core-session-children-use-narrow-environment-opt-in",
            "const SESSION_ENV_ALLOWLIST",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/bounded.rs"),
            "core-long-operations-use-fail-fast-admission",
            ".try_acquire_owned()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/bounded.rs"),
            "core-long-operation-admission-is-tested",
            "long_operation_admission_fails_fast_without_queueing",
        ),
        absent_check(
            root.join("os/systemd/goblins-os-resident.service"),
            "resident-service-does-not-receive-openai-secrets",
            "EnvironmentFile=-/etc/goblins-os/openai-secrets.env",
        ),
        absent_check(
            root.join("crates/goblins-os-resident/src/main.rs"),
            "resident-source-never-names-full-gateway-secret-environment-variable",
            "AI_GATEWAY_API_KEY",
        ),
        absent_check(
            root.join("crates/goblins-os-resident/src/main.rs"),
            "resident-source-never-names-full-openai-secret-environment-variable",
            "OPENAI_API_KEY",
        ),
        contains_check(
            root.join("os/systemd/goblins-os-core.service"),
            "core-service-codex-and-app-state-is-writable",
            "/var/lib/goblins-os/apps /var/lib/goblins-os/codex",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-prefers-goblins-agents-sdk-relay-env",
            "GOBLINS_OS_AGENTS_SDK_RELAY_URL",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-no-hidden-agents-sdk-primary-env",
            "GOBLINS_OS_AGENTS_SDK_RELAY_URL",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-no-hidden-agents-sdk-compat-env",
            "OPENAI_OS_AGENTS_SDK_RELAY_URL",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-no-false-agents-sdk-source-marker",
            "official-openai-agents-sdk",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-uses-authoritative-resident-route",
            "resident_generate_with_engine",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-does-not-route-or-authorize-directly",
            "\"Authorization\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-tests-agents-sdk-is-separate",
            "never an invisible Build Studio route",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-no-openai-centered-product-framing",
            "OpenAI-centered Linux OS",
        ),
        contains_check(
            root.join("os/codex/config.toml"),
            "codex-local-chat-wire-is-loopback-only",
            "This compatibility wire is local-only",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-audits-build-app",
            "audit_ai_action(\"build-app\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/policy.rs"),
            "policy-screen-context-control",
            "screen-context",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/policy.rs"),
            "policy-settings-control",
            "settings-control",
        ),
        contains_check(
            root.join("crates/goblins-os-resident/Cargo.toml"),
            "resident-depends-on-goblins-ai",
            "goblins-os-ai",
        ),
        contains_check(
            root.join("crates/goblins-os-resident/src/main.rs"),
            "resident-advertises-ai-registry",
            "ResidentAiRegistry",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-ai-actions",
            "/v1/ai/actions",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-ai-action-history",
            "/v1/ai/action-history",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-ai-runtime-status",
            "/v1/ai/runtime/status",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-deserializes-authoritative-engine-route-status",
            "selected: ResidentEngineSelection,\n    ready: bool,\n    provider: ResidentEngineProvider,\n    locality: ResidentEngineLocality,",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-uses-core-engine-ready-as-route-truth",
            "fn resident_engine_ready(resident: &ResidentStatus) -> bool {\n    resident.engine.ready\n}",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-recognizes-core-process-states",
            "enum ResidentProcessState {\n    Online,\n    Stale,\n    Waiting,\n}",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-resident-contract-fails-closed",
            "resident_status_deserialization_accepts_only_the_core_readiness_contract",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-does-not-guess-route-readiness-from-process-words",
            "\"active\" | \"ready\" | \"running\" | \"healthy\"",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-no-legacy-runtime-status-route",
            "\"/v1/codex/resident/status\"",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-renders-goblins-ai",
            "append_goblins_ai_settings",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-api-key-readiness-says-key-never-enters-settings",
            "The key never enters Settings",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-never-serializes-an-openai-api-key",
            r#""api_key": api_key"#,
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-has-no-openai-key-save-control",
            "Save OpenAI key",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-runtime-copy",
            "Goblins AI runtime",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-process-state-copy",
            "Goblins AI process is {}",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-process-and-route-status-are-separate",
            "resident_engine_state_label(resident)",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-fetches-ai-runtime-status",
            "/v1/ai/runtime/status",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-thinking-pulse-honors-reduced-motion",
            "is_gtk_enable_animations",
        ),
        absent_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-no-legacy-runtime-status-route",
            "\"/v1/codex/resident/status\"",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-asks-ai-runtime-route",
            "/v1/ai/runtime",
        ),
        absent_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-no-legacy-runtime-message-route",
            "\"/v1/codex/resident\"",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-engine-copy",
            "Goblins AI engine",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-diagnostics-copy-uses-update-times",
            "Service health, app activity, logs, update times, and device status.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-diagnostics-goblins-ai-section",
            "label(\"Goblins AI runtime\", &[\"gos-subsection-title\"])",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-diagnostics-readable-status-row-title",
            "diagnostic_status_row_title",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-diagnostics-redacts-private-paths",
            "replace(\"/var/lib/goblins-os\", \"private OS storage\")",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-diagnostics-no-resident-section-title",
            "label(\"Resident\", &[\"gos-subsection-title\"])",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-private-storage-boundary-copy",
            "Private storage boundary",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-private-storage-hides-secret-paths",
            "Secrets are never displayed",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-copy-sanitizes-resident",
            "old_runtime_name",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-copy-sanitizes-relay",
            "(\"Relay routes\", \"Assistant routes\")",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-renders-ai-action-history",
            "Recent Goblins AI actions",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-renders-panel-ai-help",
            "append_settings_ai_help",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-help-uses-goblin-name",
            "Ask Goblin about this Settings panel",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-renders-notification-ai-context",
            "append_notifications_ai_context",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-notification-ai-context-group",
            "Goblins AI for notifications",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-notification-ai-context-uses-registered-action",
            "answer-notification",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-notification-ai-uses-goblin-name",
            "Ask Goblin about a notification",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-notification-ai-privacy-boundary",
            "only that notification's title, body, app, and chosen action label",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-posts-ai-settings-context",
            "/v1/ai/settings-context",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-context-bounded-summary",
            "bounded_status_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/readiness.rs"),
            "readiness-ai-runtime-copy",
            "Goblins AI runtime",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/readiness.rs"),
            "readiness-shell-copy-is-product-facing",
            "Goblins OS shell",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/readiness.rs"),
            "readiness-copy-no-core-api-localhost",
            "Core API is bound to localhost",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/readiness.rs"),
            "readiness-copy-no-rust-owned-shell",
            "Rust-owned shell session",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/readiness.rs"),
            "readiness-app-builder-copy",
            "Build Studio",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/readiness.rs"),
            "readiness-copy-no-codex-resident-relay",
            concat!("Codex ", "resident relay"),
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/readiness.rs"),
            "readiness-copy-no-old-build-studio-name",
            concat!("Codex ", "app builder"),
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/system.rs"),
            "system-service-ai-runtime-label",
            "label: \"Goblins AI runtime\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/settings.rs"),
            "recovery-ai-runtime-state-label",
            "Goblins AI runtime state",
        ),
        contains_check(
            root.join("crates/goblins-os-resident/src/main.rs"),
            "resident-capability-copy-goblins-ai",
            "Local app building is ready for Goblins AI.",
        ),
        absent_check(
            root.join("crates/goblins-os-resident/src/main.rs"),
            "resident-capability-copy-no-resident-conversations",
            "resident conversations",
        ),
        absent_check(
            root.join("crates/goblins-os-resident/src/main.rs"),
            "resident-capability-copy-no-resident-handoff",
            "resident can hand off app creation work",
        ),
        contains_check(
            root.join("crates/goblins-os-file-builder/src/main.rs"),
            "file-helper-ask-goblins-mode",
            "/v1/ai/file-context",
        ),
        contains_check(
            root.join("crates/goblins-os-file-builder/src/main.rs"),
            "file-helper-errors-hide-backend-plumbing",
            "errors_hide_backend_plumbing",
        ),
        absent_check(
            root.join("crates/goblins-os-file-builder/src/main.rs"),
            "file-helper-copy-no-build-daemon",
            "build daemon",
        ),
        contains_check(
            root.join("os/nautilus/scripts/Ask Goblin about this"),
            "nautilus-ask-goblin-script",
            "goblins-os-file-builder --ask",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-ask-goblins-action",
            "AskGoblins",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-ai-action-copy-hides-backend-plumbing",
            "ai_action_copy_is_os_owned_not_backend_plumbing",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-built-app-source-copy-goblins-native",
            "Built with Build Studio",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-build-studio-copy-task-facing",
            "Create and refine Goblins OS apps",
        ),
        absent_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-ai-action-copy-no-action-registry-relay",
            "action registry and relay",
        ),
        absent_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-built-app-source-copy-no-built-with-codex",
            "Built with Codex",
        ),
        absent_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-build-studio-copy-no-agent-surface",
            "multi-turn agent surface",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-search-uses-native-themed-icon",
            "gtk::Image::from_icon_name(\"system-search-symbolic\")",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-search-entry-accessible-label",
            "Search Goblins OS",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-result-row-native-button",
            "fn result_row(item: &LauncherItem) -> gtk::Button",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-result-row-keyboard-shortcuts",
            "gtk::accessible::Property::KeyShortcuts(\"Return Space\")",
        ),
        absent_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-search-no-telephone-recorder-glyph",
            "telephone-recorder",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-assistant-mode",
            "--assistant",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-assistant-placeholder-uses-goblin",
            "Ask Goblin or describe what you need",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-assistant-action-uses-goblin",
            "Ask Goblin",
        ),
        absent_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-assistant-no-old-ask-goblins-placeholder",
            &old_ask_goblins_placeholder,
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-selected-text-mode",
            "--selected-text",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-writing-tools-mode",
            "--writing-tools",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-screen-context-mode",
            "--screen-context",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-visual-context-mode",
            "--visual-context",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-fetches-ai-action-catalog",
            "/v1/ai/actions",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-gates-ask-goblins",
            "action_availability(&catalog, \"ask-goblins\")",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-gates-build-app",
            "action_availability(&catalog, \"build-app\")",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-gates-selected-text-context",
            "action_availability(&catalog, \"ask-selected-text\")",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-gates-writing-tools",
            "action_availability(&catalog, \"write-with-goblins\")",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-gates-screen-context",
            "action_availability(&catalog, \"summarize-screen\")",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-posts-selected-text-context",
            "/v1/ai/selected-text-context",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-posts-writing-tools",
            "/v1/ai/write-selected-text",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-posts-screen-context",
            "/v1/ai/screen-context",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-posts-visual-summary",
            "\"visual_summary\": visual_summary",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-visual-context-copy-no-silent-capture",
            "Goblins OS does not capture pixels silently.",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-screenshot-copy-os-owned",
            "Capture the screen, then ask with local-only visual context",
        ),
        absent_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-screenshot-copy-hides-toolkit-brand",
            "Capture through GNOME",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-opens-screenshot-context-helper",
            "goblins-os-screenshot-context",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-screenshot-result-routes-to-helper",
            "OpenScreenshotContext",
        ),
        // Behavioral anchor: the helper actually drives the capture through the
        // ashpd portal API (not the blocked GNOME-Shell D-Bus service).
        contains_check(
            root.join("crates/goblins-os-screenshot-context/src/main.rs"),
            "screenshot-context-helper-drives-portal-via-ashpd",
            "ashpd::desktop::screenshot::Screenshot",
        ),
        // Documentation anchor: the source names the sanctioned portal interface
        // so future editors keep the explanation of which API is used.
        contains_check(
            root.join("crates/goblins-os-screenshot-context/src/main.rs"),
            "screenshot-context-helper-documents-portal-interface",
            "org.freedesktop.portal.Screenshot.Screenshot",
        ),
        // The blocked path was a `gdbus` subprocess; assert that exact call site
        // is gone (the doc comment still names the interface, so pin the code).
        absent_check(
            root.join("crates/goblins-os-screenshot-context/src/main.rs"),
            "screenshot-context-helper-drops-gdbus-subprocess",
            "Command::new(\"gdbus\")",
        ),
        contains_check(
            root.join("crates/goblins-os-screenshot-context/src/main.rs"),
            "screenshot-context-helper-user-invoked-launcher-mode",
            "--visual-context",
        ),
        contains_check(
            root.join("crates/goblins-os-screenshot-context/src/main.rs"),
            "screenshot-context-helper-local-only-copy",
            "The image pixels stay local",
        ),
        contains_check(
            root.join("crates/goblins-os-screenshot-context/src/main.rs"),
            "screenshot-context-helper-private-runtime-dir",
            "Permissions::from_mode(0o700)",
        ),
        contains_check(
            root.join("crates/goblins-os-screenshot-context/src/main.rs"),
            "screenshot-context-helper-preserves-path-for-future-vision",
            "GOBLINS_OS_SCREENSHOT_CONTEXT_PATH",
        ),
        // The portal screenshot helper only works if the image ships the portal
        // service and its GNOME backend; pin both so they cannot be dropped.
        container_contains_check(
            root,
            "screenshot-context-portal-service-shipped",
            "xdg-desktop-portal \\",
        ),
        container_contains_check(
            root,
            "screenshot-context-portal-gnome-backend-shipped",
            "xdg-desktop-portal-gnome \\",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-goblins-ai-affordance",
            "Goblins AI",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-title-case-section-copy",
            "Connection & Appearance",
        ),
        absent_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-no-all-caps-section-copy",
            "CONNECTION & APPEARANCE",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-controls-have-accessible-labels",
            "set_accessible_label_description",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-sound-section-title-case",
            "Sound",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-slider-accessible-volume",
            "Volume",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-slider-accessible-display",
            "Display brightness",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-engine-segments-accessible",
            "Use on-device GPT-OSS",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-opens-assistant-mode",
            "[\"--assistant\"]",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-ask-action-uses-goblin",
            "Ask Goblin…",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-opens-selected-text-mode",
            "[\"--selected-text\"]",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-opens-writing-tools-mode",
            "[\"--writing-tools\"]",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-opens-screen-context-mode",
            "[\"--screen-context\"]",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-opens-screenshot-context-helper",
            "goblins-os-screenshot-context",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-fetches-ai-action-catalog",
            "/v1/ai/actions",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-gates-selected-text-context",
            "ai_action_availability(&config.core, \"ask-selected-text\")",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-gates-writing-tools",
            "ai_action_availability(&config.core, \"write-with-goblins\")",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-gates-screen-context",
            "ai_action_availability(&config.core, \"summarize-screen\")",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-disables-unready-ai",
            "button.set_sensitive(false)",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-goblins-ai-affordance",
            "Goblins AI",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-ai-button-creates-popup-menu",
            "this._ai = new PanelMenu.Button(0.0, 'Goblins AI');",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-opens-assistant-mode",
            "'--assistant'",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-opens-selected-text-mode",
            "'--selected-text'",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-opens-writing-tools-mode",
            "'--writing-tools'",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-opens-screen-context-mode",
            "'--screen-context'",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-opens-screenshot-context-helper",
            "goblins-os-screenshot-context",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-passes-screen-context-metadata",
            "GOBLINS_OS_SCREEN_CONTEXT_TEXT",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-passes-active-window-title",
            "GOBLINS_OS_CONTEXT_WINDOW_TITLE",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-uses-shell-window-tracker",
            "Shell.WindowTracker.get_default()",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-no-silent-screen-capture-copy",
            "No screen content was captured automatically.",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-opens-ai-settings",
            "--panel=models",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-adaptive-chrome-overlays-light-sheet",
            "theme.load_stylesheet(this._lightChromeFile)",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-adaptive-chrome-watches-color-scheme",
            "changed::color-scheme",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-adaptive-chrome-light-sheet-path",
            "/usr/share/themes/GoblinsOS/gnome-shell/gnome-shell-light.css",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-adaptive-chrome-unloads-in-dark",
            "theme?.unload_stylesheet(this._lightChromeFile)",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-adaptive-chrome-has-reentrancy-guard",
            "if (this._applyingSchemeChrome)",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-adaptive-chrome-tracks-theme-identity",
            "currentTheme !== this._lightChromeTheme",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-input-source-uses-gnome-schema",
            "org.gnome.desktop.input-sources",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-input-source-hides-single-source",
            "sources.length <= 1",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-input-source-watches-current",
            "changed::current",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-input-source-honest-current-gate",
            "current === null",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-input-source-cjk-abbreviations",
            "normalizedId === 'libpinyin'",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/stylesheet.css"),
            "menubar-input-source-uses-canonical-accent",
            "rgba(0, 145, 255, 0.22)",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-input-source-desktop-render-proof-hook",
            "59-menubar-input-source-$suffix.png",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-input-source-render-seeds-two-sources",
            "[('xkb', 'us'), ('xkb', 'gb')]",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-input-source-render-seeds-current-index",
            "gsettings set org.gnome.desktop.input-sources current 1",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-focus-uses-goblins-focus-schema",
            "org.goblins.os.focus",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-focus-hides-when-off",
            "if (!activeMode)",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-focus-hides-unknown-active-mode",
            "modes.find(entry => entry.id === activeMode)",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-focus-opens-settings-notifications",
            "--panel=notifications",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/stylesheet.css"),
            "menubar-focus-uses-canonical-accent",
            ".goblins-focus-indicator",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-focus-desktop-render-proof-hook",
            "59b-menubar-focus-$suffix.png",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-focus-render-seeds-mode",
            r#"[{"id":"work","name":"Deep Work"}]"#,
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-focus-render-seeds-active-mode",
            "gsettings set org.goblins.os.focus active-mode work",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-today-launches-owned-binary",
            "/usr/libexec/goblins-os/goblins-os-today",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-today-button-created",
            "this._today = new PanelMenu.Button(0.0, 'Today', true);",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-today-added-to-panel",
            "Main.panel.addToStatusArea('goblins-today', this._today, 1, 'right');",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-today-opens-on-click",
            "_openToday()",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-today-spawns-owned-binary",
            "_spawn([TODAY]",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-today-uses-local-time",
            "GLib.DateTime.new_now_local().format",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-today-watches-clock-settings",
            "changed::clock-format",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"),
            "menubar-today-cleans-timer",
            "this._clearTodayClockTimer();",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-menubar@goblins.os/stylesheet.css"),
            "menubar-today-date-style",
            ".goblins-date-indicator",
        ),
        contains_check(
            root.join("os/themes/GoblinsOS/gnome-shell/gnome-shell-light.css"),
            "menubar-today-light-mode-recolor",
            ".goblins-date-indicator { color: #1a1a1f; }",
        ),
        contains_check(
            root.join("os/themes/GoblinsOS/gnome-shell/gnome-shell-light.css"),
            "menubar-light-mode-recolor-outranks-extension-base",
            "#panel .goblins-menubar-name { color: #1a1a1f; }",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-today-desktop-render-proof-hook",
            "59c-menubar-today-$suffix.png",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-today-render-seeds-clock-weekday",
            "gsettings set org.gnome.desktop.interface clock-show-weekday true",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "menubar-today-render-seeds-clock-seconds",
            "gsettings set org.gnome.desktop.interface clock-show-seconds false",
        ),
        contains_check(
            root.join("os/themes/GoblinsOS/gnome-shell/gnome-shell-light.css"),
            "shell-light-overlay-recolors-panel",
            "#panel {",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-accepts-screen-context-prefill",
            "GOBLINS_OS_SCREEN_CONTEXT_TEXT",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-accepts-visual-context-prefill",
            "GOBLINS_OS_VISUAL_CONTEXT_SUMMARY",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-accepts-selected-text-prefill",
            "GOBLINS_OS_SELECTED_TEXT_CONTEXT",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-bounds-context-metadata",
            "bounded_context_value",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-posts-context-app-metadata",
            "GOBLINS_OS_CONTEXT_APP",
        ),
        contains_check(
            root.join("crates/goblins-os-launcher/src/main.rs"),
            "launcher-posts-context-window-metadata",
            "GOBLINS_OS_CONTEXT_WINDOW_TITLE",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-copy-no-needs-gnome",
            "needs GNOME",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-copy-no-requires-gnome",
            "requires GNOME",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-copy-no-user-facing-gnome-portals",
            "GNOME desktop portals",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-user-facing-gnome-accessibility",
            "GNOME accessibility keys",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-quiet-statuses-render-as-calm-pills",
            "gos-status-quiet",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-default-height-proves-first-screen",
            "SETTINGS_DEFAULT_HEIGHT: i32 = 840",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-window-uses-default-size-contract",
            ".default_height(SETTINGS_DEFAULT_HEIGHT)",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-typography-rejects-negative-letter-spacing",
            "assert!(!css.contains(\"letter-spacing: -\"));",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-first-screen-density-contract",
            "padding: 24px 32px 28px;",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-summary-tiles-fit-first-screen",
            "min-height: 54px;",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-status-pills-map-backend-copy",
            "settings_status_display_label",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-status-pills-hide-raw-unavailable",
            "\"unavailable\" => \"Not ready\"",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-wifi-copy-no-raw-unavailable",
            "Networking is unavailable, so Settings cannot scan or join Wi-Fi here.",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-overview-network-copy-no-raw-unavailable",
            "Connectivity and active connection details are unavailable.",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-recovery-copy-no-ready-stay-unavailable",
            "repair, rollback, and reset actions stay unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-recovery-copy-no-blocked-stay-unavailable",
            "Recovery actions stay unavailable until all checks are ready",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-local-account-copy-no-uid-unavailable",
            "uid/gid unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-update-copy-no-image-unavailable",
            "update image details are unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-update-copy-no-engine-unavailable",
            "the update engine is unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-update-copy-no-health-unavailable",
            "system health checks are unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-update-copy-no-network-unavailable",
            "networking is unavailable",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-detail-copy-hides-networkmanager",
            "(\"NetworkManager\", \"Network service\")",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-detail-copy-hides-wireplumber",
            "(\"WirePlumber\", \"Audio routing\")",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-detail-copy-hides-gsettings",
            "(\"GSettings\", \"desktop preferences\")",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-facility-state-label-not-ready",
            "\"unavailable\" => \"Not ready\"",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-display-state-hides-raw-query-backend",
            "\"desktop bridge\"",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-display-query-copy-is-user-facing",
            "No displays have been detected for this session yet.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-displays-apply-route",
            "/v1/displays/apply",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/displays.rs"),
            "core-displays-uses-apply-monitors-config",
            "ApplyMonitorsConfig",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/displays.rs"),
            "core-displays-validate-apply-payload",
            "validate_logical_monitors",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-displays-reports-apply-gate",
            "Protected display apply is available",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-native-handoff-image-owned-label",
            "Not Included",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-native-handoff-image-owned-detail",
            "included in the full Goblins OS image",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-storage-pressure-plan-section",
            "append_storage_pressure_plan",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-storage-pressure-plan-title",
            "Storage pressure plan",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-storage-pressure-plan-disk-usage",
            "Open Disk Usage Analyzer",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-storage-pressure-plan-disks",
            "Open Disks",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-storage-pressure-plan-cleanup",
            "automatic removal of aged files",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-privacy-temp-cleanup-aged-copy",
            "Remove aged temporary files",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/privacy.rs"),
            "core-privacy-temp-cleanup-aged-copy",
            "Remove aged temporary files",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-privacy-no-old-temp-cleanup-copy",
            "Remove old temporary files",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-native-handoff-hides-package-manager-copy",
            "is not installed on this image",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-bluetooth-copy-not-ready",
            "Bluetooth support is not ready on this device",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-audio-copy-not-ready",
            "Audio routing support is not ready in this build",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-codex-copy-not-included",
            "Codex · not included",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-services-copy-not-included",
            "Required service support is not included in this build",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/bluetooth.rs"),
            "core-bluetooth-copy-not-ready",
            "Bluetooth support is not ready on this device",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/bluetooth.rs"),
            "core-bluetooth-copy-no-not-installed",
            "Bluetooth support is not installed",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-routing-copy-not-ready",
            "Audio routing controls are not ready",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-copy-no-not-installed",
            "WirePlumber control tooling is not installed",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "core-codex-copy-not-included",
            "Codex account support is not included in this build",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/codex.rs"),
            "core-codex-copy-no-not-installed",
            "Codex CLI is not installed",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-network-service-copy",
            "The network service is not responding on this device.",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-network-not-ready-label",
            "Networking not ready",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-network-copy-hides-networkmanager",
            "NetworkManager isn’t responding",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-network-copy-hides-raw-unavailable",
            "Networking unavailable",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-install-readiness-label",
            "Install readiness",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-status-pill-os-label",
            "status_pill(\"OS\"",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-local-os-services-copy",
            "Waiting for local OS services.",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-execute-error-local-os-services-copy",
            "Goblins OS could not reach local OS services to start the install.",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-product-copy-goblins-native-desktop",
            "Goblins-native desktop",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-product-copy-enter-goblins-desktop",
            "Enter Goblins OS",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-checks-copy",
            "Installer checks",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-disk-install-failure-copy",
            "The disk install reported a failure.",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-core-status-pill",
            "status_pill(\"Core\"",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-local-core-wait",
            "Waiting for the local Goblins OS core.",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-local-core-execute-error",
            "could not reach the local core",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-goblins-os-core-label",
            "\"Goblins OS core\"",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-installer-engine-label",
            "Installer engine",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-bootc-installer-label",
            "bootc installer",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-guarded-bootc-prep",
            "guarded bootc install preparation",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-bootc-install-command",
            "bootc install command",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-bootc-install-failure",
            "bootc install reported a failure.",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-legacy-openai-native-desktop",
            "OpenAI-native desktop",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-legacy-openai-desktop-entry",
            "Enter OpenAI desktop",
        ),
        contains_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-local-os-services-copy",
            "Waiting for local OS services.",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-local-os-services-copy",
            "Waiting for local OS services.",
        ),
        contains_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-product-copy-unlocks-goblins-desktop",
            "Unlock Goblins OS desktop",
        ),
        contains_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-product-copy-goblins-desktop-rejection",
            "Goblins OS desktop unlock was rejected by local OS services.",
        ),
        contains_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-session-checks-copy",
            "Session checks",
        ),
        contains_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-rejection-copy-local-os-services",
            "rejected by local OS services",
        ),
        absent_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-copy-no-local-core-wait",
            "Waiting for the local Goblins OS core.",
        ),
        absent_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-copy-no-local-core-wait",
            "Waiting for the Goblins OS session gate from the local core.",
        ),
        absent_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-copy-no-local-os-core-rejection",
            "rejected by the local OS core.",
        ),
        absent_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-copy-no-goblins-os-core-label",
            "\"Goblins OS core\"",
        ),
        absent_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-copy-no-legacy-openai-desktop-unlock",
            "Unlock OpenAI desktop",
        ),
        absent_check(
            root.join("crates/goblins-os-login/src/main.rs"),
            "login-copy-no-legacy-openai-desktop-rejection",
            "OpenAI desktop unlock",
        ),
        contains_check(
            root.join("crates/goblins-os-open/src/main.rs"),
            "open-service-copy-goblins-service-id",
            "unknown Goblins OS service id",
        ),
        contains_check(
            root.join("crates/goblins-os-open/src/main.rs"),
            "open-service-copy-goblins-policy-block",
            "Goblins OS service {service_id} is blocked by the active Goblins OS policy",
        ),
        absent_check(
            root.join("crates/goblins-os-open/src/main.rs"),
            "open-service-copy-no-legacy-openai-os-service",
            "OpenAI OS service",
        ),
        contains_check(
            root.join("os/systemd/goblins-os-core.service"),
            "core-service-description-goblins-ai",
            "Description=Goblins OS local AI service core",
        ),
        contains_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-prepare-copy-no-disk-changed",
            "no disk has been changed",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-iso-manual-storage-path",
            "ISO manual storage path",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-iso-installation-destination",
            "ISO Installation Destination",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-installation-destination-in-iso",
            "Installation Destination in the ISO",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/main.rs"),
            "installer-copy-no-manual-storage-from-iso",
            "manual storage from the ISO",
        ),
        absent_check(
            root.join("crates/goblins-os-installer/src/bin/goblins-os-full-installer.rs"),
            "full-installer-copy-no-fedora-anaconda",
            "Fedora/Anaconda",
        ),
        absent_check(
            root.join("os/applications/org.goblins.OS.FullInstaller.desktop"),
            "full-installer-desktop-keywords-no-anaconda",
            "Anaconda;",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.FullInstaller.desktop"),
            "full-installer-desktop-icon-resolves-to-goblins-theme",
            "Icon=org.goblins.OS.Installer",
        ),
        file_check(
            root,
            "os/icons/GoblinsOS/scalable/apps/org.goblins.OS.Installer.svg",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-detail-copy-polishes-backend-copy",
            "settings_detail_display_copy",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-action-success-polishes-core-copy",
            "Ok(settings_detail_display_copy(&outcome.text))",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-action-error-polishes-core-copy",
            "Err(settings_detail_display_copy(&outcome.text))",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-rejection-polishes-core-copy",
            "let display_error = settings_detail_display_copy(error);",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-detail-copy-stays-disabled",
            "(\"stays unavailable\", \"stays disabled\")",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-detail-copy-not-set-up",
            "(\"not configured\", \"not set up\")",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-copy-no-not-adjustable-yet",
            "not adjustable from Settings yet",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-copy-no-core-controls",
            "real core controls",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-copy-no-sample-analytics",
            "sample analytics",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-detail-copy-not-ready-yet",
            "(\"not available yet\", \"not ready yet\")",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-detail-copy-not-ready",
            "(\"is unavailable\", \"is not ready\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/appearance.rs"),
            "core-appearance-copy-not-ready",
            "Desktop preferences are not ready, so appearance cannot be changed in this session.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-copy-not-ready",
            "Audio routing controls are not ready, so Settings cannot inspect or change audio here.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-copy-not-ready",
            "Keyboard, mouse, and trackpad preferences are not ready in this session.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/network.rs"),
            "core-network-copy-not-ready",
            "The Goblins OS network service is not ready in this environment.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/privacy.rs"),
            "core-privacy-copy-read-only",
            "Desktop preferences are not ready, so privacy controls are read-only in this session.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/accessibility.rs"),
            "core-accessibility-copy-read-only",
            "Desktop preferences are not ready, so accessibility preferences are read-only in this session.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/notifications.rs"),
            "core-notifications-copy-read-only",
            "Desktop preferences are not ready, so notification preferences are read-only in this session.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-copy-not-ready",
            "The desktop-session microphone bridge is not ready.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-status-probes-live-session-audio",
            "session_bridge::voice_audio_status()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-capture-uses-typed-session-bridge",
            "session_bridge::voice_capture()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-playback-failure-propagates",
            "play_audio(reply_wav.path())?",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-uses-private-per-call-workspaces",
            ".prefix(\"voice-call-\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-explicitly-cleans-every-workspace-result",
            "finish_private_voice_operation(result, cleanup)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-startup-purges-crash-leftovers-descriptor-safely",
            "purge_stale_voice_workspaces_at",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-runs-voice-crash-cleanup-before-serving",
            "voice::purge_stale_voice_workspaces()?;",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-transcript-read-is-bounded",
            ".take((MAX_TRANSCRIPT_BYTES + 1) as u64)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-transcript-open-is-no-follow",
            "libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/bounded.rs"),
            "core-voice-has-exclusive-operation-limiter",
            "fn voice_operation_limiter()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-converse-and-dictate-use-exclusive-limiter",
            "run_voice_blocking",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice_control.rs"),
            "core-voice-control-uses-exclusive-limiter",
            "run_voice_blocking",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-voice-revalidates-untrusted-capture-wave",
            "Some(CAPTURE_PCM_FORMAT)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-authenticates-session-bridge-peer",
            "SO_PEERCRED",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-voice-ops-are-typed",
            "VoiceAudioStatus {}",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-voice-capture-has-no-input-fields",
            "VoiceCapture {}",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-voice-capture-streams-without-path",
            "Ok(output) if output.status.success() => output.stdout",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-voice-playback-streams-without-path",
            "bounded_input_output_of(command, wav, VOICE_PLAYBACK_TIMEOUT)",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-voice-validates-canonical-pcm",
            "fn valid_pcm_wave(",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-voice-authenticates-core-peer",
            "voice operations require the authenticated core service peer.",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-voice-contract-has-adversarial-tests",
            "canonical_pcm_wave_rejects_duplicate_extended_and_trailing_chunks",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-io-has-absolute-deadline",
            "fn remaining_before(deadline: Instant)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-connect-shares-absolute-deadline",
            "socket.connect_timeout(&address, remaining_before(deadline)?)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-connect-is-atomically-close-on-exec",
            "Type::STREAM.cloexec()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-tests-saturated-listener-deadline",
            "core_bridge_connect_deadline_bounds_saturated_unix_listener",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "desktop-session-bridge-io-has-absolute-deadline",
            "fn remaining_before(deadline: Instant)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-tests-trickle-deadline",
            "core_bridge_deadline_rejects_trickle_response_by_total_wall_time",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "desktop-session-bridge-tests-trickle-deadline",
            "bridge_read_deadline_rejects_trickle_input_by_total_wall_time",
        ),
        contains_check(
            root.join("os/tmpfiles/goblins-os-core.conf"),
            "voice-work-root-is-owner-only",
            "d /var/lib/goblins-os/voice/work 0700 goblins-os goblins-os -",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-wake-word-goblin",
            "VOICE_WAKE_WORD: &str = \"Goblin\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-wake-phrase-hey-goblin",
            "\"Hey Goblin\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-wake-listening-contract",
            "wake_listening",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-background-wake-truthful-copy",
            "Background wake listening is not ready",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-voice-button-says-goblin",
            "Say {voice_word}",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-voice-listening-for-goblin",
            "Listening for {wake_word}…",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-voice-search-goblin-wake-word",
            "Goblin wake word",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-voice-detail-wake-word",
            "Wake word: {}.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-voice-background-wake-truthful-copy",
            "Background wake listening is not ready",
        ),
        absent_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-voice-no-old-voice-model-title",
            &old_voice_model_title,
        ),
        absent_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-voice-no-old-talk-to-os-button",
            &old_talk_to_os_button,
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/bluetooth.rs"),
            "core-bluetooth-power-copy-not-ready",
            "Bluetooth power is not ready in this session.",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-copy-no-is-unavailable",
            "is unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/voice.rs"),
            "core-voice-copy-no-microphone-unavailable",
            "Microphone capture is unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/bluetooth.rs"),
            "core-bluetooth-copy-no-power-unavailable",
            "Bluetooth power is unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/bluetooth.rs"),
            "core-bluetooth-copy-no-connections-unavailable",
            "Existing device connections are unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/appearance.rs"),
            "core-appearance-copy-no-raw-unavailable",
            "Desktop preference support is unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/audio.rs"),
            "core-audio-copy-no-raw-unavailable",
            "WirePlumber control tooling is unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-copy-no-raw-unavailable",
            "preferences are unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/network.rs"),
            "core-network-copy-no-raw-not-available",
            "network service is not available",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/privacy.rs"),
            "core-privacy-copy-no-raw-unavailable",
            "privacy controls are unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/accessibility.rs"),
            "core-accessibility-copy-no-raw-unavailable",
            "controls are unavailable",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/notifications.rs"),
            "core-notifications-copy-no-raw-unavailable",
            "Desktop preference support is unavailable",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "desktop-assistant-keybinding",
            "<Super><Shift>space",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "desktop-selected-text-keybinding",
            "goblins-os-launcher --selected-text",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "desktop-writing-tools-keybinding",
            "goblins-os-launcher --writing-tools",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "desktop-screen-context-keybinding",
            "goblins-os-launcher --screen-context",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "desktop-visual-context-keybinding",
            "goblins-os-screenshot-context",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-packages-screenshot-context-helper",
            "goblins-os-screenshot-context \\",
        ),
        // Live Text / OCR — the on-device Tesseract runtime + the local core route,
        // honest-gated when the runtime is absent (never claims success without text).
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-packages-ocr-tesseract",
            "tesseract-langpack-eng",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-ocr-recognize-route",
            "/v1/ocr/recognize",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/ocr.rs"),
            "ocr-honest-gating-when-runtime-absent",
            "Text recognition is not available on this device.",
        ),
        contains_check(
            root.join("crates/goblins-os-screenshot-context/src/main.rs"),
            "screenshot-context-ocr-handoff",
            "/v1/ocr/recognize",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-firewall-status-route",
            "/v1/firewall/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-firewall-enabled-route",
            "/v1/firewall/enabled",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-uses-scoped-systemd-template",
            "goblins-os-firewall@enable.service",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-honest-managed-by-system-copy",
            "Turning the firewall on or off is managed by the system.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-accessibility-accommodation-rows",
            "Typing assistance",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/accessibility.rs"),
            "core-accessibility-magnifier-schema",
            "org.gnome.desktop.a11y.magnifier",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/accessibility.rs"),
            "core-accessibility-magnifier-zoom-target",
            "magnifier-zoom",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-accessibility-magnifier-controls",
            "Turn on Magnifier to adjust zoom and lens mode.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-input-sources-list",
            "Input sources",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-input-sources-write-route",
            "/v1/input/sources",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-input-source-add-route",
            "/v1/input/source",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-input-source-switch-route",
            "/v1/input/switch-next",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-sources-write-allowlist",
            "normalize_input_sources",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-source-add-probes-ibus",
            "bounded_session_command_output(\"ibus\", &[\"list-engine\"], probe_timeout())",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-source-addable-intersection",
            "addable_input_source_choices",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-source-add-rejects-unlisted",
            "installed IBus engine is reported by this session",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-source-switch-current-key",
            "gsettings(&[\"set\", INPUT_SOURCES_SCHEMA, \"current\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-source-switch-requires-multiple",
            "Only one input source is configured, so Super+Space opens the launcher.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-sources-gvariant-encoder",
            "encode_input_sources",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-input-sources-write-row",
            "input_source_action_button",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-input-sources-write-route",
            "/v1/input/sources",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-input-source-add-sheet",
            "Add input source…",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-input-source-add-route",
            "/v1/input/source",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-input-source-add-uses-core-choices",
            "input_sources.addable_sources",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/input.rs"),
            "core-input-cjk-engine-registry",
            "cjk_engine_package_statuses_with",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-cjk-engine-packages",
            "CJK engine packages",
        ),
        container_package_lockstep_check(root, "ime-libpinyin-packaged", "ibus-libpinyin"),
        container_package_lockstep_check(root, "ime-anthy-packaged", "ibus-anthy"),
        container_package_lockstep_check(root, "ime-hangul-packaged", "ibus-hangul"),
        container_contains_check(
            root,
            "ime-libpinyin-component-asserted",
            "/usr/share/ibus/component/libpinyin.xml",
        ),
        container_contains_check(
            root,
            "ime-anthy-component-asserted",
            "/usr/share/ibus/component/anthy.xml",
        ),
        container_contains_check(
            root,
            "ime-hangul-component-asserted",
            "/usr/share/ibus/component/hangul.xml",
        ),
        container_contains_check(
            root,
            "ime-libpinyin-engine-asserted",
            "/usr/libexec/ibus-engine-libpinyin",
        ),
        container_contains_check(
            root,
            "ime-anthy-engine-asserted",
            "/usr/libexec/ibus-engine-anthy",
        ),
        container_contains_check(
            root,
            "ime-hangul-engine-asserted",
            "/usr/libexec/ibus-engine-hangul",
        ),
        container_contains_check(
            root,
            "ime-gtk4-module-asserted",
            "/usr/lib64/gtk-4.0/4.0.0/immodules/libim-ibus.so",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-security-firewall-status-row",
            "/v1/firewall/status",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-security-firewall-enabled-switch",
            "/v1/firewall/enabled",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-packages-firewalld",
            "firewalld",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-rpm-asserts-firewalld",
            "firewalld \\\n      dnsmasq \\\n      ntfs-3g \\\n      exfatprogs \\\n      udisks2 \\\n      rsync \\\n      ripgrep \\\n      wl-clipboard",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-command-asserts-firewall-cmd",
            "command -v firewall-cmd",
        ),
        container_package_lockstep_check(root, "preview-papers-packaged", "papers"),
        container_package_lockstep_check(root, "preview-loupe-packaged", "loupe"),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "preview-papers-command-assertion",
            "command -v papers",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "preview-loupe-command-assertion",
            "command -v loupe",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "preview-package-desktop-entries-asserted",
            "org.gnome.Papers.desktop",
        ),
        contains_check(
            root.join("os/bootc/generate-preview-proof-fixtures.py"),
            "preview-proof-fixture-generator-creates-pdf",
            "preview-open-render.pdf",
        ),
        contains_check(
            root.join("os/bootc/generate-preview-proof-fixtures.py"),
            "preview-proof-fixture-generator-creates-png",
            "preview-open-render.png",
        ),
        contains_check(
            root.join("os/bootc/generate-preview-proof-fixtures.py"),
            "preview-proof-fixture-generator-creates-unsupported-txt",
            "preview-open-render.txt",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "preview-proof-fixture-generator-installed",
            "generate-preview-proof-fixtures.py",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "preview-proof-fixtures-asserted",
            "/usr/share/goblins-os/proof/preview-open-render.pdf",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-preview-status-route",
            "/v1/preview/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-preview-open-route",
            "/v1/preview/open",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/preview.rs"),
            "core-preview-uses-xdg-open",
            "isolated_session_command(\"xdg-open\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/preview.rs"),
            "core-preview-names-papers-and-loupe",
            "Papers for PDFs and Loupe for images",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/preview.rs"),
            "core-preview-does-not-read-file-contents",
            "It never reads file contents or claims rendered proof.",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "preview-installed-selftest-checks-status",
            "GET /v1/preview/status -> HTTP",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "preview-installed-selftest-requires-viewers",
            "available=$preview_available xdg-open=$preview_xdg_open papers=$preview_papers loupe=$preview_loupe",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "preview-installed-selftest-checks-supported-extensions",
            r#"supported_extensions | index("pdf") and index("png")"#,
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "preview-installed-selftest-opens-pdf",
            "POST /v1/preview/open PDF -> HTTP",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "preview-installed-selftest-opens-image",
            "POST /v1/preview/open image -> HTTP",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "preview-installed-selftest-rejects-unsupported",
            "POST /v1/preview/open unsupported -> HTTP",
        ),
        container_package_lockstep_check(root, "fingerprint-authselect-packaged", "authselect"),
        container_package_lockstep_check(root, "fingerprint-fprintd-packaged", "fprintd"),
        container_package_lockstep_check(root, "fingerprint-fprintd-pam-packaged", "fprintd-pam"),
        container_package_lockstep_check(root, "fingerprint-libfprint-packaged", "libfprint"),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "fingerprint-authselect-feature-enabled",
            "authselect enable-feature with-fingerprint",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "fingerprint-authselect-feature-asserted",
            "authselect current | grep -q -- '- with-fingerprint'",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "fingerprint-pam-module-asserted",
            "pam_fprintd.so",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-fingerprint-status-route",
            "/v1/fingerprint/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/fingerprint.rs"),
            "core-fingerprint-uses-authselect",
            "authselect_has_fingerprint",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/fingerprint.rs"),
            "core-fingerprint-targets-fprintd-service",
            "net.reactivated.Fprint.service",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/fingerprint.rs"),
            "core-fingerprint-keeps-password-fallback-copy",
            "password remains available",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-security-fingerprint-row",
            "Fingerprint unlock",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keychain-manager-handoff-row",
            "Open Passwords & Keys",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keychain-manager-handoff-launches-seahorse",
            "SEAHORSE_PASSWORDS_AND_KEYS: &str = \"seahorse\"",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keychain-manager-handoff-uses-native-launcher",
            "append_keychain_manager_handoff(panel)",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-network-hotspot-management",
            "append_hotspot_management",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-network-hotspot-write-route",
            "/v1/hotspot/enabled",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-network-hotspot-input-validation",
            "hotspot_settings_inputs",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-network-hotspot-password-copy",
            "Passwords are used once to configure the hotspot and are never shown here.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/hotspot.rs"),
            "core-hotspot-connected-client-readout",
            "connected_clients_known",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/hotspot.rs"),
            "core-hotspot-parses-dnsmasq-leases",
            "parse_dnsmasq_leases",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/hotspot.rs"),
            "core-hotspot-networkmanager-lease-path",
            "/var/lib/NetworkManager/dnsmasq",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-hotspot-connected-client-copy",
            "Connected devices",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-installs-firewall-helper",
            "goblins-os-firewall /usr/libexec/goblins-os/goblins-os-firewall",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-controls-only-firewalld",
            "systemctl start firewalld.service",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-persists-enabled-state",
            "systemctl enable firewalld.service",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-resets-failed-state",
            "systemctl reset-failed firewalld.service",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-unmasks-before-enable",
            "systemctl unmask firewalld.service",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-reloads-systemd-before-enable",
            "systemctl daemon-reload",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-retries-start-with-restart",
            "systemctl start firewalld.service || /usr/bin/systemctl restart firewalld.service",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-waits-for-firewalld-state",
            "firewall-cmd --state",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-status-falls-back-to-systemd-active",
            "is-active\", \"--quiet\", \"firewalld.service",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-waits-up-to-ninety-seconds",
            "while [ \"$i\" -lt 90 ]",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-diagnoses-enable-failure",
            "firewalld did not report running after enable",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-prints-systemd-status",
            "systemctl --no-pager --full status firewalld.service",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-stops-before-disabling",
            "systemctl stop firewalld.service",
        ),
        contains_check(
            root.join("os/bootc/goblins-os-firewall"),
            "bootc-firewall-helper-persists-disabled-state",
            "systemctl disable firewalld.service",
        ),
        contains_check(
            root.join("os/systemd-system/goblins-os-firewall@.service"),
            "systemd-firewall-oneshot-helper",
            "ExecStart=/usr/libexec/goblins-os/goblins-os-firewall %i",
        ),
        contains_check(
            root.join("os/bootc/60-goblins-os-firewall.rules"),
            "polkit-firewall-rule-scopes-unit-instances",
            "goblins-os-firewall@(enable|disable)",
        ),
        contains_check(
            root.join("os/bootc/60-goblins-os-firewall.rules"),
            "polkit-firewall-rule-scopes-service-user",
            "subject.user !== \"goblins-os\"",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-installs-firewall-polkit-rule-in-image-owned-path",
            "60-goblins-os-firewall.rules /usr/share/polkit-1/rules.d/60-goblins-os-firewall.rules",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-accepts-image-owned-polkit-rule-path",
            "/usr/share/polkit-1/rules.d/60-goblins-os-firewall.rules",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-uses-stable-systemctl-path",
            "/usr/bin/systemctl",
        ),
        contains_check(
            root.join("crates/goblins-os-markup/src/main.rs"),
            "markup-copy-text-ocr-handoff",
            "/v1/ocr/recognize",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-hotspot-status-route",
            "/v1/hotspot/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-hotspot-enabled-route",
            "/v1/hotspot/enabled",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/hotspot.rs"),
            "hotspot-write-policy-gated",
            "policy_state_for_control(\"settings-control\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/hotspot.rs"),
            "hotspot-start-validates-dnsmasq",
            "dnsmasq_present",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/hotspot.rs"),
            "hotspot-start-blocks-single-radio-wifi-uplink",
            "Connect to the internet over Ethernet to share it over Wi-Fi.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/hotspot.rs"),
            "hotspot-uses-nonpersistent-networkmanager-profile",
            "\"save\".to_string()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/hotspot.rs"),
            "hotspot-sanitizes-psk-errors",
            "sanitize_hotspot_error",
        ),
        container_package_lockstep_check(root, "hotspot-dnsmasq-packaged", "dnsmasq"),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "hotspot-dnsmasq-command-assertion",
            "command -v dnsmasq",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-network-hotspot-row",
            "Personal Hotspot",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-window-management-status-route",
            "/v1/window-management/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-hot-corner-route",
            "/v1/window-management/hot-corner",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/window_management.rs"),
            "core-hot-corner-allowlist",
            "hot_corner_by_id",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/window_management.rs"),
            "core-hot-corner-gsettings-string-encoder",
            "encode_gsettings_string",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-window-management-status",
            "/v1/window-management/status",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-posts-hot-corner-route",
            "/v1/window-management/hot-corner",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-multitasking-hot-corner-controls",
            "append_hot_corner_settings",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-hot-corner-action-options",
            "HOT_CORNER_ACTION_OPTIONS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-shortcuts-status-route",
            "/v1/shortcuts/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-keyboard-shortcuts-binding-route",
            "/v1/keyboard/shortcuts/binding",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-keyboard-modifier-remap-route",
            "/v1/keyboard/modifier-remap",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/shortcuts.rs"),
            "core-keyboard-shortcuts-allowlist",
            "shortcut_spec_by_id",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/shortcuts.rs"),
            "core-keyboard-shortcuts-conflict-check",
            "shortcut_conflict",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/shortcuts.rs"),
            "core-keyboard-caps-lock-remap-preserves-options",
            "remap_caps_lock_options",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-migration-capabilities-route",
            "/v1/migration/capabilities",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-migration-sources-route",
            "/v1/migration/sources",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-migration-start-route",
            "/v1/migration/start",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-migration-progress-route",
            "/v1/migration/progress",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-migration-preference-plan-route",
            "/v1/migration/preference-plan",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-migration-copy-plan-route",
            "/v1/migration/copy-plan",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-migration-estimate-route",
            "/v1/migration/estimate",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-copy-plan-builder",
            "build_migration_copy_plan",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-sources-builder",
            "build_migration_sources",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-start-response-builder",
            "build_migration_start_response",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-start-plans-without-copying",
            "Migration copy job is planned. No files were copied by this start substrate.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-start-blocks-live-execution",
            "Live migration copy execution is CI/qemu-gated; this source substrate did not run rsync.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-start-precondition-required",
            "StatusCode::PRECONDITION_REQUIRED",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-progress-state-store",
            "OnceLock<Mutex<MigrationCopyProgress>>",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-progress-log-refresh",
            "refresh_migration_copy_progress_from_logs",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-progress-log-path",
            "progress.log",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-progress-rsync-parser",
            "parse_rsync_progress_line",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-progress-ledger-parser",
            "parse_migration_ledger_counts",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-progress-skipped-ledger-counter",
            "count_migration_skipped_ledger_entries",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-progress-no-live-copy-claim",
            "executes_live_copy: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-plan-builder",
            "build_migration_preference_plan",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-plan-copy",
            "Migration preference import plan is ready. No preferences were written by this source substrate.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-dconf-parser",
            "parse_dconf_dump",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-allowlist",
            "migration_preference_target",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-unknown-skip",
            "Preference is not in the Goblins OS migration allowlist.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-wallpaper-copy-evidence",
            "wallpaper_destination_uri_from_copied_paths",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-wallpaper-skip",
            "Wallpaper file was not present in the copied-path evidence.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-schema-gate",
            "available_schemas: Option<Vec<String>>",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-preference-no-live-import-claim",
            "executes_preference_import: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/install_targets.rs"),
            "core-migration-reuses-install-target-sysfs-scan",
            "scan_migration_source_partitions_in",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-source-readability-classifier",
            "migration_filesystem_readability",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-source-scan-fixture-test",
            "migration_sources_classify_sysfs_partitions_without_mounting",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-sources-mountinfo-read",
            "/proc/self/mountinfo",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-sources-partial-scan-report",
            "scan_errors",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-sources-partial-flag",
            "partial",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-source-apfs-disabled-copy",
            "Goblins can't read this disk's format (APFS).",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-sources-no-live-mount-claim",
            "executes_live_mount: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-sources-no-live-copy-claim",
            "executes_live_copy: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-sources-honest-scan-copy",
            "Migration source scan is ready. No disks were mounted and no files were copied by this source scan.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-estimate-builder",
            "build_migration_estimate",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-estimate-skips-symlinks",
            "file_type.is_symlink()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-estimate-no-live-copy-claim",
            "No files were mounted or copied by this sizing step.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-rsync-progress-contract",
            "--info=progress2",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-additive-copy-contract",
            "--ignore-existing",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/migration.rs"),
            "core-migration-no-live-copy-claim",
            "executes_live_copy: false",
        ),
        container_package_lockstep_check(root, "migration-ntfs-3g-packaged", "ntfs-3g"),
        container_package_lockstep_check(root, "migration-exfatprogs-packaged", "exfatprogs"),
        container_package_lockstep_check(root, "migration-udisks2-packaged", "udisks2"),
        container_package_lockstep_check(root, "migration-rsync-packaged", "rsync"),
        container_contains_check(
            root,
            "migration-command-asserts-ntfs-3g",
            "command -v ntfs-3g",
        ),
        container_contains_check(
            root,
            "migration-command-asserts-mount-ntfs-3g",
            "command -v mount.ntfs-3g",
        ),
        container_contains_check(
            root,
            "migration-command-asserts-fsck-exfat",
            "command -v fsck.exfat",
        ),
        container_contains_check(
            root,
            "migration-command-asserts-udisksctl",
            "command -v udisksctl",
        ),
        container_contains_check(root, "migration-command-asserts-rsync", "command -v rsync"),
        container_contains_check(
            root,
            "migration-udisks2-service-asserted",
            "/usr/lib/systemd/system/udisks2.service",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-snapshots-status-route",
            "/v1/snapshots/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-snapshots-restore-route",
            "/v1/snapshots/restore",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/snapshots.rs"),
            "core-snapshots-mountinfo-reader",
            "/proc/self/mountinfo",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/snapshots.rs"),
            "core-snapshots-snapper-machine-parser",
            "parse_snapper_machine_readable",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/snapshots.rs"),
            "core-snapshots-btrfs-home-honesty",
            "Local snapshots need a btrfs /home",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/snapshots.rs"),
            "core-snapshots-restore-fail-closed",
            "executes_restore: false",
        ),
        container_package_lockstep_check(root, "snapshots-btrfs-progs-packaged", "btrfs-progs"),
        container_package_lockstep_check(root, "snapshots-libbtrfsutil-packaged", "libbtrfsutil"),
        container_contains_check(root, "snapshots-command-asserts-btrfs", "command -v btrfs"),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-encryption-status-route",
            "/v1/security/encryption",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/encryption.rs"),
            "core-encryption-mountinfo-reader",
            "/proc/self/mountinfo",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/encryption.rs"),
            "core-encryption-crypttab-reader",
            "/etc/crypttab",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/encryption.rs"),
            "core-encryption-cryptsetup-status-reader",
            "cryptsetup\", &[\"status\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/encryption.rs"),
            "core-encryption-cryptenroll-list-reader",
            "systemd-cryptenroll\", &[\"--list\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/encryption.rs"),
            "core-encryption-no-enrollment-write",
            "executes_enrollment: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/encryption.rs"),
            "core-encryption-recovery-key-required-copy",
            "must not enable TPM-only install without escrow",
        ),
        container_package_lockstep_check(root, "encryption-cryptsetup-packaged", "cryptsetup"),
        container_package_lockstep_check(root, "encryption-tpm2-tss-packaged", "tpm2-tss"),
        container_package_not_installed_check(
            root,
            "encryption-no-systemd-cryptsetup-rpm",
            "systemd-cryptsetup",
        ),
        container_contains_check(
            root,
            "encryption-command-asserts-cryptsetup",
            "command -v cryptsetup",
        ),
        container_contains_check(
            root,
            "encryption-command-asserts-systemd-cryptenroll",
            "command -v systemd-cryptenroll",
        ),
        container_contains_check(
            root,
            "encryption-command-asserts-tpm2-tss-lib",
            "/usr/lib64/libtss2-esys.so.0",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-focus-status-route",
            "/v1/focus/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-focus-activate-route",
            "/v1/focus/activate",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-focus-deactivate-route",
            "/v1/focus/deactivate",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-focus-tick-route",
            "/v1/focus/tick",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-focus-mode-route",
            "/v1/focus/mode",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-focus-schedule-route",
            "/v1/focus/schedule",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/focus.rs"),
            "core-focus-mode-crud-validates-references",
            "Delete schedules that use this Focus mode before deleting the mode.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/focus.rs"),
            "core-focus-schedule-crud-requires-configured-mode",
            "Focus schedules must be saved with a configured mode.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-focus-status-route",
            "/v1/focus/status",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-posts-focus-activate-route",
            "/v1/focus/activate",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-posts-focus-deactivate-route",
            "/v1/focus/deactivate",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-focus-controls-source-gated",
            "append_focus_settings",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-fetches-focus-status-route",
            "/v1/focus/status",
        ),
        absent_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-focus-has-no-activate-write",
            "/v1/focus/activate",
        ),
        absent_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-focus-has-no-deactivate-write",
            "/v1/focus/deactivate",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-focus-opens-notifications-settings",
            "--panel=notifications",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-focus-uses-core-reported-modes",
            "status.modes",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-focus-no-modes-read-only-copy",
            "No Focus modes are configured yet.",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-focus-degrades-without-core",
            "Focus status is unavailable because Goblins OS core did not respond.",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-focus-tile-source-gated",
            "focus_tile_copy",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "control-center-focus-render-light-hook",
            "37b-control-center-focus.png",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "control-center-focus-render-dark-hook",
            "39b-control-center-focus-dark.png",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "control-center-focus-render-seeds-mode",
            r#"[{"id":"work","name":"Deep Work"}]"#,
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "control-center-focus-render-seeds-active-mode",
            "gsettings set org.goblins.os.focus active-mode work",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "control-center-focus-render-restores-off",
            "gsettings set org.goblins.os.focus active-mode ''",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "control-center-focus-render-restores-modes",
            "gsettings set org.goblins.os.focus modes '[]'",
        ),
        file_check(
            root,
            "crates/goblins-os-session-tools/src/bin/goblins-os-focus-tick.rs",
        ),
        file_check(root, "os/systemd-user/org.goblins.OS.FocusTick.service"),
        file_check(root, "os/systemd-user/org.goblins.OS.FocusTick.timer"),
        contains_check(
            root.join(
                "crates/goblins-os-session-tools/src/bin/goblins-os-focus-tick.rs",
            ),
            "focus-tick-helper-posts-core-route",
            "/v1/focus/tick",
        ),
        contains_check(
            root.join(
                "crates/goblins-os-session-tools/src/bin/goblins-os-focus-tick.rs",
            ),
            "focus-tick-helper-uses-exact-unix-capability",
            "initialize(ClientKind::FocusTick)",
        ),
        contains_check(
            root.join("os/systemd-user/org.goblins.OS.FocusTick.service"),
            "focus-tick-service-execstart",
            "ExecStart=/usr/libexec/goblins-os/goblins-os-focus-tick",
        ),
        contains_check(
            root.join("os/systemd-user/org.goblins.OS.FocusTick.timer"),
            "focus-tick-timer-minutely",
            "OnCalendar=minutely",
        ),
        contains_check(
            root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
            "focus-tick-session-wants-timer",
            "Wants=org.goblins.OS.FocusTick.timer",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "focus-tick-helper-installed",
            "focus-tick:goblins-os-focus-tick",
        ),
        absent_check(
            root.join("os/bootc/Containerfile"),
            "focus-tick-old-script-is-not-copied",
            "COPY --chmod=0755 os/focus/goblins-os-focus-tick",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/focus.rs"),
            "core-focus-restores-notification-banners",
            "restore-banners",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/focus.rs"),
            "core-focus-reuses-notification-bridge",
            "apply_notification_banners",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-keychain-status-route",
            "/v1/keychain/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-keychain-collections-route",
            "/v1/keychain/collections",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/keychain.rs"),
            "core-keychain-uses-secret-service-collections",
            "org.freedesktop.Secret.Service",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/keychain.rs"),
            "core-keychain-never-returns-secret-values-copy",
            "Secret values are never returned by Goblins OS.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-keychain-collections",
            "/v1/keychain/collections",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keychain-collections-row",
            "append_keychain_collections",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keychain-never-displays-secret-values",
            "Secret values are never displayed in Settings.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-app-privacy-status-route",
            "/v1/app-privacy/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-app-privacy-revoke-route",
            "/v1/app-privacy/revoke",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_permissions.rs"),
            "core-app-privacy-uses-delete-permission",
            "PermissionStore.DeletePermission",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_permissions.rs"),
            "core-app-privacy-revoke-uses-session-bridge",
            "session_bridge::permission_store_delete_permission",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_permissions.rs"),
            "core-app-privacy-revoke-validates-ids",
            "permission_id_is_safe",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-rechecks-live-state-after-helper-error",
            "wait_for_firewall_state(enabled)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-bounds-success-state-poll-to-five-seconds",
            "const FIREWALL_STATE_POLL_ATTEMPTS: usize = 20;",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-polls-at-two-hundred-fifty-milliseconds",
            "const FIREWALL_STATE_POLL_INTERVAL: Duration = Duration::from_millis(250);",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/firewall.rs"),
            "core-firewall-requires-live-systemd-and-system-bus-runtime",
            "system_control_runtime_ready()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-text-shortcuts-route",
            "/v1/text-shortcuts",
        ),
        container_package_lockstep_check(root, "bootc-package-lockstep-ibus", "ibus"),
        container_package_lockstep_check(root, "bootc-package-lockstep-ibus-gtk4", "ibus-gtk4"),
        container_package_lockstep_check(root, "bootc-package-lockstep-ibus-gtk3", "ibus-gtk3"),
        container_package_lockstep_check(root, "bootc-package-lockstep-ibus-libs", "ibus-libs"),
        container_package_lockstep_check(
            root,
            "bootc-package-lockstep-python3-ibus",
            "python3-ibus",
        ),
        container_contains_check(root, "bootc-command-asserts-ibus", "command -v ibus"),
        container_contains_check(
            root,
            "bootc-command-asserts-ibus-daemon",
            "command -v ibus-daemon",
        ),
        container_contains_check(
            root,
            "bootc-installs-textshortcuts-engine",
            "goblins-textshortcuts-engine; do",
        ),
        container_contains_check(
            root,
            "bootc-installs-textshortcuts-ibus-adapter",
            "COPY --chmod=0755 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus /usr/libexec/goblins-os/goblins-textshortcuts-ibus",
        ),
        container_contains_check(
            root,
            "bootc-installs-textshortcuts-component",
            "COPY os/goblins-os-textshortcuts/goblins-textshortcuts.xml /usr/share/ibus/component/goblins-textshortcuts.xml",
        ),
        container_contains_check(
            root,
            "bootc-seeds-textshortcuts-user-ibus-component",
            "/var/home/goblin/.local/share/ibus/component/goblins-textshortcuts.xml",
        ),
        container_contains_check(
            root,
            "bootc-writes-textshortcuts-ibus-system-cache",
            "ibus write-cache --system",
        ),
        container_contains_check(
            root,
            "bootc-asserts-textshortcuts-ibus-system-cache",
            "ibus read-cache --system | grep -Fq 'goblins-textshortcuts'",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-ibus-adapter-pycompile",
            "python3 -m py_compile /usr/libexec/goblins-os/goblins-textshortcuts-ibus",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-ibus-adapter-self-test",
            "goblins-textshortcuts-ibus --self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-ibus-adapter-runtime-self-test",
            "goblins-textshortcuts-ibus --runtime-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-ibus-adapter-overlay-intent-self-test",
            "goblins-textshortcuts-ibus --overlay-intent-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-candidate-bubble-frame-self-test",
            "goblins-textshortcuts-ibus --candidate-bubble-frame-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-candidate-bubble-layout-self-test",
            "goblins-textshortcuts-ibus --candidate-bubble-layout-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-candidate-bubble-render-intent-self-test",
            "goblins-textshortcuts-ibus --candidate-bubble-render-intent-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-gi-adapter-contract-self-test",
            "goblins-textshortcuts-ibus --gi-adapter-contract-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-adapter-callback-ledger-self-test",
            "goblins-textshortcuts-ibus --adapter-callback-ledger-self-test",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-overlay-intent-proof",
            "goblins-textshortcuts-overlay-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-frame-proof",
            "goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-layout-proof",
            "goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-proof",
            "goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-proof",
            "goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-proof",
            "goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-overlay-intent-pass",
            "grep -q '\"status\": \"pass\"' /tmp/goblins-textshortcuts-overlay-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-frame-surface",
            "grep -q '\"surface\": \"goblins-textshortcuts-accept-bubble-frame\"' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-frame-show-count",
            "grep -q '\"show_frame_count\": 2' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-frame-hide-count",
            "grep -q '\"hide_frame_count\": 2' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-frame-dismiss",
            "grep -q '\"dismissed_frame\": true' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-frame-commit",
            "grep -q '\"committed_frame\": true' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-frame-style",
            "grep -q '\"style_class\": \"gos-text-shortcuts-candidate\"' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-layout-surface",
            "grep -q '\"surface\": \"goblins-textshortcuts-accept-bubble-layout\"' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-layout-count",
            "grep -q '\"layout_count\": 4' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-layout-visible-count",
            "grep -q '\"visible_layout_count\": 3' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-layout-clamp",
            "grep -q '\"right_edge_clamped\": true' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-layout-flip",
            "grep -q '\"bottom_edge_flipped\": true' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-layout-hide-collapse",
            "grep -q '\"hidden_frame_collapses\": true' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-surface",
            "grep -q '\"surface\": \"goblins-textshortcuts-accept-bubble-render-intent\"' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-frame-surface",
            "grep -q '\"frame_surface\": \"goblins-textshortcuts-accept-bubble-frame\"' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-layout-surface",
            "grep -q '\"layout_surface\": \"goblins-textshortcuts-accept-bubble-layout\"' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-count",
            "grep -q '\"render_intent_count\": 8' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-show-count",
            "grep -q '\"show_intent_count\": 4' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-hide-count",
            "grep -q '\"hide_intent_count\": 4' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-dismiss",
            "grep -q '\"dismissed_intent\": true' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-commit",
            "grep -q '\"committed_intent\": true' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-focus-out",
            "grep -q '\"focus_out_hide\": true' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-sensitive-hide",
            "grep -q '\"sensitive_hide\": true' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-pass-through",
            "grep -q '\"pass_through_unchanged\": true' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-key-release-candidate-preservation",
            "grep -q '\"key_release_preserved_candidate\": true' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-runtime-failure-popup-cleanup",
            "grep -q '\"runtime_failure_cleanup\": true' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-fail-open",
            "grep -q '\"sink_failure_fail_open\": true' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-style",
            "grep -q '\"style_class\": \"gos-text-shortcuts-candidate\"' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-render-intent-font",
            "grep -q '\"font_family\": \"Inter\"' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-native-ibus-lookup-renderer-self-test",
            "goblins-textshortcuts-ibus --native-ibus-lookup-renderer-self-test",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-native-ibus-renderer",
            "grep -q '\"renderer\": \"native-ibus-lookup-table\"' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-native-ibus-publication",
            "grep -q '\"show_published\": true' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-native-ibus-single-candidate",
            "grep -q '\"candidate_count\": 1' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-native-ibus-system-orientation",
            "grep -q '\"system_orientation\": true' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-native-ibus-zero-visible-preedit",
            "grep -q '\"visible_preedit_updates\": 0' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-native-only-candidate-surface",
            "grep -q '\"native_only_candidate_surface\": true' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-native-ibus-redaction",
            "grep -q '\"value_redacted\": true' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-native-ibus-hide",
            "grep -q '\"hide_published\": true' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-rejects-textshortcuts-synthetic-native-overlay",
            "grep -q '\"synthetic_overlay\": false' /tmp/goblins-textshortcuts-native-ibus-lookup-renderer.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-pass",
            "grep -q '\"status\": \"pass\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-surface",
            "grep -q '\"surface\": \"goblins-textshortcuts-gi-adapter-contract\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-entrypoint",
            "grep -q '\"adapter_entrypoint\": \"/usr/libexec/goblins-os/goblins-textshortcuts-ibus\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-component-exec",
            "grep -q '\"component_exec\": \"/usr/libexec/goblins-os/goblins-textshortcuts-ibus\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-gi-import",
            "grep -q '\"gi_ibus_import\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-text-factory",
            "grep -q '\"ibus_text_factory\": \"IBus.Text.new_from_string\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-key-unicode",
            "grep -q '\"ibus_key_unicode\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-focus-in",
            "grep -q '\"focus-in\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-process-key",
            "grep -q '\"process-key-event\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-content-type",
            "grep -q '\"set-content-type\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-focus-out",
            "grep -q '\"focus-out\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-reset",
            "grep -q '\"reset\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-candidate-metadata",
            "grep -q '\"candidate_metadata\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-zero-visible-preedit",
            "grep -q '\"visible_preedit_updates\": 0' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-native-only-candidate",
            "grep -q '\"native_only_candidate_surface\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-commit",
            "grep -q '\"boundary_commit\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-dismiss",
            "grep -q '\"escape_dismiss\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-pass-through",
            "grep -q '\"pass_through_default\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-password-refusal",
            "grep -q '\"password_refusal\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-render-surface",
            "grep -q '\"render_intent_surface\": \"goblins-textshortcuts-accept-bubble-render-intent\"' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-gi-adapter-contract-render-count",
            "grep -q '\"render_intent_count\": 8' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-candidate-bubble-frame-sensitive-refusal",
            "grep -q '\"sensitive_field_refusal\": true' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-overlay-live-claim-false",
            "grep -q '\"live_overlay_claim\": false' /tmp/goblins-textshortcuts-overlay-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-render-claim-false",
            "grep -q '\"rendered_bubble_ready_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-live-claim-false",
            "grep -q '\"live_overlay_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-runtime-claim-false",
            "grep -q '\"runtime_ready_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-frame.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-layout-render-claim-false",
            "grep -q '\"rendered_bubble_ready_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-layout-live-claim-false",
            "grep -q '\"live_overlay_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-layout-runtime-claim-false",
            "grep -q '\"runtime_ready_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-layout.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-render-intent-render-claim-false",
            "grep -q '\"rendered_bubble_ready_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-render-intent-live-claim-false",
            "grep -q '\"live_overlay_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-candidate-bubble-render-intent-runtime-claim-false",
            "grep -q '\"runtime_ready_claim\": false' /tmp/goblins-textshortcuts-candidate-bubble-render-intent.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-gi-adapter-contract-render-claim-false",
            "grep -q '\"rendered_bubble_ready_claim\": false' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-gi-adapter-contract-live-claim-false",
            "grep -q '\"live_overlay_claim\": false' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-gi-adapter-contract-bus-claim-false",
            "grep -q '\"live_ibus_bus_claim\": false' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-gi-adapter-contract-text-input-claim-false",
            "grep -q '\"text_input_v3_claim\": false' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-gi-adapter-contract-runtime-claim-false",
            "grep -q '\"runtime_ready_claim\": false' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-pass",
            "grep -q '\"status\": \"pass\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-surface",
            "grep -q '\"surface\": \"goblins-textshortcuts-adapter-callback-ledger\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-focus-in",
            "grep -q '\"focus-in\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-process-key",
            "grep -q '\"process-key-event\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-content-type",
            "grep -q '\"set-content-type\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-focus-out",
            "grep -q '\"focus-out\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-reset",
            "grep -q '\"reset\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-zero-visible-preedit",
            "grep -q '\"visible_preedit_updates\": 0' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-metadata-only-candidate",
            "grep -q '\"metadata_only_candidate_response\": true' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-delete",
            "grep -q '\"delete-surrounding-text\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-commit",
            "grep -q '\"commit-text\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-hide",
            "grep -q '\"hide-preedit-text\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-render-count",
            "grep -q '\"render_intent_count\":' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-value-redacted",
            "grep -q '\"value_redacted\": true' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-callback-ledger-no-text",
            "grep -q '\"no_typed_text_logged\": true' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-adapter-callback-ledger-render-claim-false",
            "grep -q '\"rendered_bubble_ready_claim\": false' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-adapter-callback-ledger-live-claim-false",
            "grep -q '\"live_overlay_claim\": false' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-adapter-callback-ledger-bus-claim-false",
            "grep -q '\"live_ibus_bus_claim\": false' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-adapter-callback-ledger-text-input-claim-false",
            "grep -q '\"text_input_v3_claim\": false' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-adapter-callback-ledger-runtime-claim-false",
            "grep -q '\"runtime_ready_claim\": false' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-rejects-textshortcuts-adapter-callback-ledger-trigger-leak",
            "! grep -q 'omw' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-rejects-textshortcuts-adapter-callback-ledger-replacement-leak",
            "! grep -q 'on my way' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-ibus-adapter-capability-check",
            "goblins-textshortcuts-ibus --capability-check",
        ),
        container_contains_check(
            root,
            "bootc-requires-textshortcuts-adapter-contract-ready",
            "grep -q '\"adapter_contract_ready\": true'",
        ),
        container_contains_check(
            root,
            "bootc-keeps-textshortcuts-runtime-ready-claim-false",
            "grep -q '\"runtime_ready_claim\": false'",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-component-check",
            "goblins-textshortcuts-engine --component-check /usr/share/ibus/component/goblins-textshortcuts.xml",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-keystroke-self-test",
            "goblins-textshortcuts-engine --keystroke-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-table-watch-self-test",
            "goblins-textshortcuts-engine --table-watch-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-content-purpose-self-test",
            "goblins-textshortcuts-engine --content-purpose-self-test",
        ),
        container_contains_check(
            root,
            "bootc-runs-textshortcuts-stdio-self-test",
            "goblins-textshortcuts-engine --stdio-self-test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts.xml"),
            "textshortcuts-component-engine-name",
            "<name>goblins-textshortcuts</name>",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts.xml"),
            "textshortcuts-component-engine-exec",
            "/usr/libexec/goblins-os/goblins-textshortcuts-ibus",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-requires-ibus-gi",
            "gi.require_version(\"IBus\", \"1.0\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-delegates-to-stdio-runtime",
            "RUNTIME_PATH = \"/usr/libexec/goblins-os/goblins-textshortcuts-engine\"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-uses-stdio",
            "return [_runtime_binary(), \"--stdio\"]",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-fail-open-timeout",
            "runtime did not complete one response before timeout",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-bounds-request-frames",
            "RUNTIME_REQUEST_MAX_BYTES = 64 * 1024",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-bounds-response-frames",
            "RUNTIME_RESPONSE_MAX_BYTES = 64 * 1024",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-uses-nonblocking-runtime-stdin",
            "os.set_blocking(self._process.stdin.fileno(), False)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-uses-nonblocking-runtime-stdout",
            "os.set_blocking(self._process.stdout.fileno(), False)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-has-bounded-request-frame-writer",
            "def _write_request_frame",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-has-bounded-response-frame-reader",
            "def _read_response_frame",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-rejects-extra-runtime-framing",
            "runtime returned extra response framing",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-restarts-runtime-with-bounded-backoff",
            "RUNTIME_RESTART_MAX_SECONDS = 5.0",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-requires-surrounding-text-capability",
            "\"SURROUNDING_TEXT\"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-binds-replacement-to-exact-trigger",
            "trigger = delete.get(\"expected_text\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-validates-whole-replacement-transaction",
            "def _valid_replacement_transaction",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-binds-commit-to-shown-replacement",
            "operations[1].get(\"text\") != expected_commit",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-swapped-replacement-refusal",
            "swapped_commit[\"operations\"][1][\"text\"] = \"different replacement \"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-strictly-decodes-runtime-operations",
            "def _validated_runtime_response_operations",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-validates-response-against-request",
            "def _runtime_response_valid_for_request",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-rejects-error-responses-before-application",
            "if operations is None or response.get(\"error\") is not None:",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-rejects-unknown-runtime-response-fields",
            "set(response) - allowed_keys",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-pins-destructive-operation-sequence",
            "[\"delete-surrounding-text\", \"commit-text\", \"hide-preedit-text\"]",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-does-not-coerce-runtime-preedit-text",
            "text_factory(str(operation.get(\"text\", \"\")))",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-does-not-coerce-runtime-cursor-position",
            "int(operation.get(\"cursor_pos\", 0))",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-does-not-coerce-runtime-visible-state",
            "bool(operation.get(\"visible\", False))",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-does-not-coerce-runtime-delete-offset",
            "int(operation.get(\"offset\", 0))",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-does-not-coerce-runtime-delete-length",
            "int(operation.get(\"n_chars\", 0))",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-applies-validated-runtime-text-directly",
            "text_factory(operation[\"text\"])",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-never-applies-visible-preedit",
            "target.update_preedit_text(",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-applies-validated-runtime-delete-directly",
            "operation[\"offset\"]",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-bounds-runtime-text-by-characters",
            "RUNTIME_TEXT_MAX_CHARACTERS = 64 * 1024",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-bounds-runtime-text-by-utf8-bytes",
            "RUNTIME_TEXT_MAX_BYTES = 64 * 1024",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-bounds-surrounding-text-by-utf8-bytes",
            "SURROUNDING_TEXT_MAX_BYTES = 64 * 1024",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-strictly-encodes-runtime-text",
            "value.encode(\"utf-8\", errors=\"strict\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-rejects-surrogate-keysyms",
            "0xD800 <= codepoint <= 0xDFFF",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-fail-opens-request-encoding-errors",
            "UnicodeEncodeError,",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-fail-opens-json-value-errors",
            "ValueError,",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-unencodable-request",
            "unencodable_request_runtime = RuntimeBridge(",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-huge-json-integer",
            "huge_integer_frame = (",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-huge-private-table-integer",
            "table_file.write(b\"[\" + (b\"9\" * 5000) + b\"]\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-preconstructs-text-before-editing",
            "prepared_text: dict[int, Any] = {}",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-zero-mutation-on-factory-failure",
            "assert factory_failure_target.calls == []",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-owns-focus-bound-surrounding-snapshots",
            "class FocusBoundSurroundingTextCache",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-stores-validated-callback-snapshot",
            "self._surrounding_text_cache.observe(snapshot)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-reads-only-owned-focus-snapshot",
            "snapshot = self._surrounding_text_cache.current()",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-never-reads-stale-ibus-base-cache",
            "def _read_surrounding_text",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-focus-transition-cache-refusal",
            "snapshot_cache.end_focus()",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-propagates-render-publication-result",
            "return self._sink.publish(record) is not False",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-does-not-log-render-exception-content",
            "render intent sink failed: {error}",
        ),
        absent_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-does-not-log-hide-exception-content",
            "forced candidate hide failed: {error}",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-disarms-on-publication-failure",
            "self._clear_candidate_ui(\"runtime-operation-application-failed\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-failed-publication-disarm",
            "def assert_failed_candidate_publication_disarms",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-native-show-failure",
            "\"native_show_failure_reported\": native_show_failure_reported",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-retries-native-hide",
            "\"retryable_force_hide\": retryable_force_hide",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-propagates-table-change-application-result",
            "table_changed, table_applied = _send_table_changed_if_needed(",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-cleans-up-failed-table-change-hide",
            "self._clear_candidate_ui(\"table-change-application-failed\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-table-change-hide-retry",
            "\"table_change_hide_retry\": table_change_hide_retry",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-pins-hidden-text-hint-bit",
            "IBUS_INPUT_HINT_HIDDEN_TEXT_FALLBACK = 1 << 12",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-pins-private-input-hint-bit",
            "IBUS_INPUT_HINT_PRIVATE_FALLBACK = 1 << 11",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-fail-closes-private-and-hidden-input",
            "def _effective_content_purpose",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-hidden-text-hint-refusal",
            "IBUS_INPUT_HINT_HIDDEN_TEXT_FALLBACK,",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-tests-invalid-content-type-fails-closed",
            "_effective_content_purpose(0, \"invalid\", FakeIbus) == 8",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-rejects-content-type-outside-guint32",
            "purpose_value > 0xFFFFFFFF",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-rejects-nul-triggers",
            "replace.contains('\\0')",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-rejects-nul-replacements",
            "with_text.contains('\\0')",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-handles-disable-lifecycle",
            "def do_disable",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-disable-invalidates-runtime-status",
            "self._runtime_status.set_enabled(False)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-disable-clears-candidate-ui",
            "self._clear_candidate_ui(\"disabled\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-locally-clears-stale-candidate-ui",
            "def _clear_candidate_ui",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-local-clear-hides-native-candidate",
            "self._candidate_render.clear(self._candidate_state, reason)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-failure-clears-candidate-ui",
            "self._clear_candidate_ui(\"runtime-health-failed\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-generation-change-clears-candidate-ui",
            "self._clear_candidate_ui(\"runtime-generation-changed\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-publishes-redacted-runtime-status",
            "class RuntimeStatusPublisher",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-pins-v1-schema",
            "RUNTIME_STATUS_SCHEMA = \"goblins-os.text-shortcuts-runtime-status.v1\"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-pins-fixed-private-path",
            "RUNTIME_STATUS_PATH = \"/run/goblins-os-session/text-shortcuts-runtime-status.json\"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-bounds-record",
            "RUNTIME_STATUS_MAX_BYTES = 4096",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-emits-enabled-state",
            "\"enabled\": self._enabled",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-emits-surrounding-text-state",
            "\"surrounding_text_supported\": self._surrounding_text_supported",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-emits-snapshot-state",
            "\"snapshot_valid\": self._snapshot_valid",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-writes-owner-only-file",
            "os.fchmod(descriptor, 0o600)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-syncs-file-before-rename",
            "os.fsync(descriptor)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-atomically-renames-record",
            "os.rename(",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-syncs-parent-directory",
            "os.fsync(directory)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-runtime-bridge-updates-status-publisher",
            "health_callback=self._runtime_status.runtime_transport",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-tracks-enabled-state",
            "def set_enabled",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-tracks-surrounding-text-support",
            "def set_surrounding_text_supported",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-status-publisher-tracks-snapshot-validity",
            "def set_snapshot_valid",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-heartbeats-focused-runtime",
            "def _runtime_health_tick",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-process-key-event",
            "def do_process_key_event",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-content-purpose",
            "def do_set_content_type",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-updates-preedit",
            "update_preedit_text",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-deletes-surrounding-text",
            "delete_surrounding_text",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-commits-text",
            "commit_text",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-registers-factory-engine",
            "factory.add_engine(ENGINE_NAME, GoblinsTextShortcutsEngine)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-self-test",
            "goblins_textshortcuts_ibus_adapter_selftest ok",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-self-test",
            "goblins_textshortcuts_ibus_runtime_selftest ok",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-self-test-uses-bridge",
            "RuntimeBridge(response_timeout=CAPABILITY_TIMEOUT_SECONDS)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-self-test-sensitive-refusal",
            "\"focus-in\", \"purpose\": 9",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-self-test-dismisses-with-escape",
            "_special_key_protocol_event(0xFF1B)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-self-test-covers-partial-frame-timeout",
            "partial_runtime = RuntimeBridge(",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-self-test-covers-oversize-frame",
            "oversized_runtime = RuntimeBridge(",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-self-test-rejects-malformed-responses",
            "malformed_runtime_responses = [",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-self-test-covers-relocated-trigger",
            "same_trigger_elsewhere =",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-applies-guarded-effective-response",
            "effective = self._surrounding_guard.validate_response(",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-metadata-parser",
            "def _candidate_metadata_from_response",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-state",
            "class CandidateMetadataState",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-overlay-intent",
            "def _candidate_overlay_intent",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-overlay-intent-self-test",
            "def _run_overlay_intent_self_test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-overlay-intent-proof-surface",
            "goblins-textshortcuts-ibus-adapter-overlay-intent",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-frame-surface",
            "goblins-textshortcuts-accept-bubble-frame",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-layout-surface",
            "goblins-textshortcuts-accept-bubble-layout",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-surface",
            "goblins-textshortcuts-accept-bubble-render-intent",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-frame-builder",
            "def _candidate_bubble_frame_from_intent",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-layout-builder",
            "def _candidate_bubble_layout_from_frame",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-frame-self-test",
            "def _run_candidate_bubble_frame_self_test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-layout-self-test",
            "def _run_candidate_bubble_layout_self_test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-controller",
            "class CandidateBubbleRenderIntentController",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-sink",
            "class CandidateBubbleRenderIntentSink",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-apply-wrapper",
            "def _apply_response_operations_with_render_intent",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-self-test",
            "def _run_candidate_bubble_render_intent_self_test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-frame-style",
            "gos-text-shortcuts-candidate",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-frame-cli",
            "--candidate-bubble-frame-self-test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-layout-cli",
            "--candidate-bubble-layout-self-test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-cli",
            "--candidate-bubble-render-intent-self-test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-native-lookup-renderer",
            "class NativeIbusLookupRenderer",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-native-lookup-table",
            "self._ibus.LookupTable.new",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-native-lookup-publication",
            "self._engine.update_lookup_table(lookup_table, True)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-native-pointer-acceptance",
            "def do_candidate_clicked",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-typed-candidate-acceptance",
            "{\"type\": \"accept-candidate\"}",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-key-release-preserves-candidate",
            "\"key_release_preserved_candidate\": key_release_preserved_candidate",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-failure-cleans-popup",
            "\"runtime_failure_cleanup\": runtime_failure_cleanup",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-runtime-unavailable-marker",
            "\"runtime_available\": False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-native-renderer-cli",
            "--native-ibus-lookup-renderer-self-test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-cli",
            "--gi-adapter-contract-self-test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-cli",
            "--adapter-callback-ledger-self-test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-surface",
            "goblins-textshortcuts-gi-adapter-contract",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-surface",
            "goblins-textshortcuts-adapter-callback-ledger",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-self-test",
            "def _run_gi_adapter_contract_self_test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-self-test",
            "def _run_adapter_callback_ledger_self_test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-class",
            "class RedactedAdapterCallbackLedger",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-env-gate",
            "GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-redacted",
            "\"value_redacted\": True",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-no-typed-text",
            "\"no_typed_text_logged\": True",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-process-key",
            "self._record_callback(\"process-key-event\")",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-operation-types",
            "\"operation_types\": operation_types",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-no-live-bus-claim",
            "\"live_ibus_bus_claim\": False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-callback-ledger-no-text-input-claim",
            "\"text_input_v3_claim\": False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-imports-ibus",
            "from gi.repository import IBus",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-text-factory",
            "IBus.Text.new_from_string",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-key-unicode",
            "\"ibus_key_unicode\": True",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-zero-visible-preedit",
            "\"visible_preedit_updates\": visible_preedit_updates",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-commit",
            "\"boundary_commit\": boundary_commit",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-dismiss",
            "\"escape_dismiss\": escape_dismiss",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-pass-through",
            "\"pass_through_default\": pass_through_default",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-password-refusal",
            "\"password_refusal\": password_refusal",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-render-count",
            "\"render_intent_count\": render_contract[\"render_intent_count\"]",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-no-live-bus-claim",
            "\"live_ibus_bus_claim\": False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-gi-contract-no-text-input-claim",
            "\"text_input_v3_claim\": False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-layout-clamps-right-edge",
            "\"right_edge_clamped\": edge_layout[\"clamped_x\"] is True",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-layout-flips-bottom-edge",
            "\"bottom_edge_flipped\": bottom_layout[\"flipped_y\"] is True",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-layout-collapses-hide-frame",
            "\"hidden_frame_collapses\": hidden_layout[\"visible\"] is False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-counts",
            "\"render_intent_count\": len(sink.records)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-focus-out",
            "\"focus_out_hide\": focus_out_hide",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-sensitive-hide",
            "\"sensitive_hide\": sensitive_hide",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-pass-through",
            "\"pass_through_unchanged\": pass_through_unchanged",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-bubble-render-intent-fail-open",
            "\"sink_failure_fail_open\": sink_failure_fail_open",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-overlay-intent-ledger",
            "last_overlay_intent",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-overlay-no-live-claim",
            "\"live_overlay_claim\": False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-overlay-hide-on-dismiss",
            "\"reason\": \"dismissed\"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-overlay-hide-on-commit",
            "\"reason\": \"committed\"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-overlay-intent-proof-counts",
            "\"show_count\": show_count",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-candidate-false-render-claim",
            "rendered_bubble_ready_claim is not False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-live-retains-candidate-state",
            "self._candidate_state = CandidateMetadataState()",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-live-retains-render-intent-controller",
            "self._candidate_render = CandidateBubbleRenderIntentController(",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-live-clears-render-intent-controller",
            "self._candidate_render.clear(self._candidate_state",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-capability-check",
            "--capability-check",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-capability-json",
            "def capability_payload",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-stdio-contract-self-test",
            "--stdio-self-test",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-contract-ready-key",
            "\"adapter_contract_ready\": adapter_contract_ready",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-keeps-runtime-ready-claim-false",
            "\"runtime_ready_claim\": False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-does-not-claim-live-ready",
            "\"ready\": False",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-table-bridge",
            "class ShortcutTableBridge",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-table-config-path",
            "TEXT_SHORTCUTS_CONFIG_FILE = \"text-shortcuts.json\"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-sanitizes-table",
            "def _sanitize_shortcuts",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-sends-table-changed",
            "\"type\": \"table-changed\"",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-adapter-polls-table-before-input",
            "def _send_table_changed_if_needed",
        ),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-includes-textshortcuts-assets",
            "os/goblins-os-textshortcuts/",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "textshortcuts-dconf-seeds-goblins-ibus-source",
            "sources=[('ibus', 'goblins-textshortcuts')]",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "textshortcuts-dconf-preloads-goblins-engine",
            "preload-engines=['goblins-textshortcuts']",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "textshortcuts-dconf-disables-per-window-source-switching",
            "per-window=false",
        ),
        file_check(root, "os/input/goblins-os-input-source-seed"),
        contains_check(
            root.join("os/release/source-tree-manifest.toml"),
            "source-manifest-includes-input-seed-assets",
            "os/input/",
        ),
        container_contains_check(
            root,
            "container-installs-input-source-seed-helper",
            "COPY --chmod=0755 os/input/goblins-os-input-source-seed /usr/libexec/goblins-os/goblins-os-input-source-seed",
        ),
        container_contains_check(
            root,
            "container-syntax-checks-input-source-seed-helper",
            "bash -n /usr/libexec/goblins-os/goblins-os-input-source-seed",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-uses-one-shot-marker",
            "input-source-seeded",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-appends-goblins-source",
            "set_and_verify_input_sources sources \"$canonical_sources\" \"$next_sources\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-appends-goblins-preload",
            "set_and_verify_preload \"$canonical_preload\" \"$next_preload\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-checks-writable-values-not-only-exit-status",
            "[ \"$value\" = \"true\" ]",
        ),
        absent_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-does-not-mask-read-failures",
            "|| true",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-strictly-parses-existing-values",
            "parsed = ast.literal_eval(raw)",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-accepts-typed-empty-input-source-arrays",
            "annotation = \"@a(ss) \"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-accepts-typed-empty-preload-arrays",
            "annotation = \"@as \"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-rejects-malformed-existing-values",
            "input source settings were malformed; leaving every source unchanged",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-preserves-existing-source-order-and-duplicates",
            "sources.append((item[0], item[1]))",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-preserves-existing-preload-order-and-duplicates",
            "engines.append(item)",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-verifies-each-write",
            "set_and_verify_input_sources()",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-reads-sources-before-transaction",
            "current_sources=\"$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null)\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-reads-mru-before-transaction",
            "current_mru=\"$(gsettings get org.gnome.desktop.input-sources mru-sources 2>/dev/null)\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-reads-preload-before-transaction",
            "current_preload=\"$(gsettings get org.freedesktop.ibus.general preload-engines 2>/dev/null)\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-rolls-back-partial-writes",
            "rollback_originals()",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-rolls-back-only-touched-keys",
            "if [ \"$touched\" != \"1\" ]",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-refuses-to-overwrite-concurrent-rollback-changes",
            "[ \"$canonical\" = \"$staged\" ] || return 1",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-verifies-forward-sources-write",
            "set_and_verify_input_sources sources \"$canonical_sources\" \"$next_sources\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-verifies-forward-mru-write",
            "set_and_verify_input_sources mru-sources \"$canonical_mru\" \"$next_mru\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-verifies-forward-preload-write",
            "set_and_verify_preload \"$canonical_preload\" \"$next_preload\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-conditionally-restores-sources",
            "restore_input_sources_if_unchanged sources \"$sources_touched\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-conditionally-restores-mru",
            "restore_input_sources_if_unchanged mru-sources \"$mru_touched\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-verifies-restored-preload",
            "restore_preload_if_unchanged \"$preload_touched\"",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-has-exact-marker-token",
            "printf 'seeded %s/%s\\n'",
        ),
        ordered_contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-writes-marker-only-after-verified-transaction",
            "if ! set_and_verify_input_sources sources",
            "printf 'seeded %s/%s\\n'",
        ),
        file_check(root, "os/systemd-user/org.goblins.OS.InputSourcesSeed.service"),
        contains_check(
            root.join("os/systemd-user/org.goblins.OS.InputSourcesSeed.service"),
            "input-source-seed-user-service-exec",
            "ExecStart=/usr/libexec/goblins-os/goblins-os-input-source-seed",
        ),
        contains_check(
            root.join("os/systemd-user/org.goblins.OS.InputSourcesSeed.service"),
            "input-source-seed-runs-before-ibus",
            "Before=org.freedesktop.IBus.session.GNOME.service",
        ),
        path_absent_check(
            root,
            "os/systemd-user/org.goblins.OS.IBus.service",
            "textshortcuts-custom-ibus-user-service-absent",
        ),
        contains_check(
            root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
            "textshortcuts-input-source-seed-wanted-by-session",
            "Wants=org.goblins.OS.InputSourcesSeed.service",
        ),
        contains_check(
            root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
            "textshortcuts-fedora-gnome-ibus-service-wanted-by-session",
            "Wants=org.freedesktop.IBus.session.GNOME.service",
        ),
        contains_check(
            root.join("os/session/goblins-os-session"),
            "textshortcuts-session-imports-user-service-display-env",
            "systemctl --user import-environment",
        ),
        contains_check(
            root.join("os/session/goblins-os-session"),
            "textshortcuts-session-updates-dbus-activation-display-env",
            "dbus-update-activation-environment --systemd",
        ),
        contains_check(
            root.join("os/session/goblins-os-session"),
            "textshortcuts-session-imports-wayland-display-env",
            "WAYLAND_DISPLAY",
        ),
        container_absent_check(
            root,
            "bootc-does-not-force-gtk-simple-input-method",
            "GTK_IM_MODULE=gtk-im-context-simple",
        ),
        container_absent_check(
            root,
            "bootc-does-not-force-qt-simple-input-method",
            "QT_IM_MODULE=simple",
        ),
        container_absent_check(
            root,
            "bootc-does-not-force-xim-disabled",
            "XMODIFIERS=@im=none",
        ),
        container_contains_check(
            root,
            "bootc-keeps-ibus-sync-mode-conservative",
            "IBUS_ENABLE_SYNC_MODE=0",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-requires-ibus-component",
            "/usr/share/ibus/component/goblins-textshortcuts.xml",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-requires-engine-binary",
            "/usr/libexec/goblins-os/goblins-textshortcuts-engine",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-engine-honest-ibus-only",
            "ibus_available\n        && component_registered\n        && engine_binary_available\n        && input_source_configured\n        && runtime_loop_available",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-requires-configured-input-source",
            "text_shortcuts_input_source_configured",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-input-source-read-uses-session-bridge",
            "input_source_configured_from_bridge(session_bridge::gsettings(&[",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-does-not-run-gsettings-from-system-service",
            "bounded_session_command_output(\"gsettings\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-runtime-pending-honesty",
            "lacks an active, fresh focused runtime response in this session",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-runtime-probes-live-ibus-engine",
            "session_bridge::ibus_engine()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-runtime-requires-fresh-adapter-status",
            "session_bridge::text_shortcuts_runtime_status()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-runtime-requires-ready-adapter-child",
            "TextShortcutsRuntimeStatusResult::Success(status) if status.ready()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-has-pathless-runtime-status-op",
            "TextShortcutsRuntimeStatus",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-runtime-status-pins-v1-schema",
            "const TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA: &str = \"goblins-os.text-shortcuts-runtime-status.v1\";",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-rechecks-runtime-status-freshness",
            "TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-rechecks-runtime-status-future-skew",
            "const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS: u64 = 250_000_000;",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-runtime-status-ready-requires-enabled",
            "&& self.enabled",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-runtime-status-ready-requires-surrounding-text",
            "&& self.surrounding_text_supported",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-runtime-status-ready-requires-valid-snapshot",
            "&& self.snapshot_valid",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-accepts-ibus-engine-op",
            "IbusEngine",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-uses-fixed-private-path",
            "/run/goblins-os-session/text-shortcuts-runtime-status.json",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-pins-v1-schema",
            "const TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA: &str = \"goblins-os.text-shortcuts-runtime-status.v1\";",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-pins-four-kibibyte-envelope",
            "const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES: usize = 4 * 1024;",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-pins-future-skew",
            "const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS: u64 = 250_000_000;",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-carries-enabled-state",
            "enabled: bool,",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-carries-surrounding-text-state",
            "surrounding_text_supported: bool,",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-carries-snapshot-state",
            "snapshot_valid: bool,",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-op-is-pathless",
            "TextShortcutsRuntimeStatus {}",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-open-is-nofollow",
            ".custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-requires-owner-only-mode",
            "metadata.mode() & 0o7777 != TEXT_SHORTCUTS_RUNTIME_STATUS_MODE",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-is-strict-bounded-and-fresh",
            "TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-bounds-read-before-json-decode",
            ".take((TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES + 1) as u64)",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-rechecks-device-after-read",
            "before.dev() != after.dev()",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-rechecks-inode-after-read",
            "before.ino() != after.ino()",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-rechecks-length-after-read",
            "before.len() != after.len()",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-requires-complete-read",
            "after.len() != encoded.len() as u64",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-has-adversarial-file-tests",
            "text_shortcuts_runtime_status_rejects_symlinks_and_oversize_files",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-tests-stale-and-future-timestamps",
            "text_shortcuts_runtime_status_rejects_stale_and_materially_future_timestamps",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-tests-malformed-and-unknown-fields",
            "text_shortcuts_runtime_status_rejects_malformed_and_unknown_fields",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-tests-all-readiness-signals",
            "text_shortcuts_runtime_status_preserves_all_readiness_signals",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-tests-owner-and-mode",
            "text_shortcuts_runtime_status_rejects_wrong_mode_and_owner",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-runtime-status-rejects-fifo-without-blocking",
            "text_shortcuts_runtime_status_rejects_fifo_without_blocking",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-ibus-probe-timeout-copy-is-truthful",
            "IBus did not answer before the session bridge input timeout.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-autocorrect-status",
            "pub struct TextShortcutsAutocorrectStatus",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-autocorrect-model-gate",
            "GOBLINS_TEXTSHORTCUTS_AUTOCORRECT_MODEL",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-autocorrect-hunspell-gate",
            "Hunspell dictionary",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-autocorrect-keeps-disabled",
            "enabled: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/Cargo.toml"),
            "core-text-shortcuts-depends-on-engine-substrate",
            "goblins-os-textshortcuts-engine",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-reuses-engine-sanitizer",
            "sanitize_shortcuts",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-reads-desktop-table-through-session-bridge",
            "session_bridge::text_shortcuts_read()",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-writes-desktop-table-through-session-bridge",
            "session_bridge::text_shortcuts_write(&table)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-bounds-preview-trigger",
            "MAX_PREVIEW_TRIGGER_BYTES: usize = 256",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-text-shortcuts-bounds-http-request-envelope",
            "TEXT_SHORTCUTS_REQUEST_LIMIT_BYTES: usize = 64 * 1024",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-text-shortcuts-applies-route-local-body-limit",
            ".layer(DefaultBodyLimit::max(TEXT_SHORTCUTS_REQUEST_LIMIT_BYTES))",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-text-shortcuts-tests-route-local-413",
            "text_shortcuts_route_rejects_bodies_above_its_private_table_envelope",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-has-no-service-home-table-path",
            "fn table_path()",
        ),
        absent_check(
            root.join("crates/goblins-os-core/src/text_shortcuts.rs"),
            "core-text-shortcuts-does-not-write-service-home",
            "fs::write(",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-has-typed-text-shortcuts-read",
            "TextShortcutsRead",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "core-session-bridge-has-typed-text-shortcuts-write",
            "TextShortcutsWrite",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/Cargo.toml"),
            "session-bridge-reuses-text-shortcuts-engine-store",
            "goblins-os-textshortcuts-engine",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-rejects-unknown-request-fields",
            "deny_unknown_fields",
        ),
        contains_check(
            root.join("crates/goblins-os-session-bridge/src/main.rs"),
            "session-bridge-bounds-request-before-decoding",
            "read_to_end_before(",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/control_plane.rs"),
            "release-proof-can-roundtrip-text-shortcuts-write",
            "(POST, \"/v1/text-shortcuts\")",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/control_plane.rs"),
            "release-proof-can-preview-text-shortcuts",
            "(GET, \"/v1/text-shortcuts/preview\")",
        ),
        contains_check(
            root.join("Cargo.toml"),
            "workspace-textshortcuts-engine-crate",
            "crates/goblins-os-textshortcuts-engine",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/Cargo.toml"),
            "textshortcuts-engine-bin-name",
            "name = \"goblins-textshortcuts-engine\"",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-boundary-commit",
            "CommitReplacement",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-deletes-trigger-before-commit",
            "delete_previous_chars",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-serializes-exact-trigger-for-delete",
            "expected_text: expected_text.clone()",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-has-nonmutating-health-request",
            "RuntimeProtocolRequest::Health => Ok(IbusRuntimeEvent::Health)",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-health-event-passes-through-without-edit",
            "IbusRuntimeEvent::Health => IbusRuntimeDecision::pass_through(),",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-tests-health-does-not-mutate-edit-state",
            "runtime_protocol_health_is_typed_and_does_not_mutate_edit_state",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-adapter",
            "ibus_runtime_decision",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-key-event-normalizer",
            "input_event_from_ibus_key",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-key-backspace-mapping",
            "IBUS_KEY_BACKSPACE => InputEvent::Backspace",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-key-command-reset",
            "command_modifier_active",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-key-boundary-mapping",
            "IBUS_KEY_RETURN => InputEvent::Boundary('\\n')",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-key-escape-dismiss-mapping",
            "IBUS_KEY_ESCAPE => InputEvent::DismissCandidate",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-pipeline",
            "pub struct IbusTextShortcutsRuntime",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-pipeline-normalizes-keys",
            "input_event_from_ibus_key(event)",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-pipeline-honors-content-purpose",
            "set_content_purpose",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-pipeline-reuses-operation-adapter",
            "ibus_runtime_decision(action)",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store",
            "pub struct TextShortcutTableStore",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-default-file",
            "TEXT_SHORTCUTS_CONFIG_FILE: &str = \"text-shortcuts.json\"",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-missing-degrades",
            "TableLoadStatus::Missing",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-invalid-degrades",
            "TableLoadStatus::InvalidJson",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-size-limit",
            "MAX_TEXT_SHORTCUTS_TABLE_BYTES: usize = 48 * 1024",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-bounded-read",
            ".take((MAX_TEXT_SHORTCUTS_TABLE_BYTES + 1) as u64)",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-refuses-symlinks",
            "libc::O_CLOEXEC | libc::O_NOFOLLOW",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-requires-private-mode",
            "metadata.mode() & 0o7777 == 0o600",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-atomic-replace",
            "std::fs::rename(&temporary_path, &self.path)",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-syncs-file",
            "temporary.sync_all()",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-store-private-parent",
            "Permissions::from_mode(0o700)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-bounds-private-table",
            "MAX_TEXT_SHORTCUTS_TABLE_BYTES = 48 * 1024",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-requires-absolute-config-home",
            "not os.path.isabs(config_home)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-requires-absolute-home",
            "not os.path.isabs(home)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-refuses-table-symlinks",
            "getattr(os, \"O_NOFOLLOW\", 0)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-requires-private-table-owner",
            "metadata.st_uid != os.geteuid()",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-requires-private-table-mode",
            "stat.S_IMODE(metadata.st_mode) != 0o600",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-rejects-nonstring-trigger-fields",
            "not isinstance(replace_value, str)",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-ibus-rejects-nonstring-replacement-fields",
            "not isinstance(with_value, str)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/session_bridge.rs"),
            "textshortcuts-core-maps-bridge-transport-outages-to-unavailable",
            "DetailedBridgeResult::TransportUnavailable",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-refreshes-table",
            "pub fn refresh_table",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-fingerprint",
            "pub enum TableFingerprint",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-watch-poller",
            "pub struct TextShortcutTableWatcher",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-watch-outcome",
            "pub enum TableWatchOutcome",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-table-watch-self-test-contract",
            "pub fn run_text_shortcuts_table_watch_self_test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-ibus-password-purpose",
            "IBUS_INPUT_PURPOSE_PASSWORD: u32 = 8",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-ibus-pin-purpose",
            "IBUS_INPUT_PURPOSE_PIN: u32 = 9",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-content-purpose-decoder",
            "content_purpose_from_ibus_input_purpose",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-content-purpose-self-test-contract",
            "pub fn run_text_shortcuts_content_purpose_self_test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-stdio-protocol-request",
            "pub enum RuntimeProtocolRequest",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-stdio-protocol-operation",
            "pub enum RuntimeProtocolOperation",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-stdio-runtime-loop",
            "pub fn run_text_shortcuts_stdio_runtime",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-stdio-self-test-contract",
            "pub fn run_text_shortcuts_stdio_self_test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-stdio-json-kebab-contract",
            "rename_all = \"kebab-case\"",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-event-router",
            "pub enum IbusRuntimeEvent",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-event-focus-out",
            "IbusRuntimeEvent::FocusOut",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-event-reset",
            "IbusRuntimeEvent::Reset",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-event-content-purpose",
            "IbusRuntimeEvent::ContentPurposeChanged",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-event-table-change",
            "IbusRuntimeEvent::TableChanged",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-keystroke-self-test-contract",
            "pub fn run_text_shortcuts_keystroke_self_test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-keystroke-self-test-errors",
            "pub enum KeystrokeSelfTestError",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-cli-reuses-table-store",
            "TextShortcutTableStore::from_environment()",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-self-test-uses-event-router",
            "run_text_shortcuts_keystroke_self_test()",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-keystroke-self-test-cli",
            "--keystroke-self-test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-table-watch-self-test-cli",
            "--table-watch-self-test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-content-purpose-self-test-cli",
            "--content-purpose-self-test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-stdio-cli",
            "--stdio",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-stdio-self-test-cli",
            "--stdio-self-test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-uses-delete-surrounding-text",
            "IbusOperation::DeleteSurroundingText",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-uses-commit-text",
            "IbusOperation::CommitText",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-metadata-only-candidate",
            "IbusRuntimeDecision::show_candidate(trigger, replacement)",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-protocol-candidate-metadata",
            "pub candidate: Option<RuntimeProtocolCandidate>",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-bubble-ready-honesty",
            "rendered_bubble_ready_claim: false",
        ),
        contains_check(
            root.join("os/goblins-os-textshortcuts/goblins-textshortcuts-ibus"),
            "textshortcuts-adapter-selftest-candidate-metadata",
            "\"rendered_bubble_ready_claim\": False",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-dismiss-candidate",
            "EngineAction::DismissCandidate =>",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-runtime-dismiss-handles-escape",
            "IbusRuntimeDecision::handled(vec![IbusOperation::HidePreeditText])",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/lib.rs"),
            "textshortcuts-engine-refuses-password-fields",
            "ContentPurpose::Password",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-self-test-cli",
            "--self-test",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-component-check-cli",
            "--component-check",
        ),
        contains_check(
            root.join("crates/goblins-os-textshortcuts-engine/src/main.rs"),
            "textshortcuts-engine-ibus-runtime-pending",
            "live expansion remains CI/qemu-pending",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-text-shortcuts-route",
            "/v1/text-shortcuts",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-posts-text-shortcuts-route",
            "/v1/text-shortcuts",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-text-shortcuts-engine-honesty",
            "replacement engine isn't running",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-text-shortcuts-editor-helper",
            "text_shortcuts_with_entry",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-text-shortcuts-autocorrect-status",
            "struct TextShortcutsAutocorrectStatus",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-text-shortcuts-autocorrect-row",
            "\"Autocorrect\"",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-text-shortcuts-autocorrect-helper",
            "text_shortcuts_autocorrect_state",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-mode-arg",
            "--text-shortcuts-proof",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-file-env",
            "GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-prewrites-static-proof-file",
            "prewrite_static_text_shortcuts_proof",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-normal-field",
            "TextShortcutsProofMode::Normal",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-password-purpose",
            "gtk::InputPurpose::Password",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-passthrough-field",
            "TextShortcutsProofMode::Passthrough",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-passthrough-placeholder",
            "Type hello.",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-dismiss-field",
            "TextShortcutsProofMode::Dismiss",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-dismiss-placeholder",
            "press Escape",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-candidate-mode",
            "TextShortcutsProofMode::Candidate",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-candidate-render-mode",
            "TextShortcutsProofMode::CandidateRender",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-candidate-render-cli",
            "candidate-render",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-live-runtime-render-mode",
            "TextShortcutsProofMode::LiveRuntimeRender",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-live-runtime-render-cli",
            "live-runtime-render",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-live-ledger-env",
            "GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-entry-refocus-loop",
            "saturating_add(1)",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-live-native-renderer",
            "renderer=native-ibus-lookup-table",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-live-native-publication",
            "candidate_popup_published={popup}",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-proof-live-no-synthetic-overlay",
            "synthetic_overlay=false",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-candidate-bubble-style",
            ".gos-text-shortcuts-candidate",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-candidate-render-proof-screenshot",
            "31-text-shortcuts-candidate-bubble-render.png",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-candidate-render-proof-surface",
            "rendered_candidate_surface=true",
        ),
        contains_check(
            root.join("crates/goblins-os-shell/src/main.rs"),
            "shell-textshortcuts-candidate-honest-render-claim",
            "rendered_bubble_ready_claim=false",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "render-textshortcuts-candidate-light",
            "120-text-shortcuts-candidate.png",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "render-textshortcuts-candidate-dark",
            "121-text-shortcuts-candidate-dark.png",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-voice-control-route",
            "/v1/voice/control/resolve",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-voice-control-dispatch-route",
            "/v1/voice/control",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice_control.rs"),
            "core-voice-control-dispatches-through-safe-settings",
            "dispatch_voice_safe_setting_change",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/voice_control.rs"),
            "core-voice-control-no-match-dictation-fallback",
            "fall_through_to_dictation: true",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "ai-registry-has-voice-control-action",
            "id: \"voice-control\"",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "ai-registry-has-voice-entrypoint",
            "AiEntrypoint::Voice",
        ),
        contains_check(
            root.join(
                "crates/goblins-os-session-tools/src/bin/goblins-os-voice-control.rs",
            ),
            "voice-control-helper-calls-core-route",
            "/v1/voice/control",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-installs-voice-control-helper-as-exact-capability",
            "voice-control:goblins-os-voice-control",
        ),
        absent_check(
            root.join("os/bootc/Containerfile"),
            "bootc-does-not-copy-old-voice-control-script",
            "COPY --chmod=0755 os/voice/goblins-os-voice-control",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-voice-control-source-gated-copy",
            "Voice Control is source-gated",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-visual-lookup-route",
            "/v1/ai/visual-lookup",
        ),
        contains_check(
            root.join("crates/goblins-os-ai/src/lib.rs"),
            "ai-registry-has-visual-lookup-action",
            "id: \"identify-in-image\"",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-builds-visual-lookup-native-feature",
            "goblins-os-visual-lookup/native-desktop",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.VisualLookup.desktop"),
            "visual-lookup-desktop-entry-name",
            "Name=Visual Look Up",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.VisualLookup.desktop"),
            "visual-lookup-desktop-launches-helper",
            "Exec=/usr/libexec/goblins-os/goblins-os-visual-lookup",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.VisualLookup.desktop"),
            "visual-lookup-desktop-visible-menu-entry",
            "NoDisplay=false",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.VisualLookup.desktop"),
            "visual-lookup-desktop-startup-wm-class",
            "StartupWMClass=org.goblins.OS.VisualLookup",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-validates-visual-lookup-desktop",
            "desktop-file-validate /usr/share/applications/org.goblins.OS.VisualLookup.desktop",
        ),
        contains_check(
            root.join("crates/goblins-os-visual-lookup/src/main.rs"),
            "visual-lookup-helper-checks-status-before-capture",
            "vision_status(&config.core)",
        ),
        contains_check(
            root.join("crates/goblins-os-visual-lookup/src/main.rs"),
            "visual-lookup-helper-uses-interactive-portal",
            "portal_screenshot(true)",
        ),
        contains_check(
            root.join("crates/goblins-os-visual-lookup/src/main.rs"),
            "visual-lookup-helper-portal-interactive-true",
            ".interactive(interactive)",
        ),
        contains_check(
            root.join("crates/goblins-os-visual-lookup/src/main.rs"),
            "visual-lookup-helper-posts-to-core-route",
            "/v1/ai/visual-lookup",
        ),
        contains_check(
            root.join("crates/goblins-os-visual-lookup/src/main.rs"),
            "visual-lookup-helper-deletes-private-capture",
            "fs::remove_file(&path)",
        ),
        contains_check(
            root.join("crates/goblins-os-visual-lookup/src/main.rs"),
            "visual-lookup-helper-private-file-mode",
            "Permissions::from_mode(0o600)",
        ),
        contains_check(
            root.join("crates/goblins-os-visual-lookup/src/main.rs"),
            "visual-lookup-helper-private-dir-mode",
            "Permissions::from_mode(0o700)",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-vision-status",
            "/v1/vision/status",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-models-vision-gpt-oss-text-only-gate",
            "GPT-OSS is text-only",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-switch-control-route",
            "/v1/accessibility/switch-control/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-switch-control-preference-route",
            "/v1/accessibility/switch-control/preference",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/switch_control.rs"),
            "switch-control-write-engine-honesty",
            "scanner engine must be active before highlighting",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/switch_control.rs"),
            "switch-control-write-range-normalization",
            "clamp_interval(value)",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-switch-control-status",
            "/v1/accessibility/switch-control/status",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-posts-switch-control-preference",
            "/v1/accessibility/switch-control/preference",
        ),
        contains_check(
            root.join("os/glib-schemas/org.goblins.os.a11y.switch-control.gschema.xml"),
            "switch-control-gschema-present",
            "org.goblins.os.a11y.switch-control",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
            "switch-control-shell-state-machine",
            "_advanceScan()",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
            "switch-control-shell-overlay",
            "goblins-switch-highlight",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "switch-control-desktop-render-proof-hook",
            "57-switch-control-point-$suffix.png",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
            "switch-control-shell-honest-fallback",
            "Selection is paused on this screen.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-today-route",
            "/v1/today/status",
        ),
        contains_check(
            root.join("os/glib-schemas/org.goblins.os.today.gschema.xml"),
            "today-gschema-present",
            "org.goblins.os.today",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/20-goblins-os-today"),
            "today-dconf-seeds-default-widget-layout",
            "enabled-widgets=['date', 'world-clock', 'weather', 'calendar', 'brief']",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-builds-today-native-feature",
            "goblins-os-today/native-desktop",
        ),
        contains_check(
            root.join("crates/goblins-os-today/src/main.rs"),
            "today-panel-fetches-core-status",
            "/v1/today/status",
        ),
        contains_check(
            root.join("crates/goblins-os-today/src/main.rs"),
            "today-panel-documents-gnome-layer-shell-gate",
            "GNOME Wayland is unsupported",
        ),
        contains_check(
            root.join("crates/goblins-os-today/src/main.rs"),
            "today-panel-weather-honest-empty-state",
            "Weather stays empty until location services and a weather source are available.",
        ),
        contains_check(
            root.join("crates/goblins-os-today/src/main.rs"),
            "today-panel-calendar-honest-empty-state",
            "Connect a calendar account before Today can show upcoming events.",
        ),
        contains_check(
            root.join("crates/goblins-os-today/src/main.rs"),
            "today-panel-brief-honest-empty-state",
            "Add a local model before Today can create an on-device daily brief.",
        ),
        contains_check(
            root.join("os/applications/org.goblins.OS.Today.desktop"),
            "today-desktop-launches-owned-binary",
            "Exec=/usr/libexec/goblins-os/goblins-os-today",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "today-render-screens-uses-installed-binary",
            "capture goblins-os-today \"Today\"",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "today-render-screens-light-hook",
            "122-today.png",
        ),
        contains_check(
            root.join("os/bootc/render-screens.sh"),
            "today-render-screens-dark-hook",
            "123-today-dark.png",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-sound-recognition-route",
            "/v1/sound-recognition/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-sound-recognition-preference-route",
            "/v1/sound-recognition/preference",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-sound-recognition-sound-toggle-route",
            "/v1/sound-recognition/sound-toggle",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-reliability-honesty",
            "Do not rely on it in emergencies or high-risk situations.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-model-gate",
            "No recognition model in",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-write-unknown-category-reject",
            "Unknown sound category",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-write-not-fake-listening",
            "it listens only when the local classifier model, listener, microphone capture path, and selected sounds are ready",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-sound-recognition-status",
            "/v1/sound-recognition/status",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-posts-sound-recognition-preference",
            "/v1/sound-recognition/preference",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-posts-sound-recognition-sound-toggle",
            "/v1/sound-recognition/sound-toggle",
        ),
        contains_check(
            root.join("os/glib-schemas/org.goblins.SoundRecognition.gschema.xml"),
            "sound-recognition-gschema-present",
            "org.goblins.SoundRecognition",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/40-sound-recognition"),
            "sound-recognition-defaults-all-off",
            "enabled=false",
        ),
        file_check(root, "os/sound-recognition/goblins-os-sound-listener"),
        file_check(
            root,
            "os/systemd-user/org.goblins.OS.SoundRecognition.service",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "sound-recognition-listener-copied-to-libexec",
            "COPY --chmod=0755 os/sound-recognition/goblins-os-sound-listener /usr/libexec/goblins-os/goblins-os-sound-listener",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "sound-recognition-listener-image-self-test",
            "goblins-os-sound-listener --self-test",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "sound-recognition-listener-image-capability-stays-not-ready",
            "goblins-os-sound-listener --capability-check | grep -q '\"ready\": false'",
        ),
        contains_check(
            root.join("os/sound-recognition/goblins-os-sound-listener"),
            "sound-recognition-listener-no-mic-fake-success",
            "No microphone audio is captured by this listener yet.",
        ),
        contains_check(
            root.join("os/sound-recognition/goblins-os-sound-listener"),
            "sound-recognition-listener-runtime-claim-false",
            "\"runtime_ready_claim\": False",
        ),
        contains_check(
            root.join("os/systemd-user/org.goblins.OS.SoundRecognition.service"),
            "sound-recognition-user-service-exec",
            "ExecStart=/usr/libexec/goblins-os/goblins-os-sound-listener",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-uses-listener-capability-check",
            "--capability-check",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-uses-listener-runtime-capability",
            "listener_runtime_capabilities",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-keeps-listener-not-ready-state",
            "Sound Recognition listener is installed but not ready.",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-capture-from-listener",
            "capture_runtime_ready",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-rejects-implicit-runtime-ready",
            "payload.runtime_ready_claim.unwrap_or(false)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-decision-window",
            "evaluate_sound_recognition_window",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-thresholds-detection",
            "sound_decision_threshold",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-builds-notification-payload",
            "sound_recognition_notification_payload",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-builds-notification-delivery-plan",
            "sound_recognition_notification_delivery_plan",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-notification-app-id",
            "org.goblins.OS.SoundRecognition",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/sound_recognition.rs"),
            "sound-recognition-core-notification-delivery-not-claimed",
            "delivery_ready_claim: false",
        ),
        contains_check(
            root.join("os/sound-recognition/goblins-os-sound-listener"),
            "sound-recognition-listener-decision-contract-ready",
            "\"decision_contract_ready\": True",
        ),
        contains_check(
            root.join("os/sound-recognition/goblins-os-sound-listener"),
            "sound-recognition-listener-notification-contract-ready",
            "\"notification_delivery_contract_ready\": True",
        ),
        contains_check(
            root.join("os/sound-recognition/goblins-os-sound-listener"),
            "sound-recognition-listener-capture-runtime-false",
            "\"capture_runtime_ready\": False",
        ),
        contains_check(
            root.join("os/sound-recognition/goblins-os-sound-listener"),
            "sound-recognition-listener-capture-detail",
            "Capture driver presence is reported, but live microphone capture",
        ),
        contains_check(
            root.join("os/sound-recognition/goblins-os-sound-listener"),
            "sound-recognition-listener-decision-self-test",
            "--decision-self-test",
        ),
        contains_check(
            root.join("os/sound-recognition/goblins-os-sound-listener"),
            "sound-recognition-listener-notification-self-test",
            "--notification-self-test",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "sound-recognition-listener-image-decision-self-test",
            "goblins-os-sound-listener --decision-self-test",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "sound-recognition-listener-image-notification-self-test",
            "goblins-os-sound-listener --notification-self-test",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "sound-recognition-listener-image-capture-runtime-stays-false",
            "goblins-os-sound-listener --capability-check | grep -q '\"capture_runtime_ready\": false'",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "sound-recognition-listener-image-notification-contract-ready",
            "goblins-os-sound-listener --capability-check | grep -q '\"notification_delivery_contract_ready\": true'",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-live-captions-route",
            "/v1/live-captions/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-captions-status-alias",
            "/v1/captions/status",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/main.rs"),
            "core-exposes-captions-stream-route",
            "/v1/captions/stream",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-stream-is-sse",
            "text/event-stream",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-model-gate",
            "Add a speech model to turn on Live Captions",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-whisper-argv-builder",
            "whisper_caption_args",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-pipewire-monitor-parser",
            "pipewire_monitor_targets_from_dump",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-capture-argv-builder",
            "caption_capture_args",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-capture-plan-false-runtime-claim",
            "runtime_ready_claim: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-capture-plan-false-capture-claim",
            "capture_runtime_ready: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-capture-plan-false-transcription-claim",
            "transcription_ready_claim: false",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/live_captions.rs"),
            "live-captions-capture-plan-no-live-copy",
            "no live monitor target, capture stream, or transcription loop is claimed yet",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "live-captions-image-asserts-pw-record",
            "command -v pw-record",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "live-captions-image-asserts-pw-dump",
            "command -v pw-dump",
        ),
        contains_check(
            root.join("os/glib-schemas/org.goblins.shell.extensions.captions.gschema.xml"),
            "live-captions-gschema-present",
            "org.goblins.shell.extensions.captions",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/30-captions"),
            "live-captions-defaults-off",
            "enabled=false",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/metadata.json"),
            "live-captions-extension-metadata",
            "goblins-captions@goblins.os",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-honest-waiting-copy",
            "waiting for the local caption stream",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-adds-chrome",
            "Main.layoutManager.addChrome",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-quick-settings-import",
            "resource:///org/gnome/shell/ui/quickSettings.js",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-system-indicator",
            "QuickSettings.SystemIndicator",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-quick-toggle",
            "QuickSettings.QuickToggle",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-adds-external-indicator",
            "addExternalIndicator",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-toggle-binds-enabled-schema",
            "this._settings.bind('enabled', this, 'checked'",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-waiting-render-proof-hook",
            "showWaitingRenderProof",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-disabled-actor-invariant",
            "renderProofInactive",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-waiting-actor-invariant",
            "renderProofWaiting",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-waiting-proof-requires-mapped-actor",
            "this._overlay.is_mapped()",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-disabled-clears-stale-text",
            "this._label.set_text('');",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-disabled-rejects-caption-surface",
            "!this._settings?.get_boolean('enabled')",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-clears-floating-dock",
            "const BOTTOM_DOCK_CLEARANCE = 120;",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-render-proof-false-capture-claim",
            "captureRuntimeReadyClaim: false",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js"),
            "live-captions-extension-render-proof-false-transcription-claim",
            "transcriptionReadyClaim: false",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "live-captions-desktop-render-proof-hook",
            "58-live-captions-waiting-$suffix.png",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "live-captions-desktop-disabled-baseline",
            "assert_live_captions_inactive",
        ),
        contains_check(
            root.join("os/bootc/render-desktop.sh"),
            "live-captions-desktop-waiting-mapped-proof",
            "live-captions-waiting-mapped",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-captions@goblins.os/stylesheet.css"),
            "live-captions-extension-inter",
            "font-family: \"Inter\"",
        ),
        contains_check(
            root.join("os/gnome-shell-modes/goblins-os.json"),
            "live-captions-extension-enabled-in-mode",
            "goblins-captions@goblins.os",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-fetches-live-captions-status",
            "/v1/live-captions/status",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-live-captions-row",
            "append_live_captions_settings",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-live-captions-quick-settings-boundary",
            "Toggle lives in Quick Settings",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-live-captions-local-copy",
            "Captioning stays local.",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-privacy-app-permissions",
            "App permissions",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-privacy-app-permission-revoke-control",
            "app_permission_revoke_row",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "keychain-seahorse-package",
            "seahorse",
        ),
        contains_check(
            root.join("os/glib-schemas/org.goblins.os.focus.gschema.xml"),
            "focus-gschema-present",
            "org.goblins.os.focus",
        ),
        contains_check(
            root.join("os/glib-schemas/org.goblins.os.focus.gschema.xml"),
            "focus-gschema-restore-banners-key",
            "restore-banners",
        ),
        contains_check(
            root.join("os/glib-schemas/org.goblins.os.focus.gschema.xml"),
            "focus-gschema-armed-by-schedule-key",
            "armed-by-schedule",
        ),
        contains_check(
            root.join("os/glib-schemas/org.goblins.os.focus.gschema.xml"),
            "focus-gschema-restore-apps-key",
            "restore-apps",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "focus-system-schema-compiled",
            "glib-compile-schemas /usr/share/glib-2.0/schemas",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-shortcuts-list",
            "append_keyboard_shortcuts",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-shortcuts-source-gated-copy",
            "Protected shortcut writes are source-gated",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/schemas/org.goblins.shell.extensions.wm.gschema.xml"),
            "goblins-wm-app-expose-keybinding",
            "app-expose",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-focused-app-windows-handler",
            "_showFocusedAppWindows",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-focused-app-windows-render-hook",
            "showFocusedAppWindowsDemo",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-hot-corner-render-hook",
            "showHotCornerDemo",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/schemas/org.goblins.shell.extensions.wm.gschema.xml"),
            "goblins-wm-hot-corner-keys",
            "hot-corner-top-left",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-hot-corner-handler",
            "_setupHotCorners",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/schemas/org.goblins.shell.extensions.wm.gschema.xml"),
            "goblins-wm-snap-assist-key",
            "snap-assist",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-snap-assist-handler",
            "_showSnapAssist",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-snap-assist-render-hook",
            "showSnapAssistDemo",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-snap-assist-render-rejects-one-window-fallback",
            "return false;",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-snap-assist-disambiguates-duplicate-windows",
            "`${base} — Window ${index}`",
        ),
        // Color picker — portal eyedropper helper, packaged + keybound, copying via
        // wl-clipboard with an honest no-clipboard fallback.
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-color-picker-helper",
            "goblins-os-color-picker \\",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-packages-wl-clipboard",
            "wl-clipboard",
        ),
        contains_check(
            root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
            "desktop-color-picker-keybinding",
            "goblins-os-color-picker",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-ai-state-directory",
            "/var/lib/goblins-os/ai",
        ),
        contains_check(
            root.join("os/systemd/goblins-os-core.service"),
            "core-service-ai-write-boundary",
            "/var/lib/goblins-os/ai",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-action-history-endpoint",
            "/v1/ai/action-history",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-runtime-status-endpoint",
            "/v1/ai/runtime/status",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-runtime-compatibility-endpoint",
            "/v1/codex/resident/status",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-runtime-ipc-copy",
            "Goblins AI runtime IPC socket live",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-settings-context-endpoint",
            "/v1/ai/settings-context",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-open-settings-panel-endpoint",
            "/v1/ai/open-settings-panel",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-system-status-endpoint",
            "/v1/ai/system-status",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-selected-text-context-endpoint",
            "/v1/ai/selected-text-context",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-writing-tools-endpoint",
            "/v1/ai/write-selected-text",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-ai-screen-context-endpoint",
            "/v1/ai/screen-context",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-firewall-status-endpoint",
            "/v1/firewall/status",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-firewall-enabled-endpoint",
            "/v1/firewall/enabled",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-firewall-honest-toggle-outcome",
            r#"502|503) [ "$firewall_toggle_ok" != "true" ]"#,
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-firewall-honest-failure-body",
            "firewall_toggle_body",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-app-builder-catalog-endpoint",
            "/v1/apps/build-catalog",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-app-builder-list-endpoint",
            "/v1/apps",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-app-builder-build-endpoint",
            "/v1/apps/builds",
        ),
        contains_check(
            root.join("os/bootc/run-selftest.sh"),
            "selftest-app-builder-temp-store",
            "GOBLINS_OS_APPS_DIR=/tmp/goblins-os-selftest-apps",
        ),
    ]
}

fn source_manifest_top_level_coverage_check(root: &Path) -> Check {
    let manifest = read_to_string(root.join("os/release/source-tree-manifest.toml"));
    let Ok(entries) = fs::read_dir(root) else {
        return blocked(
            "source-manifest-covers-top-level-tree",
            &format!(
                "could not read top-level source directory {}",
                root.display()
            ),
        );
    };

    let mut unclassified = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "." || name == ".." {
            continue;
        }

        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if source_manifest_classifies_top_level(&manifest, &name, is_dir) {
            continue;
        }
        unclassified.push(name);
    }
    unclassified.sort();

    if unclassified.is_empty() {
        ready(
            "source-manifest-covers-top-level-tree",
            "every top-level source-package entry is classified in source-tree-manifest.toml",
        )
    } else {
        blocked(
            "source-manifest-covers-top-level-tree",
            &format!(
                "unclassified top-level source-package entries: {}",
                unclassified.join(", ")
            ),
        )
    }
}

fn source_manifest_classifies_top_level(manifest: &str, name: &str, is_dir: bool) -> bool {
    let exact_file = format!("\"{name}\"");
    if manifest.contains(&exact_file) {
        return true;
    }

    if is_dir {
        let exact_dir = format!("\"{name}/\"");
        let nested_dir = format!("\"{name}/");
        return manifest.contains(&exact_dir) || manifest.contains(&nested_dir);
    }

    false
}

fn forbidden_source_drift_check(root: &Path) -> Check {
    let forbidden = [
        "webview",
        "WebView",
        "kiosk",
        "Kiosk",
        "vite",
        "package.json",
        "next.config",
        ".tsx",
        ".jsx",
    ];
    let mut hits = Vec::new();
    for relative in [
        "Cargo.toml",
        "os/bootc/Containerfile",
        "os/session/goblins-os-session",
        "os/gnome-session/goblins-os.session",
        "os/etc/goblins-os/environment",
    ] {
        let text = read_to_string(root.join(relative));
        for needle in forbidden {
            if text.contains(needle) {
                hits.push(format!("{relative}:{needle}"));
            }
        }
    }

    if hits.is_empty() {
        ready(
            "no-web-kiosk-packaging-drift",
            "source packaging contract has no web or kiosk drift",
        )
    } else {
        blocked(
            "no-web-kiosk-packaging-drift",
            &format!("forbidden source packaging terms: {}", hits.join(", ")),
        )
    }
}

fn installed_session_check(root: &Path) -> Check {
    contains_check(
        root.join("usr/libexec/goblins-os/goblins-os-session"),
        "installed-gnome-session",
        "gnome-session --session=goblins-os",
    )
}

fn installed_secret_dir_check(root: &Path) -> Check {
    path_mode_check(
        root,
        "var/lib/goblins-os/secrets/openai",
        0o700,
        "installed-secret-storage",
    )
}

fn installed_state_dir_check(root: &Path, relative: &str) -> Check {
    let path = root.join(relative);
    if path.is_dir() {
        ready(relative, &format!("found {}", path.display()))
    } else {
        blocked(relative, &format!("missing {}", path.display()))
    }
}

/// Resolve a username's uid/gid by parsing `<root>/etc/passwd`. Returns `None`
/// when the rootfs has no passwd database (e.g. a staged DESTDIR) or the user is
/// not present, so callers can `ready`/skip instead of failing closed.
#[cfg(unix)]
fn resolve_passwd_id(root: &Path, username: &str) -> Option<(u32, u32)> {
    let passwd = fs::read_to_string(root.join("etc/passwd")).ok()?;
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        if fields.next()? != username {
            return None;
        }
        let _password = fields.next()?;
        let uid = fields.next()?.parse().ok()?;
        let gid = fields.next()?.parse().ok()?;
        Some((uid, gid))
    })
}

#[cfg(unix)]
fn resolve_group_id(root: &Path, group_name: &str) -> Option<u32> {
    let groups = fs::read_to_string(root.join("etc/group")).ok()?;
    groups.lines().find_map(|line| {
        let mut fields = line.split(':');
        if fields.next()? != group_name {
            return None;
        }
        let _password = fields.next()?;
        fields.next()?.parse().ok()
    })
}

#[cfg(unix)]
fn installed_named_owner_check(
    root: &Path,
    relative: &str,
    expected_user: &str,
    expected_group: &str,
    id: &str,
) -> Check {
    use std::os::unix::fs::MetadataExt;

    let Some((expected_uid, _)) = resolve_passwd_id(root, expected_user) else {
        return ready(
            id,
            &format!(
                "ownership check skipped for {relative}: {expected_user} not resolvable from rootfs passwd"
            ),
        );
    };
    let Some(expected_gid) = resolve_group_id(root, expected_group) else {
        return ready(
            id,
            &format!(
                "ownership check skipped for {relative}: {expected_group} not resolvable from rootfs group"
            ),
        );
    };

    let path = root.join(relative);
    match fs::metadata(&path) {
        Ok(metadata) if metadata.uid() == expected_uid && metadata.gid() == expected_gid => ready(
            id,
            &format!(
                "{} owned by {expected_user}:{expected_group} ({expected_uid}:{expected_gid})",
                path.display()
            ),
        ),
        Ok(metadata) => blocked(
            id,
            &format!(
                "{} owned by uid {}:{}; expected {expected_user}:{expected_group} {expected_uid}:{expected_gid}",
                path.display(),
                metadata.uid(),
                metadata.gid()
            ),
        ),
        Err(_) => blocked(id, &format!("missing {}", path.display())),
    }
}

#[cfg(not(unix))]
fn installed_named_owner_check(
    _root: &Path,
    relative: &str,
    _expected_user: &str,
    _expected_group: &str,
    id: &str,
) -> Check {
    ready(
        id,
        &format!("ownership check skipped for {relative} on non-Unix host"),
    )
}

/// Assert an installed state directory is owned by the expected service account.
/// This only has teeth against a real installed `/` (where `etc/passwd` exists);
/// against a staged DESTDIR the passwd database is absent, so the check resolves
/// to `ready`/skip and never fails the stage gate. We do not assert a uniform
/// mode — those diverge per directory in the Containerfile.
#[cfg(unix)]
fn installed_state_dir_owner_check(
    root: &Path,
    relative: &str,
    expected_user: &str,
    id: &str,
) -> Check {
    use std::os::unix::fs::MetadataExt;

    let Some((expected_uid, expected_gid)) = resolve_passwd_id(root, expected_user) else {
        return ready(
            id,
            &format!(
                "ownership check skipped for {relative}: {expected_user} not resolvable from rootfs passwd"
            ),
        );
    };

    let path = root.join(relative);
    match fs::metadata(&path) {
        Ok(metadata) => {
            let uid = metadata.uid();
            let gid = metadata.gid();
            if uid == expected_uid && gid == expected_gid {
                ready(
                    id,
                    &format!("{} owned by {expected_user} ({uid}:{gid})", path.display()),
                )
            } else {
                blocked(
                    id,
                    &format!(
                        "{} owned by uid {uid}:{gid}; expected {expected_user} {expected_uid}:{expected_gid}",
                        path.display()
                    ),
                )
            }
        }
        Err(_) => blocked(id, &format!("missing {}", path.display())),
    }
}

#[cfg(not(unix))]
fn installed_state_dir_owner_check(
    _root: &Path,
    relative: &str,
    _expected_user: &str,
    id: &str,
) -> Check {
    ready(
        id,
        &format!("ownership check skipped for {relative} on non-Unix host"),
    )
}

fn desktop_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .map(str::trim)
}

static READ_CACHE: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();

fn read_to_string(path: impl AsRef<Path>) -> String {
    let path = path.as_ref().to_path_buf();
    let cache = READ_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return read_bounded_text_file_from_metadata(&path).unwrap_or_default();
    };
    if let Some(text) = cache.get(&path) {
        return text.clone();
    }
    let text = read_bounded_text_file_from_metadata(&path).unwrap_or_default();
    cache.insert(path, text.clone());
    text
}

fn read_bounded_text_file_from_metadata(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    read_bounded_text_file(path, metadata.len())
}

fn ready(id: &str, detail: &str) -> Check {
    Check {
        id: stable_id(id),
        state: CheckState::Ready,
        detail: detail.to_string(),
    }
}

fn blocked(id: &str, detail: &str) -> Check {
    Check {
        id: stable_id(id),
        state: CheckState::Blocked,
        detail: detail.to_string(),
    }
}

fn stable_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::{
        candidate_commit_is_valid, capability_groupadd_targets, cargo_lock_packages,
        client_path_binding_inventory, contains_realish_openai_key,
        core_service_writable_paths_are_exact, deprecated_github_action_pins_absent_check,
        desktop_field, first_executable_initialization, image_ref_is_digest_pinned,
        image_ref_is_valid, imports_shared_core_initializer, install_files,
        is_allowed_dummy_secret, is_suspicious_secret_line, native_design_system_checks,
        ordered_contains_check, permission_inventory, reviewed_github_action_pins_check,
        rg_secret_scan_hit, sha256_path, should_skip_secret_scan_path,
        source_manifest_classifies_top_level, stable_id, tmpfiles_capability_entries,
        verify_installer_branding_tool_provenance, write_release_evidence, CheckState,
        ForbiddenClientTokenVisitor, APPLICATIONS, AUTOSTART, BINARIES,
        CORE_SERVICE_READ_WRITE_PATHS, DCONF_FILES, DEPRECATED_GITHUB_ACTION_PINS,
        GLIB_SCHEMA_FILES, GNOME_SHELL_EXTENSION_FILES, ICON_THEME_FILES, NATIVE_DESIGN_APPS,
        NAUTILUS_SCRIPTS, POLISH_INTERACTION_PROOF, POLISH_INTERACTION_SCREENSHOTS,
        REVIEWED_GITHUB_ACTION_PINS, SETTINGS_INTERACTION_SCREENSHOTS, SETTINGS_RENDER_SCREENSHOTS,
        SYSTEMD_SYSTEM_DROPINS, SYSTEMD_UNITS, SYSTEMD_USER_UNITS,
    };
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;
    use syn::visit::Visit;

    #[test]
    fn parses_desktop_exec_field() {
        assert_eq!(
            desktop_field("[Desktop Entry]\nName=ChatGPT\nExec=/usr/libexec/goblins-os/goblins-os-open chatgpt\n", "Exec"),
            Some("/usr/libexec/goblins-os/goblins-os-open chatgpt")
        );
    }

    #[test]
    fn creates_stable_check_ids() {
        assert_eq!(
            stable_id("systemd-goblins-os-core.service-NoNewPrivileges=yes"),
            "systemd-goblins-os-core-service-nonewprivileges-yes"
        );
    }

    #[test]
    fn github_action_allowlist_rejects_retired_node20_pins() {
        let root = std::env::temp_dir().join(format!(
            "goblins-os-verify-github-actions-{}",
            std::process::id()
        ));
        let workflows = root.join(".github/workflows");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&workflows).expect("create workflow fixture directory");
        let approved_steps = REVIEWED_GITHUB_ACTION_PINS
            .iter()
            .map(|(action, pin)| format!("  - uses: {action}@{pin}\n"))
            .collect::<String>();
        let approved = format!("steps:\n{approved_steps}");
        let workflow = workflows.join("release.yml");
        fs::write(&workflow, &approved).expect("write approved workflow fixture");

        assert_eq!(
            reviewed_github_action_pins_check(&root).state,
            CheckState::Ready
        );
        assert_eq!(
            deprecated_github_action_pins_absent_check(&root).state,
            CheckState::Ready
        );

        let retired = approved.replace(
            REVIEWED_GITHUB_ACTION_PINS[0].1,
            DEPRECATED_GITHUB_ACTION_PINS[0],
        );
        fs::write(&workflow, retired).expect("write retired workflow fixture");
        assert_eq!(
            reviewed_github_action_pins_check(&root).state,
            CheckState::Blocked
        );
        assert_eq!(
            deprecated_github_action_pins_absent_check(&root).state,
            CheckState::Blocked
        );

        let bypasses = [
            format!(
                "steps:\n{approved_steps}  - \"uses\": evil/action@{}\n",
                "0".repeat(40)
            ),
            format!(
                "steps:\n{approved_steps}  - uses : evil/action@{}\n",
                "0".repeat(40)
            ),
            format!(
                "pin: &pin evil/action@{}\nsteps:\n{approved_steps}  - uses: *pin\n",
                "0".repeat(40)
            ),
            format!(
                "key: &key uses\nsteps:\n{approved_steps}  - *key: evil/action@{}\n",
                "0".repeat(40)
            ),
            format!(
                "steps:\n{approved_steps}  - {{ \"uses\": evil/action@{} }}\n",
                "0".repeat(40)
            ),
            format!("steps:\n{approved_steps}  - uses: actions/checkout@v7\n"),
            format!(
                "steps:\n{approved_steps}  - uses: actions/checkout@{}\n",
                "0".repeat(40)
            ),
        ];
        for bypass in bypasses {
            fs::write(&workflow, &bypass).expect("write bypass workflow fixture");
            assert_eq!(
                reviewed_github_action_pins_check(&root).state,
                CheckState::Blocked,
                "action allowlist accepted bypass fixture: {bypass}"
            );
        }

        fs::remove_dir_all(root).expect("remove workflow fixture directory");
    }

    #[test]
    fn core_service_writable_paths_reject_omissions_and_broad_allowances() {
        let canonical = format!(
            "[Unit]\nDescription=fixture\n\n[Service]\nProtectSystem=strict\nUMask=0077\nReadWritePaths={CORE_SERVICE_READ_WRITE_PATHS}\n"
        );
        assert!(core_service_writable_paths_are_exact(&canonical));

        let mutations = [
            canonical.replace("UMask=0077\n", ""),
            canonical.replace("UMask=0077", "UMask=0027"),
            canonical.replace("/run/goblins-os-core ", ""),
            canonical.replace(" /var/lib/goblins-os/voice/work", ""),
            canonical.replace("/run/goblins-os-core", "/run"),
            canonical.replace("/var/lib/goblins-os/voice/work", "/var/lib/goblins-os/voice"),
            canonical.replace("/var/lib/goblins-os/voice/work", "/var/lib/goblins-os"),
            canonical.replace("/var/lib/goblins-os/voice/work", "/var/lib"),
            canonical.replace(CORE_SERVICE_READ_WRITE_PATHS, "/"),
            canonical.replace(
                &format!("ReadWritePaths={CORE_SERVICE_READ_WRITE_PATHS}"),
                "ReadWritePaths=/run/goblins-os-core\nReadWritePaths=/var/lib/goblins-os/voice/work",
            ),
            format!("{canonical}ReadWritePaths={CORE_SERVICE_READ_WRITE_PATHS}\n"),
            canonical.replace("ProtectSystem=strict", "ProtectSystem=full"),
        ];
        for mutation in mutations {
            assert!(
                !core_service_writable_paths_are_exact(&mutation),
                "accepted unsafe core service fixture:\n{mutation}"
            );
        }
    }

    #[test]
    fn ordered_contains_check_reports_order() {
        let path =
            std::env::temp_dir().join(format!("goblins-os-verify-ordered-{}", std::process::id()));
        fs::write(&path, "COPY first\nRUN second\n").expect("write temp source");

        let ready = ordered_contains_check(path.clone(), "ordered", "COPY first", "RUN second");
        assert_eq!(ready.state, CheckState::Ready);

        let blocked = ordered_contains_check(path.clone(), "ordered", "RUN second", "COPY first");
        assert_eq!(blocked.state, CheckState::Blocked);

        fs::remove_file(path).expect("remove temp source");
    }

    fn parsed_main(source: &str) -> syn::ItemFn {
        syn::parse_file(source)
            .expect("valid Rust fixture")
            .items
            .into_iter()
            .find_map(|item| match item {
                syn::Item::Fn(function) if function.sig.ident == "main" => Some(function),
                _ => None,
            })
            .expect("fixture main")
    }

    #[test]
    fn capability_initialization_parser_accepts_current_leading_forms_only() {
        let shared_import = syn::parse_file(
            "use goblins_os_core_client::{initialize, ClientKind, CoreClient}; fn main() {}",
        )
        .unwrap();
        assert!(imports_shared_core_initializer(&shared_import));
        let local_only = syn::parse_file(
            "fn initialize(_: ClientKind) {} enum ClientKind { Settings } fn main() {}",
        )
        .unwrap();
        assert!(!imports_shared_core_initializer(&local_only));

        let direct = parsed_main(
            "fn main() -> Result<(), Error> { let core = initialize(ClientKind::Settings)?; Ok(()) }",
        );
        assert_eq!(
            first_executable_initialization(&direct).as_deref(),
            Some("Settings")
        );

        let matched = parsed_main(
            "fn main() { fn declaration_is_not_executable() {} let core = match initialize(ClientKind::Open) { Ok(core) => core, Err(_) => return }; }",
        );
        assert_eq!(
            first_executable_initialization(&matched).as_deref(),
            Some("Open")
        );

        let late = parsed_main(
            "fn main() { parse_arguments(); let core = initialize(ClientKind::Shell); }",
        );
        assert_eq!(first_executable_initialization(&late), None);
    }

    #[test]
    fn capability_source_scan_skips_only_code_that_cfg_implies_is_test_only() {
        let test_only = syn::parse_file(
            r#"
                #[cfg(test)]
                fn fixture() { let _ = "GOBLINS_OS_CORE_URL"; }
                #[cfg(all(test, target_os = "linux"))]
                fn linux_fixture() { let _ = std::net::TcpStream::connect("localhost:8787"); }
                fn main() {}
            "#,
        )
        .expect("valid Rust fixture");
        let mut visitor = ForbiddenClientTokenVisitor::default();
        visitor.visit_file(&test_only);
        assert!(visitor.hits.is_empty());

        let not_test_only = syn::parse_file(
            r#"
                #[cfg(any(test, feature = "diagnostics"))]
                fn production_feature() { let _ = "GOBLINS_OS_CORE_URL"; }
            "#,
        )
        .expect("valid Rust fixture");
        let mut visitor = ForbiddenClientTokenVisitor::default();
        visitor.visit_file(&not_test_only);
        assert!(visitor.hits.contains("core environment override"));
    }

    #[test]
    fn capability_manifests_parse_exact_route_and_tmpfiles_inventories() {
        let permissions = r#"
            const RESIDENT_PERMISSIONS: &[Permission] =
                permissions![(POST, "/v1/codex/resident")];
        "#;
        assert_eq!(
            permission_inventory(permissions, "RESIDENT_PERMISSIONS").unwrap(),
            [("POST".to_string(), "/v1/codex/resident".to_string())]
        );
        let binding = r#"
            impl ClientKind {
                fn permissions(self) -> &'static [Permission] {
                    match self { Self::Resident => RESIDENT_PERMISSIONS }
                }
            }
        "#;
        assert_eq!(
            client_path_binding_inventory(binding, "ClientKind", "permissions")
                .unwrap()
                .get("Resident")
                .map(String::as_str),
            Some("RESIDENT_PERMISSIONS")
        );

        let tmpfiles = concat!(
            "d /run/goblins-os-core 0755 root root -\n",
            "d /run/goblins-os-core/settings 2750 goblins-os goblins-core-settings -\n",
            "d /run/goblins-os-core/release-proof 2750 goblins-os goblins-core-release-proof -\n",
        );
        assert_eq!(
            tmpfiles_capability_entries(tmpfiles).unwrap(),
            ["settings".to_string(), "release-proof".to_string()]
        );

        let container = r#"
            RUN for client in settings release-proof; do \
                  groupadd --system "goblins-core-${client}"; \
                done \
                && groupadd --system goblins-session-bridge
        "#;
        assert_eq!(
            capability_groupadd_targets(container),
            ["goblins-core-${client}".to_string()]
        );
    }

    #[test]
    fn source_manifest_top_level_matching_distinguishes_strays() {
        let manifest = r#"
paths = [
  ".dockerignore",
  "Cargo.toml",
  "os/hardware-gate/",
  "target/",
]
"#;

        assert!(source_manifest_classifies_top_level(
            manifest,
            ".dockerignore",
            false
        ));
        assert!(source_manifest_classifies_top_level(
            manifest,
            "Cargo.toml",
            false
        ));
        assert!(source_manifest_classifies_top_level(manifest, "os", true));
        assert!(source_manifest_classifies_top_level(
            manifest, "target", true
        ));
        assert!(!source_manifest_classifies_top_level(
            manifest,
            "scratch.txt",
            false
        ));
        assert!(!source_manifest_classifies_top_level(
            manifest,
            "untracked-dir",
            true
        ));
    }

    #[test]
    fn secret_scan_skips_local_agent_and_build_state() {
        assert!(should_skip_secret_scan_path(Path::new(".claude")));
        assert!(should_skip_secret_scan_path(Path::new(
            ".claude/worktrees/example"
        )));
        assert!(should_skip_secret_scan_path(Path::new(".ci-target")));
        assert!(should_skip_secret_scan_path(Path::new(
            ".ci-target-amd64/debug"
        )));
        assert!(should_skip_secret_scan_path(Path::new("target/debug")));
        assert!(!should_skip_secret_scan_path(Path::new(
            "os/etc/goblins-os/openai-secrets.env"
        )));
    }

    #[test]
    fn rg_secret_scan_parser_keeps_existing_line_rules() {
        assert_eq!(
            rg_secret_scan_hit("src/main.rs:42:OPENAI_API_KEY=not_for_source"),
            Some("src/main.rs:42".to_string())
        );
        assert_eq!(
            rg_secret_scan_hit("README.md:7:OPENAI_API_KEY=<placeholder>"),
            None
        );
    }

    #[test]
    fn installer_branding_provenance_enforces_runtime_contract_and_hash_binding() {
        let root = std::env::temp_dir().join(format!(
            "goblins-os-branding-provenance-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let write_fixture = |relative: &str, content: &str| {
            let destination = root.join(relative);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(destination, content).unwrap();
        };
        let repository = "ghcr.io/joe-simo/goblins-os-installer-branding-tool";
        let image_ref = format!("{repository}@sha256:{}", "1".repeat(64));
        let base_image = format!("docker.io/library/fedora@sha256:{}", "2".repeat(64));
        let containerfile = format!(
            concat!(
                "ARG FEDORA_IMAGE={}\n",
                "FROM ${{FEDORA_IMAGE}}\n",
                "RUN dnf install \\\n",
                "      diffutils \\\n",
                "    && command -v cmp \\\n",
                "    && command -v xorriso\n"
            ),
            base_image
        );
        let containerfile_path = root.join("os/iso/branding-tool.Containerfile");
        write_fixture("os/iso/branding-tool.Containerfile", &containerfile);
        let containerfile_sha = sha256_path(&containerfile_path).unwrap();
        let provenance = format!(
            concat!(
                "schema = 1\n",
                "image_ref = \"{}\"\n",
                "source_commit = \"{}\"\n",
                "workflow_run = \"https://github.com/Joe-Simo/goblins-os/actions/runs/123\"\n",
                "workflow_run_attempt = 1\n",
                "base_image = \"{}\"\n",
                "containerfile_sha256 = \"{}\"\n",
                "public_pull_verified_on = \"2026-07-19\"\n",
                "inventory_path_in_image = \"/usr/share/goblins-os-installer-branding-tool/rpm-packages.tsv\"\n",
                "[architectures.x86_64]\n",
                "native_image_ref = \"{}@sha256:{}\"\n",
                "rpm_inventory_sha256 = \"{}\"\n",
                "rpm_package_count = 246\n",
                "[architectures.aarch64]\n",
                "native_image_ref = \"{}@sha256:{}\"\n",
                "rpm_inventory_sha256 = \"{}\"\n",
                "rpm_package_count = 245\n"
            ),
            image_ref,
            "3".repeat(40),
            base_image,
            containerfile_sha,
            repository,
            "4".repeat(64),
            "5".repeat(64),
            repository,
            "6".repeat(64),
            "7".repeat(64),
        );
        write_fixture("os/release/installer-branding-tool.toml", &provenance);
        write_fixture(
            "os/iso/build-iso.sh",
            &format!(
                "INSTALLER_BRANDING_IMAGE=\"${{GOBLINS_OS_INSTALLER_BRANDING_IMAGE:-{image_ref}}}\"\n"
            ),
        );
        write_fixture(
            ".github/workflows/branding-tool-image.yml",
            concat!(
                "for required_tool in checkisomd5 cmp implantisomd5 magick mksquashfs ",
                "osirrox unsquashfs xorriso; do command -v \"$required_tool\" >/dev/null; done\n"
            ),
        );
        for workflow in [
            ".github/workflows/build.yml",
            ".github/workflows/candidate-artifacts.yml",
            ".github/workflows/hardware-gate-capture.yml",
            ".github/workflows/aarch64-verification-iso.yml",
        ] {
            write_fixture(
                workflow,
                &format!("GOBLINS_OS_INSTALLER_BRANDING_IMAGE: {image_ref}\n"),
            );
        }

        assert!(verify_installer_branding_tool_provenance(&root).is_ok());

        write_fixture(
            ".github/workflows/branding-tool-image.yml",
            "runtime tool verification removed\n",
        );
        assert!(verify_installer_branding_tool_provenance(&root)
            .unwrap_err()
            .contains("must verify the remaster runtime tool contract"));
        write_fixture(
            ".github/workflows/branding-tool-image.yml",
            concat!(
                "for required_tool in checkisomd5 cmp implantisomd5 magick mksquashfs ",
                "osirrox unsquashfs xorriso; do command -v \"$required_tool\" >/dev/null; done\n"
            ),
        );

        let without_provider = containerfile.replace("      diffutils \\\n", "");
        fs::write(&containerfile_path, &without_provider).unwrap();
        let without_provider_sha = sha256_path(&containerfile_path).unwrap();
        fs::write(
            root.join("os/release/installer-branding-tool.toml"),
            provenance.replace(&containerfile_sha, &without_provider_sha),
        )
        .unwrap();
        assert!(verify_installer_branding_tool_provenance(&root)
            .unwrap_err()
            .contains("must install diffutils as the cmp provider"));

        let without_assertion = containerfile.replace("    && command -v cmp \\\n", "");
        fs::write(&containerfile_path, &without_assertion).unwrap();
        let without_assertion_sha = sha256_path(&containerfile_path).unwrap();
        fs::write(
            root.join("os/release/installer-branding-tool.toml"),
            provenance.replace(&containerfile_sha, &without_assertion_sha),
        )
        .unwrap();
        assert!(verify_installer_branding_tool_provenance(&root)
            .unwrap_err()
            .contains("must assert the required cmp executable"));

        fs::write(&containerfile_path, &containerfile).unwrap();
        fs::write(
            root.join("os/release/installer-branding-tool.toml"),
            &provenance,
        )
        .unwrap();
        fs::write(&containerfile_path, format!("{containerfile}# drift\n")).unwrap();
        assert!(verify_installer_branding_tool_provenance(&root)
            .unwrap_err()
            .contains("Containerfile hash drifted"));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parses_cargo_lock_package_fields_for_release_evidence() {
        let packages = cargo_lock_packages(
            r#"
version = 4

[[package]]
name = "alpha"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc123"
dependencies = [
 "beta",
]

[[package]]
name = "workspace-crate"
version = "0.1.0"
"#,
        );

        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "alpha");
        assert_eq!(packages[0].version, "1.0.0");
        assert_eq!(
            packages[0].source,
            "registry+https://github.com/rust-lang/crates.io-index"
        );
        assert_eq!(packages[0].checksum, "abc123");
        assert_eq!(packages[1].name, "workspace-crate");
        assert_eq!(packages[1].checksum, "");
    }

    #[test]
    fn release_evidence_writes_cargo_packages_and_rpm_command() {
        let root = std::env::temp_dir().join(format!(
            "goblins-os-verify-release-evidence-{}",
            std::process::id()
        ));
        let source = root.join("source");
        let output = root.join("sbom/aarch64");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        for generated_name in [
            "release-evidence-manifest.json",
            "cargo-lock-packages.tsv",
            "rpm-packages.command",
            "rpm-packages.tsv",
            "rpm-packages.not-generated.txt",
        ] {
            fs::write(output.join(generated_name), "stale\n").unwrap();
        }
        fs::write(
            source.join("Cargo.lock"),
            r#"
version = 4

[[package]]
name = "alpha"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc123"
"#,
        )
        .unwrap();

        let candidate_commit = "0123456789abcdef0123456789abcdef01234567";
        let image_ref = "ghcr.io/joe-simo/goblins-os@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let manifest =
            write_release_evidence(&source, "aarch64", candidate_commit, image_ref, &output)
                .unwrap();
        let cargo_tsv = fs::read_to_string(output.join("cargo-lock-packages.tsv")).unwrap();
        let rpm_command = fs::read_to_string(output.join("rpm-packages.command")).unwrap();
        let manifest_text = fs::read_to_string(manifest).unwrap();

        assert!(cargo_tsv.contains("name\tversion\tsource\tchecksum"));
        assert!(cargo_tsv.contains("alpha\t1.0.0"));
        assert!(rpm_command.contains("rpm -qa --qf"));
        assert!(rpm_command.contains("name\\tversion_release\\tarch\\tlicense"));
        assert!(manifest_text.contains("\"architecture\": \"aarch64\""));
        assert!(manifest_text.contains(&format!("\"candidate_commit\": \"{candidate_commit}\"")));
        assert!(manifest_text.contains(&format!("\"image_ref\": \"{image_ref}\"")));
        assert!(manifest_text.contains("\"image_digest_pinned\": true"));
        assert!(manifest_text.contains("\"schema\": \"goblins-os-release-evidence-v4\""));
        assert!(manifest_text.contains("\"cargo_package_count\": 1"));
        let cargo_sha = sha256_path(&output.join("cargo-lock-packages.tsv")).unwrap();
        assert!(manifest_text.contains(&format!("\"cargo_packages_sha256\": \"{cargo_sha}\"")));
        let has_rpm = output.join("rpm-packages.tsv").is_file();
        let has_rpm_blocker = output.join("rpm-packages.not-generated.txt").is_file();
        assert_ne!(has_rpm, has_rpm_blocker);
        if has_rpm {
            let rpm_sha = sha256_path(&output.join("rpm-packages.tsv")).unwrap();
            assert!(manifest_text.contains(&format!("\"rpm_packages_sha256\": \"{rpm_sha}\"")));
            assert!(manifest_text.contains("\"rpm_status\": \"generated from rpm database\""));
        } else {
            assert!(manifest_text.contains("\"rpm_packages_sha256\": null"));
            assert!(
                !fs::read_to_string(output.join("rpm-packages.not-generated.txt"))
                    .unwrap()
                    .contains("stale")
            );
        }

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn release_evidence_clears_stale_outputs_before_cargo_error() {
        let root = std::env::temp_dir().join(format!(
            "goblins-os-verify-release-evidence-error-{}",
            std::process::id()
        ));
        let source = root.join("source");
        let output = root.join("sbom/x86_64");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        let generated = [
            "release-evidence-manifest.json",
            "cargo-lock-packages.tsv",
            "rpm-packages.command",
            "rpm-packages.tsv",
            "rpm-packages.not-generated.txt",
        ];
        for generated_name in generated {
            fs::write(output.join(generated_name), "stale\n").unwrap();
        }

        let result = write_release_evidence(
            &source,
            "x86_64",
            "0123456789abcdef0123456789abcdef01234567",
            "ghcr.io/joe-simo/goblins-os@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            &output,
        );
        assert!(result.is_err());
        for generated_name in generated {
            assert!(!output.join(generated_name).exists());
        }

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn release_evidence_candidate_commit_is_full_hex() {
        assert!(candidate_commit_is_valid(
            "0123456789abcdef0123456789ABCDEF01234567"
        ));
        assert!(!candidate_commit_is_valid("0123456789abcdef"));
        assert!(!candidate_commit_is_valid(
            "0123456789abcdef0123456789abcdef0123456g"
        ));
    }

    #[test]
    fn release_evidence_image_ref_validation_distinguishes_digest_pins() {
        let digest_ref = "ghcr.io/joe-simo/goblins-os@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(image_ref_is_valid(digest_ref));
        assert!(image_ref_is_digest_pinned(digest_ref));
        assert!(image_ref_is_valid("localhost/goblins-os:aarch64"));
        assert!(!image_ref_is_digest_pinned("localhost/goblins-os:aarch64"));
        assert!(!image_ref_is_digest_pinned(
            "ghcr.io/joe-simo/goblins-os@sha256:01234567"
        ));
        let uppercase_digest = format!("ghcr.io/joe-simo/goblins-os@sha256:{}", "A".repeat(64));
        assert!(!image_ref_is_digest_pinned(&uppercase_digest));
        assert!(!image_ref_is_digest_pinned(
            "ghcr.io/joe-simo/goblins-os@tag@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!image_ref_is_valid("ghcr.io/joe-simo/goblins os:aarch64"));
    }

    #[test]
    fn settings_render_contract_covers_full_light_and_dark_surface() {
        let unique = SETTINGS_RENDER_SCREENSHOTS
            .iter()
            .copied()
            .collect::<HashSet<_>>();

        assert_eq!(SETTINGS_RENDER_SCREENSHOTS.len(), 76);
        assert_eq!(unique.len(), SETTINGS_RENDER_SCREENSHOTS.len());
        for screenshot in [
            "03-settings.png",
            "11-settings-dark.png",
            "76-settings-applications.png",
            "84-settings-power-battery.png",
            "116-settings-games.png",
            "104-settings-security.png",
            "110-settings-lock-screen.png",
            "112-settings-language-region.png",
            "107-settings-security-dark.png",
            "117-settings-games-dark.png",
            "113-settings-lock-screen-dark.png",
            "115-settings-language-region-dark.png",
        ] {
            assert!(
                unique.contains(screenshot),
                "missing required Settings render screenshot {screenshot}"
            );
        }
    }

    #[test]
    fn settings_interaction_contract_covers_search_and_firewall_paths() {
        let unique = SETTINGS_INTERACTION_SCREENSHOTS
            .iter()
            .copied()
            .collect::<HashSet<_>>();

        assert_eq!(SETTINGS_INTERACTION_SCREENSHOTS.len(), 6);
        assert_eq!(unique.len(), SETTINGS_INTERACTION_SCREENSHOTS.len());
        for screenshot in [
            "100-settings-search-wifi-filter.png",
            "101-settings-search-enter-network.png",
            "102-settings-search-no-results.png",
            "103-settings-search-cleared.png",
            "118-settings-firewall-before.png",
            "119-settings-firewall-toggle-failed.png",
        ] {
            assert!(
                unique.contains(screenshot),
                "missing required Settings interaction screenshot {screenshot}"
            );
        }
    }

    #[test]
    fn polish_interaction_contract_covers_changed_semantic_states() {
        let unique = POLISH_INTERACTION_SCREENSHOTS
            .iter()
            .copied()
            .collect::<HashSet<_>>();

        assert_eq!(POLISH_INTERACTION_SCREENSHOTS.len(), 15);
        assert_eq!(unique.len(), POLISH_INTERACTION_SCREENSHOTS.len());
        for screenshot in [
            "124-settings-models-advanced-collapsed.png",
            "125-settings-models-advanced-expanded.png",
            "126-settings-models-engine-offline-error.png",
            "127-studio-engine-menu.png",
            "128-studio-engine-offline-error.png",
            "129-first-app-grant-required.png",
            "130-first-app-policy-granted.png",
            "131-first-app-offline-error.png",
            "132-first-app-policy-blocked.png",
            "133-setup-accessibility-reduced-motion.png",
            "134-install-progress-reduced-motion-a.png",
            "135-install-progress-reduced-motion-b.png",
            "136-settings-models-advanced-expanded-dark.png",
            "137-studio-engine-menu-dark.png",
            "138-first-boot-codex-offline.png",
        ] {
            assert!(
                unique.contains(screenshot),
                "missing required polish interaction screenshot contract {screenshot}"
            );
        }
        assert_eq!(
            POLISH_INTERACTION_PROOF,
            "139-polish-interactions-proof.json"
        );
    }

    #[test]
    fn install_map_mirrors_the_container_layout() {
        let files = install_files(Path::new("/src"), Path::new("/src/target/release"));

        // One destination per binary, unit, app, autostart entry, plus the
        // session launcher, wayland session, gnome session, bootc config, env,
        // empty secret template, dconf defaults, GLib schemas, GDM autologin config,
        // AccountsService default-session profile, custom gnome-shell mode,
        // bundled shell-extension files, and first-class theme/action assets.
        let expected = BINARIES.len()
            + SYSTEMD_UNITS.len()
            + SYSTEMD_SYSTEM_DROPINS.len()
            + SYSTEMD_USER_UNITS.len()
            + APPLICATIONS.len()
            + AUTOSTART.len()
            + 9
            + DCONF_FILES.len()
            + GLIB_SCHEMA_FILES.len()
            + GNOME_SHELL_EXTENSION_FILES.len()
            + ICON_THEME_FILES.len()
            + 2
            + (NAUTILUS_SCRIPTS.len() * 2);
        assert_eq!(files.len(), expected);

        // Binaries are sourced from the build output and land under libexec.
        assert!(files.iter().any(|(src, dst)| src
            == Path::new("/src/target/release/goblins-os-core")
            && dst == "usr/libexec/goblins-os/goblins-os-core"));

        // Every destination is a relative path inside the install root.
        assert!(files
            .iter()
            .all(|(_, dst)| !dst.starts_with('/') && !dst.contains("..")));

        // The session launcher the installed contract greps for must be staged.
        assert!(files
            .iter()
            .any(|(_, dst)| dst == "usr/libexec/goblins-os/goblins-os-session"));
    }

    #[test]
    fn native_design_checks_accept_the_shared_ui_theming_boundary() {
        let root = std::env::temp_dir().join(format!(
            "goblins-os-verify-native-design-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);

        fs::create_dir_all(root.join("crates/goblins-os-design/src")).unwrap();
        fs::create_dir_all(root.join("crates/goblins-os-ui/src")).unwrap();
        fs::create_dir_all(root.join("os/gnome-shell-extensions/goblins-wm@goblins.os")).unwrap();
        for app in NATIVE_DESIGN_APPS {
            fs::create_dir_all(root.join(format!("crates/{app}/src"))).unwrap();
            fs::write(
                root.join(format!("crates/{app}/Cargo.toml")),
                concat!(
                    "[features]\n",
                    "native-desktop = [\"goblins-os-ui/native-desktop\"]\n",
                    "[dependencies]\n",
                    "goblins-os-ui = { path = \"../goblins-os-ui\" }\n",
                ),
            )
            .unwrap();
            fs::write(
                root.join(format!("crates/{app}/src/main.rs")),
                "fn main() { goblins_os_ui::init_theming(\"\"); }\n",
            )
            .unwrap();
        }
        fs::write(
            root.join("crates/goblins-os-settings/src/main.rs"),
            concat!(
                "const SETTINGS_DEFAULT_WIDTH: i32 = 1055;\n",
                "fn main() { goblins_os_ui::init_theming(\"\"); }\n",
                "fn sidebar() { nav_scroll.set_overlay_scrolling(false); }\n",
                "const CSS: &str = \".gos-settings-root .gos-side-scroll scrollbar.vertical slider\";\n",
            ),
        )
        .unwrap();

        fs::write(
            root.join("Cargo.toml"),
            "members = [\"crates/goblins-os-design\", \"crates/goblins-os-ui\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/goblins-os-design/src/lib.rs"),
            concat!(
                "pub const GOBLINS_NATIVE_CSS: &str = \"",
                ".gos-launcher-entry > text selection ",
                "background: @gos_material_active; ",
                ".gos-cc-tile.is-on {\n  background: @gos_material_regular; ",
                ".gos-cc-seg.is-active {\n  color: @gos_ink;\n  background: @gos_material_active; ",
                "\";\n",
                "fn launcher_and_control_center_selection_stays_material_not_primary_flood() {}\n",
                "pub fn native_css(app_css: &str, _dark: bool) -> String {\n",
                "    app_css.to_string()\n",
                "}\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("crates/goblins-os-ui/Cargo.toml"),
            concat!(
                "[features]\n",
                "native-desktop = [\"dep:gtk4\"]\n",
                "[dependencies]\n",
                "goblins-os-design = { path = \"../goblins-os-design\" }\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("crates/goblins-os-ui/src/lib.rs"),
            concat!(
                "pub fn init_theming(app_css: &'static str) {\n",
                "    let _ = goblins_os_design::native_css(app_css, false);\n",
                "}\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            concat!(
                "const body = new St.BoxLayout({style_class: 'goblins-wm-body', y_expand: true});\n",
                "const spaces = new St.BoxLayout({y_align: Clutter.ActorAlign.END});\n",
            ),
        )
        .unwrap();

        let blocked = native_design_system_checks(&root)
            .into_iter()
            .filter(|check| check.state == CheckState::Blocked)
            .count();

        let _ = fs::remove_dir_all(&root);
        assert_eq!(blocked, 0);
    }

    #[test]
    fn dummy_secret_allowlist_only_shields_sequential_alphabet_runs() {
        // Build the real-shaped "example" token via concatenation so the source
        // scanner never sees a contiguous 32+ char sk- literal in this file.
        let example_token = concat!("sk-", "proj-", "exampleA9f4kQ2mZ7tR1bX8nL3vC6dW0pY5hJ");
        // The load-bearing sequential-alphabet dummy stays shielded.
        assert!(is_allowed_dummy_secret(
            "sk-proj-abcdefghijklmnopqrstuvwxyz"
        ));
        // English-word substrings no longer grant a free pass: a real-shaped key
        // that merely contains "example" must NOT be treated as a dummy.
        assert!(!is_allowed_dummy_secret(example_token));
    }

    #[test]
    fn realish_key_scanner_flags_example_token_but_not_alphabet_dummy() {
        let example_line = concat!(
            "OPENAI_API_KEY=sk-",
            "proj-",
            "exampleA9f4kQ2mZ7tR1bX8nL3vC6dW0pY5hJ"
        );
        // A real-shaped 32+ char token containing "example" is reported.
        assert!(contains_realish_openai_key(example_line));
        // The sequential-alphabet dummy is still shielded.
        assert!(!contains_realish_openai_key(
            "OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwxyz"
        ));
    }

    #[test]
    fn suspicious_secret_line_flags_generic_key_secret_token_assignments() {
        // Generic *_KEY/_SECRET/_TOKEN assignments are now flagged when populated.
        assert!(is_suspicious_secret_line(
            "GOBLINS_OS_RESIDENT_RELAY_TOKEN=actual-bearer-value"
        ));
        assert!(is_suspicious_secret_line(
            "export SOME_CLIENT_SECRET=real-value"
        ));
        // Placeholders, empty values, and commented lines stay clean.
        assert!(!is_suspicious_secret_line("OPENAI_ACCOUNT_TOKEN_URL=<url>"));
        assert!(!is_suspicious_secret_line("CUSTOM_TOKEN="));
        assert!(!is_suspicious_secret_line(
            "# OPENAI_ACCOUNT_AUTH_TOKEN=set-server-side"
        ));
        // Mid-line matches must not trip the line-anchored rule.
        assert!(!is_suspicious_secret_line(
            "let label = \"the API_KEY=value pair\";"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn path_mode_check_rejects_setuid_setgid_sticky_bits() {
        use super::path_mode_check;
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("goblins-os-verify-mode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("secret"), b"x").unwrap();

        fs::set_permissions(root.join("secret"), fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            path_mode_check(&root, "secret", 0o600, "test").state,
            CheckState::Ready
        );

        // setuid bit set: now masked with 0o7777, so 04600 != 0600 and is blocked.
        fs::set_permissions(root.join("secret"), fs::Permissions::from_mode(0o4600)).unwrap();
        assert_eq!(
            path_mode_check(&root, "secret", 0o600, "test").state,
            CheckState::Blocked
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn path_owner_check_reports_owner_mismatch() {
        use super::path_owner_check;
        use std::os::unix::fs::MetadataExt;

        let root =
            std::env::temp_dir().join(format!("goblins-os-verify-owner-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("secret"), b"x").unwrap();

        let metadata = fs::metadata(root.join("secret")).unwrap();
        let uid = metadata.uid();
        let gid = metadata.gid();

        // Matching uid/gid is ready.
        assert_eq!(
            path_owner_check(&root, "secret", uid, gid, "test").state,
            CheckState::Ready
        );
        // A root:root expectation against a non-root-owned fixture is blocked
        // (the test harness does not run as root, so uid != 0).
        if uid != 0 {
            assert_eq!(
                path_owner_check(&root, "secret", 0, 0, "test").state,
                CheckState::Blocked
            );
        }
        // Missing path is blocked.
        assert_eq!(
            path_owner_check(&root, "absent", uid, gid, "test").state,
            CheckState::Blocked
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn secret_dir_check_blocks_group_other_read_bits() {
        use super::secret_dir_not_session_readable_check;
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "goblins-os-verify-secret-dir-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("dir")).unwrap();
        let owner_uid = fs::metadata(root.join("dir")).unwrap().uid();

        // 0700 with no group/other access is ready — unless the test harness
        // itself runs as the desktop session uid 1000, which is itself a block.
        fs::set_permissions(root.join("dir"), fs::Permissions::from_mode(0o700)).unwrap();
        let expected_clean = if owner_uid == 1000 {
            CheckState::Blocked
        } else {
            CheckState::Ready
        };
        assert_eq!(
            secret_dir_not_session_readable_check(&root, "dir", "test").state,
            expected_clean
        );

        // Any group/other bit makes the secret dir session-reachable: blocked.
        fs::set_permissions(root.join("dir"), fs::Permissions::from_mode(0o750)).unwrap();
        assert_eq!(
            secret_dir_not_session_readable_check(&root, "dir", "test").state,
            CheckState::Blocked
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn installed_state_dir_owner_check_skips_when_passwd_unresolvable() {
        use super::{installed_state_dir_owner_check, resolve_group_id};

        // A staged DESTDIR has no etc/passwd, so goblins-os is unresolvable and
        // the ownership assertion resolves to ready/skip rather than failing.
        let root = std::env::temp_dir().join(format!(
            "goblins-os-verify-state-owner-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("var/lib/goblins-os/apps")).unwrap();

        assert_eq!(
            installed_state_dir_owner_check(&root, "var/lib/goblins-os/apps", "goblins-os", "test")
                .state,
            CheckState::Ready
        );

        // With a resolvable passwd entry, ownership is enforced; a mismatch blocks.
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::write(
            root.join("etc/passwd"),
            "root:x:0:0:root:/root:/bin/bash\ngoblins-os:x:991:991::/var/lib/goblins-os:/usr/sbin/nologin\n",
        )
        .unwrap();
        fs::write(
            root.join("etc/group"),
            "root:x:0:\ngoblins-core-resident:x:992:goblins-os\n",
        )
        .unwrap();
        assert_eq!(resolve_group_id(&root, "goblins-core-resident"), Some(992));
        assert_eq!(resolve_group_id(&root, "missing"), None);
        assert_eq!(
            installed_state_dir_owner_check(&root, "var/lib/goblins-os/apps", "goblins-os", "test")
                .state,
            CheckState::Blocked
        );

        let _ = fs::remove_dir_all(&root);
    }
}
