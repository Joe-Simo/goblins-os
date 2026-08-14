//! Immutable system-image reporting and explicit, bounded bootc operations.
//!
//! Goblins OS is a Fedora bootc immutable OS, so the real source of truth for
//! "what is booted, what can I roll back to, what is staged" is `bootc status`,
//! not a PATH probe. A `bootc` binary that merely exists on PATH proves nothing
//! about the deployment state — it may be a stale copy, may not be privileged,
//! or this may be a non-bootc runtime (a container, the live installer, or the
//! macOS development host). So this surface actually *runs* `bootc status
//! --format json` and reports the parsed deployments.
//!
//! Mutating operations are deliberately narrower than the bootc CLI: callers
//! select one allowlisted action, provide its exact confirmation phrase, and
//! only one action can run at a time. Every child is isolated and time-bounded.
//! When `bootc` is absent, not executable, errors, or returns something that is
//! not the JSON we expect, the surface degrades calmly to `available: false`
//! with a truthful `detail` — it never fabricates a deployment.

use std::{
    process::Output,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bounded::{bounded_command_output, BoundedCommandError};

/// Bound for `bootc status --format json`. Reading deployment state walks the
/// ostree repository, which can take well past the default probe bound on
/// spinning disks, so the status call gets its own wider bound.
const BOOTC_STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const BOOTC_CHECK_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const BOOTC_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(45 * 60);
const BOOTC_ROLLBACK_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const SYSTEM_REBOOT_TIMEOUT: Duration = Duration::from_secs(30);
const SYSTEM_UPDATE_HELPER: &str = "/usr/libexec/goblins-os/goblins-os-system-update";
const SYSTEM_UPDATE_UNIT_TEMPLATE: &str =
    "/usr/lib/systemd/system/goblins-os-system-update@.service";
const SYSTEM_UPDATE_POLKIT_RULE: &str =
    "/usr/share/polkit-1/rules.d/60-goblins-os-system-update.rules";
const SYSTEMCTL_PATH: &str = "/usr/bin/systemctl";

static SYSTEM_IMAGE_ACTION_ACTIVE: AtomicBool = AtomicBool::new(false);

fn active_action() -> &'static Mutex<Option<SystemImageAction>> {
    static ACTION: OnceLock<Mutex<Option<SystemImageAction>>> = OnceLock::new();
    ACTION.get_or_init(|| Mutex::new(None))
}

/// The shape returned to clients for `GET /v1/system/image`.
///
/// `available` is true only when `bootc status` ran and produced JSON we could
/// read. The deployment fields are populated from the real status; any field
/// the running `bootc` version does not provide is simply left `None` rather
/// than guessed.
#[derive(Clone, Serialize)]
pub struct SystemImageStatus {
    source: &'static str,
    /// The booted root is image-based and is never edited in place.
    immutable: bool,
    /// True only when `bootc status --format json` ran and parsed. When false,
    /// every deployment field is `None` and `detail` explains why.
    available: bool,
    /// Whether a rollback deployment exists to fall back to.
    rollback_available: bool,
    /// Whether a staged deployment is pending for the next boot.
    staged_available: bool,
    /// `bootc rollback` has selected the previous deployment for next boot.
    rollback_queued: bool,
    /// The physical bootc sysroot is on read-only media (for example, a live ISO).
    read_only: bool,
    /// Result of a known update check. `None` means no check result is cached.
    update_available: Option<bool>,
    /// Candidate metadata cached by `bootc upgrade --check`, when reported.
    available_update: Option<Deployment>,
    booted: Option<Deployment>,
    rollback: Option<Deployment>,
    staged: Option<Deployment>,
    actions: SystemImageActions,
    operation: Option<SystemImageOperation>,
    /// Human-readable, credential-free explanation of the result. On the
    /// degraded path this names the reason (missing, not privileged, parse
    /// failure, ...) without inventing state.
    detail: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SystemImageActions {
    can_check: bool,
    can_download: bool,
    can_apply: bool,
    can_reboot: bool,
    can_rollback: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SystemImageOperation {
    action: SystemImageAction,
    phase: &'static str,
    detail: &'static str,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemImageAction {
    Check,
    Download,
    Apply,
    Reboot,
    Rollback,
}

impl SystemImageAction {
    fn confirmation(self) -> &'static str {
        match self {
            Self::Check => "CHECK FOR UPDATES",
            Self::Download => "DOWNLOAD UPDATE",
            Self::Apply => "APPLY UPDATE AND RESTART",
            Self::Reboot => "RESTART GOBLINS OS",
            Self::Rollback => "ROLL BACK GOBLINS OS",
        }
    }

    fn present_participle(self) -> &'static str {
        match self {
            Self::Check => "Checking for updates",
            Self::Download => "Downloading the update",
            Self::Apply => "Applying the update",
            Self::Reboot => "Restarting Goblins OS",
            Self::Rollback => "Preparing the previous image",
        }
    }

