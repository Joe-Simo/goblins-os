use std::{env, io::ErrorKind, process::Command};

#[derive(Clone, Copy)]
struct Launcher {
    key: &'static str,
    name: &'static str,
    app_id: &'static str,
    flathub_url: &'static str,
}

const LAUNCHERS: &[Launcher] = &[
    Launcher {
        key: "heroic",
        name: "Heroic Games Launcher",
        app_id: "com.heroicgameslauncher.hgl",
        flathub_url: "https://flathub.org/apps/com.heroicgameslauncher.hgl",
    },
    Launcher {
        key: "bottles",
        name: "Bottles",
        app_id: "com.usebottles.bottles",
        flathub_url: "https://flathub.org/apps/com.usebottles.bottles",
    },
    Launcher {
        key: "lutris",
        name: "Lutris",
        app_id: "net.lutris.Lutris",
        flathub_url: "https://flathub.org/apps/net.lutris.Lutris",
    },
];

fn main() {
    let Some(key) = env::args().nth(1) else {
        print_usage();
        std::process::exit(64);
    };

    let Some(launcher) = find_launcher(&key) else {
        eprintln!("Unknown non-Steam launcher: {key}");
        print_usage();
        std::process::exit(64);
    };

    if let Err(error) = open_install_path(launcher) {
        eprintln!("{error}");
        std::process::exit(69);
    }
}

fn print_usage() {
    eprintln!("Usage: goblins-os-install-game-launcher <heroic|bottles|lutris>");
}

fn find_launcher(key: &str) -> Option<Launcher> {
    LAUNCHERS
        .iter()
        .copied()
        .find(|launcher| launcher.key == key)
}

fn open_install_path(launcher: Launcher) -> Result<(), String> {
    for command in install_commands(launcher) {
        match Command::new(command.program).args(&command.args).spawn() {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Goblins OS could not open {} in the software installer: {}: {error}",
                    launcher.name,
                    command.display()
                ));
            }
        }
    }

    Err(format!(
        "Goblins OS could not open {}. Open {} manually to install it from Flathub.",
        launcher.name, launcher.flathub_url
    ))
}

fn install_commands(launcher: Launcher) -> Vec<InstallCommand> {
    vec![
        InstallCommand::new("gnome-software", &["--details", launcher.app_id]),
        InstallCommand::owned("xdg-open", vec![format!("appstream://{}", launcher.app_id)]),
        InstallCommand::new("xdg-open", &[launcher.flathub_url]),
    ]
}

struct InstallCommand {
    program: &'static str,
    args: Vec<String>,
}

impl InstallCommand {
    fn new(program: &'static str, args: &[&str]) -> Self {
        Self {
            program,
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
        }
    }

    fn owned(program: &'static str, args: Vec<String>) -> Self {
        Self { program, args }
    }

    fn display(&self) -> String {
        if self.args.is_empty() {
            self.program.to_string()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{find_launcher, install_commands, LAUNCHERS};

    #[test]
    fn maps_non_steam_launcher_ids_to_flathub_apps() {
        assert_eq!(
            find_launcher("heroic").expect("heroic").app_id,
            "com.heroicgameslauncher.hgl"
        );
        assert_eq!(
            find_launcher("bottles").expect("bottles").app_id,
            "com.usebottles.bottles"
        );
        assert_eq!(
            find_launcher("lutris").expect("lutris").app_id,
            "net.lutris.Lutris"
        );
    }

    #[test]
    fn opens_gnome_software_then_appstream_then_flathub() {
        let launcher = find_launcher("heroic").expect("heroic");
        let commands = install_commands(launcher);

        assert_eq!(
            commands[0].display(),
            "gnome-software --details com.heroicgameslauncher.hgl"
        );
        assert_eq!(
            commands[1].display(),
            "xdg-open appstream://com.heroicgameslauncher.hgl"
        );
        assert_eq!(
            commands[2].display(),
            "xdg-open https://flathub.org/apps/com.heroicgameslauncher.hgl"
        );
    }

    #[test]
    fn does_not_offer_steam_launcher() {
        assert!(find_launcher("steam").is_none());
        assert!(LAUNCHERS.iter().all(|launcher| {
            !launcher.key.contains("steam") && !launcher.app_id.contains("steam")
        }));
    }
}
