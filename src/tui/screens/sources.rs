//! Source management screen state.
//!
//! Displays the list of configured sources, allows adding new sources via
//! a filesystem browser or path entry, removing selected sources, and shows
//! validation warnings (overlap, symlinks).

use std::path::Path;

use crate::config::SourceConfig;
use crate::paths;
use crate::tui::browser::{Browser, BrowserConfig, EntryKind};
use crate::tui::selection::{SelectionDiff, SourceSelection};
use crate::tui::text;

/// The state of the sources management screen.
#[derive(Debug)]
pub struct SourcesScreen {
    /// Currently selected index in the source list.
    pub selected: usize,
    /// Current mode of the screen.
    pub mode: Mode,
    /// Text input buffer for adding a new source.
    pub input: String,
    /// Cursor position in the input.
    pub cursor: usize,
    /// Validation/feedback message.
    pub message: Option<Message>,
    /// The filesystem browser for source selection (rooted at $HOME).
    pub browser: Option<Browser>,
    /// Multi-selection state for the browser (persists within session).
    pub selection: Option<SourceSelection>,
    /// Cached diff for the confirm-apply dialog.
    pub pending_diff: Option<SelectionDiff>,
}

/// The mode the sources screen is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing the source list.
    List,
    /// Browsing the filesystem to select a source.
    Browse,
    /// Typing a new source path.
    AddInput,
    /// Confirming deletion of the selected source.
    ConfirmDelete,
    /// Choosing whether to apply, discard, or continue editing pending changes.
    PendingChanges,
    /// Confirming source removals before applying multi-selection changes.
    ConfirmApply,
}

/// A feedback message to display.
#[derive(Debug, Clone)]
pub struct Message {
    pub text: String,
    pub kind: MessageKind,
}

/// Kind of feedback message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Warning,
    Error,
}

impl Default for SourcesScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl SourcesScreen {
    pub fn new() -> Self {
        Self {
            selected: 0,
            mode: Mode::List,
            input: String::new(),
            cursor: 0,
            message: None,
            browser: None,
            selection: None,
            pending_diff: None,
        }
    }

    /// Ensure the browser is initialized for source selection.
    pub fn ensure_browser(&mut self, home: &Path) {
        if self.browser.is_none() {
            self.browser = Some(Browser::new(BrowserConfig {
                root: home.to_path_buf(),
                start: home.to_path_buf(),
            }));
        }
    }

    /// Ensure the selection state is initialized from config.
    ///
    /// Only initializes once per session — re-entering Browse mode reuses it.
    pub fn ensure_selection(&mut self, sources: &[SourceConfig], home: &Path) {
        if self.selection.is_none() {
            let mut sel = SourceSelection::new(home);
            sel.load_from_config(sources);
            self.selection = Some(sel);
        }
    }