    fn expects_reboot(self) -> bool {
        matches!(self, Self::Apply | Self::Reboot)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemImageActionRequest {
    action: SystemImageAction,
    confirmation: String,
}

#[derive(Serialize)]
pub struct SystemImageActionOutcome {
    ok: bool,
    action: SystemImageAction,
    phase: &'static str,
    reboot_expected: bool,
    detail: String,
    status: SystemImageStatus,
}

/// A single ostree/bootc deployment as reported by `bootc status`.
///
/// Field availability varies by `bootc` version; the parser fills in whatever
/// the status actually contained and leaves the rest `None`.
#[derive(Clone, Serialize, PartialEq, Eq, Debug, Default)]
pub struct Deployment {
    /// Full image reference, e.g. `quay.io/org/goblins-os:stable`.
    image: Option<String>,
    /// Transport the image is pulled over, e.g. `registry` or `containers-storage`.
    transport: Option<String>,
    /// Image digest as reported, e.g. `sha256:abcd...`.
    digest: Option<String>,
    /// Short form of the digest for compact display, e.g. `abcd1234`.
    digest_short: Option<String>,
    /// Image version label if the image carries one.
    version: Option<String>,
    /// Image build/creation timestamp if available.
    timestamp: Option<String>,
    /// A staged entry that bootc must explicitly unlock before it can boot.
    download_only: bool,
}

/// How an attempt to consult `bootc status` ended.
enum BootcStatusError {
    /// The `bootc` binary is not present in this runtime.
    Missing,
    /// `bootc` ran but exited non-zero; the (credential-free) message is kept.
    Failed(String),
    /// `bootc` produced output that was not the JSON document we expected.
    Unparsable,
}

pub async fn system_image_status() -> Json<SystemImageStatus> {
    Json(build_system_image_status())
}

pub async fn system_image_action(
    Json(request): Json<SystemImageActionRequest>,
) -> (StatusCode, Json<SystemImageActionOutcome>) {
    let action = request.action;
    if request.confirmation.trim() != action.confirmation() {
        return action_response(
            StatusCode::BAD_REQUEST,
            action,
            false,
            "rejected",
            format!(
                "This action needs the exact confirmation: {}.",
                action.confirmation()
            ),
        );
    }

    if systemd_system_image_operation().is_some() {
        return action_response(
            StatusCode::CONFLICT,
            action,
            false,
            "busy",
            "Another system-image operation is already running. Wait for it to finish before starting another one."
                .to_string(),
        );
    }

    let guard = match SystemImageActionGuard::begin(action) {
        Ok(guard) => guard,
        Err(()) => {
            return action_response(
                StatusCode::CONFLICT,
                action,
                false,
                "busy",
                "Another system-image operation is already running. Wait for it to finish before starting another one."
                    .to_string(),
            )
        }
    };

    let before = build_system_image_status();
    let pre_action_status = before.clone();
    let task = tokio::task::spawn_blocking(move || {
        let _guard = guard;
        execute_system_image_action_with(
            action,
            &before,
            system_update_bridge_ready(),
            bounded_command_output,
        )
    })
    .await;

    match task {
        Ok(Ok(detail)) if action.expects_reboot() => action_response_with_status(
            StatusCode::ACCEPTED,
            action,
            true,
            "queued",
            detail,
            pre_action_status,
        ),
        Ok(Ok(detail)) => completed_action_response(action, detail),
        Ok(Err(failure)) => {
            action_response(failure.status, action, false, failure.phase, failure.detail)
        }
        Err(_) => action_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            action,
            false,
            "failed",
            "The system-image worker stopped unexpectedly. No completion is reported.".to_string(),
        ),
    }
}

fn completed_action_response(
    action: SystemImageAction,
    fallback_detail: String,
) -> (StatusCode, Json<SystemImageActionOutcome>) {
    let mut status = build_system_image_status();
    let detail = match action {
        SystemImageAction::Check if status.available => {
            let available = status.update_available.unwrap_or(false);
            status.update_available = Some(available);
            if available {
                let candidate = status
                    .available_update
                    .as_ref()
                    .map(update_candidate_label)
                    .unwrap_or_else(|| "a newer immutable image".to_string());
                format!("An update is available: {candidate}. Download it when you are ready.")
            } else {
                "Goblins OS is up to date. bootc found no newer image for the configured source."
                    .to_string()
            }
        }
        SystemImageAction::Download if status.available => {
            match status.staged.as_ref() {
                Some(staged) if staged.download_only => format!(
                    "Downloaded {}. It is staged in download-only mode and will not boot until you explicitly apply it and restart.",
                    deployment_label(staged, "the update")
                ),
                Some(staged) => format!(
                    "Staged {} for the next boot.",
                    deployment_label(staged, "the update")
                ),
                None => "The download check completed, and bootc reports no staged deployment. The running image may already be current."
                    .to_string(),
            }
        }
        SystemImageAction::Rollback if status.rollback_queued => {
            "The previous immutable image is queued for the next boot. Restart when you are ready."
                .to_string()
        }
        _ => fallback_detail,
    };
    action_response_with_status(StatusCode::OK, action, true, "completed", detail, status)
}

fn deployment_label(deployment: &Deployment, fallback: &str) -> String {
    deployment
        .version
        .as_deref()
        .or(deployment.digest_short.as_deref())
        .unwrap_or(fallback)
        .to_string()
}

fn update_candidate_label(deployment: &Deployment) -> String {
    let label = deployment_label(deployment, "a newer immutable image");
    match deployment.timestamp.as_deref() {
        Some(timestamp) => format!("{label} ({timestamp})"),
        None => label,
    }
}

fn action_response_with_status(
    status_code: StatusCode,
    action: SystemImageAction,
    ok: bool,
    phase: &'static str,
    detail: String,
    mut status: SystemImageStatus,
) -> (StatusCode, Json<SystemImageActionOutcome>) {
    if ok && action.expects_reboot() {
        status.operation = Some(SystemImageOperation {
            action,
            phase,
            detail: "Restart job queued; verify the booted deployment after reconnecting.",
        });
        status.actions = SystemImageActions::default();
    }
    (
        status_code,
        Json(SystemImageActionOutcome {
            ok,
            action,
            phase,
            reboot_expected: ok && action.expects_reboot(),
            detail,
            status,
        }),
    )
}

fn action_response(
    status_code: StatusCode,
    action: SystemImageAction,
    ok: bool,
    phase: &'static str,
    detail: String,
) -> (StatusCode, Json<SystemImageActionOutcome>) {
    (
        status_code,
        Json(SystemImageActionOutcome {
            ok,
            action,
            phase,
            reboot_expected: ok && action.expects_reboot(),
            detail,
            status: build_system_image_status(),
        }),
    )
}

struct SystemImageActionGuard;

impl SystemImageActionGuard {
    fn begin(action: SystemImageAction) -> Result<Self, ()> {
        SYSTEM_IMAGE_ACTION_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ())?;
        match active_action().lock() {
            Ok(mut active) => *active = Some(action),
            Err(_) => {
                SYSTEM_IMAGE_ACTION_ACTIVE.store(false, Ordering::Release);
                return Err(());
            }
        }
        Ok(Self)
    }
}

