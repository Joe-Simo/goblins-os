use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

const DEFAULT_POLICY_STATE_DIR: &str = "/var/lib/goblins-os/policy";

#[derive(Serialize)]
pub struct PolicyStatus {
    generated_at: String,
    source: &'static str,
    state_path: String,
    permission_path: String,
    profile: PolicyProfile,
    locked: bool,
    data_boundary: String,
    secret_boundary: &'static str,
    controls: Vec<PolicyControl>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyProfile {
    Consumer,
    Business,
    Enterprise,
    LocalOnly,
}

#[derive(Serialize, Clone)]
pub struct PolicyControl {
    id: &'static str,
    label: &'static str,
    state: PolicyControlState,
    profile_state: PolicyControlState,
    grant: Option<PolicyPermissionGrant>,
    detail: String,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyControlState {
    Allowed,
    Denied,
    PermissionGated,
}

#[derive(Deserialize)]
pub struct ConfigurePolicyRequest {
    profile: PolicyProfile,
    acknowledgement: String,
}

#[derive(Serialize)]
pub struct ConfigurePolicyResponse {
    ok: bool,
    state_path: String,
    profile: PolicyProfile,
}

#[derive(Deserialize)]
pub struct GrantPermissionRequest {
    control_id: String,
    acknowledgement: String,
}

#[derive(Serialize)]
pub struct GrantPermissionResponse {
    ok: bool,
    permission_path: String,
    grant: PolicyPermissionGrant,
}

#[derive(Serialize, Deserialize)]
struct StoredPolicy {
    profile: PolicyProfile,
    configured_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredPermissionGrants {
    grants: Vec<PolicyPermissionGrant>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PolicyPermissionGrant {
    control_id: String,
    profile: PolicyProfile,
    granted_at: String,
    acknowledgement: String,
}

pub async fn policy_status() -> Json<PolicyStatus> {
    Json(build_policy_status())
}

pub async fn grant_permission(Json(request): Json<GrantPermissionRequest>) -> impl IntoResponse {
    if policy_locked() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "text": "Goblins OS policy permissions are locked by this image or enterprise configuration."
            })),
        )
            .into_response();
    }

    let profile = active_policy_profile();
    let control_id = request.control_id.trim();
    if control_state(profile, control_id) != PolicyControlState::PermissionGated {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "text": "Only permission-gated Goblins OS controls can receive explicit grants."
            })),
        )
            .into_response();
    }

    let expected = permission_acknowledgement(control_id, profile);
    if request.acknowledgement != expected {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "text": format!("Exact acknowledgement required: {expected}")
            })),
        )
            .into_response();
    }

    let grant = PolicyPermissionGrant {
        control_id: control_id.to_string(),
        profile,
        granted_at: format!("{:?}", SystemTime::now()),
        acknowledgement: expected,
    };

    if persist_permission_grant(grant.clone()).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "text": "Goblins OS could not persist the permission grant."
            })),
        )
            .into_response();
    }

    Json(GrantPermissionResponse {
        ok: true,
        permission_path: permission_state_path().display().to_string(),
        grant,
    })
    .into_response()
}

pub async fn configure_policy(Json(request): Json<ConfigurePolicyRequest>) -> impl IntoResponse {
    if policy_locked() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "text": "Goblins OS policy is locked by this image or enterprise configuration."
            })),
        )
            .into_response();
    }

    let expected = format!("SET GOBLINS OS POLICY {}", request.profile.as_str());
    if request.acknowledgement != expected {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "text": format!("Exact acknowledgement required: {expected}")
            })),
        )
            .into_response();
    }

    if persist_policy(request.profile).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "text": "Goblins OS could not persist the policy profile."
            })),
        )
            .into_response();
    }

    Json(ConfigurePolicyResponse {
        ok: true,
        state_path: policy_state_path().display().to_string(),
        profile: request.profile,
    })
    .into_response()
}