    /// Handle a key event for this screen.
    ///
    /// Returns the action to perform (if any).
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent, source_count: usize) -> Action {
        use crossterm::event::{KeyCode, KeyModifiers};

        match self.mode {
            Mode::List => match (key.modifiers, key.code) {
                // Navigation in the list.
                (_, KeyCode::Up | KeyCode::Char('k')) => {
                    if self.selected > 0 {
                        self.selected -= 1;
                        self.message = None;
                        Action::Consumed
                    } else {
                        // At upper boundary — let parent handle focus return.
                        Action::NotConsumed
                    }
                }
                (_, KeyCode::Down | KeyCode::Char('j')) => {
                    if source_count > 0 && self.selected < source_count - 1 {
                        self.selected += 1;
                    }
                    self.message = None;
                    Action::Consumed
                }

                // Add a new source (opens browser).
                (_, KeyCode::Char('a')) => {
                    self.mode = Mode::Browse;
                    self.message = None;
                    Action::Consumed
                }

                // Delete the selected source.
                (_, KeyCode::Char('d') | KeyCode::Delete) if source_count > 0 => {
                    self.mode = Mode::ConfirmDelete;
                    self.message = None;
                    Action::Consumed
                }

                _ => Action::NotConsumed,
            },

            Mode::Browse => self.handle_key_browse(key),

            Mode::AddInput => match (key.modifiers, key.code) {
                // Tab/Shift+Tab escape to tab bar even from input mode.
                (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    Action::NotConsumed
                }

                // Submit the new source path.
                (_, KeyCode::Enter) => {
                    let path = self.input.trim().to_string();
                    if path.is_empty() {
                        self.message = Some(Message {
                            text: "Path cannot be empty".to_string(),
                            kind: MessageKind::Error,
                        });
                        Action::Consumed
                    } else {
                        Action::AddSource(path)
                    }
                }

                // Cancel manual entry and return to the source browser.
                (_, KeyCode::Esc) => {
                    self.mode = Mode::Browse;
                    self.message = None;
                    Action::Consumed
                }

                // Ctrl+key shortcuts before generic Char catch-all.
                (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                    self.cursor = 0;
                    Action::Consumed
                }
                (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                    self.cursor = self.input.len();
                    Action::Consumed
                }
                (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                    self.input.clear();
                    self.cursor = 0;
                    Action::Consumed
                }

                // Text editing.
                (_, KeyCode::Char(c)) => {
                    text::insert_char(&mut self.input, &mut self.cursor, c);
                    Action::Consumed
                }
                (_, KeyCode::Backspace) => {
                    text::backspace(&mut self.input, &mut self.cursor);
                    Action::Consumed
                }
                (_, KeyCode::Delete) => {
                    text::delete(&mut self.input, &mut self.cursor);
                    Action::Consumed
                }
                (_, KeyCode::Left) => {
                    text::move_left(&self.input, &mut self.cursor);
                    Action::Consumed
                }
                (_, KeyCode::Right) => {
                    text::move_right(&self.input, &mut self.cursor);
                    Action::Consumed
                }
                (_, KeyCode::Home) => {
                    self.cursor = 0;
                    Action::Consumed
                }
                (_, KeyCode::End) => {
                    self.cursor = self.input.len();
                    Action::Consumed
                }

                _ => Action::Consumed, // Swallow unknown keys in input mode.
            },

            Mode::ConfirmDelete => match (key.modifiers, key.code) {
                // Tab/Shift+Tab escape to tab bar even from confirmation.
                (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    Action::NotConsumed
                }
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    Action::RemoveSource(self.selected)
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    self.mode = Mode::List;
                    self.message = None;
                    Action::Consumed
                }
                _ => Action::Consumed,
            },

            Mode::PendingChanges => match (key.modifiers, key.code) {
                // Tab/Shift+Tab escape to the tab bar without resolving changes.
                (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    Action::NotConsumed
                }
                (_, KeyCode::Char('a')) | (_, KeyCode::Char('A')) => Action::ChooseApply,
                (_, KeyCode::Char('d')) | (_, KeyCode::Char('D')) => Action::DiscardSelection,
                (_, KeyCode::Char('c')) | (_, KeyCode::Char('C')) | (_, KeyCode::Esc) => {
                    self.mode = Mode::Browse;
                    self.pending_diff = None;
                    self.message = None;
                    Action::Consumed
                }
                _ => Action::Consumed,
            },

            Mode::ConfirmApply => match (key.modifiers, key.code) {
                // Tab/Shift+Tab escape to tab bar.
                (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    Action::NotConsumed
                }
                (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                    // Confirm: apply the pending diff.
                    Action::ConfirmApply
                }
                (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) | (_, KeyCode::Esc) => {
                    // Cancel only the removal confirmation; the explicit
                    // apply/discard/continue choice remains available.
                    self.mode = Mode::PendingChanges;
                    self.message = None;
                    Action::Consumed
                }
                _ => Action::Consumed,
            },
        }
    }

    /// Handle key events in browser mode.
    fn handle_key_browse(&mut self, key: crossterm::event::KeyEvent) -> Action {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.modifiers, key.code) {
            // Tab/Shift+Tab escape to tab bar.
            (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                Action::NotConsumed
            }
            // Escape leaves the editing level and reviews pending changes.
            (_, KeyCode::Esc) => Action::ApplySelection,
            // ':' or '/' switches to text input mode for manual path entry.
            (_, KeyCode::Char(':')) | (_, KeyCode::Char('/')) => {
                self.mode = Mode::AddInput;
                self.input.clear();
                self.cursor = 0;
                self.message = None;
                Action::Consumed
            }
            // Space toggles the current entry's selection state.
            (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                if let Some(ref mut browser) = self.browser {
                    match browser.try_select() {
                        Ok(selection) => {
                            match selection.kind {
                                EntryKind::File | EntryKind::Directory | EntryKind::Symlink => {
                                    // Toggle the selection state.
                                    if let Some(ref mut sel) = self.selection {
                                        let is_dir = selection.kind == EntryKind::Directory;
                                        sel.toggle(&selection.path, is_dir);
                                    }
                                    self.message = None;
                                }
                                _ => {
                                    self.message = Some(Message {
                                        text: "Cannot select this file type.".to_string(),
                                        kind: MessageKind::Error,
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            self.message = Some(Message {
                                text: e.to_string(),
                                kind: MessageKind::Error,
                            });
                        }
                    }
                }
                Action::Consumed
            }
            // Delegate all other keys to the picker.
            _ => {
                if let Some(ref mut browser) = self.browser {
                    use crate::tui::picker::{PickerAction, handle_key};
                    let action = handle_key(browser, key, 20);
                    match action {
                        PickerAction::Consumed => Action::Consumed,
                        PickerAction::Select(_) => {
                            // In multi-select mode, picker's Space-based select
                            // is handled above. This branch handles the unlikely
                            // case of a key binding collision — just consume it.
                            Action::Consumed
                        }
                        PickerAction::Cancel => Action::ApplySelection,
                        PickerAction::NotConsumed => Action::NotConsumed,
                    }
                } else {
                    Action::NotConsumed
                }
            }
        }
    }

    /// Validate a source path before adding it.
    ///
    /// Returns `Ok(normalized_path)` or an error message.
    pub fn validate_source(
        path: &str,
        existing_sources: &[SourceConfig],
        home: &Path,
        repository: Option<&Path>,
    ) -> Result<SourceInfo, String> {
        // Basic validation.
        if path.is_empty() {
            return Err("Path cannot be empty".to_string());
        }
        if path.starts_with('/') {
            return Err("Source paths must be relative to $HOME".to_string());
        }
        if path.contains("..") {
            return Err("Parent traversal (..) is not allowed".to_string());
        }

        // Check for duplicate.
        let normalized = path.trim_end_matches('/');
        for src in existing_sources {
            let existing = src.path.trim_end_matches('/');
            if existing == normalized {
                return Err(format!("Source '{}' is already configured", normalized));
            }
        }

        // Check for overlap with existing sources.
        let source_abs = home.join(normalized);
        let mut all_paths: Vec<std::path::PathBuf> = existing_sources
            .iter()
            .map(|s| home.join(s.path.trim_end_matches('/')))
            .collect();
        all_paths.push(source_abs.clone());

        // Check against repository path.
        if let Some(repo) = repository {
            let overlaps = paths::check_overlaps(&all_paths, repo);
            if !overlaps.is_empty() {
                return Err(format!("Overlap detected: {}", overlaps[0]));
            }
        }

        // Check for symlinked parents.
        let is_symlink = source_abs.is_symlink();
        let symlink_warning = if is_symlink {
            Some("Source is a symlink — it will be backed up as a link, not followed.".to_string())
        } else {
            None
        };

        // Validate source path against filesystem.
        match paths::validate_source_path(home, normalized) {
            Ok(_) => {}
            Err(e) => return Err(format!("Invalid source: {e}")),
        }

        Ok(SourceInfo {
            path: normalized.to_string(),
            exists: source_abs.exists(),
            is_symlink,
            warning: symlink_warning,
        })
    }
}

/// Actions resulting from key handling on the sources screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The event was consumed, no further action needed.
    Consumed,
    /// The event was not consumed (pass to parent handler).
    NotConsumed,
    /// Add a new source with this path (from text input mode).
    AddSource(String),
    /// Remove the source at this index.
    RemoveSource(usize),
    /// Review the multi-selection diff when leaving the browser.
    ApplySelection,
    /// Choose apply from the pending-changes prompt.
    ChooseApply,
    /// Discard the pending multi-selection edits.
    DiscardSelection,
    /// Confirm and execute a pending diff that removes sources.
    ConfirmApply,
}

/// Information about a validated source path.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    /// The normalized home-relative path.
    pub path: String,
    /// Whether the source currently exists on disk.
    pub exists: bool,
    /// Whether the source root is a symlink.
    pub is_symlink: bool,
    /// Warning about symlink behavior.
    pub warning: Option<String>,
}

#[cfg(test)]
#[path = "../../../tests/unit/tui/screens/sources.rs"]
mod tests;
