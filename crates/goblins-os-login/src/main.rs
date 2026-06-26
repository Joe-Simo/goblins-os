use std::{
    env,
    error::Error,
    fmt,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8787";
const DEFAULT_CORE_WAIT_SECS: u64 = 60;
const MAX_CORE_BODY_BYTES: usize = 1024 * 1024;

type LoginResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone)]
struct LoginConfig {
    core_url: String,
    core_wait: Duration,
}

#[derive(Clone)]
struct LoginState {
    core_ready: bool,
    auth: Option<AuthStatus>,
    gate: Option<SessionGateStatus>,
}

#[derive(Clone, Deserialize)]
struct AuthStatus {
    configured: bool,
    authenticated: bool,
    provider: String,
    session_storage: String,
    message: String,
}

#[derive(Clone, Deserialize)]
struct SessionGateStatus {
    generated_at: String,
    source: String,
    unlocked: bool,
    mode: Option<String>,
    first_boot_mode: Option<String>,
    lock: SessionLock,
}

#[derive(Clone, Deserialize)]
struct SessionLock {
    state: String,
    reason: String,
    openai_account_required: bool,
    local_mode_available: bool,
    state_path: String,
}

#[derive(Debug, PartialEq, Eq)]
struct HttpEndpoint {
    host: String,
    port: u16,
}

#[derive(Debug, PartialEq, Eq)]
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
enum CoreFetchError {
    Status(u16),
    Malformed,
    Transport,
    Decode,
}

impl fmt::Display for CoreFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) => write!(formatter, "core returned HTTP {status}"),
            Self::Malformed => formatter.write_str("core response was malformed"),
            Self::Transport => formatter.write_str("core connection failed"),
            Self::Decode => formatter.write_str("core response JSON did not match the OS contract"),
        }
    }
}

fn main() -> LoginResult<()> {
    let config = LoginConfig::from_env();
    let core_ready = wait_for_core(&config.core_url, config.core_wait);
    let state = load_login_state(&config, core_ready);

    println!("Goblins OS native login started");
    println!("core={}", config.core_url);
    println!("{}", login_state_summary(&state));

    if state.gate.as_ref().is_some_and(|gate| gate.unlocked) {
        println!("session_gate=unlocked");
        return Ok(());
    }
    if first_boot_setup_pending(&state) {
        println!("session_gate=waiting_for_first_boot");
        launch_first_boot_installer()?;
        return Ok(());
    }

    run_native_login(config, state)
}

impl LoginConfig {
    fn from_env() -> Self {
        Self {
            core_url: env::var("GOBLINS_OS_CORE_URL")
                .or_else(|_| env::var("OPENAI_OS_CORE_URL"))
                .unwrap_or_else(|_| DEFAULT_CORE_URL.into()),
            core_wait: Duration::from_secs(env_u64(
                "GOBLINS_OS_LOGIN_CORE_WAIT_SECS",
                DEFAULT_CORE_WAIT_SECS,
            )),
        }
    }
}

fn load_login_state(config: &LoginConfig, core_ready: bool) -> LoginState {
    if !core_ready {
        return LoginState {
            core_ready,
            auth: None,
            gate: None,
        };
    }

    LoginState {
        core_ready,
        auth: get_core_json(&config.core_url, "/v1/auth/openai/status").ok(),
        gate: get_core_json(&config.core_url, "/v1/session/gate").ok(),
    }
}

