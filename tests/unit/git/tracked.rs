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
fn returns_empty_for_no_ignored_paths() {
    let (tmp, runner) = init_test_repo();

    let result = find_tracked_ignored(&runner, tmp.path(), &[]).unwrap();
    assert!(result.is_empty());
}

#[test]
fn detects_tracked_file_that_becomes_ignored() {
    let (tmp, runner) = init_test_repo();

    // Track a file.
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::write(home.join(".config/fish/fish_variables"), "vars").unwrap();
    fs::write(home.join(".config/fish/config.fish"), "# fish").unwrap();

    let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(tmp.path()).args(["commit", "-m", "add fish config"]);
    runner.run(&cmd).unwrap();

    // Now check which ignored paths are tracked.
    let ignored = &["home/.config/fish/fish_variables"];
    let result = find_tracked_ignored(&runner, tmp.path(), ignored).unwrap();

    assert_eq!(result, vec!["home/.config/fish/fish_variables"]);
}

#[test]
fn does_not_report_untracked_ignored_files() {
    let (tmp, runner) = init_test_repo();

    // Create a file but don't track it.
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join(".config")).unwrap();
    fs::write(home.join(".config/secret.env"), "API_KEY=xxx").unwrap();

    // It's ignored but never tracked — should not appear.
    let ignored = &["home/.config/secret.env"];
    let result = find_tracked_ignored(&runner, tmp.path(), ignored).unwrap();

    assert!(result.is_empty());
}

#[test]
fn multiple_tracked_ignored_files() {
    let (tmp, runner) = init_test_repo();

    let home = tmp.path().join("home");
    fs::create_dir_all(home.join(".config")).unwrap();
    fs::write(home.join(".config/token"), "secret").unwrap();
    fs::write(home.join(".config/cache.db"), "data").unwrap();
    fs::write(home.join(".config/settings.toml"), "keep").unwrap();

    let cmd = GitCommand::new(tmp.path()).args(["add", "--", "home/"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(tmp.path()).args(["commit", "-m", "add config"]);
    runner.run(&cmd).unwrap();

    // Only some become ignored.
    let ignored = &[
        "home/.config/token",
        "home/.config/cache.db",
        "home/.config/nonexistent", // not tracked
    ];
    let result = find_tracked_ignored(&runner, tmp.path(), ignored).unwrap();

    assert_eq!(result, vec!["home/.config/cache.db", "home/.config/token"]);
}

#[test]
fn works_with_empty_repository() {
    let (tmp, runner) = init_test_repo();

    let ignored = &["home/.bashrc"];
    let result = find_tracked_ignored(&runner, tmp.path(), ignored).unwrap();

    assert!(result.is_empty());
}
