//! History and error details screen state.
//!
//! Displays recent backup runs from the persistent state, with details
//! for the selected entry including outcome, commit, duration, and messages.
//! Also provides a scrollable log viewer for viewing run logs.

use std::path::Path;

use crate::state::{RunOutcome, RunRecord};
use crate::tui::viewport::Viewport;

/// The display mode for the history screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Normal history list view.
    History,
    /// Log viewer for the selected entry.
    LogView,
}

/// The state of the history screen.
#[derive(Debug)]
pub struct HistoryScreen {
    /// Index of the currently selected run in the history list.
    pub selected: usize,
    /// Viewport for the history list.
    pub(crate) list_viewport: Viewport,
    /// Viewport for the selected run's log.
    pub(crate) log_viewport: Viewport,
    /// Current display mode.
    pub mode: Mode,
    /// Cached log lines for the current log view.
    pub log_lines: Vec<String>,
    /// Namespace of the run whose log is displayed.
    pub log_namespace: Option<String>,
}

impl Default for HistoryScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryScreen {
    pub fn new() -> Self {
        Self {
            selected: 0,
            list_viewport: Viewport::default(),
            log_viewport: Viewport::default(),
            mode: Mode::History,
            log_lines: Vec::new(),
            log_namespace: None,
        }
    }

    /// Enter log view mode with the per-run log file for the given record.
    pub fn enter_log_view(&mut self, record: &RunRecord, state_dir: &Path) {
        self.log_lines = if let Some(ref log_file) = record.log_file {
            // Per-run log file exists: read it directly.
            let log_path = crate::diagnostics::log_dir(state_dir).join(log_file);
            Self::read_log_file(&log_path)
        } else {
            // Legacy fallback: filter by timestamp from the session log.
            let log_path = state_dir.join("dothoard.log");
            Self::filter_logs_by_timestamp(&log_path, record.started_at, record.finished_at)
        };
        self.log_namespace = (!record.namespace.is_empty()).then(|| record.namespace.clone());
        self.mode = Mode::LogView;
        self.log_viewport.home();
        self.log_viewport.clamp(self.log_lines.len());
    }

    /// Exit log view mode and return to history list.
    pub fn exit_log_view(&mut self) {
        self.mode = Mode::History;
        self.log_lines.clear();
        self.log_namespace = None;
        self.log_viewport.home();
    }

    /// Read all lines from a log file.
    pub fn read_log_file(log_path: &Path) -> Vec<String> {
        use std::io::{BufRead, BufReader};

        let file = match std::fs::File::open(log_path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        BufReader::new(file).lines().map_while(Result::ok).collect()
    }

    /// Filter log lines by timestamp range (legacy fallback for runs without per-run log files).
    ///
    /// Reads the log file and returns lines that fall within the given
    /// timestamp range (inclusive of start, exclusive of end).
    pub fn filter_logs_by_timestamp(
        log_path: &Path,
        started_at: chrono::DateTime<chrono::Utc>,
        finished_at: chrono::DateTime<chrono::Utc>,
    ) -> Vec<String> {
        use chrono::{DateTime, Utc};
        use std::io::{BufRead, BufReader};

        let file = match std::fs::File::open(log_path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut filtered = Vec::new();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            // Try to parse timestamp from the beginning of log line
            // Format: 2026-07-21T14:30:00.123456789Z ...
            if let Some(ts_end) = line.find(' ') {
                let ts_str = &line[..ts_end];
                if let Ok(ts) = ts_str.parse::<DateTime<Utc>>() {
                    if ts >= started_at && ts <= finished_at {
                        filtered.push(line);
                    }
                }
            }
        }

        filtered
    }

    /// Update the list viewport from the actual render area.
    pub(crate) fn set_list_viewport_height(&mut self, height: usize, history_len: usize) {
        self.list_viewport.set_height(height, history_len);
        self.clamp_history(history_len);
    }

    /// Update the log viewport from the actual render area.
    pub(crate) fn set_log_viewport_height(&mut self, height: usize) {
        self.log_viewport.set_height(height, self.log_lines.len());
    }

    /// Clamp selection and viewport after history is reloaded or shrinks.
    pub(crate) fn clamp_history(&mut self, history_len: usize) {
        if history_len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(history_len - 1);
        }
        self.list_viewport
            .ensure_visible(self.selected, history_len);
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent, history_len: usize) -> Action {
        self.clamp_history(history_len);
        match self.mode {
            Mode::LogView => self.handle_key_log_view(key),
            Mode::History => self.handle_key_history(key, history_len),
        }
    }

