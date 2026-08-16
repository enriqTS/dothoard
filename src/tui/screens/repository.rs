//! Repository selection screen state.
//!
//! Allows the user to browse the filesystem or input a repository path,
//! validate it against the backend (git structure, ownership), and confirm
//! initialization or attachment.

use std::path::{Path, PathBuf};

use crate::config::validate_namespace;

use crate::tui::browser::{Browser, BrowserConfig};
use crate::tui::{task::LoadState, text};

/// The interaction mode for repository selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoMode {
    /// Browser-based filesystem navigation (default).
    Browser,
    /// Text input for direct path entry.
    TextInput,
    /// Namespace management input.
    NamespaceInput,
    /// Discovered namespace list and lifecycle controls.
    Namespaces,
}

/// The state of the repository selection screen.
#[derive(Debug)]
pub struct RepoScreen {
    /// Current mode: browser or text input.
    pub mode: RepoMode,
    /// The text input buffer for the repository path.
    pub input: String,
    /// Current cursor position in the input.
    pub cursor: usize,
    /// The filesystem browser state (for Browser mode).
    pub browser: Option<Browser>,
    /// Lifecycle and last successful repository validation.
    pub validation: LoadState<RepoInfo>,
    /// Whether a confirmation dialog is active.
    pub confirm_state: ConfirmState,
    /// Error message from a failed selection attempt.
    pub selection_error: Option<String>,
    /// User-selected namespace used by repository setup and management.
    pub namespace_input: String,
    pub namespace_cursor: usize,
    pub namespace_action: NamespaceAction,
    pub namespace_origin: String,
    pub namespace_confirmation: Option<String>,
    /// Namespaces discovered in the selected repository.
    pub namespaces: Vec<NamespaceSummary>,
    /// Selected namespace in the visible namespace list.
    pub namespace_selected: usize,
}

/// A namespace discovered directly beneath a repository root.
#[derive(Debug, Clone)]
pub struct NamespaceSummary {
    pub name: String,
    pub ownership: OwnershipInfo,
    pub active: bool,
}

/// Information about a validated repository.
#[derive(Debug, Clone)]
pub struct RepoInfo {
    /// Absolute path to the repository.
    pub path: PathBuf,
    /// The current branch.
    pub branch: String,
    /// The ownership state description.
    pub ownership: OwnershipInfo,
}

/// Summary of ownership classification for display.
#[derive(Debug, Clone)]
pub enum OwnershipInfo {
    /// New namespace — can be initialized.
    New,
    /// Already owned — can be attached.
    Owned { sources: Vec<String> },
    /// Invalid manifest — cannot use.
    InvalidManifest(String),
    /// Ambiguous content — cannot use.
    Ambiguous(String),
}

impl OwnershipInfo {
    /// Whether the user needs to confirm before proceeding.
    pub fn needs_confirmation(&self) -> bool {
        matches!(self, Self::New | Self::Owned { .. })
    }

    /// Whether the state allows proceeding at all.
    pub fn can_proceed(&self) -> bool {
        matches!(self, Self::New | Self::Owned { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::New => "New",
            Self::Owned { .. } => "Owned",
            Self::InvalidManifest(_) => "Invalid",
            Self::Ambiguous(_) => "Ambiguous",
        }
    }
}

/// State of the confirmation dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmState {
    /// No dialog active.
    None,
    /// Asking "Initialize this repository?"
    AskInitialize,
    /// Asking "Attach to this repository?"
    AskAttach,
    /// The user confirmed and the operation succeeded.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceAction {
    None,
    SelectOrCreate,
    Rename,
    Delete,
}

impl Default for RepoScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl RepoScreen {
    /// Create a new repository screen, starting in browser mode.
    pub fn new() -> Self {
        Self {
            mode: RepoMode::Browser,
            input: String::new(),
            cursor: 0,
            browser: None,
            validation: LoadState::NotLoaded,
            confirm_state: ConfirmState::None,
            selection_error: None,
            namespace_input: String::new(),
            namespace_cursor: 0,
            namespace_action: NamespaceAction::None,
            namespace_origin: String::new(),
            namespace_confirmation: None,
            namespaces: Vec::new(),
            namespace_selected: 0,
        }
    }

