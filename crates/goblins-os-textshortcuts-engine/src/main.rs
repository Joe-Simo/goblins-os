use std::env;
use std::fs;
use std::process::ExitCode;

use goblins_os_textshortcuts_engine::{
    ibus_runtime_decision, validate_ibus_component_xml, ContentPurpose, EngineAction, EngineState,
    IbusOperation, InputEvent, ShortcutTable, TextShortcut,
};
use serde::Serialize;

#[derive(Serialize)]
struct Preview {
    trigger: String,
    replacement: Option<String>,
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(64)
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    match args.as_slice() {
        [flag] if flag == "--self-test" => {
            self_test()?;
            println!("goblins_textshortcuts_engine_selftest ok");
            Ok(())
        }
        [flag] if flag == "--ibus" => Err(
            "IBus runtime loop is not enabled in this source-gated build yet; install and component registration are present, but live expansion remains CI/qemu-pending."
                .to_string(),
        ),
        [flag, component_path] if flag == "--component-check" => {
            let raw = fs::read_to_string(component_path)
                .map_err(|error| format!("could not read component XML: {error}"))?;
            validate_ibus_component_xml(&raw)
                .map_err(|error| format!("invalid component XML contract: {error}"))?;
            println!("goblins_textshortcuts_component_contract ok");
            Ok(())
        }
        [flag, trigger] if flag == "--preview" => {
            let table = load_default_table()?;
            print_preview(trigger, &table)
        }
        [flag, trigger, table_path] if flag == "--preview" => {
            let table = load_table(table_path)?;
            print_preview(trigger, &table)
        }
        _ => Err(
            "usage: goblins-textshortcuts-engine --self-test | --component-check <component.xml> | --preview <trigger> [table.json]"
                .to_string(),
        ),
    }
}

fn load_default_table() -> Result<ShortcutTable, String> {
    let path = env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config")))
        .ok_or_else(|| {
            "no HOME or XDG_CONFIG_HOME is available for the Text Shortcuts table".to_string()
        })?
        .join("goblins-os")
        .join("text-shortcuts.json");
    load_table(path)
}

fn load_table(path: impl AsRef<std::path::Path>) -> Result<ShortcutTable, String> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    ShortcutTable::from_json(&raw).map_err(|error| format!("could not parse table JSON: {error}"))
}

fn print_preview(trigger: &str, table: &ShortcutTable) -> Result<(), String> {
    let preview = Preview {
        trigger: trigger.to_string(),
        replacement: table.replacement_for(trigger).map(str::to_string),
    };
    let json = serde_json::to_string(&preview)
        .map_err(|error| format!("could not encode preview JSON: {error}"))?;
    println!("{json}");
    Ok(())
}

fn self_test() -> Result<(), String> {
    let table = ShortcutTable::from_shortcuts(vec![TextShortcut::new("omw", "on my way")]);
    let mut state = EngineState::default();
    for character in "omw".chars() {
        state.handle_event(
            ContentPurpose::Normal,
            InputEvent::Character(character),
            &table,
        );
    }
    let action = state.handle_event(ContentPurpose::Normal, InputEvent::Boundary(' '), &table);
    match action.clone() {
        EngineAction::CommitReplacement {
            delete_previous_chars: 3,
            text,
        } if text == "on my way " => {
            let decision = ibus_runtime_decision(action);
            if decision.key_handled()
                && decision.operations()
                    == [
                        IbusOperation::DeleteSurroundingText {
                            offset: -3,
                            n_chars: 3,
                        },
                        IbusOperation::CommitText("on my way ".to_string()),
                        IbusOperation::HidePreeditText,
                    ]
            {
                Ok(())
            } else {
                Err(format!(
                    "unexpected Text Shortcuts runtime decision: {decision:?}"
                ))
            }
        }
        other => Err(format!(
            "unexpected Text Shortcuts self-test action: {other:?}"
        )),
    }
}
