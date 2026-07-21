//! Desktop failure and recovery notifications.
//!
//! Sends desktop notifications via `notify-send` when available. The
//! notification logic follows these rules:
//!
//! - **Failures**: Always notify when a backup fails.
//! - **Recovery**: Notify when a previously failing backup succeeds again.
//! - **Quiet success**: Successful scheduled runs produce no notification.
//! - **Tolerance**: If `notify-send` is not available, notifications are
//!   silently skipped — the TUI and persistent state still record everything.
//!
//! Notifications use the application name as the summary prefix and provide
//! actionable context in the body.

use std::process::Command;

use crate::app;
use crate::state::AppState;

/// The urgency level for a desktop notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// Normal information (recovery).
    Normal,
    /// Critical failure requiring attention.
    Critical,
}

/// Determine whether a notification should be sent based on the current run
/// outcome and the previous state.
///
/// Returns `Some((summary, body, urgency))` if a notification should be sent,
/// or `None` if the run should remain quiet.
pub fn decide_notification(
    success: bool,
    error_message: Option<&str>,
    previous_state: &AppState,
) -> Option<(String, String, Urgency)> {
    if success {
        // Only notify on recovery: previous state had an error, now it's fine.
        if previous_state.latest_error.is_some() {
            let summary = format!("{}: backup recovered", app::APP_NAME);
            let body = "Backup is working again after a previous failure.".to_string();
            Some((summary, body, Urgency::Normal))
        } else {
            // Quiet success — no notification.
            None
        }
    } else {
        // Failure: always notify.
        let summary = format!("{}: backup failed", app::APP_NAME);
        let body = error_message
            .unwrap_or("An unknown error occurred during backup.")
            .to_string();
        Some((summary, body, Urgency::Critical))
    }
}

/// Send a desktop notification via `notify-send`.
///
/// Returns `true` if the notification was sent successfully, `false` if
/// `notify-send` is unavailable or the command failed. Failures are logged
/// but never prevent the backup from completing.
pub fn send(summary: &str, body: &str, urgency: Urgency) -> bool {
    let urgency_str = match urgency {
        Urgency::Normal => "normal",
        Urgency::Critical => "critical",
    };

    let result = Command::new("notify-send")
        .args([
            "--app-name",
            app::APP_NAME,
            "--urgency",
            urgency_str,
            summary,
            body,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match result {
        Ok(status) => {
            if status.success() {
                tracing::debug!(summary = %summary, "notification sent");
                true
            } else {
                tracing::debug!(
                    code = status.code().unwrap_or(-1),
                    "notify-send exited with non-zero status"
                );
                false
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                tracing::debug!("notify-send not found; notifications disabled");
            } else {
                tracing::debug!(error = %e, "failed to run notify-send");
            }
            false
        }
    }
}

/// Convenience function: evaluate notification rules and send if appropriate.
///
/// Call this after persisting state. The `previous_state` should be the state
/// as it was *before* recording the current run.
pub fn notify_if_needed(
    success: bool,
    error_message: Option<&str>,
    previous_state: &AppState,
) -> bool {
    if let Some((summary, body, urgency)) = decide_notification(success, error_message, previous_state) {
        send(&summary, &body, urgency)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[test]
    fn success_after_clean_state_is_quiet() {
        let state = AppState::new();

        let result = decide_notification(true, None, &state);

        assert_eq!(result, None);
    }

    #[test]
    fn success_after_failure_triggers_recovery() {
        let mut state = AppState::new();
        state.latest_error = Some("previous failure".to_string());

        let result = decide_notification(true, None, &state);

        assert!(result.is_some());
        let (summary, body, urgency) = result.unwrap();
        assert!(summary.contains("recovered"));
        assert!(body.contains("working again"));
        assert_eq!(urgency, Urgency::Normal);
    }

    #[test]
    fn failure_always_notifies() {
        let state = AppState::new();

        let result = decide_notification(false, Some("source not found"), &state);

        assert!(result.is_some());
        let (summary, body, urgency) = result.unwrap();
        assert!(summary.contains("failed"));
        assert!(body.contains("source not found"));
        assert_eq!(urgency, Urgency::Critical);
    }

    #[test]
    fn failure_with_no_message_uses_generic_text() {
        let state = AppState::new();

        let result = decide_notification(false, None, &state);

        assert!(result.is_some());
        let (_summary, body, _urgency) = result.unwrap();
        assert!(body.contains("unknown error"));
    }

    #[test]
    fn repeated_failure_still_notifies() {
        let mut state = AppState::new();
        state.latest_error = Some("old error".to_string());

        let result = decide_notification(false, Some("new error"), &state);

        assert!(result.is_some());
        let (_summary, body, urgency) = result.unwrap();
        assert!(body.contains("new error"));
        assert_eq!(urgency, Urgency::Critical);
    }

    #[test]
    fn success_after_success_is_quiet() {
        let mut state = AppState::new();
        // No latest_error means last run succeeded.
        state.latest_error = None;

        let result = decide_notification(true, None, &state);

        assert_eq!(result, None);
    }

    #[test]
    fn urgency_values() {
        assert_eq!(Urgency::Normal, Urgency::Normal);
        assert_ne!(Urgency::Normal, Urgency::Critical);
    }
}
