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
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::*;

    fn init_test_repo() -> (tempfile::TempDir, GitRunner) {
        let tmp = tempfile::tempdir().unwrap();
        let runner = GitRunner::new(Duration::from_secs(10));

        let cmd = GitCommand::new(tmp.path()).args(["init", "--initial-branch=main"]);
        runner.run(&cmd).unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["commit", "--allow-empty", "-m", "initial"]);
        runner.run(&cmd).unwrap();

        (tmp, runner)
    }

    #[test]
    fn skips_commit_when_nothing_staged() {
        let (tmp, runner) = init_test_repo();

        let result = create_commit(&runner, tmp.path(), "should not happen").unwrap();

        assert_eq!(result, None);
    }

    #[test]
    fn creates_commit_from_staged_changes() {
        let (tmp, runner) = init_test_repo();

        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "# bash").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/.bashrc"]);
        runner.run(&cmd).unwrap();

        let result = create_commit(&runner, tmp.path(), "backup: test").unwrap();

        assert!(result.is_some());
        let commit = result.unwrap();
        assert!(!commit.sha.is_empty());
        assert_eq!(commit.sha.len(), 40);
    }

    #[test]
    fn commit_message_is_preserved() {
        let (tmp, runner) = init_test_repo();

        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "content").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/.bashrc"]);
        runner.run(&cmd).unwrap();

        create_commit(&runner, tmp.path(), "backup(host): 2026-07-21 14:30:00").unwrap();

        let log_cmd = GitCommand::new(tmp.path()).args(["log", "-1", "--format=%s"]);
        let log_output = runner.run(&log_cmd).unwrap();
        assert_eq!(
            log_output.stdout_trimmed(),
            "backup(host): 2026-07-21 14:30:00"
        );
    }

    #[test]
    fn preserves_hook_failure() {
        let (tmp, runner) = init_test_repo();

        // Install a pre-commit hook that always fails.
        let hooks_dir = tmp.path().join(".git/hooks");
        fs::create_dir_all(&hooks_dir).unwrap();
        let hook_path = hooks_dir.join("pre-commit");
        fs::write(&hook_path, "#!/bin/sh\necho 'hook rejected' >&2\nexit 1\n").unwrap();
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "content").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/.bashrc"]);
        runner.run(&cmd).unwrap();

        let result = create_commit(&runner, tmp.path(), "should be rejected");
        assert!(matches!(result, Err(CommitError::HookFailed { .. })));
    }

    #[test]
    fn second_commit_works() {
        let (tmp, runner) = init_test_repo();

        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();

        // First commit.
        fs::write(home.join(".bashrc"), "v1").unwrap();
        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/.bashrc"]);
        runner.run(&cmd).unwrap();
        create_commit(&runner, tmp.path(), "first").unwrap();

        // Second commit.
        fs::write(home.join(".bashrc"), "v2").unwrap();
        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/.bashrc"]);
        runner.run(&cmd).unwrap();
        let result = create_commit(&runner, tmp.path(), "second").unwrap();

        assert!(result.is_some());

        let log_cmd = GitCommand::new(tmp.path()).args(["log", "--oneline"]);
        let log_output = runner.run(&log_cmd).unwrap();
        assert_eq!(log_output.stdout_lines().len(), 3);
    }
}