fn login_state_summary(state: &LoginState) -> String {
    let auth = state
        .auth
        .as_ref()
        .map(|auth| {
            format!(
                "{}:{}:{}:{}",
                auth.provider,
                if auth.authenticated {
                    "authenticated"
                } else if auth.configured {
                    "provider-ready"
                } else {
                    "locked"
                },
                auth.session_storage,
                auth.message
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let gate = state
        .gate
        .as_ref()
        .map(|gate| {
            format!(
                "{}:{} unlocked={} mode={} first_boot={} lock={}:{}:{}:{}:{}",
                gate.source,
                gate.generated_at,
                gate.unlocked,
                gate.mode.as_deref().unwrap_or("none"),
                gate.first_boot_mode.as_deref().unwrap_or("none"),
                gate.lock.state,
                gate.lock.reason,
                gate.lock.openai_account_required,
                gate.lock.local_mode_available,
                gate.lock.state_path
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());

    format!(
        "login_state=core:{} auth=[{}] gate=[{}]",
        if state.core_ready { "ready" } else { "waiting" },
        auth,
        gate
    )
}

fn first_boot_setup_pending(state: &LoginState) -> bool {
    state.gate.as_ref().is_some_and(|gate| {
        gate.first_boot_mode.is_none() || gate.lock.state == "waiting-for-first-boot"
    })
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn local_unlock_available(state: &LoginState) -> bool {
    state
        .gate
        .as_ref()
        .is_some_and(|gate| gate.lock.local_mode_available)
        && !first_boot_setup_pending(state)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn session_gate_summary(gate: &SessionGateStatus) -> String {
    // A single reader-facing sentence per lock state — never a raw field-join of
    // internal mode/first-boot tokens.
    match gate.lock.state.as_str() {
        "unlocked" => "Signed in. The desktop is unlocked.".to_string(),
        "waiting-for-first-boot" => {
            "Waiting for first-boot setup to choose how you sign in.".to_string()
        }
        "requires-open-a-i-account" => {
            "Sign in with your OpenAI account to unlock, or keep local-only desktop access."
                .to_string()
        }
        "local-only-available" => {
            "Local-only desktop access is ready. Sign in with your OpenAI account any time."
                .to_string()
        }
        // Unknown/future lock states never expose the raw token to the user.
        _ => "Sign-in state is being prepared.".to_string(),
    }
}

fn launch_first_boot_installer() -> std::io::Result<()> {
    #[cfg(all(target_os = "linux", feature = "native-desktop"))]
    {
        std::process::Command::new("/usr/libexec/goblins-os/goblins-os-installer").spawn()?;
    }

    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn run_native_login(config: LoginConfig, state: LoginState) -> LoginResult<()> {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let application = gtk::Application::builder()
        .application_id("org.goblins.OS.Login")
        .build();

    application.connect_activate(move |app| {
        goblins_os_ui::init_theming(GOBLINS_OS_LOGIN_CSS);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Goblins OS Login")
            .decorated(false)
            .default_width(1280)
            .default_height(820)
            .build();

        // The identity gate sits over a real GSK blur-of-wallpaper material (the
        // macOS login idiom: a centered card over blurred wallpaper), not a flat
        // opaque canvas. The blur renders under cairo too, so it shows headlessly.
        window.set_child(Some(&goblins_os_ui::VibrancyBackdrop::new(
            goblins_os_ui::resolve_dark(),
            &build_login(app, &window, &config, &state),
        )));
        window.fullscreen();
        window.present();
    });

    application.run();
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn build_login(
    app: &gtk4::Application,
    window: &gtk4::ApplicationWindow,
    config: &LoginConfig,
    state: &LoginState,
) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let auth_configured = state.auth.as_ref().is_some_and(|auth| auth.configured);
    let auth_authenticated = state.auth.as_ref().is_some_and(|auth| auth.authenticated);
    let local_available = local_unlock_available(state);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("gos-login-root");

    // No top chrome bar: the macOS lock screen is edge-to-edge — just the centered
    // identity card over the blurred wallpaper. Brand identity lives in the card
    // (the OpenAI mark + "OPENAI ACCOUNT" kicker), not a redundant titlebar.

    // macOS login idiom: a single centered identity column over the canvas,
    // not a two-column dashboard. The night-gradient identity card is the hero;
    // readiness folds in below the primary action as quiet supporting context.
    let identity = gtk::Box::new(gtk::Orientation::Vertical, 18);
    identity.add_css_class("gos-identity-panel");
    identity.set_size_request(460, -1);
    identity.set_halign(gtk::Align::Center);
    // The identity hero keeps the white mark in both schemes (its card is the
    // night gradient regardless of theme), centered above the hero copy.
    let hero_mark = goblins_os_ui::brand_mark(goblins_os_design::OPENAI_MARK_LIGHT, 56);
    hero_mark.set_halign(gtk::Align::Center);
    identity.append(&hero_mark);
    identity.append(&centered_label("OpenAI account", &["gos-kicker"]));
    identity.append(&centered_label(
        if auth_authenticated {
            "Ready"
        } else if auth_configured {
            "Sign in"
        } else {
            "Locked"
        },
        &["gos-hero-title"],
    ));
    identity.append(&centered_label(
        state
            .gate
            .as_ref()
            .map(|gate| gate.lock.reason.as_str())
            .unwrap_or("Waiting for local OS services."),
        &["gos-hero-copy"],
    ));

    let feedback = centered_label(
        &state
            .gate
            .as_ref()
            .map(session_gate_summary)
            .unwrap_or_else(|| "Waiting for local OS services.".to_string()),
        &["gos-feedback"],
    );

    // A state ("OpenAI sign-in isn't set up yet" / "OpenAI account ready") is a status line,
    // never button chrome — only a real action gets a button.
    let sign_in_built = auth_configured && !auth_authenticated;
    if sign_in_built {
        let sign_in = button("Sign in with OpenAI", &["gos-primary-action"]);
        let core_url = config.core_url.clone();
        let feedback = feedback.clone();
        sign_in.connect_clicked(move |_| match openai_login_destination(&core_url) {
            Ok(destination) => {
                feedback.set_text("Opening the configured OpenAI account provider.");
                if let Err(error) = gtk::gio::AppInfo::launch_default_for_uri(
                    &destination,
                    None::<&gtk::gio::AppLaunchContext>,
                ) {
                    feedback.set_text("The desktop could not open the OpenAI account provider.");
                    eprintln!("login_openai_launch_error={error}");
                }
            }
            Err(error) => {
                feedback.set_text("OpenAI account handoff did not start.");
                eprintln!("login_openai_start_error={error}");
            }
        });
        identity.append(&sign_in);
    } else if auth_authenticated {
        // Authenticated: a calm ready status. The not-configured case needs no
        // note here — the hero copy already states the situation and the
        // feedback line states the choice, so a third paraphrase would only
        // repeat them.
        identity.append(&centered_label(
            "OpenAI account ready.",
            &["gos-identity-note"],
        ));
    }

    // The brightest, top-most control must be the usable one: reading order has
    // to match action priority. When the OpenAI account is authenticated the
    // OpenAI unlock is the live primary and leads; otherwise the live local-only
    // unlock leads and the (inert until authenticated) OpenAI unlock is demoted
    // below it. While inert it must READ inert: it wears the night-panel ghost
    // (40%-opacity label, transparent fill, half-strength hairline) so it never
    // looks tappable next to the live local primary, matching its set_sensitive.
    let unlock_openai = button(
        "Unlock Goblins OS desktop",
        if auth_authenticated {
            &["gos-primary-action"]
        } else {
            &["gos-disabled-action"]
        },
    );
    unlock_openai.set_sensitive(auth_authenticated);
    if auth_authenticated {
        let app_handle = app.clone();
        let core_url = config.core_url.clone();
        let feedback = feedback.clone();
        unlock_openai.connect_clicked(move |_| match unlock_session(&core_url, "cloud-openai") {
            Ok(()) => app_handle.quit(),
            Err(error) => {
                feedback.set_text("Goblins OS desktop unlock was rejected by local OS services.");
                eprintln!("login_unlock_openai_error={error}");
            }
        });
    }

    // Local-only is the primary white pill when it is the available path (no
    // OpenAI account configured); a quiet secondary when OpenAI is the primary.
    let unlock_local = button(
        "Unlock local-only desktop",
        if local_available && !auth_authenticated {
            &["gos-primary-action"]
        } else if local_available {
            &["gos-secondary-action"]
        } else {
            &["gos-disabled-action"]
        },
    );
    unlock_local.set_sensitive(local_available);
    if local_available {
        let app_handle = app.clone();
        let core_url = config.core_url.clone();
        let feedback = feedback.clone();
        unlock_local.connect_clicked(move |_| match unlock_session(&core_url, "local-gpt-oss") {
            Ok(()) => app_handle.quit(),
            Err(error) => {
                feedback.set_text("Local-only unlock was rejected by local OS services.");
                eprintln!("login_unlock_local_error={error}");
            }
        });
    }

    if auth_authenticated {
        // OpenAI is the live primary path: it leads, local-only is the secondary.
        identity.append(&unlock_openai);
        identity.append(&unlock_local);
    } else {
        // No OpenAI account yet: the live local-only unlock is the primary and
        // leads. The OpenAI unlock stays inert, so it is demoted below it as a
        // dimmed ghost — present for discoverability, unmistakably not yet usable.
        identity.append(&unlock_local);
        identity.append(&unlock_openai);
    }

    // Guarantee a live way forward. When local OS services never came up (or every
    // unlock path is gated), none of the buttons above is sensitive: there is no
    // sign-in, no authenticated OpenAI unlock, and no local unlock. Without an
    // always-live control the user is stranded on an inert screen. "Try again"
    // re-probes local OS services on a worker thread and rebuilds the panels.
    if !sign_in_built && !auth_authenticated && !local_available {
        let retry = button("Try again", &["gos-primary-action"]);
        let app_handle = app.clone();
        let window_handle = window.clone();
        let config = config.clone();
        let feedback = feedback.clone();
        retry.connect_clicked(move |retry| {
            retry.set_sensitive(false);
            feedback.set_text("Reconnecting to local OS services.");

            // The /health, auth, and gate fetches each block up to ~1.2s, so run
            // them off the GTK main loop and marshal the result back via a channel
            // polled with rx.try_recv() — mirroring the launcher worker pattern.
            let (tx, rx) = std::sync::mpsc::channel::<LoginState>();
            let worker_config = config.clone();
            thread::spawn(move || {
                let core_ready = wait_for_core(&worker_config.core_url, worker_config.core_wait);
                let _ = tx.send(load_login_state(&worker_config, core_ready));
            });

            let app_handle = app_handle.clone();
            let window_handle = window_handle.clone();
            let config = config.clone();
            let retry = retry.clone();
            let feedback = feedback.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(90), move || match rx.try_recv() {
                Ok(next_state) => {
                    window_handle.set_child(Some(&goblins_os_ui::VibrancyBackdrop::new(
                        goblins_os_ui::resolve_dark(),
                        &build_login(&app_handle, &window_handle, &config, &next_state),
                    )));
                    gtk::glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    retry.set_sensitive(true);
                    feedback.set_text("Could not reach local OS services. Try again.");
                    gtk::glib::ControlFlow::Break
                }
            });
        });
        identity.append(&retry);
    }

    identity.append(&feedback);

    // Readiness is folded into the centered column as a quiet, single-column block
    // BELOW the primary action — supporting context, not a co-equal 380px rail.
    // It reads as muted night-foreground facts under a hairline, never a second
    // bright panel competing with the hero. The honest gate states are kept.
    //
    // The list is LEFT-aligned within a constrained content column (not centered):
    // a label-over-description list scans against a left edge, matching the
    // left-aligned status rhythm used throughout Settings. The column itself is
    // centered under the hero, so the block stays balanced while its rows align.
    let readiness = gtk::Box::new(gtk::Orientation::Vertical, 8);
    readiness.add_css_class("gos-login-readiness");
    readiness.set_margin_top(8);
    readiness.set_size_request(320, -1);
    readiness.set_halign(gtk::Align::Center);
    // The hero feedback line above already states desktop-access status
    // (session_gate_summary), so the readiness block stays tight and
    // non-duplicative: just first-run and account sign-in facts.
    readiness.append(&label("Session checks", &["gos-kicker"]));
    if let Some(gate) = &state.gate {
        readiness.append(&readiness_fact(
            "First run",
            if gate.first_boot_mode.is_some() {
                "Setup is complete for this desktop."
            } else {
                "Setup will finish before the desktop opens."
            },
        ));
    }
    match &state.auth {
        Some(auth) => {
            // Each row states one fact once. The "continue locally" message lives
            // solely in the explanatory line beneath the title, so the not-configured
            // row here owns only its own honest fact: how tokens are handled at sign-in.
            let secure_account_copy = if auth.configured && auth.authenticated {
                "Signed in. Account tokens are stored securely by Goblins OS and never shown here."
            } else if auth.configured {
                "Ready to sign in. Account tokens will be stored securely by Goblins OS."
            } else {
                "Not configured. Account tokens are stored securely by Goblins OS at sign-in."
            };
            readiness.append(&readiness_fact("OpenAI sign-in", secure_account_copy));
        }
        None => readiness.append(&readiness_fact(
            "OpenAI sign-in",
            "Waiting for account state.",
        )),
    }
    identity.append(&readiness);

    // Center the single identity column in the viewport so first boot reads as a
    // calm, intentional macOS-style login rather than a top-left-packed dashboard.
    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.set_vexpand(true);
    center.set_valign(gtk::Align::Center);
    center.set_halign(gtk::Align::Center);
    center.append(&identity);
    root.append(&center);
    root
}

// A single readiness fact, styled for the night identity card: a white title line
// (default night foreground) over muted supporting copy, stacked as a tight two-line
// row. It reads as a quiet supporting fact in the centered column, never a bright
// light row competing with the hero — so no light `.gos-row` fill on the night
// gradient. The lines LEFT-align (and fill the constrained readiness column) so the
// list scans against one left edge, matching the Settings status-row rhythm.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn readiness_fact(title: &str, detail: &str) -> gtk4::Box {
    use gtk::prelude::*;
    use gtk4 as gtk;

    let fact = gtk::Box::new(gtk::Orientation::Vertical, 1);
    fact.add_css_class("gos-login-fact");
    fact.set_halign(gtk::Align::Fill);
    fact.append(&label(title, &["gos-identity-note"]));
    fact.append(&label(detail, &["gos-feedback"]));
    fact
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn label(text: &str, classes: &[&str]) -> gtk4::Label {
    use gtk4::prelude::*;

    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);

    for class in classes {
        label.add_css_class(class);
    }

    label
}

// A centered variant of `label` for the single macOS-style identity column: the
// text and its wrap both center instead of left-anchoring, so multi-line copy
// stays balanced under the hero.
#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn centered_label(text: &str, classes: &[&str]) -> gtk4::Label {
    use gtk4::prelude::*;

    let label = label(text, classes);
    label.set_xalign(0.5);
    label.set_halign(gtk4::Align::Center);
    label.set_justify(gtk4::Justification::Center);
    label
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn button(text: &str, classes: &[&str]) -> gtk4::Button {
    use gtk4::prelude::*;

    let button = gtk4::Button::with_label(text);

    for class in classes {
        button.add_css_class(class);
    }

    button
}

#[cfg(not(all(target_os = "linux", feature = "native-desktop")))]
fn run_native_login(_config: LoginConfig, _state: LoginState) -> LoginResult<()> {
    println!("native_login_state=unavailable");
    println!("native_login_reason=build_requires_linux_native_desktop_feature");
    Ok(())
}

fn wait_for_core(core_url: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if matches!(http_status(core_url, "/health"), Some(200)) {
            return true;
        }

        thread::sleep(Duration::from_millis(750));
    }

    false
}

fn http_status(base_url: &str, path: &str) -> Option<u16> {
    let endpoint = parse_http_endpoint(base_url)?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .ok()?
        .next()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(600)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(900)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(900)))
        .ok()?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        endpoint.host
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut buffer = [0_u8; 128];
    let read = stream.read(&mut buffer).ok()?;
    let response = std::str::from_utf8(&buffer[..read]).ok()?;
    let status = response.split_whitespace().nth(1)?;

    status.parse().ok()
}

fn get_core_json<T>(base_url: &str, path: &str) -> Result<T, CoreFetchError>
where
    T: for<'de> Deserialize<'de>,
{
    let response = http_request(base_url, "GET", path, None)?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    serde_json::from_slice(&response.body).map_err(|_| CoreFetchError::Decode)
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn openai_login_destination(core_url: &str) -> Result<String, CoreFetchError> {
    let response = http_request(core_url, "GET", "/v1/auth/openai/start", None)?;
    openai_login_destination_from_response(&response)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn openai_login_destination_from_response(
    response: &HttpResponse,
) -> Result<String, CoreFetchError> {
    if !(300..=399).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    let destination =
        header_value(&response.headers, "location").ok_or(CoreFetchError::Malformed)?;
    // The account handoff must be HTTPS: never hand a file://, plain http://, or
    // custom-scheme Location to the desktop's default URI handler.
    let scheme = destination
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .ok_or(CoreFetchError::Malformed)?;
    if !scheme.eq_ignore_ascii_case("https") {
        return Err(CoreFetchError::Malformed);
    }

    Ok(destination.to_string())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
fn unlock_session(core_url: &str, mode: &str) -> Result<(), CoreFetchError> {
    let body = format!(r#"{{"mode":"{mode}"}}"#);
    let response = http_request(
        core_url,
        "POST",
        "/v1/session/unlock",
        Some(body.as_bytes()),
    )?;

    if !(200..=299).contains(&response.status) {
        return Err(CoreFetchError::Status(response.status));
    }

    Ok(())
}

fn http_request(
    base_url: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<HttpResponse, CoreFetchError> {
    let endpoint = parse_http_endpoint(base_url).ok_or(CoreFetchError::Malformed)?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| CoreFetchError::Transport)?
        .next()
        .ok_or(CoreFetchError::Transport)?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(700))
        .map_err(|_| CoreFetchError::Transport)?;

    stream
        .set_read_timeout(Some(Duration::from_millis(1200)))
        .map_err(|_| CoreFetchError::Transport)?;
    stream
        .set_write_timeout(Some(Duration::from_millis(900)))
        .map_err(|_| CoreFetchError::Transport)?;

    let body = body.unwrap_or_default();
    let content_headers = if body.is_empty() {
        String::new()
    } else {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        )
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\n{}Connection: close\r\n\r\n",
        endpoint.host, content_headers
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|_| CoreFetchError::Transport)?;
    if !body.is_empty() {
        stream
            .write_all(body)
            .map_err(|_| CoreFetchError::Transport)?;
    }

    let mut response = Vec::new();
    stream
        .take(MAX_CORE_BODY_BYTES as u64)
        .read_to_end(&mut response)
        .map_err(|_| CoreFetchError::Transport)?;

    parse_http_response(&response)
}

fn parse_http_response(response: &[u8]) -> Result<HttpResponse, CoreFetchError> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(CoreFetchError::Malformed)?;
    let headers =
        std::str::from_utf8(&response[..header_end]).map_err(|_| CoreFetchError::Malformed)?;
    let mut lines = headers.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or(CoreFetchError::Malformed)?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();

    Ok(HttpResponse {
        status,
        headers,
        body: response[(header_end + 4)..].to_vec(),
    })
}

fn parse_http_endpoint(url: &str) -> Option<HttpEndpoint> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?.trim();

    if authority.is_empty() {
        return None;
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (authority, 80),
    };

    if host.is_empty() {
        return None;
    }

    Some(HttpEndpoint {
        host: host.to_string(),
        port,
    })
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

#[cfg(any(test, all(target_os = "linux", feature = "native-desktop")))]
fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[cfg(all(target_os = "linux", feature = "native-desktop"))]
const GOBLINS_OS_LOGIN_CSS: &str = "";

#[cfg(test)]
mod tests {
    use super::{
        first_boot_setup_pending, local_unlock_available, openai_login_destination_from_response,
        parse_http_endpoint, parse_http_response, session_gate_summary, CoreFetchError,
        HttpEndpoint, HttpResponse, LoginState, SessionGateStatus, SessionLock,
    };

    #[test]
    fn parses_local_core_endpoint() {
        assert_eq!(
            parse_http_endpoint("http://127.0.0.1:8787"),
            Some(HttpEndpoint {
                host: "127.0.0.1".to_string(),
                port: 8787,
            })
        );
    }

    #[test]
    fn rejects_non_http_endpoint() {
        assert_eq!(parse_http_endpoint("https://127.0.0.1:8787"), None);
    }

    #[test]
    fn parses_core_http_response() {
        let response = parse_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
        )
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.body, br#"{"ok":true}"#);
    }

    #[test]
    fn parses_openai_login_redirect() {
        let response = parse_http_response(
            b"HTTP/1.1 302 Found\r\nLocation: https://auth.openai.example/start\r\n\r\n",
        )
        .unwrap();

        assert_eq!(
            openai_login_destination_from_response(&response),
            Ok("https://auth.openai.example/start".to_string())
        );
    }

    #[test]
    fn rejects_non_https_login_redirect() {
        for location in ["http://evil.example/start", "file:///etc/passwd"] {
            let response = HttpResponse {
                status: 302,
                headers: vec![("location".to_string(), location.to_string())],
                body: Vec::new(),
            };

            assert_eq!(
                openai_login_destination_from_response(&response),
                Err(CoreFetchError::Malformed),
                "non-https Location must be rejected: {location}"
            );
        }
    }

    #[test]
    fn rejects_login_start_without_location() {
        let response = HttpResponse {
            status: 302,
            headers: Vec::new(),
            body: Vec::new(),
        };

        assert_eq!(
            openai_login_destination_from_response(&response),
            Err(CoreFetchError::Malformed)
        );
    }

    #[test]
    fn first_boot_pending_defers_to_installer() {
        let state = login_state_with_gate(None, "waiting-for-first-boot", true);

        assert!(first_boot_setup_pending(&state));
        assert!(!local_unlock_available(&state));
    }

    #[test]
    fn local_unlock_waits_until_first_boot_completion_exists() {
        let state = login_state_with_gate(Some("local-gpt-oss"), "local-only-available", true);

        assert!(!first_boot_setup_pending(&state));
        assert!(local_unlock_available(&state));
    }

    #[test]
    fn session_gate_summary_uses_reader_facing_labels() {
        let state = login_state_with_gate(Some("cloud-openai"), "requires-open-a-i-account", true);
        let gate = state.gate.as_ref().unwrap();

        let summary = session_gate_summary(gate);
        assert_eq!(
            summary,
            "Sign in with your OpenAI account to unlock, or keep local-only desktop access."
        );
        // No raw internal tokens / field-joins in user copy.
        assert!(!summary.contains('·'));
        assert!(!summary.contains("first boot "));
    }

    #[test]
    fn login_copy_hides_backend_core_language() {
        let source = include_str!("main.rs");

        assert!(source.contains("Waiting for local OS services."));
        assert!(source.contains("Session checks"));
        assert!(source.contains("rejected by local OS services"));
        for forbidden in [
            ["Waiting for the local Goblins OS ", "core."].join(""),
            [
                "Goblins OS desktop unlock was rejected by the local OS ",
                "core.",
            ]
            .join(""),
            ["Local-only unlock was rejected by the local OS ", "core."].join(""),
            ["\"", "Goblins OS ", "core", "\""].join(""),
        ] {
            assert!(
                !source.contains(&forbidden),
                "login UI copy must not expose backend wording: {forbidden}"
            );
        }
    }

    fn login_state_with_gate(
        first_boot_mode: Option<&str>,
        lock_state: &str,
        local_mode_available: bool,
    ) -> LoginState {
        LoginState {
            core_ready: true,
            auth: None,
            gate: Some(SessionGateStatus {
                generated_at: "test".to_string(),
                source: "test".to_string(),
                unlocked: false,
                mode: None,
                first_boot_mode: first_boot_mode.map(str::to_string),
                lock: SessionLock {
                    state: lock_state.to_string(),
                    reason: "test".to_string(),
                    openai_account_required: false,
                    local_mode_available,
                    state_path: "/tmp/gate.json".to_string(),
                },
            }),
        }
    }
}
