//! Repository selection screen state.
//!
//! Allows the user to browse the filesystem or input a repository path,
//! validate it against the backend (git structure, ownership), and confirm
//! initialization or attachment.

use std::path::{Path, PathBuf};

use crate::tui::browser::{Browser, BrowserConfig};

/// The interaction mode for repository selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoMode {
    /// Browser-based filesystem navigation (default).
    Browser,
    /// Text input for direct path entry.
    TextInput,
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
    /// Validation result after the user selects/enters a path.
    pub validation: Option<ValidationResult>,
    /// Whether a confirmation dialog is active.
    pub confirm_state: ConfirmState,
    /// Error message from a failed selection attempt.
    pub selection_error: Option<String>,
}

/// Result of validating the repository path.
#[derive(Debug, Clone)]
pub enum ValidationResult {
    /// The path is a valid git repository ready for use.
    Valid(RepoInfo),
    /// The path has an issue.
    Invalid(String),
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
            validation: None,
            confirm_state: ConfirmState::None,
            selection_error: None,
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
            validation: None,
            confirm_state: ConfirmState::None,
            selection_error: None,
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
                self.validation = None;
                KeyResult::Consumed
            }

            // Text editing.
            (_, KeyCode::Char(c)) => {
                self.input.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                self.validation = None;
                KeyResult::Consumed
            }
            (_, KeyCode::Backspace) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.input.remove(self.cursor);
                    self.validation = None;
                }
                KeyResult::Consumed
            }
            (_, KeyCode::Delete) => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                    self.validation = None;
                }
                KeyResult::Consumed
            }
            (_, KeyCode::Left) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                KeyResult::Consumed
            }
            (_, KeyCode::Right) => {
                if self.cursor < self.input.len() {
                    self.cursor += 1;
                }
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

    /// Validate the current input path against the filesystem and git.
    ///
    /// This performs synchronous validation (fast enough for a single repo check).
    pub fn validate(&mut self, home: &Path) {
        let expanded = expand_tilde(&self.input, home);

        if expanded.as_os_str().is_empty() {
            self.validation = Some(ValidationResult::Invalid(
                "Path cannot be empty".to_string(),
            ));
            return;
        }

        if !expanded.is_absolute() {
            self.validation = Some(ValidationResult::Invalid(
                "Path must be absolute or start with ~/".to_string(),
            ));
            return;
        }

        if !expanded.exists() {
            self.validation = Some(ValidationResult::Invalid(format!(
                "Directory does not exist: {}",
                expanded.display()
            )));
            return;
        }

        // Validate git repository structure.
        use crate::git::{GitRunner, classify_ownership, validate_repository};
        let runner = GitRunner::new(std::time::Duration::from_secs(120));

        match validate_repository(&runner, &expanded, "origin") {
            Ok(info) => {
                // Classify ownership.
                match classify_ownership(&expanded) {
                    Ok(state) => {
                        use crate::git::OwnershipState;
                        let ownership = match &state {
                            OwnershipState::New => OwnershipInfo::New,
                            OwnershipState::Owned { manifest } => OwnershipInfo::Owned {
                                sources: manifest.sources.iter().map(|s| s.path.clone()).collect(),
                            },
                            OwnershipState::InvalidManifest { reason } => {
                                OwnershipInfo::InvalidManifest(reason.clone())
                            }
                            OwnershipState::Ambiguous { reason } => {
                                OwnershipInfo::Ambiguous(reason.clone())
                            }
                        };

                        self.validation = Some(ValidationResult::Valid(RepoInfo {
                            path: expanded,
                            branch: info.branch,
                            ownership,
                        }));
                    }
                    Err(e) => {
                        self.validation = Some(ValidationResult::Invalid(format!(
                            "Failed to classify ownership: {e}"
                        )));
                    }
                }
            }
            Err(e) => {
                self.validation = Some(ValidationResult::Invalid(format!(
                    "Not a valid repository: {e}"
                )));
            }
        }
    }

    /// Attempt to confirm the current validated repository.
    ///
    /// Returns the path to save in config if confirmed and initialized/attached
    /// successfully, or an error message.
    pub fn confirm(&mut self, _home: &Path) -> Result<PathBuf, String> {
        let info = match &self.validation {
            Some(ValidationResult::Valid(info)) => info.clone(),
            _ => return Err("No valid repository to confirm".to_string()),
        };

        use crate::git::{classify_ownership, initialize_or_attach};

        let state = classify_ownership(&info.path).map_err(|e| e.to_string())?;
        initialize_or_attach(&info.path, &state, true).map_err(|e| e.to_string())?;

        self.confirm_state = ConfirmState::Done;
        Ok(info.path)
    }
}

