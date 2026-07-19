//! Bounded external-command execution for every core probe and control call.
//!
//! A core route handler must never hold an async runtime worker hostage: an
//! external command that outlives its bound is killed and reported as a
//! timeout so the calling surface degrades honestly instead of wedging the
//! whole daemon. Two blocked workers are enough to make every `/v1` route
//! unreachable on small machines, which is exactly what the hardware gate
//! observed before every raw `Command` call was routed through here.

use std::{
    env,
    io::Read,
    process::{Command, Output, Stdio},
    sync::{Arc, OnceLock},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

pub(crate) const PROBE_TIMEOUT_MS_DEFAULT: u64 = 4_000;
pub(crate) const PROBE_TIMEOUT_MS_MIN: u64 = 250;
pub(crate) const PROBE_TIMEOUT_MS_MAX: u64 = 120_000;

const CHILD_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const BASE_ENV_ALLOWLIST: &[&str] = &["LANG", "LC_ALL", "LC_CTYPE"];
const SESSION_ENV_ALLOWLIST: &[&str] = &[
    "DBUS_SESSION_BUS_ADDRESS",
    "DISPLAY",
    "PIPEWIRE_REMOTE",
    "PULSE_SERVER",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
];

/// Keep genuinely long model, OCR, and voice bodies from multiplying across
/// the blocking pool. Two operations are useful on ordinary hardware (for
/// example, OCR alongside a model turn), while a single-core target stays at
/// one. Admission is fail-fast: excess requests never become an unbounded
/// in-memory queue of 120-600 second jobs.
const MAX_CONCURRENT_LONG_OPERATIONS: usize = 2;

pub(crate) const LONG_OPERATION_BUSY_MESSAGE: &str =
    "Goblins OS is finishing other AI or media work. Try again in a moment.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionError {
    Busy,
}

pub(crate) enum BoundedCommandError {
    Missing,
    TimedOut,
    Failed,
}

/// Default bound for read-only status probes. Operators can widen or narrow it
/// with `GOBLINS_OS_COMMAND_TIMEOUT_MS` for slow or embedded hardware.
pub(crate) fn probe_timeout() -> Duration {
    Duration::from_millis(clamp_probe_timeout_ms(
        std::env::var("GOBLINS_OS_COMMAND_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok()),
    ))
}

pub(crate) fn clamp_probe_timeout_ms(parsed: Option<u64>) -> u64 {
    parsed
        .unwrap_or(PROBE_TIMEOUT_MS_DEFAULT)
        .clamp(PROBE_TIMEOUT_MS_MIN, PROBE_TIMEOUT_MS_MAX)
}

/// Run a genuinely long blocking body (a `codex exec` turn, OCR, a voice
/// pipeline, a model turn through the resident relay) on tokio's dedicated
/// blocking pool so it cannot pin one of the few async runtime workers for
/// minutes on small machines. A panic inside the body is resumed on the
/// calling task, so axum's panic-to-500 behavior is unchanged. Fast status
/// probes stay inline — this is only for the minutes-long operations.
pub(crate) async fn run_blocking<T, F>(task: F) -> Result<T, AdmissionError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    run_blocking_with_limiter(long_operation_limiter(), task).await
}

fn long_operation_limiter() -> Arc<Semaphore> {
    static LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(LIMITER.get_or_init(|| {
        let capacity = std::thread::available_parallelism()
            .map(|parallelism| parallelism.get().min(MAX_CONCURRENT_LONG_OPERATIONS))
            .unwrap_or(1);
        Arc::new(Semaphore::new(capacity))
    }))
}

async fn run_blocking_with_limiter<T, F>(
    limiter: Arc<Semaphore>,
    task: F,
) -> Result<T, AdmissionError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let permit = limiter
        .try_acquire_owned()
        .map_err(|_| AdmissionError::Busy)?;
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        task()
    })
    .await
    {
        Ok(value) => Ok(value),
        Err(error) => std::panic::resume_unwind(error.into_panic()),
    }
}

