use std::{
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::Duration,
};

use goblins_os_core_client::{initialize, ClientKind};
use goblins_os_session_tools::{voice_control_action, VoiceControlAction};

const ROUTE: &str = "/v1/voice/control";
const SETTINGS: &str = "/usr/libexec/goblins-os/goblins-os-settings";
const WTYPE: &str = "/usr/bin/wtype";
const READ_TIMEOUT: Duration = Duration::from_secs(45);

fn main() {
    let core = initialize(ClientKind::VoiceControl);
    let Ok(core) = core else {
        return;
    };
    let Ok(response) = core.post_json(ROUTE, b"{}", READ_TIMEOUT) else {
        return;
    };
    let Some(action) = voice_control_action(response.is_success(), &response.body) else {
        return;
    };

    match action {
        VoiceControlAction::OpenSettings(argument) => {
            let _child = Command::new(SETTINGS)
                .arg(argument)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        VoiceControlAction::TypeTranscript(transcript) => {
            let error = Command::new(WTYPE).arg("--").arg(transcript).exec();
            eprintln!("goblins-os-voice-control: could not start text input: {error}");
        }
    }
}
