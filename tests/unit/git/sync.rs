use std::fs;
use std::time::Duration;

use super::*;

/// Create a repo with a bare remote for sync testing.
fn init_repo_with_remote() -> (tempfile::TempDir, tempfile::TempDir, GitRunner) {
    let remote_dir = tempfile::tempdir().unwrap();
    let work_dir = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(Duration::from_secs(10));

    // Create bare remote.
    let cmd = GitCommand::new(remote_dir.path()).args(["init", "--bare", "--initial-branch=main"]);
    runner.run(&cmd).unwrap();

    // Create working repo.
    let cmd = GitCommand::new(work_dir.path()).args(["init", "--initial-branch=main"]);
    runner.run(&cmd).unwrap();

    // Add remote.
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
fn sync_up_to_date_when_nothing_to_push() {
    let (work_dir, _remote_dir, runner) = init_repo_with_remote();

    let result = sync_with_remote(&runner, work_dir.path(), "origin", "main").unwrap();
    assert_eq!(result, SyncResult::UpToDate);
}

#[test]
fn sync_pushes_local_commit() {
    let (work_dir, _remote_dir, runner) = init_repo_with_remote();

    // Create a local commit.
    let home = work_dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "content").unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "home/.bashrc"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "backup"]);
    runner.run(&cmd).unwrap();

    let result = sync_with_remote(&runner, work_dir.path(), "origin", "main").unwrap();
    assert!(matches!(
        result,
        SyncResult::Synced | SyncResult::PushedAfterRebase
    ));
}

#[test]
fn sync_pulls_upstream_changes() {
    let (work_dir, remote_dir, runner) = init_repo_with_remote();

    // Create a second clone that pushes a commit to the remote.
    let other_dir = tempfile::tempdir().unwrap();
    let remote_path = remote_dir.path().to_str().unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["clone", remote_path, "."]);
    runner.run(&cmd).unwrap();
    fs::write(other_dir.path().join("other.txt"), "data").unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["add", "--", "other.txt"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["commit", "-m", "upstream change"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(other_dir.path())
        .args(["push", "origin", "main"])
        .network();
    runner.run(&cmd).unwrap();

    // Now sync our working repo — it should pull the upstream change.
    let result = sync_with_remote(&runner, work_dir.path(), "origin", "main").unwrap();
    assert_eq!(result, SyncResult::UpToDate);

    // Verify the upstream file is now in our worktree.
    assert!(work_dir.path().join("other.txt").exists());
}

#[test]
fn sync_rebases_local_on_upstream() {
    let (work_dir, remote_dir, runner) = init_repo_with_remote();

    // Push from another clone.
    let other_dir = tempfile::tempdir().unwrap();
    let remote_path = remote_dir.path().to_str().unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["clone", remote_path, "."]);
    runner.run(&cmd).unwrap();
    fs::write(other_dir.path().join("upstream.txt"), "upstream").unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["add", "--", "upstream.txt"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["commit", "-m", "upstream"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(other_dir.path())
        .args(["push", "origin", "main"])
        .network();
    runner.run(&cmd).unwrap();

    // Create a local commit (non-conflicting).
    let home = work_dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "local").unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "home/.bashrc"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "local backup"]);
    runner.run(&cmd).unwrap();

    // Sync should rebase our commit on top of upstream and push.
    let result = sync_with_remote(&runner, work_dir.path(), "origin", "main").unwrap();
    assert_eq!(result, SyncResult::PushedAfterRebase);

    // Both changes should be present.
    assert!(work_dir.path().join("upstream.txt").exists());
    assert!(work_dir.path().join("home/.bashrc").exists());
}

