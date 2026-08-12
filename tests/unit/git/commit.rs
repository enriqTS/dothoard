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
