//! Backup coordinator: executes the complete backup workflow.
//!
//! The coordinator is the top-level orchestrator that executes the backup
//! workflow in the exact order specified by `PLAN.md`:
//!
//! 1. Acquire exclusive application lock.
//! 2. Load and validate configuration.
//! 3. Validate the Git clone, branch, remote, and ownership state.
//! 4. Reject source overlap and repository recursion.
//! 5. Reject repositories in merge/rebase/cherry-pick/bisect states.
//! 6. Inspect worktree changes (block on unmanaged dirty paths).
//! 7. Preflight all configured sources and destinations.
//! 8. Mirror every configured source into `repository/home/...`.
//! 9. Update the repository manifest.
//! 10. Stage the complete managed namespace.
//! 11. Verify all staged paths are managed.
//! 12. Commit only when the staged tree changed.
//! 13. Pull with rebase (reconcile with remote).
//! 14. Push local commits.
//! 15. Persist the result.
//! 16. Send notification on failure or recovery.
//!
//! Steps 8-9 are handled together by the mirror executor.
//! Steps 10-11 are handled by the staging module.
//! Steps 13-14 are handled by the sync module.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use thiserror::Error;

use crate::config::Config;
use crate::git::{self, GitRunner, OwnershipState};
use crate::paths::AppPaths;
use crate::state::{AppState, RunOutcome, RunRecord};

use super::planner::{PlanInputs, plan_backup};

/// The outcome of a complete backup run.
#[derive(Debug)]
pub struct BackupOutcome {
    /// Whether the backup completed successfully.
    pub success: bool,

    /// The commit SHA if a commit was created.
    pub commit: Option<String>,

    /// Whether a push succeeded.
    pub pushed: bool,

    /// Whether there are pending commits waiting to be pushed.
    pub pending_push: bool,

    /// Warning messages accumulated during the run.
    pub warnings: Vec<String>,

    /// The error message if the run failed.
    pub error: Option<String>,

    /// Number of files copied/updated.
    pub copies: usize,

    /// Number of files deleted.
    pub deletions: usize,
}

