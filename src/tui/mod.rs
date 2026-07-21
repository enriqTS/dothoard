//! Ratatui application and screens.
//!
//! This module provides the terminal user interface for configuring and
//! monitoring dothoard. It depends on backend services but the backend
//! never depends on TUI code.

mod event;
pub mod task;
mod terminal;
mod ui;

pub use terminal::run;

/// The screens available in the TUI, corresponding to tab navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Sources,
    Ignore,
    Preview,
    Automation,
    History,
}

impl Screen {
    /// All screens in tab order.
    pub const ALL: &'static [Screen] = &[
        Screen::Dashboard,
        Screen::Sources,
        Screen::Ignore,
        Screen::Preview,
        Screen::Automation,
        Screen::History,
    ];

    /// Human-readable label for the tab bar.
    pub fn label(self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Sources => "Sources",
            Screen::Ignore => "Ignore",
            Screen::Preview => "Preview",
            Screen::Automation => "Automation",
            Screen::History => "History",
        }
    }

    /// Move to the next screen (wraps around).
    pub fn next(self) -> Screen {
        let all = Self::ALL;
        let idx = all.iter().position(|&s| s == self).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    /// Move to the previous screen (wraps around).
    pub fn prev(self) -> Screen {
        let all = Self::ALL;
        let idx = all.iter().position(|&s| s == self).unwrap_or(0);
        all[(idx + all.len() - 1) % all.len()]
    }
}