    /// Create with a pre-filled path from the existing config.
    pub fn with_path(path: &str) -> Self {
        let cursor = path.len();
        Self {
            mode: RepoMode::Browser,
            input: path.to_string(),
            cursor,
            browser: None,
            validation: LoadState::NotLoaded,
            confirm_state: ConfirmState::None,
            selection_error: None,
            namespace_input: String::new(),
            namespace_cursor: 0,
            namespace_action: NamespaceAction::None,
            namespace_origin: String::new(),
            namespace_confirmation: None,
            namespaces: Vec::new(),
            namespace_selected: 0,
        }
    }

    pub fn set_namespace(&mut self, namespace: &str) {
        self.namespace_input = namespace.to_string();
        self.namespace_cursor = self.namespace_input.len();
        if let Some(index) = self
            .namespaces
            .iter()
            .position(|item| item.name == namespace)
        {
            self.namespace_selected = index;
        }
    }

    /// Discover direct namespace directories without claiming ownership of them.
    pub fn refresh_namespaces(&mut self, repository: &Path, active: &str) -> Result<(), String> {
        let mut namespaces = Vec::new();
        let entries = std::fs::read_dir(repository)
            .map_err(|error| format!("Cannot list repository namespaces: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("Cannot read repository entry: {error}"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if validate_namespace(&name).is_err() {
                continue;
            }
            let metadata = std::fs::symlink_metadata(entry.path())
                .map_err(|error| format!("Cannot inspect namespace {name:?}: {error}"))?;
            let ownership = if metadata.file_type().is_symlink() {
                OwnershipInfo::InvalidManifest(
                    "namespace directory is a symbolic link and cannot be used".to_string(),
                )
            } else if metadata.file_type().is_dir() {
                match crate::git::classify_ownership(repository, &name) {
                    Ok(state) => ownership_info(state),
                    Err(error) => OwnershipInfo::InvalidManifest(error.to_string()),
                }
            } else {
                continue;
            };
            namespaces.push(NamespaceSummary {
                active: name == active,
                name,
                ownership,
            });
        }
        if validate_namespace(active).is_ok() && !namespaces.iter().any(|item| item.name == active)
        {
            namespaces.push(NamespaceSummary {
                name: active.to_string(),
                ownership: OwnershipInfo::New,
                active: true,
            });
        }
        namespaces.sort_by(|left, right| left.name.cmp(&right.name));
        self.namespaces = namespaces;
        self.namespace_selected = self
            .namespaces
            .iter()
            .position(|item| item.active)
            .unwrap_or(0);
        Ok(())
    }

    pub fn selected_namespace(&self) -> Option<&NamespaceSummary> {
        self.namespaces.get(self.namespace_selected)
    }

    pub fn begin_namespace(&mut self, action: NamespaceAction, current: &str) {
        self.set_namespace(current);
        self.namespace_action = action;
        self.namespace_origin = current.to_string();
        self.namespace_confirmation = None;
        self.mode = RepoMode::NamespaceInput;
    }

