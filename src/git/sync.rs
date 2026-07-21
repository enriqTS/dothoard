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
    #[error("sync failed")]
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
}

/// Check if an error message indicates a push rejection.
fn is_push_rejected(stderr: &str) -> bool {
    stderr.contains("non-fast-forward")
        || stderr.contains("rejected")
        || stderr.contains("failed to push")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::*;

    /// Create a repo with a bare remote for sync testing.
    fn init_repo_with_remote() -> (tempfile::TempDir, tempfile::TempDir, GitRunner) {
        let remote_dir = tempfile::tempdir().unwrap();
        let work_dir = tempfile::tempdir().unwrap();
        let runner = GitRunner::new(Duration::from_secs(10));

        // Create bare remote.
        let cmd =
            GitCommand::new(remote_dir.path()).args(["init", "--bare", "--initial-branch=main"]);
        runner.run(&cmd).unwrap();

        // Create working repo.
        let cmd = GitCommand::new(work_dir.path()).args(["init", "--initial-branch=main"]);
        runner.run(&cmd).unwrap();

        // Add remote.
        let remote_path = remote_dir.path().to_str().unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["remote", "add", "origin", remote_path]);
        runner.run(&cmd).unwrap();

        // Initial commit and push.
        let cmd =
            GitCommand::new(work_dir.path()).args(["commit", "--allow-empty", "-m", "initial"]);
        runner.run(&cmd).unwrap();

        let cmd = GitCommand::new(work_dir.path())
            .args(["push", "-u", "origin", "main"])
            .network();
        runner.run(&cmd).unwrap();

        (work_dir, remote_dir, runner)
    }

    #[test]
    fn sync_up_to_date_when_nothing_to_push() {
        let (work_dir, _remote_dir, runner) = init_repo_with_remote();

        let result = sync_with_remote(&runner, work_dir.path(), "origin", "main").unwrap();
        assert_eq!(result, SyncResult::UpToDate);
    }

    #[test]
    fn sync_pushes_local_commit() {
        let (work_dir, _remote_dir, runner) = init_repo_with_remote();

        // Create a local commit.
        let home = work_dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "content").unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "home/.bashrc"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "backup"]);
        runner.run(&cmd).unwrap();

        let result = sync_with_remote(&runner, work_dir.path(), "origin", "main").unwrap();
        assert!(matches!(
            result,
            SyncResult::Synced | SyncResult::PushedAfterRebase
        ));
    }

    #[test]
    fn sync_pulls_upstream_changes() {
        let (work_dir, remote_dir, runner) = init_repo_with_remote();

        // Create a second clone that pushes a commit to the remote.
        let other_dir = tempfile::tempdir().unwrap();
        let remote_path = remote_dir.path().to_str().unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["clone", remote_path, "."]);
        runner.run(&cmd).unwrap();
        fs::write(other_dir.path().join("other.txt"), "data").unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["add", "--", "other.txt"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["commit", "-m", "upstream change"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(other_dir.path())
            .args(["push", "origin", "main"])
            .network();
        runner.run(&cmd).unwrap();

        // Now sync our working repo — it should pull the upstream change.
        let result = sync_with_remote(&runner, work_dir.path(), "origin", "main").unwrap();
        assert_eq!(result, SyncResult::UpToDate);

        // Verify the upstream file is now in our worktree.
        assert!(work_dir.path().join("other.txt").exists());
    }

    #[test]
    fn sync_rebases_local_on_upstream() {
        let (work_dir, remote_dir, runner) = init_repo_with_remote();

        // Push from another clone.
        let other_dir = tempfile::tempdir().unwrap();
        let remote_path = remote_dir.path().to_str().unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["clone", remote_path, "."]);
        runner.run(&cmd).unwrap();
        fs::write(other_dir.path().join("upstream.txt"), "upstream").unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["add", "--", "upstream.txt"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["commit", "-m", "upstream"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(other_dir.path())
            .args(["push", "origin", "main"])
            .network();
        runner.run(&cmd).unwrap();

        // Create a local commit (non-conflicting).
        let home = work_dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "local").unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "home/.bashrc"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "local backup"]);
        runner.run(&cmd).unwrap();

        // Sync should rebase our commit on top of upstream and push.
        let result = sync_with_remote(&runner, work_dir.path(), "origin", "main").unwrap();
        assert_eq!(result, SyncResult::PushedAfterRebase);

        // Both changes should be present.
        assert!(work_dir.path().join("upstream.txt").exists());
        assert!(work_dir.path().join("home/.bashrc").exists());
    }

    #[test]
    fn sync_handles_conflict_by_aborting_rebase() {
        let (work_dir, remote_dir, runner) = init_repo_with_remote();

        // Create a file in our repo and push.
        fs::write(work_dir.path().join("conflict.txt"), "local version").unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "conflict.txt"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "local"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(work_dir.path())
            .args(["push", "origin", "main"])
            .network();
        runner.run(&cmd).unwrap();

        // Push a conflicting change from another clone.
        let other_dir = tempfile::tempdir().unwrap();
        let remote_path = remote_dir.path().to_str().unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["clone", remote_path, "."]);
        runner.run(&cmd).unwrap();
        fs::write(other_dir.path().join("conflict.txt"), "upstream version\n").unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["add", "--", "conflict.txt"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(other_dir.path()).args(["commit", "-m", "upstream conflict"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(other_dir.path())
            .args(["push", "--force", "origin", "main"])
            .network();
        runner.run(&cmd).unwrap();

        // Now create a conflicting local commit (different content in same file).
        fs::write(work_dir.path().join("conflict.txt"), "new local version\n").unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "conflict.txt"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "conflicting local"]);
        runner.run(&cmd).unwrap();

        // Sync should detect conflict, abort rebase, and preserve local commit.
        let result = sync_with_remote(&runner, work_dir.path(), "origin", "main");
        assert!(matches!(result, Err(SyncError::Conflict)));

        // Verify we're not in a rebase state.
        let git_dir = work_dir.path().join(".git");
        assert!(!git_dir.join("rebase-merge").exists());
        assert!(!git_dir.join("rebase-apply").exists());

        // Verify our local commit is still there.
        let log_cmd = GitCommand::new(work_dir.path()).args(["log", "-1", "--format=%s"]);
        let log_output = runner.run(&log_cmd).unwrap();
        assert_eq!(log_output.stdout_trimmed(), "conflicting local");
    }

    #[test]
    fn has_unpushed_returns_false_when_synced() {
        let (work_dir, _remote_dir, runner) = init_repo_with_remote();

        let result = has_unpushed_commits(&runner, work_dir.path(), "origin", "main").unwrap();
        assert!(!result);
    }

    #[test]
    fn has_unpushed_returns_true_with_local_commit() {
        let (work_dir, _remote_dir, runner) = init_repo_with_remote();

        fs::write(work_dir.path().join("file.txt"), "data").unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "file.txt"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "local"]);
        runner.run(&cmd).unwrap();

        let result = has_unpushed_commits(&runner, work_dir.path(), "origin", "main").unwrap();
        assert!(result);
    }

    #[test]
    fn network_error_detection() {
        assert!(is_network_error(
            "fatal: Could not resolve host example.com"
        ));
        assert!(is_network_error(
            "ssh: connect to host example.com port 22: Connection refused"
        ));
        assert!(is_network_error(
            "fatal: unable to access 'https://example.com/repo.git'"
        ));
        assert!(!is_network_error(
            "error: src refspec main does not match any"
        ));
    }

    #[test]
    fn conflict_error_detection() {
        assert!(is_conflict_error(
            "CONFLICT (content): Merge conflict in file.txt"
        ));
        assert!(is_conflict_error(
            "error: could not apply abc1234... commit msg"
        ));
        assert!(!is_conflict_error("Everything up-to-date"));
    }

    #[test]
    fn push_rejected_detection() {
        assert!(is_push_rejected(
            "! [rejected] main -> main (non-fast-forward)"
        ));
        assert!(is_push_rejected("error: failed to push some refs"));
        assert!(!is_push_rejected("Everything up-to-date"));
    }
}
