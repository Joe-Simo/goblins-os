//! App-from-intent: Goblins OS has no pre-installed apps. The user describes the
//! app they need; the explicitly selected Goblins AI engine designs it; the OS
//! persists it as an OS-owned app record and lists it so it can be opened and
//! re-built ("edited") later.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ai::{audit_ai_action, AiActionOutcome},
    policy::{policy_state_for_control, PolicyControlState},
};

const DEFAULT_APPS_DIR: &str = "/var/lib/goblins-os/apps";
const MAX_INTENT_CHARS: usize = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuilderStatus {
    Local,
    ServerGated,
    NotConfigured,
    PermissionGated,
    Blocked,
}

#[derive(Serialize)]
pub struct AppBuildSurface {
    id: &'static str,
    name: &'static str,
    role: &'static str,
    status: BuilderStatus,
}

#[derive(Serialize)]
pub struct AppBuilderCatalog {
    model: &'static str,
    builder: BuilderStatus,
    surfaces: Vec<AppBuildSurface>,
}

#[derive(Deserialize)]
pub struct AppBuildRequest {
    intent: String,
}

/// An OS-owned application built from intent by the Goblins AI runtime. In Goblins OS
/// an "app" is this generated, persisted definition — not an installed binary.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct BuiltApp {
    id: String,
    name: String,
    intent: String,
    plan: String,
    source: String,
    created_at: String,
}

#[derive(Serialize)]
pub struct AppList {
    model: &'static str,
    count: usize,
    apps: Vec<BuiltApp>,
}

pub async fn app_builder_catalog() -> Json<AppBuilderCatalog> {
    let policy = policy_state_for_control("app-builder");
    let builder = builder_status_for_policy(policy, crate::resident::active_engine_locality());

    Json(AppBuilderCatalog {
        model: "gpt-oss-builds-apps-not-installs",
        surfaces: vec![
            AppBuildSurface {
                id: "intent-builder",
                name: "Build from intent",
                role:
                    "Describe an app; the active Goblins AI runtime designs it and the OS owns it",
                status: builder,
            },
            AppBuildSurface {
                id: "app-library",
                name: "Local app library",
                role: "Built apps are persisted under the OS state directory and listed for review and launch",
                status: BuilderStatus::Local,
            },
            AppBuildSurface {
                id: "policy-gates",
                name: "Policy gates",
                role: "Building is gated by the active Goblins OS policy profile and permissions",
                status: BuilderStatus::Local,
            },
            AppBuildSurface {
                id: "external-catalog",
                name: "External software catalog",
                role: "Not part of Goblins OS; apps are built from intent and local source",
                status: BuilderStatus::Blocked,
            },
        ],
        builder,
    })
}

pub async fn list_apps() -> Json<AppList> {
    let apps = list_apps_from(&apps_dir());
    Json(AppList {
        model: "gpt-oss-builds-apps-not-installs",
        count: apps.len(),
        apps,
    })
}

/// Designing the app plan is a model turn through the explicitly selected
/// resident engine, so the body runs on the blocking pool instead of pinning an
/// async runtime worker.
pub async fn create_app_build(
    Json(payload): Json<AppBuildRequest>,
) -> (StatusCode, Json<BuildOutcome>) {
    crate::bounded::run_blocking(move || create_app_build_blocking(payload))
        .await
        .unwrap_or_else(|_| {
            outcome(
                StatusCode::TOO_MANY_REQUESTS,
                crate::bounded::LONG_OPERATION_BUSY_MESSAGE.to_string(),
                None,
            )
        })
}

