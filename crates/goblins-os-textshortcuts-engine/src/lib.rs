//! Pure decision logic for the Goblins Text Shortcuts IBus engine.
//!
//! The live IBus/GNOME integration is intentionally CI/qemu-gated. This crate
//! owns the host-testable substrate: table sanitizing, word-boundary trigger
//! tracking, replacement commit decisions, and hard refusal in sensitive text
//! fields.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_SHORTCUTS: usize = 500;
pub const TEXT_SHORTCUTS_CONFIG_DIR: &str = "goblins-os";
pub const TEXT_SHORTCUTS_CONFIG_FILE: &str = "text-shortcuts.json";
pub const IBUS_ENGINE_NAME: &str = "goblins-textshortcuts";
pub const IBUS_COMPONENT_EXEC: &str = "/usr/libexec/goblins-os/goblins-textshortcuts-engine --ibus";
pub const IBUS_COMPONENT_LONGNAME: &str = "Goblins Text Shortcuts";
pub const IBUS_COMPONENT_LAYOUT: &str = "default";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentContractError {
    MissingTagValue {
        tag: &'static str,
        expected: &'static str,
    },
}

impl std::fmt::Display for ComponentContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTagValue { tag, expected } => {
                write!(f, "missing <{tag}>{expected}</{tag}>")
            }
        }
    }
}

impl std::error::Error for ComponentContractError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextShortcut {
    replace: String,
    #[serde(rename = "with")]
    with_text: String,
}

impl TextShortcut {
    pub fn new(replace: impl Into<String>, with_text: impl Into<String>) -> Self {
        Self {
            replace: replace.into(),
            with_text: with_text.into(),
        }
    }

    pub fn replace(&self) -> &str {
        &self.replace
    }