impl Drop for SystemImageActionGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = active_action().lock() {
            *active = None;
        }
        SYSTEM_IMAGE_ACTION_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(Debug)]
struct SystemImageActionFailure {
    status: StatusCode,
    phase: &'static str,
    detail: String,
}

fn action_failure(
    status: StatusCode,
    phase: &'static str,
    detail: &str,
) -> SystemImageActionFailure {
    SystemImageActionFailure {
        status,
        phase,
        detail: detail.to_string(),
    }
}

fn execute_system_image_action_with<F>(
    action: SystemImageAction,
    status: &SystemImageStatus,
    bridge_ready: bool,
    runner: F,
) -> Result<String, SystemImageActionFailure>
where
    F: FnOnce(&str, &[&str], Duration) -> Result<Output, BoundedCommandError>,
{
    if !status.available || status.booted.is_none() {
        return Err(action_failure(
            StatusCode::PRECONDITION_FAILED,
            "blocked",
            "Goblins OS could not verify a booted immutable deployment, so the image action was not started.",
        ));
    }
    if status.read_only && action != SystemImageAction::Check {
        return Err(action_failure(
            StatusCode::PRECONDITION_FAILED,
            "blocked",
            "This session is running from read-only boot media. Install Goblins OS to writable storage before downloading, applying, restarting into, or rolling back system images.",
        ));
    }
    if action == SystemImageAction::Reboot && !status.staged_available && !status.rollback_queued {
        return Err(action_failure(
            StatusCode::PRECONDITION_FAILED,
            "blocked",
            "No staged deployment is waiting. Download an update or queue a rollback before using this restart action.",
        ));
    }
    if action == SystemImageAction::Rollback && !status.rollback_available {
        return Err(action_failure(
            StatusCode::PRECONDITION_FAILED,
            "blocked",
            "No previous immutable deployment is available to roll back to.",
        ));
    }

    if !bridge_ready {
        return Err(action_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "blocked",
            "The protected Goblins OS update bridge is not installed in this system image.",
        ));
    }
    let binary = SYSTEMCTL_PATH;
    let unit = system_update_unit_for_status(action, status);
    let blocking_args = ["start", unit];
    let queued_args = ["--no-block", "start", unit];
    let args: &[&str] = if action.expects_reboot() {
        &queued_args
    } else {
        &blocking_args
    };
    let timeout = match action {
        SystemImageAction::Check => BOOTC_CHECK_TIMEOUT,
        SystemImageAction::Download => BOOTC_DOWNLOAD_TIMEOUT,
        SystemImageAction::Apply => SYSTEM_REBOOT_TIMEOUT,
        SystemImageAction::Reboot => SYSTEM_REBOOT_TIMEOUT,
        SystemImageAction::Rollback => BOOTC_ROLLBACK_TIMEOUT,
    };

    let output = runner(binary, args, timeout).map_err(|error| match error {
        BoundedCommandError::Missing => action_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "blocked",
            "The system service manager is not available in this session.",
        ),
        BoundedCommandError::TimedOut => action_failure(
            StatusCode::GATEWAY_TIMEOUT,
            "timed-out",
            "The systemctl client exceeded its safety limit, but the protected systemd operation may still be running. Completion is unknown; refresh operation and image status before trying again.",
        ),
        BoundedCommandError::Failed => action_failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed",
            "Goblins OS could not start the requested system-image operation.",
        ),
    })?;

    if !output.status.success() {
        return Err(action_failure(
            StatusCode::BAD_GATEWAY,
            "failed",
            "The platform update tool exited without completing the requested operation. No completion is reported; refresh image status before trying again.",
        ));
    }

    Ok(system_image_success_detail(action, &output))
}

