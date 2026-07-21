//! Integration tests for the complete mirror pipeline (planner + executor).
//!
//! These tests exercise the full flow: set up source files under a temporary
//! home directory, run the planner to detect changes, then run the executor
//! to apply them. They verify that the destination reflects source truth after
//! each run.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use config_sync::backup::changeset::ChangeSet;
use config_sync::backup::executor::{MirrorResult, execute_mirror};
use config_sync::backup::planner::{PlanInputs, plan_backup};
use config_sync::config::SourceConfig;

/// Helper that runs the full plan+execute pipeline and returns the result.
fn mirror(home: &Path, repo: &Path, sources: &[SourceConfig]) -> MirrorResult {
    let inputs = PlanInputs {
        home,
        repository: repo,
        sources,
    };
    let changeset = plan_backup(&inputs).expect("planner should succeed");
    execute_mirror(home, repo, sources, &changeset)
        .expect("executor should not fail with preflight error")
}

/// Helper to read a changeset without executing.
fn plan(home: &Path, repo: &Path, sources: &[SourceConfig]) -> ChangeSet {
    let inputs = PlanInputs {
        home,
        repository: repo,
        sources,
    };
    plan_backup(&inputs).expect("planner should succeed")
}

fn source(path: &str, ignore: &[&str]) -> SourceConfig {
    SourceConfig {
        path: path.to_string(),
        ignore: ignore.iter().map(|s| s.to_string()).collect(),
    }
}

// =============================================================================
// Initial copies
// =============================================================================

#[test]
fn initial_backup_copies_all_source_files() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish/functions")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "set PATH").unwrap();
    fs::write(
        home.join(".config/fish/functions/hello.fish"),
        "function hello",
    )
    .unwrap();

    let sources = vec![source(".config/fish", &[])];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 2);
    assert_eq!(
        fs::read_to_string(repo.join("home/.config/fish/config.fish")).unwrap(),
        "set PATH"
    );
    assert_eq!(
        fs::read_to_string(repo.join("home/.config/fish/functions/hello.fish")).unwrap(),
        "function hello"
    );
}

#[test]
fn initial_backup_single_file_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".bashrc"), "# bash config").unwrap();

    let sources = vec![source(".bashrc", &[])];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);
    assert_eq!(
        fs::read_to_string(repo.join("home/.bashrc")).unwrap(),
        "# bash config"
    );
}

#[test]
fn initial_backup_multiple_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(home.join(".config/waybar")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "fish").unwrap();
    fs::write(home.join(".config/waybar/config"), "waybar").unwrap();
    fs::write(home.join(".bashrc"), "bash").unwrap();

    let sources = vec![
        source(".config/fish", &[]),
        source(".config/waybar", &[]),
        source(".bashrc", &[]),
    ];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 3);
    assert!(repo.join("home/.config/fish/config.fish").exists());
    assert!(repo.join("home/.config/waybar/config").exists());
    assert!(repo.join("home/.bashrc").exists());
}

// =============================================================================
// No-op (unchanged)
// =============================================================================

#[test]
fn noop_when_nothing_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "content").unwrap();

    let sources = vec![source(".config/fish", &[])];

    // First run — initial copy.
    mirror(&home, &repo, &sources);

    // Second run — nothing changed.
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 0);
    assert_eq!(result.deletions_completed, 0);
}

// =============================================================================
// Modifications
// =============================================================================

#[test]
fn modified_file_is_updated() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "version 1").unwrap();
    let sources = vec![source(".config/fish", &[])];

    // Initial backup.
    mirror(&home, &repo, &sources);

    // Modify the source.
    fs::write(home.join(".config/fish/config.fish"), "version 2").unwrap();

    // Second run detects modification.
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);
    assert_eq!(
        fs::read_to_string(repo.join("home/.config/fish/config.fish")).unwrap(),
        "version 2"
    );
}

#[test]
fn executable_bit_change_is_applied() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join("bin/script.sh"), "#!/bin/bash").unwrap();

    let sources = vec![source("bin", &[])];

    // Initial — non-executable.
    mirror(&home, &repo, &sources);
    let mode = fs::metadata(repo.join("home/bin/script.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0); // Not executable.

    // Make it executable.
    fs::set_permissions(
        home.join("bin/script.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);
    let mode = fs::metadata(repo.join("home/bin/script.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert!(mode & 0o111 != 0); // Now executable.
}

// =============================================================================
// Deletions
// =============================================================================

#[test]
fn removed_source_file_is_deleted_from_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "keep").unwrap();
    fs::write(home.join(".config/fish/old.fish"), "remove me").unwrap();

    let sources = vec![source(".config/fish", &[])];

    // Initial backup copies both files.
    mirror(&home, &repo, &sources);
    assert!(repo.join("home/.config/fish/old.fish").exists());

    // Remove source file.
    fs::remove_file(home.join(".config/fish/old.fish")).unwrap();

    // Next run detects deletion.
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.deletions_completed, 1);
    assert!(!repo.join("home/.config/fish/old.fish").exists());
    // Other file still there.
    assert!(repo.join("home/.config/fish/config.fish").exists());
}

#[test]
fn empty_directories_cleaned_up_after_all_files_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish/functions")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/functions/only.fish"), "fn").unwrap();

    let sources = vec![source(".config/fish", &[])];
    mirror(&home, &repo, &sources);

    // Remove the only file in the subdirectory.
    fs::remove_file(home.join(".config/fish/functions/only.fish")).unwrap();

    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    // The functions/ directory should be cleaned up.
    assert!(!repo.join("home/.config/fish/functions").exists());
}

