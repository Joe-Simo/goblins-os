use std::{
    env,
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
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
    "goblins-os-settings",
    "goblins-os-shell",
    "goblins-os-verify",
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
];

const AUTOSTART: &[&str] = &["org.goblins.OS.Installer.desktop"];

const DCONF_FILES: &[&str] = &["00-goblins-os-first-run", "10-goblins-os-desktop"];

const GNOME_SHELL_EXTENSION_FILES: &[&str] = &[
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
        root.join("os/etc/goblins-os/environment"),
        "environment-primary-core-url-is-goblins-native",
        "GOBLINS_OS_CORE_URL=http://127.0.0.1:8787",
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
    checks.push(session_contains_check(
        root,
        "ibus-disabled-for-native-session",
        "GTK_IM_MODULE",
    ));
    checks.push(contains_check(
        root.join("os/dconf/db/local.d/10-goblins-os-desktop"),
        "super-space-launcher",
        "binding='<Super>space'",
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
        root.join("os/bootc/render.suffix.Dockerfile"),
        "render-settings-scope-build-arg",
        "ARG GOBLINS_OS_RENDER_SCOPE=all",
    ));
    checks.push(contains_check(
        root.join("os/bootc/render.suffix.Dockerfile"),
        "render-settings-scope-build-env",
        "GOBLINS_OS_RENDER_SCOPE=\"$GOBLINS_OS_RENDER_SCOPE\"",
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
        "render-wm-hud",
        "56-wm-hud-$suffix.png",
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
    checks.push(file_check(root, "usr/bin/pw-cli"));
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
    let mut hits = Vec::new();
    scan_source_for_secrets(root, root, &mut hits);
    if hits.is_empty() {
        ready(
            "source-secret-scan",
            "no live OpenAI-style keys or active secret assignments found in source package",
        )
    } else {
        blocked(
            "source-secret-scan",
            &format!("possible live secrets found: {}", hits.join(", ")),
        )
    }
}

fn scan_source_for_secrets(root: &Path, dir: &Path, hits: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if should_skip_secret_scan_path(relative) {
            continue;
        }
        if path.is_dir() {
            scan_source_for_secrets(root, &path, hits);
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            if is_suspicious_secret_line(line) {
                hits.push(format!("{}:{}", relative.display(), index + 1));
            }
        }
    }
}

fn should_skip_secret_scan_path(relative: &Path) -> bool {
    let path = relative.to_string_lossy();
    path == ".git"
        || path.starts_with(".git/")
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
            "docker build -f os/bootc/Containerfile",
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
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-accessibility-accommodation-rows",
            "Typing assistance",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-keyboard-input-sources-list",
            "Input sources",
        ),
        contains_check(
            root.join("crates/goblins-os-settings/src/main.rs"),
            "settings-security-firewall-status-row",
            "/v1/firewall/status",
        ),
        contains_check(
            root.join("crates/goblins-os-markup/src/main.rs"),
            "markup-copy-text-ocr-handoff",
            "/v1/ocr/recognize",
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

fn read_to_string(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap_or_default()
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
        source_manifest_classifies_top_level, stable_id, write_release_evidence, CheckState,
        APPLICATIONS, AUTOSTART, BINARIES, DCONF_FILES, GNOME_SHELL_EXTENSION_FILES,
        ICON_THEME_FILES, NATIVE_DESIGN_APPS, NAUTILUS_SCRIPTS, SETTINGS_INTERACTION_SCREENSHOTS,
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
    fn settings_interaction_contract_covers_search_paths() {
        let unique = SETTINGS_INTERACTION_SCREENSHOTS
            .iter()
            .copied()
            .collect::<HashSet<_>>();

        assert_eq!(SETTINGS_INTERACTION_SCREENSHOTS.len(), 4);
        assert_eq!(unique.len(), SETTINGS_INTERACTION_SCREENSHOTS.len());
        for screenshot in [
            "100-settings-search-wifi-filter.png",
            "101-settings-search-enter-network.png",
            "102-settings-search-no-results.png",
            "103-settings-search-cleared.png",
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
        // empty secret template, dconf defaults, GDM autologin config,
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
