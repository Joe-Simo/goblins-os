//! Pure decision logic for the Goblins Text Shortcuts IBus engine.
//!
//! The live IBus/GNOME integration is intentionally CI/qemu-gated. This crate
//! owns the host-testable substrate: table sanitizing, word-boundary trigger
//! tracking, replacement commit decisions, and hard refusal in sensitive text
//! fields.

use serde::{Deserialize, Serialize};

const MAX_SHORTCUTS: usize = 500;

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
    use super::{
        sanitize_shortcuts, ContentPurpose, EngineAction, EngineState, InputEvent, ShortcutTable,
        TextShortcut,
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
}
