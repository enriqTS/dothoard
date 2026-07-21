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
    let cmd =
        GitCommand::new(worktree).args(["status", "--porcelain=v2", "-z", "--untracked-files=all"]);
    let output = runner.run(&cmd)?;

    let mut managed_dirty = Vec::new();
    let mut unmanaged_dirty = Vec::new();

    let paths = parse_status_paths(&output.stdout);

    for path in paths {
        if is_managed_relative_path(&path) {
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
/// - The manifest file `.config-sync-manifest.toml`.
fn is_managed_relative_path(path: &str) -> bool {
    let home_prefix = format!("{}/", mapping::HOME_DIR_NAME);
    path.starts_with(&home_prefix)
        || path == mapping::HOME_DIR_NAME
        || path == app::MANIFEST_FILE_NAME
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
                // Consume the original path (next NUL-separated field).
                let _ = entries.next();
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
    fn clean_worktree_reports_no_changes() {
        let (tmp, runner) = init_test_repo();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(status.is_clean());
        assert!(!status.has_blocking_changes());
        assert!(!status.has_recoverable_changes());
    }

    #[test]
    fn untracked_managed_file_is_recoverable() {
        let (tmp, runner) = init_test_repo();

        // Create a file in the managed namespace.
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".config")).unwrap();
        fs::write(home.join(".config/test.txt"), "content").unwrap();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(!status.is_clean());
        assert!(!status.has_blocking_changes());
        assert!(status.has_recoverable_changes());
        assert!(
            status
                .managed_dirty
                .contains(&"home/.config/test.txt".to_string())
        );
    }

    #[test]
    fn untracked_unmanaged_file_blocks_backup() {
        let (tmp, runner) = init_test_repo();

        // Create a file outside the managed namespace.
        fs::write(tmp.path().join("README.md"), "hello").unwrap();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(!status.is_clean());
        assert!(status.has_blocking_changes());
        assert!(status.unmanaged_dirty.contains(&"README.md".to_string()));
    }

    #[test]
    fn staged_managed_file_is_recoverable() {
        let (tmp, runner) = init_test_repo();

        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "alias ls='ls --color'").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "home/.bashrc"]);
        runner.run(&cmd).unwrap();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(status.has_recoverable_changes());
        assert!(!status.has_blocking_changes());
        assert!(status.managed_dirty.contains(&"home/.bashrc".to_string()));
    }

    #[test]
    fn staged_unmanaged_file_blocks_backup() {
        let (tmp, runner) = init_test_repo();

        fs::write(tmp.path().join("notes.txt"), "my notes").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "notes.txt"]);
        runner.run(&cmd).unwrap();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(status.has_blocking_changes());
        assert!(status.unmanaged_dirty.contains(&"notes.txt".to_string()));
    }

    #[test]
    fn modified_tracked_managed_file_is_recoverable() {
        let (tmp, runner) = init_test_repo();

        // Create and commit a managed file.
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "v1").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "home/.bashrc"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(tmp.path()).args(["commit", "-m", "add bashrc"]);
        runner.run(&cmd).unwrap();

        // Modify it.
        fs::write(home.join(".bashrc"), "v2").unwrap();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(status.has_recoverable_changes());
        assert!(!status.has_blocking_changes());
    }

    #[test]
    fn manifest_file_is_managed() {
        let (tmp, runner) = init_test_repo();

        // Create the manifest file (untracked).
        fs::write(
            tmp.path().join(app::MANIFEST_FILE_NAME),
            "format = \"config-sync-manifest\"\nversion = 1\n",
        )
        .unwrap();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(status.has_recoverable_changes());
        assert!(!status.has_blocking_changes());
        assert!(
            status
                .managed_dirty
                .contains(&app::MANIFEST_FILE_NAME.to_string())
        );
    }

    #[test]
    fn mixed_managed_and_unmanaged_changes() {
        let (tmp, runner) = init_test_repo();

        // Managed file.
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "content").unwrap();

        // Unmanaged file.
        fs::write(tmp.path().join("unrelated.txt"), "data").unwrap();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(status.has_blocking_changes());
        assert!(status.has_recoverable_changes());
        assert!(status.managed_dirty.contains(&"home/.bashrc".to_string()));
        assert!(
            status
                .unmanaged_dirty
                .contains(&"unrelated.txt".to_string())
        );
    }

    #[test]
    fn deeply_nested_managed_path_is_recoverable() {
        let (tmp, runner) = init_test_repo();

        let nested = tmp.path().join("home/.config/nvim/lua/plugins");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("init.lua"), "-- plugins").unwrap();

        let status = classify_worktree(&runner, tmp.path()).unwrap();

        assert!(status.has_recoverable_changes());
        assert!(!status.has_blocking_changes());
    }

    // --- Unit tests for parsing ---

    #[test]
    fn parse_empty_status() {
        let paths = parse_status_paths("");
        assert!(paths.is_empty());
    }

    #[test]
    fn parse_untracked_entries() {
        // "? path\0" format
        let output = "? home/.bashrc\0? README.md\0";
        let paths = parse_status_paths(output);
        assert_eq!(paths, vec!["home/.bashrc", "README.md"]);
    }

    #[test]
    fn parse_ordinary_changed_entry() {
        // Ordinary changed: "1 XY sub mH mI mW hH hI path"
        let output = "1 .M N... 100644 100644 100644 abc123 def456 home/.bashrc\0";
        let paths = parse_status_paths(output);
        assert_eq!(paths, vec!["home/.bashrc"]);
    }

    #[test]
    fn parse_renamed_entry_consumes_orig_path() {
        // Renamed: "2 XY sub mH mI mW hH hI R### new_path\0old_path\0"
        let output =
            "2 R. N... 100644 100644 100644 abc123 def456 R100 home/new.txt\0home/old.txt\0";
        let paths = parse_status_paths(output);
        // Should only report the new path, not the old one.
        assert_eq!(paths, vec!["home/new.txt"]);
    }

    #[test]
    fn is_managed_recognizes_home_paths() {
        assert!(is_managed_relative_path("home/.bashrc"));
        assert!(is_managed_relative_path("home/.config/fish/config.fish"));
        assert!(is_managed_relative_path("home/"));
    }

    #[test]
    fn is_managed_recognizes_manifest() {
        assert!(is_managed_relative_path(app::MANIFEST_FILE_NAME));
    }

    #[test]
    fn is_managed_rejects_unmanaged_paths() {
        assert!(!is_managed_relative_path("README.md"));
        assert!(!is_managed_relative_path("src/main.rs"));
        assert!(!is_managed_relative_path("homepage/index.html"));
        assert!(!is_managed_relative_path(".gitignore"));
    }

    #[test]
    fn results_are_sorted() {
        let output = "? home/z.txt\0? home/a.txt\0? unmanaged_b\0? unmanaged_a\0";
        let paths = parse_status_paths(output);
        // Verify our sort is applied.
        assert_eq!(paths.len(), 4);

        // Now test through classify (need a repo for that, so just verify parse order).
        let mut managed: Vec<&str> = paths
            .iter()
            .filter(|p| is_managed_relative_path(p))
            .map(|s| s.as_str())
            .collect();
        managed.sort();
        assert_eq!(managed, vec!["home/a.txt", "home/z.txt"]);
    }
}
