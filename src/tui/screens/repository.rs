//! Repository selection screen state.
//!
//! Allows the user to input a repository path, validate it against the
//! backend (git structure, ownership), and confirm initialization or
//! attachment.

use std::path::{Path, PathBuf};

/// The state of the repository selection screen.
#[derive(Debug)]
pub struct RepoScreen {
    /// The text input buffer for the repository path.
    pub input: String,
    /// Current cursor position in the input.
    pub cursor: usize,
    /// Validation result after the user presses Enter.
    pub validation: Option<ValidationResult>,
    /// Whether a confirmation dialog is active.
    pub confirm_state: ConfirmState,
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
    /// Create a new repository screen, pre-populated with the current config.
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            validation: None,
            confirm_state: ConfirmState::None,
        }
    }

    /// Create with a pre-filled path from the existing config.
    pub fn with_path(path: &str) -> Self {
        let cursor = path.len();
        Self {
            input: path.to_string(),
            cursor,
            validation: None,
            confirm_state: ConfirmState::None,
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
            return match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => KeyResult::Confirm,
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.confirm_state = ConfirmState::None;
                    KeyResult::Consumed
                }
                _ => KeyResult::Consumed,
            };
        }

        match (key.modifiers, key.code) {
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
        screen.handle_key(key(KeyCode::Backspace));
        assert_eq!(screen.input, "/tm");
        assert_eq!(screen.cursor, 3);
    }

    #[test]
    fn left_right_moves_cursor() {
        let mut screen = RepoScreen::with_path("/tmp");
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
        screen.handle_key(key(KeyCode::Home));
        assert_eq!(screen.cursor, 0);
        screen.handle_key(key(KeyCode::End));
        assert_eq!(screen.cursor, 15);
    }

    #[test]
    fn ctrl_u_clears_input() {
        let mut screen = RepoScreen::with_path("/some/path");
        screen.handle_key(key_mod(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(screen.input.is_empty());
        assert_eq!(screen.cursor, 0);
    }

    #[test]
    fn enter_returns_validate() {
        let mut screen = RepoScreen::with_path("/tmp");
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
}
