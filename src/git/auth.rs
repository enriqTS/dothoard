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
#[path = "../../tests/unit/git/auth.rs"]
mod tests;