// =============================================================================
// Ignore rules
// =============================================================================

#[test]
fn ignored_files_never_enter_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "config").unwrap();
    fs::write(home.join(".config/fish/debug.log"), "log data").unwrap();
    fs::write(home.join(".config/fish/fish_variables"), "vars").unwrap();

    let sources = vec![source(".config/fish", &["*.log", "fish_variables"])];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1); // Only config.fish
    assert!(repo.join("home/.config/fish/config.fish").exists());
    assert!(!repo.join("home/.config/fish/debug.log").exists());
    assert!(!repo.join("home/.config/fish/fish_variables").exists());
}

#[test]
fn newly_ignored_tracked_file_is_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/app")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/app/config.toml"), "config").unwrap();
    fs::write(home.join(".config/app/secret.key"), "key data").unwrap();

    // First backup: no ignore rules.
    let sources_v1 = vec![source(".config/app", &[])];
    mirror(&home, &repo, &sources_v1);
    assert!(repo.join("home/.config/app/secret.key").exists());

    // Second backup: now ignoring *.key.
    let sources_v2 = vec![source(".config/app", &["*.key"])];
    let result = mirror(&home, &repo, &sources_v2);

    assert!(result.may_publish);
    assert_eq!(result.deletions_completed, 1);
    assert!(!repo.join("home/.config/app/secret.key").exists());
    assert!(repo.join("home/.config/app/config.toml").exists());
}

// =============================================================================
// Missing source roots
// =============================================================================

#[test]
fn missing_source_root_preserves_existing_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "config").unwrap();

    let sources = vec![source(".config/fish", &[])];
    mirror(&home, &repo, &sources);

    // Remove the source directory entirely.
    fs::remove_dir_all(home.join(".config/fish")).unwrap();

    // Next run: source is missing, but backup is preserved.
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.deletions_completed, 0);
    // Backup is still there.
    assert!(repo.join("home/.config/fish/config.fish").exists());
}

#[test]
fn mixed_present_and_missing_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "fish config").unwrap();
    fs::write(home.join(".bashrc"), "bash config").unwrap();

    let sources = vec![
        source(".config/fish", &[]),
        source(".config/missing", &[]), // Doesn't exist.
        source(".bashrc", &[]),
    ];

    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    // Only two sources have content.
    assert_eq!(result.copies_completed, 2);
    assert!(repo.join("home/.config/fish/config.fish").exists());
    assert!(repo.join("home/.bashrc").exists());
}

// =============================================================================
// Symlinks
// =============================================================================

#[test]
fn symlink_is_copied_as_link() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/links")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    std::os::unix::fs::symlink("/usr/share/something", home.join(".config/links/thing")).unwrap();

    let sources = vec![source(".config/links", &[])];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);

    let dest = repo.join("home/.config/links/thing");
    let meta = fs::symlink_metadata(&dest).unwrap();
    assert!(meta.file_type().is_symlink());
    assert_eq!(
        fs::read_link(&dest).unwrap(),
        PathBuf::from("/usr/share/something")
    );
}

#[test]
fn symlink_target_change_is_updated() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/links")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    std::os::unix::fs::symlink("/old/target", home.join(".config/links/link")).unwrap();
    let sources = vec![source(".config/links", &[])];

    // Initial backup.
    mirror(&home, &repo, &sources);

    // Change the symlink target.
    fs::remove_file(home.join(".config/links/link")).unwrap();
    std::os::unix::fs::symlink("/new/target", home.join(".config/links/link")).unwrap();

    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);
    assert_eq!(
        fs::read_link(repo.join("home/.config/links/link")).unwrap(),
        PathBuf::from("/new/target")
    );
}

#[test]
fn dangling_symlink_is_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    std::os::unix::fs::symlink("/nonexistent/target", home.join(".config/dangling")).unwrap();

    let sources = vec![source(".config", &[])];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    let dest = repo.join("home/.config/dangling");
    assert!(
        fs::symlink_metadata(&dest)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_link(&dest).unwrap(),
        PathBuf::from("/nonexistent/target")
    );
}

