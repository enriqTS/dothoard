//! Integration tests for the complete Git workflow.
//!
//! These tests exercise the full Git layer with temporary worktrees and bare
//! remotes, covering: initial push, no-op runs, offline commits, retries,
//! conflicts, hooks, unmanaged changes, and pathspec metacharacters.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use dothoard::app;
use dothoard::backup::manifest::Manifest;
use dothoard::config::SourceConfig;
use dothoard::git::{
    GitCommand, GitRunner, check_auth, classify_ownership, classify_worktree, create_commit,
    has_staged_changes, stage_managed_namespace, sync_with_remote, validate_repository,
    verify_staged_boundaries,
};

/// Sets up a working repository with a bare remote, initial commit, and push.
struct TestGitEnv {
    work_dir: tempfile::TempDir,
    remote_dir: tempfile::TempDir,
    runner: GitRunner,
}

impl TestGitEnv {
    fn new() -> Self {
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

        // Add remote.
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

        Self {
            work_dir,
            remote_dir,
            runner,
        }
    }

    fn worktree(&self) -> &std::path::Path {
        self.work_dir.path()
    }

    fn home_dir(&self) -> std::path::PathBuf {
        self.work_dir.path().join("home")
    }

    /// Clone the remote into a new temporary directory (simulates another machine).
    fn clone_remote(&self) -> tempfile::TempDir {
        let clone_dir = tempfile::tempdir().unwrap();
        let remote_path = self.remote_dir.path().to_str().unwrap();
        let cmd = GitCommand::new(clone_dir.path()).args(["clone", remote_path, "."]);
        self.runner.run(&cmd).unwrap();
        clone_dir
    }
}

// --- Initial push workflow ---

#[test]
fn initial_backup_creates_commit_and_pushes() {
    let env = TestGitEnv::new();

    // Set up managed content.
    let home = env.home_dir();
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::write(home.join(".config/fish/config.fish"), "# fish config").unwrap();
    fs::write(home.join(".bashrc"), "# bashrc").unwrap();

    // Create manifest.
    let manifest = Manifest::from_sources(&[
        SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        },
        SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        },
    ]);
    manifest.save(env.worktree()).unwrap();

    // Stage, verify, commit, push.
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();
    let staged = verify_staged_boundaries(&env.runner, env.worktree()).unwrap();
    assert!(!staged.is_empty());

    let commit = create_commit(&env.runner, env.worktree(), "backup: initial").unwrap();
    assert!(commit.is_some());

    let result = sync_with_remote(&env.runner, env.worktree(), "origin", "main").unwrap();
    assert!(matches!(
        result,
        dothoard::git::SyncResult::Synced | dothoard::git::SyncResult::PushedAfterRebase
    ));

    // Verify the remote got the content.
    let clone = env.clone_remote();
    assert!(clone.path().join("home/.bashrc").exists());
    assert!(clone.path().join("home/.config/fish/config.fish").exists());
    assert!(clone.path().join(app::MANIFEST_FILE_NAME).exists());
}

// --- No-op runs ---

#[test]
fn noop_run_creates_no_commit() {
    let env = TestGitEnv::new();

    // Initial backup.
    let home = env.home_dir();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "content").unwrap();
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();
    create_commit(&env.runner, env.worktree(), "backup: first").unwrap();
    sync_with_remote(&env.runner, env.worktree(), "origin", "main").unwrap();

    // Second run with no changes.
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();
    let commit = create_commit(&env.runner, env.worktree(), "should not happen").unwrap();
    assert_eq!(commit, None);
}

// --- Offline commits and retries ---

#[test]
fn offline_commit_is_preserved_and_pushed_on_retry() {
    let env = TestGitEnv::new();

    // Create content and commit.
    let home = env.home_dir();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "content").unwrap();
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();
    create_commit(&env.runner, env.worktree(), "backup: offline").unwrap();

    // Break the remote to simulate offline.
    let broken_path = env.remote_dir.path().join("HEAD_backup");
    fs::rename(env.remote_dir.path().join("HEAD"), &broken_path).unwrap();

    // Sync should fail (remote broken).
    let result = sync_with_remote(&env.runner, env.worktree(), "origin", "main");
    assert!(result.is_err());

    // Verify local commit is still there.
    let log_cmd = GitCommand::new(env.worktree()).args(["log", "--oneline"]);
    let log = env.runner.run(&log_cmd).unwrap();
    assert!(log.stdout.contains("backup: offline"));

    // Fix remote.
    fs::rename(&broken_path, env.remote_dir.path().join("HEAD")).unwrap();

    // Retry should succeed.
    let result = sync_with_remote(&env.runner, env.worktree(), "origin", "main").unwrap();
    assert!(matches!(
        result,
        dothoard::git::SyncResult::Synced | dothoard::git::SyncResult::PushedAfterRebase
    ));
}