pub(crate) fn bounded_command_output(
    binary: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, BoundedCommandError> {
    let mut command = isolated_command(binary);
    command.args(args);
    bounded_output_of(&mut command, timeout)
}

/// Run a command that must connect to the active desktop/audio session. This is
/// a narrow opt-in: it receives only the base environment plus known non-secret
/// session addresses, never the core's OpenAI/OAuth credential material.
pub(crate) fn bounded_session_command_output(
    binary: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, BoundedCommandError> {
    let mut command = isolated_session_command(binary);
    command.args(args);
    bounded_output_of(&mut command, timeout)
}

/// Construct a child with a closed environment suitable for ordinary probes
/// and control tools such as nmcli, bluetoothctl, firewall-cmd, and bootc.
pub(crate) fn isolated_command(binary: &str) -> Command {
    isolated_command_with(binary, &[])
}

/// Construct a child with the minimal extra environment required to reach the
/// active D-Bus, display, PipeWire, or PulseAudio session.
pub(crate) fn isolated_session_command(binary: &str) -> Command {
    isolated_command_with(binary, SESSION_ENV_ALLOWLIST)
}

fn isolated_command_with(binary: &str, extra_allowlist: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command.env_clear();
    command.env("PATH", CHILD_PATH);
    for name in BASE_ENV_ALLOWLIST.iter().chain(extra_allowlist) {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

/// Same bound for a pre-configured `Command` (environment, working directory).
///
/// Both pipes are drained on background threads while the child runs: a
/// kernel pipe buffer holds ~64KB, so polling `try_wait` without reading
/// would block any chattier child on its own write and falsely kill it at
/// the bound. Captured output is capped so a runaway child cannot exhaust
/// memory inside its window; the overflow is drained and discarded.
pub(crate) fn bounded_output_of(
    command: &mut Command,
    timeout: Duration,
) -> Result<Output, BoundedCommandError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BoundedCommandError::Missing
            } else {
                BoundedCommandError::Failed
            }
        })?;
    let stdout_reader = spawn_capped_drain(child.stdout.take());
    let stderr_reader = spawn_capped_drain(child.stderr.take());
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Do NOT join the drain threads here: a killed child may
                    // leave a grandchild holding the pipe write end open, and
                    // joining would block this thread until that grandchild
                    // exits. The detached drains cost at most the output cap
                    // each and end when the pipes finally close.
                    drop(stdout_reader);
                    drop(stderr_reader);
                    return Err(BoundedCommandError::TimedOut);
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                drop(stdout_reader);
                drop(stderr_reader);
                return Err(BoundedCommandError::Failed);
            }
        }
    };

    Ok(Output {
        status,
        stdout: stdout_reader.join().unwrap_or_default(),
        stderr: stderr_reader.join().unwrap_or_default(),
    })
}

const CAPTURED_OUTPUT_CAP_BYTES: u64 = 8 * 1024 * 1024;

