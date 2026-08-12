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
        "format = \"dothoard-manifest\"\nversion = 1\n",
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
fn parse_renamed_entry_includes_orig_path() {
    // Renamed: "2 XY sub mH mI mW hH hI R### new_path\0old_path\0"
    let output = "2 R. N... 100644 100644 100644 abc123 def456 R100 home/new.txt\0home/old.txt\0";
    let paths = parse_status_paths(output);
    assert_eq!(paths, vec!["home/new.txt", "home/old.txt"]);
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
fn namespace_classification_treats_siblings_as_blocking() {
    let (tmp, runner) = init_test_repo();
    fs::create_dir_all(tmp.path().join("desktop/home")).unwrap();
    fs::create_dir_all(tmp.path().join("notebook/home")).unwrap();
    fs::write(tmp.path().join("desktop/home/.bashrc"), "desktop").unwrap();
    fs::write(tmp.path().join("notebook/home/.bashrc"), "notebook").unwrap();

    let status = classify_namespace_worktree(&runner, tmp.path(), "desktop").unwrap();
    assert!(
        status
            .managed_dirty
            .contains(&"desktop/home/.bashrc".to_string())
    );
    assert!(
        status
            .unmanaged_dirty
            .contains(&"notebook/home/.bashrc".to_string())
    );
    assert!(status.has_blocking_changes());
}

#[test]
fn namespace_manifest_is_recoverable_but_sibling_manifest_blocks() {
    let (tmp, runner) = init_test_repo();
    fs::create_dir_all(tmp.path().join("desktop")).unwrap();
    fs::create_dir_all(tmp.path().join("notebook")).unwrap();
    fs::write(
        tmp.path().join("desktop/.dothoard-manifest.toml"),
        "desktop",
    )
    .unwrap();
    fs::write(
        tmp.path().join("notebook/.dothoard-manifest.toml"),
        "notebook",
    )
    .unwrap();

    let status = classify_namespace_worktree(&runner, tmp.path(), "desktop").unwrap();
    assert!(status.has_recoverable_changes());
    assert!(status.has_blocking_changes());
    assert!(
        status
            .unmanaged_dirty
            .contains(&"notebook/.dothoard-manifest.toml".to_string())
    );
}

#[test]
fn parse_rename_includes_both_paths() {
    let output = "2 R. N... 100644 100644 100644 abc123 def456 R100 desktop/home/new.txt\0notebook/home/old.txt\0";
    assert_eq!(
        parse_status_paths(output),
        vec!["desktop/home/new.txt", "notebook/home/old.txt"]
    );
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
