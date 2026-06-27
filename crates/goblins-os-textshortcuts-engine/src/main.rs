use std::env;
use std::fs;
use std::process::ExitCode;

use goblins_os_textshortcuts_engine::{
    run_text_shortcuts_content_purpose_self_test, run_text_shortcuts_keystroke_self_test,
    run_text_shortcuts_table_watch_self_test, validate_ibus_component_xml, ShortcutTable,
    TextShortcutTableStore,
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
        [flag] if flag == "--keystroke-self-test" => {
            run_text_shortcuts_keystroke_self_test().map_err(|error| error.to_string())?;
            println!("goblins_textshortcuts_keystroke_selftest ok");
            Ok(())
        }
        [flag] if flag == "--table-watch-self-test" => {
            run_text_shortcuts_table_watch_self_test().map_err(|error| error.to_string())?;
            println!("goblins_textshortcuts_table_watch_selftest ok");
            Ok(())
        }
        [flag] if flag == "--content-purpose-self-test" => {
            run_text_shortcuts_content_purpose_self_test().map_err(|error| error.to_string())?;
            println!("goblins_textshortcuts_content_purpose_selftest ok");
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
            "usage: goblins-textshortcuts-engine --self-test | --keystroke-self-test | --table-watch-self-test | --content-purpose-self-test | --component-check <component.xml> | --preview <trigger> [table.json]"
                .to_string(),
        ),
    }
}

fn load_default_table() -> Result<ShortcutTable, String> {
    let store = TextShortcutTableStore::from_environment().map_err(|error| error.to_string())?;
    Ok(store.load().into_table())
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
    run_text_shortcuts_keystroke_self_test().map_err(|error| error.to_string())
}