    pub fn namespace_action_name(&self) -> &'static str {
        match self.namespace_action {
            NamespaceAction::SelectOrCreate => "select or create",
            NamespaceAction::Rename => "rename",
            NamespaceAction::Delete => "delete (type replacement)",
            NamespaceAction::None => "manage",
        }
    }

    /// Initialize the browser if not yet created. Uses `/` as root.
    pub fn ensure_browser(&mut self, home: &Path) {
        if self.browser.is_none() {
            let start = if !self.input.is_empty() {
                let expanded = expand_tilde(&self.input, home);
                if expanded.is_dir() {
                    expanded
                } else {
                    expanded
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| home.to_path_buf())
                }
            } else {
                home.to_path_buf()
            };

            self.browser = Some(Browser::new(BrowserConfig {
                root: PathBuf::from("/"),
                start,
            }));
        }
    }

    /// Handle a key event for this screen.
    ///
    /// Returns `true` if the event was consumed by this screen.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> KeyResult {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.namespace_confirmation.is_some() {
            return match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    KeyResult::NotConsumed
                }
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    self.namespace_confirmation = None;
                    KeyResult::Namespace
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    self.namespace_confirmation = None;
                    KeyResult::Consumed
                }
                _ => KeyResult::Consumed,
            };
        }

        // If a confirmation dialog is active, handle it.
        if self.confirm_state == ConfirmState::AskInitialize
            || self.confirm_state == ConfirmState::AskAttach
        {
            return match (key.modifiers, key.code) {
                // Tab/Shift+Tab escape to tab bar even from confirmation.
                (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    KeyResult::NotConsumed
                }
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => KeyResult::Confirm,
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    self.confirm_state = ConfirmState::None;
                    KeyResult::Consumed
                }
                _ => KeyResult::Consumed,
            };
        }

        match self.mode {
            RepoMode::Browser => self.handle_key_browser(key),
            RepoMode::TextInput => self.handle_key_text(key),
            RepoMode::NamespaceInput => self.handle_key_namespace(key),
            RepoMode::Namespaces => self.handle_key_namespaces(key),
        }
    }

    fn handle_key_namespaces(&mut self, key: crossterm::event::KeyEvent) -> KeyResult {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                KeyResult::NotConsumed
            }
            (_, KeyCode::Esc) => {
                self.mode = RepoMode::Browser;
                KeyResult::Consumed
            }
            (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                self.namespace_selected = self.namespace_selected.saturating_sub(1);
                KeyResult::Consumed
            }
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                self.namespace_selected = self
                    .namespace_selected
                    .saturating_add(1)
                    .min(self.namespaces.len().saturating_sub(1));
                KeyResult::Consumed
            }
            (_, KeyCode::Char('n')) => {
                self.begin_namespace(NamespaceAction::SelectOrCreate, "");
                KeyResult::Consumed
            }
            (_, KeyCode::Enter) => {
                if let Some(name) = self
                    .selected_namespace()
                    .filter(|item| item.ownership.can_proceed())
                    .map(|item| item.name.clone())
                {
                    self.begin_namespace(NamespaceAction::SelectOrCreate, &name);
                }
                KeyResult::Consumed
            }
            (_, KeyCode::Char('r')) => {
                if let Some(name) = self
                    .selected_namespace()
                    .filter(|item| item.active)
                    .map(|item| item.name.clone())
                {
                    self.begin_namespace(NamespaceAction::Rename, &name);
                }
                KeyResult::Consumed
            }
            (_, KeyCode::Char('d')) => {
                if let Some(name) = self
                    .selected_namespace()
                    .filter(|item| item.active)
                    .map(|item| item.name.clone())
                {
                    self.begin_namespace(NamespaceAction::Delete, &name);
                }
                KeyResult::Consumed
            }
            _ => KeyResult::Consumed,
        }
    }

    fn handle_key_namespace(&mut self, key: crossterm::event::KeyEvent) -> KeyResult {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                KeyResult::NotConsumed
            }
            (_, KeyCode::Esc) => {
                self.mode = RepoMode::Browser;
                self.namespace_action = NamespaceAction::None;
                KeyResult::Consumed
            }
            (_, KeyCode::Enter) => {
                self.namespace_confirmation = Some(match self.namespace_action {
                    NamespaceAction::Rename => {
                        format!("Rename namespace to '{}' ? y/n", self.namespace_input)
                    }
                    NamespaceAction::Delete => {
                        format!(
                            "Delete '{}' home/manifest (replacement required)? y/n",
                            self.namespace_origin
                        )
                    }
                    _ => format!(
                        "Use namespace '{}' (create if needed)? y/n",
                        self.namespace_input
                    ),
                });
                KeyResult::Consumed
            }
            (_, KeyCode::Backspace) => {
                text::backspace(&mut self.namespace_input, &mut self.namespace_cursor);
                KeyResult::Consumed
            }
            (_, KeyCode::Delete) => {
                text::delete(&mut self.namespace_input, &mut self.namespace_cursor);
                KeyResult::Consumed
            }
            (_, KeyCode::Left) => {
                text::move_left(&self.namespace_input, &mut self.namespace_cursor);
                KeyResult::Consumed
            }
            (_, KeyCode::Right) => {
                text::move_right(&self.namespace_input, &mut self.namespace_cursor);
                KeyResult::Consumed
            }
            (_, KeyCode::Home) => {
                self.namespace_cursor = 0;
                KeyResult::Consumed
            }
            (_, KeyCode::End) => {
                self.namespace_cursor = self.namespace_input.len();
                KeyResult::Consumed
            }
            (_, KeyCode::Char(c)) => {
                text::insert_char(&mut self.namespace_input, &mut self.namespace_cursor, c);
                KeyResult::Consumed
            }
            _ => KeyResult::Consumed,
        }
    }

    /// Handle key events in browser mode.
    fn handle_key_browser(&mut self, key: crossterm::event::KeyEvent) -> KeyResult {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.modifiers, key.code) {
            // Tab/Shift+Tab escape to tab bar.
            (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                KeyResult::NotConsumed
            }
            // Switch to text input mode.
            (_, KeyCode::Char(':')) | (_, KeyCode::Char('/')) => {
                self.mode = RepoMode::TextInput;
                KeyResult::Consumed
            }
            // Space selects the current directory for validation.
            (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                if let Some(ref mut browser) = self.browser {
                    match browser.try_select() {
                        Ok(selection) => {
                            use crate::tui::browser::EntryKind;
                            if selection.kind == EntryKind::Directory {
                                // Set the input to the selected path for validation.
                                self.input = selection.path.to_string_lossy().to_string();
                                self.cursor = self.input.len();
                                self.selection_error = None;
                                return KeyResult::Validate;
                            } else {
                                self.selection_error = Some(
                                    "Only directories can be used as repositories.".to_string(),
                                );
                            }
                        }
                        Err(e) => {
                            self.selection_error = Some(e.to_string());
                        }
                    }
                }
                KeyResult::Consumed
            }
            // Delegate all other keys to the picker.
            _ => {
                if let Some(ref mut browser) = self.browser {
                    use crate::tui::picker::{PickerAction, handle_key};
                    let action = handle_key(browser, key, 20);
                    match action {
                        PickerAction::Consumed => KeyResult::Consumed,
                        PickerAction::Select(Ok(selection)) => {
                            use crate::tui::browser::EntryKind;
                            if selection.kind == EntryKind::Directory {
                                self.input = selection.path.to_string_lossy().to_string();
                                self.cursor = self.input.len();
                                self.selection_error = None;
                                KeyResult::Validate
                            } else {
                                self.selection_error = Some(
                                    "Only directories can be used as repositories.".to_string(),
                                );
                                KeyResult::Consumed
                            }
                        }
                        PickerAction::Select(Err(e)) => {
                            self.selection_error = Some(e.to_string());
                            KeyResult::Consumed
                        }
                        PickerAction::Cancel => {
                            // Esc in browser mode → pass to parent (quit/escape).
                            KeyResult::NotConsumed
                        }
                        PickerAction::NotConsumed => KeyResult::NotConsumed,
                    }
                } else {
                    KeyResult::NotConsumed
                }
            }
        }
    }

    /// Handle key events in text input mode.
    fn handle_key_text(&mut self, key: crossterm::event::KeyEvent) -> KeyResult {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.modifiers, key.code) {
            // Tab/Shift+Tab escape to tab bar even from text input.
            (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                KeyResult::NotConsumed
            }
            // Escape returns to browser mode.
            (_, KeyCode::Esc) => {
                self.mode = RepoMode::Browser;
                KeyResult::Consumed
            }
            // Submit path for validation.
            (_, KeyCode::Enter) => KeyResult::Validate,

            // Ctrl+key shortcuts (must be before the generic Char catch-all).
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.cursor = 0;
                KeyResult::Consumed
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.cursor = self.input.len();
                KeyResult::Consumed
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.input.clear();
                self.cursor = 0;
                self.validation.reset();
                KeyResult::Consumed
            }

            // Text editing.
            (_, KeyCode::Char(c)) => {
                text::insert_char(&mut self.input, &mut self.cursor, c);
                self.validation.reset();
                KeyResult::Consumed
            }
            (_, KeyCode::Backspace) => {
                if text::backspace(&mut self.input, &mut self.cursor) {
                    self.validation.reset();
                }
                KeyResult::Consumed
            }
            (_, KeyCode::Delete) => {
                if text::delete(&mut self.input, &mut self.cursor) {
                    self.validation.reset();
                }
                KeyResult::Consumed
            }
            (_, KeyCode::Left) => {
                text::move_left(&self.input, &mut self.cursor);
                KeyResult::Consumed
            }
            (_, KeyCode::Right) => {
                text::move_right(&self.input, &mut self.cursor);
                KeyResult::Consumed
            }
            (_, KeyCode::Home) => {
                self.cursor = 0;
                KeyResult::Consumed
            }
            (_, KeyCode::End) => {
                self.cursor = self.input.len();
                KeyResult::Consumed
            }

            _ => KeyResult::NotConsumed,
        }
    }

    /// Validate a path snapshot on a background worker.
    pub fn validate_path(
        input: &str,
        home: &Path,
        namespace: &str,
        remote: &str,
        timeout_seconds: u32,
    ) -> Result<RepoInfo, String> {
        let expanded = expand_tilde(input, home);

        if expanded.as_os_str().is_empty() {
            return Err("Path cannot be empty".to_string());
        }
        if !expanded.is_absolute() {
            return Err("Path must be absolute or start with ~/".to_string());
        }
        if !expanded.exists() {
            return Err(format!("Directory does not exist: {}", expanded.display()));
        }

        use crate::git::{GitRunner, classify_ownership, validate_repository};
        let runner = GitRunner::new(std::time::Duration::from_secs(u64::from(timeout_seconds)));
        let info = validate_repository(&runner, &expanded, remote)
            .map_err(|e| format!("Not a valid repository: {e}"))?;
        let ownership = if namespace.is_empty() {
            // First-run repository validation intentionally precedes namespace
            // selection so the repository's owned namespaces can be listed.
            OwnershipInfo::New
        } else {
            let state = classify_ownership(&expanded, namespace)
                .map_err(|e| format!("Failed to classify ownership: {e}"))?;
            ownership_info(state)
        };

        Ok(RepoInfo {
            path: expanded,
            branch: info.branch,
            ownership,
        })
    }

    /// Attempt to confirm the current validated repository.
    ///
    /// Returns the path to save in config if confirmed and initialized/attached
    /// successfully, or an error message.
    pub fn confirm(&mut self, _home: &Path, namespace: &str) -> Result<PathBuf, String> {
        let info = match &self.validation {
            LoadState::Loaded(info) => info.clone(),
            _ => return Err("No current repository validation to confirm".to_string()),
        };

        use crate::git::{classify_ownership, initialize_or_attach};

        let state = classify_ownership(&info.path, namespace).map_err(|e| e.to_string())?;
        initialize_or_attach(&info.path, namespace, &state, true).map_err(|e| e.to_string())?;

        self.confirm_state = ConfirmState::Done;
        Ok(info.path)
    }
}

fn ownership_info(state: crate::git::OwnershipState) -> OwnershipInfo {
    match state {
        crate::git::OwnershipState::New => OwnershipInfo::New,
        crate::git::OwnershipState::Owned { manifest } => OwnershipInfo::Owned {
            sources: manifest
                .sources
                .into_iter()
                .map(|source| source.path)
                .collect(),
        },
        crate::git::OwnershipState::InvalidManifest { reason } => {
            OwnershipInfo::InvalidManifest(reason)
        }
        crate::git::OwnershipState::Ambiguous { reason } => OwnershipInfo::Ambiguous(reason),
    }
}

/// The result of handling a key in the repo screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    /// The key was consumed (input edited, dialog handled).
    Consumed,
    /// The user wants to validate the current path.
    Validate,
    /// The user submitted a namespace operation.
    Namespace,
    /// The user confirmed in the dialog.
    Confirm,
    /// The key was not consumed (pass to parent handler).
    NotConsumed,
}

/// Expand a leading `~` or `~/` to the home directory.
fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        home.to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(path)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/tui/screens/repository.rs"]
mod tests;