fn create_app_build_blocking(payload: AppBuildRequest) -> (StatusCode, Json<BuildOutcome>) {
    let intent = payload.intent.trim();
    if intent.is_empty() || intent.chars().count() > MAX_INTENT_CHARS {
        return outcome(
            StatusCode::BAD_REQUEST,
            format!("App intent must be between 1 and {MAX_INTENT_CHARS} characters."),
            None,
        );
    }

    if let Err(detail) = authorize_app_builder("launcher") {
        return outcome(StatusCode::FORBIDDEN, detail.to_string(), None);
    }

    let (plan, source) = match design_app_plan(intent) {
        Ok(plan) => plan,
        Err(detail) => {
            audit_ai_action("build-app", Some("launcher"), AiActionOutcome::Blocked);
            return outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Goblins OS could not design the app: {detail}."),
                None,
            );
        }
    };

    let app = build_app_record(intent, &plan, source);
    if write_app_to(&apps_dir(), &app).is_err() {
        audit_ai_action("build-app", Some("launcher"), AiActionOutcome::Failed);
        return outcome(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The built app could not be written to the local app library.".to_string(),
            None,
        );
    }

    audit_ai_action("build-app", Some("launcher"), AiActionOutcome::Succeeded);
    outcome(
        StatusCode::OK,
        format!(
            "Built \"{}\" from intent and added it to Goblins OS.",
            app.name
        ),
        Some(app),
    )
}

/// One authoritative authorization guard for every surface that can ask an AI
/// engine to create app files. It audits the attempted entry point and returns
/// before any model call, session mutation, or workspace write.
pub(crate) fn authorize_app_builder(entrypoint: &str) -> Result<(), &'static str> {
    authorize_app_builder_for_state(policy_state_for_control("app-builder"), entrypoint)
}

pub(crate) fn authorize_app_builder_for_state(
    state: PolicyControlState,
    entrypoint: &str,
) -> Result<(), &'static str> {
    let Some((audit_outcome, detail)) = app_builder_policy_denial(state) else {
        return Ok(());
    };
    audit_ai_action("build-app", Some(entrypoint), audit_outcome);
    Err(detail)
}

fn app_builder_policy_denial(state: PolicyControlState) -> Option<(AiActionOutcome, &'static str)> {
    match state {
        PolicyControlState::Allowed => None,
        PolicyControlState::Denied => Some((
            AiActionOutcome::Denied,
            "App building is blocked by the active Goblins OS policy profile.",
        )),
        PolicyControlState::PermissionGated => Some((
            AiActionOutcome::PermissionGated,
            "App building requires an explicit Goblins OS permission review first.",
        )),
    }
}

#[derive(Serialize)]
pub struct BuildOutcome {
    ok: bool,
    text: String,
    app: Option<BuiltApp>,
}

fn outcome(
    status: StatusCode,
    text: String,
    app: Option<BuiltApp>,
) -> (StatusCode, Json<BuildOutcome>) {
    (
        status,
        Json(BuildOutcome {
            ok: app.is_some(),
            text,
            app,
        }),
    )
}

fn build_prompt(intent: &str) -> String {
    format!(
        "You are the app builder for Goblins OS, a Fedora bootc Linux OS whose apps are \
         generated from intent rather than installed. Design a single, focused application for \
         this user intent. Reply with a short, concrete plan: the app name on the first line, \
         then what it does, its main screens and actions, and how the active Goblins AI runtime \
         powers it. Keep it practical and calm.\n\nUser intent: {intent}"
    )
}

fn design_app_plan(intent: &str) -> Result<(String, &'static str), &'static str> {
    crate::resident::resident_generate_with_engine(&build_prompt(intent))
}

fn build_app_record(intent: &str, plan: &str, source: &'static str) -> BuiltApp {
    BuiltApp {
        id: app_id(intent),
        name: display_name(plan, intent),
        intent: intent.to_string(),
        plan: plan.trim().to_string(),
        source: format!("{source}-built"),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs().to_string())
            .unwrap_or_default(),
    }
}

/// A stable id derived from the intent, so re-building the same intent updates the
/// same app ("edit") rather than spawning duplicates.
pub(crate) fn app_id(intent: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(intent.trim().as_bytes()));
    let slug = slugify(&derive_app_name(intent));
    let slug = if slug.is_empty() {
        "app".to_string()
    } else {
        slug
    };
    format!("{slug}-{}", &digest[..8])
}

