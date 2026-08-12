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

#[test]
fn namespace_staging_leaves_siblings_and_legacy_paths_unstaged() {
    let (tmp, runner) = init_test_repo();
    fs::create_dir_all(tmp.path().join("desktop/home")).unwrap();
    fs::create_dir_all(tmp.path().join("notebook/home")).unwrap();
    fs::write(tmp.path().join("desktop/home/.bashrc"), "desktop").unwrap();
    fs::write(
        tmp.path().join("desktop/.dothoard-manifest.toml"),
        "desktop",
    )
    .unwrap();
    fs::write(tmp.path().join("notebook/home/.bashrc"), "notebook").unwrap();
    fs::create_dir_all(tmp.path().join("home")).unwrap();
    fs::write(tmp.path().join("home/.bashrc"), "legacy").unwrap();

    stage_namespace(&runner, tmp.path(), "desktop").unwrap();

    let staged = verify_namespace_boundaries(&runner, tmp.path(), "desktop").unwrap();
    assert!(staged.contains(&"desktop/home/.bashrc".to_string()));
    assert!(staged.contains(&"desktop/.dothoard-manifest.toml".to_string()));
    assert!(!staged.iter().any(|path| path.starts_with("notebook/")));
    assert!(!staged.contains(&"home/.bashrc".to_string()));
}

#[test]
fn namespace_boundary_rejects_staged_sibling_and_rename_source() {
    let (tmp, runner) = init_test_repo();
    fs::create_dir_all(tmp.path().join("notebook/home")).unwrap();
    fs::write(tmp.path().join("notebook/home/.bashrc"), "sibling").unwrap();
    let cmd = GitCommand::new(tmp.path()).args(["add", "--", "notebook/home/.bashrc"]);
    runner.run(&cmd).unwrap();

    let result = verify_namespace_boundaries(&runner, tmp.path(), "desktop");
    assert!(matches!(result, Err(StagingError::UnmanagedStaged { .. })));

    let cmd = GitCommand::new(tmp.path()).args(["reset", "--hard"]);
    runner.run(&cmd).unwrap();
    fs::create_dir_all(tmp.path().join("notebook/home")).unwrap();
    fs::write(tmp.path().join("notebook/home/.bashrc"), "sibling").unwrap();
    let cmd = GitCommand::new(tmp.path()).args(["add", "--", "notebook/home/.bashrc"]);
    runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(tmp.path()).args(["commit", "-m", "sibling"]);
    runner.run(&cmd).unwrap();
    fs::create_dir_all(tmp.path().join("desktop/home")).unwrap();
    fs::rename(
        tmp.path().join("notebook/home/.bashrc"),
        tmp.path().join("desktop/home/.bashrc"),
    )
    .unwrap();
    let cmd = GitCommand::new(tmp.path()).args(["add", "--all"]);
    runner.run(&cmd).unwrap();

    let result = verify_namespace_boundaries(&runner, tmp.path(), "desktop");
    let Err(StagingError::UnmanagedStaged { paths }) = result else {
        panic!("cross-namespace rename must be rejected");
    };
    assert!(paths.contains(&"notebook/home/.bashrc".to_string()));
}

#[test]
fn parse_staged_paths_includes_both_rename_endpoints() {
    assert_eq!(
        parse_staged_paths("M\0desktop/home/a\0R100\0notebook/home/a\0desktop/home/a\0"),
        vec!["desktop/home/a", "notebook/home/a", "desktop/home/a"]
    );
}
