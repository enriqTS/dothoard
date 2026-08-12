//! Worktree change classification.
//!
//! Inspects the Git worktree to identify dirty paths (staged, unstaged, or
//! untracked) and classifies each as **managed** or **unmanaged**.
//!
//! - **Managed paths** are beneath `home/` or equal to the manifest file.
//!   Dirty managed paths are recoverable: the mirror executor normalizes them
//!   on the next run.
//! - **Unmanaged paths** are everything else. Any dirty unmanaged path blocks
//!   the backup to prevent silently committing or discarding user data.
//!
//! This module uses `git status --porcelain=v2 -z` for machine-readable,
//! NUL-delimited output that handles filenames with special characters safely.

use std::path::Path;

use thiserror::Error;

use crate::app;
use crate::backup::mapping;

use super::runner::{GitCommand, GitError, GitRunner};

/// The result of worktree classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeStatus {
    /// Dirty paths within the managed namespace (recoverable).
    pub managed_dirty: Vec<String>,
    /// Dirty paths outside the managed namespace (blocking).
    pub unmanaged_dirty: Vec<String>,
}

impl WorktreeStatus {
    /// Returns true if the worktree is completely clean.
    pub fn is_clean(&self) -> bool {
        self.managed_dirty.is_empty() && self.unmanaged_dirty.is_empty()
    }

    /// Returns true if there are unmanaged dirty paths that block backup.
    pub fn has_blocking_changes(&self) -> bool {
        !self.unmanaged_dirty.is_empty()
    }

    /// Returns true if there are dirty managed paths that will be recovered.
    pub fn has_recoverable_changes(&self) -> bool {
        !self.managed_dirty.is_empty()
    }
}

