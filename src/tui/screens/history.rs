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
                if let Ok(ts) = ts_str.parse::<DateTime<Utc>>()
                    && ts >= started_at
                    && ts <= finished_at
                {
                    filtered.push(line);
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
            KeyCode::Char('r') => Action::Refresh,
            // A log can be opened only for an existing run.
            KeyCode::Enter if history_len > 0 => Action::ViewLogs,
            KeyCode::Enter => Action::Consumed,
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
    /// Request to reload history from persistent state.
    Refresh,
}

#[cfg(test)]
#[path = "../../../tests/unit/tui/screens/history.rs"]
mod tests;
