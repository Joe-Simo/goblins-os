//! Bluetooth adapter and device controls for Settings.
//!
//! The Settings capability socket authorizes every route in this module. The
//! core then narrows the mutable surface again: device addresses are validated,
//! operations map to fixed `bluetoothctl` argument vectors, subprocesses are
//! bounded, and one shared permit prevents overlapping scans/device requests.
//! Pairing stays with the desktop Bluetooth agent so passkeys and numeric
//! confirmation are never reduced to an unactionable one-shot command.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::bounded::{bounded_command_output, probe_timeout, BoundedCommandError};

const MAX_BLUETOOTH_DEVICES: usize = 64;
const BLUETOOTH_SCAN_SECONDS: &str = "8";
const BLUETOOTH_SCAN_TIMEOUT: Duration = Duration::from_secs(12);
const BLUETOOTH_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const BLUETOOTH_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const BLUETOOTH_FORGET_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize)]
pub struct BluetoothStatus {
    source: &'static str,
    bluez_available: bool,
    service_active: bool,
    adapter_present: bool,
    powered: Option<bool>,
    discoverable: Option<bool>,
    pairable: Option<bool>,
    discovering: Option<bool>,
    adapter: Option<BluetoothAdapter>,
    devices_available: bool,
    devices: Vec<BluetoothDevice>,
    detail: String,
}

#[derive(Serialize, PartialEq, Eq, Debug)]
pub struct BluetoothAdapter {
    name: Option<String>,
    alias: Option<String>,
    address: String,
}

#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
pub struct BluetoothDevice {
    address: String,
    name: String,
    paired: Option<bool>,
    trusted: Option<bool>,
    connected: Option<bool>,
}

#[derive(Deserialize)]
pub struct BluetoothPowerRequest {
    powered: bool,
}

#[derive(Serialize)]
pub struct BluetoothPowerOutcome {
    ok: bool,
    powered: bool,
    text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BluetoothDeviceAction {
    Connect,
    Disconnect,
    Forget,
}

#[derive(Deserialize)]
pub struct BluetoothDeviceActionRequest {
    address: String,
    action: BluetoothDeviceAction,
}

#[derive(Serialize)]
pub struct BluetoothDeviceActionOutcome {
    ok: bool,
    address: String,
    action: BluetoothDeviceAction,
    text: String,
}

#[derive(Serialize)]
pub struct BluetoothScanOutcome {
    ok: bool,
    devices: Vec<BluetoothDevice>,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BluetoothCommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BluetoothCommandError {
    Missing,
    TimedOut,
    Failed,
}

trait BluetoothCommandRunner {
    fn run(
        &self,
        args: &[&str],
        timeout: Duration,
    ) -> Result<BluetoothCommandOutput, BluetoothCommandError>;
}

struct SystemBluetoothCommandRunner;

impl BluetoothCommandRunner for SystemBluetoothCommandRunner {
    fn run(
        &self,
        args: &[&str],
        timeout: Duration,
    ) -> Result<BluetoothCommandOutput, BluetoothCommandError> {
        bounded_command_output("bluetoothctl", args, timeout)
            .map(|output| BluetoothCommandOutput {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
            .map_err(|error| match error {
                BoundedCommandError::Missing => BluetoothCommandError::Missing,
                BoundedCommandError::TimedOut => BluetoothCommandError::TimedOut,
                BoundedCommandError::Failed => BluetoothCommandError::Failed,
            })
    }
}

pub async fn bluetooth_status() -> Json<BluetoothStatus> {
    let status = tokio::task::spawn_blocking(build_bluetooth_status)
        .await
        .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()));
    Json(status)
}

pub async fn set_bluetooth_power(
    Json(request): Json<BluetoothPowerRequest>,
) -> (StatusCode, Json<BluetoothPowerOutcome>) {
    let Some(permit) = bluetooth_operation_permit() else {
        return bluetooth_power_response(
            StatusCode::CONFLICT,
            false,
            request.powered,
            "Another Bluetooth operation is already in progress.",
        );
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        bluetooth_power_outcome(&SystemBluetoothCommandRunner, request.powered)
    })
    .await
    .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
}

pub async fn scan_bluetooth_devices() -> (StatusCode, Json<BluetoothScanOutcome>) {
    let Some(permit) = bluetooth_operation_permit() else {
        return bluetooth_scan_response(
            StatusCode::CONFLICT,
            false,
            Vec::new(),
            "Another Bluetooth operation is already in progress.",
        );
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        bluetooth_scan_outcome(&SystemBluetoothCommandRunner)
    })
    .await
    .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
}

