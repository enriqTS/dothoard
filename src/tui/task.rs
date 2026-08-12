//! Nonblocking background task execution for the TUI.
//!
//! Long-running backend operations (backup, check) run in a background thread
//! and communicate results back to the main event loop via a channel. This
//! prevents the UI from freezing during I/O-heavy operations.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// Monotonic identity for a screen-data request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

impl RequestId {
    #[cfg(test)]
    pub(crate) fn for_test(value: u64) -> Self {
        Self(value)
    }
}

/// Typed lifecycle for data loaded by a TUI background task.
#[derive(Debug)]
pub enum LoadState<T> {
    NotLoaded,
    Loading {
        request_id: RequestId,
        previous: Option<T>,
    },
    Loaded(T),
    Stale {
        previous: Option<T>,
    },
    Failed {
        error: String,
        previous: Option<T>,
    },
}

impl<T> LoadState<T> {
    pub fn data(&self) -> Option<&T> {
        match self {
            Self::Loading { previous, .. }
            | Self::Stale { previous }
            | Self::Failed { previous, .. } => previous.as_ref(),
            Self::Loaded(data) => Some(data),
            Self::NotLoaded => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed { error, .. } => Some(error),
            _ => None,
        }
    }

    pub fn loading_id(&self) -> Option<RequestId> {
        match self {
            Self::Loading { request_id, .. } => Some(*request_id),
            _ => None,
        }
    }

    pub fn begin(&mut self, request_id: RequestId, preserve_previous: bool) {
        let previous = if preserve_previous {
            match std::mem::replace(self, Self::NotLoaded) {
                Self::Loaded(data) => Some(data),
                Self::Loading { previous, .. }
                | Self::Stale { previous }
                | Self::Failed { previous, .. } => previous,
                Self::NotLoaded => None,
            }
        } else {
            None
        };
        *self = Self::Loading {
            request_id,
            previous,
        };
    }

    /// Complete only the currently expected request.
    pub fn finish(&mut self, request_id: RequestId, result: Result<T, String>) -> bool {
        if self.loading_id() != Some(request_id) {
            return false;
        }
        let previous = match std::mem::replace(self, Self::NotLoaded) {
            Self::Loading { previous, .. } => previous,
            _ => unreachable!("loading request checked above"),
        };
        *self = match result {
            Ok(data) => Self::Loaded(data),
            Err(error) => Self::Failed { error, previous },
        };
        true
    }

    /// Invalidate current work while retaining the last usable data.
    pub fn invalidate(&mut self) {
        let previous = match std::mem::replace(self, Self::NotLoaded) {
            Self::Loaded(data) => Some(data),
            Self::Loading { previous, .. }
            | Self::Stale { previous }
            | Self::Failed { previous, .. } => previous,
            Self::NotLoaded => None,
        };
        *self = Self::Stale { previous };
    }

    pub fn fail(&mut self, error: String, preserve_previous: bool) {
        let previous = if preserve_previous {
            match std::mem::replace(self, Self::NotLoaded) {
                Self::Loaded(data) => Some(data),
                Self::Loading { previous, .. }
                | Self::Stale { previous }
                | Self::Failed { previous, .. } => previous,
                Self::NotLoaded => None,
            }
        } else {
            None
        };
        *self = Self::Failed { error, previous };
    }

    pub fn reset(&mut self) {
        *self = Self::NotLoaded;
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading { .. })
    }
}

/// The result of a background task, sent back to the UI thread.
#[derive(Debug)]
pub enum TaskResult {
    /// A backup operation completed.
    Backup(BackupResult),
    /// A check operation completed.
    Check(CheckResult),
    /// A push operation completed.
    Push(PushResult),
    RepositoryValidation {
        request_id: RequestId,
        result: Result<crate::tui::screens::repository::RepoInfo, String>,
    },
    BackupPreview {
        request_id: RequestId,
        result: Result<crate::tui::screens::preview::PreviewData, String>,
    },
    IgnorePreview {
        request_id: RequestId,
        source_idx: usize,
        result: Result<Vec<crate::tui::screens::ignore::PreviewEntry>, String>,
    },
    AutomationInspection {
        request_id: RequestId,
        result: Result<String, String>,
    },
}

