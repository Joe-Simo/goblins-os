//! Trusted, single-purpose OpenAI API-key entry surface.
//!
//! Settings can request that this fixed executable be launched, but it never
//! receives a user ID, lease, credential, or operation from the launcher. The
//! setgid bootstrap consumes the broker-only core capability first, permanently
//! drops that group, enables no-new-privileges, and re-executes this root-owned
//! payload. The broker then atomically claims the sole pending operation for its
//! kernel-authenticated UID. Plaintext exists only in this process and is piped
//! directly to `systemd-creds`; only bounded encrypted ciphertext reaches core.

use std::{
    error::Error,
    io,
    io::{Read, Write},
    os::{fd::AsRawFd as _, unix::process::CommandExt as _},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use goblins_os_core_client::{initialize, ClientKind, CoreClient};
use serde::Deserialize;
use zeroize::Zeroizing;

const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const ENCRYPT_TIMEOUT: Duration = Duration::from_secs(15);
const COMMAND_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(20);
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const DIALOG_TIMEOUT: Duration = Duration::from_secs(180);
const SYSTEMD_CREDS: &str = "/usr/bin/systemd-creds";
const CREDENTIAL_NAME_ARG: &str = "--name=goblins.openai-api-key.v1";
const MAX_PLAINTEXT_BYTES: usize = 512;
const MAX_ENCRYPTED_CREDENTIAL_BYTES: usize = 16 * 1024;
const SYSTEMD_CREDS_ARGS: &[&str] = &[
    "--no-ask-password",
    "--quiet",
    "--user",
    "--uid=self",
    "--with-key=auto",
    CREDENTIAL_NAME_ARG,
    "encrypt",
    "-",
    "-",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyAction {
    Add,
    Rotate,
    Remove,
}

impl KeyAction {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "add" => Some(Self::Add),
            "rotate" => Some(Self::Rotate),
            "remove" => Some(Self::Remove),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimResponse {
    ok: bool,
    lease_id: Option<String>,
    action: Option<String>,
    configured: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationResponse {
    ok: bool,
}

fn valid_lease_id(lease_id: &str) -> bool {
    lease_id.len() == 64
        && lease_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_plaintext(value: &[u8]) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PLAINTEXT_BYTES
        && value
            .iter()
            .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
}

fn valid_encrypted_credential(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ENCRYPTED_CREDENTIAL_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

fn main() -> Result<(), Box<dyn Error>> {
    // This must remain the first action. Arguments, environment, pending state,
    // GTK, and systemd credential tooling are all untrusted until the dedicated
    // capability has been consumed and the unprivileged payload is running.
    let core = initialize(ClientKind::OpenAiKeyBroker)?;
    if std::env::args().any(|argument| argument == "--self-test") {
        println!("goblins_os_openai_key_broker_self_test=ready");
        return Ok(());
    }

    let Some((lease_id, action, configured)) = claim_operation(&core) else {
        return Ok(());
    };
    if !prepare_trusted_ui_path() {
        let _ = submit_decision(&core, &lease_id, "abort");
        return Ok(());
    }

    match action {
        KeyAction::Add | KeyAction::Rotate => {
            let Some(plaintext) = run_key_entry(action, configured, true) else {
                let _ = submit_decision(&core, &lease_id, "cancel");
                return Ok(());
            };
            let Some(encrypted_credential) = encrypt_credential(plaintext) else {
                let _ = show_storage_failure();
                let _ = submit_decision(&core, &lease_id, "abort");
                return Ok(());
            };
            if !commit_encrypted(&core, &lease_id, &encrypted_credential) {
                let _ = show_commit_failure();
                let _ = submit_decision(&core, &lease_id, "abort");
            }
        }
        KeyAction::Remove => {
            let decision = if run_remove_confirmation(configured) {
                "remove"
            } else {
                "cancel"
            };
            if !submit_decision(&core, &lease_id, decision) && decision == "remove" {
                let _ = show_remove_failure();
            }
        }
    }
    Ok(())
}

fn claim_operation(core: &CoreClient) -> Option<(String, KeyAction, bool)> {
    let response = core
        .post_json("/v1/openai-key-broker/claim", b"{}", CONTROL_TIMEOUT)
        .ok()?;
    if !response.is_success() {
        return None;
    }
    let response = serde_json::from_slice::<ClaimResponse>(&response.body).ok()?;
    let lease_id = response.ok.then_some(response.lease_id).flatten()?;
    let action = response
        .ok
        .then_some(response.action)
        .flatten()
        .and_then(|action| KeyAction::parse(&action))?;
    let configured = response.ok.then_some(response.configured).flatten()?;
    if !valid_lease_id(&lease_id)
        || matches!(action, KeyAction::Add) == configured
        || matches!(action, KeyAction::Rotate | KeyAction::Remove) != configured
    {
        return None;
    }
    Some((lease_id, action, configured))
}

fn commit_encrypted(core: &CoreClient, lease_id: &str, encrypted_credential: &str) -> bool {
    if !valid_lease_id(lease_id) || !valid_encrypted_credential(encrypted_credential) {
        return false;
    }
    let body = serde_json::json!({
        "lease_id": lease_id,
        "encrypted_credential": encrypted_credential,
    })
    .to_string();
    let Ok(response) = core.post_json(
        "/v1/openai-key-broker/commit",
        body.as_bytes(),
        CONTROL_TIMEOUT,
    ) else {
        return false;
    };
    response.is_success()
        && serde_json::from_slice::<MutationResponse>(&response.body)
            .is_ok_and(|outcome| outcome.ok)
}

fn submit_decision(core: &CoreClient, lease_id: &str, decision: &str) -> bool {
    if !valid_lease_id(lease_id) || !matches!(decision, "cancel" | "abort" | "remove") {
        return false;
    }
    let body = serde_json::json!({
        "lease_id": lease_id,
        "decision": decision,
    })
    .to_string();
    let Ok(response) = core.post_json(
        "/v1/openai-key-broker/decision",
        body.as_bytes(),
        CONTROL_TIMEOUT,
    ) else {
        return false;
    };
    response.is_success()
        && serde_json::from_slice::<MutationResponse>(&response.body)
            .is_ok_and(|outcome| outcome.ok)
}

fn encrypt_credential(plaintext: Zeroizing<Vec<u8>>) -> Option<String> {
    if !valid_plaintext(&plaintext) {
        return None;
    }

    let output = sensitive_command_output(
        SYSTEMD_CREDS,
        SYSTEMD_CREDS_ARGS,
        plaintext,
        MAX_ENCRYPTED_CREDENTIAL_BYTES,
        ENCRYPT_TIMEOUT,
    )?;
    let output = String::from_utf8(output.to_vec()).ok()?;
    let encrypted = output.trim();
    valid_encrypted_credential(encrypted).then(|| encrypted.to_string())
}

fn sensitive_command_output(
    binary: &str,
    args: &[&str],
    input: Zeroizing<Vec<u8>>,
    output_limit: usize,
    timeout: Duration,
) -> Option<Zeroizing<Vec<u8>>> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // A successful systemd-creds process can still leave a descendant with
        // a copy of stdout. Give the utility an isolated process group so every
        // terminal path can close the complete secret-bearing pipe topology.
        .process_group(0);
    let mut child = command.spawn().ok()?;
    let process_group = child.id();
    let mut stdin = Some(child.stdin.take()?);
    let mut stdout = child.stdout.take()?;
    if set_nonblocking(stdin.as_ref()?.as_raw_fd()).is_err()
        || set_nonblocking(stdout.as_raw_fd()).is_err()
    {
        terminate_process_group(&mut child, process_group);
        return None;
    }

    let mut input_offset = 0;
    let mut output = Zeroizing::new(Vec::with_capacity(output_limit));
    let started = Instant::now();
    loop {
        if pump_stdin(&mut stdin, &input, &mut input_offset).is_err()
            || pump_stdout(&mut stdout, &mut output, output_limit).is_err()
        {
            drop(stdin);
            terminate_process_group(&mut child, process_group);
            return None;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                drop(stdin);
                terminate_process_group(&mut child, process_group);
                if !status.success() || input_offset != input.len() {
                    return None;
                }
                return drain_stdout_after_exit(
                    stdout,
                    output,
                    output_limit,
                    COMMAND_CLEANUP_TIMEOUT,
                );
            }
            Ok(None) if started.elapsed() < timeout => thread::sleep(COMMAND_POLL_INTERVAL),
            Ok(None) | Err(_) => {
                drop(stdin);
                terminate_process_group(&mut child, process_group);
                return None;
            }
        }
    }
}

fn set_nonblocking(descriptor: libc::c_int) -> io::Result<()> {
    // SAFETY: fcntl operates on an owned live pipe fd and dereferences no pointer.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn pump_stdin(stdin: &mut Option<ChildStdin>, input: &[u8], offset: &mut usize) -> io::Result<()> {
    while *offset < input.len() {
        let pipe = stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "credential pipe closed"))?;
        match pipe.write(&input[*offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "credential pipe closed",
                ))
            }
            Ok(written) => *offset += written,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error),
        }
    }
    if *offset == input.len() {
        stdin.take();
    }
    Ok(())
}