pub async fn change_bluetooth_device(
    Json(request): Json<BluetoothDeviceActionRequest>,
) -> (StatusCode, Json<BluetoothDeviceActionOutcome>) {
    let Some(permit) = bluetooth_operation_permit() else {
        return bluetooth_action_response(
            StatusCode::CONFLICT,
            false,
            request.address,
            request.action,
            "Another Bluetooth operation is already in progress.",
        );
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        bluetooth_device_action_outcome(&SystemBluetoothCommandRunner, request)
    })
    .await
    .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
}

fn bluetooth_operation_permit() -> Option<tokio::sync::OwnedSemaphorePermit> {
    static LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(LIMITER.get_or_init(|| Arc::new(Semaphore::new(1))))
        .try_acquire_owned()
        .ok()
}

fn build_bluetooth_status() -> BluetoothStatus {
    let service_active =
        command_success("systemctl", &["is-active", "--quiet", "bluetooth.service"]);
    let daemon_available = executable_exists("bluetoothd");
    let client_available = executable_exists("bluetoothctl");
    build_bluetooth_status_with(
        &SystemBluetoothCommandRunner,
        service_active,
        daemon_available,
        client_available,
    )
}

fn build_bluetooth_status_with(
    runner: &impl BluetoothCommandRunner,
    service_active: bool,
    daemon_available: bool,
    client_available: bool,
) -> BluetoothStatus {
    let bluez_available = service_active || daemon_available || client_available;
    if !client_available {
        return BluetoothStatus {
            source: "goblins-os-core",
            bluez_available,
            service_active,
            adapter_present: false,
            powered: None,
            discoverable: None,
            pairable: None,
            discovering: None,
            adapter: None,
            devices_available: false,
            devices: Vec::new(),
            detail: "Bluetooth support is not ready on this device, so Settings cannot inspect adapters."
                .to_string(),
        };
    }

    let parsed = match bluetoothctl_show(runner) {
        Ok(stdout) => parse_bluetoothctl_show(&stdout),
        Err(detail) => {
            return BluetoothStatus {
                source: "goblins-os-core",
                bluez_available,
                service_active,
                adapter_present: false,
                powered: None,
                discoverable: None,
                pairable: None,
                discovering: None,
                adapter: None,
                devices_available: false,
                devices: Vec::new(),
                detail,
            }
        }
    };

    let adapter_present = parsed.adapter.is_some();
    let (devices_available, devices) = if adapter_present {
        match bluetooth_devices(runner) {
            Ok(devices) => (true, devices),
            Err(_) => (false, Vec::new()),
        }
    } else {
        (false, Vec::new())
    };

    BluetoothStatus {
        source: "goblins-os-core",
        bluez_available,
        service_active,
        adapter_present,
        powered: parsed.powered,
        discoverable: parsed.discoverable,
        pairable: parsed.pairable,
        discovering: parsed.discovering,
        adapter: parsed.adapter,
        devices_available,
        devices,
        detail: bluetooth_detail(
            service_active,
            bluez_available,
            adapter_present,
            parsed.powered,
        ),
    }
}

struct ParsedBluetoothStatus {
    adapter: Option<BluetoothAdapter>,
    powered: Option<bool>,
    discoverable: Option<bool>,
    pairable: Option<bool>,
    discovering: Option<bool>,
}

fn bluetoothctl_show(runner: &impl BluetoothCommandRunner) -> Result<String, String> {
    command_stdout(runner, &["show"], probe_timeout()).map_err(|error| {
        bluetooth_command_error_detail(
            error,
            "Bluetooth adapter status is not ready.",
            "Bluetooth support is not ready on this device, so Settings cannot inspect adapters.",
        )
    })
}

fn bluetooth_devices(
    runner: &impl BluetoothCommandRunner,
) -> Result<Vec<BluetoothDevice>, BluetoothCommandError> {
    let all = command_stdout(runner, &["devices"], probe_timeout())?;
    let paired = filtered_device_addresses(runner, "Paired");
    let trusted = filtered_device_addresses(runner, "Trusted");
    let connected = filtered_device_addresses(runner, "Connected");
    Ok(merge_bluetooth_devices(
        &all,
        paired.as_ref().ok(),
        trusted.as_ref().ok(),
        connected.as_ref().ok(),
    ))
}

