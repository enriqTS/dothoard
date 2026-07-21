//! Automation controls screen state.
//!
//! Provides install, remove, and status inspection of the systemd user timer.

/// The state of the automation controls screen.
#[derive(Debug)]
pub struct AutomationScreen {
    /// Cached automation status description.
    pub status_text: Option<String>,
    /// Whether the status needs to be refreshed.
    pub stale: bool,
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
            status_text: None,
            stale: true,
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
            KeyCode::Char('r') => {
                self.stale = true;
                Action::RefreshStatus
            }
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

    /// Refresh the automation status.
    pub fn refresh_status(&mut self, config: &crate::config::Config, home: &std::path::Path) {
        use crate::systemd;

        match systemd::params_from_config(config) {
            Ok(params) => {
                let unit_dir = systemd::user_unit_dir(home);
                match systemd::status(&params, &unit_dir) {
                    Ok(status) => {
                        self.status_text = Some(status.to_string());
                        self.stale = false;
                    }
                    Err(e) => {
                        self.status_text = Some(format!("error: {e}"));
                        self.stale = false;
                    }
                }
            }
            Err(e) => {
                self.status_text = Some(format!("error: {e}"));
                self.stale = false;
            }
        }
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
                        self.stale = true;
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
                self.stale = true;
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
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn new_screen_is_stale() {
        let screen = AutomationScreen::new();
        assert!(screen.stale);
        assert!(screen.status_text.is_none());
    }

    #[test]
    fn r_triggers_refresh() {
        let mut screen = AutomationScreen::new();
        screen.stale = false;
        let action = screen.handle_key(key(KeyCode::Char('r')));
        assert_eq!(action, Action::RefreshStatus);
        assert!(screen.stale);
    }

    #[test]
    fn i_prompts_install_confirmation() {
        let mut screen = AutomationScreen::new();
        let action = screen.handle_key(key(KeyCode::Char('i')));
        assert_eq!(action, Action::Consumed);
        assert_eq!(screen.confirm, ConfirmAction::Install);
    }

    #[test]
    fn x_prompts_remove_confirmation() {
        let mut screen = AutomationScreen::new();
        let action = screen.handle_key(key(KeyCode::Char('x')));
        assert_eq!(action, Action::Consumed);
        assert_eq!(screen.confirm, ConfirmAction::Remove);
    }

    #[test]
    fn confirm_y_installs() {
        let mut screen = AutomationScreen::new();
        screen.confirm = ConfirmAction::Install;
        let action = screen.handle_key(key(KeyCode::Char('y')));
        assert_eq!(action, Action::Install);
        assert_eq!(screen.confirm, ConfirmAction::None);
    }

    #[test]
    fn confirm_y_removes() {
        let mut screen = AutomationScreen::new();
        screen.confirm = ConfirmAction::Remove;
        let action = screen.handle_key(key(KeyCode::Char('y')));
        assert_eq!(action, Action::Remove);
        assert_eq!(screen.confirm, ConfirmAction::None);
    }

    #[test]
    fn confirm_n_cancels() {
        let mut screen = AutomationScreen::new();
        screen.confirm = ConfirmAction::Install;
        let action = screen.handle_key(key(KeyCode::Char('n')));
        assert_eq!(action, Action::Consumed);
        assert_eq!(screen.confirm, ConfirmAction::None);
    }

    #[test]
    fn confirm_esc_cancels() {
        let mut screen = AutomationScreen::new();
        screen.confirm = ConfirmAction::Remove;
        let action = screen.handle_key(key(KeyCode::Esc));
        assert_eq!(action, Action::Consumed);
        assert_eq!(screen.confirm, ConfirmAction::None);
    }
}
