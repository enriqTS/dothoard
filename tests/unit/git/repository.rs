use std::fs;
use std::time::Duration;

use super::*;

/// Create a temporary git repository for testing.
fn init_test_repo() -> (tempfile::TempDir, GitRunner) {
    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(Duration::from_secs(10));

    // Initialize with a branch name to avoid issues with default branch config.
    let cmd = GitCommand::new(tmp.path()).args(["init", "--initial-branch=main"]);
    runner.run(&cmd).unwrap();

    // Create an initial commit so HEAD is valid.
    let cmd = GitCommand::new(tmp.path()).args(["commit", "--allow-empty", "-m", "initial"]);
    runner.run(&cmd).unwrap();

    // Add a remote.
    let cmd = GitCommand::new(tmp.path()).args([
        "remote",
        "add",
        "origin",
        "https://example.com/repo.git",
    ]);
    runner.run(&cmd).unwrap();

    (tmp, runner)
}

#[test]
fn validates_well_formed_repository() {
    let (tmp, runner) = init_test_repo();

    let info = validate_repository(&runner, tmp.path(), "origin").unwrap();

    assert_eq!(info.worktree, tmp.path().canonicalize().unwrap());
    assert_eq!(info.branch, "main");
    assert_eq!(info.remote, "origin");
}

#[test]
fn rejects_non_repository_path() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(Duration::from_secs(10));

    let result = validate_repository(&runner, tmp.path(), "origin");

    assert!(matches!(
        result,
        Err(RepositoryError::NotARepository { .. })
    ));
}

#[test]
fn rejects_bare_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(Duration::from_secs(10));

    let cmd = GitCommand::new(tmp.path()).args(["init", "--bare"]);
    runner.run(&cmd).unwrap();

    let cmd = GitCommand::new(tmp.path()).args(["remote", "add", "origin", "/tmp/fake-remote.git"]);
    runner.run(&cmd).unwrap();

    let result = validate_repository(&runner, tmp.path(), "origin");

    assert!(matches!(
        result,
        Err(RepositoryError::BareRepository { .. })
    ));
}

#[test]
fn rejects_detached_head() {
    let (tmp, runner) = init_test_repo();

    // Detach HEAD by checking out the commit directly.
    let cmd = GitCommand::new(tmp.path()).args(["checkout", "--detach", "HEAD"]);
    runner.run(&cmd).unwrap();

    let result = validate_repository(&runner, tmp.path(), "origin");

    assert!(matches!(result, Err(RepositoryError::DetachedHead)));
}

#[test]
fn rejects_missing_remote() {
    let (tmp, runner) = init_test_repo();

    let result = validate_repository(&runner, tmp.path(), "nonexistent");

    assert!(matches!(
        result,
        Err(RepositoryError::RemoteNotFound { ref remote }) if remote == "nonexistent"
    ));
}

#[test]
fn rejects_in_progress_merge() {
    let (tmp, runner) = init_test_repo();

    // Simulate an in-progress merge by creating MERGE_HEAD.
    let git_dir = tmp.path().join(".git");
    fs::write(
        git_dir.join("MERGE_HEAD"),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .unwrap();

    let result = validate_repository(&runner, tmp.path(), "origin");

    assert!(matches!(
        result,
        Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::Merge
        })
    ));
}

#[test]
fn rejects_in_progress_rebase_merge() {
    let (tmp, runner) = init_test_repo();

    let git_dir = tmp.path().join(".git");
    fs::create_dir(git_dir.join("rebase-merge")).unwrap();

    let result = validate_repository(&runner, tmp.path(), "origin");

    assert!(matches!(
        result,
        Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::Rebase
        })
    ));
}

#[test]
fn rejects_in_progress_rebase_apply() {
    let (tmp, runner) = init_test_repo();

    let git_dir = tmp.path().join(".git");
    fs::create_dir(git_dir.join("rebase-apply")).unwrap();

    let result = validate_repository(&runner, tmp.path(), "origin");

    assert!(matches!(
        result,
        Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::Rebase
        })
    ));
}

#[test]
fn rejects_in_progress_cherry_pick() {
    let (tmp, runner) = init_test_repo();

    let git_dir = tmp.path().join(".git");
    fs::write(
        git_dir.join("CHERRY_PICK_HEAD"),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .unwrap();

    let result = validate_repository(&runner, tmp.path(), "origin");

    assert!(matches!(
        result,
        Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::CherryPick
        })
    ));
}

#[test]
fn rejects_in_progress_bisect() {
    let (tmp, runner) = init_test_repo();

    let git_dir = tmp.path().join(".git");
    fs::write(git_dir.join("BISECT_LOG"), "# some bisect log\n").unwrap();

    let result = validate_repository(&runner, tmp.path(), "origin");

    assert!(matches!(
        result,
        Err(RepositoryError::BlockingOperation {
            operation: BlockingOperation::Bisect
        })
    ));
}

#[test]
fn blocking_operation_display() {
    assert_eq!(BlockingOperation::Merge.to_string(), "merge");
    assert_eq!(BlockingOperation::Rebase.to_string(), "rebase");
    assert_eq!(BlockingOperation::CherryPick.to_string(), "cherry-pick");
    assert_eq!(BlockingOperation::Bisect.to_string(), "bisect");
}

#[test]
fn accepts_repository_with_different_branch() {
    let (tmp, runner) = init_test_repo();

    // Create and switch to a different branch.
    let cmd = GitCommand::new(tmp.path()).args(["checkout", "-b", "develop"]);
    runner.run(&cmd).unwrap();

    let info = validate_repository(&runner, tmp.path(), "origin").unwrap();
    assert_eq!(info.branch, "develop");
}

#[test]
fn accepts_repository_from_subdirectory() {
    let (tmp, runner) = init_test_repo();

    // Create a subdirectory and validate from there.
    let subdir = tmp.path().join("some").join("subdir");
    fs::create_dir_all(&subdir).unwrap();

    let info = validate_repository(&runner, &subdir, "origin").unwrap();
    // Should still resolve to the repo root.
    assert_eq!(info.worktree, tmp.path().canonicalize().unwrap());
}
