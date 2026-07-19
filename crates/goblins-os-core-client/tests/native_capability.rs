//! Native Linux proof for the installed desktop capability lifecycle.
//!
//! The normal test process never changes credentials. When explicitly opted in
//! as root, it creates one isolated production-shaped socket and invokes a copy
//! of this test binary through `setpriv`. The copied binary is setgid to the
//! socket capability group, so its first action exercises the real
//! `CoreClient::initialize` path before inspecting any test configuration.

use goblins_os_core_client::{initialize, ClientKind};

fn main() {
    // Keep this as the first executable statement. In helper mode the binary is
    // genuinely setgid, so no test argument, environment, or fixture state may
    // be parsed before the production initialization contract is consumed.
    let initialization = initialize(ClientKind::Today);

    #[cfg(target_os = "linux")]
    {
        if let Err(error) = linux::dispatch(initialization) {
            eprintln!("native_capability_test=fail detail={error}");
            std::process::exit(1);
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = initialization;
        eprintln!("native_capability_test=skip reason=linux-only");
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use goblins_os_core_client::{CoreClient, Error};
    use std::{
        env,
        ffi::CString,
        fs,
        io::{self, Read, Write},
        os::{
            fd::AsRawFd,
            unix::{
                ffi::OsStrExt,
                fs::PermissionsExt,
                net::{UnixListener, UnixStream},
                process::ExitStatusExt,
            },
        },
        path::{Path, PathBuf},
        process::Command,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    const OPT_IN_ENV: &str = "GOBLINS_CORE_CLIENT_NATIVE_TEST";
    const HELPER_ENV: &str = "GOBLINS_CORE_CLIENT_NATIVE_HELPER";
    const TEST_UID: libc::uid_t = 59_021;
    const REAL_GID: libc::gid_t = 59_022;
    const CAPABILITY_GID: libc::gid_t = 59_023;
    const CONTROL_ROOT: &str = "/run/goblins-os-core";
    const CLIENT_DIRECTORY: &str = "/run/goblins-os-core/today";
    const SOCKET_PATH: &str = "/run/goblins-os-core/today/control.sock";
    const PAYLOAD_PARENT: &str = "/usr/libexec/goblins-os";
    const PAYLOAD_DIRECTORY: &str = "/usr/libexec/goblins-os/ui";
    const PAYLOAD_PATH: &str = "/usr/libexec/goblins-os/ui/goblins-os-today";
    const SETPRIV: &str = "/usr/bin/setpriv";

    pub(super) fn dispatch(initialization: Result<CoreClient, Error>) -> Result<(), String> {
        match initialization {
            Ok(core) => run_helper(core),
            Err(error) if env::var_os(HELPER_ENV).is_some() => {
                let no_new_privs = prctl_get(libc::PR_GET_NO_NEW_PRIVS).unwrap_or(-1);
                if no_new_privs == 1 || executable_mount_is_nosuid().unwrap_or(false) {
                    eprintln!(
                        "native_capability_test=skip reason=setgid-unavailable error={error}"
                    );
                    std::process::exit(77);
                }
                Err(format!(
                    "setgid helper could not initialize the Today capability: {error}"
                ))
            }
            Err(Error::UnsupportedPlatform) => {
                eprintln!("native_capability_test=skip reason=linux-only");
                Ok(())
            }
            Err(error) => run_parent(error),
        }
    }

    fn run_parent(initialization_error: Error) -> Result<(), String> {
        if env::var_os(OPT_IN_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
            eprintln!("native_capability_test=skip reason=explicit-opt-in-required");
            return Ok(());
        }
        // SAFETY: geteuid has no preconditions and cannot fail.
        if unsafe { libc::geteuid() } != 0 {
            eprintln!("native_capability_test=skip reason=root-required");
            return Ok(());
        }
        if !matches!(initialization_error, Error::PrivilegeContract(_)) {
            return Err(format!(
                "root parent expected the desktop privilege-contract refusal, got {initialization_error}"
            ));
        }
        if !Path::new(SETPRIV).is_file() {
            eprintln!("native_capability_test=skip reason=setpriv-unavailable");
            return Ok(());
        }
        if Path::new(CONTROL_ROOT).exists() {
            eprintln!("native_capability_test=skip reason=live-control-root-present");
            return Ok(());
        }
        if Path::new(PAYLOAD_PATH).exists() || Path::new(PAYLOAD_DIRECTORY).exists() {
            eprintln!("native_capability_test=skip reason=live-payload-present");
            return Ok(());
        }

        let fixture = RootFixture::create().map_err(|error| error.to_string())?;
        if executable_mount_is_nosuid_for(&fixture.helper).map_err(|error| error.to_string())? {
            eprintln!("native_capability_test=skip reason=helper-filesystem-nosuid");
            return Ok(());
        }

        let listener = UnixListener::bind(SOCKET_PATH).map_err(|error| error.to_string())?;
        chown(Path::new(SOCKET_PATH), 0, CAPABILITY_GID).map_err(|error| error.to_string())?;
        fs::set_permissions(SOCKET_PATH, fs::Permissions::from_mode(0o660))
            .map_err(|error| error.to_string())?;

        let server = thread::spawn(move || serve_one_native_request(listener));
        let output = Command::new(SETPRIV)
            .arg(format!("--reuid={TEST_UID}"))
            .arg(format!("--regid={REAL_GID}"))
            .arg("--clear-groups")
            .arg("--")
            .arg(&fixture.helper)
            .env(HELPER_ENV, "1")
            .env(OPT_IN_ENV, "1")
            .output()
            .map_err(|error| format!("could not launch setgid helper: {error}"))?;

        if output.status.code() == Some(77) {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            drop(server);
            return Ok(());
        }
        if !output.status.success() {
            return Err(format!(
                "setgid helper failed status={:?} signal={:?} stdout={} stderr={}",
                output.status.code(),
                output.status.signal(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        server
            .join()
            .map_err(|_| "native socket server panicked".to_string())??;
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        eprintln!(
            "native_capability_test=pass uid={TEST_UID} real_gid={REAL_GID} capability_gid={CAPABILITY_GID}"
        );
        Ok(())
    }

    fn run_helper(core: CoreClient) -> Result<(), String> {
        if env::var_os(HELPER_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
            return Err("setgid helper marker is missing".to_string());
        }

        let (real, effective, saved) = process_gids().map_err(|error| error.to_string())?;
        if (real, effective, saved) != (REAL_GID, REAL_GID, REAL_GID) {
            return Err(format!(
                "desktop group was not permanently dropped: real={real} effective={effective} saved={saved}"
            ));
        }
        let groups = supplementary_groups().map_err(|error| error.to_string())?;
        if !groups.is_empty() {
            return Err(format!(
                "desktop helper retained supplementary groups: {groups:?}"
            ));
        }
        if prctl_get(libc::PR_GET_DUMPABLE).map_err(|error| error.to_string())? != 0 {
            return Err("desktop helper remained dumpable".to_string());
        }
        if prctl_get(libc::PR_GET_NO_NEW_PRIVS).map_err(|error| error.to_string())? != 0 {
            return Err("desktop helper unexpectedly enabled no-new-privileges".to_string());
        }
        // SAFETY: `getauxval` reads the immutable process auxiliary vector.
        if unsafe { libc::getauxval(libc::AT_SECURE) } != 0 {
            return Err("regular desktop payload retained AT_SECURE".to_string());
        }

        let descriptor = connected_control_descriptor().map_err(|error| error.to_string())?;
        // SAFETY: F_GETFD reads flags from the live descriptor discovered above.
        let descriptor_flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
        if descriptor_flags < 0 || descriptor_flags & libc::FD_CLOEXEC == 0 {
            return Err("desktop capability descriptor is not close-on-exec".to_string());
        }

        prove_fork_child_is_detached(&core, descriptor)?;
        let response = core
            .get("/v1/today/status", Duration::from_secs(2))
            .map_err(|error| format!("parent connection failed after fork: {error}"))?;
        if response.status != 200 || response.body != br#"{"source":"native-capability-test"}"# {
            return Err(format!(
                "unexpected native fixture response status={} body={}",
                response.status,
                String::from_utf8_lossy(&response.body)
            ));
        }
        Ok(())
    }

    fn prove_fork_child_is_detached(core: &CoreClient, descriptor: i32) -> Result<(), String> {
        let mut report_pipe = [-1; 2];
        // SAFETY: report_pipe points to two writable descriptors and O_CLOEXEC is valid.
        if unsafe { libc::pipe2(report_pipe.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }

        // SAFETY: the helper is single-threaded here. The registered atfork child
        // callback performs only lock-free atomics and close(2).
        let child = unsafe { libc::fork() };
        if child < 0 {
            close_fd(report_pipe[0]);
            close_fd(report_pipe[1]);
            return Err(io::Error::last_os_error().to_string());
        }
        if child == 0 {
            close_fd(report_pipe[0]);
            // SAFETY: F_GETFD only inspects the numeric descriptor.
            let closed = unsafe { libc::fcntl(descriptor, libc::F_GETFD) } == -1
                && io::Error::last_os_error().raw_os_error() == Some(libc::EBADF);
            let relaunch = matches!(
                core.get("/v1/today/status", Duration::from_secs(1)),
                Err(Error::RelaunchRequired)
            );
            let report = u8::from(closed) | (u8::from(relaunch) << 1);
            // SAFETY: report points to one readable byte and the pipe is live.
            let written = unsafe {
                libc::write(
                    report_pipe[1],
                    std::ptr::addr_of!(report).cast(),
                    std::mem::size_of_val(&report),
                )
            };
            close_fd(report_pipe[1]);
            // SAFETY: after fork, _exit avoids running inherited Rust destructors.
            unsafe { libc::_exit(if written == 1 && report == 0b11 { 0 } else { 1 }) };
        }

        close_fd(report_pipe[1]);
        let mut report = 0_u8;
        // SAFETY: report is one writable byte and the read-end belongs to the parent.
        let read = unsafe {
            libc::read(
                report_pipe[0],
                std::ptr::addr_of_mut!(report).cast(),
                std::mem::size_of_val(&report),
            )
        };
        close_fd(report_pipe[0]);

        let mut status = 0;
        let waited = loop {
            // SAFETY: child is the direct child PID and status is writable.
            let result = unsafe { libc::waitpid(child, &mut status, 0) };
            if result < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break result;
        };
        if read != 1
            || report != 0b11
            || waited != child
            || !libc::WIFEXITED(status)
            || libc::WEXITSTATUS(status) != 0
        {
            return Err(format!(
                "fork child retained capability state: read={read} report={report:#04b} waited={waited} status={status}"
            ));
        }
        Ok(())
    }

    fn connected_control_descriptor() -> io::Result<i32> {
        let mut matches = Vec::new();
        for entry in fs::read_dir("/proc/self/fd")? {
            let entry = entry?;
            let Ok(descriptor) = entry.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            if descriptor > libc::STDERR_FILENO && peer_path(descriptor) == Some(SOCKET_PATH) {
                matches.push(descriptor);
            }
        }
        match matches.as_slice() {
            [descriptor] => Ok(*descriptor),
            _ => Err(io::Error::other(format!(
                "expected one connected control descriptor, found {matches:?}"
            ))),
        }
    }

    fn peer_path(descriptor: i32) -> Option<&'static str> {
        let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
        // SAFETY: address and length form a valid sockaddr output buffer.
        if unsafe {
            libc::getpeername(
                descriptor,
                std::ptr::addr_of_mut!(address).cast(),
                &mut length,
            )
        } != 0
            || address.sun_family != libc::AF_UNIX as libc::sa_family_t
        {
            return None;
        }
        let offset = std::mem::offset_of!(libc::sockaddr_un, sun_path);
        let path_length = (length as usize).saturating_sub(offset);
        let bytes = unsafe {
            std::slice::from_raw_parts(address.sun_path.as_ptr().cast::<u8>(), path_length)
        };
        let path = bytes.split(|byte| *byte == 0).next()?;
        (path == SOCKET_PATH.as_bytes()).then_some(SOCKET_PATH)
    }

    fn serve_one_native_request(listener: UnixListener) -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let credentials = peer_credentials(&stream).map_err(|error| error.to_string())?;
        if credentials.uid != TEST_UID || credentials.gid != CAPABILITY_GID {
            return Err(format!(
                "connection did not consume the expected setgid capability: uid={} gid={}",
                credentials.uid, credentials.gid
            ));
        }
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| error.to_string())?;
        let request = read_request_head(&mut stream).map_err(|error| error.to_string())?;
        if !request.starts_with("GET /v1/today/status HTTP/1.1\r\n") {
            return Err(format!("unexpected native request: {request:?}"));
        }
        let body = br#"{"source":"native-capability-test"}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
            body.len()
        )
        .and_then(|()| stream.write_all(body))
        .map_err(|error| error.to_string())?;

        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(100));
        if !matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock) {
            return Err("helper opened more than one capability connection".to_string());
        }
        Ok(())
    }

    fn read_request_head(stream: &mut UnixStream) -> io::Result<String> {
        let mut bytes = Vec::new();
        let mut byte = [0_u8; 1];
        while bytes.len() < 16 * 1024 {
            stream.read_exact(&mut byte)?;
            bytes.push(byte[0]);
            if bytes.ends_with(b"\r\n\r\n") {
                return String::from_utf8(bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native request headers exceeded 16 KiB",
        ))
    }

    fn peer_credentials(stream: &UnixStream) -> io::Result<libc::ucred> {
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: credentials and length form a valid getsockopt output buffer.
        if unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                std::ptr::addr_of_mut!(credentials).cast(),
                &mut length,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if length as usize != std::mem::size_of::<libc::ucred>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SO_PEERCRED returned an unexpected size",
            ));
        }
        Ok(credentials)
    }

    fn process_gids() -> io::Result<(libc::gid_t, libc::gid_t, libc::gid_t)> {
        let mut real = 0;
        let mut effective = 0;
        let mut saved = 0;
        // SAFETY: all pointers reference writable gid_t values.
        if unsafe { libc::getresgid(&mut real, &mut effective, &mut saved) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((real, effective, saved))
    }

    fn supplementary_groups() -> io::Result<Vec<libc::gid_t>> {
        // SAFETY: a zero-sized query accepts a null output pointer.
        let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut groups = vec![0; count as usize];
        if count > 0 {
            // SAFETY: groups has capacity for count gid_t values.
            let written = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
            if written < 0 {
                return Err(io::Error::last_os_error());
            }
            groups.truncate(written as usize);
        }
        Ok(groups)
    }

    fn prctl_get(operation: libc::c_int) -> io::Result<libc::c_int> {
        // SAFETY: both getter operations used by this test take zero padding.
        let result = unsafe { libc::prctl(operation, 0, 0, 0, 0) };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }

    fn close_fd(descriptor: i32) {
        if descriptor >= 0 {
            // SAFETY: closing an owned pipe end or the inherited test descriptor is valid.
            unsafe {
                libc::close(descriptor);
            }
        }
    }

    fn chown(path: &Path, uid: libc::uid_t, gid: libc::gid_t) -> io::Result<()> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        // SAFETY: path is a valid NUL-terminated filesystem path.
        if unsafe { libc::chown(path.as_ptr(), uid, gid) } != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn executable_mount_is_nosuid() -> io::Result<bool> {
        executable_mount_is_nosuid_for(&env::current_exe()?)
    }

    fn executable_mount_is_nosuid_for(path: &Path) -> io::Result<bool> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        let mut filesystem: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: path and filesystem are valid statvfs input/output values.
        if unsafe { libc::statvfs(path.as_ptr(), &mut filesystem) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(filesystem.f_flag & libc::ST_NOSUID as libc::c_ulong != 0)
    }

    struct RootFixture {
        helper: PathBuf,
        helper_directory: PathBuf,
        helper_created: bool,
        helper_directory_created: bool,
        payload_created: bool,
        payload_directory_created: bool,
        payload_parent_created: bool,
        control_root_created: bool,
        client_directory_created: bool,
    }

    impl RootFixture {
        fn create() -> io::Result<Self> {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos();
            let helper_directory = env::temp_dir().join(format!(
                "goblins-core-client-native-{}-{nonce}",
                std::process::id()
            ));
            let helper = helper_directory.join("native-capability-helper");
            let mut fixture = Self {
                helper,
                helper_directory,
                helper_created: false,
                helper_directory_created: false,
                payload_created: false,
                payload_directory_created: false,
                payload_parent_created: false,
                control_root_created: false,
                client_directory_created: false,
            };

            fs::create_dir(&fixture.helper_directory)?;
            fixture.helper_directory_created = true;
            fs::set_permissions(&fixture.helper_directory, fs::Permissions::from_mode(0o755))?;
            fs::copy(env::current_exe()?, &fixture.helper)?;
            fixture.helper_created = true;
            chown(&fixture.helper, 0, CAPABILITY_GID)?;
            fs::set_permissions(&fixture.helper, fs::Permissions::from_mode(0o2755))?;

            if !Path::new(PAYLOAD_PARENT).exists() {
                fs::create_dir(PAYLOAD_PARENT)?;
                fixture.payload_parent_created = true;
                fs::set_permissions(PAYLOAD_PARENT, fs::Permissions::from_mode(0o755))?;
            }
            fs::create_dir(PAYLOAD_DIRECTORY)?;
            fixture.payload_directory_created = true;
            fs::set_permissions(PAYLOAD_DIRECTORY, fs::Permissions::from_mode(0o755))?;
            fs::copy(env::current_exe()?, PAYLOAD_PATH)?;
            fixture.payload_created = true;
            chown(Path::new(PAYLOAD_PATH), 0, 0)?;
            fs::set_permissions(PAYLOAD_PATH, fs::Permissions::from_mode(0o755))?;

            fs::create_dir(CONTROL_ROOT)?;
            fixture.control_root_created = true;
            chown(Path::new(CONTROL_ROOT), 0, 0)?;
            fs::set_permissions(CONTROL_ROOT, fs::Permissions::from_mode(0o755))?;
            fs::create_dir(CLIENT_DIRECTORY)?;
            fixture.client_directory_created = true;
            chown(Path::new(CLIENT_DIRECTORY), 0, CAPABILITY_GID)?;
            fs::set_permissions(CLIENT_DIRECTORY, fs::Permissions::from_mode(0o2750))?;

            Ok(fixture)
        }
    }

    impl Drop for RootFixture {
        fn drop(&mut self) {
            if self.control_root_created {
                match fs::remove_file(SOCKET_PATH) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => eprintln!("native_capability_cleanup=socket error={error}"),
                }
            }
            if self.client_directory_created {
                if let Err(error) = fs::remove_dir(CLIENT_DIRECTORY) {
                    eprintln!("native_capability_cleanup=client-directory error={error}");
                }
            }
            if self.control_root_created {
                if let Err(error) = fs::remove_dir(CONTROL_ROOT) {
                    eprintln!("native_capability_cleanup=control-root error={error}");
                }
            }
            if self.helper_created {
                if let Err(error) = fs::remove_file(&self.helper) {
                    eprintln!("native_capability_cleanup=helper error={error}");
                }
            }
            if self.payload_created {
                if let Err(error) = fs::remove_file(PAYLOAD_PATH) {
                    eprintln!("native_capability_cleanup=payload error={error}");
                }
            }
            if self.payload_directory_created {
                if let Err(error) = fs::remove_dir(PAYLOAD_DIRECTORY) {
                    eprintln!("native_capability_cleanup=payload-directory error={error}");
                }
            }
            if self.payload_parent_created {
                if let Err(error) = fs::remove_dir(PAYLOAD_PARENT) {
                    eprintln!("native_capability_cleanup=payload-parent error={error}");
                }
            }
            if self.helper_directory_created {
                if let Err(error) = fs::remove_dir(&self.helper_directory) {
                    eprintln!("native_capability_cleanup=helper-directory error={error}");
                }
            }
        }
    }
}