pub(crate) fn build_policy_status() -> PolicyStatus {
    let profile = active_policy_profile();
    PolicyStatus {
        generated_at: format!("{:?}", SystemTime::now()),
        source: "goblins-os-core",
        state_path: policy_state_path().display().to_string(),
        permission_path: permission_state_path().display().to_string(),
        profile,
        locked: policy_locked(),
        data_boundary: data_boundary(profile),
        secret_boundary: "OpenAI credentials stay in protected Goblins OS services or a managed organization service.",
        controls: policy_controls(profile),
    }
}

pub(crate) fn policy_state_for_control(id: &str) -> PolicyControlState {
    let profile = active_policy_profile();
    effective_control_state(profile, id)
}

impl PolicyProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Consumer => "consumer",
            Self::Business => "business",
            Self::Enterprise => "enterprise",
            Self::LocalOnly => "local-only",
        }
    }
}

fn active_policy_profile() -> PolicyProfile {
    env::var("GOBLINS_OS_POLICY_PROFILE")
        .ok()
        .and_then(|value| parse_policy_profile(&value))
        .or_else(read_policy_profile)
        .unwrap_or(PolicyProfile::Consumer)
}

fn parse_policy_profile(value: &str) -> Option<PolicyProfile> {
    match value {
        "consumer" => Some(PolicyProfile::Consumer),
        "business" => Some(PolicyProfile::Business),
        "enterprise" => Some(PolicyProfile::Enterprise),
        "local-only" => Some(PolicyProfile::LocalOnly),
        _ => None,
    }
}

fn read_policy_profile() -> Option<PolicyProfile> {
    let bytes = fs::read(policy_state_path()).ok()?;
    serde_json::from_slice::<StoredPolicy>(&bytes)
        .ok()
        .map(|stored| stored.profile)
}

fn read_permission_grants() -> StoredPermissionGrants {
    fs::read(permission_state_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoredPermissionGrants>(&bytes).ok())
        .unwrap_or_else(|| StoredPermissionGrants { grants: Vec::new() })
}

fn persist_policy(profile: PolicyProfile) -> std::io::Result<()> {
    let path = policy_state_path();
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("policy state path has no parent"));
    };

    create_state_dir(parent)?;
    let body = serde_json::to_vec(&StoredPolicy {
        profile,
        configured_at: format!("{:?}", SystemTime::now()),
    })?;
    // Write to a sibling temp file then rename onto the final path, so a crash or
    // concurrent overwrite can never leave a truncated profile.json — which would
    // parse to None and silently fail open to the most permissive Consumer profile.
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body)?;
    fs::rename(tmp, path)
}

fn persist_permission_grant(grant: PolicyPermissionGrant) -> std::io::Result<()> {
    let path = permission_state_path();
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("permission state path has no parent"));
    };

    create_state_dir(parent)?;
    let mut stored = read_permission_grants();
    stored.grants.retain(|existing| {
        existing.control_id != grant.control_id || existing.profile != grant.profile
    });
    stored.grants.push(grant);
    let body = serde_json::to_vec(&stored)?;
    // Atomic temp+rename so a torn write can never corrupt permissions.json (which
    // would silently re-prompt every grant).
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body)?;
    fs::rename(tmp, path)
}

fn create_state_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o750))?;
    }

    Ok(())
}

