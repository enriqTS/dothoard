//! History and error details screen state.
//!
//! Displays recent backup runs from the persistent state, with details
//! for the selected entry including outcome, commit, duration, and messages.
//! Also provides a scrollable log viewer for viewing run logs.

use std::path::Path;

use crate::state::{RunOutcome, RunRecord};

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
    /// Scroll offset for long detail views.
    pub scroll: usize,
    /// Current display mode.
    pub mode: Mode,
    /// Cached log lines for the current log view.
    pub log_lines: Vec<String>,
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
            scroll: 0,
            mode: Mode::History,
            log_lines: Vec::new(),
        }
    }

    /// Enter log view mode with filtered log lines for the given record.
    pub fn enter_log_view(&mut self, record: &RunRecord, log_path: &Path) {
        self.log_lines =
            Self::filter_logs_by_timestamp(log_path, record.started_at, record.finished_at);
        self.mode = Mode::LogView;
        self.scroll = 0;
    }

    /// Exit log view mode and return to history list.
    pub fn exit_log_view(&mut self) {
        self.mode = Mode::History;
        self.log_lines.clear();
        self.scroll = 0;
    }

    /// Filter log lines by timestamp range.
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

    /// Handle a key event.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent, history_len: usize) -> Action {
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
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
                Action::Consumed
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // Allow scrolling past end - actual bounds checked during rendering
                self.scroll += 1;
                Action::Consumed
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                Action::Consumed
            }
            KeyCode::PageDown => {
                self.scroll += 10;
                Action::Consumed
            }
            KeyCode::Home => {
                self.scroll = 0;
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
                    self.scroll = 0;
                    Action::Consumed
                } else {
                    // At upper boundary — let parent handle focus return.
                    Action::NotConsumed
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if history_len > 0 && self.selected < history_len - 1 {
                    self.selected += 1;
                    self.scroll = 0;
                }
                Action::Consumed
            }
            KeyCode::Home => {
                self.selected = 0;
                self.scroll = 0;
                Action::Consumed
            }
            KeyCode::End => {
                if history_len > 0 {
                    self.selected = history_len - 1;
                }
                self.scroll = 0;
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
            started_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 3).unwrap(),
            outcome,
            commit: Some("abc123".to_string()),
            message: None,
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
        screen.scroll = 5;

        let result = screen.handle_key(key(KeyCode::Esc), 5);

        assert_eq!(result, Action::Consumed);
        assert!(matches!(screen.mode, Mode::History));
        assert!(screen.log_lines.is_empty());
        assert_eq!(screen.scroll, 0);
    }

    #[test]
    fn log_view_scroll_up_down() {
        let mut screen = HistoryScreen::new();
        screen.mode = Mode::LogView;
        screen.log_lines = vec!["line1".to_string(), "line2".to_string()];
        screen.scroll = 1;

        // Scroll up
        screen.handle_key(key(KeyCode::Up), 5);
        assert_eq!(screen.scroll, 0);

        // Scroll up at 0 stays at 0
        screen.handle_key(key(KeyCode::Up), 5);
        assert_eq!(screen.scroll, 0);

        // Scroll down
        screen.handle_key(key(KeyCode::Down), 5);
        assert_eq!(screen.scroll, 1);
    }

    #[test]
    fn log_view_page_up_down() {
        let mut screen = HistoryScreen::new();
        screen.mode = Mode::LogView;
        screen.scroll = 25;

        screen.handle_key(key(KeyCode::PageUp), 5);
        assert_eq!(screen.scroll, 15);

        screen.handle_key(key(KeyCode::PageDown), 5);
        assert_eq!(screen.scroll, 25);
    }

    #[test]
    fn log_view_home_resets_scroll() {
        let mut screen = HistoryScreen::new();
        screen.mode = Mode::LogView;
        screen.scroll = 50;

        screen.handle_key(key(KeyCode::Home), 5);
        assert_eq!(screen.scroll, 0);
    }

    #[test]
    fn enter_log_view_clears_previous_lines() {
        let mut screen = HistoryScreen::new();
        screen.log_lines = vec!["old".to_string()];

        let record = sample_record(RunOutcome::Success);
        let tmp = tempfile::tempdir().unwrap();
        let log_path = tmp.path().join("test.log");

        screen.enter_log_view(&record, &log_path);

        assert!(matches!(screen.mode, Mode::LogView));
        assert!(screen.log_lines.is_empty()); // File doesn't exist, so empty
        assert_eq!(screen.scroll, 0);
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
}