fn spawn_capped_drain<R>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>>
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        let Some(pipe) = pipe else {
            return Vec::new();
        };
        let mut limited = pipe.take(CAPTURED_OUTPUT_CAP_BYTES);
        let mut captured = Vec::new();
        let _ = limited.read_to_end(&mut captured);
        let _ = std::io::copy(&mut limited.into_inner(), &mut std::io::sink());
        captured
    })
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_command_output, bounded_session_command_output, clamp_probe_timeout_ms,
        run_blocking_with_limiter, AdmissionError, BoundedCommandError, PROBE_TIMEOUT_MS_DEFAULT,
        PROBE_TIMEOUT_MS_MAX, PROBE_TIMEOUT_MS_MIN,
    };
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::Semaphore;

    #[test]
    fn clamps_probe_timeout_into_safe_bounds() {
        assert_eq!(clamp_probe_timeout_ms(None), PROBE_TIMEOUT_MS_DEFAULT);
        assert_eq!(clamp_probe_timeout_ms(Some(0)), PROBE_TIMEOUT_MS_MIN);
        assert_eq!(clamp_probe_timeout_ms(Some(u64::MAX)), PROBE_TIMEOUT_MS_MAX);
        assert_eq!(clamp_probe_timeout_ms(Some(2_000)), 2_000);
    }

    #[test]
    fn missing_binary_reports_missing() {
        assert!(matches!(
            bounded_command_output(
                "goblins-os-test-binary-that-does-not-exist",
                &[],
                Duration::from_millis(500),
            ),
            Err(BoundedCommandError::Missing)
        ));
    }

    #[test]
    fn chatty_child_is_drained_not_killed() {
        let output = bounded_command_output(
            "sh",
            &["-c", "yes goblins | head -c 200000"],
            Duration::from_secs(10),
        )
        .unwrap_or_else(|_| panic!("chatty child must not be killed at the bound"));
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 200_000);
    }

    #[test]
    fn hung_child_is_killed_at_the_bound() {
        let started = Instant::now();
        let result = bounded_command_output("sleep", &["30"], Duration::from_millis(300));
        assert!(matches!(result, Err(BoundedCommandError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn generic_child_environment_excludes_daemon_secrets() {
        const SECRET_NAME: &str = "GOBLINS_OS_BOUNDED_TEST_OPENAI_SECRET";
        let previous = std::env::var_os(SECRET_NAME);
        std::env::set_var(SECRET_NAME, "must-not-reach-child");

        let output = bounded_command_output("env", &[], Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("isolated environment probe must run"));

        match previous {
            Some(value) => std::env::set_var(SECRET_NAME, value),
            None => std::env::remove_var(SECRET_NAME),
        }
        let environment = String::from_utf8(output.stdout).expect("environment is UTF-8");
        assert!(environment.contains("PATH="));
        assert!(!environment.contains(SECRET_NAME));
        assert!(!environment.contains("must-not-reach-child"));
    }

    #[test]
    fn session_child_receives_only_narrow_runtime_opt_ins() {
        const SESSION_NAME: &str = "DBUS_SESSION_BUS_ADDRESS";
        const SESSION_VALUE: &str = "unix:path=/tmp/goblins-os-bounded-test-bus";
        const SECRET_NAME: &str = "OPENAI_ACCOUNT_CLIENT_SECRET";
        let previous_session = std::env::var_os(SESSION_NAME);
        let previous_secret = std::env::var_os(SECRET_NAME);
        std::env::set_var(SESSION_NAME, SESSION_VALUE);
        std::env::set_var(SECRET_NAME, "must-not-reach-session-child");

        let output = bounded_session_command_output("env", &[], Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("isolated session environment probe must run"));

        match previous_session {
            Some(value) => std::env::set_var(SESSION_NAME, value),
            None => std::env::remove_var(SESSION_NAME),
        }
        match previous_secret {
            Some(value) => std::env::set_var(SECRET_NAME, value),
            None => std::env::remove_var(SECRET_NAME),
        }
        let environment = String::from_utf8(output.stdout).expect("environment is UTF-8");
        assert!(environment.contains(&format!("{SESSION_NAME}={SESSION_VALUE}")));
        assert!(!environment.contains(SECRET_NAME));
        assert!(!environment.contains("must-not-reach-session-child"));
    }

    #[tokio::test]
    async fn run_blocking_returns_the_body_value() {
        assert_eq!(super::run_blocking(|| 21 * 2).await, Ok(42));
    }

    #[tokio::test]
    async fn long_operation_admission_fails_fast_without_queueing() {
        let limiter = Arc::new(Semaphore::new(1));
        let held = Arc::clone(&limiter)
            .try_acquire_owned()
            .expect("test permit");

        let result = run_blocking_with_limiter(limiter, || 42).await;

        assert_eq!(result, Err(AdmissionError::Busy));
        drop(held);
    }

    #[tokio::test]
    async fn run_blocking_resumes_a_body_panic_on_the_caller() {
        // The panic must resurface on the awaiting task (axum turns it into a
        // 500), not vanish inside a JoinError.
        let result = tokio::spawn(super::run_blocking::<(), _>(|| panic!("boom"))).await;
        assert!(result.is_err_and(|error| error.is_panic()));
    }
}
