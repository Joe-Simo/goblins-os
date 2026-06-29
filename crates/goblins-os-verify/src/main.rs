#[cfg(not(unix))]
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::FileExt;
use std::{
    collections::HashMap,
    env,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

const BINARIES: &[&str] = &[
    "goblins-os-control-center",
    "goblins-os-core",
    "goblins-os-file-builder",
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
}

struct Config {
    mode: Mode,
    root: PathBuf,
    source: PathBuf,
    binaries: PathBuf,
    quiet: bool,
    release_arch: Option<String>,
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
        let manifest = write_release_evidence(&config.source, arch, &config.root)?;
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
                "--quiet" => quiet = true,
                "--help" | "-h" => return Err(VerifyError::Usage),
                _ => return Err(VerifyError::UnknownArgument(arg)),
            }
        }

        if let Some(output) = release_evidence_output {
            if installed_root.is_some() || stage_root.is_some() || binaries.is_some() {
                return Err(VerifyError::Usage);
            }
            let arch = release_arch.ok_or(VerifyError::Usage)?;
            if !SUPPORTED_RELEASE_ARCHES.contains(&arch.as_str()) {
                return Err(VerifyError::InvalidArchitecture(arch));
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
            }),
            (None, Some(root)) => Ok(Self {
                mode: Mode::Installed,
                source: root.clone(),
                binaries: default_binaries(&root),
                root,
                quiet,
                release_arch: None,
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
                    })
                } else {
                    Ok(Self {
                        mode: Mode::Installed,
                        source: PathBuf::from("/"),
                        binaries: PathBuf::from("/usr/libexec/goblins-os"),
                        root: PathBuf::from("/"),
                        quiet,
                        release_arch: None,
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
                "usage: goblins-os-verify [--source-root <path> | --installed-root <path> | --stage <destdir> [--binaries <dir>] | --release-evidence <output-dir> --arch <aarch64|x86_64>] [--quiet]",
            ),
            Self::UnknownArgument(arg) => write!(formatter, "unknown argument {arg}"),
            Self::MissingValue(arg) => write!(formatter, "missing value for {arg}"),
            Self::InvalidArchitecture(arch) => write!(
                formatter,
                "unsupported architecture {arch}; expected aarch64 or x86_64"
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
    checks.push(contains_check(
        root.join("os/gnome-session/goblins-os.session"),
        "gnome-session-shell-component",
        "org.goblins.OS.Shell",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "gnome-session-systemd-gnome-shell-service",
        "Requires=org.gnome.Shell@user.service",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "gnome-session-systemd-shell-target",
        "Requires=org.goblins.OS.Shell.target",
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
        root.join("os/bootc/Containerfile"),
        "bootc-creates-session-bridge-group",
        "groupadd --system goblins-session-bridge",
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
        root.join("crates/goblins-os-core/src/session_bridge.rs"),
        "core-session-bridge-client-uses-unix-socket",
        "UnixStream::connect",
    ));
    checks.push(contains_check(
        root.join("os/etc/goblins-os/environment"),
        "environment-primary-core-url-is-goblins-native",
        "GOBLINS_OS_CORE_URL=http://127.0.0.1:8787",
    ));
    checks.push(contains_check(
        root.join("os/etc/goblins-os/environment"),
        "environment-session-bridge-socket-is-stable",
        "GOBLINS_OS_SESSION_BRIDGE_SOCKET=/run/user/1000/goblins-os/session-bridge.sock",
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
    checks.push(contains_check(
        root.join("os/session/goblins-os-session"),
        "session-exports-goblins-core-url-first",
        "export GOBLINS_OS_CORE_URL=\"${GOBLINS_OS_CORE_URL:-${OPENAI_OS_CORE_URL:-http://127.0.0.1:8787}}\"",
    ));
    checks.push(absent_check(
        root.join("os/session/goblins-os-session"),
        "session-does-not-export-legacy-core-url",
        "export OPENAI_OS_CORE_URL=",
    ));
    checks.push(contains_check(
        root.join("os/systemd-user/org.goblins.OS.Shell.service"),
        "shell-service-primary-core-url-is-goblins-native",
        "Environment=GOBLINS_OS_CORE_URL=http://127.0.0.1:8787",
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
            "installer-core-url-prefers-goblins-env-name",
            "crates/goblins-os-installer/src/main.rs",
        ),
        (
            "login-core-url-prefers-goblins-env-name",
            "crates/goblins-os-login/src/main.rs",
        ),
        (
            "shell-core-url-prefers-goblins-env-name",
            "crates/goblins-os-shell/src/main.rs",
        ),
        (
            "settings-core-url-prefers-goblins-env-name",
            "crates/goblins-os-settings/src/main.rs",
        ),
        (
            "launcher-core-url-prefers-goblins-env-name",
            "crates/goblins-os-launcher/src/main.rs",
        ),
        (
            "control-center-core-url-prefers-goblins-env-name",
            "crates/goblins-os-control-center/src/main.rs",
        ),
        (
            "open-helper-core-url-prefers-goblins-env-name",
            "crates/goblins-os-open/src/main.rs",
        ),
        (
            "file-builder-core-url-prefers-goblins-env-name",
            "crates/goblins-os-file-builder/src/main.rs",
        ),
        (
            "resident-core-url-prefers-goblins-env-name",
            "crates/goblins-os-resident/src/main.rs",
        ),
    ] {
        checks.push(contains_check(
            root.join(path),
            id,
            "env::var(\"GOBLINS_OS_CORE_URL\")",
        ));
    }
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
    checks.extend(systemd_hardening_checks(root));
    checks.push(bootc_install_config_check(root));
    checks.extend(goblins_ai_contract_checks(root));
    checks.extend(native_design_system_checks(root));
    checks.extend(acquisition_readiness_checks(root));
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
        "gdm-autologin-config",
        "COPY os/gdm/custom.conf /etc/gdm/custom.conf",
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
        "gdm-autologin-enabled",
        "AutomaticLogin=goblin",
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
        "docker/setup-buildx-action@v3",
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
        "render-wm-mission-control",
        "52-wm-mission-control-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-app-expose",
        "52b-wm-app-expose-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-hot-corner",
        "52c-wm-hot-corner-$suffix.png",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render-desktop.sh"),
        "render-wm-spaces",
        "53-wm-spaces-$suffix.png",
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
        "Point selection needs live qemu proof before pointer injection is enabled.",
    ));
    checks.push(contains_check(
        root.join("os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js"),
        "goblins-switch-render-hook",
        "showPointScanDemo",
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
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "installed-gnome-session-systemd-gnome-shell-service",
        "Requires=org.gnome.Shell@user.service",
    ));
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
        "installed-gnome-session-systemd-shell-target",
        "Requires=org.goblins.OS.Shell.target",
    ));
    checks.push(contains_check(
        root.join("usr/lib/systemd/user/org.goblins.OS.Shell.service"),
        "installed-goblins-shell-user-service-exec",
        "ExecStart=/usr/libexec/goblins-os/goblins-os-shell",
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
    for relative in [
        "var/lib/goblins-os/models",
        "var/lib/goblins-os/apps",
        "var/lib/goblins-os/installer",
        "var/lib/goblins-os/session",
        "var/lib/goblins-os/policy",
        "var/lib/goblins-os/resident",
    ] {
        checks.push(installed_state_dir_check(root, relative));
        checks.push(installed_state_dir_owner_check(
            root,
            relative,
            &format!("{relative}-owner"),
        ));
    }
    checks.push(file_check(root, "etc/gdm/custom.conf"));
    checks.push(contains_check(
        root.join("etc/gdm/custom.conf"),
        "installed-gdm-autologin",
        "AutomaticLogin=goblin",
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

fn write_release_evidence(
    source: &Path,
    arch: &str,
    output: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    if !SUPPORTED_RELEASE_ARCHES.contains(&arch) {
        return Err(format!("unsupported architecture {arch}").into());
    }

    fs::create_dir_all(output)?;
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

    fs::write(
        output.join("cargo-lock-packages.tsv"),
        cargo_lock_packages_tsv(&packages),
    )?;
    fs::write(output.join("rpm-packages.command"), rpm_packages_command())?;
    let rpm_status = write_rpm_packages_if_available(output)?;
    let manifest = output.join("release-evidence-manifest.json");
    fs::write(
        &manifest,
        release_evidence_manifest(arch, packages.len(), &rpm_status),
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

fn rpm_packages_command() -> &'static str {
    "#!/usr/bin/env sh\nset -eu\ntmp=\"${TMPDIR:-/tmp}/goblins-os-rpm-packages.$$\"\ntrap 'rm -f \"$tmp\"' EXIT\nrpm -qa --qf '%{NAME}\\t%{VERSION}-%{RELEASE}\\t%{ARCH}\\t%{LICENSE}\\n' | LC_ALL=C sort > \"$tmp\"\n{\n  printf 'name\\tversion_release\\tarch\\tlicense\\n'\n  cat \"$tmp\"\n} > rpm-packages.tsv\n"
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
            Ok("generated from rpm database".to_string())
        }
        Ok(result) => {
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

fn release_evidence_manifest(arch: &str, cargo_package_count: usize, rpm_status: &str) -> String {
    format!(
        concat!(
            "{{\n",
            "  \"schema\": \"goblins-os-release-evidence-v1\",\n",
            "  \"architecture\": \"{}\",\n",
            "  \"cargo_lock\": \"Cargo.lock\",\n",
            "  \"cargo_package_count\": {},\n",
            "  \"cargo_packages_tsv\": \"cargo-lock-packages.tsv\",\n",
            "  \"rpm_packages_tsv\": \"rpm-packages.tsv\",\n",
            "  \"rpm_command_file\": \"rpm-packages.command\",\n",
            "  \"rpm_status\": \"{}\",\n",
            "  \"asset_provenance\": \"os/release/asset-provenance.toml\",\n",
            "  \"third_party_notices\": \"os/release/third-party-notices.toml\",\n",
            "  \"trademark_posture\": \"os/release/trademark-posture.toml\",\n",
            "  \"source_tree_manifest\": \"os/release/source-tree-manifest.toml\"\n",
            "}}\n"
        ),
        json_escape(arch),
        cargo_package_count,
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
    let crate_name = binary.strip_prefix("goblins-os-").unwrap_or(binary);
    let member = format!("crates/goblins-os-{crate_name}");
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
    let expected = format!(
        "COPY --from=rust-build /src/target/release/{binary} /usr/libexec/goblins-os/{binary}"
    );
    container_contains_check(root, &format!("container-copy-{binary}"), &expected)
}

fn container_contains_check(root: &Path, id: &str, needle: &str) -> Check {
    contains_check(root.join("os/bootc/Containerfile"), id, needle)
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

fn source_secret_scan_check(root: &Path) -> Check {
    match source_secret_scan_hits(root) {
        Ok(hits) if hits.is_empty() => ready(
            "source-secret-scan",
            "no live OpenAI-style keys or active secret assignments found in source package",
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
            "Goblins OS builds it locally with the selected engine",
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
            "GOBLINS_OS_RENDER_SCOPE=$RENDER_SCOPE (expected all, chrome, installer, settings, or settings-interactions)",
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
            "native-mission-control-body-reserves-footer-space",
            "style_class: 'goblins-wm-body', y_expand: true",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "native-mission-control-spaces-strip-footer-aligned",
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

fn acquisition_readiness_checks(root: &Path) -> Vec<Check> {
    vec![
        file_check(root, "os/release/source-tree-manifest.toml"),
        file_check(root, "os/release/asset-provenance.toml"),
        file_check(root, "os/release/third-party-notices.toml"),
        file_check(root, "os/release/trademark-posture.toml"),
        file_check(root, "os/release/architectures.toml"),
        file_check(root, "os/release/acquisition-readiness-delta.toml"),
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
            "source-manifest-classifies-voice-helpers",
            "os/voice/",
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
            "rpm-packages.command",
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
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-records-rust-source-gates",
            "rust_source_gates_available",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-records-source-package-materialized",
            "source_package_materialized",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-source-root-is-portable",
            "root = \".\"",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-links-source-tree-manifest",
            "source_tree_manifest = \"os/release/source-tree-manifest.toml\"",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-records-dual-arch-rpm-sbom-proof",
            "dual_arch_rpm_sbom_present",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-blocks-on-native-linux-runner",
            "native_linux_release_runner_required",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-blocks-on-shippable-release-isos",
            "shippable_release_iso_artifacts_incomplete",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-blocks-on-display-proofs",
            "display_backed_architecture_proofs_missing",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-records-x86-rpm-sbom-proof",
            "x86_64_rpm_sbom_present",
        ),
        contains_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-blocks-on-complete-signoff",
            "complete_signoff_rows_missing",
        ),
        absent_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-no-stale-rust-missing-blocker",
            "rust_toolchain_missing",
        ),
        absent_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-no-stale-dataless-blocker",
            "source_files_dataless",
        ),
        absent_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-no-stale-disk-space-blocker",
            "disk_space_low",
        ),
        absent_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-no-stale-x86-rpm-sbom-blocker",
            "x86_64_rpm_sbom_missing",
        ),
        absent_check(
            root.join("os/release/acquisition-readiness-delta.toml"),
            "acquisition-delta-no-local-user-path",
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
            "client GUIs only ever receive booleans and file paths",
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
        source_secret_scan_check(root),
    ]
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
            "iso-builder-docker-local-registry-handoff",
            "host.docker.internal",
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
            "hardware-gate-direct-registry-build-action",
            "docker/build-push-action@v7",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-direct-registry-build-action-push",
            "push: true",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-buildx-builder-action",
            "docker/setup-buildx-action@v3",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-buildkit-gha-cache-scope",
            "type=gha,scope=goblins-os-bootc-${{ matrix.arch }}",
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
            "RELEASE_IMAGE=<registry>/<namespace>/goblins-os:$ARCH",
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
            "runbook-documents-textshortcuts-live-keystroke-proof",
            "text-shortcuts-live-keystroke-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-metadata-proof",
            "text-shortcuts-candidate-metadata-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-overlay-intent-proof",
            "text-shortcuts-overlay-intent-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-bubble-frame-proof",
            "text-shortcuts-candidate-bubble-frame-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-bubble-layout-proof",
            "text-shortcuts-candidate-bubble-layout-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-bubble-render-intent-proof",
            "text-shortcuts-candidate-bubble-render-intent-proof.json",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-candidate-bubble-render-proof",
            "text-shortcuts-candidate-bubble-render-proof.json",
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
            "runbook-documents-textshortcuts-candidate-no-live-overlay-claim",
            "live_overlay_claim=false",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-escape-dismiss-proof",
            "Escape dismiss without a",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-passthrough-proof",
            "unknown word stays pass-through",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-final-runtime-proof",
            "focused-field callback",
        ),
        contains_check(
            root.join("os/hardware-gate/runbook.md"),
            "runbook-documents-textshortcuts-core-readiness-deferred",
            "core_readiness_flip=deferred",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-firewall-live-toggle-proof",
            "/proof/firewall-live-toggle",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-session-proof",
            "/proof/text-shortcuts-session-enable",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-posts-textshortcuts-live-keystroke-proof",
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
            "capture-harness-drives-textshortcuts-live-runtime-render-proof-app",
            "--text-shortcuts-proof live-runtime-render",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-propagates-textshortcuts-live-proof-ledger-env",
            "systemctl --user set-environment GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS",
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
            "capture-harness-app-privacy-seeds-permission-store",
            "PermissionStore.SetPermission",
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
            "capture-harness-verifies-textshortcuts-user-service",
            "org.goblins.OS.IBus.service",
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
            "capture-harness-drives-textshortcuts-with-wtype",
            "wtype -- \"omw.\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-opens-textshortcuts-dismiss-proof",
            "goblins-os-shell\" --text-shortcuts-proof dismiss",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-opens-textshortcuts-passthrough-proof",
            "goblins-os-shell\" --text-shortcuts-proof passthrough",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-drives-textshortcuts-passthrough",
            "wtype -- \"hello.\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-textshortcuts-passthrough-unchanged",
            "passthrough_unchanged=true",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-drives-textshortcuts-escape-dismiss",
            "wtype -P Escape -p Escape",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/in-session-orchestrator.sh"),
            "capture-harness-checks-textshortcuts-dismiss-no-commit",
            "dismiss_no_commit=true",
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
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-writes-textshortcuts-live-keystroke-proof-json",
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
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-firewall-proof",
            "HONESTY GUARD: missing or failing live firewall toggle proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-session-proof",
            "HONESTY GUARD: missing or failing Text Shortcuts session-enable proof",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-keystroke-proof",
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
            "capture-run-guards-textshortcuts-live-ibus-text-input-v3",
            "\"text_input_v3_commit\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-rendered-accept-bubble",
            "\"rendered_accept_bubble\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-live-ibus-core-readiness-deferred",
            "\"core_readiness_flip\": \"deferred\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-dismiss-proof",
            "\"dismiss_no_commit\": \"true\"",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-guards-textshortcuts-passthrough-proof",
            "\"passthrough_unchanged\": \"true\"",
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
            "passthrough_unchanged",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-passthrough-proof",
            "passthrough_unchanged",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-dismiss-proof",
            "dismiss_no_commit",
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-dismiss-proof",
            "dismiss_no_commit",
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
            "capture-driver-waits-for-automated-kickstart-progress-diagnostics",
            "Anaconda automated kickstart progress",
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
            root.join("os/hardware-gate/capture-harness/run-capture.sh"),
            "capture-run-does-not-pretend-oemdrv-overrides-embedded-osbuild-ks",
            "make-oemdrv.sh",
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
            "capture-driver-selects-first-boot-private-path",
            "first boot setup: selecting welcome window and clicking private offline path",
        ),
        contains_check(
            root.join("os/hardware-gate/capture-harness/drive-capture.py"),
            "capture-driver-saves-post-first-boot-dismiss-debug-frame",
            "post first boot dismiss",
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
            "capture-run-copies-failure-capture-logs",
            "_capture-logs",
        ),
        contains_check(
            root.join(".github/workflows/hardware-gate-capture.yml"),
            "hardware-gate-uploads-artifact-on-failure",
            "if: always()",
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
            "docker/build-push-action@v7",
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
            "hardware-gate-iso-sha-verify",
            "sha256sum -c",
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
            "shipping-status-validates-screenshot-png-signature",
            "89504e470d0a1a0a",
        ),
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-screenshot-proof-manifest",
            "proof-manifest.json",
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
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-requires-textshortcuts-live-keystroke-proof",
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
            "signoff_block_required_proof_is_complete",
            "shipping-status-signoff-requires-preview-open-render-proof",
            r#"signoff_block_contains "$block" "^- Preview open/render checked: yes" || return 1"#,
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
        contains_check(
            root.join("os/hardware-gate/verify-shipping-status.sh"),
            "shipping-status-textshortcuts-live-keystroke-proof-filename",
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
            r#"rg -Fq 'Command::new(\"xdg-open\")'"#,
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
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-requires-textshortcuts-live-keystroke-proof",
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
            "close-signoff-records-textshortcuts-live-keystroke-proof",
            "Text Shortcuts live keystrokes checked",
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
            "close-signoff-preview-open-render-status-completes-project",
            r#"[[ "$PREVIEW_OPEN_RENDER_STATUS" == yes* ]]"#,
        ),
        contains_check(
            root.join("os/hardware-gate/close-signoff.sh"),
            "close-signoff-focus-arm-roundtrip-status-completes-project",
            r#"[[ "$FOCUS_ARM_ROUNDTRIP_STATUS" == yes* ]]"#,
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
            "rpm-packages.command",
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
            "release-workflow-buildx-builder-action",
            "docker/setup-buildx-action@v3",
        ),
        contains_check(
            root.join(".github/workflows/release.yml"),
            "release-workflow-buildkit-gha-cache-scope",
            "type=gha,scope=goblins-os-bootc-${{ matrix.arch }}",
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
            "core-openai-default-model-current-gpt55",
            "gpt-5.5",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-declares-openai-agents-sdk",
            "Official OpenAI Agents SDK",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-build-studio-agents-sdk-relay",
            "Ready through the server-side official OpenAI Agents SDK relay.",
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
            "openai-secret-template-build-studio-agents-sdk-relay",
            "Build Studio can use",
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
            root.join("crates/goblins-os-core/src/service_catalog.rs"),
            "service-catalog-prefers-goblins-agents-sdk-relay-env",
            "GOBLINS_OS_AGENTS_SDK_RELAY_URL",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-agents-sdk-relay-primary-env",
            "const AGENTS_SDK_RELAY_ENV: &str = \"GOBLINS_OS_AGENTS_SDK_RELAY_URL\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-agents-sdk-relay-compat-env",
            "const AGENTS_SDK_RELAY_LEGACY_ENV: &str = \"OPENAI_OS_AGENTS_SDK_RELAY_URL\"",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-agents-sdk-source-marker",
            "official-openai-agents-sdk",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-agents-sdk-capabilities",
            "sandbox-execution",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-agents-sdk-https-only",
            "server_https_url(&url)",
        ),
        contains_check(
            root.join("crates/goblins-os-core/src/app_builder.rs"),
            "app-builder-agents-sdk-secret-header",
            "\"Authorization\"",
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
            "settings-ai-runtime-copy",
            "Goblins AI runtime",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-ai-runtime-state-copy",
            "Goblins AI runtime is {}",
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
            "settings-ai-route-copy",
            "Goblins AI route",
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
            "ai_action_availability(&config.core_url, \"ask-selected-text\")",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-gates-writing-tools",
            "ai_action_availability(&config.core_url, \"write-with-goblins\")",
        ),
        contains_check(
            root.join("crates/goblins-os-control-center/src/main.rs"),
            "control-center-gates-screen-context",
            "ai_action_availability(&config.core_url, \"summarize-screen\")",
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
            "Enter Goblins OS desktop",
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
            "Microphone capture is not ready.",
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
            "goblins-os-screenshot-context /usr/libexec/goblins-os/goblins-os-screenshot-context",
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
            "Command::new(\"ibus\").arg(\"list-engine\")",
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
            "firewalld \\\n      dnsmasq \\\n      ntfs-3g \\\n      exfatprogs \\\n      udisks2 \\\n      rsync \\\n      wl-clipboard",
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
            "Command::new(\"xdg-open\")",
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
            "systemctl enable --now firewalld.service",
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
        file_check(root, "os/focus/goblins-os-focus-tick"),
        file_check(root, "os/systemd-user/org.goblins.OS.FocusTick.service"),
        file_check(root, "os/systemd-user/org.goblins.OS.FocusTick.timer"),
        contains_check(
            root.join("os/focus/goblins-os-focus-tick"),
            "focus-tick-helper-posts-core-route",
            "/v1/focus/tick",
        ),
        contains_check(
            root.join("os/focus/goblins-os-focus-tick"),
            "focus-tick-helper-local-core-only",
            "core URL must be local HTTP",
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
            "COPY --chmod=0755 os/focus/goblins-os-focus-tick /usr/libexec/goblins-os/goblins-os-focus-tick",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "focus-tick-python-asserted",
            "command -v python3",
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
            "core-app-privacy-revoke-validates-ids",
            "permission_id_is_safe",
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
            "COPY --from=rust-build /src/target/release/goblins-textshortcuts-engine /usr/libexec/goblins-os/goblins-textshortcuts-engine",
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
            "bootc-requires-textshortcuts-gi-adapter-contract-preedit",
            "grep -q '\"preedit_update\": true' /tmp/goblins-textshortcuts-gi-adapter-contract.json",
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
            "bootc-requires-textshortcuts-adapter-callback-ledger-preedit",
            "grep -q '\"update-preedit-text\"' /tmp/goblins-textshortcuts-adapter-callback-ledger.json",
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
            "runtime did not answer before timeout",
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
            "textshortcuts-ibus-adapter-gi-contract-preedit",
            "\"preedit_update\": preedit_update",
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
            "gsettings set org.gnome.desktop.input-sources sources",
        ),
        contains_check(
            root.join("os/input/goblins-os-input-source-seed"),
            "input-source-seed-appends-goblins-preload",
            "gsettings set org.freedesktop.ibus.general preload-engines",
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
            "Before=org.goblins.OS.IBus.service",
        ),
        file_check(root, "os/systemd-user/org.goblins.OS.IBus.service"),
        contains_check(
            root.join("os/systemd-user/org.goblins.OS.IBus.service"),
            "textshortcuts-ibus-user-service-exec",
            "ExecStart=/usr/bin/ibus-daemon --replace --xim --panel disable",
        ),
        contains_check(
            root.join("os/systemd-user/org.goblins.OS.IBus.service"),
            "textshortcuts-ibus-waits-for-input-source-seed",
            "After=gnome-session-initialized.target org.gnome.Shell@user.service org.goblins.OS.InputSourcesSeed.service",
        ),
        contains_check(
            root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
            "textshortcuts-input-source-seed-wanted-by-session",
            "Wants=org.goblins.OS.InputSourcesSeed.service",
        ),
        contains_check(
            root.join("os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf"),
            "textshortcuts-ibus-user-service-wanted-by-session",
            "Wants=org.goblins.OS.IBus.service",
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
            "core-text-shortcuts-runtime-pending-honesty",
            "runtime loop is still pending CI/qemu proof",
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
            "textshortcuts-engine-runtime-preedit-candidate",
            "IbusOperation::UpdatePreeditText",
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
            "shell-textshortcuts-proof-live-render-marker",
            "rendered_accept_bubble={rendered}",
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
            root.join("os/voice/goblins-os-voice-control"),
            "voice-control-helper-calls-core-route",
            "/v1/voice/control",
        ),
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-copies-voice-control-helper",
            "goblins-os-voice-control",
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
            "vision_status(&config.core_host)",
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
            "goblins-wm-app-expose-handler",
            "_showAppExpose",
        ),
        contains_check(
            root.join("os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js"),
            "goblins-wm-app-expose-render-hook",
            "showAppExposeDemo",
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
        // Color picker — portal eyedropper helper, packaged + keybound, copying via
        // wl-clipboard with an honest no-clipboard fallback.
        contains_check(
            root.join("os/bootc/Containerfile"),
            "bootc-color-picker-helper",
            "goblins-os-color-picker /usr/libexec/goblins-os/goblins-os-color-picker",
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

/// Assert an installed state directory is owned by the `goblins-os` service
/// account. This only has teeth against a real installed `/` (where
/// `etc/passwd` exists); against a staged DESTDIR the passwd database is absent,
/// so the check resolves to `ready`/skip and never fails the stage gate. We do
/// not assert a uniform mode — those diverge per directory in the Containerfile.
#[cfg(unix)]
fn installed_state_dir_owner_check(root: &Path, relative: &str, id: &str) -> Check {
    use std::os::unix::fs::MetadataExt;

    let Some((expected_uid, expected_gid)) = resolve_passwd_id(root, "goblins-os") else {
        return ready(
            id,
            &format!("ownership check skipped for {relative}: goblins-os not resolvable from rootfs passwd"),
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
                    &format!("{} owned by goblins-os ({uid}:{gid})", path.display()),
                )
            } else {
                blocked(
                    id,
                    &format!(
                        "{} owned by uid {uid}:{gid}; expected goblins-os {expected_uid}:{expected_gid}",
                        path.display()
                    ),
                )
            }
        }
        Err(_) => blocked(id, &format!("missing {}", path.display())),
    }
}

#[cfg(not(unix))]
fn installed_state_dir_owner_check(_root: &Path, relative: &str, id: &str) -> Check {
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
        cargo_lock_packages, contains_realish_openai_key, desktop_field, install_files,
        is_allowed_dummy_secret, is_suspicious_secret_line, native_design_system_checks,
        rg_secret_scan_hit, should_skip_secret_scan_path, source_manifest_classifies_top_level,
        stable_id, write_release_evidence, CheckState, APPLICATIONS, AUTOSTART, BINARIES,
        DCONF_FILES, GLIB_SCHEMA_FILES, GNOME_SHELL_EXTENSION_FILES, ICON_THEME_FILES,
        NATIVE_DESIGN_APPS, NAUTILUS_SCRIPTS, SETTINGS_INTERACTION_SCREENSHOTS,
        SETTINGS_RENDER_SCREENSHOTS, SYSTEMD_SYSTEM_DROPINS, SYSTEMD_UNITS, SYSTEMD_USER_UNITS,
    };
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

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

        let manifest = write_release_evidence(&source, "aarch64", &output).unwrap();
        let cargo_tsv = fs::read_to_string(output.join("cargo-lock-packages.tsv")).unwrap();
        let rpm_command = fs::read_to_string(output.join("rpm-packages.command")).unwrap();
        let manifest_text = fs::read_to_string(manifest).unwrap();

        assert!(cargo_tsv.contains("name\tversion\tsource\tchecksum"));
        assert!(cargo_tsv.contains("alpha\t1.0.0"));
        assert!(rpm_command.contains("rpm -qa --qf"));
        assert!(rpm_command.contains("name\\tversion_release\\tarch\\tlicense"));
        assert!(manifest_text.contains("\"architecture\": \"aarch64\""));
        assert!(manifest_text.contains("\"cargo_package_count\": 1"));
        assert!(
            output.join("rpm-packages.tsv").is_file()
                || output.join("rpm-packages.not-generated.txt").is_file()
        );

        fs::remove_dir_all(&root).unwrap();
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
        use super::installed_state_dir_owner_check;

        // A staged DESTDIR has no etc/passwd, so goblins-os is unresolvable and
        // the ownership assertion resolves to ready/skip rather than failing.
        let root = std::env::temp_dir().join(format!(
            "goblins-os-verify-state-owner-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("var/lib/goblins-os/apps")).unwrap();

        assert_eq!(
            installed_state_dir_owner_check(&root, "var/lib/goblins-os/apps", "test").state,
            CheckState::Ready
        );

        // With a resolvable passwd entry, ownership is enforced; a mismatch blocks.
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::write(
            root.join("etc/passwd"),
            "root:x:0:0:root:/root:/bin/bash\ngoblins-os:x:991:991::/var/lib/goblins-os:/usr/sbin/nologin\n",
        )
        .unwrap();
        assert_eq!(
            installed_state_dir_owner_check(&root, "var/lib/goblins-os/apps", "test").state,
            CheckState::Blocked
        );

        let _ = fs::remove_dir_all(&root);
    }
}