/// The result of handling a key in the repo screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    /// The key was consumed (input edited, dialog handled).
    Consumed,
    /// The user wants to validate the current path.
    Validate,
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
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn new_screen_is_empty() {
        let screen = RepoScreen::new();
        assert!(screen.input.is_empty());
        assert_eq!(screen.cursor, 0);
        assert!(screen.validation.is_none());
    }

    #[test]
    fn with_path_prefills_input() {
        let screen = RepoScreen::with_path("~/dotfiles");
        assert_eq!(screen.input, "~/dotfiles");
        assert_eq!(screen.cursor, 10);
    }

    #[test]
    fn typing_inserts_characters() {
        let mut screen = RepoScreen::new();
        screen.mode = RepoMode::TextInput;
        screen.handle_key(key(KeyCode::Char('/')));
        screen.handle_key(key(KeyCode::Char('t')));
        screen.handle_key(key(KeyCode::Char('m')));
        screen.handle_key(key(KeyCode::Char('p')));
        assert_eq!(screen.input, "/tmp");
        assert_eq!(screen.cursor, 4);
    }

    #[test]
    fn backspace_deletes_before_cursor() {
        let mut screen = RepoScreen::with_path("/tmp");
        screen.mode = RepoMode::TextInput;
        screen.handle_key(key(KeyCode::Backspace));
        assert_eq!(screen.input, "/tm");
        assert_eq!(screen.cursor, 3);
    }

    #[test]
    fn left_right_moves_cursor() {
        let mut screen = RepoScreen::with_path("/tmp");
        screen.mode = RepoMode::TextInput;
        screen.handle_key(key(KeyCode::Left));
        assert_eq!(screen.cursor, 3);
        screen.handle_key(key(KeyCode::Left));
        assert_eq!(screen.cursor, 2);
        screen.handle_key(key(KeyCode::Right));
        assert_eq!(screen.cursor, 3);
    }

    #[test]
    fn home_end_jump_cursor() {
        let mut screen = RepoScreen::with_path("/home/user/repo");
        screen.mode = RepoMode::TextInput;
        screen.handle_key(key(KeyCode::Home));
        assert_eq!(screen.cursor, 0);
        screen.handle_key(key(KeyCode::End));
        assert_eq!(screen.cursor, 15);
    }

    #[test]
    fn ctrl_u_clears_input() {
        let mut screen = RepoScreen::with_path("/some/path");
        screen.mode = RepoMode::TextInput;
        screen.handle_key(key_mod(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(screen.input.is_empty());
        assert_eq!(screen.cursor, 0);
    }

    #[test]
    fn enter_returns_validate() {
        let mut screen = RepoScreen::with_path("/tmp");
        screen.mode = RepoMode::TextInput;
        let result = screen.handle_key(key(KeyCode::Enter));
        assert_eq!(result, KeyResult::Validate);
    }

    #[test]
    fn validate_rejects_empty_path() {
        let mut screen = RepoScreen::new();
        let home = PathBuf::from("/home/test");
        screen.validate(&home);
        match &screen.validation {
            Some(ValidationResult::Invalid(msg)) => assert!(msg.contains("empty")),
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn validate_rejects_relative_path() {
        let mut screen = RepoScreen::with_path("relative/path");
        let home = PathBuf::from("/home/test");
        screen.validate(&home);
        match &screen.validation {
            Some(ValidationResult::Invalid(msg)) => assert!(msg.contains("absolute")),
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn validate_rejects_nonexistent_path() {
        let mut screen = RepoScreen::with_path("/nonexistent/path/12345");
        let home = PathBuf::from("/home/test");
        screen.validate(&home);
        match &screen.validation {
            Some(ValidationResult::Invalid(msg)) => assert!(msg.contains("does not exist")),
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn validate_rejects_non_repo_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let mut screen = RepoScreen::with_path(&path);
        let home = PathBuf::from("/home/test");
        screen.validate(&home);
        match &screen.validation {
            Some(ValidationResult::Invalid(msg)) => {
                assert!(
                    msg.contains("repository") || msg.contains("git"),
                    "unexpected message: {msg}"
                );
            }
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn validate_accepts_valid_repo() {
        // Create a minimal git repo.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["remote", "add", "origin", "/dev/null"])
            .current_dir(repo)
            .output()
            .unwrap();
        // Make an initial commit so HEAD exists.
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(repo)
            .output()
            .unwrap();

        let path = repo.to_str().unwrap().to_string();
        let mut screen = RepoScreen::with_path(&path);
        let home = PathBuf::from("/home/test");
        screen.validate(&home);

        match &screen.validation {
            Some(ValidationResult::Valid(info)) => {
                assert_eq!(info.branch, "main");
                assert!(matches!(info.ownership, OwnershipInfo::New));
            }
            Some(ValidationResult::Invalid(msg)) => panic!("unexpected invalid: {msg}"),
            None => panic!("expected validation result"),
        }
    }

    #[test]
    fn expand_tilde_expands_home() {
        let home = Path::new("/home/user");
        assert_eq!(
            expand_tilde("~/repo", home),
            PathBuf::from("/home/user/repo")
        );
        assert_eq!(expand_tilde("~", home), PathBuf::from("/home/user"));
        assert_eq!(expand_tilde("/abs/path", home), PathBuf::from("/abs/path"));
    }

    #[test]
    fn confirm_dialog_y_confirms() {
        let mut screen = RepoScreen::new();
        screen.confirm_state = ConfirmState::AskInitialize;
        let result = screen.handle_key(key(KeyCode::Char('y')));
        assert_eq!(result, KeyResult::Confirm);
    }

    #[test]
    fn confirm_dialog_n_cancels() {
        let mut screen = RepoScreen::new();
        screen.confirm_state = ConfirmState::AskAttach;
        let result = screen.handle_key(key(KeyCode::Char('n')));
        assert_eq!(result, KeyResult::Consumed);
        assert_eq!(screen.confirm_state, ConfirmState::None);
    }

    #[test]
    fn confirm_dialog_esc_cancels() {
        let mut screen = RepoScreen::new();
        screen.confirm_state = ConfirmState::AskInitialize;
        let result = screen.handle_key(key(KeyCode::Esc));
        assert_eq!(result, KeyResult::Consumed);
        assert_eq!(screen.confirm_state, ConfirmState::None);
    }

    // --- Browser mode tests ---

    #[test]
    fn new_screen_defaults_to_browser_mode() {
        let screen = RepoScreen::new();
        assert_eq!(screen.mode, RepoMode::Browser);
    }

    #[test]
    fn colon_switches_to_text_input() {
        let mut screen = RepoScreen::new();
        screen.ensure_browser(Path::new("/tmp"));
        let result = screen.handle_key(key(KeyCode::Char(':')));
        assert_eq!(result, KeyResult::Consumed);
        assert_eq!(screen.mode, RepoMode::TextInput);
    }

    #[test]
    fn slash_switches_to_text_input() {
        let mut screen = RepoScreen::new();
        screen.ensure_browser(Path::new("/tmp"));
        let result = screen.handle_key(key(KeyCode::Char('/')));
        assert_eq!(result, KeyResult::Consumed);
        assert_eq!(screen.mode, RepoMode::TextInput);
    }

    #[test]
    fn esc_in_text_input_returns_to_browser() {
        let mut screen = RepoScreen::new();
        screen.mode = RepoMode::TextInput;
        let result = screen.handle_key(key(KeyCode::Esc));
        assert_eq!(result, KeyResult::Consumed);
        assert_eq!(screen.mode, RepoMode::Browser);
    }

    #[test]
    fn browser_ensure_creates_browser() {
        let mut screen = RepoScreen::new();
        assert!(screen.browser.is_none());
        screen.ensure_browser(Path::new("/tmp"));
        assert!(screen.browser.is_some());
    }

    #[test]
    fn browser_space_on_directory_validates() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();

        let mut screen = RepoScreen::new();
        screen.ensure_browser(tmp.path());
        // Navigate browser to start at the temp dir.
        if let Some(ref mut browser) = screen.browser {
            browser.navigate_to(tmp.path());
            let _ = browser.current_listing();
            // Select the subdir (should be index 0 since it's the only entry).
        }

        let result = screen.handle_key(key(KeyCode::Char(' ')));
        // Should try to validate a directory.
        assert_eq!(result, KeyResult::Validate);
    }

    #[test]
    fn browser_rejects_file_selection() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("file.txt"), "x").unwrap();

        let mut screen = RepoScreen::new();
        screen.ensure_browser(tmp.path());
        if let Some(ref mut browser) = screen.browser {
            browser.navigate_to(tmp.path());
            let _ = browser.current_listing();
        }

        let result = screen.handle_key(key(KeyCode::Char(' ')));
        // Should not validate — only dirs allowed.
        assert_eq!(result, KeyResult::Consumed);
        assert!(screen.selection_error.is_some());
    }
}
