//! Ignore rule editor screen state.
//!
//! Allows the user to select a source, view/edit its ignore patterns,
//! preview which files would be matched, and see warnings about secrets
//! or already-tracked files.

use std::path::Path;

/// The state of the ignore editor screen.
#[derive(Debug)]
pub struct IgnoreScreen {
    /// Index of the currently selected source.
    pub source_idx: usize,
    /// Current mode.
    pub mode: Mode,
    /// Index of the selected pattern in the list.
    pub pattern_idx: usize,
    /// Text input buffer for adding/editing a pattern.
    pub input: String,
    /// Cursor position in the input.
    pub cursor: usize,
    /// Preview of matched files for the current patterns.
    pub preview: Vec<PreviewEntry>,
    /// Whether the preview is stale and needs refresh.
    pub preview_stale: bool,
    /// Feedback message.
    pub message: Option<String>,
}

/// A preview entry showing a file and its match status.
#[derive(Debug, Clone)]
pub struct PreviewEntry {
    /// Relative path from the source root.
    pub path: String,
    /// Whether this file is ignored by the current patterns.
    pub ignored: bool,
    /// The pattern that matched (if ignored).
    pub matched_by: Option<String>,
    /// Whether this file looks like a secret.
    pub secret_warning: bool,
}

/// The mode the ignore screen is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing the pattern list.
    List,
    /// Adding a new pattern.
    AddInput,
    /// Viewing the file preview.
    Preview,
}

impl Default for IgnoreScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl IgnoreScreen {
    pub fn new() -> Self {
        Self {
            source_idx: 0,
            mode: Mode::List,
            pattern_idx: 0,
            input: String::new(),
            cursor: 0,
            preview: Vec::new(),
            preview_stale: true,
            message: None,
        }
    }

    /// Handle a key event for this screen.
    pub fn handle_key(
        &mut self,
        key: crossterm::event::KeyEvent,
        pattern_count: usize,
        source_count: usize,
    ) -> Action {
        use crossterm::event::{KeyCode, KeyModifiers};

        match self.mode {
            Mode::List => match (key.modifiers, key.code) {
                // Navigate patterns.
                (_, KeyCode::Up | KeyCode::Char('k')) => {
                    if self.pattern_idx > 0 {
                        self.pattern_idx -= 1;
                    }
                    Action::Consumed
                }
                (_, KeyCode::Down | KeyCode::Char('j')) => {
                    if pattern_count > 0 && self.pattern_idx < pattern_count - 1 {
                        self.pattern_idx += 1;
                    }
                    Action::Consumed
                }

                // Switch source (Left/Right or h/l).
                (_, KeyCode::Left | KeyCode::Char('h')) => {
                    if self.source_idx > 0 {
                        self.source_idx -= 1;
                        self.pattern_idx = 0;
                        self.preview_stale = true;
                    }
                    Action::Consumed
                }
                (_, KeyCode::Right | KeyCode::Char('l')) => {
                    if source_count > 0 && self.source_idx < source_count - 1 {
                        self.source_idx += 1;
                        self.pattern_idx = 0;
                        self.preview_stale = true;
                    }
                    Action::Consumed
                }

                // Add a new pattern.
                (_, KeyCode::Char('a')) => {
                    self.mode = Mode::AddInput;
                    self.input.clear();
                    self.cursor = 0;
                    self.message = None;
                    Action::Consumed
                }

                // Delete the selected pattern.
                (_, KeyCode::Char('d') | KeyCode::Delete) if pattern_count > 0 => {
                    Action::RemovePattern(self.source_idx, self.pattern_idx)
                }

                // Show/refresh preview.
                (_, KeyCode::Char('p')) => {
                    self.mode = Mode::Preview;
                    Action::RefreshPreview(self.source_idx)
                }

                _ => Action::NotConsumed,
            },

            Mode::AddInput => match (key.modifiers, key.code) {
                // Submit the new pattern.
                (_, KeyCode::Enter) => {
                    let pattern = self.input.clone();
                    if pattern.is_empty() {
                        self.message = Some("Pattern cannot be empty".to_string());
                        Action::Consumed
                    } else {
                        Action::AddPattern(self.source_idx, pattern)
                    }
                }

                // Cancel.
                (_, KeyCode::Esc) => {
                    self.mode = Mode::List;
                    self.message = None;
                    Action::Consumed
                }

                // Ctrl shortcuts before generic Char.
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

                _ => Action::Consumed,
            },

            Mode::Preview => match key.code {
                // Return to list.
                KeyCode::Esc | KeyCode::Char('p') | KeyCode::Char('q') => {
                    self.mode = Mode::List;
                    Action::Consumed
                }
                // Scroll preview.
                KeyCode::Up | KeyCode::Char('k') => {
                    // Scrolling would be handled by a scroll offset but
                    // for now we just consume the event.
                    Action::Consumed
                }
                KeyCode::Down | KeyCode::Char('j') => Action::Consumed,
                _ => Action::Consumed,
            },
        }
    }

