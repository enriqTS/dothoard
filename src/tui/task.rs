//! Nonblocking background task execution for the TUI.
//!
//! Long-running backend operations (backup, check) run in a background thread
//! and communicate results back to the main event loop via a channel. This
//! prevents the UI from freezing during I/O-heavy operations.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// The result of a background task, sent back to the UI thread.
#[derive(Debug)]
pub enum TaskResult {
    /// A backup operation completed.
    Backup(BackupResult),
    /// A check operation completed.
    Check(CheckResult),
}

/// Outcome of a background backup.
#[derive(Debug, Clone)]
pub struct BackupResult {
    pub success: bool,
    pub commit: Option<String>,
    pub pushed: bool,
    pub copies: usize,
    pub deletions: usize,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// Outcome of a background check.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub healthy: bool,
    pub results: Vec<CheckItem>,
}

/// A single check result item for display.
#[derive(Debug, Clone)]
pub struct CheckItem {
    pub label: String,
    pub status: CheckItemStatus,
    pub detail: Option<String>,
}

/// Status of an individual check item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckItemStatus {
    Ok,
    Warning,
    Error,
}

/// Identifies which background task is currently running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Backup,
    Check,
}

/// Manages background task spawning and result collection.
pub struct TaskManager {
    /// Channel receiver for completed task results.
    receiver: Receiver<TaskResult>,
    /// Channel sender cloned into spawned threads.
    pub(crate) sender: Sender<TaskResult>,
    /// Which task is currently running, if any.
    pub(crate) active: Option<TaskKind>,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    /// Create a new task manager.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            receiver,
            sender,
            active: None,
        }
    }

    /// Whether a background task is currently running.
    pub fn is_busy(&self) -> bool {
        self.active.is_some()
    }

    /// Which task is currently running.
    pub fn active_task(&self) -> Option<TaskKind> {
        self.active
    }

    /// Poll for a completed task result without blocking.
    ///
    /// Returns `Some(result)` if a task completed since the last poll,
    /// and clears the active task state. Returns `None` if no result
    /// is available yet.
    pub fn poll(&mut self) -> Option<TaskResult> {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.active = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                // The sender was dropped (thread panicked or finished without sending).
                self.active = None;
                None
            }
        }
    }

    /// Spawn a backup operation in the background.
    ///
    /// Returns `false` if a task is already running.
    pub fn spawn_backup(&mut self, paths: crate::paths::AppPaths) -> bool {
        if self.is_busy() {
            return false;
        }
        self.active = Some(TaskKind::Backup);
        let sender = self.sender.clone();

        thread::spawn(move || {
            let result = run_backup_task(&paths);
            // Ignore send error — the receiver may have been dropped if
            // the user quit while the task was running.
            let _ = sender.send(TaskResult::Backup(result));
        });

        true
    }

    /// Spawn a check operation in the background.
    ///
    /// Returns `false` if a task is already running.
    pub fn spawn_check(&mut self, paths: crate::paths::AppPaths) -> bool {
        if self.is_busy() {
            return false;
        }
        self.active = Some(TaskKind::Check);
        let sender = self.sender.clone();

        thread::spawn(move || {
            let result = run_check_task(&paths);
            let _ = sender.send(TaskResult::Check(result));
        });

        true
    }
}

/// Execute the backup workflow on the background thread.
fn run_backup_task(paths: &crate::paths::AppPaths) -> BackupResult {
    use crate::backup::coordinator;

    match coordinator::run_backup(paths) {
        Ok(outcome) => BackupResult {
            success: outcome.success,
            commit: outcome.commit,
            pushed: outcome.pushed,
            copies: outcome.copies,
            deletions: outcome.deletions,
            warnings: outcome.warnings,
            error: outcome.error,
        },
        Err(e) => BackupResult {
            success: false,
            commit: None,
            pushed: false,
            copies: 0,
            deletions: 0,
            warnings: Vec::new(),
            error: Some(format!("{e:#}")),
        },
    }
}

