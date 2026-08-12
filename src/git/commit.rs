//! Commit creation with safety guards.
//!
//! Creates commits from staged changes with the following rules:
//!
//! - **Skip empty**: If nothing is staged, no commit is created and the
//!   function returns `Ok(None)`.
//! - **Unsigned by default**: The `--no-gpg-sign` flag prevents GPG pinentry
//!   from blocking a background run.
//! - **Preserve hook failures**: Repository hooks (pre-commit, commit-msg) are
//!   not bypassed. If a hook fails, the error is propagated.

use std::path::Path;

use thiserror::Error;

use super::runner::{GitCommand, GitError, GitRunner};
use super::staging;

/// The result of a successful commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResult {
    /// The full SHA-1 hash of the created commit.
    pub sha: String,
}

/// Errors from commit operations.
#[derive(Debug, Error)]
pub enum CommitError {
    /// A git command failed during commit.
    #[error("commit failed")]
    Git(#[from] GitError),

    /// A pre-commit or commit-msg hook failed.
    #[error("repository hook rejected the commit: {stderr}")]
    HookFailed { stderr: String },

    /// Staging check failed.
    #[error("failed to check staged changes")]
    Staging(#[from] staging::StagingError),
}

/// Create a commit from the currently staged changes.
///
/// If nothing is staged, returns `Ok(None)` without creating a commit.
/// If a repository hook rejects the commit, returns
/// `Err(CommitError::HookFailed)`.
///
/// The commit is unsigned by default (`--no-gpg-sign`) to prevent GPG
/// pinentry from blocking background runs.
///
/// # Arguments
///
/// * `runner` - The Git command runner.
/// * `worktree` - Absolute path to the repository worktree root.
/// * `message` - The commit message.
pub fn create_commit(
    runner: &GitRunner,
    worktree: &Path,
    message: &str,
) -> Result<Option<CommitResult>, CommitError> {
    // Check if there are staged changes.
    if !staging::has_staged_changes(runner, worktree)? {
        tracing::debug!("no staged changes; skipping commit");
        return Ok(None);
    }

    // Create the commit (unsigned, hooks run normally).
    let cmd = GitCommand::new(worktree).args(["commit", "--no-gpg-sign", "-m", message]);

    match runner.run(&cmd) {
        Ok(_) => {}
        Err(GitError::Failed { code, stderr, .. }) => {
            // Hook failures typically exit with code 1 and leave staged
            // changes in place. Check if changes are still staged to
            // distinguish hook failure from other errors.
            if staging::has_staged_changes(runner, worktree).unwrap_or(false) {
                return Err(CommitError::HookFailed { stderr });
            }
            return Err(CommitError::Git(GitError::Failed {
                args: "commit --no-gpg-sign -m <message>".to_string(),
                code,
                stdout: String::new(),
                stderr,
            }));
        }
        Err(e) => return Err(CommitError::Git(e)),
    }

    // Get the SHA of the commit we just created.
    let sha_cmd = GitCommand::new(worktree).args(["rev-parse", "HEAD"]);
    let sha_output = runner.run(&sha_cmd)?;

    Ok(Some(CommitResult {
        sha: sha_output.stdout_trimmed().to_string(),
    }))
}

#[cfg(test)]
#[path = "../../tests/unit/git/commit.rs"]
mod tests;