fn system_update_unit(action: SystemImageAction) -> &'static str {
    match action {
        SystemImageAction::Check => "goblins-os-system-update@check.service",
        SystemImageAction::Download => "goblins-os-system-update@download.service",
        SystemImageAction::Apply => "goblins-os-system-update@apply.service",
        SystemImageAction::Reboot => "goblins-os-system-update@reboot.service",
        SystemImageAction::Rollback => "goblins-os-system-update@rollback.service",
    }
}

fn system_update_unit_for_status(
    action: SystemImageAction,
    status: &SystemImageStatus,
) -> &'static str {
    if matches!(action, SystemImageAction::Apply | SystemImageAction::Reboot)
        && !status.rollback_queued
        && status
            .staged
            .as_ref()
            .is_some_and(|entry| entry.download_only)
    {
        "goblins-os-system-update@apply-downloaded.service"
    } else {
        system_update_unit(action)
    }
}

fn system_update_bridge_ready() -> bool {
    [
        SYSTEM_UPDATE_HELPER,
        SYSTEM_UPDATE_UNIT_TEMPLATE,
        SYSTEM_UPDATE_POLKIT_RULE,
        SYSTEMCTL_PATH,
    ]
    .iter()
    .all(|path| std::path::Path::new(path).is_file())
}

fn system_image_success_detail(action: SystemImageAction, _output: &Output) -> String {
    match action {
        SystemImageAction::Check => "The protected update check completed. Deployment status was refreshed; no availability result is inferred from systemctl output.".to_string(),
        SystemImageAction::Download => {
            "The protected download operation completed. Deployment status was refreshed; restart is offered only when bootc reports a staged image or queued rollback."
                .to_string()
        }
        SystemImageAction::Apply => {
            "The update-and-restart job was queued. The connection may close before completion; verify the booted image after Goblins OS returns."
                .to_string()
        }
        SystemImageAction::Reboot => {
            "The restart job was queued. The connection may close before completion; verify the booted image after Goblins OS returns.".to_string()
        }
        SystemImageAction::Rollback => {
            "The previous immutable image is queued for the next boot. Restart when you are ready."
                .to_string()
        }
    }
}

/// A compact, truthful summary of system-image readiness for the recovery view.
///
/// Unlike a bare PATH probe, this reflects whether `bootc status` actually
/// reported a booted deployment. `reportable` is true only when the real status
/// produced deployment state; `rollback_available` says whether there is
/// something to roll back to. The recovery surface uses this so its copy never
/// claims an action it cannot perform.
pub struct SystemImageSummary {
    /// True only when `bootc status` ran and reported a booted deployment.
    pub reportable: bool,
    /// True when a rollback deployment exists per `bootc status`.
    pub rollback_available: bool,
}

pub fn system_image_summary() -> SystemImageSummary {
    let status = build_system_image_status();
    SystemImageSummary {
        reportable: status.available && status.booted.is_some(),
        rollback_available: status.rollback_available,
    }
}

fn build_system_image_status() -> SystemImageStatus {
    let mut status = match run_bootc_status_json() {
        Ok(json) => parse_system_image_status(&json),
        Err(BootcStatusError::Missing) => degraded(
            "bootc is not available in this session, so system image status and rollback cannot be reported here.",
        ),
        Err(BootcStatusError::Failed(detail)) => degraded(&if detail.is_empty() {
            "bootc status could not be read in this session.".to_string()
        } else {
            format!("bootc status could not be read in this session: {detail}")
        }),
        Err(BootcStatusError::Unparsable) => degraded(
            "bootc status did not return the expected JSON in this session, so no deployment state is reported.",
        ),
    };
    if !system_update_bridge_ready() {
        status.actions = SystemImageActions::default();
    }
    status.operation = current_system_image_operation().or_else(systemd_system_image_operation);
    if status.operation.is_some() {
        status.actions = SystemImageActions::default();
    }
    status
}

fn current_system_image_operation() -> Option<SystemImageOperation> {
    if !SYSTEM_IMAGE_ACTION_ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    active_action()
        .lock()
        .ok()
        .and_then(|active| *active)
        .map(|action| SystemImageOperation {
            action,
            phase: "running",
            detail: action.present_participle(),
        })
}

fn systemd_system_image_operation() -> Option<SystemImageOperation> {
    if !system_update_bridge_ready() {
        return None;
    }
    let units = [
        (
            "goblins-os-system-update@check.service",
            SystemImageAction::Check,
        ),
        (
            "goblins-os-system-update@download.service",
            SystemImageAction::Download,
        ),
        (
            "goblins-os-system-update@apply.service",
            SystemImageAction::Apply,
        ),
        (
            "goblins-os-system-update@apply-downloaded.service",
            SystemImageAction::Reboot,
        ),
        (
            "goblins-os-system-update@reboot.service",
            SystemImageAction::Reboot,
        ),
        (
            "goblins-os-system-update@rollback.service",
            SystemImageAction::Rollback,
        ),
    ];
    let args = [
        "is-active",
        units[0].0,
        units[1].0,
        units[2].0,
        units[3].0,
        units[4].0,
        units[5].0,
    ];
    let output = bounded_command_output(SYSTEMCTL_PATH, &args, Duration::from_secs(2)).ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .zip(units)
        .find(|(state, _)| matches!(state.trim(), "active" | "activating"))
        .map(|(_, (_, action))| SystemImageOperation {
            action,
            phase: "running",
            detail: action.present_participle(),
        })
}