#[test]
fn sync_handles_conflict_by_aborting_rebase() {
    let (work_dir, remote_dir, runner) = init_repo_with_remote();

    // Create a file in our repo and push.
    fs::write(work_dir.path().join("conflict.txt"), "local version").unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "conflict.txt"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "local"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(work_dir.path())
        .args(["push", "origin", "main"])
        .network();
    runner.run(&cmd).unwrap();

    // Push a conflicting change from another clone.
    let other_dir = tempfile::tempdir().unwrap();
    let remote_path = remote_dir.path().to_str().unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["clone", remote_path, "."]);
    runner.run(&cmd).unwrap();
    fs::write(other_dir.path().join("conflict.txt"), "upstream version\n").unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["add", "--", "conflict.txt"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(other_dir.path()).args(["commit", "-m", "upstream conflict"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(other_dir.path())
        .args(["push", "--force", "origin", "main"])
        .network();
    runner.run(&cmd).unwrap();

    // Now create a conflicting local commit (different content in same file).
    fs::write(work_dir.path().join("conflict.txt"), "new local version\n").unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "conflict.txt"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "conflicting local"]);
    runner.run(&cmd).unwrap();

    // Sync should detect conflict, abort rebase, and preserve local commit.
    let result = sync_with_remote(&runner, work_dir.path(), "origin", "main");
    assert!(matches!(result, Err(SyncError::Conflict)));

    // Verify we're not in a rebase state.
    let git_dir = work_dir.path().join(".git");
    assert!(!git_dir.join("rebase-merge").exists());
    assert!(!git_dir.join("rebase-apply").exists());

    // Verify our local commit is still there.
    let log_cmd = GitCommand::new(work_dir.path()).args(["log", "-1", "--format=%s"]);
    let log_output = runner.run(&log_cmd).unwrap();
    assert_eq!(log_output.stdout_trimmed(), "conflicting local");
}

#[test]
fn has_unpushed_returns_false_when_synced() {
    let (work_dir, _remote_dir, runner) = init_repo_with_remote();

    let result = has_unpushed_commits(&runner, work_dir.path(), "origin", "main").unwrap();
    assert!(!result);
}

#[test]
fn has_unpushed_returns_true_with_local_commit() {
    let (work_dir, _remote_dir, runner) = init_repo_with_remote();

    fs::write(work_dir.path().join("file.txt"), "data").unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["add", "--", "file.txt"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(work_dir.path()).args(["commit", "-m", "local"]);
    runner.run(&cmd).unwrap();

    let result = has_unpushed_commits(&runner, work_dir.path(), "origin", "main").unwrap();
    assert!(result);
}

#[test]
fn network_error_detection() {
    assert!(is_network_error(
        "fatal: Could not resolve host example.com"
    ));
    assert!(is_network_error(
        "ssh: connect to host example.com port 22: Connection refused"
    ));
    assert!(is_network_error(
        "fatal: unable to access 'https://example.com/repo.git'"
    ));
    assert!(is_network_error(
        "fatal: Cannot use the 'secretservice' credential backing store without a graphical interface"
    ));
    assert!(is_network_error(
        "fatal: could not read Password for 'https://example.com': terminal prompts disabled"
    ));
    assert!(is_network_error(
        "fatal: could not read Username for 'https://example.com': terminal prompts disabled"
    ));
    assert!(!is_network_error(
        "error: src refspec main does not match any"
    ));
}

#[test]
fn conflict_error_detection() {
    assert!(is_conflict_error(
        "CONFLICT (content): Merge conflict in file.txt"
    ));
    assert!(is_conflict_error(
        "error: could not apply abc1234... commit msg"
    ));
    assert!(!is_conflict_error("Everything up-to-date"));
}

#[test]
fn push_rejected_detection() {
    assert!(is_push_rejected(
        "! [rejected] main -> main (non-fast-forward)"
    ));
    assert!(is_push_rejected("error: failed to push some refs"));
    assert!(!is_push_rejected("Everything up-to-date"));
}

#[test]
fn sync_error_git_includes_inner_error_details() {
    use super::GitError;

    let inner_error = GitError::Failed {
        code: 128,
        args: "push origin main".to_string(),
        stdout: String::new(),
        stderr: "fatal: Authentication failed".to_string(),
    };
    let sync_error = SyncError::Git(inner_error);
    let message = format!("{sync_error}");

    assert!(
        message.contains("sync failed"),
        "message should contain 'sync failed' prefix"
    );
    assert!(
        message.contains("Authentication failed"),
        "message should include inner error details: {message}"
    );
}