/// Execute the check workflow on the background thread.
fn run_check_task(paths: &crate::paths::AppPaths) -> CheckResult {
    use crate::backup::check;

    let report = check::run_check(paths);
    let results = report
        .results
        .iter()
        .map(|r| CheckItem {
            label: r.label.clone(),
            status: match &r.status {
                check::CheckStatus::Ok => CheckItemStatus::Ok,
                check::CheckStatus::Warning(_) => CheckItemStatus::Warning,
                check::CheckStatus::Error(_) => CheckItemStatus::Error,
            },
            detail: match &r.status {
                check::CheckStatus::Ok => None,
                check::CheckStatus::Warning(msg) => Some(msg.clone()),
                check::CheckStatus::Error(msg) => Some(msg.clone()),
            },
        })
        .collect();

    CheckResult {
        healthy: report.is_healthy(),
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_manager_is_not_busy() {
        let tm = TaskManager::new();
        assert!(!tm.is_busy());
        assert_eq!(tm.active_task(), None);
    }

    #[test]
    fn poll_returns_none_when_empty() {
        let mut tm = TaskManager::new();
        assert!(tm.poll().is_none());
    }

    #[test]
    fn cannot_spawn_when_busy() {
        let mut tm = TaskManager::new();
        // Manually set active to simulate a running task.
        tm.active = Some(TaskKind::Backup);

        // Cannot spawn another task while one is active.
        let paths = unsafe_test_paths();
        assert!(!tm.spawn_backup(paths.clone()));
        assert!(!tm.spawn_check(paths));
    }

    #[test]
    fn direct_channel_send_receive() {
        // Test the channel mechanism directly without spawning real tasks.
        let mut tm = TaskManager::new();
        tm.active = Some(TaskKind::Check);

        // Simulate a task completing by sending directly on the channel.
        let sender = tm.sender.clone();
        sender
            .send(TaskResult::Check(CheckResult {
                healthy: true,
                results: vec![CheckItem {
                    label: "test".to_string(),
                    status: CheckItemStatus::Ok,
                    detail: Some("all good".to_string()),
                }],
            }))
            .unwrap();

        let result = tm.poll();
        assert!(result.is_some());
        assert!(!tm.is_busy());

        match result.unwrap() {
            TaskResult::Check(cr) => {
                assert!(cr.healthy);
                assert_eq!(cr.results.len(), 1);
            }
            _ => panic!("expected Check result"),
        }
    }

    #[test]
    fn poll_clears_active_state() {
        let mut tm = TaskManager::new();
        tm.active = Some(TaskKind::Backup);

        let sender = tm.sender.clone();
        sender
            .send(TaskResult::Backup(BackupResult {
                success: true,
                commit: Some("abc123".to_string()),
                pushed: true,
                copies: 5,
                deletions: 1,
                warnings: Vec::new(),
                error: None,
            }))
            .unwrap();

        assert!(tm.is_busy());
        let _ = tm.poll();
        assert!(!tm.is_busy());
        assert_eq!(tm.active_task(), None);
    }

    /// Create AppPaths suitable for tests that won't actually run tasks.
    /// This is only used to test spawn rejection logic — the paths won't be
    /// accessed because we verify spawn is rejected when busy.
    fn unsafe_test_paths() -> crate::paths::AppPaths {
        // Create a temporary directory structure for path resolution.
        let tmp = std::env::temp_dir().join("dothoard-task-test");
        let _ = std::fs::create_dir_all(&tmp);
        let config_dir = tmp.join("config");
        let _ = std::fs::create_dir_all(&config_dir);
        let state_dir = tmp.join("state");
        let _ = std::fs::create_dir_all(&state_dir);
        let runtime_dir = tmp.join("runtime");
        let _ = std::fs::create_dir_all(&runtime_dir);

        let inputs = crate::paths::PathInputs {
            home: Some(tmp.clone()),
            config_dir: Some(config_dir),
            state_dir: Some(state_dir),
            runtime_dir: Some(runtime_dir),
            use_environment: false,
        };

        crate::paths::AppPaths::resolve(inputs).unwrap_or_else(|_| {
            // Fallback — construct manually if resolution fails.
            // This shouldn't happen with the dirs we created above.
            panic!("failed to create test AppPaths");
        })
    }
}
