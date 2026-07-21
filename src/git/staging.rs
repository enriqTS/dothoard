//! Restricted staging and staged-boundary verification.
//!
//! This module stages only the managed namespace (`home/` and the manifest file)
//! using literal pathspecs with `--` separation to prevent pathspec
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
    let home_dir = worktree.join(mapping::HOME_DIR_NAME);
    let manifest_file = worktree.join(app::MANIFEST_FILE_NAME);

    let mut args: Vec<String> = vec!["add".to_string(), "--all".to_string(), "--".to_string()];

    // Only include paths that exist on disk or are tracked (for deletions).
    let home_exists = home_dir.exists();
    let manifest_exists = manifest_file.exists();

    // Check if home/ or manifest are tracked (for deletion staging).
    let home_tracked = is_path_tracked(runner, worktree, mapping::HOME_DIR_NAME)?;
    let manifest_tracked = is_path_tracked(runner, worktree, app::MANIFEST_FILE_NAME)?;

    if home_exists || home_tracked {
        args.push(format!(":(literal){}", mapping::HOME_DIR_NAME));
    }
    if manifest_exists || manifest_tracked {
        args.push(format!(":(literal){}", app::MANIFEST_FILE_NAME));
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
/// Uses `git diff --cached --name-only -z` to get the complete list of staged
/// paths (NUL-delimited for safe parsing), then checks each against the
/// managed namespace boundaries.
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
    let cmd = GitCommand::new(worktree).args(["diff", "--cached", "--name-only", "-z"]);
    let output = runner.run(&cmd)?;

    let staged_paths: Vec<String> = output
        .stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    let unmanaged: Vec<String> = staged_paths
        .iter()
        .filter(|path| !is_managed_relative_path(path))
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
fn is_managed_relative_path(path: &str) -> bool {
    let home_prefix = format!("{}/", mapping::HOME_DIR_NAME);
    path.starts_with(&home_prefix)
        || path == mapping::HOME_DIR_NAME
        || path == app::MANIFEST_FILE_NAME
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
    fn stages_managed_files() {
        let (tmp, runner) = init_test_repo();

        // Create managed content.
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".config")).unwrap();
        fs::write(home.join(".bashrc"), "# bash").unwrap();
        fs::write(home.join(".config/test.conf"), "key=value").unwrap();
        fs::write(
            tmp.path().join(app::MANIFEST_FILE_NAME),
            "format = \"dothoard-manifest\"\nversion = 1\n",
        )
        .unwrap();

        stage_managed_namespace(&runner, tmp.path()).unwrap();

        // Verify all files are staged.
        let staged = verify_staged_boundaries(&runner, tmp.path()).unwrap();
        assert!(staged.contains(&"home/.bashrc".to_string()));
        assert!(staged.contains(&"home/.config/test.conf".to_string()));
        assert!(staged.contains(&app::MANIFEST_FILE_NAME.to_string()));
    }

    #[test]
    fn does_not_stage_unmanaged_files() {
        let (tmp, runner) = init_test_repo();

        // Create managed and unmanaged content.
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "# bash").unwrap();
        fs::write(tmp.path().join("README.md"), "hello").unwrap();
        fs::write(tmp.path().join("notes.txt"), "my notes").unwrap();

        stage_managed_namespace(&runner, tmp.path()).unwrap();

        // Verify only managed files are staged.
        let staged = verify_staged_boundaries(&runner, tmp.path()).unwrap();
        assert!(staged.contains(&"home/.bashrc".to_string()));
        assert!(!staged.contains(&"README.md".to_string()));
        assert!(!staged.contains(&"notes.txt".to_string()));
    }

    #[test]
    fn stages_deletions() {
        let (tmp, runner) = init_test_repo();

        // Create, commit, then delete a managed file.
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "v1").unwrap();

        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/.bashrc"]);
        runner.run(&cmd).unwrap();
        let cmd = GitCommand::new(tmp.path()).args(["commit", "-m", "add bashrc"]);
        runner.run(&cmd).unwrap();

        // Delete the file.
        fs::remove_file(home.join(".bashrc")).unwrap();

        // Stage should pick up the deletion.
        stage_managed_namespace(&runner, tmp.path()).unwrap();

        let staged = verify_staged_boundaries(&runner, tmp.path()).unwrap();
        assert!(staged.contains(&"home/.bashrc".to_string()));
    }

    #[test]
    fn verify_rejects_externally_staged_unmanaged_file() {
        let (tmp, runner) = init_test_repo();

        // Manually stage an unmanaged file (simulating a bad state).
        fs::write(tmp.path().join("evil.txt"), "data").unwrap();
        let cmd = GitCommand::new(tmp.path()).args(["add", "--", "evil.txt"]);
        runner.run(&cmd).unwrap();

        let result = verify_staged_boundaries(&runner, tmp.path());
        assert!(matches!(result, Err(StagingError::UnmanagedStaged { .. })));

        if let Err(StagingError::UnmanagedStaged { paths }) = result {
            assert!(paths.contains(&"evil.txt".to_string()));
        }
    }

    #[test]
    fn verify_passes_for_only_managed_paths() {
        let (tmp, runner) = init_test_repo();

        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "content").unwrap();

        stage_managed_namespace(&runner, tmp.path()).unwrap();

        let staged = verify_staged_boundaries(&runner, tmp.path()).unwrap();
        assert!(!staged.is_empty());
        for path in &staged {
            assert!(is_managed_relative_path(path), "unmanaged: {path}");
        }
    }

    #[test]
    fn has_staged_changes_returns_false_for_clean_index() {
        let (tmp, runner) = init_test_repo();

        let has_changes = has_staged_changes(&runner, tmp.path()).unwrap();
        assert!(!has_changes);
    }

    #[test]
    fn has_staged_changes_returns_true_after_staging() {
        let (tmp, runner) = init_test_repo();

        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "content").unwrap();

        stage_managed_namespace(&runner, tmp.path()).unwrap();

        let has_changes = has_staged_changes(&runner, tmp.path()).unwrap();
        assert!(has_changes);
    }

    #[test]
    fn empty_managed_namespace_stages_nothing() {
        let (tmp, runner) = init_test_repo();

        // No home/ or manifest exists; staging should succeed without error.
        // (git add with non-existent paths just does nothing for directories)
        stage_managed_namespace(&runner, tmp.path()).unwrap();

        let has_changes = has_staged_changes(&runner, tmp.path()).unwrap();
        assert!(!has_changes);
    }

    #[test]
    fn handles_filenames_with_special_characters() {
        let (tmp, runner) = init_test_repo();

        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        // Create files with glob-like characters.
        fs::write(home.join("file[1].txt"), "content").unwrap();
        fs::write(home.join("star*.conf"), "data").unwrap();

        stage_managed_namespace(&runner, tmp.path()).unwrap();

        let staged = verify_staged_boundaries(&runner, tmp.path()).unwrap();
        assert!(staged.contains(&"home/file[1].txt".to_string()));
        assert!(staged.contains(&"home/star*.conf".to_string()));
    }

    #[test]
    fn verify_returns_empty_vec_when_no_staged_changes() {
        let (tmp, runner) = init_test_repo();

        let staged = verify_staged_boundaries(&runner, tmp.path()).unwrap();
        assert!(staged.is_empty());
    }

    #[test]
    fn is_managed_recognizes_correct_paths() {
        assert!(is_managed_relative_path("home/.bashrc"));
        assert!(is_managed_relative_path("home/.config/fish/config.fish"));
        assert!(is_managed_relative_path(app::MANIFEST_FILE_NAME));
        assert!(!is_managed_relative_path("README.md"));
        assert!(!is_managed_relative_path("src/main.rs"));
        assert!(!is_managed_relative_path("homepage/index.html"));
    }
}