fn filtered_device_addresses(
    runner: &impl BluetoothCommandRunner,
    filter: &str,
) -> Result<BTreeSet<String>, BluetoothCommandError> {
    command_stdout(runner, &["devices", filter], probe_timeout())
        .map(|stdout| parse_device_lines(&stdout).into_keys().collect())
}

fn bluetooth_power_outcome(
    runner: &impl BluetoothCommandRunner,
    powered: bool,
) -> (StatusCode, Json<BluetoothPowerOutcome>) {
    let power = if powered { "on" } else { "off" };
    match runner.run(&["power", power], probe_timeout()) {
        Ok(output) if bluetoothctl_succeeded(&output) => bluetooth_power_response(
            StatusCode::OK,
            true,
            powered,
            bluetooth_power_success_detail(powered),
        ),
        Ok(output) => bluetooth_power_response(
            StatusCode::BAD_GATEWAY,
            false,
            powered,
            bluetoothctl_error_detail(&output.stderr, &output.stdout),
        ),
        Err(error) => bluetooth_power_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            powered,
            bluetooth_command_error_detail(
                error,
                "Bluetooth power is not ready in this session.",
                "Bluetooth support is not ready on this device, so Settings cannot change Bluetooth power.",
            ),
        ),
    }
}

fn bluetooth_scan_outcome(
    runner: &impl BluetoothCommandRunner,
) -> (StatusCode, Json<BluetoothScanOutcome>) {
    let scan = runner.run(
        &["--timeout", BLUETOOTH_SCAN_SECONDS, "scan", "on"],
        BLUETOOTH_SCAN_TIMEOUT,
    );
    let output = match scan {
        Ok(output) if bluetoothctl_succeeded(&output) => output,
        Ok(output) => {
            return bluetooth_scan_response(
                StatusCode::BAD_GATEWAY,
                false,
                Vec::new(),
                &bluetoothctl_error_detail(&output.stderr, &output.stdout),
            )
        }
        Err(error) => {
            return bluetooth_scan_response(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                Vec::new(),
                &bluetooth_command_error_detail(
                    error,
                    "Bluetooth discovery did not finish in time.",
                    "Bluetooth support is not ready on this device, so Settings cannot discover devices.",
                ),
            )
        }
    };

    let mut devices = bluetooth_devices(runner)
        .unwrap_or_else(|_| merge_bluetooth_devices(&output.stdout, None, None, None));
    devices.truncate(MAX_BLUETOOTH_DEVICES);
    let count = devices.len();
    bluetooth_scan_response(
        StatusCode::OK,
        true,
        devices,
        &format!(
            "Bluetooth discovery finished. {count} device{} available.",
            if count == 1 { " is" } else { "s are" }
        ),
    )
}

fn bluetooth_device_action_outcome(
    runner: &impl BluetoothCommandRunner,
    request: BluetoothDeviceActionRequest,
) -> (StatusCode, Json<BluetoothDeviceActionOutcome>) {
    let action = request.action;
    let address = match normalize_bluetooth_address(&request.address) {
        Some(address) => address,
        None => {
            return bluetooth_action_response(
                StatusCode::BAD_REQUEST,
                false,
                request.address,
                action,
                "Bluetooth device addresses must use six hexadecimal pairs.",
            )
        }
    };
    let (command, timeout) = bluetooth_action_command(action);
    match runner.run(&[command, &address], timeout) {
        Ok(output) if bluetoothctl_succeeded(&output) => bluetooth_action_response(
            StatusCode::OK,
            true,
            address,
            action,
            bluetooth_action_success_detail(action),
        ),
        Ok(output) => bluetooth_action_response(
            StatusCode::BAD_GATEWAY,
            false,
            address,
            action,
            &bluetooth_action_error_detail(action, &output.stderr, &output.stdout),
        ),
        Err(error) => bluetooth_action_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            address,
            action,
            &bluetooth_command_error_detail(
                error,
                "The Bluetooth device operation did not finish in time.",
                "Bluetooth support is not ready on this device.",
            ),
        ),
    }
}

