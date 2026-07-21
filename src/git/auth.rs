//! Noninteractive authentication readiness checks.
//!
//! Verifies that the configured remote is accessible without user interaction
//! (no password prompts, no host-key confirmations). This uses `git ls-remote`
//! which performs a network connection test without modifying the repository.
//!
//! The check reports readiness status without exposing credentials or remote
//! URLs containing credentials in its output.

use std::path::Path;

use thiserror::Error;

use crate::diagnostics;

use super::runner::{GitCommand, GitError, GitRunner};

/// The result of an authentication readiness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStatus {
    /// The remote is accessible noninteractively.
    Ready,
    /// The remote is not accessible. The reason is redacted of credentials.
    NotReady { reason: String },
}

impl AuthStatus {
    /// Returns true if the remote is accessible.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl std::fmt::Display for AuthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => write!(f, "remote is accessible noninteractively"),
            Self::NotReady { reason } => write!(f, "remote not accessible: {reason}"),
        }
    }
}

/// Errors from authentication checks.
#[derive(Debug, Error)]
pub enum AuthCheckError {
    /// A non-network git error prevented the check.
    #[error("authentication check failed")]
    Git(#[from] GitError),
}

/// Check if the configured remote is accessible noninteractively.
///
/// Uses `git ls-remote --exit-code <remote>` to test connectivity. This
/// performs a lightweight network operation (list remote refs) without
/// modifying the repository.
///
/// Returns `AuthStatus::Ready` if the remote responds, or
/// `AuthStatus::NotReady` with a redacted reason if it does not.
///
/// # Arguments
///
/// * `runner` - The Git command runner.
/// * `worktree` - Absolute path to the repository worktree root.
/// * `remote` - The remote name (e.g., "origin").
pub fn check_auth(
    runner: &GitRunner,
    worktree: &Path,
    remote: &str,
) -> Result<AuthStatus, AuthCheckError> {
    let cmd = GitCommand::new(worktree)
        .args(["ls-remote", "--exit-code", remote])
        .network();

    match runner.run(&cmd) {
        Ok(_) => Ok(AuthStatus::Ready),
        Err(GitError::Failed { stderr, .. }) => {
            let redacted = diagnostics::redact_sensitive_text(&stderr).into_owned();
            Ok(AuthStatus::NotReady { reason: redacted })
        }
        Err(GitError::Timeout { timeout, .. }) => Ok(AuthStatus::NotReady {
            reason: format!("connection timed out after {timeout:?}"),
        }),
        Err(e) => Err(AuthCheckError::Git(e)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::*;

    fn init_repo_with_local_remote() -> (tempfile::TempDir, tempfile::TempDir, GitRunner) {
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
    fn reports_ready_for_accessible_local_remote() {
        let (work_dir, _remote_dir, runner) = init_repo_with_local_remote();

        let status = check_auth(&runner, work_dir.path(), "origin").unwrap();
        assert_eq!(status, AuthStatus::Ready);
        assert!(status.is_ready());
    }

    #[test]
    fn reports_not_ready_for_nonexistent_remote_path() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = GitRunner::new(Duration::from_secs(10));

        let cmd = GitCommand::new(tmp.path()).args(["init", "--initial-branch=main"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(tmp.path()).args([
            "remote",
            "add",
            "origin",
            "/nonexistent/path/repo.git",
        ]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(tmp.path()).args(["commit", "--allow-empty", "-m", "init"]);
        runner.run(&cmd).unwrap();

        let status = check_auth(&runner, tmp.path(), "origin").unwrap();
        assert!(matches!(status, AuthStatus::NotReady { .. }));
        assert!(!status.is_ready());
    }

    #[test]
    fn auth_status_display() {
        let ready = AuthStatus::Ready;
        assert!(ready.to_string().contains("accessible"));

        let not_ready = AuthStatus::NotReady {
            reason: "connection refused".to_string(),
        };
        assert!(not_ready.to_string().contains("connection refused"));
    }

    #[test]
    fn reports_not_ready_for_deleted_remote() {
        let (work_dir, remote_dir, runner) = init_repo_with_local_remote();

        // Delete the remote repository.
        fs::remove_dir_all(remote_dir.path()).unwrap();

        let status = check_auth(&runner, work_dir.path(), "origin").unwrap();
        assert!(matches!(status, AuthStatus::NotReady { .. }));
    }
}