pub(crate) fn derive_app_name(intent: &str) -> String {
    let first_line = intent.trim().lines().next().unwrap_or("").trim();
    let name: String = first_line.chars().take(60).collect();
    let name = name.trim_end_matches(['.', ',', '!', '?']).trim();
    if name.is_empty() {
        "Untitled app".to_string()
    } else {
        name.to_string()
    }
}

/// The app's *display* name. The builder prompt asks the model to put the app name
/// on the plan's first line, so prefer that (stripped of Markdown ** / # chrome) —
/// that is how the user gets "PomodoroFocus" instead of a slice of their prompt.
/// Falls back to the first line of the intent. Always word-boundary-ellipsized so a
/// name never hard-cuts mid-word, and the first letter is capitalized.
fn display_name(plan: &str, intent: &str) -> String {
    let first = plan
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let cleaned = first
        .trim_matches(|c: char| c == '#' || c == '*' || c.is_whitespace())
        .trim();
    let candidate = if !cleaned.is_empty() && cleaned.chars().count() <= 48 {
        cleaned.to_string()
    } else {
        let line = intent.trim().lines().next().unwrap_or("").trim();
        if line.is_empty() {
            "Untitled app".to_string()
        } else {
            line.to_string()
        }
    };
    capitalize_first(&ellipsize_words(candidate.trim(), 48))
}

/// Truncate to at most `max` chars on a word boundary with an ellipsis — never a
/// mid-word slice. Short strings pass through unchanged.
fn ellipsize_words(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let head: String = value.chars().take(max).collect();
    let cut = match head.rfind(' ') {
        Some(idx) if idx >= max / 2 => &head[..idx],
        _ => head.trim_end(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '-', ' ']))
}

fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => value.to_string(),
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = true; // avoid leading dash
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').chars().take(48).collect()
}

pub(crate) fn apps_dir() -> PathBuf {
    env::var("GOBLINS_OS_APPS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_APPS_DIR).to_path_buf())
}

fn write_app_to(dir: &Path, app: &BuiltApp) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", app.id));
    let tmp = dir.join(format!("{}.json.tmp", app.id));
    fs::write(&tmp, serde_json::to_vec_pretty(app)?)?;
    fs::rename(tmp, path)
}

fn list_apps_from(dir: &Path) -> Vec<BuiltApp> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut apps: Vec<BuiltApp> = entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<BuiltApp>(&bytes).ok())
        .collect();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

