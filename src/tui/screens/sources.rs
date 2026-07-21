//! Source management screen state.
//!
//! Displays the list of configured sources, allows adding new sources by path,
//! removing selected sources, and shows validation warnings (overlap, symlinks).

use std::path::Path;

use crate::config::SourceConfig;
use crate::paths;

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
}

/// The mode the sources screen is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing the source list.
    List,
    /// Typing a new source path.
    AddInput,
    /// Confirming deletion of the selected source.
    ConfirmDelete,
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
                    }
                    self.message = None;
                    Action::Consumed
                }
                (_, KeyCode::Down | KeyCode::Char('j')) => {
                    if source_count > 0 && self.selected < source_count - 1 {
                        self.selected += 1;
                    }
                    self.message = None;
                    Action::Consumed
                }

                // Add a new source.
                (_, KeyCode::Char('a')) => {
                    self.mode = Mode::AddInput;
                    self.input.clear();
                    self.cursor = 0;
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

            Mode::AddInput => match (key.modifiers, key.code) {
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

                // Cancel adding.
                (_, KeyCode::Esc) => {
                    self.mode = Mode::List;
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
                    self.input.insert(self.cursor, c);
                    self.cursor += c.len_utf8();
                    Action::Consumed
                }
                (_, KeyCode::Backspace) => {
                    if self.cursor > 0 {
                        self.cursor -= 1;
                        self.input.remove(self.cursor);
                    }
                    Action::Consumed
                }
                (_, KeyCode::Delete) => {
                    if self.cursor < self.input.len() {
                        self.input.remove(self.cursor);
                    }
                    Action::Consumed
                }
                (_, KeyCode::Left) => {
                    if self.cursor > 0 {
                        self.cursor -= 1;
                    }
                    Action::Consumed
                }
                (_, KeyCode::Right) => {
                    if self.cursor < self.input.len() {
                        self.cursor += 1;
                    }
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

            Mode::ConfirmDelete => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => Action::RemoveSource(self.selected),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.mode = Mode::List;
                    self.message = None;
                    Action::Consumed
                }
                _ => Action::Consumed,
            },
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
    /// Add a new source with this path.
    AddSource(String),
    /// Remove the source at this index.
    RemoveSource(usize),
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
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn new_screen_starts_in_list_mode() {
        let screen = SourcesScreen::new();
        assert_eq!(screen.mode, Mode::List);
        assert_eq!(screen.selected, 0);
    }

    #[test]
    fn up_down_navigates_list() {
        let mut screen = SourcesScreen::new();
        screen.handle_key(key(KeyCode::Down), 3);
        assert_eq!(screen.selected, 1);
        screen.handle_key(key(KeyCode::Down), 3);
        assert_eq!(screen.selected, 2);
        // Should not go past the end.
        screen.handle_key(key(KeyCode::Down), 3);
        assert_eq!(screen.selected, 2);
        screen.handle_key(key(KeyCode::Up), 3);
        assert_eq!(screen.selected, 1);
    }

    #[test]
    fn up_does_not_go_negative() {
        let mut screen = SourcesScreen::new();
        screen.handle_key(key(KeyCode::Up), 3);
        assert_eq!(screen.selected, 0);
    }

    #[test]
    fn a_enters_add_mode() {
        let mut screen = SourcesScreen::new();
        let action = screen.handle_key(key(KeyCode::Char('a')), 0);
        assert_eq!(action, Action::Consumed);
        assert_eq!(screen.mode, Mode::AddInput);
    }

    #[test]
    fn typing_in_add_mode() {
        let mut screen = SourcesScreen::new();
        screen.mode = Mode::AddInput;
        screen.handle_key(key(KeyCode::Char('.')), 0);
        screen.handle_key(key(KeyCode::Char('c')), 0);
        screen.handle_key(key(KeyCode::Char('o')), 0);
        assert_eq!(screen.input, ".co");
    }

    #[test]
    fn enter_in_add_mode_returns_add_action() {
        let mut screen = SourcesScreen::new();
        screen.mode = Mode::AddInput;
        screen.input = ".config/fish".to_string();
        screen.cursor = screen.input.len();
        let action = screen.handle_key(key(KeyCode::Enter), 0);
        assert_eq!(action, Action::AddSource(".config/fish".to_string()));
    }

    #[test]
    fn esc_in_add_mode_returns_to_list() {
        let mut screen = SourcesScreen::new();
        screen.mode = Mode::AddInput;
        screen.input = "partial".to_string();
        screen.handle_key(key(KeyCode::Esc), 0);
        assert_eq!(screen.mode, Mode::List);
    }

    #[test]
    fn d_enters_confirm_delete() {
        let mut screen = SourcesScreen::new();
        let action = screen.handle_key(key(KeyCode::Char('d')), 2);
        assert_eq!(action, Action::Consumed);
        assert_eq!(screen.mode, Mode::ConfirmDelete);
    }

    #[test]
    fn d_does_nothing_when_empty() {
        let mut screen = SourcesScreen::new();
        let action = screen.handle_key(key(KeyCode::Char('d')), 0);
        // Not consumed because the guard `source_count > 0` fails.
        assert_eq!(action, Action::NotConsumed);
        assert_eq!(screen.mode, Mode::List);
    }

    #[test]
    fn confirm_delete_y_removes() {
        let mut screen = SourcesScreen::new();
        screen.selected = 1;
        screen.mode = Mode::ConfirmDelete;
        let action = screen.handle_key(key(KeyCode::Char('y')), 3);
        assert_eq!(action, Action::RemoveSource(1));
    }

    #[test]
    fn confirm_delete_n_cancels() {
        let mut screen = SourcesScreen::new();
        screen.mode = Mode::ConfirmDelete;
        screen.handle_key(key(KeyCode::Char('n')), 3);
        assert_eq!(screen.mode, Mode::List);
    }

    #[test]
    fn validate_rejects_absolute_path() {
        let result = SourcesScreen::validate_source("/etc/foo", &[], Path::new("/home/user"), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("relative"));
    }

    #[test]
    fn validate_rejects_parent_traversal() {
        let result =
            SourcesScreen::validate_source("../outside", &[], Path::new("/home/user"), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("traversal"));
    }

    #[test]
    fn validate_rejects_duplicate() {
        let existing = vec![SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];
        let result = SourcesScreen::validate_source(
            ".config/fish",
            &existing,
            Path::new("/home/user"),
            None,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already configured"));
    }

    #[test]
    fn validate_accepts_new_valid_source() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let source_dir = home.join(".config/test");
        std::fs::create_dir_all(&source_dir).unwrap();

        let result = SourcesScreen::validate_source(".config/test", &[], home, None);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.path, ".config/test");
        assert!(info.exists);
        assert!(!info.is_symlink);
    }

    #[test]
    fn validate_detects_symlink_source() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let target = home.join("real-dir");
        std::fs::create_dir_all(&target).unwrap();
        let link = home.join("link-dir");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = SourcesScreen::validate_source("link-dir", &[], home, None);
        assert!(result.is_ok());
        let info = result.unwrap();
        assert!(info.is_symlink);
        assert!(info.warning.is_some());
    }

    #[test]
    fn validate_rejects_empty() {
        let result = SourcesScreen::validate_source("", &[], Path::new("/home/user"), None);
        assert!(result.is_err());
    }

    #[test]
    fn j_k_navigate_like_vim() {
        let mut screen = SourcesScreen::new();
        screen.handle_key(key(KeyCode::Char('j')), 5);
        assert_eq!(screen.selected, 1);
        screen.handle_key(key(KeyCode::Char('k')), 5);
        assert_eq!(screen.selected, 0);
    }
}