// --- Unmanaged changes block backup ---

#[test]
fn unmanaged_changes_detected_as_blocking() {
    let env = TestGitEnv::new();

    // Create both managed and unmanaged content.
    let home = env.home_dir();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "managed").unwrap();
    fs::write(env.worktree().join("README.md"), "unmanaged").unwrap();

    let status = classify_worktree(&env.runner, env.worktree()).unwrap();
    assert!(status.has_blocking_changes());
    assert!(status.has_recoverable_changes());
}

// --- Hooks ---

#[test]
fn pre_commit_hook_failure_is_reported() {
    let env = TestGitEnv::new();

    // Install failing hook.
    let hooks_dir = env.worktree().join(".git/hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    let hook = hooks_dir.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\necho 'blocked by policy' >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

    // Stage content.
    let home = env.home_dir();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "content").unwrap();
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();

    // Commit should fail with hook error.
    let result = create_commit(&env.runner, env.worktree(), "backup: test");
    assert!(matches!(
        result,
        Err(dothoard::git::CommitError::HookFailed { .. })
    ));

    // Staged changes should still be present.
    assert!(has_staged_changes(&env.runner, env.worktree()).unwrap());
}

// --- Pathspec metacharacters ---

#[test]
fn pathspec_metacharacters_in_filenames_handled_safely() {
    let env = TestGitEnv::new();

    // Create files with glob-like characters in the managed namespace.
    let home = env.home_dir();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("file[1].txt"), "bracket").unwrap();
    fs::write(home.join("star*.conf"), "star").unwrap();
    fs::write(home.join("question?.md"), "question").unwrap();

    // Stage should handle these correctly with :(literal) pathspecs.
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();
    let staged = verify_staged_boundaries(&env.runner, env.worktree()).unwrap();

    assert!(staged.contains(&"home/file[1].txt".to_string()));
    assert!(staged.contains(&"home/star*.conf".to_string()));
    assert!(staged.contains(&"home/question?.md".to_string()));

    // Commit and push should work.
    let commit = create_commit(&env.runner, env.worktree(), "backup: special chars").unwrap();
    assert!(commit.is_some());

    let result = sync_with_remote(&env.runner, env.worktree(), "origin", "main").unwrap();
    assert!(matches!(
        result,
        dothoard::git::SyncResult::Synced | dothoard::git::SyncResult::PushedAfterRebase
    ));

    // Verify remote has the files.
    let clone = env.clone_remote();
    assert!(clone.path().join("home/file[1].txt").exists());
    assert!(clone.path().join("home/star*.conf").exists());
    assert!(clone.path().join("home/question?.md").exists());
}

// --- Conflict handling ---

#[test]
fn conflict_aborts_rebase_and_preserves_local_commit() {
    let env = TestGitEnv::new();

    // Create and push initial content.
    let home = env.home_dir();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "original").unwrap();
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();
    create_commit(&env.runner, env.worktree(), "backup: original").unwrap();
    sync_with_remote(&env.runner, env.worktree(), "origin", "main").unwrap();

    // Create a conflicting change from "another machine".
    let clone = env.clone_remote();
    fs::write(clone.path().join("home/.bashrc"), "upstream change\n").unwrap();
    let cmd = GitCommand::new(clone.path()).args(["add", "--", "home/.bashrc"]);
    env.runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(clone.path()).args(["commit", "-m", "upstream edit"]);
    env.runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(clone.path())
        .args(["push", "origin", "main"])
        .network();
    env.runner.run(&cmd).unwrap();

    // Now make a conflicting local change.
    fs::write(home.join(".bashrc"), "conflicting local change\n").unwrap();
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();
    create_commit(&env.runner, env.worktree(), "backup: local conflict").unwrap();

    // Sync should detect conflict.
    let result = sync_with_remote(&env.runner, env.worktree(), "origin", "main");
    assert!(matches!(result, Err(dothoard::git::SyncError::Conflict)));

    // Verify no rebase-in-progress.
    let git_dir = env.worktree().join(".git");
    assert!(!git_dir.join("rebase-merge").exists());
    assert!(!git_dir.join("rebase-apply").exists());

    // Local commit preserved.
    let log_cmd = GitCommand::new(env.worktree()).args(["log", "-1", "--format=%s"]);
    let log = env.runner.run(&log_cmd).unwrap();
    assert_eq!(log.stdout_trimmed(), "backup: local conflict");
}

