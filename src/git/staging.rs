//! Restricted staging and staged-boundary verification.
//!
//! This module stages only the selected managed namespace (`<namespace>/home/`
//! and its manifest) using literal pathspecs with `--` separation to prevent pathspec
//! metacharacters from being interpreted. After staging, it verifies that every
//! staged path is within the managed namespace before allowing a commit.
//!
//! Safety invariants:
//! - Only `home/` and `.dothoard-manifest.toml` are ever staged.
//! - Pathspecs use `:(literal)` prefix to disable glob interpretation.
//! - `--` separates options from paths to prevent argument injection.
//! - Staged paths are verified with `git diff --cached --name-only -z`.
//! - Any staged path outside the managed namespace aborts before commit.

use std::path::Path;

use thiserror::Error;

use crate::app;
use crate::backup::mapping;

use super::runner::{GitCommand, GitError, GitRunner};

/// Errors from staging and verification operations.
#[derive(Debug, Error)]
pub enum StagingError {
    /// A git command failed during staging.
    #[error("staging failed")]
    Git(#[from] GitError),

    /// Staged paths include files outside the managed namespace.
    #[error(
        "staged paths include unmanaged files that would be committed: {}",
        paths.join(", ")
    )]
    UnmanagedStaged { paths: Vec<String> },
}

/// Stage the complete managed namespace using literal pathspecs.
///
/// This stages `home/` and the manifest file (if they exist) using:
/// ```text
/// git add --all -- :(literal)home :(literal).dothoard-manifest.toml
/// ```
///
/// The `--all` flag ensures deletions are staged. The `:(literal)` pathspec
/// magic disables glob interpretation so filenames containing `*`, `?`, `[`,
/// etc. are handled safely. The `--` separates options from pathspecs.
///
/// Only paths that exist in the worktree or index are included to avoid
/// errors from git when referencing nonexistent paths.
///
/// # Arguments
///
/// * `runner` - The Git command runner.
/// * `worktree` - Absolute path to the repository worktree root.
pub fn stage_managed_namespace(runner: &GitRunner, worktree: &Path) -> Result<(), StagingError> {
    stage_namespace(runner, worktree, "")
}

/// Stage only the selected namespace's home directory and manifest.
pub fn stage_namespace(
    runner: &GitRunner,
    worktree: &Path,
    namespace: &str,
) -> Result<(), StagingError> {
    let prefix = if namespace.is_empty() {
        String::new()
    } else {
        format!("{namespace}/")
    };
    let home_relative = format!("{}{}/", prefix, mapping::HOME_DIR_NAME);
    let home_relative = home_relative.trim_end_matches('/');
    let manifest_relative = format!(
        "{}{}/{}",
        "",
        prefix.trim_end_matches('/'),
        app::MANIFEST_FILE_NAME
    )
    .trim_start_matches('/')
    .to_string();
    let home_dir = worktree.join(home_relative);
    let manifest_file = worktree.join(&manifest_relative);

    let mut args: Vec<String> = vec!["add".to_string(), "--all".to_string(), "--".to_string()];

    // Only include paths that exist on disk or are tracked (for deletions).
    let home_exists = home_dir.exists();
    let manifest_exists = manifest_file.exists();

    // Check if home/ or manifest are tracked (for deletion staging).
    let home_tracked = is_path_tracked(runner, worktree, home_relative)?;
    let manifest_tracked = is_path_tracked(runner, worktree, &manifest_relative)?;

    if home_exists || home_tracked {
        args.push(format!(":(literal){home_relative}"));
    }
    if manifest_exists || manifest_tracked {
        args.push(format!(":(literal){manifest_relative}"));
    }

    // If nothing to stage, return early.
    if args.len() <= 3 {
        return Ok(());
    }

    let cmd = GitCommand::new(worktree).args(args.iter().map(|s| s.as_str()));
    runner.run(&cmd)?;
    Ok(())
}

