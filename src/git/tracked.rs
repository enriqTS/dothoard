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
#[path = "../../tests/unit/git/tracked.rs"]
mod tests;
