//! Trusted, single-dialog hosted-context consent UI.
//!
//! The setgid bootstrap consumes the broker-only core capability, drops its
//! elevated group, and atomically claims the one canonical review intended for
//! its kernel-authenticated desktop UID. It posts `approve` or `cancel` over
//! that protected socket. No requesting client, command-line value, stdin, or
//! user-owned session-bridge response can attest approval.

use std::{error::Error, time::Duration};

use goblins_os_core_client::{initialize, ClientKind, CoreClient};
use serde::Deserialize;

const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const REVIEW_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_SHORT_FIELD_CHARS: usize = 512;
const MAX_EXACT_CONTENT_CHARS: usize = 48 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostedContextReview {
    destination: String,
    action: String,
    context: String,
    content_label: String,
    exact_content: String,
    consequence: String,
}

impl HostedContextReview {
    fn valid(&self) -> bool {
        valid_short_field(&self.destination)
            && valid_short_field(&self.action)
            && valid_short_field(&self.context)
            && valid_short_field(&self.content_label)
            && valid_short_field(&self.consequence)
            && !self.exact_content.trim().is_empty()
            && self.exact_content.chars().count() <= MAX_EXACT_CONTENT_CHARS
    }
}

#[derive(Deserialize)]
struct ReviewResponse {
    ok: bool,
    lease_id: Option<String>,
    review: Option<HostedContextReview>,
}

#[derive(Deserialize)]
struct DecisionResponse {
    ok: bool,
}

fn valid_short_field(value: &str) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= MAX_SHORT_FIELD_CHARS
        && !value.chars().any(|character| character == '\0')
}

fn valid_lease_id(lease_id: &str) -> bool {
    lease_id.len() == 64
        && lease_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn render_proof_scheme() -> Option<&'static str> {
    std::env::args().find_map(|argument| match argument.as_str() {
        "--render-proof=light" => Some("light"),
        "--render-proof=dark" => Some("dark"),
        _ => None,
    })
}