    /// Handle keys in log view mode.
    fn handle_key_log_view(&mut self, key: crossterm::event::KeyEvent) -> Action {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Esc => {
                self.exit_log_view();
                Action::Consumed
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.log_viewport.scroll_up(1);
                Action::Consumed
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.log_viewport.scroll_down(1, self.log_lines.len());
                Action::Consumed
            }
            KeyCode::PageUp => {
                self.log_viewport.scroll_up(self.log_viewport.page_size());
                Action::Consumed
            }
            KeyCode::PageDown => {
                self.log_viewport
                    .scroll_down(self.log_viewport.page_size(), self.log_lines.len());
                Action::Consumed
            }
            KeyCode::Home => {
                self.log_viewport.home();
                Action::Consumed
            }
            KeyCode::End => {
                self.log_viewport.end(self.log_lines.len());
                Action::Consumed
            }
            _ => Action::NotConsumed,
        }
    }

    /// Handle keys in history list mode.
    fn handle_key_history(
        &mut self,
        key: crossterm::event::KeyEvent,
        history_len: usize,
    ) -> Action {
        use crossterm::event::KeyCode;

        match key.code {
            // Enter log view mode.
            KeyCode::Enter => Action::ViewLogs,
            // Navigate history list.
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.list_viewport
                        .ensure_visible(self.selected, history_len);
                    Action::Consumed
                } else {
                    // At upper boundary — let parent handle focus return.
                    Action::NotConsumed
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if history_len > 0 && self.selected < history_len - 1 {
                    self.selected += 1;
                    self.list_viewport
                        .ensure_visible(self.selected, history_len);
                }
                Action::Consumed
            }
            KeyCode::Home => {
                self.selected = 0;
                self.list_viewport.home();
                Action::Consumed
            }
            KeyCode::End => {
                if history_len > 0 {
                    self.selected = history_len - 1;
                }
                self.list_viewport
                    .ensure_visible(self.selected, history_len);
                Action::Consumed
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(self.list_viewport.page_size());
                self.list_viewport
                    .ensure_visible(self.selected, history_len);
                Action::Consumed
            }
            KeyCode::PageDown => {
                if history_len > 0 {
                    self.selected = self
                        .selected
                        .saturating_add(self.list_viewport.page_size())
                        .min(history_len - 1);
                }
                self.list_viewport
                    .ensure_visible(self.selected, history_len);
                Action::Consumed
            }
            _ => Action::NotConsumed,
        }
    }

    /// Format a run record for display.
    pub fn format_entry(record: &RunRecord) -> EntryDisplay {
        let outcome_str = match record.outcome {
            RunOutcome::Success => "Success",
            RunOutcome::NoChanges => "No changes",
            RunOutcome::Failed => "Failed",
            RunOutcome::CommittedOffline => "Committed (offline)",
        };

        let duration = record.finished_at - record.started_at;
        let duration_str = if duration.num_seconds() < 1 {
            format!("{}ms", duration.num_milliseconds())
        } else {
            format!("{}s", duration.num_seconds())
        };

        let time_str = record.started_at.format("%Y-%m-%d %H:%M:%S").to_string();

        EntryDisplay {
            time: time_str,
            namespace: (!record.namespace.is_empty()).then(|| record.namespace.clone()),
            outcome: outcome_str.to_string(),
            duration: duration_str,
            commit: record.commit.clone(),
            message: record.message.clone(),
            is_error: record.outcome == RunOutcome::Failed,
            is_warning: record.outcome == RunOutcome::CommittedOffline,
        }
    }
}

/// Formatted display data for a single history entry.
#[derive(Debug, Clone)]
pub struct EntryDisplay {
    /// Formatted timestamp.
    pub time: String,
    /// Namespace recorded for this run, if available in legacy state.
    pub namespace: Option<String>,
    /// Human-readable outcome.
    pub outcome: String,
    /// Duration string.
    pub duration: String,
    /// Commit SHA (if any).
    pub commit: Option<String>,
    /// Error/warning message (if any).
    pub message: Option<String>,
    /// Whether this is an error entry.
    pub is_error: bool,
    /// Whether this is a warning entry.
    pub is_warning: bool,
}