/// Top-level TUI application state.
pub struct App {
    /// The currently active screen/tab.
    pub active_screen: Screen,
    /// Whether the user has requested to quit.
    pub should_quit: bool,
    /// Background task manager for nonblocking backend operations.
    pub tasks: task::TaskManager,
    /// Last backup result received from the background thread.
    pub last_backup: Option<task::BackupResult>,
    /// Last check result received from the background thread.
    pub last_check: Option<task::CheckResult>,
    /// Resolved application paths (populated on startup if available).
    pub paths: Option<crate::paths::AppPaths>,
    /// Status message displayed temporarily in the help bar.
    pub status_message: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let paths = crate::paths::AppPaths::from_environment().ok();
        Self {
            active_screen: Screen::Dashboard,
            should_quit: false,
            tasks: task::TaskManager::new(),
            last_backup: None,
            last_check: None,
            paths,
            status_message: None,
        }
    }

    /// Poll for completed background tasks and update state.
    pub fn poll_tasks(&mut self) {
        if let Some(result) = self.tasks.poll() {
            match result {
                task::TaskResult::Backup(r) => {
                    self.status_message = if r.success {
                        Some("Backup completed successfully.".to_string())
                    } else {
                        Some(format!(
                            "Backup failed: {}",
                            r.error.as_deref().unwrap_or("unknown error")
                        ))
                    };
                    self.last_backup = Some(r);
                }
                task::TaskResult::Check(r) => {
                    self.status_message = if r.healthy {
                        Some("All checks passed.".to_string())
                    } else {
                        Some("Some checks reported issues.".to_string())
                    };
                    self.last_check = Some(r);
                }
            }
        }
    }

    /// Handle a key event and update application state.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.modifiers, key.code) {
            // Quit: q, Ctrl+C, or Esc
            (_, KeyCode::Char('q')) => self.should_quit = true,
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => self.should_quit = true,
            (_, KeyCode::Esc) => self.should_quit = true,

            // Tab navigation: Tab/Shift+Tab or number keys
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.active_screen = self.active_screen.next();
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.active_screen = self.active_screen.prev();
            }

            // Direct screen selection via number keys
            (_, KeyCode::Char('1')) => self.active_screen = Screen::Dashboard,
            (_, KeyCode::Char('2')) => self.active_screen = Screen::Sources,
            (_, KeyCode::Char('3')) => self.active_screen = Screen::Ignore,
            (_, KeyCode::Char('4')) => self.active_screen = Screen::Preview,
            (_, KeyCode::Char('5')) => self.active_screen = Screen::Automation,
            (_, KeyCode::Char('6')) => self.active_screen = Screen::History,

            // Trigger backup with 'b' (only from Dashboard)
            (_, KeyCode::Char('b'))
                if self.active_screen == Screen::Dashboard && !self.tasks.is_busy() =>
            {
                if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_backup(paths.clone()) {
                        self.status_message = Some("Running backup...".to_string());
                    }
                } else {
                    self.status_message =
                        Some("Cannot run backup: paths not resolved.".to_string());
                }
            }

            // Trigger check with 'c' (only from Dashboard)
            (_, KeyCode::Char('c'))
                if self.active_screen == Screen::Dashboard && !self.tasks.is_busy() =>
            {
                if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_check(paths.clone()) {
                        self.status_message = Some("Running check...".to_string());
                    }
                } else {
                    self.status_message = Some("Cannot run check: paths not resolved.".to_string());
                }
            }

            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a minimal App for testing navigation and keys.
    fn test_app() -> App {
        App {
            active_screen: Screen::Dashboard,
            should_quit: false,
            tasks: task::TaskManager::new(),
            last_backup: None,
            last_check: None,
            paths: None,
            status_message: None,
        }
    }

    #[test]
    fn screen_next_wraps_around() {
        assert_eq!(Screen::Dashboard.next(), Screen::Sources);
        assert_eq!(Screen::History.next(), Screen::Dashboard);
    }

    #[test]
    fn screen_prev_wraps_around() {
        assert_eq!(Screen::Dashboard.prev(), Screen::History);
        assert_eq!(Screen::Sources.prev(), Screen::Dashboard);
    }

    #[test]
    fn all_screens_have_labels() {
        for screen in Screen::ALL {
            assert!(!screen.label().is_empty());
        }
    }

    #[test]
    fn app_starts_on_dashboard() {
        let app = test_app();
        assert_eq!(app.active_screen, Screen::Dashboard);
        assert!(!app.should_quit);
    }

    #[test]
    fn quit_on_q() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn quit_on_ctrl_c() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn quit_on_esc() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn tab_navigates_forward() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_screen, Screen::Sources);
    }

    #[test]
    fn shift_tab_navigates_backward() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.active_screen, Screen::History);
    }

    #[test]
    fn number_keys_select_screens() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();

        app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert_eq!(app.active_screen, Screen::Ignore);

        app.handle_key(KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE));
        assert_eq!(app.active_screen, Screen::History);

        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(app.active_screen, Screen::Dashboard);
    }

    #[test]
    fn backup_key_sets_status_when_no_paths() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        // paths is None, so backup should set an error message.
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert!(
            app.status_message
                .as_ref()
                .unwrap()
                .contains("not resolved")
        );
    }

    #[test]
    fn check_key_sets_status_when_no_paths() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(
            app.status_message
                .as_ref()
                .unwrap()
                .contains("not resolved")
        );
    }

    #[test]
    fn poll_tasks_updates_last_backup() {
        let mut app = test_app();
        app.tasks.active = Some(task::TaskKind::Backup);

        // Send a result directly on the channel.
        app.tasks
            .sender
            .send(task::TaskResult::Backup(task::BackupResult {
                success: true,
                commit: Some("deadbeef".to_string()),
                pushed: true,
                copies: 3,
                deletions: 0,
                warnings: Vec::new(),
                error: None,
            }))
            .unwrap();

        app.poll_tasks();

        assert!(app.last_backup.is_some());
        let result = app.last_backup.as_ref().unwrap();
        assert!(result.success);
        assert_eq!(result.commit.as_deref(), Some("deadbeef"));
        assert!(app.status_message.as_ref().unwrap().contains("success"));
    }

    #[test]
    fn poll_tasks_updates_last_check() {
        let mut app = test_app();
        app.tasks.active = Some(task::TaskKind::Check);

        app.tasks
            .sender
            .send(task::TaskResult::Check(task::CheckResult {
                healthy: false,
                results: vec![task::CheckItem {
                    label: "config".to_string(),
                    status: task::CheckItemStatus::Error,
                    detail: Some("missing".to_string()),
                }],
            }))
            .unwrap();

        app.poll_tasks();

        assert!(app.last_check.is_some());
        let result = app.last_check.as_ref().unwrap();
        assert!(!result.healthy);
        assert!(app.status_message.as_ref().unwrap().contains("issues"));
    }
}