/// Run `bootc status --format json`, returning the parsed document on success.
///
/// Mirrors the command-running idiom used elsewhere in core (see
/// `network::nmcli`): a `NotFound` error means the tool is simply absent, a
/// run killed at the bound is `Failed` with honest timeout copy (the tool IS
/// present, it just did not answer), a non-zero exit is `Failed` with the
/// captured stderr, and non-JSON stdout is `Unparsable`. The arguments carry
/// no secrets, so this never needs to scrub output.
fn run_bootc_status_json() -> Result<Value, BootcStatusError> {
    let output = match bounded_command_output(
        "bootc",
        &["status", "--format", "json"],
        BOOTC_STATUS_TIMEOUT,
    ) {
        Ok(output) => output,
        Err(BoundedCommandError::TimedOut) => {
            return Err(BootcStatusError::Failed(
                "bootc did not answer before the status timeout.".to_string(),
            ))
        }
        Err(_) => return Err(BootcStatusError::Missing),
    };

    if !output.status.success() {
        return Err(BootcStatusError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    serde_json::from_slice::<Value>(&output.stdout).map_err(|_| BootcStatusError::Unparsable)
}

fn degraded(detail: &str) -> SystemImageStatus {
    SystemImageStatus {
        source: "goblins-os-core",
        immutable: true,
        available: false,
        rollback_available: false,
        staged_available: false,
        rollback_queued: false,
        read_only: false,
        update_available: None,
        available_update: None,
        booted: None,
        rollback: None,
        staged: None,
        actions: SystemImageActions::default(),
        operation: None,
        detail: detail.to_string(),
    }
}

/// Parse a `bootc status --format json` document into our response shape.
///
/// The bootc status schema has shifted across versions, so this is deliberately
/// tolerant: it looks for `status.{booted,rollback,staged}`, but also accepts a
/// top-level `{booted,rollback,staged}` for older/newer layouts. Every nested
/// field is optional — a missing field becomes `None`, never a fabricated
/// value.
fn parse_system_image_status(json: &Value) -> SystemImageStatus {
    // Newer bootc nests host state under `status`; some versions/spec dumps put
    // the deployments at the top level. Accept either.
    let status = json.get("status").unwrap_or(json);

    let booted_entry = status.get("booted");
    let booted = booted_entry.and_then(parse_deployment);
    let rollback = status.get("rollback").and_then(parse_deployment);
    let staged = status.get("staged").and_then(parse_deployment);
    let available_update = booted_entry
        .and_then(|entry| entry.get("cachedUpdate"))
        .and_then(parse_deployment);
    let rollback_queued = status
        .get("rollbackQueued")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let read_only = status
        .get("readOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let rollback_available = rollback.is_some();
    let staged_available = staged.is_some();
    let image_available = booted.is_some();
    let update_available = available_update.as_ref().and_then(|candidate| {
        let candidate_digest = candidate.digest.as_deref()?;
        let booted_digest = booted.as_ref()?.digest.as_deref()?;
        Some(candidate_digest != booted_digest)
    });

    let detail = if booted.is_some() {
        let mut parts = vec!["Booted system image reported by bootc status.".to_string()];
        if rollback_available {
            parts.push("A rollback image is available.".to_string());
        } else {
            parts.push("No rollback image is recorded.".to_string());
        }
        if staged_available {
            if staged.as_ref().is_some_and(|entry| entry.download_only) {
                parts.push(
                    "A downloaded image is staged but must be explicitly applied before it can boot."
                        .to_string(),
                );
            } else {
                parts.push("A staged image is pending for the next boot.".to_string());
            }
        } else if rollback_queued {
            parts.push("The rollback image is queued for the next boot.".to_string());
        }
        if update_available == Some(true) {
            parts.push("A checked update is available.".to_string());
        }
        if read_only {
            parts.push(
                "This read-only boot medium cannot download, apply, restart into, or roll back system images."
                    .to_string(),
            );
        }
        parts.join(" ")
    } else {
        "bootc status reported no booted deployment in this session.".to_string()
    };

    SystemImageStatus {
        source: "goblins-os-core",
        immutable: true,
        available: true,
        rollback_available,
        staged_available,
        rollback_queued,
        read_only,
        update_available,
        available_update,
        booted,
        rollback,
        staged,
        actions: SystemImageActions {
            can_check: image_available,
            can_download: image_available && !read_only,
            can_apply: image_available && !read_only,
            can_reboot: !read_only && (staged_available || rollback_queued),
            can_rollback: rollback_available && !read_only,
        },
        operation: None,
        detail,
    }
}

/// Pull a single deployment out of a status entry.
///
/// Each entry typically looks like `{ "image": { "image": { "image": "...",
/// "transport": "..." }, "imageDigest": "sha256:...", "version": "...",
/// "timestamp": "..." } }`, but layouts vary by version. We probe a few known
/// shapes and keep only what is actually present. Returns `None` for a JSON
/// `null` so a `"rollback": null` entry honestly means "no rollback".
fn parse_deployment(entry: &Value) -> Option<Deployment> {
    if entry.is_null() {
        return None;
    }

    // Official host-v1 shape: BootEntry.image is ImageStatus, whose `image`
    // member is an ImageReference containing `{image, transport}`. Retain the
    // shallower fallbacks for older bootc status documents.
    let image_status = entry.get("image").unwrap_or(entry);
    let image_reference = image_status.get("image").unwrap_or(image_status);

    let image = string_field(image_reference, "image")
        .or_else(|| {
            entry
                .get("image")
                .and_then(|reference| string_field(reference, "image"))
        })
        .or_else(|| string_field(image_status, "image"))
        .or_else(|| string_field(entry, "image"))
        .or_else(|| string_field(image_reference, "imageName"));

    let transport = string_field(image_reference, "transport")
        .or_else(|| string_field(image_status, "transport"))
        .or_else(|| string_field(entry, "transport"));

    // Digest can live under several keys depending on version.
    let digest = string_field(entry, "imageDigest")
        .or_else(|| string_field(image_status, "imageDigest"))
        .or_else(|| string_field(image_reference, "imageDigest"))
        .or_else(|| string_field(entry, "digest"))
        .or_else(|| string_field(image_status, "digest"))
        .or_else(|| string_field(image_reference, "digest"));

    let version = string_field(entry, "version")
        .or_else(|| string_field(image_status, "version"))
        .or_else(|| string_field(image_reference, "version"))
        .or_else(|| string_field(image_status, "imageVersion"));

    let timestamp = string_field(entry, "timestamp")
        .or_else(|| string_field(image_status, "timestamp"))
        .or_else(|| string_field(image_reference, "timestamp"))
        .or_else(|| string_field(entry, "imageTimestamp"));

    let digest_short = digest.as_deref().map(short_digest);

    let deployment = Deployment {
        image,
        transport,
        digest,
        digest_short,
        version,
        timestamp,
        download_only: entry
            .get("downloadOnly")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };

    // A present-but-empty entry carries no honest information; treat it as
    // absent rather than emitting an all-null deployment.
    if deployment == Deployment::default() {
        None
    } else {
        Some(deployment)
    }
}

/// Read a string field, trimming and dropping empties so blanks never surface
/// as fake data.
fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Compact a digest for display: keep the algorithm prefix's hex short, e.g.
/// `sha256:abcd1234...` -> `abcd1234`, `abcd1234ef...` -> `abcd1234`.
fn short_digest(digest: &str) -> String {
    let hex = digest.rsplit(':').next().unwrap_or(digest);
    hex.chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        build_system_image_status, execute_system_image_action_with, parse_system_image_status,
        short_digest, system_update_unit, system_update_unit_for_status, SystemImageAction,
        BOOTC_CHECK_TIMEOUT, SYSTEMCTL_PATH,
    };
    use crate::bounded::BoundedCommandError;
    use serde_json::json;

    #[test]
    fn parses_booted_and_rollback_from_nested_status() {
        let doc = json!({
            "status": {
                "booted": {
                    "image": {
                        "image": "quay.io/goblins-os/base:stable",
                        "transport": "registry"
                    },
                    "imageDigest": "sha256:0123456789abcdef0123456789abcdef",
                    "version": "40.20260601.0",
                    "timestamp": "2026-06-01T00:00:00Z"
                },
                "rollback": {
                    "image": {
                        "image": "quay.io/goblins-os/base:stable",
                        "transport": "registry"
                    },
                    "imageDigest": "sha256:fedcba9876543210fedcba9876543210",
                    "version": "40.20260520.0"
                },
                "staged": null
            }
        });

        let status = parse_system_image_status(&doc);

        assert!(status.available);
        assert!(status.rollback_available);
        assert!(!status.staged_available);

        let booted = status.booted.expect("booted deployment present");
        assert_eq!(
            booted.image.as_deref(),
            Some("quay.io/goblins-os/base:stable")
        );
        assert_eq!(booted.transport.as_deref(), Some("registry"));
        assert_eq!(
            booted.digest.as_deref(),
            Some("sha256:0123456789abcdef0123456789abcdef")
        );
        assert_eq!(booted.digest_short.as_deref(), Some("0123456789ab"));
        assert_eq!(booted.version.as_deref(), Some("40.20260601.0"));
        assert_eq!(booted.timestamp.as_deref(), Some("2026-06-01T00:00:00Z"));

        let rollback = status.rollback.expect("rollback deployment present");
        assert_eq!(rollback.version.as_deref(), Some("40.20260520.0"));
        assert!(status.staged.is_none());
    }

    #[test]
    fn parses_host_v1_image_reference_and_queued_rollback() {
        let doc = json!({
            "status": {
                "booted": {
                    "image": {
                        "image": {
                            "image": "ghcr.io/goblins-os/goblins-os:stable",
                            "transport": "registry"
                        },
                        "imageDigest": "sha256:0123456789abcdef",
                        "version": "41.20260813.0",
                        "timestamp": "2026-08-13T10:00:00Z"
                    }
                },
                "rollback": {
                    "image": {
                        "image": {
                            "image": "ghcr.io/goblins-os/goblins-os:stable",
                            "transport": "registry"
                        },
                        "imageDigest": "sha256:fedcba9876543210",
                        "version": "41.20260812.0"
                    }
                },
                "staged": null,
                "rollbackQueued": true
            }
        });

        let status = parse_system_image_status(&doc);
        let booted = status.booted.as_ref().expect("host-v1 booted deployment");
        assert_eq!(
            booted.image.as_deref(),
            Some("ghcr.io/goblins-os/goblins-os:stable")
        );
        assert_eq!(booted.transport.as_deref(), Some("registry"));
        assert_eq!(booted.version.as_deref(), Some("41.20260813.0"));
        assert!(status.rollback_queued);
        assert!(!status.staged_available);
        assert!(status.actions.can_reboot);
    }

    #[test]
    fn parses_host_v1_cached_update_read_only_and_download_only_state() {
        let doc = json!({
            "apiVersion": "org.containers.bootc/v1",
            "kind": "BootcHost",
            "status": {
                "readOnly": true,
                "booted": {
                    "image": {
                        "image": {
                            "image": "ghcr.io/goblins-os/goblins-os:stable",
                            "transport": "registry"
                        },
                        "imageDigest": "sha256:booted",
                        "version": "41.20260813.0"
                    },
                    "cachedUpdate": {
                        "image": {
                            "image": "ghcr.io/goblins-os/goblins-os:stable",
                            "transport": "registry"
                        },
                        "imageDigest": "sha256:candidate",
                        "version": "41.20260814.0",
                        "timestamp": "2026-08-14T10:00:00Z"
                    },
                    "downloadOnly": false,
                    "incompatible": false,
                    "pinned": false
                },
                "staged": {
                    "image": {
                        "image": {
                            "image": "ghcr.io/goblins-os/goblins-os:stable",
                            "transport": "registry"
                        },
                        "imageDigest": "sha256:candidate",
                        "version": "41.20260814.0"
                    },
                    "downloadOnly": true,
                    "incompatible": false,
                    "pinned": false
                },
                "rollback": null,
                "rollbackQueued": false
            }
        });

        let status = parse_system_image_status(&doc);
        assert!(status.read_only);
        assert_eq!(status.update_available, Some(true));
        let candidate = status
            .available_update
            .as_ref()
            .expect("cached update is exposed");
        assert_eq!(candidate.version.as_deref(), Some("41.20260814.0"));
        assert_eq!(
            candidate.image.as_deref(),
            Some("ghcr.io/goblins-os/goblins-os:stable")
        );
        assert!(status
            .staged
            .as_ref()
            .is_some_and(|staged| staged.download_only));
        assert!(status.actions.can_check);
        assert!(!status.actions.can_download);
        assert!(!status.actions.can_apply);
        assert!(!status.actions.can_reboot);
        assert!(!status.actions.can_rollback);
    }

    #[test]
    fn cached_update_with_booted_digest_reports_current() {
        let status = parse_system_image_status(&json!({
            "status": {
                "booted": {
                    "image": { "imageDigest": "sha256:same" },
                    "cachedUpdate": { "imageDigest": "sha256:same" }
                }
            }
        }));
        assert_eq!(status.update_available, Some(false));
    }

    #[test]
    fn parses_top_level_layout_without_status_wrapper() {
        let doc = json!({
            "booted": {
                "image": { "image": "ostree-image:latest", "transport": "oci" },
                "digest": "abcdef1234567890"
            }
        });

        let status = parse_system_image_status(&doc);

        assert!(status.available);
        assert!(!status.rollback_available);
        let booted = status.booted.expect("booted deployment present");
        assert_eq!(booted.image.as_deref(), Some("ostree-image:latest"));
        assert_eq!(booted.transport.as_deref(), Some("oci"));
        assert_eq!(booted.digest_short.as_deref(), Some("abcdef123456"));
    }

    #[test]
    fn missing_deployments_do_not_fabricate_state() {
        let doc = json!({ "status": { "booted": null, "rollback": null } });

        let status = parse_system_image_status(&doc);

        assert!(status.available);
        assert!(status.booted.is_none());
        assert!(!status.rollback_available);
        assert!(status.detail.contains("no booted deployment"));
    }

    #[test]
    fn short_digest_handles_prefixed_and_bare_forms() {
        assert_eq!(short_digest("sha256:0123456789abcdef"), "0123456789ab");
        assert_eq!(short_digest("0123456789abcdef"), "0123456789ab");
        assert_eq!(short_digest("short"), "short");
    }

    #[test]
    fn update_actions_use_only_fixed_systemd_units() {
        let status = parse_system_image_status(&json!({
            "status": {
                "booted": { "image": "example.test/goblins:stable" },
                "rollback": { "image": "example.test/goblins:previous" },
                "staged": { "image": "example.test/goblins:next" }
            }
        }));
        let detail = execute_system_image_action_with(
            SystemImageAction::Check,
            &status,
            true,
            |binary, args, timeout| {
                assert_eq!(binary, SYSTEMCTL_PATH);
                assert_eq!(args, ["start", "goblins-os-system-update@check.service"]);
                assert_eq!(timeout, BOOTC_CHECK_TIMEOUT);
                std::process::Command::new("true")
                    .output()
                    .map_err(|_| BoundedCommandError::Failed)
            },
        )
        .expect("fixed unit action succeeds");
        assert!(detail.contains("protected update check completed"));
        assert_eq!(
            system_update_unit(SystemImageAction::Rollback),
            "goblins-os-system-update@rollback.service"
        );
    }

    #[test]
    fn rebooting_actions_enqueue_without_waiting_for_shutdown() {
        let status = parse_system_image_status(&json!({
            "status": {
                "booted": { "image": "example.test/goblins:stable" },
                "rollback": { "image": "example.test/goblins:previous" },
                "staged": null,
                "rollbackQueued": true
            }
        }));
        let detail = execute_system_image_action_with(
            SystemImageAction::Reboot,
            &status,
            true,
            |binary, args, timeout| {
                assert_eq!(binary, SYSTEMCTL_PATH);
                assert_eq!(
                    args,
                    [
                        "--no-block",
                        "start",
                        "goblins-os-system-update@reboot.service"
                    ]
                );
                assert_eq!(timeout, super::SYSTEM_REBOOT_TIMEOUT);
                std::process::Command::new("true")
                    .output()
                    .map_err(|_| BoundedCommandError::Failed)
            },
        )
        .expect("queued rollback reboot is allowed");
        assert!(detail.contains("job was queued"));
    }

    #[test]
    fn systemctl_timeout_never_claims_the_systemd_job_was_stopped() {
        let status = parse_system_image_status(&json!({
            "status": { "booted": { "image": "example.test/goblins:stable" } }
        }));
        let failure = execute_system_image_action_with(
            SystemImageAction::Download,
            &status,
            true,
            |_, _, _| Err(BoundedCommandError::TimedOut),
        )
        .expect_err("timeout must be reported honestly");
        assert!(failure.detail.contains("may still be running"));
        assert!(!failure.detail.contains("was stopped"));
    }

    #[test]
    fn installed_apply_helper_uses_bootcs_fixed_rebooting_action() {
        let helper = include_str!("../../../os/bootc/goblins-os-system-update");
        assert!(helper.starts_with("#!/bin/sh\n"));
        let apply = helper
            .split("  apply)\n")
            .nth(1)
            .and_then(|section| section.split("    ;;\n").next())
            .expect("fixed apply action");
        assert!(apply.contains("exec \"$BOOTC\" upgrade --apply\n"));
        assert!(!apply.contains("systemctl"));
        let apply_downloaded = helper
            .split("  apply-downloaded)\n")
            .nth(1)
            .and_then(|section| section.split("    ;;\n").next())
            .expect("fixed downloaded apply action");
        assert!(apply_downloaded.contains("exec \"$BOOTC\" upgrade --from-downloaded --apply\n"));
    }

    #[test]
    fn downloaded_staging_routes_both_apply_entry_points_to_exact_image() {
        let status = parse_system_image_status(&json!({
            "status": {
                "booted": { "image": "example.test/goblins:stable" },
                "staged": {
                    "image": "example.test/goblins:next",
                    "downloadOnly": true
                }
            }
        }));
        assert_eq!(
            system_update_unit_for_status(SystemImageAction::Apply, &status),
            "goblins-os-system-update@apply-downloaded.service"
        );
        assert_eq!(
            system_update_unit_for_status(SystemImageAction::Reboot, &status),
            "goblins-os-system-update@apply-downloaded.service"
        );
    }

    #[test]
    fn read_only_media_blocks_mutating_actions_before_systemd() {
        let status = parse_system_image_status(&json!({
            "status": {
                "readOnly": true,
                "booted": { "image": "example.test/goblins:stable" },
                "rollback": { "image": "example.test/goblins:previous" }
            }
        }));
        let failure = execute_system_image_action_with(
            SystemImageAction::Rollback,
            &status,
            true,
            |_, _, _| panic!("read-only media must be rejected before systemd"),
        )
        .expect_err("mutating action is blocked");
        assert_eq!(failure.phase, "blocked");
        assert!(failure.detail.contains("read-only boot media"));
    }

    #[test]
    fn update_actions_fail_closed_without_bridge_or_deployment() {
        let status = parse_system_image_status(&json!({
            "status": { "booted": { "image": "example.test/goblins:stable" } }
        }));
        let blocked = execute_system_image_action_with(
            SystemImageAction::Download,
            &status,
            false,
            |_, _, _| panic!("runner must not execute without the installed bridge"),
        )
        .expect_err("missing bridge blocks the action");
        assert_eq!(blocked.phase, "blocked");

        let missing_rollback = execute_system_image_action_with(
            SystemImageAction::Rollback,
            &status,
            true,
            |_, _, _| panic!("runner must not execute without a rollback deployment"),
        )
        .expect_err("missing rollback blocks the action");
        assert_eq!(missing_rollback.phase, "blocked");
    }

    #[test]
    fn degrades_when_bootc_is_unavailable() {
        // On the macOS development host (and any non-bootc runtime) bootc is not
        // present, so this must degrade honestly rather than fabricate.
        let status = build_system_image_status();
        if !status.available {
            assert!(status.booted.is_none());
            assert!(status.rollback.is_none());
            assert!(!status.rollback_available);
            assert!(!status.detail.trim().is_empty());
        }
    }
}
