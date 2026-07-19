use std::{os::unix::process::CommandExt, process::Command, time::Duration};

use goblins_os_core_client::{initialize, ClientKind};
use goblins_os_session_tools::dictation_transcript;

const ROUTE: &str = "/v1/voice/dictate";
const WTYPE: &str = "/usr/bin/wtype";
const READ_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
    let core = initialize(ClientKind::Dictate);
    let Ok(core) = core else {
        return;
    };
    let Ok(response) = core.post_json(ROUTE, b"{}", READ_TIMEOUT) else {
        return;
    };
    let Some(transcript) = dictation_transcript(response.is_success(), &response.body) else {
        return;
    };

    let error = Command::new(WTYPE).arg("--").arg(transcript).exec();
    eprintln!("goblins-os-dictate: could not start text input: {error}");
}
