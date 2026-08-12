use std::fs;
use std::time::Duration;

use super::*;

fn init_repo_with_local_remote() -> (tempfile::TempDir, tempfile::TempDir, GitRunner) {
    let remote_dir = tempfile::tempdir().unwrap();
    let work_dir = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(Duration::from_secs(10));

    // Create bare remote.
    let cmd = GitCommand::new(remote_dir.path()).args(["init", "--bare", "--initial-branch=main"]);
    runner.run(&cmd).unwrap();

    // Create working repo.
    let cmd = GitCommand::new(work_dir.path()).args(["init", "--initial-branch=main"]);
    runner.run(&cmd).unwrap();

    let remote_path = remote_dir.path().to_str().unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["remote", "add", "origin", remote_path]);
    runner.run(&cmd).unwrap();

    // Initial commit and push.
    let cmd = GitCommand::new(work_dir.path()).args(["commit", "--allow-empty", "-m", "initial"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(work_dir.path())
        .args(["push", "-u", "origin", "main"])
        .network();
    runner.run(&cmd).unwrap();

    (work_dir, remote_dir, runner)
}

#[test]
fn reports_ready_for_accessible_local_remote() {
    let (work_dir, _remote_dir, runner) = init_repo_with_local_remote();

    let status = check_auth(&runner, work_dir.path(), "origin").unwrap();
    assert_eq!(status, AuthStatus::Ready);
    assert!(status.is_ready());
}

#[test]
fn reports_not_ready_for_nonexistent_remote_path() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(Duration::from_secs(10));

    let cmd = GitCommand::new(tmp.path()).args(["init", "--initial-branch=main"]);
    runner.run(&cmd).unwrap();
    let cmd =
        GitCommand::new(tmp.path()).args(["remote", "add", "origin", "/nonexistent/path/repo.git"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(tmp.path()).args(["commit", "--allow-empty", "-m", "init"]);
    runner.run(&cmd).unwrap();

    let status = check_auth(&runner, tmp.path(), "origin").unwrap();
    assert!(matches!(status, AuthStatus::NotReady { .. }));
    assert!(!status.is_ready());
}

#[test]
fn auth_status_display() {
    let ready = AuthStatus::Ready;
    assert!(ready.to_string().contains("accessible"));

    let not_ready = AuthStatus::NotReady {
        reason: "connection refused".to_string(),
    };
    assert!(not_ready.to_string().contains("connection refused"));
}

#[test]
fn reports_not_ready_for_deleted_remote() {
    let (work_dir, remote_dir, runner) = init_repo_with_local_remote();

    // Delete the remote repository.
    fs::remove_dir_all(remote_dir.path()).unwrap();

    let status = check_auth(&runner, work_dir.path(), "origin").unwrap();
    assert!(matches!(status, AuthStatus::NotReady { .. }));
}
