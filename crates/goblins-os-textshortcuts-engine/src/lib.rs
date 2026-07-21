//! Pure decision logic for the Goblins Text Shortcuts IBus engine.
//!
//! The live IBus/GNOME integration is intentionally CI/qemu-gated. This crate
//! owns the host-testable substrate: table sanitizing, word-boundary trigger
//! tracking, replacement commit decisions, and hard refusal in sensitive text
//! fields.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

const MAX_SHORTCUTS: usize = 500;
pub const MAX_TEXT_SHORTCUTS_TABLE_BYTES: usize = 48 * 1024;
pub const TEXT_SHORTCUTS_CONFIG_DIR: &str = "goblins-os";
pub const TEXT_SHORTCUTS_CONFIG_FILE: &str = "text-shortcuts.json";
pub const IBUS_ENGINE_NAME: &str = "goblins-textshortcuts";
pub const IBUS_COMPONENT_EXEC: &str = "/usr/libexec/goblins-os/goblins-textshortcuts-ibus";
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
        if replace.is_empty()
            || with_text.is_empty()
            || replace == with_text
            || replace.contains('\0')
            || with_text.contains('\0')
        {
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

/// Whether the canonical JSON representation, including its trailing newline,
/// fits the one bounded table contract shared by the editor and runtime.
pub fn text_shortcuts_table_is_within_size_limit(shortcuts: &[TextShortcut]) -> bool {
    serde_json::to_vec(shortcuts)
        .is_ok_and(|raw| raw.len().saturating_add(1) <= MAX_TEXT_SHORTCUTS_TABLE_BYTES)
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
    TooLarge,
    UnsafeFile,
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
            Self::TooLarge => {
                "Text Shortcuts table exceeds the private storage limit; expansion is disabled until it is reduced."
            }
            Self::UnsafeFile => {
                "Text Shortcuts table is not a private regular file; expansion is disabled until it is fixed."
            }
            Self::Unreadable => {
                "Text Shortcuts table could not be read; expansion is disabled until it is accessible."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableFingerprint {
    Present { bytes: u64, content_hash: u64 },
    Missing,
    TooLarge { bytes: u64 },
    UnsafeFile,
    Unreadable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableWriteError {
    TooLarge,
    UnsafeParent,
    UnsafeDestination,
    Io,
}

impl std::fmt::Display for TableWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "Text Shortcuts table exceeds the private storage limit.",
            Self::UnsafeParent => {
                "Text Shortcuts private configuration directory is not safe to use."
            }
            Self::UnsafeDestination => "Text Shortcuts table is not a private regular file.",
            Self::Io => "Text Shortcuts table could not be saved privately.",
        })
    }
}

impl std::error::Error for TableWriteError {}

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
pub struct TableLoadSnapshot {
    outcome: TableLoadOutcome,
    fingerprint: TableFingerprint,
}

impl TableLoadSnapshot {
    pub fn table(&self) -> &ShortcutTable {
        self.outcome.table()
    }

    pub fn status(&self) -> &TableLoadStatus {
        self.outcome.status()
    }

    pub fn fingerprint(&self) -> TableFingerprint {
        self.fingerprint
    }

    pub fn into_outcome(self) -> TableLoadOutcome {
        self.outcome
    }

    pub fn into_parts(self) -> (TableLoadOutcome, TableFingerprint) {
        (self.outcome, self.fingerprint)
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
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|home| home.is_absolute())
                    .map(|home| home.join(".config"))
            })
            .ok_or(TableStoreError::NoConfigHome)?;
        Ok(Self::from_config_home(base))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> TableLoadOutcome {
        self.load_snapshot().into_outcome()
    }

    pub fn load_snapshot(&self) -> TableLoadSnapshot {
        match read_bounded_table(&self.path) {
            BoundedTableRead::Present(raw) => {
                let fingerprint = TableFingerprint::Present {
                    bytes: raw.len() as u64,
                    content_hash: stable_content_hash(&raw),
                };
                let outcome = match std::str::from_utf8(&raw)
                    .ok()
                    .and_then(|raw| ShortcutTable::from_json(raw).ok())
                {
                    Some(table) => TableLoadOutcome {
                        status: TableLoadStatus::Loaded {
                            shortcuts: table.len(),
                        },
                        table,
                    },
                    None => TableLoadOutcome {
                        table: ShortcutTable::default(),
                        status: TableLoadStatus::InvalidJson,
                    },
                };
                TableLoadSnapshot {
                    outcome,
                    fingerprint,
                }
            }
            BoundedTableRead::Missing => TableLoadSnapshot {
                outcome: TableLoadOutcome {
                    table: ShortcutTable::default(),
                    status: TableLoadStatus::Missing,
                },
                fingerprint: TableFingerprint::Missing,
            },
            BoundedTableRead::TooLarge { bytes } => TableLoadSnapshot {
                outcome: TableLoadOutcome {
                    table: ShortcutTable::default(),
                    status: TableLoadStatus::TooLarge,
                },
                fingerprint: TableFingerprint::TooLarge { bytes },
            },
            BoundedTableRead::UnsafeFile => TableLoadSnapshot {
                outcome: TableLoadOutcome {
                    table: ShortcutTable::default(),
                    status: TableLoadStatus::UnsafeFile,
                },
                fingerprint: TableFingerprint::UnsafeFile,
            },
            BoundedTableRead::Unreadable => TableLoadSnapshot {
                outcome: TableLoadOutcome {
                    table: ShortcutTable::default(),
                    status: TableLoadStatus::Unreadable,
                },
                fingerprint: TableFingerprint::Unreadable,
            },
        }
    }

    /// Canonicalize and atomically replace the desktop user's one private table.
    /// The temporary file lives beside the destination, so readers observe the
    /// complete old table or the complete new table, never a partial write.
    pub fn save(&self, shortcuts: Vec<TextShortcut>) -> Result<Vec<TextShortcut>, TableWriteError> {
        let shortcuts = sanitize_shortcuts(shortcuts);
        let mut raw = serde_json::to_vec(&shortcuts).map_err(|_| TableWriteError::Io)?;
        raw.push(b'\n');
        if raw.len() > MAX_TEXT_SHORTCUTS_TABLE_BYTES {
            return Err(TableWriteError::TooLarge);
        }

        let parent = self.path.parent().ok_or(TableWriteError::UnsafeParent)?;
        prepare_private_table_parent(parent)?;
        validate_table_destination(&self.path)?;

        let (mut temporary, temporary_path) = create_private_temporary(parent)?;
        let write_result = (|| {
            temporary.write_all(&raw).map_err(|_| TableWriteError::Io)?;
            temporary.flush().map_err(|_| TableWriteError::Io)?;
            temporary.sync_all().map_err(|_| TableWriteError::Io)?;
            temporary
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|_| TableWriteError::Io)?;
            std::fs::rename(&temporary_path, &self.path).map_err(|_| TableWriteError::Io)?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| TableWriteError::Io)
        })();
        if write_result.is_err() {
            let _ = std::fs::remove_file(&temporary_path);
        }
        write_result.map(|()| shortcuts)
    }
}

enum BoundedTableRead {
    Present(Vec<u8>),
    Missing,
    TooLarge { bytes: u64 },
    UnsafeFile,
    Unreadable,
}

