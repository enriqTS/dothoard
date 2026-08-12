//! Automation controls screen state.
//!
//! Provides install, remove, and status inspection of the systemd user timer.

use crate::tui::task::LoadState;

/// The state of the automation controls screen.
#[derive(Debug)]
pub struct AutomationScreen {
    /// Lifecycle and last usable automation status description.
    pub status_state: LoadState<String>,
    /// Feedback message from the last operation.
    pub message: Option<Message>,
    /// Active confirmation dialog.
    pub confirm: ConfirmAction,
}

/// A feedback message.
#[derive(Debug, Clone)]
pub struct Message {
    pub text: String,
    pub success: bool,
}

/// Active confirmation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    None,
    /// Asking to install the timer.
    Install,
    /// Asking to remove the timer.
    Remove,
}

impl Default for AutomationScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl AutomationScreen {
    pub fn new() -> Self {
        Self {
            status_state: LoadState::NotLoaded,
            message: None,
            confirm: ConfirmAction::None,
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Action {
        use crossterm::event::KeyCode;

        // Handle confirmation dialogs.
        if self.confirm != ConfirmAction::None {
            return match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let action = match self.confirm {
                        ConfirmAction::Install => Action::Install,
                        ConfirmAction::Remove => Action::Remove,
                        ConfirmAction::None => Action::Consumed,
                    };
                    self.confirm = ConfirmAction::None;
                    action
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.confirm = ConfirmAction::None;
                    self.message = None;
                    Action::Consumed
                }
                _ => Action::Consumed,
            };
        }

        match key.code {
            // Refresh status.
            KeyCode::Char('r') => Action::RefreshStatus,
            // Install timer.
            KeyCode::Char('i') => {
                self.confirm = ConfirmAction::Install;
                Action::Consumed
            }
            // Remove timer.
            KeyCode::Char('x') => {
                self.confirm = ConfirmAction::Remove;
                Action::Consumed
            }
            _ => Action::NotConsumed,
        }
    }

    /// Inspect automation status on a background worker.
    pub fn inspect(
        config: &crate::config::Config,
        home: &std::path::Path,
    ) -> Result<String, String> {
        use crate::systemd;

        let params = systemd::params_from_config(config).map_err(|e| e.to_string())?;
        let unit_dir = systemd::user_unit_dir(home);
        systemd::status(&params, &unit_dir)
            .map(|status| status.to_string())
            .map_err(|e| e.to_string())
    }

    /// Install the timer.
    pub fn install(&mut self, config: &crate::config::Config, home: &std::path::Path) {
        use crate::systemd;

        match systemd::params_from_config(config) {
            Ok(params) => {
                let unit_dir = systemd::user_unit_dir(home);
                match systemd::install(&params, &unit_dir) {
                    Ok(()) => {
                        self.message = Some(Message {
                            text: format!(
                                "Timer installed (every {} min).",
                                config.interval_minutes
                            ),
                            success: true,
                        });
                    }
                    Err(e) => {
                        self.message = Some(Message {
                            text: format!("Install failed: {e}"),
                            success: false,
                        });
                    }
                }
            }
            Err(e) => {
                self.message = Some(Message {
                    text: format!("Cannot install: {e}"),
                    success: false,
                });
            }
        }
    }

    /// Remove the timer.
    pub fn remove(&mut self, home: &std::path::Path) {
        use crate::systemd;

        let unit_dir = systemd::user_unit_dir(home);
        match systemd::remove(&unit_dir) {
            Ok(()) => {
                self.message = Some(Message {
                    text: "Timer removed.".to_string(),
                    success: true,
                });
            }
            Err(e) => {
                self.message = Some(Message {
                    text: format!("Remove failed: {e}"),
                    success: false,
                });
            }
        }
    }
}

/// Actions from the automation screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Consumed,
    NotConsumed,
    /// Refresh the status display.
    RefreshStatus,
    /// Install the timer (confirmed).
    Install,
    /// Remove the timer (confirmed).
    Remove,
}

#[cfg(test)]
#[path = "../../../tests/unit/tui/screens/automation.rs"]
mod tests;
