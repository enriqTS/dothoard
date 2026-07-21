//! Tracked-ignored file detection.
//!
//! Identifies destination paths that are tracked by Git but would be ignored
//! by the current configuration. These are exposed as preview warnings because
//! ignoring a previously tracked file does not remove it from Git history.
//!
//! The user should be informed that:
//! - The file will be staged for deletion (the mirror removes it).
//! - The file remains in Git history.
//! - If the file contained secrets, the credentials should be rotated.

use std::path::Path;

use thiserror::Error;

use crate::backup::mapping;

use super::runner::{GitCommand, GitError, GitRunner};

/// Errors from tracked-file detection.
#[derive(Debug, Error)]
pub enum TrackedIgnoredError {
    /// A git command failed.
    #[error("failed to list tracked files")]
    Git(#[from] GitError),
}

/// Find tracked files within the managed namespace that match the given
/// ignored destination paths.
///
/// Takes a list of destination paths (relative to the repository root, e.g.,
/// `home/.config/fish/fish_variables`) that are currently ignored by the
/// configuration, and returns those that Git already tracks.
///
/// # Arguments
///
/// * `runner` - The Git command runner.
/// * `worktree` - Absolute path to the repository worktree root.
/// * `ignored_destinations` - Paths (relative to worktree) that are currently
///   being excluded by ignore rules.
pub fn find_tracked_ignored(
    runner: &GitRunner,
    worktree: &Path,
    ignored_destinations: &[&str],
) -> Result<Vec<String>, TrackedIgnoredError> {
    if ignored_destinations.is_empty() {
        return Ok(Vec::new());
    }

    // Get all tracked files in the managed namespace.
    let cmd = GitCommand::new(worktree).args([
        "ls-files",
        "--cached",
        "-z",
        "--",
        mapping::HOME_DIR_NAME,
    ]);
    let output = runner.run(&cmd)?;

    let tracked: std::collections::HashSet<&str> = output
        .stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect();

    let mut result: Vec<String> = ignored_destinations
        .iter()
        .filter(|path| tracked.contains(**path))
        .map(|s| s.to_string())
        .collect();

    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::fs;
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
    fn returns_empty_for_no_ignored_paths() {
        let (tmp, runner) = init_test_repo();

        let result = find_tracked_ignored(&runner, tmp.path(), &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn detects_tracked_file_that_becomes_ignored() {
        let (tmp, runner) = init_test_repo();

        // Track a file.
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".config/fish")).unwrap();
        fs::write(home.join(".config/fish/fish_variables"), "vars").unwrap();
        fs::write(home.join(".config/fish/config.fish"), "# fish").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(tmp.path()).args(["commit", "-m", "add fish config"]);
        runner.run(&cmd).unwrap();

        // Now check which ignored paths are tracked.
        let ignored = &["home/.config/fish/fish_variables"];
        let result = find_tracked_ignored(&runner, tmp.path(), ignored).unwrap();

        assert_eq!(result, vec!["home/.config/fish/fish_variables"]);
    }

    #[test]
    fn does_not_report_untracked_ignored_files() {
        let (tmp, runner) = init_test_repo();

        // Create a file but don't track it.
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".config")).unwrap();
        fs::write(home.join(".config/secret.env"), "API_KEY=xxx").unwrap();

        // It's ignored but never tracked — should not appear.
        let ignored = &["home/.config/secret.env"];
        let result = find_tracked_ignored(&runner, tmp.path(), ignored).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn multiple_tracked_ignored_files() {
        let (tmp, runner) = init_test_repo();

        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".config")).unwrap();
        fs::write(home.join(".config/token"), "secret").unwrap();
        fs::write(home.join(".config/cache.db"), "data").unwrap();
        fs::write(home.join(".config/settings.toml"), "keep").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(tmp.path()).args(["commit", "-m", "add config"]);
        runner.run(&cmd).unwrap();

        // Only some become ignored.
        let ignored = &[
            "home/.config/token",
            "home/.config/cache.db",
            "home/.config/nonexistent", // not tracked
        ];
        let result = find_tracked_ignored(&runner, tmp.path(), ignored).unwrap();

        assert_eq!(result, vec!["home/.config/cache.db", "home/.config/token"]);
    }

    #[test]
    fn works_with_empty_repository() {
        let (tmp, runner) = init_test_repo();

        let ignored = &["home/.bashrc"];
        let result = find_tracked_ignored(&runner, tmp.path(), ignored).unwrap();

        assert!(result.is_empty());
    }
}
