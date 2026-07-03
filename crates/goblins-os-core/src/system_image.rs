//! Honest, read-only system image reporting backed by `bootc status`.
//!
//! Goblins OS is a Fedora bootc immutable OS, so the real source of truth for
//! "what is booted, what can I roll back to, what is staged" is `bootc status`,
//! not a PATH probe. A `bootc` binary that merely exists on PATH proves nothing
//! about the deployment state — it may be a stale copy, may not be privileged,
//! or this may be a non-bootc runtime (a container, the live installer, or the
//! macOS development host). So this surface actually *runs* `bootc status
//! --format json` and reports the parsed deployments.
//!
//! Everything here is read-only. There is deliberately no upgrade or rollback
//! executor: recovery stays honest about being a report, never an action it
//! cannot perform. When `bootc` is absent, not executable, errors, or returns
//! something that is not the JSON we expect, the surface degrades calmly to
//! `available: false` with a truthful `detail` — it never fabricates a
//! deployment.

use std::time::Duration;

use axum::Json;
use serde::Serialize;
use serde_json::Value;

use crate::bounded::{bounded_command_output, BoundedCommandError};

/// Bound for `bootc status --format json`. Reading deployment state walks the
/// ostree repository, which can take well past the default probe bound on
/// spinning disks, so the status call gets its own wider bound.
const BOOTC_STATUS_TIMEOUT: Duration = Duration::from_secs(10);

/// The shape returned to clients for `GET /v1/system/image`.
///
/// `available` is true only when `bootc status` ran and produced JSON we could
/// read. The deployment fields are populated from the real status; any field
/// the running `bootc` version does not provide is simply left `None` rather
/// than guessed.
#[derive(Serialize)]
pub struct SystemImageStatus {
    source: &'static str,
    /// True only when `bootc status --format json` ran and parsed. When false,
    /// every deployment field is `None` and `detail` explains why.
    available: bool,
    /// Whether a rollback deployment exists to fall back to.
    rollback_available: bool,
    /// Whether a staged deployment is pending for the next boot.
    staged_available: bool,
    booted: Option<Deployment>,
    rollback: Option<Deployment>,
    staged: Option<Deployment>,
    /// Human-readable, credential-free explanation of the result. On the
    /// degraded path this names the reason (missing, not privileged, parse
    /// failure, ...) without inventing state.
    detail: String,
}

/// A single ostree/bootc deployment as reported by `bootc status`.
///
/// Field availability varies by `bootc` version; the parser fills in whatever
/// the status actually contained and leaves the rest `None`.
#[derive(Serialize, PartialEq, Eq, Debug, Default)]
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
    match run_bootc_status_json() {
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
    }
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
        available: false,
        rollback_available: false,
        staged_available: false,
        booted: None,
        rollback: None,
        staged: None,
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

    let booted = status.get("booted").and_then(parse_deployment);
    let rollback = status.get("rollback").and_then(parse_deployment);
    let staged = status.get("staged").and_then(parse_deployment);

    let rollback_available = rollback.is_some();
    let staged_available = staged.is_some();

    let detail = if booted.is_some() {
        let mut parts = vec!["Booted system image reported by bootc status.".to_string()];
        if rollback_available {
            parts.push("A rollback image is available.".to_string());
        } else {
            parts.push("No rollback image is recorded.".to_string());
        }
        if staged_available {
            parts.push("A staged image is pending for the next boot.".to_string());
        }
        parts.join(" ")
    } else {
        "bootc status reported no booted deployment in this session.".to_string()
    };

    SystemImageStatus {
        source: "goblins-os-core",
        available: true,
        rollback_available,
        staged_available,
        booted,
        rollback,
        staged,
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

    // The image spec block. Newer bootc: entry.image.image (a string) plus
    // entry.image.transport. Some layouts expose entry.image as the spec object
    // directly. We try entry.image first, then fall back to entry itself.
    let image_block = entry.get("image").unwrap_or(entry);

    let image = string_field(image_block, "image")
        .or_else(|| string_field(entry, "image"))
        .or_else(|| string_field(image_block, "imageName"));

    let transport =
        string_field(image_block, "transport").or_else(|| string_field(entry, "transport"));

    // Digest can live under several keys depending on version.
    let digest = string_field(entry, "imageDigest")
        .or_else(|| string_field(image_block, "imageDigest"))
        .or_else(|| string_field(entry, "digest"))
        .or_else(|| string_field(image_block, "digest"));

    let version = string_field(entry, "version")
        .or_else(|| string_field(image_block, "version"))
        .or_else(|| string_field(image_block, "imageVersion"));

    let timestamp = string_field(entry, "timestamp")
        .or_else(|| string_field(image_block, "timestamp"))
        .or_else(|| string_field(entry, "imageTimestamp"));

    let digest_short = digest.as_deref().map(short_digest);

    let deployment = Deployment {
        image,
        transport,
        digest,
        digest_short,
        version,
        timestamp,
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
    use super::{build_system_image_status, parse_system_image_status, short_digest};
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
