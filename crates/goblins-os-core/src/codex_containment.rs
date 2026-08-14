//! Outer Linux containment for non-interactive Codex turns.
//!
//! Codex keeps its own permission profile as the policy boundary for agent
//! tools. This layer is independent defense in depth: it gives each turn an
//! exact mount view and a private PID namespace whose lifetime is tied to the
//! direct `bwrap` child. Killing that child therefore also ends descendants
//! that ignored signals or kept output pipes open.

use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const SANDBOX_WORKSPACE: &str = "/workspace";
pub(crate) const SANDBOX_CODEX_HOME: &str = "/codex-home";
pub(crate) const SANDBOX_RESULT: &str = "/run/goblins-codex/result.txt";

const BWRAP_BINARY: &str = "/usr/bin/bwrap";
const SANDBOX_CODEX_BINARY: &str = "/run/goblins-codex/codex";
const CODEX_CHILD_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const CODEX_CHILD_ENV_ALLOWLIST: &[&str] = &[
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// A Bubblewrap command plus every descriptor named by `--bind-fd`.
///
/// The files stay `CLOEXEC` in the multithreaded core until `pre_exec`, where
/// the forked child alone clears that bit. This avoids a window in which an
/// unrelated concurrent subprocess could inherit the Codex credential home or
/// a Studio workspace descriptor.
pub(crate) struct ContainedCodexCommand {
    command: Command,
    inherited_files: Vec<File>,
}

impl ContainedCodexCommand {
    pub(crate) fn command_mut(&mut self) -> &mut Command {
        debug_assert!(self.inherited_files.len() >= 5);
        &mut self.command
    }
}

/// Construct the outer containment boundary for one `codex exec` turn.
///
/// `workspace`, `codex_home`, and `result` must already be opened and
/// validated by the caller. Bubblewrap binds those exact inodes rather than
/// resolving attacker-swappable host paths. The network namespace is
/// deliberately shared because this is the hosted OpenAI route; no other host
/// runtime namespace or Goblins OS private state is mounted.
pub(crate) fn contained_codex_command(
    codex_binary: &str,
    workspace: File,
    codex_home: File,
    result: Option<File>,
) -> io::Result<ContainedCodexCommand> {
    require_directory(&workspace, "Codex workspace")?;
    require_private_directory(&codex_home, "Codex home")?;
    if let Some(result) = result.as_ref() {
        require_private_writable_file(result, "Codex result")?;
    }

    let executable_path = resolve_executable(codex_binary)?;
    let executable = File::open(&executable_path)?;
    require_executable(&executable)?;
    let resolver = File::open("/etc/resolv.conf")?;
    require_regular_file(&resolver, "resolver configuration")?;
    let hosts = File::open("/etc/hosts")?;
    require_regular_file(&hosts, "hosts configuration")?;
    let nsswitch = match File::open("/etc/nsswitch.conf") {
        Ok(file) => {
            require_regular_file(&file, "name-service configuration")?;
            Some(file)
        }
        Err(_) if !cfg!(target_os = "linux") => None,
        Err(error) => return Err(error),
    };

    let workspace_fd = workspace.as_raw_fd();
    let codex_home_fd = codex_home.as_raw_fd();
    let executable_fd = executable.as_raw_fd();
    let resolver_fd = resolver.as_raw_fd();
    let hosts_fd = hosts.as_raw_fd();
    let nsswitch_fd = nsswitch.as_ref().map(AsRawFd::as_raw_fd);
    let result_fd = result.as_ref().map(AsRawFd::as_raw_fd);

    let mut command = Command::new(BWRAP_BINARY);
    command.env_clear();
    command.args([
        "--ro-bind",
        "/usr",
        "/usr",
        "--symlink",
        "usr/bin",
        "/bin",
        "--symlink",
        "usr/sbin",
        "/sbin",
        "--symlink",
        "usr/lib",
        "/lib",
        "--symlink",
        "usr/lib64",
        "/lib64",
        // Public certificate trust only. Do not mount `/etc` or its private,
        // host-identifying account/machine/network configuration.
        "--ro-bind-try",
        "/etc/pki/ca-trust",
        "/etc/pki/ca-trust",
        "--ro-bind-try",
        "/etc/pki/tls/certs",
        "/etc/pki/tls/certs",
        "--ro-bind-try",
        "/etc/pki/tls/openssl.cnf",
        "/etc/pki/tls/openssl.cnf",
        "--ro-bind-try",
        "/etc/ssl/certs",
        "/etc/ssl/certs",
        "--ro-bind-try",
        "/etc/ssl/openssl.cnf",
        "/etc/ssl/openssl.cnf",
    ]);
    command
        .arg("--ro-bind-fd")
        .arg(resolver_fd.to_string())
        .arg("/etc/resolv.conf")
        .arg("--ro-bind-fd")
        .arg(hosts_fd.to_string())
        .arg("/etc/hosts");
    if let Some(nsswitch_fd) = nsswitch_fd {
        command
            .arg("--ro-bind-fd")
            .arg(nsswitch_fd.to_string())
            .arg("/etc/nsswitch.conf");
    }
    command
        .args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"])
        .arg("--bind-fd")
        .arg(workspace_fd.to_string())
        .arg(SANDBOX_WORKSPACE)
        .arg("--bind-fd")
        .arg(codex_home_fd.to_string())
        .arg(SANDBOX_CODEX_HOME)
        .arg("--ro-bind-fd")
        .arg(executable_fd.to_string())
        .arg(SANDBOX_CODEX_BINARY);
    if let Some(result_fd) = result_fd {
        command
            .arg("--bind-fd")
            .arg(result_fd.to_string())
            .arg(SANDBOX_RESULT);
    }
    command.args([
        "--remount-ro",
        "/",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--unshare-cgroup-try",
        "--die-with-parent",
        "--new-session",
        "--clearenv",
        "--setenv",
        "PATH",
        CODEX_CHILD_PATH,
        "--setenv",
        "HOME",
        SANDBOX_CODEX_HOME,
        "--setenv",
        "CODEX_HOME",
        SANDBOX_CODEX_HOME,
        "--setenv",
        "LANG",
        "C.UTF-8",
    ]);
    // Do not add `--disable-userns` or `--assert-userns-disabled`: current
    // Codex uses a nested Bubblewrap user namespace to enforce its own inner
    // permission profile on Linux. That nested namespace inherits this
    // already-minimal mount/PID view and cannot recover hidden host paths.
    for name in CODEX_CHILD_ENV_ALLOWLIST {
        if let Some(value) = env::var_os(name) {
            command.arg("--setenv").arg(name).arg(value);
        }
    }
    command
        .arg("--chdir")
        .arg(SANDBOX_WORKSPACE)
        .arg("--")
        .arg(SANDBOX_CODEX_BINARY);

    let mut inherited_files = vec![workspace, codex_home, executable, resolver, hosts];
    if let Some(nsswitch) = nsswitch {
        inherited_files.push(nsswitch);
    }
    if let Some(result) = result {
        inherited_files.push(result);
    }
    let inherited_fds = inherited_files
        .iter()
        .map(AsRawFd::as_raw_fd)
        .collect::<Vec<_>>();
    // SAFETY: the closure performs only direct `prctl`/`fcntl` system calls,
    // allocates nothing, and touches only descriptors owned by
    // `ContainedCodexCommand`, which remains alive through `spawn` and the
    // bounded wait.
    unsafe {
        command.pre_exec(move || {
            #[cfg(target_os = "linux")]
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            for fd in &inherited_fds {
                let flags = libc::fcntl(*fd, libc::F_GETFD);
                if flags < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::fcntl(*fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }

    Ok(ContainedCodexCommand {
        command,
        inherited_files,
    })
}

fn resolve_executable(binary: &str) -> io::Result<PathBuf> {
    let binary = Path::new(binary);
    let candidate = if binary.components().count() > 1 {
        binary.to_path_buf()
    } else {
        env::split_paths(OsStr::new(CODEX_CHILD_PATH))
            .map(|directory| directory.join(binary))
            .find(|candidate| executable_metadata(candidate).is_some())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Codex binary is missing"))?
    };
    let canonical = fs::canonicalize(candidate)?;
    if executable_metadata(&canonical).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Codex binary is not an executable regular file",
        ));
    }
    Ok(canonical)
}

fn executable_metadata(path: &Path) -> Option<fs::Metadata> {
    let metadata = fs::metadata(path).ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(metadata)
}

fn require_directory(file: &File, label: &str) -> io::Result<()> {
    if file.metadata()?.is_dir() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} is not a directory"),
        ))
    }
}