// --- Repository validation ---

#[test]
fn validates_repository_structure() {
    let env = TestGitEnv::new();

    let info = validate_repository(&env.runner, env.worktree(), "origin").unwrap();
    assert_eq!(info.branch, "main");
    assert_eq!(info.remote, "origin");
}

// --- Ownership classification ---

#[test]
fn new_repository_classifies_as_new() {
    let env = TestGitEnv::new();

    let state = classify_ownership(env.worktree()).unwrap();
    assert!(matches!(state, dothoard::git::OwnershipState::New));
}

#[test]
fn repository_with_manifest_classifies_as_owned() {
    let env = TestGitEnv::new();

    let manifest = Manifest::from_sources(&[SourceConfig {
        path: ".bashrc".to_string(),
        ignore: vec![],
    }]);
    manifest.save(env.worktree()).unwrap();

    let state = classify_ownership(env.worktree()).unwrap();
    assert!(matches!(state, dothoard::git::OwnershipState::Owned { .. }));
}

// --- Authentication ---

#[test]
fn auth_check_reports_ready_for_local_remote() {
    let env = TestGitEnv::new();

    let status = check_auth(&env.runner, env.worktree(), "origin").unwrap();
    assert!(status.is_ready());
}

// --- Full end-to-end workflow ---

#[test]
fn full_backup_workflow_end_to_end() {
    let env = TestGitEnv::new();

    // 1. Validate repository.
    let info = validate_repository(&env.runner, env.worktree(), "origin").unwrap();
    assert_eq!(info.branch, "main");

    // 2. Classify ownership (new).
    let state = classify_ownership(env.worktree()).unwrap();
    assert!(matches!(state, dothoard::git::OwnershipState::New));

    // 3. Check worktree is clean.
    let wt_status = classify_worktree(&env.runner, env.worktree()).unwrap();
    assert!(wt_status.is_clean());

    // 4. Mirror content into home/ (simulated).
    let home = env.home_dir();
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::write(
        home.join(".config/fish/config.fish"),
        "set -x PATH /usr/bin",
    )
    .unwrap();
    fs::write(home.join(".bashrc"), "alias ls='ls --color'").unwrap();

    // 5. Write manifest.
    let manifest = Manifest::from_sources(&[
        SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["fish_variables".to_string()],
        },
        SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        },
    ]);
    manifest.save(env.worktree()).unwrap();

    // 6. Stage managed namespace.
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();

    // 7. Verify staged boundaries.
    let staged = verify_staged_boundaries(&env.runner, env.worktree()).unwrap();
    assert!(staged.len() >= 3); // bashrc, config.fish, manifest

    // 8. Commit.
    let commit = create_commit(&env.runner, env.worktree(), "backup(test): 2026-07-21")
        .unwrap()
        .unwrap();
    assert_eq!(commit.sha.len(), 40);

    // 9. Sync with remote.
    let sync_result = sync_with_remote(&env.runner, env.worktree(), "origin", "main").unwrap();
    assert!(matches!(
        sync_result,
        dothoard::git::SyncResult::Synced | dothoard::git::SyncResult::PushedAfterRebase
    ));

    // 10. Verify remote has everything.
    let clone = env.clone_remote();
    assert!(clone.path().join("home/.bashrc").exists());
    assert!(clone.path().join("home/.config/fish/config.fish").exists());
    assert!(clone.path().join(app::MANIFEST_FILE_NAME).exists());

    // 11. Second run with no changes produces no commit.
    stage_managed_namespace(&env.runner, env.worktree()).unwrap();
    let no_commit = create_commit(&env.runner, env.worktree(), "should skip").unwrap();
    assert_eq!(no_commit, None);
}