fn bluetooth_action_command(action: BluetoothDeviceAction) -> (&'static str, Duration) {
    match action {
        BluetoothDeviceAction::Connect => ("connect", BLUETOOTH_CONNECT_TIMEOUT),
        BluetoothDeviceAction::Disconnect => ("disconnect", BLUETOOTH_DISCONNECT_TIMEOUT),
        BluetoothDeviceAction::Forget => ("remove", BLUETOOTH_FORGET_TIMEOUT),
    }
}

fn command_stdout(
    runner: &impl BluetoothCommandRunner,
    args: &[&str],
    timeout: Duration,
) -> Result<String, BluetoothCommandError> {
    let output = runner.run(args, timeout)?;
    if bluetoothctl_succeeded(&output) {
        Ok(output.stdout)
    } else {
        Err(BluetoothCommandError::Failed)
    }
}

fn bluetoothctl_succeeded(output: &BluetoothCommandOutput) -> bool {
    output.success
        && !format!("{} {}", output.stdout, output.stderr)
            .to_ascii_lowercase()
            .contains("failed to")
}

fn bluetooth_power_response(
    status: StatusCode,
    ok: bool,
    powered: bool,
    text: impl Into<String>,
) -> (StatusCode, Json<BluetoothPowerOutcome>) {
    (
        status,
        Json(BluetoothPowerOutcome {
            ok,
            powered,
            text: text.into(),
        }),
    )
}

fn bluetooth_scan_response(
    status: StatusCode,
    ok: bool,
    devices: Vec<BluetoothDevice>,
    text: &str,
) -> (StatusCode, Json<BluetoothScanOutcome>) {
    (
        status,
        Json(BluetoothScanOutcome {
            ok,
            devices,
            text: text.to_string(),
        }),
    )
}

fn bluetooth_action_response(
    status: StatusCode,
    ok: bool,
    address: String,
    action: BluetoothDeviceAction,
    text: &str,
) -> (StatusCode, Json<BluetoothDeviceActionOutcome>) {
    (
        status,
        Json(BluetoothDeviceActionOutcome {
            ok,
            address,
            action,
            text: text.to_string(),
        }),
    )
}

fn parse_bluetoothctl_show(stdout: &str) -> ParsedBluetoothStatus {
    let mut adapter = None;
    let mut name = None;
    let mut alias = None;
    let mut powered = None;
    let mut discoverable = None;
    let mut pairable = None;
    let mut discovering = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(address) = trimmed
            .strip_prefix("Controller ")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(normalize_bluetooth_address)
        {
            adapter = Some(BluetoothAdapter {
                name: None,
                alias: None,
                address,
            });
            continue;
        }

        if let Some(value) = trimmed.strip_prefix("Name: ") {
            name = bluetooth_device_name(value);
        } else if let Some(value) = trimmed.strip_prefix("Alias: ") {
            alias = bluetooth_device_name(value);
        } else if let Some(value) = trimmed.strip_prefix("Powered: ") {
            powered = parse_yes_no(value);
        } else if let Some(value) = trimmed.strip_prefix("Discoverable: ") {
            discoverable = parse_yes_no(value);
        } else if let Some(value) = trimmed.strip_prefix("Pairable: ") {
            pairable = parse_yes_no(value);
        } else if let Some(value) = trimmed.strip_prefix("Discovering: ") {
            discovering = parse_yes_no(value);
        }
    }

    if let Some(adapter) = &mut adapter {
        adapter.name = name;
        adapter.alias = alias;
    }

    ParsedBluetoothStatus {
        adapter,
        powered,
        discoverable,
        pairable,
        discovering,
    }
}

fn merge_bluetooth_devices(
    all: &str,
    paired: Option<&BTreeSet<String>>,
    trusted: Option<&BTreeSet<String>>,
    connected: Option<&BTreeSet<String>>,
) -> Vec<BluetoothDevice> {
    parse_device_lines(all)
        .into_iter()
        .take(MAX_BLUETOOTH_DEVICES)
        .map(|(address, name)| BluetoothDevice {
            paired: paired.map(|addresses| addresses.contains(&address)),
            trusted: trusted.map(|addresses| addresses.contains(&address)),
            connected: connected.map(|addresses| addresses.contains(&address)),
            address,
            name,
        })
        .collect()
}

