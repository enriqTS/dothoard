//! Remote reconciliation and conflict recovery.
//!
//! Implements the pull-with-rebase and push workflow:
//!
//! 1. **Pull with rebase**: Fetches from the remote and rebases local commits
//!    on top of upstream changes.
//! 2. **Push**: Pushes local commits to the remote.
//! 3. **Conflict recovery**: If a rebase conflicts, aborts the rebase,
//!    preserves the original local commit, and reports that manual
//!    intervention is required.
//!
//! On network or remote failure, local commits are preserved. Later runs
//! retry synchronization even if no new source files changed.

use std::path::Path;

use thiserror::Error;

use super::runner::{GitCommand, GitError, GitRunner};

/// The result of a synchronization attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncResult {
    /// Synchronization succeeded: pulled and pushed.
    Synced,
    /// Nothing to push (remote is up to date).
    UpToDate,
    /// Push succeeded after pulling upstream changes.
    PushedAfterRebase,
}

/// Errors from remote synchronization.
#[derive(Debug, Error)]
pub enum SyncError {
    /// A git command failed.
    #[error("sync failed: {0}")]
    Git(#[from] GitError),

    /// The pull resulted in a conflict that could not be automatically resolved.
    /// The rebase has been aborted and local commits preserved.
    #[error("rebase conflict detected; manual intervention required")]
    Conflict,

    /// The remote is not reachable (network or auth failure).
    /// Local commits are preserved for later retry.
    #[error("remote not reachable: {reason}")]
    RemoteUnreachable { reason: String },

    /// Push was rejected (e.g., non-fast-forward after a concurrent push).
    /// Local commits are preserved for later retry.
    #[error("push rejected by remote: {reason}")]
    PushRejected { reason: String },
}

/// Synchronize local commits with the remote.
///
/// Performs:
/// 1. Check if there are local commits ahead of the remote tracking branch.
/// 2. Pull with rebase from `remote/branch`.
/// 3. Push to `remote/branch`.
///
/// If the remote is unreachable, returns `Err(SyncError::RemoteUnreachable)`
/// but local commits are preserved.
///
/// If pull-with-rebase conflicts, aborts the rebase, preserves the local
/// commit, and returns `Err(SyncError::Conflict)`.
///
/// # Arguments
///
/// * `runner` - The Git command runner.
/// * `worktree` - Absolute path to the repository worktree root.
/// * `remote` - The remote name (e.g., "origin").
/// * `branch` - The current branch name (e.g., "main").
pub fn sync_with_remote(
    runner: &GitRunner,
    worktree: &Path,
    remote: &str,
    branch: &str,
) -> Result<SyncResult, SyncError> {
    // Check if there are commits to push.
    let has_local_commits = has_unpushed_commits(runner, worktree, remote, branch)?;

    // Try to pull with rebase (fetch + rebase in one step).
    match pull_with_rebase(runner, worktree, remote, branch) {
        Ok(()) => {}
        Err(SyncError::Conflict) => {
            // Abort the rebase to restore the pre-pull state.
            abort_rebase(runner, worktree);
            return Err(SyncError::Conflict);
        }
        Err(SyncError::RemoteUnreachable { reason }) => {
            // Network failure: local commits preserved.
            if has_local_commits {
                tracing::info!("remote unreachable; local commits preserved for later retry");
            }
            return Err(SyncError::RemoteUnreachable { reason });
        }
        Err(e) => return Err(e),
    }

    // Re-check after rebase (pull may have made us up-to-date).
    let still_has_commits = has_unpushed_commits(runner, worktree, remote, branch)?;

    if !still_has_commits && !has_local_commits {
        return Ok(SyncResult::UpToDate);
    }

    if !still_has_commits {
        // Our commits were already on the remote (fast-forward pull).
        return Ok(SyncResult::UpToDate);
    }

    // Push.
    match push(runner, worktree, remote, branch) {
        Ok(()) => {
            if has_local_commits {
                Ok(SyncResult::PushedAfterRebase)
            } else {
                Ok(SyncResult::Synced)
            }
        }
        Err(SyncError::RemoteUnreachable { reason }) => {
            tracing::info!("push failed; local commits preserved: {reason}");
            Err(SyncError::RemoteUnreachable { reason })
        }
        Err(e) => Err(e),
    }
}

/// Check if there are local commits not yet on the remote tracking branch.
fn has_unpushed_commits(
    runner: &GitRunner,
    worktree: &Path,
    remote: &str,
    branch: &str,
) -> Result<bool, SyncError> {
    let tracking = format!("{remote}/{branch}");

    // Check if the remote tracking ref exists.
    let cmd = GitCommand::new(worktree).args(["rev-parse", "--verify", &tracking]);
    let output = runner.run_raw(&cmd)?;
    if !output.status.success() {
        // No tracking branch yet — any local commit is unpushed.
        let cmd = GitCommand::new(worktree).args(["rev-parse", "--verify", "HEAD"]);
        let head_output = runner.run_raw(&cmd)?;
        return Ok(head_output.status.success());
    }

    // Count commits ahead.
    let range = format!("{tracking}..HEAD");
    let cmd = GitCommand::new(worktree).args(["rev-list", "--count", &range]);
    let output = runner.run(&cmd)?;
    let count: usize = output.stdout_trimmed().parse().unwrap_or(0);
    Ok(count > 0)
}

/// Pull with rebase from the remote.
fn pull_with_rebase(
    runner: &GitRunner,
    worktree: &Path,
    remote: &str,
    branch: &str,
) -> Result<(), SyncError> {
    let cmd = GitCommand::new(worktree)
        .args(["pull", "--rebase", remote, branch])
        .network();

    match runner.run(&cmd) {
        Ok(_) => Ok(()),
        Err(GitError::Failed { stderr, .. }) => {
            if is_conflict_error(&stderr) {
                Err(SyncError::Conflict)
            } else if is_network_error(&stderr) {
                Err(SyncError::RemoteUnreachable { reason: stderr })
            } else {
                // Could be that the remote branch doesn't exist yet.
                // In that case, there's nothing to pull — that's fine.
                if stderr.contains("Couldn't find remote ref")
                    || stderr.contains("no such ref was fetched")
                {
                    Ok(())
                } else {
                    Err(SyncError::Git(GitError::Failed {
                        args: format!("pull --rebase {remote} {branch}"),
                        code: 1,
                        stdout: String::new(),
                        stderr,
                    }))
                }
            }
        }
        Err(GitError::Timeout { timeout, args }) => Err(SyncError::RemoteUnreachable {
            reason: format!("timed out after {timeout:?}: {args}"),
        }),
        Err(e) => Err(SyncError::Git(e)),
    }
}

/// Push to the remote.
fn push(runner: &GitRunner, worktree: &Path, remote: &str, branch: &str) -> Result<(), SyncError> {
    let cmd = GitCommand::new(worktree)
        .args(["push", remote, branch])
        .network();

    match runner.run(&cmd) {
        Ok(_) => Ok(()),
        Err(GitError::Failed { stderr, .. }) => {
            if is_network_error(&stderr) {
                Err(SyncError::RemoteUnreachable { reason: stderr })
            } else if is_push_rejected(&stderr) {
                Err(SyncError::PushRejected { reason: stderr })
            } else {
                Err(SyncError::Git(GitError::Failed {
                    args: format!("push {remote} {branch}"),
                    code: 1,
                    stdout: String::new(),
                    stderr,
                }))
            }
        }
        Err(GitError::Timeout { timeout, args }) => Err(SyncError::RemoteUnreachable {
            reason: format!("timed out after {timeout:?}: {args}"),
        }),
        Err(e) => Err(SyncError::Git(e)),
    }
}

/// Abort an in-progress rebase, preserving the local commit.
fn abort_rebase(runner: &GitRunner, worktree: &Path) {
    let cmd = GitCommand::new(worktree).args(["rebase", "--abort"]);
    if let Err(e) = runner.run(&cmd) {
        tracing::warn!("failed to abort rebase: {e}");
    }
}

/// Check if an error message indicates a rebase conflict.
fn is_conflict_error(stderr: &str) -> bool {
    stderr.contains("CONFLICT")
        || stderr.contains("could not apply")
        || stderr.contains("Failed to merge")
}

/// Check if an error message indicates a network/connectivity problem.
fn is_network_error(stderr: &str) -> bool {
    stderr.contains("Could not resolve host")
        || stderr.contains("Connection refused")
        || stderr.contains("Connection timed out")
        || stderr.contains("Network is unreachable")
        || stderr.contains("unable to access")
        || stderr.contains("fatal: unable to connect")
        || stderr.contains("ssh: connect to host")
        || stderr.contains("Connection reset by peer")
        || stderr.contains("Permission denied")
        || stderr.contains("Host key verification failed")
        || stderr.contains("terminal prompts disabled")
        || stderr.contains("credential backing store")
        || stderr.contains("could not read Password")
        || stderr.contains("could not read Username")
}

/// Check if an error message indicates a push rejection.
fn is_push_rejected(stderr: &str) -> bool {
    stderr.contains("non-fast-forward")
        || stderr.contains("rejected")
        || stderr.contains("failed to push")
}

#[cfg(test)]
#[path = "../../tests/unit/git/sync.rs"]
mod tests;