fn read_bounded_table(path: &Path) -> BoundedTableRead {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return BoundedTableRead::Missing;
        }
        Err(_) => return BoundedTableRead::Unreadable,
    };
    if !private_regular_file_metadata(&metadata, effective_uid()) {
        return BoundedTableRead::UnsafeFile;
    }
    if metadata.len() > MAX_TEXT_SHORTCUTS_TABLE_BYTES as u64 {
        return BoundedTableRead::TooLarge {
            bytes: metadata.len(),
        };
    }

    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(_) => return BoundedTableRead::Unreadable,
    };
    let opened_metadata = match file.metadata() {
        Ok(metadata) if private_regular_file_metadata(&metadata, effective_uid()) => metadata,
        Ok(_) => return BoundedTableRead::UnsafeFile,
        Err(_) => return BoundedTableRead::Unreadable,
    };
    if opened_metadata.len() > MAX_TEXT_SHORTCUTS_TABLE_BYTES as u64 {
        return BoundedTableRead::TooLarge {
            bytes: opened_metadata.len(),
        };
    }

    let mut raw = Vec::with_capacity(opened_metadata.len() as usize);
    if file
        .take((MAX_TEXT_SHORTCUTS_TABLE_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .is_err()
    {
        return BoundedTableRead::Unreadable;
    }
    if raw.len() > MAX_TEXT_SHORTCUTS_TABLE_BYTES {
        return BoundedTableRead::TooLarge {
            bytes: raw.len() as u64,
        };
    }
    BoundedTableRead::Present(raw)
}

fn prepare_private_table_parent(parent: &Path) -> Result<(), TableWriteError> {
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) => validate_private_table_parent_metadata(&metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(parent).map_err(|_| TableWriteError::Io)?;
            let metadata = std::fs::symlink_metadata(parent).map_err(|_| TableWriteError::Io)?;
            validate_private_table_parent_metadata(&metadata)?;
        }
        Err(_) => return Err(TableWriteError::Io),
    }
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| TableWriteError::Io)
}

fn validate_private_table_parent_metadata(
    metadata: &std::fs::Metadata,
) -> Result<(), TableWriteError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.uid() != effective_uid()
    {
        Err(TableWriteError::UnsafeParent)
    } else {
        Ok(())
    }
}

fn validate_table_destination(path: &Path) -> Result<(), TableWriteError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.uid() != effective_uid() =>
        {
            Err(TableWriteError::UnsafeDestination)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(TableWriteError::Io),
    }
}

fn private_regular_file_metadata(metadata: &std::fs::Metadata, expected_uid: u32) -> bool {
    !metadata.file_type().is_symlink()
        && metadata.is_file()
        && metadata.uid() == expected_uid
        && metadata.mode() & 0o7777 == 0o600
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    unsafe { libc::geteuid() }
}

static NEXT_TABLE_TEMPORARY: AtomicU64 = AtomicU64::new(1);

fn create_private_temporary(parent: &Path) -> Result<(File, PathBuf), TableWriteError> {
    for _ in 0..128 {
        let sequence = NEXT_TABLE_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".text-shortcuts.json.tmp-{}-{sequence}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(TableWriteError::Io),
        }
    }
    Err(TableWriteError::Io)
}

fn stable_content_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextShortcutTableWatcher {
    fingerprint: Option<TableFingerprint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TableWatchOutcome {
    Reloaded(RuntimeTableRefresh),
    Unchanged { status: TableLoadStatus },
}

impl TableWatchOutcome {
    pub fn status(&self) -> &TableLoadStatus {
        match self {
            Self::Reloaded(refresh) => refresh.status(),
            Self::Unchanged { status } => status,
        }
    }

    pub fn reloaded(&self) -> bool {
        matches!(self, Self::Reloaded(_))
    }

    pub fn decision(&self) -> Option<&IbusRuntimeDecision> {
        match self {
            Self::Reloaded(refresh) => Some(refresh.decision()),
            Self::Unchanged { .. } => None,
        }
    }
}

impl TextShortcutTableWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fingerprint(&self) -> Option<TableFingerprint> {
        self.fingerprint
    }

    pub fn poll(
        &mut self,
        runtime: &mut IbusTextShortcutsRuntime,
        store: &TextShortcutTableStore,
    ) -> TableWatchOutcome {
        let snapshot = store.load_snapshot();
        let fingerprint = snapshot.fingerprint();
        if self.fingerprint == Some(fingerprint) {
            return TableWatchOutcome::Unchanged {
                status: snapshot.status().clone(),
            };
        }

        self.fingerprint = Some(fingerprint);
        let (outcome, _) = snapshot.into_parts();
        let status = outcome.status().clone();
        let decision = runtime.set_table(outcome.into_table());
        TableWatchOutcome::Reloaded(RuntimeTableRefresh { status, decision })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TableWatchSelfTestError {
    Io(String),
    UnexpectedCurrentWord {
        phase: &'static str,
        expected: &'static str,
        actual: String,
    },
    UnexpectedOutcome {
        phase: &'static str,
        expected: &'static str,
        actual: TableWatchOutcome,
    },
    UnexpectedDecision {
        phase: &'static str,
        expected: IbusRuntimeDecision,
        actual: IbusRuntimeDecision,
    },
}

impl std::fmt::Display for TableWatchSelfTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "{message}"),
            Self::UnexpectedCurrentWord {
                phase,
                expected,
                actual,
            } => write!(
                f,
                "unexpected Text Shortcuts current word during {phase}: expected {expected:?}, got {actual:?}"
            ),
            Self::UnexpectedOutcome {
                phase,
                expected,
                actual,
            } => write!(
                f,
                "unexpected Text Shortcuts table-watch outcome during {phase}: expected {expected}, got {actual:?}"
            ),
            Self::UnexpectedDecision {
                phase,
                expected,
                actual,
            } => write!(
                f,
                "unexpected Text Shortcuts table-watch decision during {phase}: expected {expected:?}, got {actual:?}"
            ),
        }
    }
}

impl std::error::Error for TableWatchSelfTestError {}

pub fn run_text_shortcuts_table_watch_self_test() -> Result<(), TableWatchSelfTestError> {
    let path = std::env::temp_dir().join(format!(
        "goblins-os-textshortcuts-watch-selftest-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    write_table_for_watch_self_test(&path, r#"[{"replace":"omw","with":"on my way"}]"#)?;

    let store = TextShortcutTableStore::new(&path);
    let mut runtime = IbusTextShortcutsRuntime::default();
    let mut watcher = TextShortcutTableWatcher::new();

    expect_watch_reload_status(
        "initial-load",
        watcher.poll(&mut runtime, &store),
        TableLoadStatus::Loaded { shortcuts: 1 },
    )?;
    let candidate = type_runtime_text_for_self_test(&mut runtime, "omw");
    expect_watch_decision(
        "initial-candidate",
        candidate,
        IbusRuntimeDecision::show_candidate("omw", "on my way"),
    )?;

    expect_watch_unchanged_status(
        "unchanged-table",
        watcher.poll(&mut runtime, &store),
        TableLoadStatus::Loaded { shortcuts: 1 },
    )?;
    expect_current_word("unchanged-keeps-current-word", &runtime, "omw")?;

    write_table_for_watch_self_test(&path, r#"[{"replace":"omw","with":"on my way now"}]"#)?;
    expect_watch_reload_decision(
        "changed-table",
        watcher.poll(&mut runtime, &store),
        TableLoadStatus::Loaded { shortcuts: 1 },
        IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText]),
    )?;
    expect_current_word("changed-table-clears-current-word", &runtime, "")?;

    let updated_candidate = type_runtime_text_for_self_test(&mut runtime, "omw");
    expect_watch_decision(
        "updated-candidate",
        updated_candidate,
        IbusRuntimeDecision::show_candidate("omw", "on my way now"),
    )?;

    write_table_for_watch_self_test(&path, "not-json")?;
    expect_watch_reload_decision(
        "invalid-table",
        watcher.poll(&mut runtime, &store),
        TableLoadStatus::InvalidJson,
        IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText]),
    )?;
    expect_watch_decision(
        "invalid-table-pass-through",
        type_runtime_text_for_self_test(&mut runtime, "omw "),
        IbusRuntimeDecision::pass_through(),
    )?;

    std::fs::remove_file(&path)
        .map_err(|error| TableWatchSelfTestError::Io(format!("could not remove table: {error}")))?;
    expect_watch_reload_status(
        "missing-table",
        watcher.poll(&mut runtime, &store),
        TableLoadStatus::Missing,
    )?;
    Ok(())
}