/// Errors from the backup coordinator.
///
/// These represent failures that prevent the backup from completing.
/// The coordinator catches most errors internally and produces a
/// [`BackupOutcome`] with appropriate metadata. This error type is
/// reserved for truly unrecoverable situations.
#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("failed to load configuration: {0}")]
    Config(#[from] crate::config::ConfigError),

    #[error("configuration is invalid: {0}")]
    Validation(String),

    #[error("path resolution failed: {0}")]
    Paths(#[from] crate::paths::PathError),

    #[error("failed to acquire exclusive lock: {0}")]
    Lock(#[from] crate::locking::LockError),

    #[error("failed to load state: {0}")]
    State(#[from] crate::state::StateError),
}

/// Execute a complete backup run.
///
/// This is the main entry point for both the CLI `backup` command and the
/// systemd timer service. It performs the entire workflow from lock acquisition
/// through notification, recording the result in persistent state.
///
/// # Arguments
///
/// * `paths` - Resolved application paths.
///
/// # Returns
///
/// Always returns a `BackupOutcome` describing what happened. The caller
/// should use the outcome to determine the exit code and produce diagnostics.
pub fn run_backup(paths: &AppPaths) -> Result<BackupOutcome, CoordinatorError> {
    let started_at = Utc::now();

    // Step 1: Acquire exclusive lock.
    let _lock = crate::locking::try_acquire(paths.runtime_dir())?;
    tracing::info!("backup lock acquired");

    // Step 2: Load and validate configuration.
    let config = Config::load(paths.config_file())?;
    let validation_errors = config.validate();
    if !validation_errors.is_empty() {
        let messages: Vec<String> = validation_errors.iter().map(|e| e.to_string()).collect();
        return Err(CoordinatorError::Validation(messages.join("; ")));
    }
    tracing::info!(
        sources = config.sources.len(),
        repository = %config.repository,
        "configuration loaded"
    );

    // Resolve repository path.
    let repository = config.repository_path(paths.home());
    let timeout = Duration::from_secs(u64::from(config.network_timeout_seconds));
    let runner = GitRunner::new(timeout);

    // Execute the backup workflow (steps 3-14).
    let outcome = execute_workflow(paths, &config, &repository, &runner, started_at);

    // Step 15: Persist the result.
    if let Err(e) = persist_outcome(paths, &outcome, started_at) {
        tracing::error!(error = %e, "failed to persist run state");
        // Non-fatal: we still return the outcome to the caller.
    }

    Ok(outcome)
}

/// Execute the backup workflow steps 3-14.
///
/// Separated from `run_backup` so that state persistence and notification
/// can always run regardless of workflow outcome.
fn execute_workflow(
    paths: &AppPaths,
    config: &Config,
    repository: &PathBuf,
    runner: &GitRunner,
    started_at: chrono::DateTime<Utc>,
) -> BackupOutcome {
    let mut warnings: Vec<String> = Vec::new();

    // Step 3: Validate the Git repository.
    let repo_info = match git::validate_repository(runner, repository, &config.remote) {
        Ok(info) => info,
        Err(e) => {
            return BackupOutcome::failed(format!("repository validation failed: {e}"), warnings);
        }
    };
    tracing::info!(
        worktree = %repo_info.worktree.display(),
        branch = %repo_info.branch,
        remote = %repo_info.remote,
        "repository validated"
    );

    // Step 3 (continued): Validate ownership state.
    let ownership = match git::classify_ownership(repository) {
        Ok(state) => state,
        Err(e) => {
            return BackupOutcome::failed(format!("ownership check failed: {e}"), warnings);
        }
    };

    // For headless backup, we auto-initialize new namespaces and attach to
    // owned repositories. InvalidManifest and Ambiguous are hard failures.
    match &ownership {
        OwnershipState::New => {
            // Auto-initialize for headless backup (user chose the repo in config).
            if let Err(e) = git::initialize_or_attach(repository, &ownership, true) {
                return BackupOutcome::failed(format!("initialization failed: {e}"), warnings);
            }
            tracing::info!("initialized new managed namespace");
        }
        OwnershipState::Owned { .. } => {
            // Already owned — proceed.
            tracing::debug!("repository ownership confirmed");
        }
        OwnershipState::InvalidManifest { reason } => {
            return BackupOutcome::failed(
                format!("repository has invalid manifest: {reason}"),
                warnings,
            );
        }
        OwnershipState::Ambiguous { reason } => {
            return BackupOutcome::failed(
                format!("repository content is ambiguous: {reason}"),
                warnings,
            );
        }
    }

    // Step 4: Reject source overlap and repository recursion.
    let source_paths: Vec<PathBuf> = config
        .sources
        .iter()
        .map(|s| paths.home().join(&s.path))
        .collect();
    let overlaps = crate::paths::check_overlaps(&source_paths, repository);
    if !overlaps.is_empty() {
        let messages: Vec<String> = overlaps.iter().map(|e| e.to_string()).collect();
        return BackupOutcome::failed(
            format!("source/repository overlap: {}", messages.join("; ")),
            warnings,
        );
    }

    // Step 5: Already handled by validate_repository (blocking operations).

    // Step 6: Inspect worktree changes.
    let worktree_status = match git::classify_worktree(runner, &repo_info.worktree) {
        Ok(status) => status,
        Err(e) => {
            return BackupOutcome::failed(format!("worktree inspection failed: {e}"), warnings);
        }
    };

    if worktree_status.has_blocking_changes() {
        return BackupOutcome::failed(
            format!(
                "unmanaged changes block backup: {}",
                worktree_status.unmanaged_dirty.join(", ")
            ),
            warnings,
        );
    }

    if worktree_status.has_recoverable_changes() {
        tracing::info!(
            count = worktree_status.managed_dirty.len(),
            "recovering dirty managed paths"
        );
    }

    // Steps 7-9: Plan and execute the mirror.
    let plan_inputs = PlanInputs {
        home: paths.home(),
        repository,
        sources: &config.sources,
    };

    let changeset = match plan_backup(&plan_inputs) {
        Ok(cs) => cs,
        Err(e) => {
            return BackupOutcome::failed(format!("backup planning failed: {e}"), warnings);
        }
    };

    // Collect warnings from the changeset.
    for w in &changeset.warnings {
        warnings.push(format!("{}: {}", w.path.display(), w.kind));
    }

    let mirror_result = match super::executor::execute_mirror(
        paths.home(),
        repository,
        &config.sources,
        &changeset,
    ) {
        Ok(result) => result,
        Err(e) => {
            return BackupOutcome::failed(format!("mirror preflight failed: {e}"), warnings);
        }
    };

    let copies = mirror_result.copies_completed;
    let deletions = mirror_result.deletions_completed;

    if !mirror_result.may_publish {
        let error_messages: Vec<String> =
            mirror_result.errors.iter().map(|e| e.to_string()).collect();
        return BackupOutcome::failed(
            format!(
                "mirror failed, publication blocked: {}",
                error_messages.join("; ")
            ),
            warnings,
        );
    }

    tracing::info!(
        copies = copies,
        deletions = deletions,
        "mirror completed"
    );

    // Steps 10-11: Stage the managed namespace and verify boundaries.
    if let Err(e) = git::stage_managed_namespace(runner, &repo_info.worktree) {
        return BackupOutcome::failed(format!("staging failed: {e}"), warnings);
    }

    if let Err(e) = git::verify_staged_boundaries(runner, &repo_info.worktree) {
        return BackupOutcome::failed(format!("staged boundary check failed: {e}"), warnings);
    }

    // Step 12: Commit only when staged tree changed.
    let commit_message = format_commit_message(&started_at);
    let commit_result = match git::create_commit(runner, &repo_info.worktree, &commit_message) {
        Ok(result) => result,
        Err(e) => {
            return BackupOutcome::failed(format!("commit failed: {e}"), warnings);
        }
    };

    let commit_sha = commit_result.map(|r| r.sha);
    if let Some(ref sha) = commit_sha {
        tracing::info!(commit = %sha, "commit created");
    } else {
        tracing::info!("no changes to commit");
    }

    // Steps 13-14: Reconcile with remote (pull + push).
    // Also handle pending commits from previous offline runs (O03).
    let has_something_to_sync = commit_sha.is_some() || has_pending_push(paths);

    if has_something_to_sync {
        match git::sync_with_remote(runner, &repo_info.worktree, &config.remote, &repo_info.branch)
        {
            Ok(_sync_result) => {
                tracing::info!("synchronized with remote");
                BackupOutcome {
                    success: true,
                    commit: commit_sha,
                    pushed: true,
                    pending_push: false,
                    warnings,
                    error: None,
                    copies,
                    deletions,
                }
            }
            Err(git::SyncError::RemoteUnreachable { reason }) => {
                // Offline: local commit preserved, push later.
                tracing::warn!(reason = %reason, "remote unreachable, commit preserved");
                warnings.push(format!("push deferred: {reason}"));
                BackupOutcome {
                    success: true,
                    commit: commit_sha,
                    pushed: false,
                    pending_push: true,
                    warnings,
                    error: None,
                    copies,
                    deletions,
                }
            }
            Err(git::SyncError::Conflict) => {
                // Conflict: rebase aborted, commit preserved.
                tracing::error!("rebase conflict detected; manual intervention required");
                BackupOutcome::failed_with_commit(
                    "rebase conflict: manual intervention required".to_string(),
                    commit_sha,
                    warnings,
                    copies,
                    deletions,
                )
            }
            Err(git::SyncError::PushRejected { reason }) => {
                // Push rejected: commit preserved, retry later.
                tracing::warn!(reason = %reason, "push rejected, will retry");
                warnings.push(format!("push rejected: {reason}"));
                BackupOutcome {
                    success: true,
                    commit: commit_sha,
                    pushed: false,
                    pending_push: true,
                    warnings,
                    error: None,
                    copies,
                    deletions,
                }
            }
            Err(e) => {
                // Other sync errors.
                BackupOutcome::failed_with_commit(
                    format!("sync failed: {e}"),
                    commit_sha,
                    warnings,
                    copies,
                    deletions,
                )
            }
        }
    } else {
        // Nothing to sync: no new commit and no pending push.
        BackupOutcome {
            success: true,
            commit: None,
            pushed: false,
            pending_push: false,
            warnings,
            error: None,
            copies,
            deletions,
        }
    }
}

/// Check if there are pending commits from a previous offline run.
fn has_pending_push(paths: &AppPaths) -> bool {
    AppState::load(paths.state_dir())
        .map(|state| state.pending_push)
        .unwrap_or(false)
}

/// Persist the backup outcome to the state file.
fn persist_outcome(
    paths: &AppPaths,
    outcome: &BackupOutcome,
    started_at: chrono::DateTime<Utc>,
) -> Result<(), crate::state::StateError> {
    let mut state = AppState::load(paths.state_dir()).unwrap_or_default();
    let finished_at = Utc::now();

    let run_outcome = if outcome.success {
        if outcome.commit.is_some() {
            if outcome.pushed {
                RunOutcome::Success
            } else {
                RunOutcome::CommittedOffline
            }
        } else {
            RunOutcome::NoChanges
        }
    } else {
        RunOutcome::Failed
    };

    let message = if let Some(ref err) = outcome.error {
        Some(err.clone())
    } else if !outcome.warnings.is_empty() && outcome.pending_push {
        // Record the push warning in the message for CommittedOffline.
        outcome.warnings.last().cloned()
    } else {
        None
    };

    let record = RunRecord {
        started_at,
        finished_at,
        outcome: run_outcome,
        commit: outcome.commit.clone(),
        message,
    };

    state.record_run(record);
    state.save(paths.state_dir())
}

/// Format the commit message per PLAN.md suggestion.
fn format_commit_message(timestamp: &chrono::DateTime<Utc>) -> String {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());
    let time_str = timestamp.format("%Y-%m-%d %H:%M:%S");
    format!("backup({hostname}): {time_str}")
}

impl BackupOutcome {
    fn failed(error: String, warnings: Vec<String>) -> Self {
        Self {
            success: false,
            commit: None,
            pushed: false,
            pending_push: false,
            warnings,
            error: Some(error),
            copies: 0,
            deletions: 0,
        }
    }

    fn failed_with_commit(
        error: String,
        commit: Option<String>,
        warnings: Vec<String>,
        copies: usize,
        deletions: usize,
    ) -> Self {
        let pending = commit.is_some();
        Self {
            success: false,
            commit,
            pushed: false,
            pending_push: pending,
            warnings,
            error: Some(error),
            copies,
            deletions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_commit_message_contains_timestamp() {
        let ts = Utc::now();
        let msg = format_commit_message(&ts);

        assert!(msg.starts_with("backup("));
        assert!(msg.contains("): "));
        // Should contain a date-like string.
        assert!(msg.contains('-'));
        assert!(msg.contains(':'));
    }

    #[test]
    fn backup_outcome_failed_sets_fields_correctly() {
        let outcome = BackupOutcome::failed("some error".to_string(), vec!["warn".to_string()]);

        assert!(!outcome.success);
        assert_eq!(outcome.error, Some("some error".to_string()));
        assert_eq!(outcome.warnings, vec!["warn"]);
        assert_eq!(outcome.commit, None);
        assert!(!outcome.pushed);
        assert!(!outcome.pending_push);
    }

    #[test]
    fn backup_outcome_failed_with_commit_preserves_pending() {
        let outcome = BackupOutcome::failed_with_commit(
            "conflict".to_string(),
            Some("abc123".to_string()),
            vec![],
            5,
            2,
        );

        assert!(!outcome.success);
        assert_eq!(outcome.commit, Some("abc123".to_string()));
        assert!(outcome.pending_push);
        assert_eq!(outcome.copies, 5);
        assert_eq!(outcome.deletions, 2);
    }

    #[test]
    fn persist_outcome_records_success() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path().join("state");

        // Create the required directories.
        std::fs::create_dir_all(tmp.path().join("home")).unwrap();
        std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();

        let paths = AppPaths::resolve(crate::paths::PathInputs {
            home: Some(tmp.path().join("home")),
            config_dir: Some(tmp.path().join("config")),
            state_dir: Some(state_dir.clone()),
            runtime_dir: Some(tmp.path().join("runtime")),
            use_environment: false,
        })
        .unwrap();

        let outcome = BackupOutcome {
            success: true,
            commit: Some("deadbeef".to_string()),
            pushed: true,
            pending_push: false,
            warnings: vec![],
            error: None,
            copies: 3,
            deletions: 1,
        };

        let started_at = Utc::now();
        persist_outcome(&paths, &outcome, started_at).unwrap();

        let loaded = AppState::load(&state_dir).unwrap();
        assert_eq!(loaded.last_commit, Some("deadbeef".to_string()));
        assert!(!loaded.pending_push);
        assert_eq!(loaded.history.len(), 1);
        assert_eq!(loaded.history[0].outcome, RunOutcome::Success);
    }

    #[test]
    fn persist_outcome_records_offline_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path().join("state");
        std::fs::create_dir_all(tmp.path().join("home")).unwrap();
        std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();

        let paths = AppPaths::resolve(crate::paths::PathInputs {
            home: Some(tmp.path().join("home")),
            config_dir: Some(tmp.path().join("config")),
            state_dir: Some(state_dir.clone()),
            runtime_dir: Some(tmp.path().join("runtime")),
            use_environment: false,
        })
        .unwrap();

        let outcome = BackupOutcome {
            success: true,
            commit: Some("abc123".to_string()),
            pushed: false,
            pending_push: true,
            warnings: vec!["push deferred: network unreachable".to_string()],
            error: None,
            copies: 1,
            deletions: 0,
        };

        let started_at = Utc::now();
        persist_outcome(&paths, &outcome, started_at).unwrap();

        let loaded = AppState::load(&state_dir).unwrap();
        assert!(loaded.pending_push);
        assert_eq!(loaded.last_commit, Some("abc123".to_string()));
        assert_eq!(loaded.history[0].outcome, RunOutcome::CommittedOffline);
    }

    #[test]
    fn persist_outcome_records_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path().join("state");
        std::fs::create_dir_all(tmp.path().join("home")).unwrap();
        std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();

        let paths = AppPaths::resolve(crate::paths::PathInputs {
            home: Some(tmp.path().join("home")),
            config_dir: Some(tmp.path().join("config")),
            state_dir: Some(state_dir.clone()),
            runtime_dir: Some(tmp.path().join("runtime")),
            use_environment: false,
        })
        .unwrap();

        let outcome = BackupOutcome::failed("source not found".to_string(), vec![]);

        let started_at = Utc::now();
        persist_outcome(&paths, &outcome, started_at).unwrap();

        let loaded = AppState::load(&state_dir).unwrap();
        assert_eq!(
            loaded.latest_error,
            Some("source not found".to_string())
        );
        assert_eq!(loaded.history[0].outcome, RunOutcome::Failed);
    }
}