fn parse_device_lines(stdout: &str) -> BTreeMap<String, String> {
    let mut devices = BTreeMap::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        let device = trimmed
            .strip_prefix("Device ")
            .or_else(|| trimmed.split_once(" Device ").map(|(_, rest)| rest));
        let Some(device) = device else { continue };
        let mut parts = device.splitn(2, char::is_whitespace);
        let Some(address) = parts.next().and_then(normalize_bluetooth_address) else {
            continue;
        };
        let raw_name = parts.next().unwrap_or_default().trim();
        if bluetooth_property_change(raw_name) {
            continue;
        }
        let name = bluetooth_device_name(raw_name).unwrap_or_else(|| address.clone());
        devices.insert(address, name);
        if devices.len() >= MAX_BLUETOOTH_DEVICES {
            break;
        }
    }
    devices
}

fn bluetooth_property_change(value: &str) -> bool {
    [
        "RSSI:",
        "TxPower:",
        "Connected:",
        "Paired:",
        "Trusted:",
        "Blocked:",
        "ServicesResolved:",
        "ManufacturerData",
        "ServiceData",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn bluetooth_device_name(value: &str) -> Option<String> {
    let name = value
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(120)
        .collect::<String>();
    (!name.is_empty()).then_some(name)
}

fn normalize_bluetooth_address(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() != 17 {
        return None;
    }
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 2 | 5 | 8 | 11 | 14) {
            if *byte != b':' {
                return None;
            }
        } else if !byte.is_ascii_hexdigit() {
            return None;
        }
    }
    Some(value.to_ascii_uppercase())
}

fn bluetooth_detail(
    service_active: bool,
    bluez_available: bool,
    adapter_present: bool,
    powered: Option<bool>,
) -> String {
    if !bluez_available {
        return "Bluetooth support is not ready, so Bluetooth cannot be managed on this device."
            .to_string();
    }
    if !service_active {
        return "Bluetooth support is present, but Bluetooth is not running.".to_string();
    }
    if !adapter_present {
        return "No Bluetooth adapter is connected.".to_string();
    }
    match powered {
        Some(true) => "Bluetooth is powered on. Known devices and discovery are ready.".to_string(),
        Some(false) => "A Bluetooth adapter is present, but it is powered off.".to_string(),
        None => "A Bluetooth adapter is present, but its power state was not reported.".to_string(),
    }
}

fn bluetooth_power_success_detail(powered: bool) -> &'static str {
    if powered {
        "Bluetooth is powered on."
    } else {
        "Bluetooth is powered off. Existing connections were closed by BlueZ."
    }
}

fn bluetooth_action_success_detail(action: BluetoothDeviceAction) -> &'static str {
    match action {
        BluetoothDeviceAction::Connect => "The Bluetooth device is connected.",
        BluetoothDeviceAction::Disconnect => "The Bluetooth device was disconnected.",
        BluetoothDeviceAction::Forget => "The Bluetooth device was forgotten.",
    }
}

fn bluetooth_action_error_detail(
    action: BluetoothDeviceAction,
    stderr: &str,
    stdout: &str,
) -> String {
    let raw = sanitized_command_detail(stderr, stdout);
    let lower = raw.to_ascii_lowercase();
    if lower.contains("not available") || lower.contains("notready") {
        return "That Bluetooth device is not available right now.".to_string();
    }
    if lower.contains("already connected") {
        return "That Bluetooth device is already connected.".to_string();
    }
    if lower.contains("not connected") {
        return "That Bluetooth device is not connected.".to_string();
    }
    let verb = match action {
        BluetoothDeviceAction::Connect => "connect",
        BluetoothDeviceAction::Disconnect => "disconnect",
        BluetoothDeviceAction::Forget => "forget",
    };
    if raw.is_empty() {
        format!("Goblins OS could not {verb} that Bluetooth device.")
    } else {
        format!("Goblins OS could not {verb} that Bluetooth device: {raw}")
    }
}

fn bluetooth_command_error_detail(
    error: BluetoothCommandError,
    unavailable: &str,
    missing: &str,
) -> String {
    match error {
        BluetoothCommandError::Missing => missing.to_string(),
        BluetoothCommandError::TimedOut => unavailable.to_string(),
        BluetoothCommandError::Failed => unavailable.to_string(),
    }
}