// =============================================================================
// Boundary enforcement (symlinked destination parents)
// =============================================================================

#[test]
fn symlinked_destination_parent_blocks_mirror() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    let escape = tmp.path().join("escape");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(repo.join("home")).unwrap();
    fs::create_dir_all(&escape).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "data").unwrap();

    // Symlink inside the managed namespace.
    std::os::unix::fs::symlink(&escape, repo.join("home").join(".config")).unwrap();

    let sources = vec![source(".config/fish", &[])];
    let inputs = PlanInputs {
        home: &home,
        repository: &repo,
        sources: &sources,
    };
    let changeset = plan_backup(&inputs).unwrap();
    let result = execute_mirror(&home, &repo, &sources, &changeset);

    // Should fail at preflight.
    assert!(result.is_err());
}

// =============================================================================
// Recovery from interrupted runs
// =============================================================================

#[test]
fn recovery_after_partial_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(repo.join("home/.config/fish")).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "correct").unwrap();
    // Simulate a stale file from an interrupted previous run.
    fs::write(
        repo.join("home/.config/fish/config.fish"),
        "stale from crash",
    )
    .unwrap();

    let sources = vec![source(".config/fish", &[])];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    // The planner detects the content difference and the executor corrects it.
    assert_eq!(result.copies_completed, 1);
    assert_eq!(
        fs::read_to_string(repo.join("home/.config/fish/config.fish")).unwrap(),
        "correct"
    );
}

#[test]
fn recovery_after_failed_deletion() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(repo.join("home/.config/fish")).unwrap();

    // Source has one file, destination has an extra leftover.
    fs::write(home.join(".config/fish/config.fish"), "keep").unwrap();
    fs::write(repo.join("home/.config/fish/config.fish"), "keep").unwrap();
    fs::write(
        repo.join("home/.config/fish/leftover.fish"),
        "should be gone",
    )
    .unwrap();

    let sources = vec![source(".config/fish", &[])];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);
    assert_eq!(result.deletions_completed, 1);
    assert!(!repo.join("home/.config/fish/leftover.fish").exists());
}

// =============================================================================
// Manifest
// =============================================================================

#[test]
fn manifest_is_created_on_successful_mirror() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    fs::write(home.join(".config/fish/config.fish"), "config").unwrap();

    let sources = vec![source(".config/fish", &["*.log"])];
    let result = mirror(&home, &repo, &sources);

    assert!(result.may_publish);

    let manifest_path = repo.join(".config-sync-manifest.toml");
    assert!(manifest_path.exists());

    let content = fs::read_to_string(&manifest_path).unwrap();
    assert!(content.contains("config-sync-manifest"));
    assert!(content.contains(".config/fish"));
    assert!(content.contains("*.log"));
}

// =============================================================================
// Full round-trip scenario
// =============================================================================

#[test]
fn full_lifecycle_add_modify_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(home.join(".config/app")).unwrap();
    fs::create_dir_all(&repo).unwrap();

    let sources = vec![source(".config/app", &[])];

    // --- Run 1: Initial backup ---
    fs::write(home.join(".config/app/a.txt"), "aaa").unwrap();
    fs::write(home.join(".config/app/b.txt"), "bbb").unwrap();
    let r1 = mirror(&home, &repo, &sources);
    assert!(r1.may_publish);
    assert_eq!(r1.copies_completed, 2);

    // --- Run 2: No-op ---
    let r2 = mirror(&home, &repo, &sources);
    assert!(r2.may_publish);
    assert_eq!(r2.copies_completed, 0);
    assert_eq!(r2.deletions_completed, 0);

    // --- Run 3: Modify a.txt, add c.txt ---
    fs::write(home.join(".config/app/a.txt"), "aaa modified").unwrap();
    fs::write(home.join(".config/app/c.txt"), "ccc").unwrap();
    let r3 = mirror(&home, &repo, &sources);
    assert!(r3.may_publish);
    assert_eq!(r3.copies_completed, 2); // a.txt modified + c.txt added
    assert_eq!(
        fs::read_to_string(repo.join("home/.config/app/a.txt")).unwrap(),
        "aaa modified"
    );
    assert!(repo.join("home/.config/app/c.txt").exists());

    // --- Run 4: Delete b.txt from source ---
    fs::remove_file(home.join(".config/app/b.txt")).unwrap();
    let r4 = mirror(&home, &repo, &sources);
    assert!(r4.may_publish);
    assert_eq!(r4.deletions_completed, 1);
    assert!(!repo.join("home/.config/app/b.txt").exists());

    // --- Run 5: Verify final state ---
    let changeset = plan(&home, &repo, &sources);
    assert!(changeset.is_empty());
}