fn require_regular_file(file: &File, label: &str) -> io::Result<()> {
    if file.metadata()?.is_file() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} is not a regular file"),
        ))
    }
}

fn require_private_directory(file: &File, label: &str) -> io::Result<()> {
    let metadata = file.metadata()?;
    // SAFETY: `geteuid` has no preconditions and does not dereference pointers.
    let effective_uid = unsafe { libc::geteuid() };
    if metadata.is_dir()
        && metadata.permissions().mode() & 0o7777 == 0o700
        && metadata.uid() == effective_uid
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{label} is not an owner-only service directory"),
        ))
    }
}

fn require_private_writable_file(file: &File, label: &str) -> io::Result<()> {
    let metadata = file.metadata()?;
    // SAFETY: `geteuid` has no preconditions and does not dereference pointers.
    let effective_uid = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.uid() != effective_uid
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{label} is not a private regular file"),
        ));
    }
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if flags & libc::O_ACCMODE == libc::O_RDONLY {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{label} is not writable"),
        ));
    }
    Ok(())
}

fn require_executable(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if metadata.is_file()
        && metadata.permissions().mode() & 0o111 != 0
        && metadata.permissions().mode() & 0o022 == 0
        && metadata.uid() == 0
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Codex binary is not an immutable root-owned executable",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        contained_codex_command, BWRAP_BINARY, SANDBOX_CODEX_BINARY, SANDBOX_CODEX_HOME,
        SANDBOX_RESULT, SANDBOX_WORKSPACE,
    };
    use crate::bounded::{bounded_output_of, BoundedCommandError};
    use std::{
        ffi::{OsStr, OsString},
        fs::{self, File, OpenOptions},
        os::fd::AsRawFd,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
        path::Path,
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    fn fixture() -> (tempfile::TempDir, File, File, File) {
        let root = tempfile::tempdir().expect("containment fixture");
        let workspace = root.path().join("workspace");
        let home = root.path().join("codex-home");
        fs::create_dir(&workspace).expect("workspace fixture");
        fs::create_dir(&home).expect("home fixture");
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))
            .expect("private home fixture");
        fs::write(home.join("config.toml"), "fixture").expect("home canary");
        let result_path = root.path().join("result.txt");
        let result = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(result_path)
            .expect("result fixture");
        (
            root,
            File::open(workspace).expect("open workspace"),
            File::open(home).expect("open home"),
            result,
        )
    }

    fn command_arguments(command: &Command) -> Vec<OsString> {
        command.get_args().map(OsString::from).collect()
    }

    fn has_sequence(arguments: &[OsString], expected: &[&str]) -> bool {
        arguments.windows(expected.len()).any(|window| {
            window
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual == OsStr::new(expected))
        })
    }

    #[test]
    fn command_has_exact_mount_and_lifetime_contract() {
        let (_root, workspace, home, result) = fixture();
        let mut contained = contained_codex_command("/bin/sh", workspace, home, Some(result))
            .expect("construct containment");
        let command = contained.command_mut();
        assert_eq!(command.get_program(), OsStr::new(BWRAP_BINARY));
        let arguments = command_arguments(command);

        for expected in [
            ["--ro-bind", "/usr", "/usr"].as_slice(),
            ["--ro-bind-try", "/etc/pki/ca-trust", "/etc/pki/ca-trust"].as_slice(),
            ["--ro-bind-fd"].as_slice(),
            ["--proc", "/proc"].as_slice(),
            ["--dev", "/dev"].as_slice(),
            ["--tmpfs", "/tmp"].as_slice(),
            ["--remount-ro", "/"].as_slice(),
            ["--setenv", "HOME", SANDBOX_CODEX_HOME].as_slice(),
            ["--setenv", "CODEX_HOME", SANDBOX_CODEX_HOME].as_slice(),
            ["--chdir", SANDBOX_WORKSPACE].as_slice(),
            ["--", SANDBOX_CODEX_BINARY].as_slice(),
        ] {
            assert!(has_sequence(&arguments, expected), "missing {expected:?}");
        }
        for destination in [
            SANDBOX_WORKSPACE,
            SANDBOX_CODEX_HOME,
            SANDBOX_RESULT,
            SANDBOX_CODEX_BINARY,
        ] {
            assert!(arguments.iter().any(|argument| argument == destination));
        }
        for flag in [
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--unshare-cgroup-try",
            "--die-with-parent",
            "--new-session",
        ] {
            assert!(arguments.iter().any(|argument| argument == flag));
        }
        assert!(!arguments.iter().any(|argument| argument == "--unshare-net"));
        assert!(!arguments.iter().any(|argument| argument == "--unshare-all"));
        assert!(!arguments
            .iter()
            .any(|argument| argument == "--disable-userns"));
        assert!(!arguments
            .iter()
            .any(|argument| argument == "--assert-userns-disabled"));
        assert!(!has_sequence(&arguments, &["--ro-bind", "/etc", "/etc"]));
        for hidden in [
            "/etc/goblins-os",
            "/etc/group",
            "/etc/machine-id",
            "/etc/passwd",
            "/run/credentials",
            "/var/lib/goblins-os",
        ] {
            assert!(!arguments.iter().any(|argument| argument == hidden));
        }
        assert!(!arguments
            .iter()
            .any(|argument| argument.to_string_lossy().contains("goblins-os/apps")));
    }

    #[test]
    fn command_rejects_nonprivate_home_and_result_handles() {
        let (root, workspace, home, result) = fixture();
        fs::set_permissions(
            root.path().join("codex-home"),
            fs::Permissions::from_mode(0o750),
        )
        .expect("nonprivate home mode");
        assert!(contained_codex_command("/bin/sh", workspace, home, Some(result)).is_err());

        let (root, workspace, home, result) = fixture();
        fs::set_permissions(
            root.path().join("result.txt"),
            fs::Permissions::from_mode(0o640),
        )
        .expect("nonprivate result mode");
        assert!(contained_codex_command("/bin/sh", workspace, home, Some(result)).is_err());
    }

    #[test]
    fn command_rejects_service_writable_executable() {
        let root = tempfile::tempdir().expect("containment fixture");
        let executable = root.path().join("codex");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("executable fixture");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o775))
            .expect("executable mode");
        let (_fixture, workspace, home, result) = fixture();
        assert!(contained_codex_command(
            executable.to_str().expect("UTF-8 fixture path"),
            workspace,
            home,
            Some(result),
        )
        .is_err());
    }

    fn fedora_bubblewrap_available() -> bool {
        cfg!(target_os = "linux")
            && Path::new("/etc/fedora-release").is_file()
            && Path::new(BWRAP_BINARY).is_file()
    }

    #[test]
    fn fedora_runtime_hides_private_state_closes_bind_fds_and_preserves_hosted_network() {
        if !fedora_bubblewrap_available() {
            eprintln!("codex_containment_test=skip reason=fedora-bubblewrap-unavailable");
            return;
        }
        let (root, workspace, home, result) = fixture();
        fs::write(
            root.path().join("workspace/outer-net"),
            fs::read_link("/proc/self/ns/net")
                .expect("host network namespace")
                .to_string_lossy()
                .as_bytes(),
        )
        .expect("network namespace canary");
        let mut contained = contained_codex_command("/bin/sh", workspace, home, Some(result))
            .expect("construct containment");
        let closed_bind_descriptors = contained
            .inherited_files
            .iter()
            .map(|file| format!("test ! -e /proc/self/fd/{}", file.as_raw_fd()))
            .collect::<Vec<_>>()
            .join("; ");
        let probe = format!(
            "test -r /codex-home/config.toml; test -r /etc/resolv.conf; test -r /etc/hosts; test -r /etc/nsswitch.conf; test ! -e /etc/machine-id; test ! -e /etc/passwd; test ! -e /etc/group; test ! -e /etc/goblins-os; test ! -e /var/lib/goblins-os; test ! -e /run/credentials; grep -Eq '^NoNewPrivs:[[:space:]]+1$' /proc/self/status; ! touch /root-mutation 2>/dev/null; test \"$(cat outer-net)\" = \"$(readlink /proc/self/ns/net)\"; getent ahosts api.openai.com >/dev/null; test -r /etc/pki/tls/certs/ca-bundle.crt || test -r /etc/ssl/certs/ca-certificates.crt; curl --silent --show-error --output /dev/null --max-time 15 https://api.openai.com/; {closed_bind_descriptors}; bwrap --ro-bind /usr /usr --symlink usr/bin /bin --symlink usr/lib /lib --symlink usr/lib64 /lib64 --proc /proc --dev /dev -- /bin/true; printf built > built.txt; printf exact > /run/goblins-codex/result.txt"
        );
        contained.command_mut().args(["-ec", &probe]);
        let output = bounded_output_of(contained.command_mut(), Duration::from_secs(30))
            .unwrap_or_else(|_| panic!("contained shell must run"));
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(root.path().join("workspace/built.txt")).expect("workspace output"),
            "built"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("result.txt")).expect("exact result output"),
            "exact"
        );
    }

    #[test]
    fn timeout_ends_descendants_without_pipe_or_post_turn_mutation() {
        if !fedora_bubblewrap_available() {
            eprintln!("codex_containment_test=skip reason=fedora-bubblewrap-unavailable");
            return;
        }
        let (root, workspace, home, _result) = fixture();
        let mut contained = contained_codex_command("/bin/sh", workspace, home, None)
            .expect("construct containment");
        contained.command_mut().args([
            "-ec",
            "trap '' HUP TERM; (trap '' HUP TERM; sleep 1; printf escaped > /workspace/escaped) & wait",
        ]);
        let started = Instant::now();
        let outcome = bounded_output_of(contained.command_mut(), Duration::from_millis(200));
        assert!(matches!(outcome, Err(BoundedCommandError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(1_200));
        assert!(!root.path().join("workspace/escaped").exists());
    }
}