    /// Generate the file preview for a source's current patterns.
    ///
    /// Walks the source directory and applies the ignore matcher to each file.
    pub fn generate_preview(
        source_path: &str,
        patterns: &[String],
        home: &Path,
    ) -> Vec<PreviewEntry> {
        use crate::backup::ignore::IgnoreMatcher;
        use crate::backup::secrets;

        let source_abs = home.join(source_path);
        if !source_abs.exists() {
            return vec![PreviewEntry {
                path: format!("(source '{}' does not exist)", source_path),
                ignored: false,
                matched_by: None,
                secret_warning: false,
            }];
        }

        let (matcher, _errors) = IgnoreMatcher::new(&source_abs, patterns);

        let mut entries = Vec::new();

        // Walk the source directory (limited depth for preview performance).
        if let Ok(walker) = walk_for_preview(&source_abs) {
            for entry in walker.into_iter().take(100) {
                let rel = match entry.strip_prefix(&source_abs) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if rel.as_os_str().is_empty() {
                    continue;
                }

                let is_dir = entry.is_dir();
                let match_result = matcher.matches(rel, is_dir);

                let ignored = matches!(
                    &match_result,
                    crate::backup::ignore::MatchResult::Ignored { .. }
                );
                let matched_by = match &match_result {
                    crate::backup::ignore::MatchResult::Ignored { pattern } => {
                        Some(pattern.clone())
                    }
                    _ => None,
                };

                let rel_str = rel.to_string_lossy().to_string();
                let secret_warning = secrets::detect_secret(rel).is_some();

                entries.push(PreviewEntry {
                    path: rel_str,
                    ignored,
                    matched_by,
                    secret_warning,
                });
            }
        }

        entries
    }
}

/// Simple recursive directory listing for preview (no symlink following).
fn walk_for_preview(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut results = Vec::new();
    walk_recursive(root, root, &mut results, 3)?;
    results.sort();
    Ok(results)
}

/// Recursive helper limited to a max depth.
fn walk_recursive(
    _root: &Path,
    current: &Path,
    results: &mut Vec<std::path::PathBuf>,
    max_depth: usize,
) -> std::io::Result<()> {
    if max_depth == 0 || results.len() >= 100 {
        return Ok(());
    }

    let entries = std::fs::read_dir(current)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip .git directories.
        if meta.is_dir() && path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }

        results.push(path.clone());

        if meta.is_dir() && !meta.is_symlink() {
            walk_recursive(_root, &path, results, max_depth - 1)?;
        }

        if results.len() >= 100 {
            break;
        }
    }
    Ok(())
}