/// Check if a path is tracked in the git index.
fn is_path_tracked(runner: &GitRunner, worktree: &Path, path: &str) -> Result<bool, StagingError> {
    let cmd = GitCommand::new(worktree).args(["ls-files", "--error-unmatch", "--", path]);
    let output = runner.run_raw(&cmd)?;
    Ok(output.status.success())
}

/// Verify that all currently staged paths are within the managed namespace.
///
/// Uses `git diff --cached --name-status -z` to get the complete list of staged
/// paths (NUL-delimited for safe parsing), including both endpoints of a rename
/// or copy, then checks each against the managed namespace boundaries.
///
/// Returns `Ok(staged_paths)` if all paths are managed, or
/// `Err(StagingError::UnmanagedStaged)` if any path is outside the namespace.
///
/// # Arguments
///
/// * `runner` - The Git command runner.
/// * `worktree` - Absolute path to the repository worktree root.
pub fn verify_staged_boundaries(
    runner: &GitRunner,
    worktree: &Path,
) -> Result<Vec<String>, StagingError> {
    verify_namespace_boundaries(runner, worktree, "")
}

/// Verify that every staged path belongs to the selected namespace.
pub fn verify_namespace_boundaries(
    runner: &GitRunner,
    worktree: &Path,
    namespace: &str,
) -> Result<Vec<String>, StagingError> {
    let cmd = GitCommand::new(worktree).args(["diff", "--cached", "--name-status", "-z"]);
    let output = runner.run(&cmd)?;

    let staged_paths = parse_staged_paths(&output.stdout);

    let unmanaged: Vec<String> = staged_paths
        .iter()
        .filter(|path| !is_namespace_managed_relative_path(path, namespace))
        .cloned()
        .collect();

    if !unmanaged.is_empty() {
        return Err(StagingError::UnmanagedStaged { paths: unmanaged });
    }

    Ok(staged_paths)
}

/// Check whether there are staged changes ready to commit.
///
/// Uses `git diff --cached --quiet` which exits 0 if no staged changes
/// and exits 1 if there are staged changes.
///
/// # Arguments
///
/// * `runner` - The Git command runner.
/// * `worktree` - Absolute path to the repository worktree root.
pub fn has_staged_changes(runner: &GitRunner, worktree: &Path) -> Result<bool, StagingError> {
    let cmd = GitCommand::new(worktree).args(["diff", "--cached", "--quiet"]);
    let output = runner.run_raw(&cmd)?;

    // Exit 0 = no changes, exit 1 = changes exist.
    Ok(!output.status.success())
}

/// Check if a path (relative to the worktree) is within the managed namespace.
#[cfg(test)]
fn is_managed_relative_path(path: &str) -> bool {
    is_namespace_managed_relative_path(path, "")
}

/// Parse NUL-delimited `git diff --name-status` output.
///
/// Rename and copy records include two paths. Both are verified because a
/// rename from a sibling namespace into the active one would otherwise expose
/// only its active destination to a name-only listing.
fn parse_staged_paths(output: &str) -> Vec<String> {
    let mut fields = output.split('\0').filter(|field| !field.is_empty());
    let mut paths = Vec::new();

    while let Some(status) = fields.next() {
        let is_rename_or_copy = status.starts_with('R') || status.starts_with('C');
        let Some(path) = fields.next() else {
            break;
        };
        paths.push(path.to_string());
        if is_rename_or_copy {
            if let Some(path) = fields.next() {
                paths.push(path.to_string());
            }
        }
    }

    paths
}

fn is_namespace_managed_relative_path(path: &str, namespace: &str) -> bool {
    let prefix = if namespace.is_empty() {
        String::new()
    } else {
        format!("{namespace}/")
    };
    let home = format!("{}{}", prefix, mapping::HOME_DIR_NAME);
    path == home
        || path.starts_with(&format!("{home}/"))
        || (!namespace.is_empty() && path == format!("{namespace}/{}", app::MANIFEST_FILE_NAME))
        || (namespace.is_empty() && path == app::MANIFEST_FILE_NAME)
}

#[cfg(test)]
#[path = "../../tests/unit/git/staging.rs"]
mod tests;
