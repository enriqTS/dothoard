//! Automation controls screen state.
//!
//! Provides install, remove, and status inspection of backup automation.

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
    /// Asking to install the selected automation backend.
    Install,
    /// Asking to remove the selected automation backend.
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
            // Select the next available backend.
            KeyCode::Char('b') => Action::SelectNextBackend,
            // Refresh status.
            KeyCode::Char('r') => Action::RefreshStatus,
            // Install automation.
            KeyCode::Char('i') => {
                self.confirm = ConfirmAction::Install;
                Action::Consumed
            }
            // Remove automation.
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
        paths: &crate::paths::AppPaths,
    ) -> Result<String, String> {
        crate::automation::status(config, paths)
            .map(|status| status.to_string())
            .map_err(|e| e.to_string())
    }

    /// Install the selected automation backend.
    pub fn install(&mut self, config: &crate::config::Config, paths: &crate::paths::AppPaths) {
        match crate::automation::install(config, paths) {
            Ok(()) => {
                self.message = Some(Message {
                    text: format!(
                        "Automation installed (every {} min).",
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

    /// Remove the selected automation backend.
    pub fn remove(&mut self, config: &crate::config::Config, paths: &crate::paths::AppPaths) {
        match crate::automation::remove(config, paths) {
            Ok(()) => {
                self.message = Some(Message {
                    text: "Automation removed.".to_string(),
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
    /// Persist selection of the next automation backend.
    SelectNextBackend,
    /// Refresh the status display.
    RefreshStatus,
    /// Install the selected automation backend (confirmed).
    Install,
    /// Remove the selected automation backend (confirmed).
    Remove,
}

#[cfg(test)]
#[path = "../../../tests/unit/tui/screens/automation.rs"]
mod tests;
