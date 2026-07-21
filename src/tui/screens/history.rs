//! History and error details screen state.
//!
//! Displays recent backup runs from the persistent state, with details
//! for the selected entry including outcome, commit, duration, and messages.

use crate::state::{RunOutcome, RunRecord};

/// The state of the history screen.
#[derive(Debug)]
pub struct HistoryScreen {
    /// Index of the currently selected run in the history list.
    pub selected: usize,
    /// Scroll offset for long detail views.
    pub scroll: usize,
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
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent, history_len: usize) -> Action {
        use crossterm::event::KeyCode;

        match key.code {
            // Navigate history list.
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.scroll = 0;
                }
                Action::Consumed
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