fn render_proof_review() -> HostedContextReview {
    HostedContextReview {
        destination: "OpenAI hosted models".to_string(),
        action: "Review protected context".to_string(),
        context: "the exact content selected for this one request".to_string(),
        content_label: "Exact content leaving this device".to_string(),
        exact_content: "No personal context is loaded in visual proof mode. Real reviews show the exact content that would leave this device.".to_string(),
        consequence: "Visual proof only. Sharing is disabled and no decision can be submitted."
            .to_string(),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Capability bootstrap is deliberately the first action: the installed
    // setgid entrypoint authenticates the broker-only core socket, drops its
    // effective group, and re-execs the root-owned unprivileged payload before
    // arguments, environment, core review state, or GTK are inspected.
    let core = initialize(ClientKind::ConsentBroker)?;
    if std::env::args().any(|argument| argument == "--self-test") {
        println!("goblins_os_consent_broker_self_test=ready");
        return Ok(());
    }
    if let Some(scheme) = render_proof_scheme() {
        prepare_render_proof_environment(scheme);
        let _ = run_review(render_proof_review(), false);
        return Ok(());
    }

    // The protected broker capability atomically claims the core's sole
    // pending review bound to this kernel-authenticated desktop UID. No id,
    // content, lease, or review authority crossed the user-owned session bridge
    // that launched this fixed executable.
    let Some((lease_id, review)) = fetch_review(&core) else {
        return Ok(());
    };
    if !prepare_trusted_ui_path() {
        let _ = submit_decision(&core, &lease_id, "abort");
        return Ok(());
    }
    let decision = if run_review(review, true) {
        "approve"
    } else {
        "cancel"
    };
    let _ = submit_decision(&core, &lease_id, decision);
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn prepare_render_proof_environment(scheme: &str) {
    std::env::set_var("GOBLINS_OS_THEME", scheme);
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn prepare_render_proof_environment(_scheme: &str) {}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn prepare_trusted_ui_path() -> bool {
    use std::{
        fs,
        os::{
            fd::{AsRawFd as _, IntoRawFd as _},
            unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _},
        },
        path::PathBuf,
    };

    let uid = unsafe { libc::getuid() };
    if !active_wayland_session_for_uid(uid) {
        return false;
    }
    let runtime = PathBuf::from(format!("/run/user/{uid}"));
    let Ok(runtime_metadata) = fs::symlink_metadata(&runtime) else {
        return false;
    };
    if !runtime_metadata.is_dir()
        || runtime_metadata.file_type().is_symlink()
        || runtime_metadata.uid() != uid
        || runtime_metadata.permissions().mode() & 0o7777 != 0o700
    {
        return false;
    }
    let socket_path = runtime.join("wayland-0");
    let Ok(socket_metadata) = fs::symlink_metadata(&socket_path) else {
        return false;
    };
    if !socket_metadata.file_type().is_socket()
        || socket_metadata.file_type().is_symlink()
        || socket_metadata.uid() != uid
    {
        return false;
    }
    let Ok(stream) = std::os::unix::net::UnixStream::connect(&socket_path) else {
        return false;
    };
    let Some((peer_pid, peer_uid)) = unix_peer_credentials(stream.as_raw_fd()) else {
        return false;
    };
    if peer_uid != uid || !trusted_gnome_shell_peer(peer_pid) {
        return false;
    }

    // GTK/libwayland consumes this already-authenticated connection. No
    // launcher-supplied display name, X server, or nested compositor is used.
    let wayland_fd = stream.into_raw_fd();
    std::env::remove_var("DISPLAY");
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("XAUTHORITY");
    std::env::set_var("GDK_BACKEND", "wayland");
    std::env::set_var("XDG_RUNTIME_DIR", &runtime);
    std::env::set_var("WAYLAND_SOCKET", wayland_fd.to_string());
    true
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn active_wayland_session_for_uid(uid: u32) -> bool {
    use std::{
        fs,
        io::Read as _,
        os::unix::fs::{MetadataExt as _, PermissionsExt as _},
        path::Path,
    };

    let sessions = Path::new("/run/systemd/sessions");
    let Ok(directory_metadata) = fs::symlink_metadata(sessions) else {
        return false;
    };
    if !directory_metadata.is_dir()
        || directory_metadata.file_type().is_symlink()
        || directory_metadata.uid() != 0
        || directory_metadata.permissions().mode() & 0o022 != 0
    {
        return false;
    }
    let Ok(entries) = fs::read_dir(sessions) else {
        return false;
    };
    let mut matches = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.uid() != 0
            || metadata.permissions().mode() & 0o022 != 0
            || metadata.len() > 16 * 1024
        {
            continue;
        }
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        let mut text = String::new();
        if file.take(16 * 1024 + 1).read_to_string(&mut text).is_err() || text.len() > 16 * 1024 {
            continue;
        }
        let uid_matches =
            session_value(&text, "UID").and_then(|value| value.parse::<u32>().ok()) == Some(uid);
        if uid_matches
            && session_value(&text, "ACTIVE") == Some("1")
            && session_value(&text, "STATE") == Some("active")
            && session_value(&text, "TYPE") == Some("wayland")
            && session_value(&text, "REMOTE") != Some("1")
        {
            matches += 1;
        }
    }
    matches == 1
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn session_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        line.split_once('=')
            .filter(|(candidate, _)| *candidate == key)
            .map(|(_, value)| value)
    })
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn unix_peer_credentials(fd: libc::c_int) -> Option<(u32, u32)> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials and length are valid writable storage and fd is a
    // connected Unix stream owned by this process.
    let status = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if status != 0 || length as usize != std::mem::size_of::<libc::ucred>() || credentials.pid <= 0
    {
        return None;
    }
    Some((credentials.pid as u32, credentials.uid))
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn trusted_gnome_shell_peer(pid: u32) -> bool {
    use std::{
        fs,
        os::unix::fs::{MetadataExt as _, PermissionsExt as _},
        path::{Path, PathBuf},
    };

    let executable = PathBuf::from(format!("/proc/{pid}/exe"));
    let Ok(target) = fs::read_link(executable) else {
        return false;
    };
    if target != Path::new("/usr/bin/gnome-shell") {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(&target) else {
        return false;
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o022 != 0
    {
        return false;
    }
    let cgroup = fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
    trusted_gnome_shell_cgroup(&cgroup)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn trusted_gnome_shell_cgroup(cgroup: &str) -> bool {
    cgroup.lines().any(|line| {
        let Some((_, path)) = line.rsplit_once(':') else {
            return false;
        };
        let Some(unit) = path.rsplit('/').next() else {
            return false;
        };
        matches!(
            unit,
            "org.gnome.Shell@wayland.service" | "org.gnome.Shell@user.service"
        ) || (path.contains("/app.slice/")
            && generated_gnome_shell_scope(unit, "org.gnome.Shell@wayland.service"))
            || (path.contains("/app.slice/")
                && generated_gnome_shell_scope(unit, "org.gnome.Shell@user.service"))
    })
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn generated_gnome_shell_scope(unit: &str, service: &str) -> bool {
    let Some(instance) = unit
        .strip_prefix("app-gnome-")
        .and_then(|unit| unit.strip_prefix(service))
        .and_then(|unit| unit.strip_prefix('-'))
        .and_then(|unit| unit.strip_suffix(".scope"))
    else {
        return false;
    };
    !instance.is_empty() && instance.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn prepare_trusted_ui_path() -> bool {
    false
}

fn fetch_review(core: &CoreClient) -> Option<(String, HostedContextReview)> {
    let body = serde_json::json!({}).to_string();
    let response = core
        .post_json("/v1/consent/review", body.as_bytes(), CONTROL_TIMEOUT)
        .ok()?;
    if !response.is_success() {
        return None;
    }
    let response = serde_json::from_slice::<ReviewResponse>(&response.body).ok()?;
    let lease_id = response.ok.then_some(response.lease_id).flatten()?;
    if !valid_lease_id(&lease_id) {
        return None;
    }
    let review = response.ok.then_some(response.review).flatten()?;
    review.valid().then_some((lease_id, review))
}

fn submit_decision(core: &CoreClient, lease_id: &str, decision: &str) -> bool {
    if !matches!(decision, "approve" | "cancel" | "abort") {
        return false;
    }
    let body = serde_json::json!({
        "lease_id": lease_id,
        "decision": decision,
    })
    .to_string();
    let Ok(response) = core.post_json("/v1/consent/decision", body.as_bytes(), CONTROL_TIMEOUT)
    else {
        return false;
    };
    response.is_success()
        && serde_json::from_slice::<DecisionResponse>(&response.body)
            .is_ok_and(|outcome| outcome.ok)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_review(review: HostedContextReview, approval_enabled: bool) -> bool {
    use std::{cell::Cell, rc::Rc};

    use gtk::prelude::*;
    use gtk4 as gtk;

    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.ConsentBroker")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let approved = Rc::new(Cell::new(false));
    let result = approved.clone();
    application.connect_activate(move |application| {
        goblins_os_ui::init_theming(CSS);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 18);
        root.add_css_class("gos-consent-root");
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(24);
        root.set_margin_end(24);

        let eyebrow_text = if approval_enabled {
            "HOSTED AI REVIEW"
        } else {
            "VISUAL PROOF · SHARING DISABLED"
        };
        let eyebrow = gtk::Label::new(Some(eyebrow_text));
        eyebrow.add_css_class("gos-consent-eyebrow");
        eyebrow.set_xalign(0.0);
        root.append(&eyebrow);

        let title = gtk::Label::new(Some("Share this context?"));
        title.add_css_class("gos-consent-title");
        title.set_xalign(0.0);
        title.set_wrap(true);
        root.append(&title);

        root.append(&detail_row("Destination", &review.destination));
        root.append(&detail_row("Action", &review.action));
        root.append(&detail_row("Included", &review.context));

        let exact_label = gtk::Label::new(Some(&review.content_label));
        exact_label.add_css_class("gos-consent-section-label");
        exact_label.set_xalign(0.0);
        root.append(&exact_label);

        let exact = gtk::TextView::new();
        exact.add_css_class("gos-consent-exact");
        exact.set_editable(false);
        exact.set_cursor_visible(false);
        exact.set_wrap_mode(gtk::WrapMode::WordChar);
        exact.buffer().set_text(&review.exact_content);
        exact.update_property(&[gtk::accessible::Property::Label(
            review.content_label.as_str(),
        )]);
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(180)
            .max_content_height(300)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&exact)
            .build();
        scroll.add_css_class("gos-consent-scroll");
        root.append(&scroll);

        let consequence = gtk::Label::new(Some(&review.consequence));
        consequence.add_css_class("gos-consent-consequence");
        consequence.set_xalign(0.0);
        consequence.set_wrap(true);
        root.append(&consequence);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        actions.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Cancel");
        cancel.update_property(&[gtk::accessible::Property::Label("Cancel without sharing")]);
        let share = gtk::Button::with_label("Share once");
        share.add_css_class("suggested-action");
        share.set_sensitive(approval_enabled);
        share.update_property(&[gtk::accessible::Property::Label(
            "Share this reviewed context once",
        )]);
        actions.append(&cancel);
        actions.append(&share);
        root.append(&actions);

        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("Goblins OS Hosted AI Review")
            .default_width(620)
            .default_height(640)
            .resizable(true)
            .child(&root)
            .build();
        window.set_modal(true);
        window.set_default_widget(Some(&cancel));
        cancel.grab_focus();

        let cancel_window = window.clone();
        cancel.connect_clicked(move |_| cancel_window.close());
        if approval_enabled {
            let share_window = window.clone();
            let approved = approved.clone();
            share.connect_clicked(move |_| {
                approved.set(true);
                share_window.close();
            });
        }

        let escape_window = window.clone();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                escape_window.close();
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        window.add_controller(keys);

        let timeout_window = window.clone();
        gtk::glib::timeout_add_local_once(REVIEW_TIMEOUT, move || timeout_window.close());
        window.present();
    });
    application.run_with_args::<&str>(&[]);
    result.get()
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn detail_row(label: &str, value: &str) -> gtk4::Box {
    use gtk4::prelude::*;

    let row = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    let key = gtk4::Label::new(Some(label));
    key.add_css_class("gos-consent-detail-key");
    key.set_xalign(0.0);
    let value = gtk4::Label::new(Some(value));
    value.add_css_class("gos-consent-detail-value");
    value.set_xalign(0.0);
    value.set_wrap(true);
    row.append(&key);
    row.append(&value);
    row
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_review(_review: HostedContextReview, _approval_enabled: bool) -> bool {
    false
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const CSS: &str = r#"
.gos-consent-root {
  background: @gos_surface;
}

.gos-consent-eyebrow,
.gos-consent-detail-key,
.gos-consent-section-label {
  color: @gos_ink_muted;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.06em;
}

.gos-consent-title {
  color: @gos_ink;
  font-size: 28px;
  font-weight: 750;
}

.gos-consent-detail-value {
  color: @gos_ink;
  font-size: 14px;
}

.gos-consent-scroll {
  border: 1px solid alpha(@gos_ink, 0.14);
  border-radius: 12px;
  background: @gos_surface_sunken;
}

.gos-consent-exact {
  padding: 14px;
  color: @gos_ink;
  background: transparent;
  font-size: 13px;
}

.gos-consent-consequence {
  color: @gos_ink_secondary;
  font-size: 13px;
}
"#;

#[cfg(test)]
mod tests {
    use super::{
        render_proof_review, valid_lease_id, valid_short_field, HostedContextReview,
        MAX_EXACT_CONTENT_CHARS,
    };

    #[test]
    fn review_requires_bounded_nonempty_exact_content() {
        let valid = HostedContextReview {
            destination: "OpenAI through Codex".to_string(),
            action: "Ask Goblins AI".to_string(),
            context: "the selected text".to_string(),
            content_label: "Exact content leaving this device".to_string(),
            exact_content: "exact selected text".to_string(),
            consequence: "This exact content will leave this device once.".to_string(),
        };
        assert!(valid.valid());
        assert!(!HostedContextReview {
            exact_content: " ".to_string(),
            ..valid
        }
        .valid());
        assert!(!HostedContextReview {
            destination: "destination".to_string(),
            action: "action".to_string(),
            context: "context".to_string(),
            content_label: "content".to_string(),
            exact_content: "x".repeat(MAX_EXACT_CONTENT_CHARS + 1),
            consequence: "consequence".to_string(),
        }
        .valid());
        assert!(!valid_short_field("\0"));
        assert!(valid_lease_id(&"a".repeat(64)));
        assert!(!valid_lease_id(&"A".repeat(64)));
    }

    #[test]
    fn render_proof_is_decision_incapable_and_contains_no_personal_fixture() {
        let review = render_proof_review();
        assert!(review.valid());
        assert!(review
            .exact_content
            .contains("No personal context is loaded"));
        assert!(review.consequence.contains("no decision can be submitted"));
    }

    #[test]
    fn capability_initialization_precedes_args_review_state_and_gtk() {
        let source = include_str!("main.rs");
        let initialize = source
            .find("let core = initialize(ClientKind::ConsentBroker)?;")
            .expect("capability initialization");
        for later in [
            "std::env::args().any",
            "fetch_review(&core)",
            "prepare_trusted_ui_path()",
            "run_review(review, true)",
        ] {
            assert!(initialize < source.find(later).expect("ordered broker action"));
        }
    }

    #[test]
    fn claimed_review_preflight_failure_aborts_immediately() {
        let source = include_str!("main.rs");
        let fetch = source
            .find("let Some((lease_id, review)) = fetch_review(&core)")
            .expect("protected review claim");
        let preflight = source
            .find("if !prepare_trusted_ui_path()")
            .expect("trusted display preflight");
        let abort = source
            .find("submit_decision(&core, &lease_id, \"abort\")")
            .expect("immediate abort");
        assert!(fetch < preflight && preflight < abort);
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn gnome_shell_cgroup_accepts_shipped_units_and_known_app_scope_shape() {
        assert!(super::trusted_gnome_shell_cgroup(
            "0::/user.slice/user-1000.slice/user@1000.service/session.slice/org.gnome.Shell@wayland.service\n"
        ));
        assert!(super::trusted_gnome_shell_cgroup(
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.Shell@wayland.service-1827.scope\n"
        ));
        assert!(super::trusted_gnome_shell_cgroup(
            "0::/user.slice/user-1000.slice/user@1000.service/session.slice/org.gnome.Shell@user.service\n"
        ));
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn gnome_shell_cgroup_rejects_lookalike_units_and_substrings() {
        for cgroup in [
            "0::/user.slice/org.gnome.Shell@wayland.service.evil\n",
            "0::/user.slice/evil-org.gnome.Shell@wayland.service\n",
            "0::/user.slice/app.slice/app-gnome-org.gnome.Shell@wayland.service-evil.scope\n",
            "0::/user.slice/session.slice/not-org.gnome.Shell@user.service\n",
        ] {
            assert!(!super::trusted_gnome_shell_cgroup(cgroup), "{cgroup}");
        }
    }
}
