//! Ignore rule editor screen state.
//!
//! Allows the user to select a source, view/edit its ignore patterns,
//! preview which files would be matched, and see warnings about secrets
//! or already-tracked files.

use std::path::Path;

use crate::tui::{task::LoadState, text, viewport::Viewport};

/// The state of the ignore editor screen.
#[derive(Debug)]
pub struct IgnoreScreen {
    /// Index of the currently selected source.
    pub source_idx: usize,
    /// Current mode.
    pub mode: Mode,
    /// Index of the selected pattern in the list.
    pub pattern_idx: usize,
    /// Which nested level has focus within List mode.
    pub list_focus: ListFocus,
    /// Text input buffer for adding/editing a pattern.
    pub input: String,
    /// Cursor position in the input.
    pub cursor: usize,
    /// Lifecycle and last usable preview of matched files.
    pub preview_state: LoadState<Vec<PreviewEntry>>,
    /// Viewport for the file preview.
    pub(crate) preview_viewport: Viewport,
    /// Feedback message promoted to the global status region.
    pub message: Option<Message>,
}

/// Within List mode, which nested control has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFocus {
    /// The source selector row at the top.
    SourceSelector,
    /// The pattern list below the source selector.
    PatternList,
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

/// Semantic category for ignore-editor feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub text: String,
    pub kind: MessageKind,
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
            list_focus: ListFocus::SourceSelector,
            input: String::new(),
            cursor: 0,
            preview_state: LoadState::NotLoaded,
            preview_viewport: Viewport::default(),
            message: None,
        }
    }

    /// Replace preview data while preserving and clamping its viewport.
    #[cfg(test)]
    pub(crate) fn replace_preview(&mut self, preview: Vec<PreviewEntry>) {
        let len = preview.len();
        self.preview_state = LoadState::Loaded(preview);
        self.preview_viewport.clamp(len);
    }

    pub(crate) fn preview(&self) -> Option<&[PreviewEntry]> {
        self.preview_state.data().map(Vec::as_slice)
    }

    pub(crate) fn mark_preview_stale(&mut self) {
        self.preview_state.invalidate();
    }

    /// Update the preview viewport from the actual render area.
    pub(crate) fn set_preview_viewport_height(&mut self, height: usize) {
        let len = self.preview().map_or(0, <[PreviewEntry]>::len);
        self.preview_viewport.set_height(height, len);
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
                // Navigate patterns or move between nested levels.
                (_, KeyCode::Up | KeyCode::Char('k')) => match self.list_focus {
                    ListFocus::PatternList => {
                        if self.pattern_idx > 0 {
                            self.pattern_idx -= 1;
                        } else {
                            // At top of pattern list — move to source selector.
                            self.list_focus = ListFocus::SourceSelector;
                        }
                        Action::Consumed
                    }
                    ListFocus::SourceSelector => {
                        // At upper boundary — let parent handle focus return.
                        Action::NotConsumed
                    }
                },
                (_, KeyCode::Down | KeyCode::Char('j')) => match self.list_focus {
                    ListFocus::SourceSelector => {
                        // Move into the pattern list.
                        self.list_focus = ListFocus::PatternList;
                        Action::Consumed
                    }
                    ListFocus::PatternList => {
                        if pattern_count > 0 && self.pattern_idx < pattern_count - 1 {
                            self.pattern_idx += 1;
                        }
                        Action::Consumed
                    }
                },

                // Switch source (Left/Right or h/l) — works in both focus levels.
                (_, KeyCode::Left | KeyCode::Char('h')) => {
                    if self.source_idx > 0 {
                        self.source_idx -= 1;
                        self.pattern_idx = 0;
                        self.list_focus = ListFocus::SourceSelector;
                        self.preview_viewport.home();
                        self.mark_preview_stale();
                    }
                    Action::Consumed
                }
                (_, KeyCode::Right | KeyCode::Char('l')) => {
                    if source_count > 0 && self.source_idx < source_count - 1 {
                        self.source_idx += 1;
                        self.pattern_idx = 0;
                        self.list_focus = ListFocus::SourceSelector;
                        self.preview_viewport.home();
                        self.mark_preview_stale();
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
                // Tab/Shift+Tab escape to tab bar even from input mode.
                (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    Action::NotConsumed
                }

                // Submit the new pattern.
                (_, KeyCode::Enter) => {
                    let pattern = self.input.clone();
                    if pattern.is_empty() {
                        self.message = Some(Message {
                            text: "Pattern cannot be empty".to_string(),
                            kind: MessageKind::Error,
                        });
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

                _ => Action::Consumed,
            },

            Mode::Preview => match (key.modifiers, key.code) {
                // Tab/Shift+Tab escape to tab bar even from preview mode.
                (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                    Action::NotConsumed
                }
                // Return to list. q remains the explicit application quit key.
                (_, KeyCode::Esc) | (_, KeyCode::Char('p')) => {
                    self.mode = Mode::List;
                    Action::Consumed
                }
                (_, KeyCode::Char('r')) => Action::RefreshPreview(self.source_idx),
                (_, KeyCode::Char('q')) => Action::NotConsumed,
                // Scroll preview.
                (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
                    self.preview_viewport.scroll_up(1);
                    Action::Consumed
                }
                (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
                    let len = self.preview().map_or(0, <[PreviewEntry]>::len);
                    self.preview_viewport.scroll_down(1, len);
                    Action::Consumed
                }
                (_, KeyCode::PageUp) => {
                    self.preview_viewport
                        .scroll_up(self.preview_viewport.page_size());
                    Action::Consumed
                }
                (_, KeyCode::PageDown) => {
                    let len = self.preview().map_or(0, <[PreviewEntry]>::len);
                    self.preview_viewport
                        .scroll_down(self.preview_viewport.page_size(), len);
                    Action::Consumed
                }
                (_, KeyCode::Home) => {
                    self.preview_viewport.home();
                    Action::Consumed
                }
                (_, KeyCode::End) => {
                    let len = self.preview().map_or(0, <[PreviewEntry]>::len);
                    self.preview_viewport.end(len);
                    Action::Consumed
                }
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
        // Start at SourceSelector; Down enters PatternList.
        screen.handle_key(key(KeyCode::Down), 3, 1);
        assert_eq!(screen.list_focus, ListFocus::PatternList);
        assert_eq!(screen.pattern_idx, 0);
        // Down again advances pattern_idx.
        screen.handle_key(key(KeyCode::Down), 3, 1);
        assert_eq!(screen.pattern_idx, 1);
        // Up returns to previous pattern.
        screen.handle_key(key(KeyCode::Up), 3, 1);
        assert_eq!(screen.pattern_idx, 0);
        // Up at pattern_idx 0 returns to SourceSelector.
        screen.handle_key(key(KeyCode::Up), 3, 1);
        assert_eq!(screen.list_focus, ListFocus::SourceSelector);
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
    fn add_input_handles_multibyte_characters() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::AddInput;
        screen.handle_key(key(KeyCode::Char('界')), 0, 1);
        screen.handle_key(key(KeyCode::Char('é')), 0, 1);
        screen.handle_key(key(KeyCode::Left), 0, 1);
        screen.handle_key(key(KeyCode::Backspace), 0, 1);
        assert_eq!(screen.input, "é");
        assert_eq!(screen.cursor, 0);
        screen.handle_key(key(KeyCode::Delete), 0, 1);
        assert!(screen.input.is_empty());
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
    fn r_refreshes_while_preview_remains_open() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::Preview;
        assert_eq!(
            screen.handle_key(key(KeyCode::Char('r')), 0, 1),
            Action::RefreshPreview(0)
        );
        assert_eq!(screen.mode, Mode::Preview);
    }

    #[test]
    fn one_row_preview_does_not_scroll_out_of_view() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::Preview;
        screen.replace_preview(vec![PreviewEntry {
            path: "only-file".to_string(),
            ignored: false,
            matched_by: None,
            secret_warning: false,
        }]);
        screen.set_preview_viewport_height(4);

        screen.handle_key(key(KeyCode::End), 0, 1);
        screen.handle_key(key(KeyCode::Down), 0, 1);

        assert_eq!(screen.preview_viewport.visible_range(1), 0..1);
    }

    #[test]
    fn preview_scrolls_with_arrows_pages_home_and_end() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::Preview;
        screen.replace_preview(
            (0..12)
                .map(|i| PreviewEntry {
                    path: format!("file-{i}"),
                    ignored: false,
                    matched_by: None,
                    secret_warning: false,
                })
                .collect(),
        );
        screen.set_preview_viewport_height(4);

        screen.handle_key(key(KeyCode::Down), 0, 1);
        assert_eq!(screen.preview_viewport.offset(), 1);
        screen.handle_key(key(KeyCode::PageDown), 0, 1);
        assert_eq!(screen.preview_viewport.offset(), 5);
        screen.handle_key(key(KeyCode::End), 0, 1);
        assert_eq!(screen.preview_viewport.visible_range(12), 8..12);
        screen.handle_key(key(KeyCode::PageUp), 0, 1);
        assert_eq!(screen.preview_viewport.offset(), 4);
        screen.handle_key(key(KeyCode::Home), 0, 1);
        assert_eq!(screen.preview_viewport.offset(), 0);
    }

    #[test]
    fn preview_viewport_clamps_after_refresh_shrinks_data() {
        let mut screen = IgnoreScreen::new();
        screen.replace_preview(
            (0..10)
                .map(|i| PreviewEntry {
                    path: format!("file-{i}"),
                    ignored: false,
                    matched_by: None,
                    secret_warning: false,
                })
                .collect(),
        );
        screen.set_preview_viewport_height(3);
        screen.preview_viewport.end(10);

        let shortened = screen.preview().unwrap()[..2].to_vec();
        screen.replace_preview(shortened);

        assert_eq!(screen.preview_viewport.visible_range(2), 0..2);
    }

    #[test]
    fn preview_viewport_survives_tab_focus_escape() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::Preview;
        screen.replace_preview(
            (0..8)
                .map(|i| PreviewEntry {
                    path: format!("file-{i}"),
                    ignored: false,
                    matched_by: None,
                    secret_warning: false,
                })
                .collect(),
        );
        screen.set_preview_viewport_height(3);
        screen.handle_key(key(KeyCode::PageDown), 0, 1);

        let action = screen.handle_key(key(KeyCode::Tab), 0, 1);

        assert_eq!(action, Action::NotConsumed);
        assert_eq!(screen.preview_viewport.offset(), 3);
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

    #[test]
    fn up_at_source_selector_returns_not_consumed() {
        let mut screen = IgnoreScreen::new();
        screen.list_focus = ListFocus::SourceSelector;
        let action = screen.handle_key(key(KeyCode::Up), 3, 2);
        assert_eq!(action, Action::NotConsumed);
    }

    #[test]
    fn up_at_pattern_zero_moves_to_source_selector() {
        let mut screen = IgnoreScreen::new();
        screen.list_focus = ListFocus::PatternList;
        screen.pattern_idx = 0;
        let action = screen.handle_key(key(KeyCode::Up), 3, 2);
        assert_eq!(action, Action::Consumed);
        assert_eq!(screen.list_focus, ListFocus::SourceSelector);
    }

    #[test]
    fn tab_in_add_input_returns_not_consumed() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::AddInput;
        screen.input = "*.log".to_string();
        let action = screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 0, 1);
        assert_eq!(action, Action::NotConsumed);
        assert_eq!(screen.input, "*.log");
    }

    #[test]
    fn tab_in_preview_returns_not_consumed() {
        let mut screen = IgnoreScreen::new();
        screen.mode = Mode::Preview;
        let action = screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 0, 1);
        assert_eq!(action, Action::NotConsumed);
    }
}