/// Actions from the ignore screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Consumed,
    NotConsumed,
    /// Add a pattern to the source at the given index.
    AddPattern(usize, String),
    /// Remove the pattern at (source_idx, pattern_idx).
    RemovePattern(usize, usize),
    /// Refresh the preview for the source at the given index.
    RefreshPreview(usize),
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
        let screen = IgnoreScreen::new();
        assert_eq!(screen.mode, Mode::List);
        assert_eq!(screen.source_idx, 0);
        assert_eq!(screen.pattern_idx, 0);
    }

    #[test]
    fn up_down_navigates_patterns() {
        let mut screen = IgnoreScreen::new();
        screen.handle_key(key(KeyCode::Down), 3, 1);
        assert_eq!(screen.pattern_idx, 1);
        screen.handle_key(key(KeyCode::Up), 3, 1);
        assert_eq!(screen.pattern_idx, 0);
    }

    #[test]
    fn left_right_switches_source() {
        let mut screen = IgnoreScreen::new();
        screen.handle_key(key(KeyCode::Right), 0, 3);
        assert_eq!(screen.source_idx, 1);
        screen.handle_key(key(KeyCode::Left), 0, 3);
        assert_eq!(screen.source_idx, 0);
    }

    #[test]
    fn a_enters_add_mode() {
        let mut screen = IgnoreScreen::new();
        let action = screen.handle_key(key(KeyCode::Char('a')), 0, 1);
        assert_eq!(action, Action::Consumed);
        assert_eq!(screen.mode, Mode::AddInput);
    }

    #[test]
    fn enter_in_add_mode_returns_add_action() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::AddInput;
        screen.input = "*.log".to_string();
        screen.cursor = screen.input.len();
        let action = screen.handle_key(key(KeyCode::Enter), 0, 1);
        assert_eq!(action, Action::AddPattern(0, "*.log".to_string()));
    }

    #[test]
    fn esc_in_add_mode_returns_to_list() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::AddInput;
        screen.handle_key(key(KeyCode::Esc), 0, 1);
        assert_eq!(screen.mode, Mode::List);
    }

    #[test]
    fn d_returns_remove_action() {
        let mut screen = IgnoreScreen::new();
        screen.pattern_idx = 1;
        let action = screen.handle_key(key(KeyCode::Char('d')), 3, 1);
        assert_eq!(action, Action::RemovePattern(0, 1));
    }

    #[test]
    fn p_enters_preview_mode() {
        let mut screen = IgnoreScreen::new();
        let action = screen.handle_key(key(KeyCode::Char('p')), 0, 1);
        assert_eq!(action, Action::RefreshPreview(0));
        assert_eq!(screen.mode, Mode::Preview);
    }

    #[test]
    fn esc_in_preview_returns_to_list() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::Preview;
        screen.handle_key(key(KeyCode::Esc), 0, 1);
        assert_eq!(screen.mode, Mode::List);
    }

    #[test]
    fn generate_preview_for_nonexistent_source() {
        let home = std::path::Path::new("/tmp/nonexistent-dothoard-test");
        let entries = IgnoreScreen::generate_preview("missing-source", &[], home);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.contains("does not exist"));
    }

    #[test]
    fn generate_preview_marks_ignored_files() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let source = home.join("test-source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("keep.txt"), "keep").unwrap();
        std::fs::write(source.join("remove.log"), "log").unwrap();

        let patterns = vec!["*.log".to_string()];
        let entries = IgnoreScreen::generate_preview("test-source", &patterns, home);

        let log_entry = entries.iter().find(|e| e.path.contains("remove.log"));
        let keep_entry = entries.iter().find(|e| e.path.contains("keep.txt"));

        assert!(log_entry.is_some(), "log file should appear in preview");
        assert!(log_entry.unwrap().ignored);
        assert!(keep_entry.is_some(), "keep file should appear in preview");
        assert!(!keep_entry.unwrap().ignored);
    }

    #[test]
    fn generate_preview_detects_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let source = home.join("ssh-test");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("id_rsa"), "private key").unwrap();
        std::fs::write(source.join("config"), "config").unwrap();

        let entries = IgnoreScreen::generate_preview("ssh-test", &[], home);

        let key_entry = entries.iter().find(|e| e.path.contains("id_rsa"));
        assert!(key_entry.is_some());
        assert!(key_entry.unwrap().secret_warning);
    }
}
