//! Bounded external-command execution for every core probe and control call.
//!
//! A core route handler must never hold an async runtime worker hostage: an
//! external command that outlives its bound is killed and reported as a
//! timeout so the calling surface degrades honestly instead of wedging the
//! whole daemon. Two blocked workers are enough to make every `/v1` route
//! unreachable on small machines, which is exactly what the hardware gate
//! observed before every raw `Command` call was routed through here.

use std::{
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(crate) const PROBE_TIMEOUT_MS_DEFAULT: u64 = 4_000;
pub(crate) const PROBE_TIMEOUT_MS_MIN: u64 = 250;
pub(crate) const PROBE_TIMEOUT_MS_MAX: u64 = 120_000;

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

pub(crate) fn bounded_command_output(
    binary: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, BoundedCommandError> {
    let mut command = Command::new(binary);
    command.args(args);
    bounded_output_of(&mut command, timeout)
}

/// Same bound for a pre-configured `Command` (environment, working directory).
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
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|_| BoundedCommandError::Failed)
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait_with_output();
                    return Err(BoundedCommandError::TimedOut);
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait_with_output();
                return Err(BoundedCommandError::Failed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_command_output, clamp_probe_timeout_ms, BoundedCommandError,
        PROBE_TIMEOUT_MS_DEFAULT, PROBE_TIMEOUT_MS_MAX, PROBE_TIMEOUT_MS_MIN,
    };
    use std::time::{Duration, Instant};

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
    fn hung_child_is_killed_at_the_bound() {
        let started = Instant::now();
        let result = bounded_command_output("sleep", &["30"], Duration::from_millis(300));
        assert!(matches!(result, Err(BoundedCommandError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