fn builder_status_for_policy(
    policy: PolicyControlState,
    locality: Option<crate::resident::EngineLocality>,
) -> BuilderStatus {
    match policy {
        PolicyControlState::Allowed => match locality {
            Some(crate::resident::EngineLocality::OnDevice) => BuilderStatus::Local,
            Some(crate::resident::EngineLocality::Cloud) => BuilderStatus::ServerGated,
            None => BuilderStatus::NotConfigured,
        },
        PolicyControlState::PermissionGated => BuilderStatus::PermissionGated,
        PolicyControlState::Denied => BuilderStatus::Blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        app_builder_policy_denial, app_id, build_app_record, build_prompt,
        builder_status_for_policy, derive_app_name, display_name, list_apps_from, slugify,
        write_app_to, BuilderStatus,
    };

    #[test]
    fn display_name_prefers_model_title_and_never_mid_slices() {
        // The model puts the app name on the plan's first line (often bold markdown).
        assert_eq!(
            display_name(
                "**PomodoroFocus**\n\nA calm timer.",
                "a quiet timer that logs sessions"
            ),
            "PomodoroFocus"
        );
        assert_eq!(
            display_name("## Pomodoro Flow\nwhat it does", "x"),
            "Pomodoro Flow"
        );
        // Empty/garbage plan -> fall back to the intent, capitalized.
        assert_eq!(display_name("   \n  ", "budget tracker"), "Budget tracker");
        // A long fallback ellipsizes on a word boundary — never a mid-word cut.
        let got = display_name(
            "",
            "a quiet pomodoro timer that logs each session and shows a weekly streak",
        );
        assert!(got.ends_with('…'), "long names get an ellipsis: {got}");
        assert!(got.chars().count() <= 49, "kept within the cap: {got}");
        assert!(
            !got.contains(" we…") && !got.contains("strea"),
            "no mid-word slice: {got}"
        );
    }
    use crate::policy::PolicyControlState;
    use std::path::PathBuf;

    fn unique_tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    #[test]
    fn builder_surface_reflects_os_policy_gate() {
        assert_eq!(
            builder_status_for_policy(
                PolicyControlState::Allowed,
                Some(crate::resident::EngineLocality::OnDevice),
            ),
            BuilderStatus::Local
        );
        assert_eq!(
            builder_status_for_policy(
                PolicyControlState::Allowed,
                Some(crate::resident::EngineLocality::Cloud),
            ),
            BuilderStatus::ServerGated
        );
        assert_eq!(
            builder_status_for_policy(PolicyControlState::Allowed, None),
            BuilderStatus::NotConfigured
        );
        assert_eq!(
            builder_status_for_policy(PolicyControlState::PermissionGated, None),
            BuilderStatus::PermissionGated
        );
        assert_eq!(
            builder_status_for_policy(PolicyControlState::Denied, None),
            BuilderStatus::Blocked
        );
    }

    #[test]
    fn every_build_surface_uses_the_same_fail_closed_policy_contract() {
        assert!(app_builder_policy_denial(PolicyControlState::Allowed).is_none());

        let (denied_outcome, denied_copy) =
            app_builder_policy_denial(PolicyControlState::Denied).expect("denied gate");
        assert_eq!(denied_outcome, crate::ai::AiActionOutcome::Denied);
        assert!(denied_copy.contains("blocked"));

        let (gated_outcome, gated_copy) =
            app_builder_policy_denial(PolicyControlState::PermissionGated)
                .expect("permission gate");
        assert_eq!(gated_outcome, crate::ai::AiActionOutcome::PermissionGated);
        assert!(gated_copy.contains("explicit"));
    }

    #[test]
    fn names_and_slugs_are_derived_cleanly() {
        assert_eq!(
            derive_app_name("A timer for steeping tea.\nmore detail"),
            "A timer for steeping tea"
        );
        assert_eq!(derive_app_name("   "), "Untitled app");
        assert_eq!(
            slugify("A Timer for Steeping Tea!"),
            "a-timer-for-steeping-tea"
        );
        assert_eq!(slugify("***"), "");
    }

    #[test]
    fn app_id_is_stable_per_intent_so_rebuild_edits_in_place() {
        let a = app_id("a notes app with tags");
        let b = app_id("a notes app with tags");
        let c = app_id("a budgeting app");
        assert_eq!(a, b, "the same intent must map to the same app id");
        assert_ne!(a, c, "different intents must map to different app ids");
        assert!(a.starts_with("a-notes-app-with-tags-"));
    }

    #[test]
    fn built_apps_persist_to_and_list_from_the_os_store() {
        let dir = unique_tmp("goblins-os-apps");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(list_apps_from(&dir).is_empty());

        let app = build_app_record("a notes app with tags", "Notes\nKeep tagged notes.", "test");
        write_app_to(&dir, &app).expect("write built app");
        // Re-building the same intent updates the same record (no duplicate).
        let edited = build_app_record(
            "a notes app with tags",
            "Notes v2\nNow with search.",
            "test",
        );
        write_app_to(&dir, &edited).expect("rebuild edits in place");

        let listed = list_apps_from(&dir);
        assert_eq!(
            listed.len(),
            1,
            "rebuilding the same intent must not duplicate"
        );
        assert_eq!(listed[0].id, app.id);
        assert_eq!(listed[0].plan, "Notes v2\nNow with search.");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_prompt_keeps_goblins_product_framing() {
        let prompt = build_prompt("a reminders app");
        assert!(prompt.contains("Goblins OS"));
        assert!(prompt.contains("Fedora bootc Linux OS"));
        let old_product_frame = ["OpenAI-centered", "Linux OS"].join(" ");
        assert!(!prompt.contains(&old_product_frame));
    }
}