fn expect_current_word(
    phase: &'static str,
    runtime: &IbusTextShortcutsRuntime,
    expected: &'static str,
) -> Result<(), TableWatchSelfTestError> {
    if runtime.current_word() == expected {
        Ok(())
    } else {
        Err(TableWatchSelfTestError::UnexpectedCurrentWord {
            phase,
            expected,
            actual: runtime.current_word().to_string(),
        })
    }
}

fn write_table_for_watch_self_test(path: &Path, raw: &str) -> Result<(), TableWatchSelfTestError> {
    std::fs::write(path, raw)
        .and_then(|()| std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)))
        .map_err(|error| TableWatchSelfTestError::Io(format!("could not write table: {error}")))
}

fn expect_watch_reload_status(
    phase: &'static str,
    actual: TableWatchOutcome,
    expected_status: TableLoadStatus,
) -> Result<(), TableWatchSelfTestError> {
    match actual {
        TableWatchOutcome::Reloaded(refresh) if refresh.status == expected_status => Ok(()),
        actual => Err(TableWatchSelfTestError::UnexpectedOutcome {
            phase,
            expected: "table reload with expected status",
            actual,
        }),
    }
}

fn expect_watch_reload_decision(
    phase: &'static str,
    actual: TableWatchOutcome,
    expected_status: TableLoadStatus,
    expected_decision: IbusRuntimeDecision,
) -> Result<(), TableWatchSelfTestError> {
    match actual {
        TableWatchOutcome::Reloaded(refresh)
            if refresh.status == expected_status && refresh.decision == expected_decision =>
        {
            Ok(())
        }
        actual => Err(TableWatchSelfTestError::UnexpectedOutcome {
            phase,
            expected: "table reload with expected status and decision",
            actual,
        }),
    }
}

fn expect_watch_unchanged_status(
    phase: &'static str,
    actual: TableWatchOutcome,
    expected_status: TableLoadStatus,
) -> Result<(), TableWatchSelfTestError> {
    match actual {
        TableWatchOutcome::Unchanged { status } if status == expected_status => Ok(()),
        actual => Err(TableWatchSelfTestError::UnexpectedOutcome {
            phase,
            expected: "unchanged table with expected status",
            actual,
        }),
    }
}

fn expect_watch_decision(
    phase: &'static str,
    actual: IbusRuntimeDecision,
    expected: IbusRuntimeDecision,
) -> Result<(), TableWatchSelfTestError> {
    if actual == expected {
        Ok(())
    } else {
        Err(TableWatchSelfTestError::UnexpectedDecision {
            phase,
            expected,
            actual,
        })
    }
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

pub const IBUS_INPUT_PURPOSE_FREE_FORM: u32 = 0;
pub const IBUS_INPUT_PURPOSE_PASSWORD: u32 = 8;
pub const IBUS_INPUT_PURPOSE_PIN: u32 = 9;

pub fn content_purpose_from_ibus_input_purpose(value: u32) -> ContentPurpose {
    match value {
        IBUS_INPUT_PURPOSE_PASSWORD => ContentPurpose::Password,
        IBUS_INPUT_PURPOSE_PIN => ContentPurpose::HiddenText,
        _ => ContentPurpose::Normal,
    }
}

pub fn content_purpose_from_ibus_input_purpose_name(value: &str) -> ContentPurpose {
    let normalized = value
        .trim()
        .trim_start_matches("IBUS_INPUT_PURPOSE_")
        .trim_start_matches("InputPurpose.")
        .replace(['-', ' '], "_")
        .to_ascii_uppercase();
    match normalized.as_str() {
        "PASSWORD" => ContentPurpose::Password,
        "PIN" | "HIDDEN_TEXT" | "HIDDENTEXT" => ContentPurpose::HiddenText,
        "SENSITIVE" => ContentPurpose::Sensitive,
        _ => ContentPurpose::Normal,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContentPurposeSelfTestError {
    UnexpectedPurpose {
        phase: &'static str,
        expected: ContentPurpose,
        actual: ContentPurpose,
    },
    UnexpectedDecision {
        phase: &'static str,
        expected: IbusRuntimeDecision,
        actual: IbusRuntimeDecision,
    },
}

impl std::fmt::Display for ContentPurposeSelfTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedPurpose {
                phase,
                expected,
                actual,
            } => write!(
                f,
                "unexpected Text Shortcuts content purpose during {phase}: expected {expected:?}, got {actual:?}"
            ),
            Self::UnexpectedDecision {
                phase,
                expected,
                actual,
            } => write!(
                f,
                "unexpected Text Shortcuts content-purpose decision during {phase}: expected {expected:?}, got {actual:?}"
            ),
        }
    }
}

impl std::error::Error for ContentPurposeSelfTestError {}

pub fn run_text_shortcuts_content_purpose_self_test() -> Result<(), ContentPurposeSelfTestError> {
    expect_content_purpose(
        "password-numeric-purpose",
        content_purpose_from_ibus_input_purpose(IBUS_INPUT_PURPOSE_PASSWORD),
        ContentPurpose::Password,
    )?;
    expect_content_purpose(
        "pin-numeric-purpose",
        content_purpose_from_ibus_input_purpose(IBUS_INPUT_PURPOSE_PIN),
        ContentPurpose::HiddenText,
    )?;
    expect_content_purpose(
        "unknown-purpose-free-form",
        content_purpose_from_ibus_input_purpose(999),
        ContentPurpose::Normal,
    )?;
    expect_content_purpose(
        "password-symbolic-purpose",
        content_purpose_from_ibus_input_purpose_name("IBUS_INPUT_PURPOSE_PASSWORD"),
        ContentPurpose::Password,
    )?;
    expect_content_purpose(
        "pin-symbolic-purpose",
        content_purpose_from_ibus_input_purpose_name("InputPurpose.PIN"),
        ContentPurpose::HiddenText,
    )?;

    let table = ShortcutTable::from_shortcuts(vec![TextShortcut::new("omw", "on my way")]);
    let mut runtime = IbusTextShortcutsRuntime::new(table);
    let purpose = content_purpose_from_ibus_input_purpose(IBUS_INPUT_PURPOSE_PIN);
    expect_content_decision(
        "pin-focus",
        runtime.handle_event(IbusRuntimeEvent::FocusIn(purpose)),
        IbusRuntimeDecision::pass_through(),
    )?;
    expect_content_decision(
        "pin-pass-through",
        type_runtime_text_for_self_test(&mut runtime, "omw "),
        IbusRuntimeDecision::pass_through(),
    )
}

