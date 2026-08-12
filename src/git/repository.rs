//! Repository structure validation.
//!
//! This module inspects an existing Git repository to determine whether it is
//! suitable for use by dothoard. It verifies:
//!
//! - The path is a valid Git worktree (not bare, not inside `.git`).
//! - A branch is checked out (detached HEAD is rejected).
//! - The configured remote exists in the repository.
//! - The repository is not in a conflicting operation state (merge, rebase,
//!   cherry-pick, or bisect).

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::runner::{GitCommand, GitError, GitRunner};

/// The result of validating a repository's structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryInfo {
    /// The absolute path to the worktree root.
    pub worktree: PathBuf,
    /// The name of the currently checked-out branch.
    pub branch: String,
    /// The configured remote that was validated.
    pub remote: String,
}

/// An in-progress operation that blocks backup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockingOperation {
    Merge,
    Rebase,
    CherryPick,
    Bisect,
}

impl std::fmt::Display for BlockingOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Merge => write!(f, "merge"),
            Self::Rebase => write!(f, "rebase"),
            Self::CherryPick => write!(f, "cherry-pick"),
            Self::Bisect => write!(f, "bisect"),
        }
    }
}

/// Errors from repository structure validation.
#[derive(Debug, Error)]
pub enum RepositoryError {
    /// The path is not inside a Git worktree.
    #[error("not a git repository: {path}")]
    NotARepository { path: PathBuf },

    /// The repository is bare (no worktree).
    #[error("repository is bare (no worktree): {path}")]
    BareRepository { path: PathBuf },

    /// HEAD is detached (no branch checked out).
    #[error("HEAD is detached; a branch must be checked out")]
    DetachedHead,

    /// The configured remote does not exist in the repository.
    #[error("remote \"{remote}\" does not exist in the repository")]
    RemoteNotFound { remote: String },

    /// A blocking operation is in progress.
    #[error("repository has an in-progress {operation}; resolve it before running a backup")]
    BlockingOperation { operation: BlockingOperation },

    /// An underlying Git command failed.
    #[error("git command failed during repository validation")]
    Git(#[from] GitError),
}

/// Validate that the given path is a usable Git repository for dothoard.
///
/// Checks:
/// 1. The path is a valid Git repository (not bare).
/// 2. A branch is checked out (not detached HEAD).
/// 3. The specified remote exists.
/// 4. No blocking operation (merge, rebase, cherry-pick, bisect) is active.
///
/// Returns structured information about the repository on success.
pub fn validate_repository(
    runner: &GitRunner,
    path: &Path,
    remote: &str,
) -> Result<RepositoryInfo, RepositoryError> {
    // 1. Verify this is a git repository (works for both bare and non-bare).
    check_is_git_repository(runner, path)?;

    // 2. Check for bare repository (before trying to get worktree root).
    check_not_bare(runner, path)?;

    // 3. Get the worktree root.
    let worktree = get_worktree_root(runner, path)?;

    // 4. Get the current branch (rejects detached HEAD).
    let branch = get_current_branch(runner, &worktree)?;

    // 5. Validate the remote exists.
    validate_remote(runner, &worktree, remote)?;

    // 6. Check for blocking operations.
    check_no_blocking_operation(runner, &worktree)?;

    Ok(RepositoryInfo {
        worktree,
        branch,
        remote: remote.to_string(),
    })
}

/// Verify the path is inside any git repository (bare or non-bare).
fn check_is_git_repository(runner: &GitRunner, path: &Path) -> Result<(), RepositoryError> {
    let cmd = GitCommand::new(path).args(["rev-parse", "--git-dir"]);
    runner.run(&cmd).map_err(|e| match &e {
        GitError::Failed { .. } => RepositoryError::NotARepository {
            path: path.to_path_buf(),
        },
        _ => RepositoryError::Git(e),
    })?;
    Ok(())
}

/// Get the absolute worktree root for the repository at `path`.
fn get_worktree_root(runner: &GitRunner, path: &Path) -> Result<PathBuf, RepositoryError> {
    let cmd = GitCommand::new(path).args(["rev-parse", "--show-toplevel"]);

    let output = runner.run(&cmd).map_err(|e| match &e {
        GitError::Failed { .. } => RepositoryError::NotARepository {
            path: path.to_path_buf(),
        },
        _ => RepositoryError::Git(e),
    })?;

    let toplevel = output.stdout_trimmed();
    if toplevel.is_empty() {
        return Err(RepositoryError::NotARepository {
            path: path.to_path_buf(),
        });
    }

    Ok(PathBuf::from(toplevel))
}

/// Verify the repository is not bare.
fn check_not_bare(runner: &GitRunner, worktree: &Path) -> Result<(), RepositoryError> {
    let cmd = GitCommand::new(worktree).args(["rev-parse", "--is-bare-repository"]);
    let output = runner.run(&cmd)?;

    if output.stdout_trimmed() == "true" {
        return Err(RepositoryError::BareRepository {
            path: worktree.to_path_buf(),
        });
    }

    Ok(())
}

/// Get the current branch name, rejecting detached HEAD.
fn get_current_branch(runner: &GitRunner, worktree: &Path) -> Result<String, RepositoryError> {
    let cmd = GitCommand::new(worktree).args(["symbolic-ref", "--short", "HEAD"]);

    let output = runner.run(&cmd).map_err(|e| match &e {
        GitError::Failed { .. } => RepositoryError::DetachedHead,
        _ => RepositoryError::Git(e),
    })?;

    let branch = output.stdout_trimmed().to_string();
    if branch.is_empty() {
        return Err(RepositoryError::DetachedHead);
    }

    Ok(branch)
}

/// Validate that the named remote exists.
fn validate_remote(
    runner: &GitRunner,
    worktree: &Path,
    remote: &str,
) -> Result<(), RepositoryError> {
    let cmd = GitCommand::new(worktree).args(["remote", "get-url", remote]);

    runner.run(&cmd).map_err(|e| match &e {
        GitError::Failed { .. } => RepositoryError::RemoteNotFound {
            remote: remote.to_string(),
        },
        _ => RepositoryError::Git(e),
    })?;

    Ok(())
}

/// Check that no blocking operation (merge, rebase, cherry-pick, bisect) is active.
fn check_no_blocking_operation(runner: &GitRunner, worktree: &Path) -> Result<(), RepositoryError> {
    // Get the .git directory path (handles both regular repos and worktrees).
    let git_dir = get_git_dir(runner, worktree)?;

    // Check for in-progress merge.
    if git_dir.join("MERGE_HEAD").exists() {
        return Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::Merge,
        });
    }

    // Check for in-progress rebase (multiple possible locations).
    if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        return Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::Rebase,
        });
    }

    // Check for in-progress cherry-pick.
    if git_dir.join("CHERRY_PICK_HEAD").exists() {
        return Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::CherryPick,
        });
    }

    // Check for in-progress bisect.
    if git_dir.join("BISECT_LOG").exists() {
        return Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::Bisect,
        });
    }

    Ok(())
}

/// Get the absolute path to the `.git` directory for the repository.
fn get_git_dir(runner: &GitRunner, worktree: &Path) -> Result<PathBuf, RepositoryError> {
    let cmd = GitCommand::new(worktree).args(["rev-parse", "--absolute-git-dir"]);
    let output = runner.run(&cmd)?;
    Ok(PathBuf::from(output.stdout_trimmed()))
}

#[cfg(test)]
#[path = "../../tests/unit/git/repository.rs"]
mod tests;