    pub fn with_text(&self) -> &str {
        &self.with_text
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShortcutTable {
    shortcuts: Vec<TextShortcut>,
}

impl ShortcutTable {
    pub fn from_shortcuts(shortcuts: Vec<TextShortcut>) -> Self {
        Self {
            shortcuts: sanitize_shortcuts(shortcuts),
        }
    }

    pub fn from_json(raw: &str) -> Result<Self, serde_json::Error> {
        let shortcuts = serde_json::from_str(raw)?;
        Ok(Self::from_shortcuts(shortcuts))
    }

    pub fn shortcuts(&self) -> &[TextShortcut] {
        &self.shortcuts
    }

    pub fn len(&self) -> usize {
        self.shortcuts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shortcuts.is_empty()
    }

    pub fn replacement_for(&self, trigger: &str) -> Option<&str> {
        self.shortcuts
            .iter()
            .find(|shortcut| shortcut.replace == trigger)
            .map(TextShortcut::with_text)
    }
}

pub fn sanitize_shortcuts(shortcuts: Vec<TextShortcut>) -> Vec<TextShortcut> {
    let mut seen = std::collections::HashMap::new();
    let mut order = Vec::new();
    for shortcut in shortcuts {
        let replace = shortcut.replace.trim().to_string();
        let with_text = shortcut.with_text.trim().to_string();
        if replace.is_empty() || with_text.is_empty() || replace == with_text {
            continue;
        }
        if !seen.contains_key(&replace) {
            order.push(replace.clone());
        }
        seen.insert(replace, with_text);
    }
    order
        .into_iter()
        .take(MAX_SHORTCUTS)
        .map(|replace| TextShortcut {
            with_text: seen.remove(&replace).unwrap_or_default(),
            replace,
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TableStoreError {
    NoConfigHome,
}

impl std::fmt::Display for TableStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoConfigHome => {
                write!(
                    f,
                    "no HOME or XDG_CONFIG_HOME is available for the Text Shortcuts table"
                )
            }
        }
    }
}

impl std::error::Error for TableStoreError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TableLoadStatus {
    Loaded { shortcuts: usize },
    Missing,
    InvalidJson,
    Unreadable,
}

impl TableLoadStatus {
    pub fn detail(&self) -> &'static str {
        match self {
            Self::Loaded { .. } => "Text Shortcuts table loaded.",
            Self::Missing => "No Text Shortcuts table is configured yet.",
            Self::InvalidJson => {
                "Text Shortcuts table could not be parsed; expansion is disabled until it is fixed."
            }
            Self::Unreadable => {
                "Text Shortcuts table could not be read; expansion is disabled until it is accessible."
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableLoadOutcome {
    table: ShortcutTable,
    status: TableLoadStatus,
}

impl TableLoadOutcome {
    pub fn table(&self) -> &ShortcutTable {
        &self.table
    }

    pub fn status(&self) -> &TableLoadStatus {
        &self.status
    }

    pub fn into_table(self) -> ShortcutTable {
        self.table
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextShortcutTableStore {
    path: PathBuf,
}

impl TextShortcutTableStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn from_config_home(config_home: impl AsRef<Path>) -> Self {
        Self::new(default_text_shortcuts_table_path(config_home))
    }

    pub fn from_environment() -> Result<Self, TableStoreError> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .ok_or(TableStoreError::NoConfigHome)?;
        Ok(Self::from_config_home(base))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> TableLoadOutcome {
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => match ShortcutTable::from_json(&raw) {
                Ok(table) => TableLoadOutcome {
                    status: TableLoadStatus::Loaded {
                        shortcuts: table.len(),
                    },
                    table,
                },
                Err(_) => TableLoadOutcome {
                    table: ShortcutTable::default(),
                    status: TableLoadStatus::InvalidJson,
                },
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => TableLoadOutcome {
                table: ShortcutTable::default(),
                status: TableLoadStatus::Missing,
            },
            Err(_) => TableLoadOutcome {
                table: ShortcutTable::default(),
                status: TableLoadStatus::Unreadable,
            },
        }
    }
}

pub fn default_text_shortcuts_table_path(config_home: impl AsRef<Path>) -> PathBuf {
    config_home
        .as_ref()
        .join(TEXT_SHORTCUTS_CONFIG_DIR)
        .join(TEXT_SHORTCUTS_CONFIG_FILE)
}

pub fn validate_ibus_component_xml(raw: &str) -> Result<(), ComponentContractError> {
    require_tag_value(raw, "exec", IBUS_COMPONENT_EXEC)?;
    require_tag_value(raw, "name", IBUS_ENGINE_NAME)?;
    require_tag_value(raw, "longname", IBUS_COMPONENT_LONGNAME)?;
    require_tag_value(raw, "layout", IBUS_COMPONENT_LAYOUT)?;
    Ok(())
}

fn require_tag_value(
    raw: &str,
    tag: &'static str,
    expected: &'static str,
) -> Result<(), ComponentContractError> {
    if tag_values(raw, tag).iter().any(|value| value == expected) {
        Ok(())
    } else {
        Err(ComponentContractError::MissingTagValue { tag, expected })
    }
}

fn tag_values(raw: &str, tag: &str) -> Vec<String> {
    let open_tag = format!("<{tag}>");
    let close_tag = format!("</{tag}>");
    let mut values = Vec::new();
    let mut rest = raw;
    while let Some(open) = rest.find(&open_tag) {
        let after_open = &rest[open + open_tag.len()..];
        let Some(close) = after_open.find(&close_tag) else {
            break;
        };
        values.push(after_open[..close].trim().to_string());
        rest = &after_open[close + close_tag.len()..];
    }
    values
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentPurpose {
    Normal,
    Password,
    HiddenText,
    Sensitive,
}

impl ContentPurpose {
    pub fn permits_replacement(self) -> bool {
        matches!(self, Self::Normal)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputEvent {
    Character(char),
    Boundary(char),
    Backspace,
    Reset,
    Other,
}

impl InputEvent {
    pub fn from_typed_char(value: char) -> Self {
        if is_boundary_char(value) {
            Self::Boundary(value)
        } else {
            Self::Character(value)
        }
    }
}

pub const IBUS_KEY_BACKSPACE: u32 = 0xff08;
pub const IBUS_KEY_TAB: u32 = 0xff09;
pub const IBUS_KEY_RETURN: u32 = 0xff0d;
pub const IBUS_KEY_ESCAPE: u32 = 0xff1b;
pub const IBUS_KEY_LEFT: u32 = 0xff51;
pub const IBUS_KEY_UP: u32 = 0xff52;
pub const IBUS_KEY_RIGHT: u32 = 0xff53;
pub const IBUS_KEY_DOWN: u32 = 0xff54;
pub const IBUS_KEY_DELETE: u32 = 0xffff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IbusKeyEvent {
    keyval: u32,
    unicode: Option<char>,
    pressed: bool,
    command_modifier_active: bool,
}

impl IbusKeyEvent {
    pub fn new(
        keyval: u32,
        unicode: Option<char>,
        pressed: bool,
        command_modifier_active: bool,
    ) -> Self {
        Self {
            keyval,
            unicode,
            pressed,
            command_modifier_active,
        }
    }

    pub fn keyval(&self) -> u32 {
        self.keyval
    }

    pub fn unicode(&self) -> Option<char> {
        self.unicode
    }
}

pub fn input_event_from_ibus_key(event: IbusKeyEvent) -> InputEvent {
    if !event.pressed {
        return InputEvent::Other;
    }
    if event.command_modifier_active {
        return InputEvent::Reset;
    }
    match event.keyval {
        IBUS_KEY_BACKSPACE => InputEvent::Backspace,
        IBUS_KEY_TAB => InputEvent::Boundary('\t'),
        IBUS_KEY_RETURN => InputEvent::Boundary('\n'),
        IBUS_KEY_ESCAPE | IBUS_KEY_LEFT | IBUS_KEY_UP | IBUS_KEY_RIGHT | IBUS_KEY_DOWN
        | IBUS_KEY_DELETE => InputEvent::Reset,
        _ => event
            .unicode
            .filter(|value| !value.is_control())
            .map(InputEvent::from_typed_char)
            .unwrap_or(InputEvent::Other),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineAction {
    PassThrough,
    ShowCandidate {
        trigger: String,
        replacement: String,
    },
    ClearCandidate,
    CommitReplacement {
        delete_previous_chars: usize,
        text: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IbusOperation {
    UpdatePreeditText {
        text: String,
        cursor_pos: u32,
        visible: bool,
    },
    HidePreeditText,
    DeleteSurroundingText {
        offset: i32,
        n_chars: u32,
    },
    CommitText(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IbusRuntimeDecision {
    handled: bool,
    operations: Vec<IbusOperation>,
}

impl IbusRuntimeDecision {
    pub fn pass_through() -> Self {
        Self::default()
    }

    pub fn handled(operations: Vec<IbusOperation>) -> Self {
        Self {
            handled: true,
            operations,
        }
    }

    pub fn side_effects(operations: Vec<IbusOperation>) -> Self {
        Self {
            handled: false,
            operations,
        }
    }

    pub fn key_handled(&self) -> bool {
        self.handled
    }

    pub fn operations(&self) -> &[IbusOperation] {
        &self.operations
    }
}

pub fn ibus_runtime_decision(action: EngineAction) -> IbusRuntimeDecision {
    match action {
        EngineAction::PassThrough => IbusRuntimeDecision::pass_through(),
        EngineAction::ShowCandidate { replacement, .. } => {
            let cursor_pos = replacement.chars().count() as u32;
            IbusRuntimeDecision::side_effects(vec![IbusOperation::UpdatePreeditText {
                text: replacement,
                cursor_pos,
                visible: true,
            }])
        }
        EngineAction::ClearCandidate => {
            IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        }
        EngineAction::CommitReplacement {
            delete_previous_chars,
            text,
        } => IbusRuntimeDecision::handled(vec![
            IbusOperation::DeleteSurroundingText {
                offset: -(delete_previous_chars as i32),
                n_chars: delete_previous_chars as u32,
            },
            IbusOperation::CommitText(text),
            IbusOperation::HidePreeditText,
        ]),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IbusTextShortcutsRuntime {
    state: EngineState,
    table: ShortcutTable,
    content_purpose: ContentPurpose,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IbusRuntimeEvent {
    Key(IbusKeyEvent),
    FocusIn(ContentPurpose),
    FocusOut,
    Reset,
    ContentPurposeChanged(ContentPurpose),
    TableChanged(ShortcutTable),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeTableRefresh {
    status: TableLoadStatus,
    decision: IbusRuntimeDecision,
}

impl RuntimeTableRefresh {
    pub fn status(&self) -> &TableLoadStatus {
        &self.status
    }

    pub fn decision(&self) -> &IbusRuntimeDecision {
        &self.decision
    }
}

impl IbusTextShortcutsRuntime {
    pub fn new(table: ShortcutTable) -> Self {
        Self {
            state: EngineState::default(),
            table,
            content_purpose: ContentPurpose::Normal,
        }
    }

    pub fn set_table(&mut self, table: ShortcutTable) -> IbusRuntimeDecision {
        self.table = table;
        self.clear_state()
    }

    pub fn refresh_table(&mut self, store: &TextShortcutTableStore) -> RuntimeTableRefresh {
        let outcome = store.load();
        let status = outcome.status().clone();
        let decision = self.set_table(outcome.into_table());
        RuntimeTableRefresh { status, decision }
    }

    pub fn set_content_purpose(&mut self, purpose: ContentPurpose) -> IbusRuntimeDecision {
        self.content_purpose = purpose;
        if purpose.permits_replacement() {
            IbusRuntimeDecision::pass_through()
        } else {
            self.clear_state()
        }
    }

    pub fn content_purpose(&self) -> ContentPurpose {
        self.content_purpose
    }

    pub fn current_word(&self) -> &str {
        self.state.current_word()
    }

    pub fn handle_event(&mut self, event: IbusRuntimeEvent) -> IbusRuntimeDecision {
        match event {
            IbusRuntimeEvent::Key(event) => self.handle_key(event),
            IbusRuntimeEvent::FocusIn(purpose)
            | IbusRuntimeEvent::ContentPurposeChanged(purpose) => self.set_content_purpose(purpose),
            IbusRuntimeEvent::FocusOut | IbusRuntimeEvent::Reset => self.clear_state(),
            IbusRuntimeEvent::TableChanged(table) => self.set_table(table),
        }
    }

    pub fn handle_key(&mut self, event: IbusKeyEvent) -> IbusRuntimeDecision {
        let input = input_event_from_ibus_key(event);
        let action = self
            .state
            .handle_event(self.content_purpose, input, &self.table);
        ibus_runtime_decision(action)
    }

    fn clear_state(&mut self) -> IbusRuntimeDecision {
        let action = self
            .state
            .handle_event(self.content_purpose, InputEvent::Reset, &self.table);
        ibus_runtime_decision(action)
    }
}

impl Default for IbusTextShortcutsRuntime {
    fn default() -> Self {
        Self::new(ShortcutTable::default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeystrokeSelfTestError {
    UnexpectedDecision {
        phase: &'static str,
        expected: IbusRuntimeDecision,
        actual: IbusRuntimeDecision,
    },
}

impl std::fmt::Display for KeystrokeSelfTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedDecision {
                phase,
                expected,
                actual,
            } => write!(
                f,
                "unexpected Text Shortcuts keystroke decision during {phase}: expected {expected:?}, got {actual:?}"
            ),
        }
    }
}

impl std::error::Error for KeystrokeSelfTestError {}

pub fn run_text_shortcuts_keystroke_self_test() -> Result<(), KeystrokeSelfTestError> {
    let table = ShortcutTable::from_shortcuts(vec![TextShortcut::new("omw", "on my way")]);

    let mut runtime = IbusTextShortcutsRuntime::new(table.clone());
    let candidate = type_runtime_text_for_self_test(&mut runtime, "omw");
    expect_keystroke_decision(
        "candidate-preedit",
        candidate,
        IbusRuntimeDecision::side_effects(vec![IbusOperation::UpdatePreeditText {
            text: "on my way".to_string(),
            cursor_pos: 9,
            visible: true,
        }]),
    )?;
    expect_keystroke_decision(
        "boundary-commit",
        runtime.handle_event(IbusRuntimeEvent::Key(char_key_event_for_self_test(' '))),
        IbusRuntimeDecision::handled(vec![
            IbusOperation::DeleteSurroundingText {
                offset: -3,
                n_chars: 3,
            },
            IbusOperation::CommitText("on my way ".to_string()),
            IbusOperation::HidePreeditText,
        ]),
    )?;

    let mut password_runtime = IbusTextShortcutsRuntime::new(table.clone());
    expect_keystroke_decision(
        "password-focus",
        password_runtime.handle_event(IbusRuntimeEvent::FocusIn(ContentPurpose::Password)),
        IbusRuntimeDecision::pass_through(),
    )?;
    expect_keystroke_decision(
        "password-pass-through",
        type_runtime_text_for_self_test(&mut password_runtime, "omw "),
        IbusRuntimeDecision::pass_through(),
    )?;

    let mut focus_runtime = IbusTextShortcutsRuntime::new(table);
    let focus_candidate = type_runtime_text_for_self_test(&mut focus_runtime, "omw");
    expect_keystroke_decision(
        "focus-candidate",
        focus_candidate,
        IbusRuntimeDecision::side_effects(vec![IbusOperation::UpdatePreeditText {
            text: "on my way".to_string(),
            cursor_pos: 9,
            visible: true,
        }]),
    )?;
    expect_keystroke_decision(
        "focus-out",
        focus_runtime.handle_event(IbusRuntimeEvent::FocusOut),
        IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText]),
    )
}

fn type_runtime_text_for_self_test(
    runtime: &mut IbusTextShortcutsRuntime,
    value: &str,
) -> IbusRuntimeDecision {
    let mut decision = IbusRuntimeDecision::pass_through();
    for character in value.chars() {
        decision = runtime.handle_event(IbusRuntimeEvent::Key(char_key_event_for_self_test(
            character,
        )));
    }
    decision
}

fn char_key_event_for_self_test(value: char) -> IbusKeyEvent {
    IbusKeyEvent::new(value as u32, Some(value), true, false)
}

fn expect_keystroke_decision(
    phase: &'static str,
    actual: IbusRuntimeDecision,
    expected: IbusRuntimeDecision,
) -> Result<(), KeystrokeSelfTestError> {
    if actual == expected {
        Ok(())
    } else {
        Err(KeystrokeSelfTestError::UnexpectedDecision {
            phase,
            expected,
            actual,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EngineState {
    current_word: String,
    candidate_visible: bool,
}

impl EngineState {
    pub fn current_word(&self) -> &str {
        &self.current_word
    }

    pub fn handle_event(
        &mut self,
        purpose: ContentPurpose,
        event: InputEvent,
        table: &ShortcutTable,
    ) -> EngineAction {
        if !purpose.permits_replacement() {
            return self.clear_sensitive_state();
        }

        match event {
            InputEvent::Character(value) => self.handle_character(value, table),
            InputEvent::Boundary(value) => self.handle_boundary(value, table),
            InputEvent::Backspace => self.handle_backspace(table),
            InputEvent::Reset => self.clear_candidate(),
            InputEvent::Other => EngineAction::PassThrough,
        }
    }

    fn handle_character(&mut self, value: char, table: &ShortcutTable) -> EngineAction {
        self.current_word.push(value);
        self.candidate_for_current_word(table)
    }

    fn handle_boundary(&mut self, value: char, table: &ShortcutTable) -> EngineAction {
        if let Some(replacement) = table.replacement_for(&self.current_word) {
            let delete_previous_chars = self.current_word.chars().count();
            let text = format!("{replacement}{value}");
            self.current_word.clear();
            self.candidate_visible = false;
            EngineAction::CommitReplacement {
                delete_previous_chars,
                text,
            }
        } else {
            self.clear_candidate()
        }
    }

    fn handle_backspace(&mut self, table: &ShortcutTable) -> EngineAction {
        self.current_word.pop();
        if self.current_word.is_empty() {
            self.clear_candidate()
        } else {
            self.candidate_for_current_word(table)
        }
    }

    fn candidate_for_current_word(&mut self, table: &ShortcutTable) -> EngineAction {
        if let Some(replacement) = table.replacement_for(&self.current_word) {
            self.candidate_visible = true;
            EngineAction::ShowCandidate {
                trigger: self.current_word.clone(),
                replacement: replacement.to_string(),
            }
        } else if self.candidate_visible {
            self.candidate_visible = false;
            EngineAction::ClearCandidate
        } else {
            EngineAction::PassThrough
        }
    }

    fn clear_sensitive_state(&mut self) -> EngineAction {
        self.current_word.clear();
        self.clear_candidate()
    }

    fn clear_candidate(&mut self) -> EngineAction {
        let had_candidate = self.candidate_visible;
        self.current_word.clear();
        self.candidate_visible = false;
        if had_candidate {
            EngineAction::ClearCandidate
        } else {
            EngineAction::PassThrough
        }
    }
}

pub fn is_boundary_char(value: char) -> bool {
    value.is_whitespace() || matches!(value, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}')
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        default_text_shortcuts_table_path, ibus_runtime_decision, input_event_from_ibus_key,
        run_text_shortcuts_keystroke_self_test, sanitize_shortcuts, ContentPurpose, EngineAction,
        EngineState, IbusKeyEvent, IbusOperation, IbusRuntimeDecision, IbusRuntimeEvent,
        IbusTextShortcutsRuntime, InputEvent, ShortcutTable, TableLoadStatus, TextShortcut,
        TextShortcutTableStore, IBUS_KEY_BACKSPACE, IBUS_KEY_DELETE, IBUS_KEY_DOWN,
        IBUS_KEY_ESCAPE, IBUS_KEY_LEFT, IBUS_KEY_RETURN, IBUS_KEY_RIGHT, IBUS_KEY_TAB, IBUS_KEY_UP,
    };

    fn table() -> ShortcutTable {
        ShortcutTable::from_shortcuts(vec![
            TextShortcut::new("omw", "on my way"),
            TextShortcut::new("teh", "the"),
        ])
    }

    fn type_chars(state: &mut EngineState, value: &str, table: &ShortcutTable) -> EngineAction {
        let mut action = EngineAction::PassThrough;
        for character in value.chars() {
            action = state.handle_event(
                ContentPurpose::Normal,
                InputEvent::from_typed_char(character),
                table,
            );
        }
        action
    }

    fn ibus_char(value: char) -> IbusKeyEvent {
        IbusKeyEvent::new(value as u32, Some(value), true, false)
    }

    fn temp_table_path(slug: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "goblins-os-textshortcuts-engine-{}-{slug}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        path
    }

    fn type_ibus_chars(runtime: &mut IbusTextShortcutsRuntime, value: &str) -> IbusRuntimeDecision {
        let mut decision = IbusRuntimeDecision::pass_through();
        for character in value.chars() {
            decision = runtime.handle_key(ibus_char(character));
        }
        decision
    }

    #[test]
    fn sanitizes_table_like_core_bridge() {
        let shortcuts = sanitize_shortcuts(vec![
            TextShortcut::new("  omw ", " on my way "),
            TextShortcut::new("same", "same"),
            TextShortcut::new("", "value"),
            TextShortcut::new("drop", ""),
            TextShortcut::new("omw", "on my way now"),
        ]);
        assert_eq!(shortcuts, vec![TextShortcut::new("omw", "on my way now")]);
    }

    #[test]
    fn table_store_path_uses_the_goblins_config_contract() {
        assert_eq!(
            default_text_shortcuts_table_path("/tmp/config"),
            std::path::PathBuf::from("/tmp/config/goblins-os/text-shortcuts.json")
        );
        assert_eq!(
            TextShortcutTableStore::from_config_home("/tmp/config").path(),
            std::path::Path::new("/tmp/config/goblins-os/text-shortcuts.json")
        );
    }

    #[test]
    fn table_store_loads_and_sanitizes_shortcuts() {
        let path = temp_table_path("load");
        fs::write(
            &path,
            r#"
[
  {"replace":" omw ","with":" on my way "},
  {"replace":"same","with":"same"},
  {"replace":"omw","with":"on my way now"}
]
"#,
        )
        .unwrap();

        let outcome = TextShortcutTableStore::new(&path).load();
        assert_eq!(outcome.status(), &TableLoadStatus::Loaded { shortcuts: 1 });
        assert_eq!(
            outcome.table().replacement_for("omw"),
            Some("on my way now")
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn table_store_missing_or_invalid_tables_degrade_to_empty() {
        let missing_path = temp_table_path("missing");
        let missing = TextShortcutTableStore::new(&missing_path).load();
        assert_eq!(missing.status(), &TableLoadStatus::Missing);
        assert!(missing.table().is_empty());

        let invalid_path = temp_table_path("invalid");
        fs::write(&invalid_path, "not-json").unwrap();
        let invalid = TextShortcutTableStore::new(&invalid_path).load();
        assert_eq!(invalid.status(), &TableLoadStatus::InvalidJson);
        assert!(invalid.table().is_empty());
        fs::remove_file(invalid_path).unwrap();
    }

    #[test]
    fn valid_component_xml_matches_the_registration_contract() {
        let xml = r#"
<component>
  <exec>/usr/libexec/goblins-os/goblins-textshortcuts-engine --ibus</exec>
  <engines>
    <engine>
      <name>goblins-textshortcuts</name>
      <longname>Goblins Text Shortcuts</longname>
      <layout>default</layout>
    </engine>
  </engines>
</component>
"#;

        assert_eq!(super::validate_ibus_component_xml(xml), Ok(()));
    }

    #[test]
    fn component_xml_rejects_wrong_exec_target() {
        let xml = r#"
<component>
  <exec>/usr/bin/ibus-engine-simple</exec>
  <engines>
    <engine>
      <name>goblins-textshortcuts</name>
      <longname>Goblins Text Shortcuts</longname>
      <layout>default</layout>
    </engine>
  </engines>
</component>
"#;

        assert_eq!(
            super::validate_ibus_component_xml(xml),
            Err(super::ComponentContractError::MissingTagValue {
                tag: "exec",
                expected: super::IBUS_COMPONENT_EXEC
            })
        );
    }

    #[test]
    fn shows_candidate_then_commits_on_boundary() {
        let table = table();
        let mut state = EngineState::default();
        assert_eq!(
            type_chars(&mut state, "omw", &table),
            EngineAction::ShowCandidate {
                trigger: "omw".to_string(),
                replacement: "on my way".to_string()
            }
        );
        assert_eq!(
            state.handle_event(ContentPurpose::Normal, InputEvent::Boundary(' '), &table),
            EngineAction::CommitReplacement {
                delete_previous_chars: 3,
                text: "on my way ".to_string()
            }
        );
        assert_eq!(state.current_word(), "");
    }

    #[test]
    fn commits_with_punctuation_boundary() {
        let table = table();
        let mut state = EngineState::default();
        assert_eq!(
            type_chars(&mut state, "teh", &table),
            EngineAction::ShowCandidate {
                trigger: "teh".to_string(),
                replacement: "the".to_string()
            }
        );
        assert_eq!(
            state.handle_event(ContentPurpose::Normal, InputEvent::Boundary('.'), &table),
            EngineAction::CommitReplacement {
                delete_previous_chars: 3,
                text: "the.".to_string()
            }
        );
    }

    #[test]
    fn unknown_words_pass_through_and_clear_on_boundary() {
        let table = table();
        let mut state = EngineState::default();
        assert_eq!(
            type_chars(&mut state, "hello", &table),
            EngineAction::PassThrough
        );
        assert_eq!(
            state.handle_event(ContentPurpose::Normal, InputEvent::Boundary(' '), &table),
            EngineAction::PassThrough
        );
        assert_eq!(state.current_word(), "");
    }

    #[test]
    fn backspace_recomputes_candidate_state() {
        let table = table();
        let mut state = EngineState::default();
        assert!(matches!(
            type_chars(&mut state, "omw", &table),
            EngineAction::ShowCandidate { .. }
        ));
        assert_eq!(
            state.handle_event(ContentPurpose::Normal, InputEvent::Backspace, &table),
            EngineAction::ClearCandidate
        );
        assert_eq!(state.current_word(), "om");
    }

    #[test]
    fn sensitive_content_purposes_never_replace() {
        let table = table();
        for purpose in [
            ContentPurpose::Password,
            ContentPurpose::HiddenText,
            ContentPurpose::Sensitive,
        ] {
            let mut state = EngineState::default();
            for character in "omw".chars() {
                assert_eq!(
                    state.handle_event(purpose, InputEvent::Character(character), &table),
                    EngineAction::PassThrough
                );
            }
            assert_eq!(
                state.handle_event(purpose, InputEvent::Boundary(' '), &table),
                EngineAction::PassThrough
            );
            assert_eq!(state.current_word(), "");
        }
    }

    #[test]
    fn parses_core_table_json_shape() {
        let table = ShortcutTable::from_json(
            r#"[{"replace":"brb","with":"be right back"},{"replace":"brb","with":"back soon"}]"#,
        )
        .unwrap();
        assert_eq!(table.replacement_for("brb"), Some("back soon"));
    }

    #[test]
    fn ibus_key_events_normalize_printable_boundaries_and_backspace() {
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new('o' as u32, Some('o'), true, false)),
            InputEvent::Character('o')
        );
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new(' ' as u32, Some(' '), true, false)),
            InputEvent::Boundary(' ')
        );
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new('.' as u32, Some('.'), true, false)),
            InputEvent::Boundary('.')
        );
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new(IBUS_KEY_BACKSPACE, None, true, false)),
            InputEvent::Backspace
        );
    }

    #[test]
    fn ibus_key_events_reset_on_navigation_and_command_modifiers() {
        for keyval in [
            IBUS_KEY_ESCAPE,
            IBUS_KEY_LEFT,
            IBUS_KEY_UP,
            IBUS_KEY_RIGHT,
            IBUS_KEY_DOWN,
            IBUS_KEY_DELETE,
        ] {
            assert_eq!(
                input_event_from_ibus_key(IbusKeyEvent::new(keyval, None, true, false)),
                InputEvent::Reset
            );
        }
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new('w' as u32, Some('w'), true, true)),
            InputEvent::Reset
        );
    }

    #[test]
    fn ibus_key_events_keep_releases_and_unknown_keys_passthrough() {
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new('o' as u32, Some('o'), false, false)),
            InputEvent::Other
        );
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new(0, None, true, false)),
            InputEvent::Other
        );
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new(0, Some('\u{7f}'), true, false)),
            InputEvent::Other
        );
    }

    #[test]
    fn ibus_key_events_treat_return_and_tab_as_boundaries() {
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new(IBUS_KEY_RETURN, None, true, false)),
            InputEvent::Boundary('\n')
        );
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new(IBUS_KEY_TAB, None, true, false)),
            InputEvent::Boundary('\t')
        );
    }

    #[test]
    fn ibus_adapter_passes_through_plain_keys() {
        let decision = ibus_runtime_decision(EngineAction::PassThrough);
        assert!(!decision.key_handled());
        assert!(decision.operations().is_empty());
    }

    #[test]
    fn ibus_adapter_shows_candidate_without_swallowing_typed_key() {
        let decision = ibus_runtime_decision(EngineAction::ShowCandidate {
            trigger: "omw".to_string(),
            replacement: "on my way".to_string(),
        });
        assert_eq!(
            decision,
            IbusRuntimeDecision::side_effects(vec![IbusOperation::UpdatePreeditText {
                text: "on my way".to_string(),
                cursor_pos: 9,
                visible: true,
            }])
        );
    }

    #[test]
    fn ibus_adapter_commits_replacement_atomically_on_boundary() {
        let decision = ibus_runtime_decision(EngineAction::CommitReplacement {
            delete_previous_chars: 3,
            text: "on my way ".to_string(),
        });
        assert_eq!(
            decision,
            IbusRuntimeDecision::handled(vec![
                IbusOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                },
                IbusOperation::CommitText("on my way ".to_string()),
                IbusOperation::HidePreeditText,
            ])
        );
    }

    #[test]
    fn ibus_adapter_hides_candidate_without_swallowing_backspace() {
        let decision = ibus_runtime_decision(EngineAction::ClearCandidate);
        assert_eq!(
            decision,
            IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        );
    }

    #[test]
    fn ibus_runtime_pipeline_shows_candidate_then_commits_replacement() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        let candidate = type_ibus_chars(&mut runtime, "omw");
        assert_eq!(
            candidate,
            IbusRuntimeDecision::side_effects(vec![IbusOperation::UpdatePreeditText {
                text: "on my way".to_string(),
                cursor_pos: 9,
                visible: true,
            }])
        );
        assert!(!candidate.key_handled());
        assert_eq!(runtime.current_word(), "omw");

        let committed = runtime.handle_key(ibus_char(' '));
        assert_eq!(
            committed,
            IbusRuntimeDecision::handled(vec![
                IbusOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                },
                IbusOperation::CommitText("on my way ".to_string()),
                IbusOperation::HidePreeditText,
            ])
        );
        assert_eq!(runtime.current_word(), "");
    }

    #[test]
    fn ibus_runtime_pipeline_passes_unknown_words_and_releases() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            type_ibus_chars(&mut runtime, "hello"),
            IbusRuntimeDecision::pass_through()
        );
        assert_eq!(runtime.current_word(), "hello");
        assert_eq!(
            runtime.handle_key(IbusKeyEvent::new('o' as u32, Some('o'), false, false)),
            IbusRuntimeDecision::pass_through()
        );
        assert_eq!(runtime.current_word(), "hello");
        assert_eq!(
            runtime.handle_key(ibus_char(' ')),
            IbusRuntimeDecision::pass_through()
        );
        assert_eq!(runtime.current_word(), "");
    }

    #[test]
    fn ibus_runtime_pipeline_clears_on_sensitive_content_purpose() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::side_effects(vec![IbusOperation::UpdatePreeditText {
                text: "on my way".to_string(),
                cursor_pos: 9,
                visible: true,
            }])
        );

        assert_eq!(
            runtime.set_content_purpose(ContentPurpose::Password),
            IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        );
        assert_eq!(runtime.content_purpose(), ContentPurpose::Password);
        assert_eq!(runtime.current_word(), "");
        assert_eq!(
            runtime.handle_key(ibus_char(' ')),
            IbusRuntimeDecision::pass_through()
        );
    }

    #[test]
    fn ibus_runtime_pipeline_command_modifier_resets_without_commit() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert!(matches!(
            type_ibus_chars(&mut runtime, "omw").operations(),
            [IbusOperation::UpdatePreeditText { .. }]
        ));

        assert_eq!(
            runtime.handle_key(IbusKeyEvent::new('c' as u32, Some('c'), true, true)),
            IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        );
        assert_eq!(runtime.current_word(), "");
        assert_eq!(
            runtime.handle_key(ibus_char(' ')),
            IbusRuntimeDecision::pass_through()
        );
    }

    #[test]
    fn ibus_runtime_event_router_commits_key_events() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        for character in "omw".chars() {
            runtime.handle_event(IbusRuntimeEvent::Key(ibus_char(character)));
        }
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::Key(ibus_char(' '))),
            IbusRuntimeDecision::handled(vec![
                IbusOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                },
                IbusOperation::CommitText("on my way ".to_string()),
                IbusOperation::HidePreeditText,
            ])
        );
    }

    #[test]
    fn ibus_runtime_event_router_focus_out_and_reset_clear_candidates() {
        for event in [IbusRuntimeEvent::FocusOut, IbusRuntimeEvent::Reset] {
            let mut runtime = IbusTextShortcutsRuntime::new(table());
            assert!(matches!(
                type_ibus_chars(&mut runtime, "omw").operations(),
                [IbusOperation::UpdatePreeditText { .. }]
            ));
            assert_eq!(
                runtime.handle_event(event.clone()),
                IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
            );
            assert_eq!(runtime.current_word(), "");
            assert_eq!(
                runtime.handle_event(IbusRuntimeEvent::Key(ibus_char(' '))),
                IbusRuntimeDecision::pass_through()
            );
        }
    }

    #[test]
    fn ibus_runtime_event_router_refuses_sensitive_focus() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::FocusIn(ContentPurpose::Password)),
            IbusRuntimeDecision::pass_through()
        );
        assert_eq!(runtime.content_purpose(), ContentPurpose::Password);
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::pass_through()
        );
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::Key(ibus_char(' '))),
            IbusRuntimeDecision::pass_through()
        );
    }

    #[test]
    fn ibus_runtime_event_router_content_purpose_change_hides_candidate() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert!(matches!(
            type_ibus_chars(&mut runtime, "omw").operations(),
            [IbusOperation::UpdatePreeditText { .. }]
        ));
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::ContentPurposeChanged(
                ContentPurpose::Sensitive
            )),
            IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        );
        assert_eq!(runtime.current_word(), "");
        assert_eq!(runtime.content_purpose(), ContentPurpose::Sensitive);
    }

    #[test]
    fn ibus_runtime_event_router_table_change_hides_stale_candidate() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert!(matches!(
            type_ibus_chars(&mut runtime, "omw").operations(),
            [IbusOperation::UpdatePreeditText { .. }]
        ));
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::TableChanged(
                ShortcutTable::from_shortcuts(vec![TextShortcut::new("brb", "be right back")])
            )),
            IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        );
        assert_eq!(runtime.current_word(), "");
        assert_eq!(
            type_ibus_chars(&mut runtime, "brb"),
            IbusRuntimeDecision::side_effects(vec![IbusOperation::UpdatePreeditText {
                text: "be right back".to_string(),
                cursor_pos: 13,
                visible: true,
            }])
        );
    }

    #[test]
    fn text_shortcuts_keystroke_self_test_covers_runtime_contract() {
        assert_eq!(run_text_shortcuts_keystroke_self_test(), Ok(()));
    }

    #[test]
    fn runtime_refresh_reloads_table_and_hides_stale_candidate() {
        let path = temp_table_path("refresh");
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert!(matches!(
            type_ibus_chars(&mut runtime, "omw").operations(),
            [IbusOperation::UpdatePreeditText { .. }]
        ));

        fs::write(&path, r#"[{"replace":"brb","with":"be right back"}]"#).unwrap();
        let refresh = runtime.refresh_table(&TextShortcutTableStore::new(&path));
        assert_eq!(refresh.status(), &TableLoadStatus::Loaded { shortcuts: 1 });
        assert_eq!(
            refresh.decision(),
            &IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        );
        assert_eq!(runtime.current_word(), "");
        assert_eq!(
            runtime.handle_key(ibus_char(' ')),
            IbusRuntimeDecision::pass_through()
        );
        fs::remove_file(path).unwrap();
    }
}