/// Actions from the history screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Consumed,
    NotConsumed,
    /// Request to view logs for the selected entry.
    ViewLogs,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::io::Write;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn sample_record(outcome: RunOutcome) -> RunRecord {
        RunRecord {
            namespace: String::new(),
            started_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 3).unwrap(),
            outcome,
            commit: Some("abc123".to_string()),
            message: None,
            log_file: None,
        }
    }

    #[test]
    fn new_screen_starts_at_zero() {
        let screen = HistoryScreen::new();
        assert_eq!(screen.selected, 0);
        assert!(matches!(screen.mode, Mode::History));
    }

    #[test]
    fn navigate_up_down() {
        let mut screen = HistoryScreen::new();
        screen.handle_key(key(KeyCode::Down), 5);
        assert_eq!(screen.selected, 1);
        screen.handle_key(key(KeyCode::Down), 5);
        assert_eq!(screen.selected, 2);
        screen.handle_key(key(KeyCode::Up), 5);
        assert_eq!(screen.selected, 1);
    }

    #[test]
    fn does_not_go_past_bounds() {
        let mut screen = HistoryScreen::new();
        screen.handle_key(key(KeyCode::Up), 5);
        assert_eq!(screen.selected, 0);

        screen.selected = 4;
        screen.handle_key(key(KeyCode::Down), 5);
        assert_eq!(screen.selected, 4);
    }

    #[test]
    fn home_end_navigation() {
        let mut screen = HistoryScreen::new();
        screen.selected = 3;
        screen.handle_key(key(KeyCode::Home), 10);
        assert_eq!(screen.selected, 0);
        screen.handle_key(key(KeyCode::End), 10);
        assert_eq!(screen.selected, 9);
    }

    #[test]
    fn one_row_history_has_stable_selection_and_viewport() {
        let mut screen = HistoryScreen::new();
        screen.set_list_viewport_height(4, 1);
        screen.handle_key(key(KeyCode::End), 1);
        screen.handle_key(key(KeyCode::Down), 1);

        assert_eq!(screen.selected, 0);
        assert_eq!(screen.list_viewport.visible_range(1), 0..1);
    }

    #[test]
    fn long_list_navigation_keeps_selection_visible() {
        let mut screen = HistoryScreen::new();
        screen.set_list_viewport_height(3, 10);

        for _ in 0..5 {
            screen.handle_key(key(KeyCode::Down), 10);
        }

        assert_eq!(screen.selected, 5);
        assert_eq!(screen.list_viewport.visible_range(10), 3..6);
    }

    #[test]
    fn history_page_navigation_uses_rendered_viewport_height() {
        let mut screen = HistoryScreen::new();
        screen.set_list_viewport_height(4, 12);

        screen.handle_key(key(KeyCode::PageDown), 12);
        assert_eq!(screen.selected, 4);
        assert_eq!(screen.list_viewport.visible_range(12), 1..5);

        screen.handle_key(key(KeyCode::End), 12);
        assert_eq!(screen.selected, 11);
        assert_eq!(screen.list_viewport.visible_range(12), 8..12);

        screen.handle_key(key(KeyCode::PageUp), 12);
        assert_eq!(screen.selected, 7);
        assert_eq!(screen.list_viewport.visible_range(12), 7..11);
    }

    #[test]
    fn history_viewport_clamps_after_data_shrinks() {
        let mut screen = HistoryScreen::new();
        screen.set_list_viewport_height(3, 10);
        screen.selected = 9;
        screen.clamp_history(10);

        screen.clamp_history(2);

        assert_eq!(screen.selected, 1);
        assert_eq!(screen.list_viewport.visible_range(2), 0..2);
    }

    #[test]
    fn list_viewport_survives_tab_focus_escape() {
        let mut screen = HistoryScreen::new();
        screen.set_list_viewport_height(3, 10);
        screen.selected = 7;
        screen.clamp_history(10);
        let range = screen.list_viewport.visible_range(10);

        let action = screen.handle_key(key(KeyCode::Tab), 10);

        assert_eq!(action, Action::NotConsumed);
        assert_eq!(screen.list_viewport.visible_range(10), range);
    }

    #[test]
    fn enter_returns_view_logs_action() {
        let mut screen = HistoryScreen::new();
        let result = screen.handle_key(key(KeyCode::Enter), 5);
        assert_eq!(result, Action::ViewLogs);
    }

    #[test]
    fn log_view_mode_esc_exits() {
        let mut screen = HistoryScreen::new();
        screen.mode = Mode::LogView;
        screen.log_lines = vec!["line1".to_string()];
        screen.log_viewport.scroll_down(5, 10);

        let result = screen.handle_key(key(KeyCode::Esc), 5);

        assert_eq!(result, Action::Consumed);
        assert!(matches!(screen.mode, Mode::History));
        assert!(screen.log_lines.is_empty());
        assert_eq!(screen.log_viewport.offset(), 0);
    }

    #[test]
    fn log_view_scroll_up_down() {
        let mut screen = HistoryScreen::new();
        screen.mode = Mode::LogView;
        screen.log_lines = vec!["line1".to_string(), "line2".to_string()];
        screen.log_viewport.scroll_down(1, screen.log_lines.len());

        // Scroll up
        screen.handle_key(key(KeyCode::Up), 5);
        assert_eq!(screen.log_viewport.offset(), 0);

        // Scroll up at 0 stays at 0
        screen.handle_key(key(KeyCode::Up), 5);
        assert_eq!(screen.log_viewport.offset(), 0);

        // Scroll down
        screen.handle_key(key(KeyCode::Down), 5);
        assert_eq!(screen.log_viewport.offset(), 1);
    }

    #[test]
    fn log_view_page_up_down() {
        let mut screen = HistoryScreen::new();
        screen.mode = Mode::LogView;
        screen.log_lines = (0..50).map(|i| format!("line {i}")).collect();
        screen.set_log_viewport_height(10);
        screen.log_viewport.scroll_down(25, screen.log_lines.len());

        screen.handle_key(key(KeyCode::PageUp), 5);
        assert_eq!(screen.log_viewport.offset(), 15);

        screen.handle_key(key(KeyCode::PageDown), 5);
        assert_eq!(screen.log_viewport.offset(), 25);
    }

    #[test]
    fn log_view_home_resets_scroll() {
        let mut screen = HistoryScreen::new();
        screen.mode = Mode::LogView;
        screen.log_lines = (0..60).map(|i| format!("line {i}")).collect();
        screen.log_viewport.scroll_down(50, screen.log_lines.len());

        screen.handle_key(key(KeyCode::Home), 5);
        assert_eq!(screen.log_viewport.offset(), 0);
    }

    #[test]
    fn enter_log_view_clears_previous_lines() {
        let mut screen = HistoryScreen::new();
        screen.log_lines = vec!["old".to_string()];

        let record = sample_record(RunOutcome::Success);
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path();

        screen.enter_log_view(&record, state_dir);

        assert!(matches!(screen.mode, Mode::LogView));
        assert!(screen.log_lines.is_empty()); // No log file exists, so empty
        assert_eq!(screen.log_viewport.offset(), 0);
    }

    #[test]
    fn filter_logs_by_timestamp_extracts_matching_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let log_path = tmp.path().join("dothoard.log");
        let mut file = std::fs::File::create(&log_path).unwrap();

        // Write log lines with timestamps
        writeln!(file, "2026-07-21T14:30:01.123456789Z INFO before").unwrap();
        writeln!(file, "2026-07-21T14:30:02.123456789Z INFO during 1").unwrap();
        writeln!(file, "2026-07-21T14:30:02.500000000Z INFO during 2").unwrap();
        writeln!(file, "2026-07-21T14:30:03.123456789Z INFO at end").unwrap();
        writeln!(file, "2026-07-21T14:30:04.000000000Z INFO after").unwrap();
        writeln!(file, "not a timestamp line").unwrap();

        let start = Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 2).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 3).unwrap();

        let lines = HistoryScreen::filter_logs_by_timestamp(&log_path, start, end);

        // Lines with timestamps >= start (14:30:02) and <= end (14:30:03)
        // - 14:30:01 - before start (excluded)
        // - 14:30:02.123 - during 1 (included)
        // - 14:30:02.500 - during 2 (included)
        // - 14:30:03.123 - after end (excluded, 03.123 > 03.000)
        // - 14:30:04 - after end (excluded)
        // - not a timestamp (excluded)
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("during 1"));
        assert!(lines[1].contains("during 2"));
    }

    #[test]
    fn filter_logs_handles_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let log_path = tmp.path().join("nonexistent.log");

        let start = Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 10).unwrap();

        let lines = HistoryScreen::filter_logs_by_timestamp(&log_path, start, end);

        assert!(lines.is_empty());
    }

    #[test]
    fn format_entry_includes_namespace_identity() {
        let mut record = sample_record(RunOutcome::Success);
        record.namespace = "notebook".to_string();

        let display = HistoryScreen::format_entry(&record);

        assert_eq!(display.namespace.as_deref(), Some("notebook"));
    }

    #[test]
    fn log_view_retains_run_namespace_context() {
        let mut record = sample_record(RunOutcome::Success);
        record.namespace = "desktop".to_string();
        let tmp = tempfile::tempdir().unwrap();
        let mut screen = HistoryScreen::new();

        screen.enter_log_view(&record, tmp.path());

        assert_eq!(screen.log_namespace.as_deref(), Some("desktop"));
        screen.exit_log_view();
        assert!(screen.log_namespace.is_none());
    }

    #[test]
    fn format_success_entry() {
        let record = sample_record(RunOutcome::Success);
        let display = HistoryScreen::format_entry(&record);
        assert_eq!(display.outcome, "Success");
        assert_eq!(display.duration, "3s");
        assert!(!display.is_error);
        assert!(!display.is_warning);
        assert!(display.time.contains("2026-07-21"));
    }

    #[test]
    fn format_failed_entry() {
        let mut record = sample_record(RunOutcome::Failed);
        record.message = Some("network timeout".to_string());
        record.commit = None;
        let display = HistoryScreen::format_entry(&record);
        assert_eq!(display.outcome, "Failed");
        assert!(display.is_error);
        assert_eq!(display.message.as_deref(), Some("network timeout"));
    }

    #[test]
    fn format_offline_entry() {
        let record = sample_record(RunOutcome::CommittedOffline);
        let display = HistoryScreen::format_entry(&record);
        assert_eq!(display.outcome, "Committed (offline)");
        assert!(display.is_warning);
    }

    #[test]
    fn format_no_changes_entry() {
        let mut record = sample_record(RunOutcome::NoChanges);
        record.commit = None;
        let display = HistoryScreen::format_entry(&record);
        assert_eq!(display.outcome, "No changes");
    }

    #[test]
    fn format_sub_second_duration() {
        let mut record = sample_record(RunOutcome::Success);
        record.finished_at = record.started_at + chrono::Duration::milliseconds(450);
        let display = HistoryScreen::format_entry(&record);
        assert_eq!(display.duration, "450ms");
    }

    #[test]
    fn enter_log_view_reads_per_run_log_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path();

        // Create the logs directory and a per-run log file.
        let logs_dir = state_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let log_filename = "run-2026-07-21T14-30-00-000.log";
        let log_path = logs_dir.join(log_filename);
        std::fs::write(&log_path, "INFO backup started\nINFO mirror completed\n").unwrap();

        // Create a record with the log_file field set.
        let mut record = sample_record(RunOutcome::Success);
        record.log_file = Some(log_filename.to_string());

        let mut screen = HistoryScreen::new();
        screen.enter_log_view(&record, state_dir);

        assert!(matches!(screen.mode, Mode::LogView));
        assert_eq!(screen.log_lines.len(), 2);
        assert!(screen.log_lines[0].contains("backup started"));
        assert!(screen.log_lines[1].contains("mirror completed"));
    }

    #[test]
    fn enter_log_view_falls_back_to_timestamp_filter_when_no_log_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path();

        // Create the session log in the state directory.
        let session_log = state_dir.join("dothoard.log");
        let mut f = std::fs::File::create(&session_log).unwrap();
        writeln!(f, "2026-07-21T14:30:01.000000000Z INFO during run").unwrap();
        writeln!(f, "2026-07-21T14:31:00.000000000Z INFO after run").unwrap();

        // Record has no log_file — should use legacy timestamp filtering.
        let record = sample_record(RunOutcome::Success);
        assert!(record.log_file.is_none());

        let mut screen = HistoryScreen::new();
        screen.enter_log_view(&record, state_dir);

        assert!(matches!(screen.mode, Mode::LogView));
        // The first line (14:30:01) is within [14:30:00, 14:30:03].
        assert_eq!(screen.log_lines.len(), 1);
        assert!(screen.log_lines[0].contains("during run"));
    }

    #[test]
    fn read_log_file_returns_empty_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nonexistent.log");

        let lines = HistoryScreen::read_log_file(&missing);
        assert!(lines.is_empty());
    }
}