impl TaskResult {
    fn load_identity(&self) -> Option<(LoadTaskKind, RequestId)> {
        match self {
            Self::RepositoryValidation { request_id, .. } => {
                Some((LoadTaskKind::RepositoryValidation, *request_id))
            }
            Self::BackupPreview { request_id, .. } => {
                Some((LoadTaskKind::BackupPreview, *request_id))
            }
            Self::IgnorePreview { request_id, .. } => {
                Some((LoadTaskKind::IgnorePreview, *request_id))
            }
            Self::AutomationInspection { request_id, .. } => {
                Some((LoadTaskKind::AutomationInspection, *request_id))
            }
            Self::Backup(_) | Self::Check(_) | Self::Push(_) => None,
        }
    }
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

/// Outcome of a background push.
#[derive(Debug, Clone)]
pub struct PushResult {
    pub success: bool,
    pub error: Option<String>,
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

/// Identifies which user operation is currently running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Backup,
    Check,
    Push,
}

/// Identifies independently loadable screen data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoadTaskKind {
    RepositoryValidation,
    BackupPreview,
    IgnorePreview,
    AutomationInspection,
}

/// Manages background task spawning and result collection.
pub struct TaskManager {
    /// Channel receiver for completed task results.
    receiver: Receiver<TaskResult>,
    /// Channel sender cloned into spawned threads.
    pub(crate) sender: Sender<TaskResult>,
    /// Which user operation is currently running, if any.
    pub(crate) active: Option<TaskKind>,
    /// Active screen-data request per logical resource.
    active_loads: HashMap<LoadTaskKind, RequestId>,
    next_request_id: u64,
    spawn_workers: bool,
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
            active_loads: HashMap::new(),
            next_request_id: 1,
            spawn_workers: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_controlled() -> Self {
        let mut manager = Self::new();
        manager.spawn_workers = false;
        manager
    }

    /// Whether a background task is currently running.
    pub fn is_busy(&self) -> bool {
        self.active.is_some()
    }

    /// Which task is currently running.
    pub fn active_task(&self) -> Option<TaskKind> {
        self.active
    }

    pub fn is_load_active(&self, kind: LoadTaskKind) -> bool {
        self.active_loads.contains_key(&kind)
    }

    /// Forget an invalidated request so a replacement may start immediately.
    pub fn invalidate_load(&mut self, kind: LoadTaskKind) {
        self.active_loads.remove(&kind);
    }

    fn begin_load(&mut self, kind: LoadTaskKind) -> Option<RequestId> {
        if self.active_loads.contains_key(&kind) {
            return None;
        }
        let request_id = RequestId(self.next_request_id);
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.active_loads.insert(kind, request_id);
        Some(request_id)
    }