fn bluetoothctl_error_detail(stderr: &str, stdout: &str) -> String {
    let raw = sanitized_command_detail(stderr, stdout);
    let lower = raw.to_ascii_lowercase();
    if raw.is_empty() {
        return "No Bluetooth adapter was reported.".to_string();
    }
    if lower.contains("dbus")
        || lower.contains("d-bus")
        || lower.contains("host is down")
        || lower.contains("no medium found")
    {
        return "Bluetooth adapter status is not ready from this session.".to_string();
    }
    if lower.contains("no default controller") || lower.contains("no controller available") {
        return "Bluetooth adapter status is not ready because no default Bluetooth adapter is present."
            .to_string();
    }
    format!("Bluetooth adapter status is not ready: {raw}")
}

fn sanitized_command_detail(stderr: &str, stdout: &str) -> String {
    let raw = if !stderr.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

fn parse_yes_no(value: &str) -> Option<bool> {
    match value.trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

fn command_success(binary: &str, args: &[&str]) -> bool {
    bounded_command_output(binary, args, probe_timeout())
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn executable_exists(binary: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            let candidate = path.join(binary);
            std::fs::metadata(candidate)
                .map(|metadata| metadata.is_file())
                .unwrap_or(false)
        })
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque, time::Duration};

    use axum::http::StatusCode;

    use super::{
        bluetooth_action_command, bluetooth_detail, bluetooth_device_action_outcome,
        bluetooth_power_success_detail, bluetooth_scan_outcome, bluetoothctl_error_detail,
        build_bluetooth_status_with, merge_bluetooth_devices, normalize_bluetooth_address,
        parse_bluetoothctl_show, BluetoothAdapter, BluetoothCommandError, BluetoothCommandOutput,
        BluetoothCommandRunner, BluetoothDeviceAction, BluetoothDeviceActionRequest,
        BLUETOOTH_SCAN_TIMEOUT,
    };

    #[derive(Default)]
    struct FakeBluetoothRunner {
        calls: RefCell<Vec<(Vec<String>, Duration)>>,
        results: RefCell<VecDeque<Result<BluetoothCommandOutput, BluetoothCommandError>>>,
    }

    impl FakeBluetoothRunner {
        fn with_results(
            results: impl IntoIterator<Item = Result<BluetoothCommandOutput, BluetoothCommandError>>,
        ) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                results: RefCell::new(results.into_iter().collect()),
            }
        }
    }

    impl BluetoothCommandRunner for FakeBluetoothRunner {
        fn run(
            &self,
            args: &[&str],
            timeout: Duration,
        ) -> Result<BluetoothCommandOutput, BluetoothCommandError> {
            self.calls.borrow_mut().push((
                args.iter().map(|value| (*value).to_string()).collect(),
                timeout,
            ));
            self.results
                .borrow_mut()
                .pop_front()
                .expect("fake Bluetooth command result")
        }
    }

    fn success(stdout: &str) -> Result<BluetoothCommandOutput, BluetoothCommandError> {
        Ok(BluetoothCommandOutput {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    #[test]
    fn parses_default_controller_status() {
        let parsed = parse_bluetoothctl_show(
            "Controller 00:11:22:33:44:55 (public)\n\
             \tName: goblins\n\
             \tAlias: Goblins Workstation\n\
             \tPowered: yes\n\
             \tDiscoverable: no\n\
             \tPairable: yes\n\
             \tDiscovering: no\n",
        );

        assert_eq!(
            parsed.adapter,
            Some(BluetoothAdapter {
                name: Some("goblins".to_string()),
                alias: Some("Goblins Workstation".to_string()),
                address: "00:11:22:33:44:55".to_string(),
            })
        );
        assert_eq!(parsed.powered, Some(true));
        assert_eq!(parsed.discoverable, Some(false));
        assert_eq!(parsed.pairable, Some(true));
        assert_eq!(parsed.discovering, Some(false));
    }

    #[test]
    fn merges_known_device_state_without_treating_changes_as_names() {
        let paired = ["00:11:22:33:44:55".to_string()].into_iter().collect();
        let connected = ["AA:BB:CC:DD:EE:FF".to_string()].into_iter().collect();
        let devices = merge_bluetooth_devices(
            "Device 00:11:22:33:44:55 Keyboard\n\
             [NEW] Device AA:BB:CC:DD:EE:FF Headphones\n\
             [CHG] Device AA:BB:CC:DD:EE:FF RSSI: -55\n",
            Some(&paired),
            None,
            Some(&connected),
        );
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "Keyboard");
        assert_eq!(devices[0].paired, Some(true));
        assert_eq!(devices[0].trusted, None);
        assert_eq!(devices[1].name, "Headphones");
        assert_eq!(devices[1].connected, Some(true));
    }

    #[test]
    fn device_actions_validate_then_use_fixed_arguments_and_bounds() {
        assert!(serde_json::from_str::<BluetoothDeviceActionRequest>(
            r#"{"address":"AA:BB:CC:DD:EE:FF","action":"pair"}"#
        )
        .is_err());

        let runner = FakeBluetoothRunner::with_results([success("Connection successful")]);
        let (status, outcome) = bluetooth_device_action_outcome(
            &runner,
            BluetoothDeviceActionRequest {
                address: "aa:bb:cc:dd:ee:ff".to_string(),
                action: BluetoothDeviceAction::Connect,
            },
        );
        assert_eq!(status, StatusCode::OK);
        assert!(outcome.ok);
        assert_eq!(outcome.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(
            runner.calls.borrow().as_slice(),
            &[(
                vec!["connect".to_string(), "AA:BB:CC:DD:EE:FF".to_string()],
                super::BLUETOOTH_CONNECT_TIMEOUT,
            )]
        );

        let rejected = FakeBluetoothRunner::default();
        let (status, _) = bluetooth_device_action_outcome(
            &rejected,
            BluetoothDeviceActionRequest {
                address: "AA:BB:CC:DD:EE:FF; power off".to_string(),
                action: BluetoothDeviceAction::Forget,
            },
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(rejected.calls.borrow().is_empty());
        assert_eq!(
            bluetooth_action_command(BluetoothDeviceAction::Disconnect),
            ("disconnect", super::BLUETOOTH_DISCONNECT_TIMEOUT)
        );
        assert!(normalize_bluetooth_address("not-an-address").is_none());
    }

    #[test]
    fn scan_is_bounded_and_returns_real_command_results() {
        let runner = FakeBluetoothRunner::with_results([
            success("[NEW] Device AA:BB:CC:DD:EE:FF Headphones"),
            success("Device AA:BB:CC:DD:EE:FF Headphones"),
            success("Device AA:BB:CC:DD:EE:FF Headphones"),
            success(""),
            success("Device AA:BB:CC:DD:EE:FF Headphones"),
        ]);
        let (status, outcome) = bluetooth_scan_outcome(&runner);
        assert_eq!(status, StatusCode::OK);
        assert!(outcome.ok);
        assert_eq!(outcome.devices.len(), 1);
        assert_eq!(outcome.devices[0].connected, Some(true));
        assert_eq!(runner.calls.borrow()[0].1, BLUETOOTH_SCAN_TIMEOUT);
        assert_eq!(
            runner.calls.borrow()[0].0,
            vec!["--timeout", "8", "scan", "on"]
        );
    }

    #[test]
    fn status_uses_injected_service_and_command_state() {
        let runner = FakeBluetoothRunner::with_results([
            success("Controller 00:11:22:33:44:55\nPowered: yes\n"),
            success("Device AA:BB:CC:DD:EE:FF Headphones"),
            success(""),
            success(""),
            success(""),
        ]);
        let status = build_bluetooth_status_with(&runner, true, true, true);
        assert!(status.bluez_available);
        assert!(status.adapter_present);
        assert!(status.devices_available);
        assert_eq!(status.devices.len(), 1);
    }

    #[test]
    fn bluetooth_copy_distinguishes_missing_service_and_adapter() {
        assert!(bluetooth_detail(false, true, false, None).contains("not running"));
        assert!(bluetooth_detail(true, true, false, None).contains("No Bluetooth adapter"));
        assert!(bluetooth_detail(true, true, true, Some(false)).contains("powered off"));
        assert!(bluetooth_power_success_detail(true).contains("powered on"));
        assert!(bluetooth_power_success_detail(false).contains("powered off"));
        assert!(bluetooth_power_success_detail(false).contains("connections"));
    }

    #[test]
    fn bluetoothctl_errors_are_sanitized_for_settings() {
        assert_eq!(
            bluetoothctl_error_detail("dbus assertion connection != NULL", ""),
            "Bluetooth adapter status is not ready from this session."
        );
        assert_eq!(
            bluetoothctl_error_detail("", "No default controller available"),
            "Bluetooth adapter status is not ready because no default Bluetooth adapter is present."
        );
    }
}
