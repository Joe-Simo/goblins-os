use std::time::Duration;

use goblins_os_core_client::{initialize, ClientKind};
use goblins_os_session_tools::focus_log_text;

const ROUTE: &str = "/v1/focus/tick";
const READ_TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    let core = initialize(ClientKind::FocusTick);
    let Ok(core) = core else {
        return;
    };
    let Ok(response) = core.post_json(ROUTE, b"{}", READ_TIMEOUT) else {
        return;
    };
    if let Some(text) = focus_log_text(&response.body) {
        eprintln!("{text}");
    }
}