    /// Poll for a completed task result without blocking.
    ///
    /// Returns `Some(result)` if a task completed since the last poll,
    /// and clears the active task state. Returns `None` if no result
    /// is available yet.
    pub fn poll(&mut self) -> Option<TaskResult> {
        match self.receiver.try_recv() {
            Ok(result) => {
                match result.load_identity() {
                    Some((kind, request_id)) => {
                        if self.active_loads.get(&kind) == Some(&request_id) {
                            self.active_loads.remove(&kind);
                        }
                    }
                    None => self.active = None,
                }
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

    pub fn spawn_repository_validation(
        &mut self,
        input: String,
        home: PathBuf,
        namespace: String,
        remote: String,
        timeout_seconds: u32,
    ) -> Option<RequestId> {
        let request_id = self.begin_load(LoadTaskKind::RepositoryValidation)?;
        if !self.spawn_workers {
            return Some(request_id);
        }
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = crate::tui::screens::repository::RepoScreen::validate_path(
                &input,
                &home,
                &namespace,
                &remote,
                timeout_seconds,
            );
            let _ = sender.send(TaskResult::RepositoryValidation { request_id, result });
        });
        Some(request_id)
    }

    pub fn spawn_backup_preview(
        &mut self,
        config: crate::config::Config,
        home: PathBuf,
        repository: PathBuf,
    ) -> Option<RequestId> {
        let request_id = self.begin_load(LoadTaskKind::BackupPreview)?;
        if !self.spawn_workers {
            return Some(request_id);
        }
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result =
                crate::tui::screens::preview::PreviewScreen::generate(&config, &home, &repository);
            let _ = sender.send(TaskResult::BackupPreview { request_id, result });
        });
        Some(request_id)
    }

    pub fn spawn_ignore_preview(
        &mut self,
        source_idx: usize,
        source_path: String,
        patterns: Vec<String>,
        home: PathBuf,
    ) -> Option<RequestId> {
        let request_id = self.begin_load(LoadTaskKind::IgnorePreview)?;
        if !self.spawn_workers {
            return Some(request_id);
        }
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = Ok(crate::tui::screens::ignore::IgnoreScreen::generate_preview(
                &source_path,
                &patterns,
                &home,
            ));
            let _ = sender.send(TaskResult::IgnorePreview {
                request_id,
                source_idx,
                result,
            });
        });
        Some(request_id)
    }

    pub fn spawn_automation_inspection(
        &mut self,
        config: crate::config::Config,
        home: PathBuf,
    ) -> Option<RequestId> {
        let request_id = self.begin_load(LoadTaskKind::AutomationInspection)?;
        if !self.spawn_workers {
            return Some(request_id);
        }
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = crate::tui::screens::automation::AutomationScreen::inspect(&config, &home);
            let _ = sender.send(TaskResult::AutomationInspection { request_id, result });
        });
        Some(request_id)
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

    /// Spawn a push-only operation in the background.
    ///
    /// This performs a pull-with-rebase and push without running the full
    /// backup workflow. Useful for retrying after a previous push failure.
    ///
    /// Returns `false` if a task is already running.
    pub fn spawn_push(&mut self, paths: crate::paths::AppPaths) -> bool {
        if self.is_busy() {
            return false;
        }
        self.active = Some(TaskKind::Push);
        let sender = self.sender.clone();

        thread::spawn(move || {
            let result = run_push_task(&paths);
            let _ = sender.send(TaskResult::Push(result));
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

/// Execute a push-only sync on the background thread.
///
/// Loads config, validates the repository, and runs pull+push without
/// performing any backup/copy operations.
fn run_push_task(paths: &crate::paths::AppPaths) -> PushResult {
    use crate::config::Config;
    use crate::git;
    use crate::state::{AppState, RunOutcome, RunRecord};
    use std::time::Duration;

    let started_at = chrono::Utc::now();
    tracing::info!("starting push task");

    // Load config.
    let config = match Config::load(paths.config_file()) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "push: failed to load config");
            let finished_at = chrono::Utc::now();
            record_push_outcome(
                paths,
                "",
                started_at,
                finished_at,
                RunOutcome::Failed,
                Some(format!("failed to load config: {e}")),
            );
            return PushResult {
                success: false,
                error: Some(format!("failed to load config: {e}")),
            };
        }
    };

    let repo_path = config.repository_path(paths.home());

    // Validate repository state.
    let runner = git::GitRunner::new(Duration::from_secs(u64::from(
        config.network_timeout_seconds,
    )));

    let repo_info = match git::validate_repository(&runner, &repo_path, &config.remote) {
        Ok(info) => info,
        Err(e) => {
            tracing::error!(error = %e, "push: repository validation failed");
            let finished_at = chrono::Utc::now();
            record_push_outcome(
                paths,
                &config.namespace,
                started_at,
                finished_at,
                RunOutcome::Failed,
                Some(format!("repository error: {e}")),
            );
            return PushResult {
                success: false,
                error: Some(format!("repository error: {e}")),
            };
        }
    };

    tracing::info!(
        worktree = %repo_info.worktree.display(),
        remote = %config.remote,
        branch = %repo_info.branch,
        "push: syncing with remote"
    );

    // Run sync (pull with rebase + push).
    match git::sync_with_remote(
        &runner,
        &repo_info.worktree,
        &config.remote,
        &repo_info.branch,
    ) {
        Ok(sync_result) => {
            tracing::info!(result = ?sync_result, "push: sync succeeded");
            let finished_at = chrono::Utc::now();
            // Update state to clear pending_push and record success.
            let mut state = AppState::load(paths.state_dir()).unwrap_or_default();
            state.pending_push = false;
            state.last_push = Some(finished_at);
            state.record_run(RunRecord {
                namespace: config.namespace.clone(),
                started_at,
                finished_at,
                outcome: RunOutcome::Success,
                commit: None,
                message: Some("push completed".to_string()),
                log_file: None,
            });
            let _ = state.save(paths.state_dir());
            PushResult {
                success: true,
                error: None,
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "push: sync failed");
            let finished_at = chrono::Utc::now();
            record_push_outcome(
                paths,
                &config.namespace,
                started_at,
                finished_at,
                RunOutcome::Failed,
                Some(format!("{e}")),
            );
            PushResult {
                success: false,
                error: Some(format!("{e}")),
            }
        }
    }
}

/// Record a push outcome in the run history.
fn record_push_outcome(
    paths: &crate::paths::AppPaths,
    namespace: &str,
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: chrono::DateTime<chrono::Utc>,
    outcome: crate::state::RunOutcome,
    message: Option<String>,
) {
    use crate::state::{AppState, RunRecord};

    let mut state = AppState::load(paths.state_dir()).unwrap_or_default();
    state.record_run(RunRecord {
        namespace: namespace.to_string(),
        started_at,
        finished_at,
        outcome,
        commit: None,
        message,
        log_file: None,
    });
    let _ = state.save(paths.state_dir());
}

#[cfg(test)]
#[path = "../../tests/unit/tui/task.rs"]
mod tests;