fn pump_stdout(
    stdout: &mut ChildStdout,
    output: &mut Zeroizing<Vec<u8>>,
    limit: usize,
) -> io::Result<bool> {
    let mut chunk = [0_u8; 4096];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => return Ok(true),
            Ok(read) => {
                let remaining = limit.saturating_sub(output.len());
                if read > remaining {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "credential command output exceeded its private bound",
                    ));
                }
                output.extend_from_slice(&chunk[..read]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error),
        }
    }
}

fn drain_stdout_after_exit(
    mut stdout: ChildStdout,
    mut output: Zeroizing<Vec<u8>>,
    limit: usize,
    timeout: Duration,
) -> Option<Zeroizing<Vec<u8>>> {
    let started = Instant::now();
    loop {
        match pump_stdout(&mut stdout, &mut output, limit) {
            Ok(true) => return Some(output),
            Ok(false) if started.elapsed() < timeout => thread::sleep(COMMAND_POLL_INTERVAL),
            Ok(false) | Err(_) => return None,
        }
    }
}

fn terminate_process_group(child: &mut Child, process_group: u32) {
    if let Ok(process_group) = libc::pid_t::try_from(process_group) {
        // SAFETY: a negative pid targets only the isolated child process group.
        unsafe {
            libc::kill(-process_group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if started.elapsed() < COMMAND_CLEANUP_TIMEOUT => {
                thread::sleep(COMMAND_POLL_INTERVAL);
            }
            Ok(None) | Err(_) => return,
        }
    }
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_key_entry(
    action: KeyAction,
    configured: bool,
    commit_enabled: bool,
) -> Option<Zeroizing<Vec<u8>>> {
    use std::{cell::RefCell, rc::Rc};

    use gtk::prelude::*;
    use gtk4 as gtk;

    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.OpenAIKeyBroker")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let result: Rc<RefCell<Option<Zeroizing<Vec<u8>>>>> = Rc::new(RefCell::new(None));
    let returned = result.clone();
    application.connect_activate(move |application| {
        goblins_os_ui::init_theming(CSS);
        let root = dialog_root();
        root.append(&eyebrow("PROTECTED CREDENTIAL ENTRY"));
        root.append(&heading(if action == KeyAction::Rotate {
            "Replace OpenAI API key"
        } else {
            "Add OpenAI API key"
        }));
        root.append(&body_copy(if configured {
            "Enter the replacement key. The existing encrypted key remains active unless the replacement is stored successfully."
        } else {
            "Enter your API key. It is encrypted for this user and this device before it reaches Goblins OS core."
        }));

        let entry = gtk::PasswordEntry::new();
        // A credential should never become screenshot-visible. Paste is
        // supported, but there is deliberately no reveal control.
        entry.set_show_peek_icon(false);
        entry.set_placeholder_text(Some("API key"));
        entry.set_max_length(MAX_PLAINTEXT_BYTES as i32);
        entry.add_css_class("gos-key-broker-entry");
        entry.update_property(&[
            gtk::accessible::Property::Label("OpenAI API key"),
            gtk::accessible::Property::Description(
                "Encrypted on this device. The key is never returned by Settings or status services.",
            ),
        ]);
        root.append(&entry);

        let feedback = gtk::Label::new(Some(
            "The key is used only after you explicitly select the hosted OpenAI engine.",
        ));
        feedback.add_css_class("gos-key-broker-detail");
        feedback.set_xalign(0.0);
        feedback.set_wrap(true);
        root.append(&feedback);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        actions.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Cancel");
        let save = gtk::Button::with_label(if action == KeyAction::Rotate {
            "Replace key"
        } else {
            "Encrypt and save"
        });
        save.add_css_class("suggested-action");
        save.set_sensitive(commit_enabled);
        actions.append(&cancel);
        actions.append(&save);
        root.append(&actions);

        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("Goblins OS Protected Credential")
            .default_width(560)
            .default_height(390)
            .resizable(true)
            .child(&root)
            .build();
        window.set_modal(true);
        window.set_default_widget(Some(&save));
        entry.grab_focus();

        let close_entry = entry.clone();
        window.connect_close_request(move |_| {
            close_entry.set_text("");
            gtk::glib::Propagation::Proceed
        });

        let cancel_entry = entry.clone();
        let cancel_window = window.clone();
        cancel.connect_clicked(move |_| {
            cancel_entry.set_text("");
            cancel_window.close();
        });

        if commit_enabled {
            let save_entry = entry.clone();
            let save_window = window.clone();
            let save_feedback = feedback.clone();
            let result = result.clone();
            save.connect_clicked(move |_| {
                let text = save_entry.text();
                let secret = Zeroizing::new(text.as_bytes().to_vec());
                drop(text);
                save_entry.set_text("");
                if !valid_plaintext(&secret) {
                    save_feedback.set_text(
                        "Enter a non-empty key without spaces or control characters.",
                    );
                    return;
                }
                *result.borrow_mut() = Some(secret);
                save_window.close();
            });
        }

        install_escape_to_clear(&window, &entry);
        let timeout_entry = entry.clone();
        let timeout_window = window.clone();
        gtk::glib::timeout_add_local_once(DIALOG_TIMEOUT, move || {
            timeout_entry.set_text("");
            timeout_window.close();
        });
        window.present();
    });
    application.run_with_args::<&str>(&[]);
    returned.borrow_mut().take()
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_key_entry(
    _action: KeyAction,
    _configured: bool,
    _commit_enabled: bool,
) -> Option<Zeroizing<Vec<u8>>> {
    None
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_remove_confirmation(configured: bool) -> bool {
    use std::{cell::Cell, rc::Rc};

    use gtk::prelude::*;
    use gtk4 as gtk;

    if !configured {
        return false;
    }
    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.OpenAIKeyBroker")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let confirmed = Rc::new(Cell::new(false));
    let returned = confirmed.clone();
    application.connect_activate(move |application| {
        goblins_os_ui::init_theming(CSS);
        let root = dialog_root();
        root.append(&eyebrow("PROTECTED CREDENTIAL"));
        root.append(&heading("Remove OpenAI API key?"));
        root.append(&body_copy(
            "Goblins AI will switch away from the API-key engine before the encrypted credential is removed. You can add another key later.",
        ));

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        actions.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Keep key");
        let remove = gtk::Button::with_label("Remove key");
        remove.add_css_class("destructive-action");
        actions.append(&cancel);
        actions.append(&remove);
        root.append(&actions);

        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("Goblins OS Protected Credential")
            .default_width(540)
            .default_height(300)
            .resizable(true)
            .child(&root)
            .build();
        window.set_modal(true);
        window.set_default_widget(Some(&cancel));
        cancel.grab_focus();

        let cancel_window = window.clone();
        cancel.connect_clicked(move |_| cancel_window.close());
        let remove_window = window.clone();
        let confirmed = confirmed.clone();
        remove.connect_clicked(move |_| {
            confirmed.set(true);
            remove_window.close();
        });
        install_escape(&window);
        let timeout_window = window.clone();
        gtk::glib::timeout_add_local_once(DIALOG_TIMEOUT, move || timeout_window.close());
        window.present();
    });
    application.run_with_args::<&str>(&[]);
    returned.get()
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_remove_confirmation(_configured: bool) -> bool {
    false
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn show_storage_failure() -> bool {
    run_notice(
        "The key could not be encrypted",
        "Secure credential storage is unavailable. No key was saved and your current engine was not changed.",
    )
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn show_storage_failure() -> bool {
    false
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn show_commit_failure() -> bool {
    run_notice(
        "The key could not be saved",
        "Goblins OS did not accept the encrypted credential. No existing key or engine selection was changed.",
    )
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn show_commit_failure() -> bool {
    false
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn show_remove_failure() -> bool {
    run_notice(
        "The key could not be removed",
        "Goblins OS kept the encrypted credential. Review the engine status and try again.",
    )
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn show_remove_failure() -> bool {
    false
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_notice(title: &str, detail: &str) -> bool {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.OpenAIKeyBroker")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let title = title.to_string();
    let detail = detail.to_string();
    application.connect_activate(move |application| {
        goblins_os_ui::init_theming(CSS);
        let root = dialog_root();
        root.append(&eyebrow("CREDENTIAL NOT CHANGED"));
        root.append(&heading(&title));
        root.append(&body_copy(&detail));
        let close = gtk::Button::with_label("Done");
        close.set_halign(gtk::Align::End);
        root.append(&close);
        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("Goblins OS Protected Credential")
            .default_width(520)
            .default_height(280)
            .resizable(true)
            .child(&root)
            .build();
        window.set_default_widget(Some(&close));
        let close_window = window.clone();
        close.connect_clicked(move |_| close_window.close());
        install_escape(&window);
        window.present();
    });
    application.run_with_args::<&str>(&[]);
    true
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn dialog_root() -> gtk4::Box {
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
    root.add_css_class("gos-key-broker-root");
    root.set_margin_top(28);
    root.set_margin_bottom(28);
    root.set_margin_start(28);
    root.set_margin_end(28);
    root
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn eyebrow(text: &str) -> gtk4::Label {
    use gtk4::prelude::*;
    let label = gtk4::Label::new(Some(text));
    label.add_css_class("gos-key-broker-eyebrow");
    label.set_xalign(0.0);
    label
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn heading(text: &str) -> gtk4::Label {
    use gtk4::prelude::*;
    let label = gtk4::Label::new(Some(text));
    label.add_css_class("gos-key-broker-title");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn body_copy(text: &str) -> gtk4::Label {
    use gtk4::prelude::*;
    let label = gtk4::Label::new(Some(text));
    label.add_css_class("gos-key-broker-copy");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_escape(window: &gtk4::ApplicationWindow) {
    use gtk4::prelude::*;
    let escape_window = window.clone();
    let keys = gtk4::EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            escape_window.close();
            return gtk4::glib::Propagation::Stop;
        }
        gtk4::glib::Propagation::Proceed
    });
    window.add_controller(keys);
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn install_escape_to_clear(window: &gtk4::ApplicationWindow, entry: &gtk4::PasswordEntry) {
    use gtk4::prelude::*;
    let escape_window = window.clone();
    let escape_entry = entry.clone();
    let keys = gtk4::EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            escape_entry.set_text("");
            escape_window.close();
            return gtk4::glib::Propagation::Stop;
        }
        gtk4::glib::Propagation::Proceed
    });
    window.add_controller(keys);
}

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

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const CSS: &str = r#"
.gos-key-broker-root {
  background: @theme_bg_color;
  color: @theme_fg_color;
}
.gos-key-broker-eyebrow {
  color: alpha(@theme_fg_color, 0.62);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.08em;
}
.gos-key-broker-title {
  font-size: 26px;
  font-weight: 700;
  letter-spacing: -0.025em;
}
.gos-key-broker-copy, .gos-key-broker-detail {
  color: alpha(@theme_fg_color, 0.74);
  font-size: 14px;
  line-height: 1.35;
}
.gos-key-broker-entry {
  min-height: 44px;
  border-radius: 10px;
}
"#;

#[cfg(test)]
mod tests {
    use super::{
        sensitive_command_output, valid_encrypted_credential, valid_lease_id, valid_plaintext,
        KeyAction, CREDENTIAL_NAME_ARG, MAX_ENCRYPTED_CREDENTIAL_BYTES, SYSTEMD_CREDS,
        SYSTEMD_CREDS_ARGS,
    };
    use std::time::{Duration, Instant};
    use zeroize::Zeroizing;

    const TEST_SENTINEL: &[u8] = b"GOBLINS_OS_TEST_CREDENTIAL_V1";

    #[test]
    fn plaintext_policy_is_bounded_and_rejects_whitespace_and_controls() {
        assert!(valid_plaintext(TEST_SENTINEL));
        assert!(!valid_plaintext(b""));
        assert!(!valid_plaintext(b"test only"));
        assert!(!valid_plaintext(b"test\nonly"));
        assert!(!valid_plaintext(&vec![
            b'x';
            super::MAX_PLAINTEXT_BYTES + 1
        ]));
    }

    #[test]
    fn ciphertext_policy_accepts_only_bounded_base64_text() {
        assert!(valid_encrypted_credential("R09CTElOU19PU19URVNU"));
        assert!(!valid_encrypted_credential(""));
        assert!(!valid_encrypted_credential("not encrypted\ntext"));
        assert!(!valid_encrypted_credential(
            &"A".repeat(MAX_ENCRYPTED_CREDENTIAL_BYTES + 1)
        ));
    }

    #[test]
    fn systemd_creds_invocation_has_no_secret_bearing_argument_or_fallback_key() {
        assert_eq!(SYSTEMD_CREDS, "/usr/bin/systemd-creds");
        assert_eq!(CREDENTIAL_NAME_ARG, "--name=goblins.openai-api-key.v1");
        assert_eq!(SYSTEMD_CREDS_ARGS.last(), Some(&"-"));
        assert_eq!(SYSTEMD_CREDS_ARGS[SYSTEMD_CREDS_ARGS.len() - 2], "-");
        assert!(SYSTEMD_CREDS_ARGS.contains(&"--with-key=auto"));
        assert!(!SYSTEMD_CREDS_ARGS.iter().any(|argument| {
            argument.contains("null") || String::from_utf8_lossy(TEST_SENTINEL) == *argument
        }));
    }

    #[test]
    fn encryption_subprocess_lifecycle_is_bounded_on_every_terminal_path() {
        let started = Instant::now();
        let success = sensitive_command_output(
            "/bin/sh",
            &[
                "-c",
                "value=$(cat); [ \"$value\" = fixture ] || exit 9; (sleep 30) & printf ok; exit 0",
            ],
            Zeroizing::new(b"fixture".to_vec()),
            16,
            Duration::from_secs(2),
        );
        assert_eq!(
            success.as_ref().map(|value| value.as_slice()),
            Some(b"ok".as_slice())
        );
        assert!(started.elapsed() < Duration::from_secs(3));

        for (script, limit, timeout) in [
            ("(sleep 30) & exit 7", 16, Duration::from_secs(2)),
            (
                "i=0; while [ $i -lt 32 ]; do printf A; i=$((i+1)); done; sleep 30",
                16,
                Duration::from_secs(2),
            ),
            ("sleep 30", 16, Duration::from_millis(100)),
        ] {
            let started = Instant::now();
            assert!(sensitive_command_output(
                "/bin/sh",
                &["-c", script],
                Zeroizing::new(b"fixture".to_vec()),
                limit,
                timeout,
            )
            .is_none());
            assert!(started.elapsed() < Duration::from_secs(3));
        }
    }

    #[test]
    fn operation_and_lease_syntax_fail_closed() {
        assert_eq!(KeyAction::parse("add"), Some(KeyAction::Add));
        assert_eq!(KeyAction::parse("rotate"), Some(KeyAction::Rotate));
        assert_eq!(KeyAction::parse("remove"), Some(KeyAction::Remove));
        assert_eq!(KeyAction::parse("replace-and-export"), None);
        assert!(valid_lease_id(&"a".repeat(64)));
        assert!(!valid_lease_id(&"a".repeat(63)));
        assert!(!valid_lease_id(&"G".repeat(64)));
    }

    #[test]
    fn capability_initialization_precedes_args_claim_display_and_gtk() {
        let source = include_str!("main.rs");
        let initialize = source
            .find("let core = initialize(ClientKind::OpenAiKeyBroker)?;")
            .expect("capability initialization");
        for later in [
            "std::env::args().any",
            "claim_operation(&core)",
            "prepare_trusted_ui_path()",
            "run_key_entry(action, configured, true)",
        ] {
            assert!(initialize < source.find(later).expect("ordered broker action"));
        }
    }

    #[test]
    fn broker_has_only_claim_commit_and_decision_core_routes() {
        let source = include_str!("main.rs");
        let routes: std::collections::BTreeSet<_> = source
            .split('"')
            .filter(|value| value.starts_with(concat!("/", "v1/")))
            .collect();
        assert_eq!(
            routes,
            [
                "/v1/openai-key-broker/claim",
                "/v1/openai-key-broker/commit",
                "/v1/openai-key-broker/decision",
            ]
            .into_iter()
            .collect()
        );
    }

    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    #[test]
    fn gnome_shell_cgroup_rejects_lookalikes() {
        assert!(super::trusted_gnome_shell_cgroup(
            "0::/user.slice/user-1000.slice/user@1000.service/session.slice/org.gnome.Shell@wayland.service\n"
        ));
        for cgroup in [
            "0::/user.slice/org.gnome.Shell@wayland.service.evil\n",
            "0::/user.slice/evil-org.gnome.Shell@wayland.service\n",
            "0::/user.slice/app.slice/app-gnome-org.gnome.Shell@wayland.service-evil.scope\n",
        ] {
            assert!(!super::trusted_gnome_shell_cgroup(cgroup));
        }
    }
}