fn expect_content_purpose(
    phase: &'static str,
    actual: ContentPurpose,
    expected: ContentPurpose,
) -> Result<(), ContentPurposeSelfTestError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ContentPurposeSelfTestError::UnexpectedPurpose {
            phase,
            expected,
            actual,
        })
    }
}

fn expect_content_decision(
    phase: &'static str,
    actual: IbusRuntimeDecision,
    expected: IbusRuntimeDecision,
) -> Result<(), ContentPurposeSelfTestError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ContentPurposeSelfTestError::UnexpectedDecision {
            phase,
            expected,
            actual,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputEvent {
    Character(char),
    Boundary(char),
    Backspace,
    DismissCandidate,
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
        IBUS_KEY_ESCAPE => InputEvent::DismissCandidate,
        IBUS_KEY_LEFT | IBUS_KEY_UP | IBUS_KEY_RIGHT | IBUS_KEY_DOWN | IBUS_KEY_DELETE => {
            InputEvent::Reset
        }
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
    DismissCandidate,
    CommitReplacement {
        trigger: String,
        text: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IbusOperation {
    HidePreeditText,
    DeleteSurroundingText {
        offset: i32,
        n_chars: u32,
        expected_text: String,
    },
    CommitText(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IbusCandidateMetadata {
    trigger: String,
    replacement: String,
}

impl IbusCandidateMetadata {
    pub fn trigger(&self) -> &str {
        &self.trigger
    }

    pub fn replacement(&self) -> &str {
        &self.replacement
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IbusRuntimeDecision {
    handled: bool,
    operations: Vec<IbusOperation>,
    candidate: Option<Box<IbusCandidateMetadata>>,
}

impl IbusRuntimeDecision {
    pub fn pass_through() -> Self {
        Self::default()
    }

    pub fn handled(operations: Vec<IbusOperation>) -> Self {
        Self {
            handled: true,
            operations,
            candidate: None,
        }
    }

    pub fn side_effects(operations: Vec<IbusOperation>) -> Self {
        Self {
            handled: false,
            operations,
            candidate: None,
        }
    }

    pub fn show_candidate(trigger: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self {
            handled: false,
            operations: Vec::new(),
            candidate: Some(Box::new(IbusCandidateMetadata {
                trigger: trigger.into(),
                replacement: replacement.into(),
            })),
        }
    }

    pub fn key_handled(&self) -> bool {
        self.handled
    }

    pub fn operations(&self) -> &[IbusOperation] {
        &self.operations
    }

    pub fn candidate(&self) -> Option<&IbusCandidateMetadata> {
        self.candidate.as_deref()
    }
}

pub fn ibus_runtime_decision(action: EngineAction) -> IbusRuntimeDecision {
    match action {
        EngineAction::PassThrough => IbusRuntimeDecision::pass_through(),
        EngineAction::ShowCandidate {
            trigger,
            replacement,
        } => IbusRuntimeDecision::show_candidate(trigger, replacement),
        EngineAction::ClearCandidate => {
            IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        }
        EngineAction::DismissCandidate => {
            IbusRuntimeDecision::handled(vec![IbusOperation::HidePreeditText])
        }
        EngineAction::CommitReplacement { trigger, text } => {
            IbusRuntimeDecision::handled(replacement_operations(trigger, text))
        }
    }
}

fn ibus_runtime_decision_forwarding_boundary(
    action: EngineAction,
    boundary: char,
) -> IbusRuntimeDecision {
    match action {
        EngineAction::CommitReplacement { trigger, text } => {
            let replacement = text
                .strip_suffix(boundary)
                .unwrap_or(text.as_str())
                .to_string();
            IbusRuntimeDecision::side_effects(replacement_operations(trigger, replacement))
        }
        action => ibus_runtime_decision(action),
    }
}

fn replacement_operations(expected_text: String, text: String) -> Vec<IbusOperation> {
    let delete_previous_chars = expected_text.chars().count();
    vec![
        IbusOperation::DeleteSurroundingText {
            offset: -(delete_previous_chars as i32),
            n_chars: delete_previous_chars as u32,
            expected_text,
        },
        IbusOperation::CommitText(text),
        IbusOperation::HidePreeditText,
    ]
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RuntimeProtocolRequest {
    Key {
        keyval: u32,
        unicode: Option<String>,
        pressed: bool,
        command_modifier_active: bool,
    },
    FocusIn {
        purpose: RuntimeProtocolPurpose,
    },
    ContentPurposeChanged {
        purpose: RuntimeProtocolPurpose,
    },
    Health,
    AcceptCandidate,
    FocusOut,
    Reset,
    TableChanged {
        shortcuts: Vec<TextShortcut>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum RuntimeProtocolPurpose {
    IbusInputPurpose(u32),
    Name(String),
}

impl RuntimeProtocolPurpose {
    fn into_content_purpose(self) -> ContentPurpose {
        match self {
            Self::IbusInputPurpose(value) => content_purpose_from_ibus_input_purpose(value),
            Self::Name(value) => content_purpose_from_ibus_input_purpose_name(&value),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeProtocolResponse {
    pub handled: bool,
    pub operations: Vec<RuntimeProtocolOperation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<RuntimeProtocolCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeProtocolCandidate {
    pub trigger: String,
    pub replacement: String,
    pub accept_on: String,
    pub dismiss_key: String,
    pub rendered_bubble_ready_claim: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RuntimeProtocolOperation {
    HidePreeditText,
    DeleteSurroundingText {
        offset: i32,
        n_chars: u32,
        expected_text: String,
    },
    CommitText {
        text: String,
    },
}

impl RuntimeProtocolResponse {
    fn from_decision(decision: IbusRuntimeDecision) -> Self {
        let operations: Vec<RuntimeProtocolOperation> = decision
            .operations()
            .iter()
            .map(RuntimeProtocolOperation::from)
            .collect();
        Self {
            handled: decision.key_handled(),
            candidate: decision.candidate().map(RuntimeProtocolCandidate::from),
            operations,
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            handled: false,
            operations: Vec::new(),
            candidate: None,
            error: Some(message.into()),
        }
    }
}

impl From<&IbusCandidateMetadata> for RuntimeProtocolCandidate {
    fn from(candidate: &IbusCandidateMetadata) -> Self {
        Self {
            trigger: candidate.trigger().to_string(),
            replacement: candidate.replacement().to_string(),
            accept_on: "word-boundary".to_string(),
            dismiss_key: "Escape".to_string(),
            rendered_bubble_ready_claim: false,
        }
    }
}

impl From<&IbusOperation> for RuntimeProtocolOperation {
    fn from(operation: &IbusOperation) -> Self {
        match operation {
            IbusOperation::HidePreeditText => Self::HidePreeditText,
            IbusOperation::DeleteSurroundingText {
                offset,
                n_chars,
                expected_text,
            } => Self::DeleteSurroundingText {
                offset: *offset,
                n_chars: *n_chars,
                expected_text: expected_text.clone(),
            },
            IbusOperation::CommitText(text) => Self::CommitText { text: text.clone() },
        }
    }
}

pub fn handle_runtime_protocol_request(
    runtime: &mut IbusTextShortcutsRuntime,
    request: RuntimeProtocolRequest,
) -> RuntimeProtocolResponse {
    match runtime_protocol_request_event(request) {
        Ok(event) => {
            let decision = runtime.handle_event(event);
            RuntimeProtocolResponse::from_decision(decision)
        }
        Err(message) => RuntimeProtocolResponse::error(message),
    }
}

pub fn handle_runtime_protocol_line(
    runtime: &mut IbusTextShortcutsRuntime,
    line: &str,
) -> RuntimeProtocolResponse {
    match serde_json::from_str::<RuntimeProtocolRequest>(line) {
        Ok(request) => handle_runtime_protocol_request(runtime, request),
        Err(error) => RuntimeProtocolResponse::error(format!("invalid runtime request: {error}")),
    }
}

pub fn run_text_shortcuts_stdio_runtime<R: BufRead, W: Write>(
    mut runtime: IbusTextShortcutsRuntime,
    reader: R,
    mut writer: W,
) -> Result<(), RuntimeProtocolIoError> {
    for line in reader.lines() {
        let line = line.map_err(|error| {
            RuntimeProtocolIoError::Io(format!("could not read stdin: {error}"))
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_runtime_protocol_line(&mut runtime, &line);
        serde_json::to_writer(&mut writer, &response).map_err(|error| {
            RuntimeProtocolIoError::Encode(format!("could not encode runtime response: {error}"))
        })?;
        writer.write_all(b"\n").map_err(|error| {
            RuntimeProtocolIoError::Io(format!("could not write stdout: {error}"))
        })?;
        writer.flush().map_err(|error| {
            RuntimeProtocolIoError::Io(format!("could not flush stdout: {error}"))
        })?;
    }
    Ok(())
}

fn runtime_protocol_request_event(
    request: RuntimeProtocolRequest,
) -> Result<IbusRuntimeEvent, String> {
    match request {
        RuntimeProtocolRequest::Key {
            keyval,
            unicode,
            pressed,
            command_modifier_active,
        } => Ok(IbusRuntimeEvent::Key(IbusKeyEvent::new(
            keyval,
            runtime_protocol_char(unicode)?,
            pressed,
            command_modifier_active,
        ))),
        RuntimeProtocolRequest::FocusIn { purpose } => {
            Ok(IbusRuntimeEvent::FocusIn(purpose.into_content_purpose()))
        }
        RuntimeProtocolRequest::ContentPurposeChanged { purpose } => Ok(
            IbusRuntimeEvent::ContentPurposeChanged(purpose.into_content_purpose()),
        ),
        RuntimeProtocolRequest::Health => Ok(IbusRuntimeEvent::Health),
        RuntimeProtocolRequest::AcceptCandidate => Ok(IbusRuntimeEvent::AcceptCandidate),
        RuntimeProtocolRequest::FocusOut => Ok(IbusRuntimeEvent::FocusOut),
        RuntimeProtocolRequest::Reset => Ok(IbusRuntimeEvent::Reset),
        RuntimeProtocolRequest::TableChanged { shortcuts } => Ok(IbusRuntimeEvent::TableChanged(
            ShortcutTable::from_shortcuts(shortcuts),
        )),
    }
}

fn runtime_protocol_char(value: Option<String>) -> Result<Option<char>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let mut chars = value.chars();
    let Some(character) = chars.next() else {
        return Ok(None);
    };
    if chars.next().is_some() {
        return Err("runtime key unicode must be empty or a single scalar value".to_string());
    }
    Ok(Some(character))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeProtocolIoError {
    Io(String),
    Encode(String),
}

impl std::fmt::Display for RuntimeProtocolIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) | Self::Encode(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for RuntimeProtocolIoError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeProtocolSelfTestError {
    Io(RuntimeProtocolIoError),
    Utf8(String),
    Decode(String),
    UnexpectedResponseCount {
        expected: usize,
        actual: usize,
    },
    UnexpectedResponse {
        phase: &'static str,
        expected: Box<RuntimeProtocolResponse>,
        actual: Box<RuntimeProtocolResponse>,
    },
}

impl std::fmt::Display for RuntimeProtocolSelfTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Utf8(message) | Self::Decode(message) => write!(f, "{message}"),
            Self::UnexpectedResponseCount { expected, actual } => write!(
                f,
                "unexpected Text Shortcuts stdio response count: expected {expected}, got {actual}"
            ),
            Self::UnexpectedResponse {
                phase,
                expected,
                actual,
            } => write!(
                f,
                "unexpected Text Shortcuts stdio response during {phase}: expected {expected:?}, got {actual:?}"
            ),
        }
    }
}

impl std::error::Error for RuntimeProtocolSelfTestError {}

pub fn run_text_shortcuts_stdio_self_test() -> Result<(), RuntimeProtocolSelfTestError> {
    let requests = [
        r#"{"type":"table-changed","shortcuts":[{"replace":"omw","with":"on my way"}]}"#,
        r#"{"type":"key","keyval":111,"unicode":"o","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":109,"unicode":"m","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":119,"unicode":"w","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":65307,"unicode":null,"pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":111,"unicode":"o","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":109,"unicode":"m","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":119,"unicode":"w","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":32,"unicode":" ","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":111,"unicode":"o","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":109,"unicode":"m","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":119,"unicode":"w","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"accept-candidate"}"#,
        r#"{"type":"focus-in","purpose":9}"#,
        r#"{"type":"key","keyval":111,"unicode":"o","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":109,"unicode":"m","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":119,"unicode":"w","pressed":true,"command_modifier_active":false}"#,
        r#"{"type":"key","keyval":32,"unicode":" ","pressed":true,"command_modifier_active":false}"#,
    ]
    .join("\n");
    let input = format!("{requests}\n");
    let mut output = Vec::new();
    run_text_shortcuts_stdio_runtime(
        IbusTextShortcutsRuntime::default(),
        std::io::Cursor::new(input),
        &mut output,
    )
    .map_err(RuntimeProtocolSelfTestError::Io)?;

    let raw = String::from_utf8(output)
        .map_err(|error| RuntimeProtocolSelfTestError::Utf8(format!("{error}")))?;
    let responses = raw
        .lines()
        .map(|line| {
            serde_json::from_str::<RuntimeProtocolResponse>(line).map_err(|error| {
                RuntimeProtocolSelfTestError::Decode(format!(
                    "could not decode runtime response: {error}"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if responses.len() != 18 {
        return Err(RuntimeProtocolSelfTestError::UnexpectedResponseCount {
            expected: 18,
            actual: responses.len(),
        });
    }
    expect_protocol_response(
        "initial-table-change",
        &responses[0],
        RuntimeProtocolResponse {
            handled: false,
            operations: Vec::new(),
            candidate: None,
            error: None,
        },
    )?;
    expect_protocol_response(
        "candidate-metadata",
        &responses[3],
        RuntimeProtocolResponse {
            handled: false,
            operations: Vec::new(),
            candidate: Some(RuntimeProtocolCandidate {
                trigger: "omw".to_string(),
                replacement: "on my way".to_string(),
                accept_on: "word-boundary".to_string(),
                dismiss_key: "Escape".to_string(),
                rendered_bubble_ready_claim: false,
            }),
            error: None,
        },
    )?;
    expect_protocol_response(
        "escape-dismiss",
        &responses[4],
        RuntimeProtocolResponse {
            handled: true,
            operations: vec![RuntimeProtocolOperation::HidePreeditText],
            candidate: None,
            error: None,
        },
    )?;
    expect_protocol_response(
        "boundary-commit",
        &responses[8],
        RuntimeProtocolResponse {
            handled: true,
            operations: vec![
                RuntimeProtocolOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                    expected_text: "omw".to_string(),
                },
                RuntimeProtocolOperation::CommitText {
                    text: "on my way ".to_string(),
                },
                RuntimeProtocolOperation::HidePreeditText,
            ],
            candidate: None,
            error: None,
        },
    )?;
    expect_protocol_response(
        "pointer-accept",
        &responses[12],
        RuntimeProtocolResponse {
            handled: true,
            operations: vec![
                RuntimeProtocolOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                    expected_text: "omw".to_string(),
                },
                RuntimeProtocolOperation::CommitText {
                    text: "on my way".to_string(),
                },
                RuntimeProtocolOperation::HidePreeditText,
            ],
            candidate: None,
            error: None,
        },
    )?;
    expect_protocol_response(
        "pin-pass-through",
        &responses[17],
        RuntimeProtocolResponse {
            handled: false,
            operations: Vec::new(),
            candidate: None,
            error: None,
        },
    )
}

fn expect_protocol_response(
    phase: &'static str,
    actual: &RuntimeProtocolResponse,
    expected: RuntimeProtocolResponse,
) -> Result<(), RuntimeProtocolSelfTestError> {
    if *actual == expected {
        Ok(())
    } else {
        Err(RuntimeProtocolSelfTestError::UnexpectedResponse {
            phase,
            expected: Box::new(expected),
            actual: Box::new(actual.clone()),
        })
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
    Health,
    AcceptCandidate,
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
            IbusRuntimeEvent::Health => IbusRuntimeDecision::pass_through(),
            IbusRuntimeEvent::AcceptCandidate => self.accept_candidate(),
            IbusRuntimeEvent::FocusOut | IbusRuntimeEvent::Reset => self.clear_state(),
            IbusRuntimeEvent::TableChanged(table) => self.set_table(table),
        }
    }

    pub fn handle_key(&mut self, event: IbusKeyEvent) -> IbusRuntimeDecision {
        let forwarded_boundary = match event.keyval() {
            IBUS_KEY_RETURN => Some('\n'),
            IBUS_KEY_TAB => Some('\t'),
            _ => None,
        };
        let input = input_event_from_ibus_key(event);
        let action = self
            .state
            .handle_event(self.content_purpose, input, &self.table);
        if let Some(boundary) = forwarded_boundary {
            ibus_runtime_decision_forwarding_boundary(action, boundary)
        } else {
            ibus_runtime_decision(action)
        }
    }

    /// Accept the currently visible candidate from a non-keyboard IBus action,
    /// such as selecting the native lookup-table row with a pointer. This does
    /// not fabricate a Space or Return boundary, so only the replacement itself
    /// is committed. Missing and sensitive candidates remain fail-open.
    pub fn accept_candidate(&mut self) -> IbusRuntimeDecision {
        let action = self
            .state
            .accept_candidate(self.content_purpose, &self.table);
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
        "candidate-metadata",
        candidate,
        IbusRuntimeDecision::show_candidate("omw", "on my way"),
    )?;
    expect_keystroke_decision(
        "escape-dismiss",
        runtime.handle_event(IbusRuntimeEvent::Key(IbusKeyEvent::new(
            IBUS_KEY_ESCAPE,
            None,
            true,
            false,
        ))),
        IbusRuntimeDecision::handled(vec![IbusOperation::HidePreeditText]),
    )?;
    expect_keystroke_decision(
        "post-dismiss-boundary-pass-through",
        runtime.handle_event(IbusRuntimeEvent::Key(char_key_event_for_self_test(' '))),
        IbusRuntimeDecision::pass_through(),
    )?;
    let candidate = type_runtime_text_for_self_test(&mut runtime, "omw");
    expect_keystroke_decision(
        "candidate-metadata-after-dismiss",
        candidate,
        IbusRuntimeDecision::show_candidate("omw", "on my way"),
    )?;
    expect_keystroke_decision(
        "boundary-commit",
        runtime.handle_event(IbusRuntimeEvent::Key(char_key_event_for_self_test(' '))),
        IbusRuntimeDecision::handled(vec![
            IbusOperation::DeleteSurroundingText {
                offset: -3,
                n_chars: 3,
                expected_text: "omw".to_string(),
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
        IbusRuntimeDecision::show_candidate("omw", "on my way"),
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
            InputEvent::DismissCandidate => self.dismiss_candidate(),
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
            let trigger = self.current_word.clone();
            let text = format!("{replacement}{value}");
            self.current_word.clear();
            self.candidate_visible = false;
            EngineAction::CommitReplacement { trigger, text }
        } else {
            self.clear_candidate()
        }
    }

    fn accept_candidate(&mut self, purpose: ContentPurpose, table: &ShortcutTable) -> EngineAction {
        if !purpose.permits_replacement() {
            return self.clear_sensitive_state();
        }
        if !self.candidate_visible {
            self.current_word.clear();
            return EngineAction::PassThrough;
        }
        if let Some(replacement) = table.replacement_for(&self.current_word) {
            let trigger = self.current_word.clone();
            let text = replacement.to_string();
            self.current_word.clear();
            self.candidate_visible = false;
            EngineAction::CommitReplacement { trigger, text }
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

    fn dismiss_candidate(&mut self) -> EngineAction {
        let had_candidate = self.candidate_visible;
        self.current_word.clear();
        self.candidate_visible = false;
        if had_candidate {
            EngineAction::DismissCandidate
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
    use std::os::unix::fs::{symlink, PermissionsExt};

    use super::{
        content_purpose_from_ibus_input_purpose, content_purpose_from_ibus_input_purpose_name,
        default_text_shortcuts_table_path, effective_uid, handle_runtime_protocol_line,
        ibus_runtime_decision, input_event_from_ibus_key, private_regular_file_metadata,
        run_text_shortcuts_content_purpose_self_test, run_text_shortcuts_keystroke_self_test,
        run_text_shortcuts_stdio_self_test, run_text_shortcuts_table_watch_self_test,
        sanitize_shortcuts, ContentPurpose, EngineAction, EngineState, IbusKeyEvent, IbusOperation,
        IbusRuntimeDecision, IbusRuntimeEvent, IbusTextShortcutsRuntime, InputEvent, ShortcutTable,
        TableLoadStatus, TableWriteError, TextShortcut, TextShortcutTableStore,
        IBUS_INPUT_PURPOSE_FREE_FORM, IBUS_INPUT_PURPOSE_PASSWORD, IBUS_INPUT_PURPOSE_PIN,
        IBUS_KEY_BACKSPACE, IBUS_KEY_DELETE, IBUS_KEY_DOWN, IBUS_KEY_ESCAPE, IBUS_KEY_LEFT,
        IBUS_KEY_RETURN, IBUS_KEY_RIGHT, IBUS_KEY_TAB, IBUS_KEY_UP, MAX_TEXT_SHORTCUTS_TABLE_BYTES,
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

    fn write_private_test_table(path: &std::path::Path, raw: impl AsRef<[u8]>) {
        fs::write(path, raw).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn temp_store_path(slug: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "goblins-os-textshortcuts-store-{}-{slug}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("goblins-os").join("text-shortcuts.json");
        (root, path)
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
            TextShortcut::new("bad\0trigger", "value"),
            TextShortcut::new("bad", "value\0replacement"),
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
        write_private_test_table(
            &path,
            r#"
[
  {"replace":" omw ","with":" on my way "},
  {"replace":"same","with":"same"},
  {"replace":"omw","with":"on my way now"}
]
"#,
        );

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
        write_private_test_table(&invalid_path, "not-json");
        let invalid = TextShortcutTableStore::new(&invalid_path).load();
        assert_eq!(invalid.status(), &TableLoadStatus::InvalidJson);
        assert!(invalid.table().is_empty());
        fs::remove_file(invalid_path).unwrap();
    }

    #[test]
    fn table_store_rejects_oversize_permissive_and_wrong_owner_metadata() {
        let oversize_path = temp_table_path("oversize");
        write_private_test_table(
            &oversize_path,
            vec![b'x'; MAX_TEXT_SHORTCUTS_TABLE_BYTES + 1],
        );
        let oversize = TextShortcutTableStore::new(&oversize_path).load();
        assert_eq!(oversize.status(), &TableLoadStatus::TooLarge);
        assert!(oversize.table().is_empty());
        fs::remove_file(&oversize_path).unwrap();

        let permissive_path = temp_table_path("permissive");
        write_private_test_table(&permissive_path, b"[]");
        let private_metadata = fs::symlink_metadata(&permissive_path).unwrap();
        assert!(private_regular_file_metadata(
            &private_metadata,
            effective_uid()
        ));
        assert!(!private_regular_file_metadata(
            &private_metadata,
            effective_uid().wrapping_add(1)
        ));
        fs::set_permissions(&permissive_path, fs::Permissions::from_mode(0o644)).unwrap();
        let metadata = fs::symlink_metadata(&permissive_path).unwrap();
        assert!(!private_regular_file_metadata(&metadata, effective_uid()));
        let permissive = TextShortcutTableStore::new(&permissive_path).load();
        assert_eq!(permissive.status(), &TableLoadStatus::UnsafeFile);
        fs::remove_file(permissive_path).unwrap();
    }

    #[test]
    fn table_store_rejects_symlink_and_nonregular_destinations() {
        let (symlink_root, symlink_path) = temp_store_path("symlink");
        fs::create_dir_all(symlink_path.parent().unwrap()).unwrap();
        fs::set_permissions(
            symlink_path.parent().unwrap(),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let target = symlink_root.join("target.json");
        write_private_test_table(&target, b"[]");
        symlink(&target, &symlink_path).unwrap();
        let store = TextShortcutTableStore::new(&symlink_path);
        assert_eq!(store.load().status(), &TableLoadStatus::UnsafeFile);
        assert_eq!(
            store.save(vec![TextShortcut::new("omw", "on my way")]),
            Err(TableWriteError::UnsafeDestination)
        );
        fs::remove_dir_all(symlink_root).unwrap();

        let (directory_root, directory_path) = temp_store_path("directory");
        fs::create_dir_all(&directory_path).unwrap();
        fs::set_permissions(
            directory_path.parent().unwrap(),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let store = TextShortcutTableStore::new(&directory_path);
        assert_eq!(store.load().status(), &TableLoadStatus::UnsafeFile);
        assert_eq!(
            store.save(vec![TextShortcut::new("omw", "on my way")]),
            Err(TableWriteError::UnsafeDestination)
        );
        fs::remove_dir_all(directory_root).unwrap();
    }

    #[test]
    fn table_store_saves_atomically_with_private_modes_and_preserves_old_on_overflow() {
        let (root, path) = temp_store_path("atomic-private");
        let store = TextShortcutTableStore::new(&path);
        let saved = store
            .save(vec![TextShortcut::new("omw", "on my way")])
            .unwrap();
        assert_eq!(saved, vec![TextShortcut::new("omw", "on my way")]);
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let before = fs::read(&path).unwrap();

        assert_eq!(
            store.save(vec![TextShortcut::new(
                "large",
                "x".repeat(MAX_TEXT_SHORTCUTS_TABLE_BYTES)
            )]),
            Err(TableWriteError::TooLarge)
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(fs::read_dir(path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".text-shortcuts.json.tmp-")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn valid_component_xml_matches_the_registration_contract() {
        let xml = r#"
<component>
  <exec>/usr/libexec/goblins-os/goblins-textshortcuts-ibus</exec>
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
                trigger: "omw".to_string(),
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
                trigger: "teh".to_string(),
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
    fn escape_dismisses_visible_candidate_without_commit() {
        let table = table();
        let mut state = EngineState::default();
        assert!(matches!(
            type_chars(&mut state, "omw", &table),
            EngineAction::ShowCandidate { .. }
        ));
        assert_eq!(
            state.handle_event(ContentPurpose::Normal, InputEvent::DismissCandidate, &table),
            EngineAction::DismissCandidate
        );
        assert_eq!(state.current_word(), "");
        assert_eq!(
            state.handle_event(ContentPurpose::Normal, InputEvent::Boundary(' '), &table),
            EngineAction::PassThrough
        );
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
    fn ibus_content_purposes_decode_to_safe_runtime_purposes() {
        assert_eq!(
            content_purpose_from_ibus_input_purpose(IBUS_INPUT_PURPOSE_FREE_FORM),
            ContentPurpose::Normal
        );
        assert_eq!(
            content_purpose_from_ibus_input_purpose(IBUS_INPUT_PURPOSE_PASSWORD),
            ContentPurpose::Password
        );
        assert_eq!(
            content_purpose_from_ibus_input_purpose(IBUS_INPUT_PURPOSE_PIN),
            ContentPurpose::HiddenText
        );
        assert_eq!(
            content_purpose_from_ibus_input_purpose_name("IBUS_INPUT_PURPOSE_PASSWORD"),
            ContentPurpose::Password
        );
        assert_eq!(
            content_purpose_from_ibus_input_purpose_name("InputPurpose.PIN"),
            ContentPurpose::HiddenText
        );
        assert_eq!(
            content_purpose_from_ibus_input_purpose_name("unknown"),
            ContentPurpose::Normal
        );
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
    fn ibus_key_events_normalize_escape_as_candidate_dismiss() {
        assert_eq!(
            input_event_from_ibus_key(IbusKeyEvent::new(IBUS_KEY_ESCAPE, None, true, false)),
            InputEvent::DismissCandidate
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
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );
        assert!(decision.operations().is_empty());
    }

    #[test]
    fn ibus_adapter_commits_replacement_atomically_on_boundary() {
        let decision = ibus_runtime_decision(EngineAction::CommitReplacement {
            trigger: "omw".to_string(),
            text: "on my way ".to_string(),
        });
        assert_eq!(
            decision,
            IbusRuntimeDecision::handled(vec![
                IbusOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                    expected_text: "omw".to_string(),
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
    fn ibus_adapter_dismisses_candidate_by_handling_escape() {
        let decision = ibus_runtime_decision(EngineAction::DismissCandidate);
        assert_eq!(
            decision,
            IbusRuntimeDecision::handled(vec![IbusOperation::HidePreeditText])
        );
    }

    #[test]
    fn ibus_runtime_pipeline_shows_candidate_then_commits_replacement() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        let candidate = type_ibus_chars(&mut runtime, "omw");
        assert_eq!(
            candidate,
            IbusRuntimeDecision::show_candidate("omw", "on my way")
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
                    expected_text: "omw".to_string(),
                },
                IbusOperation::CommitText("on my way ".to_string()),
                IbusOperation::HidePreeditText,
            ])
        );
        assert_eq!(runtime.current_word(), "");
    }

    #[test]
    fn ibus_runtime_return_commits_replacement_without_swallowing_activation() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );

        let committed = runtime.handle_key(IbusKeyEvent::new(IBUS_KEY_RETURN, None, true, false));
        assert_eq!(
            committed,
            IbusRuntimeDecision::side_effects(vec![
                IbusOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                    expected_text: "omw".to_string(),
                },
                IbusOperation::CommitText("on my way".to_string()),
                IbusOperation::HidePreeditText,
            ])
        );
        assert!(!committed.key_handled());
        assert_eq!(runtime.current_word(), "");
    }

    #[test]
    fn ibus_runtime_tab_commits_replacement_without_swallowing_focus_traversal() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );

        let committed = runtime.handle_key(IbusKeyEvent::new(IBUS_KEY_TAB, None, true, false));
        assert_eq!(
            committed,
            IbusRuntimeDecision::side_effects(vec![
                IbusOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                    expected_text: "omw".to_string(),
                },
                IbusOperation::CommitText("on my way".to_string()),
                IbusOperation::HidePreeditText,
            ])
        );
        assert!(!committed.key_handled());
        assert_eq!(runtime.current_word(), "");
    }

    #[test]
    fn ibus_runtime_pointer_accept_commits_pending_candidate_without_boundary() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );

        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::AcceptCandidate),
            IbusRuntimeDecision::handled(vec![
                IbusOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                    expected_text: "omw".to_string(),
                },
                IbusOperation::CommitText("on my way".to_string()),
                IbusOperation::HidePreeditText,
            ])
        );
        assert_eq!(runtime.current_word(), "");
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::AcceptCandidate),
            IbusRuntimeDecision::pass_through()
        );
    }

    #[test]
    fn runtime_protocol_pointer_accept_is_typed_and_fail_open_without_candidate() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        for character in "omw".chars() {
            let request = format!(
                r#"{{"type":"key","keyval":{},"unicode":"{}","pressed":true,"command_modifier_active":false}}"#,
                character as u32, character
            );
            let _ = handle_runtime_protocol_line(&mut runtime, &request);
        }
        let accepted = handle_runtime_protocol_line(&mut runtime, r#"{"type":"accept-candidate"}"#);
        assert!(accepted.handled);
        assert_eq!(
            accepted.operations,
            vec![
                super::RuntimeProtocolOperation::DeleteSurroundingText {
                    offset: -3,
                    n_chars: 3,
                    expected_text: "omw".to_string(),
                },
                super::RuntimeProtocolOperation::CommitText {
                    text: "on my way".to_string(),
                },
                super::RuntimeProtocolOperation::HidePreeditText,
            ]
        );
        assert!(accepted.candidate.is_none());
        assert!(accepted.error.is_none());

        let absent = handle_runtime_protocol_line(&mut runtime, r#"{"type":"accept-candidate"}"#);
        assert!(!absent.handled);
        assert!(absent.operations.is_empty());
        assert!(absent.candidate.is_none());
        assert!(absent.error.is_none());
    }

    #[test]
    fn runtime_protocol_health_is_typed_and_does_not_mutate_edit_state() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        for character in "omw".chars() {
            runtime.handle_event(IbusRuntimeEvent::Key(ibus_char(character)));
        }
        assert_eq!(runtime.current_word(), "omw");

        let response = handle_runtime_protocol_line(&mut runtime, r#"{"type":"health"}"#);
        assert!(!response.handled);
        assert!(response.operations.is_empty());
        assert!(response.candidate.is_none());
        assert!(response.error.is_none());
        assert_eq!(runtime.current_word(), "omw");
    }

    #[test]
    fn ibus_runtime_pointer_accept_refuses_sensitive_fields_and_clears_state() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::FocusIn(ContentPurpose::Password)),
            IbusRuntimeDecision::pass_through()
        );
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::pass_through()
        );
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::AcceptCandidate),
            IbusRuntimeDecision::pass_through()
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
    fn ibus_runtime_pipeline_handles_escape_only_for_visible_candidate() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            runtime.handle_key(IbusKeyEvent::new(IBUS_KEY_ESCAPE, None, true, false)),
            IbusRuntimeDecision::pass_through()
        );
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );
        assert_eq!(
            runtime.handle_key(IbusKeyEvent::new(IBUS_KEY_ESCAPE, None, true, false)),
            IbusRuntimeDecision::handled(vec![IbusOperation::HidePreeditText])
        );
        assert_eq!(runtime.current_word(), "");
        assert_eq!(
            runtime.handle_key(ibus_char(' ')),
            IbusRuntimeDecision::pass_through()
        );
    }

    #[test]
    fn ibus_runtime_pipeline_clears_on_sensitive_content_purpose() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
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
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );

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
                    expected_text: "omw".to_string(),
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
            assert_eq!(
                type_ibus_chars(&mut runtime, "omw"),
                IbusRuntimeDecision::show_candidate("omw", "on my way")
            );
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
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );
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
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );
        assert_eq!(
            runtime.handle_event(IbusRuntimeEvent::TableChanged(
                ShortcutTable::from_shortcuts(vec![TextShortcut::new("brb", "be right back")])
            )),
            IbusRuntimeDecision::side_effects(vec![IbusOperation::HidePreeditText])
        );
        assert_eq!(runtime.current_word(), "");
        assert_eq!(
            type_ibus_chars(&mut runtime, "brb"),
            IbusRuntimeDecision::show_candidate("brb", "be right back")
        );
    }

    #[test]
    fn text_shortcuts_keystroke_self_test_covers_runtime_contract() {
        assert_eq!(run_text_shortcuts_keystroke_self_test(), Ok(()));
    }

    #[test]
    fn text_shortcuts_table_watch_self_test_covers_reload_contract() {
        assert_eq!(run_text_shortcuts_table_watch_self_test(), Ok(()));
    }

    #[test]
    fn text_shortcuts_content_purpose_self_test_covers_hidden_input_contract() {
        assert_eq!(run_text_shortcuts_content_purpose_self_test(), Ok(()));
    }

    #[test]
    fn text_shortcuts_stdio_self_test_covers_protocol_contract() {
        assert_eq!(run_text_shortcuts_stdio_self_test(), Ok(()));
    }

    #[test]
    fn runtime_protocol_rejects_multi_scalar_unicode_without_state_change() {
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        let response = handle_runtime_protocol_line(
            &mut runtime,
            r#"{"type":"key","keyval":111,"unicode":"om","pressed":true,"command_modifier_active":false}"#,
        );
        assert!(!response.handled);
        assert!(response.operations.is_empty());
        assert!(response
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("single scalar"));
        assert_eq!(runtime.current_word(), "");
    }

    #[test]
    fn runtime_refresh_reloads_table_and_hides_stale_candidate() {
        let path = temp_table_path("refresh");
        let mut runtime = IbusTextShortcutsRuntime::new(table());
        assert_eq!(
            type_ibus_chars(&mut runtime, "omw"),
            IbusRuntimeDecision::show_candidate("omw", "on my way")
        );

        write_private_test_table(&path, r#"[{"replace":"brb","with":"be right back"}]"#);
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