/// Errors from worktree classification.
#[derive(Debug, Error)]
pub enum WorktreeError {
    /// The git status command failed.
    #[error("failed to inspect worktree status")]
    Git(#[from] GitError),
}

/// Classify all dirty paths in the worktree as managed or unmanaged.
///
/// Uses `git status --porcelain=v2 -z` to get NUL-delimited, machine-readable
/// output. Each changed path is classified based on whether it falls within
/// the managed namespace (`home/` or the manifest file).
///
/// # Arguments
///
/// * `runner` - The Git command runner.
/// * `worktree` - Absolute path to the repository worktree root.
pub fn classify_worktree(
    runner: &GitRunner,
    worktree: &Path,
) -> Result<WorktreeStatus, WorktreeError> {
    classify_namespace_worktree(runner, worktree, "")
}

/// Classify dirty paths for one namespace only.
pub fn classify_namespace_worktree(
    runner: &GitRunner,
    worktree: &Path,
    namespace: &str,
) -> Result<WorktreeStatus, WorktreeError> {
    let cmd =
        GitCommand::new(worktree).args(["status", "--porcelain=v2", "-z", "--untracked-files=all"]);
    let output = runner.run(&cmd)?;

    let mut managed_dirty = Vec::new();
    let mut unmanaged_dirty = Vec::new();

    let paths = parse_status_paths(&output.stdout);

    for path in paths {
        if is_namespace_managed_relative_path(&path, namespace) {
            managed_dirty.push(path);
        } else {
            unmanaged_dirty.push(path);
        }
    }

    managed_dirty.sort();
    unmanaged_dirty.sort();

    Ok(WorktreeStatus {
        managed_dirty,
        unmanaged_dirty,
    })
}

/// Check if a path (relative to the worktree) is within the managed namespace.
///
/// Managed paths are:
/// - Anything under `home/` (the backed-up content directory).
/// - The manifest file `.dothoard-manifest.toml`.
#[cfg(test)]
fn is_managed_relative_path(path: &str) -> bool {
    is_namespace_managed_relative_path(path, "")
}

/// Check whether a relative path belongs to the selected namespace.
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

/// Parse file paths from `git status --porcelain=v2 -z` output.
///
/// The v2 format uses NUL as delimiter between entries and has a structured
/// format for each entry type:
///
/// - Ordinary changed: `1 <XY> ... <path>\0`
/// - Renamed/copied: `2 <XY> ... <path>\0<orig_path>\0`
/// - Unmerged: `u <XY> ... <path>\0`
/// - Untracked: `? <path>\0`
/// - Ignored: `! <path>\0`
///
/// We extract the path from each entry and ignore the details (we only care
/// whether something is dirty, not what kind of change it is).
fn parse_status_paths(output: &str) -> Vec<String> {
    if output.is_empty() {
        return Vec::new();
    }

    let mut paths = Vec::new();
    let mut entries = output.split('\0').peekable();

    while let Some(entry) = entries.next() {
        if entry.is_empty() {
            continue;
        }

        let first_char = entry.chars().next().unwrap_or(' ');

        match first_char {
            // Ordinary changed entry: "1 XY sub mH mI mW hH hI path"
            '1' => {
                if let Some(path) = extract_path_from_ordinary(entry) {
                    paths.push(path);
                }
            }
            // Renamed/copied entry: "2 XY sub mH mI mW hH hI X### path\0orig_path"
            '2' => {
                if let Some(path) = extract_path_from_rename(entry) {
                    paths.push(path);
                }
                // Both endpoints must be classified. A sibling path renamed
                // into the active namespace is still an unmanaged change.
                if let Some(original_path) = entries.next() {
                    paths.push(original_path.to_string());
                }
            }
            // Unmerged entry: "u XY sub m1 m2 m3 mW h1 h2 h3 path"
            'u' => {
                if let Some(path) = extract_path_from_unmerged(entry) {
                    paths.push(path);
                }
            }
            // Untracked: "? path"
            '?' => {
                let path = entry[2..].to_string();
                if !path.is_empty() {
                    paths.push(path);
                }
            }
            // Ignored: "! path" — we don't care about ignored files.
            '!' => {}
            // Unknown format — skip.
            _ => {}
        }
    }

    paths
}

/// Extract the path from a porcelain v2 ordinary change entry.
/// Format: "1 XY sub mH mI mW hH hI path"
/// The path is everything after the 8th space.
fn extract_path_from_ordinary(entry: &str) -> Option<String> {
    // Skip "1 " prefix, then find the path after the field separators.
    // Fields: header(1), XY(1), sub(1), mH(1), mI(1), mW(1), hH(1), hI(1), path
    // That's 8 space-separated fields before the path.
    let mut spaces = 0;
    for (i, ch) in entry.char_indices() {
        if ch == ' ' {
            spaces += 1;
            if spaces == 8 {
                return Some(entry[i + 1..].to_string());
            }
        }
    }
    None
}

/// Extract the path from a porcelain v2 rename/copy entry.
/// Format: "2 XY sub mH mI mW hH hI X### path"
/// The path is everything after the 9th space.
fn extract_path_from_rename(entry: &str) -> Option<String> {
    // Fields: header(1), XY(1), sub(1), mH(1), mI(1), mW(1), hH(1), hI(1), X###(1), path
    // That's 9 space-separated fields before the path.
    let mut spaces = 0;
    for (i, ch) in entry.char_indices() {
        if ch == ' ' {
            spaces += 1;
            if spaces == 9 {
                return Some(entry[i + 1..].to_string());
            }
        }
    }
    None
}

/// Extract the path from a porcelain v2 unmerged entry.
/// Format: "u XY sub m1 m2 m3 mW h1 h2 h3 path"
/// The path is everything after the 10th space.
fn extract_path_from_unmerged(entry: &str) -> Option<String> {
    // Fields: header(1), XY(1), sub(1), m1(1), m2(1), m3(1), mW(1), h1(1), h2(1), h3(1), path
    // That's 10 space-separated fields before the path.
    let mut spaces = 0;
    for (i, ch) in entry.char_indices() {
        if ch == ' ' {
            spaces += 1;
            if spaces == 10 {
                return Some(entry[i + 1..].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "../../tests/unit/git/worktree.rs"]
mod tests;
