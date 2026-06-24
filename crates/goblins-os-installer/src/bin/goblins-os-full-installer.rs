use std::{env, io::ErrorKind, path::Path, process::Command};

fn main() {
    if let Err(error) = launch_full_installer() {
        eprintln!("{error}");
        std::process::exit(69);
    }
}

fn launch_full_installer() -> Result<(), String> {
    let mut candidates = Vec::new();
    if let Ok(command) = env::var("GOBLINS_OS_FULL_INSTALLER_COMMAND") {
        if let Some(candidate) = InstallerCommand::parse(&command) {
            candidates.push(candidate);
        }
    }
    candidates.extend([
        InstallerCommand::new("liveinst"),
        InstallerCommand::new("/usr/bin/liveinst"),
        InstallerCommand::with_args("anaconda", &["--liveinst"]),
        InstallerCommand::with_args("/usr/bin/anaconda", &["--liveinst"]),
        InstallerCommand::new("/usr/libexec/anaconda/run-anaconda"),
    ]);

    let mut failures = Vec::new();
    for candidate in candidates {
        if candidate.program.starts_with('/') && !Path::new(&candidate.program).exists() {
            continue;
        }

        match Command::new(&candidate.program)
            .args(&candidate.args)
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => failures.push(format!("{}: {error}", candidate.display())),
        }
    }

    if failures.is_empty() {
        Err("Goblins OS could not find advanced storage. Simple install remains disabled for dual boot, manual storage, scan-unknown disks, encryption, or a non-blank dedicated disk; reboot from Goblins OS install media and choose Install Goblins OS Beside Another OS.".to_string())
    } else {
        Err(format!(
            "Goblins OS could not start advanced storage: {}. No disk was changed; simple install remains disabled for dual boot, manual storage, and scan-unknown disks.",
            failures.join("; ")
        ))
    }
}

#[derive(Clone)]
struct InstallerCommand {
    program: String,
    args: Vec<String>,
}

impl InstallerCommand {
    fn new(program: &str) -> Self {
        Self {
            program: program.to_string(),
            args: Vec::new(),
        }
    }

    fn with_args(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
        }
    }

    fn parse(command: &str) -> Option<Self> {
        let mut parts = command.split_whitespace();
        let program = parts.next()?.to_string();
        let args = parts.map(str::to_string).collect();
        Some(Self { program, args })
    }

    fn display(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InstallerCommand;

    #[test]
    fn parses_override_without_shell_expansion() {
        let command =
            InstallerCommand::parse("/usr/bin/anaconda --liveinst --kickstart /tmp/goblins.ks")
                .expect("override should parse");

        assert_eq!(command.program, "/usr/bin/anaconda");
        assert_eq!(
            command.args,
            vec![
                "--liveinst".to_string(),
                "--kickstart".to_string(),
                "/tmp/goblins.ks".to_string()
            ]
        );
    }

    #[test]
    fn ignores_empty_override() {
        assert!(InstallerCommand::parse("   ").is_none());
    }

    #[test]
    fn displays_command_for_error_messages() {
        let command = InstallerCommand::with_args("anaconda", &["--liveinst"]);

        assert_eq!(command.display(), "anaconda --liveinst");
    }
}