fn policy_state_path() -> PathBuf {
    env::var("GOBLINS_OS_POLICY_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_POLICY_STATE_DIR).to_path_buf())
        .join("profile.json")
}

fn permission_state_path() -> PathBuf {
    env::var("GOBLINS_OS_POLICY_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_POLICY_STATE_DIR).to_path_buf())
        .join("permissions.json")
}

fn policy_locked() -> bool {
    env::var("GOBLINS_OS_POLICY_LOCKED").as_deref() == Ok("1")
}

fn data_boundary(profile: PolicyProfile) -> String {
    match profile {
        PolicyProfile::Consumer => {
            "Consumer profile allows OpenAI cloud services and local gpt-oss with explicit consent."
                .to_string()
        }
        PolicyProfile::Business => {
            "Business profile allows team OpenAI services, local models, and permission-gated automation."
                .to_string()
        }
        PolicyProfile::Enterprise => {
            "Enterprise profile enables admin-managed OpenAI services, audit-sensitive controls, and strict permission gates."
                .to_string()
        }
        PolicyProfile::LocalOnly => {
            "Local-only profile denies cloud OpenAI services and keeps model execution on user-controlled infrastructure."
                .to_string()
        }
    }
}

fn policy_controls(profile: PolicyProfile) -> Vec<PolicyControl> {
    vec![
        control(profile, "cloud-openai", "OpenAI cloud services"),
        control(profile, "local-models", "Local gpt-oss models"),
        control(profile, "resident-assistant", "Goblins AI assistant"),
        control(
            profile,
            "screen-context",
            "Screen and selected text context",
        ),
        control(profile, "file-context", "File and folder context"),
        control(profile, "settings-control", "Confirmed Settings changes"),
        control(profile, "notification-context", "Notification context"),
        control(profile, "system-troubleshooting", "System troubleshooting"),
        control(profile, "app-builder", "Goblins AI app builder"),
        control(profile, "agents", "Agents"),
        control(profile, "computer-use", "Computer Use"),
        control(profile, "enterprise-controls", "Enterprise controls"),
    ]
}

fn control(profile: PolicyProfile, id: &'static str, label: &'static str) -> PolicyControl {
    let profile_state = control_state(profile, id);
    let grant = permission_grant_for_control(profile, id);
    let state = effective_control_state(profile, id);
    PolicyControl {
        id,
        label,
        profile_state,
        state,
        grant,
        detail: control_detail(profile, id, state),
    }
}

fn effective_control_state(profile: PolicyProfile, id: &str) -> PolicyControlState {
    let state = control_state(profile, id);
    if state == PolicyControlState::PermissionGated
        && permission_grant_for_control(profile, id).is_some()
    {
        PolicyControlState::Allowed
    } else {
        state
    }
}

fn permission_grant_for_control(profile: PolicyProfile, id: &str) -> Option<PolicyPermissionGrant> {
    read_permission_grants()
        .grants
        .into_iter()
        .find(|grant| grant.profile == profile && grant.control_id == id)
}

fn control_state(profile: PolicyProfile, id: &str) -> PolicyControlState {
    match profile {
        PolicyProfile::Consumer => match id {
            "enterprise-controls" => PolicyControlState::Denied,
            "computer-use"
            | "app-builder"
            | "agents"
            | "screen-context"
            | "file-context"
            | "settings-control"
            | "notification-context" => PolicyControlState::PermissionGated,
            _ => PolicyControlState::Allowed,
        },
        PolicyProfile::Business => match id {
            "computer-use"
            | "app-builder"
            | "agents"
            | "enterprise-controls"
            | "screen-context"
            | "file-context"
            | "settings-control"
            | "notification-context" => PolicyControlState::PermissionGated,
            _ => PolicyControlState::Allowed,
        },
        PolicyProfile::Enterprise => match id {
            "computer-use"
            | "app-builder"
            | "agents"
            | "screen-context"
            | "file-context"
            | "settings-control"
            | "notification-context"
            | "system-troubleshooting" => PolicyControlState::PermissionGated,
            _ => PolicyControlState::Allowed,
        },
        PolicyProfile::LocalOnly => match id {
            "cloud-openai" | "agents" | "enterprise-controls" => PolicyControlState::Denied,
            "computer-use"
            | "app-builder"
            | "screen-context"
            | "file-context"
            | "settings-control"
            | "notification-context"
            | "system-troubleshooting" => PolicyControlState::PermissionGated,
            _ => PolicyControlState::Allowed,
        },
    }
}

fn control_detail(profile: PolicyProfile, id: &str, state: PolicyControlState) -> String {
    if control_state(profile, id) == PolicyControlState::PermissionGated
        && state == PolicyControlState::Allowed
    {
        return "Allowed by an explicit Goblins OS permission grant.".to_string();
    }

    match (profile, id, state) {
        (PolicyProfile::LocalOnly, "cloud-openai", _) => {
            "Cloud OpenAI services are blocked by the local-only policy.".to_string()
        }
        (_, "computer-use", PolicyControlState::PermissionGated) => {
            "Computer Use requires explicit OS permission prompts before action.".to_string()
        }
        (_, "screen-context", PolicyControlState::PermissionGated) => {
            "Screen, screenshot, selected-text, and current-window context require explicit consent before Goblins AI can read them.".to_string()
        }
        (_, "file-context", PolicyControlState::PermissionGated) => {
            "File and folder context requires choosing the item and granting explicit access.".to_string()
        }
        (_, "settings-control", PolicyControlState::PermissionGated) => {
            "Settings changes require policy approval and a final confirmation before Goblins AI can apply them.".to_string()
        }
        (_, "notification-context", PolicyControlState::PermissionGated) => {
            "Notification context requires explicit approval before Goblins AI can inspect or act on a notification.".to_string()
        }
        (_, "system-troubleshooting", PolicyControlState::PermissionGated) => {
            "System troubleshooting can inspect OS status only after policy approval in this profile.".to_string()
        }
        (_, "app-builder", PolicyControlState::PermissionGated) => {
            "Goblins AI app creation requires OS sandbox and policy review before execution.".to_string()
        }
        (_, "agents", PolicyControlState::PermissionGated) => {
            "Agent workflows require explicit tool and data-boundary approval.".to_string()
        }
        (_, "enterprise-controls", PolicyControlState::Denied) => {
            "Enterprise controls are not available in the active policy profile.".to_string()
        }
        (_, _, PolicyControlState::Allowed) => {
            "Allowed by the active Goblins OS policy profile.".to_string()
        }
        (_, _, PolicyControlState::Denied) => {
            "Denied by the active Goblins OS policy profile.".to_string()
        }
        (_, _, PolicyControlState::PermissionGated) => {
            "Permission-gated by the active Goblins OS policy profile.".to_string()
        }
    }
}

fn permission_acknowledgement(control_id: &str, profile: PolicyProfile) -> String {
    format!(
        "GRANT GOBLINS OS PERMISSION {} FOR {}",
        control_id,
        profile.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        build_policy_status, control_state, parse_policy_profile, permission_acknowledgement,
        PolicyControlState, PolicyProfile,
    };

    #[test]
    fn default_policy_is_consumer() {
        let status = build_policy_status();

        assert_eq!(status.source, "goblins-os-core");
        assert!(status.state_path.contains("/var/lib/goblins-os/policy"));
        assert_eq!(status.profile, PolicyProfile::Consumer);
    }

    #[test]
    fn local_only_blocks_cloud_controls() {
        assert_eq!(
            control_state(PolicyProfile::LocalOnly, "cloud-openai"),
            PolicyControlState::Denied
        );
        assert_eq!(
            control_state(PolicyProfile::LocalOnly, "app-builder"),
            PolicyControlState::PermissionGated
        );
        assert_eq!(
            control_state(PolicyProfile::LocalOnly, "local-models"),
            PolicyControlState::Allowed
        );
    }

    #[test]
    fn permission_grants_require_stable_exact_acknowledgement() {
        assert_eq!(
            permission_acknowledgement("app-builder", PolicyProfile::LocalOnly),
            "GRANT GOBLINS OS PERMISSION app-builder FOR local-only"
        );
    }

    #[test]
    fn parses_stable_profile_names() {
        assert_eq!(
            parse_policy_profile("business"),
            Some(PolicyProfile::Business)
        );
        assert_eq!(
            parse_policy_profile("local-only"),
            Some(PolicyProfile::LocalOnly)
        );
        assert_eq!(parse_policy_profile("unknown"), None);
    }
}
