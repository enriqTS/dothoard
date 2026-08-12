use super::*;
use std::os::unix::fs::PermissionsExt;

fn create_test_tree(tmp: &Path) {
    // Regular files
    std::fs::create_dir_all(tmp.join("subdir")).unwrap();
    std::fs::write(tmp.join("file.txt"), "hello").unwrap();
    std::fs::write(tmp.join("subdir/nested.txt"), "world").unwrap();

    // Hidden file
    std::fs::write(tmp.join(".hidden"), "secret").unwrap();

    // Executable file
    std::fs::write(tmp.join("script.sh"), "#!/bin/bash").unwrap();
    std::fs::set_permissions(
        tmp.join("script.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    // Symlink
    std::os::unix::fs::symlink("/some/target", tmp.join("link")).unwrap();

    // Nested .git directory
    std::fs::create_dir_all(tmp.join(".git/objects")).unwrap();
    std::fs::write(tmp.join(".git/HEAD"), "ref: refs/heads/main").unwrap();

    // Hidden directory with content
    std::fs::create_dir_all(tmp.join(".config")).unwrap();
    std::fs::write(tmp.join(".config/settings"), "key=value").unwrap();
}

#[test]
fn walks_regular_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "b").unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| e.kind == WalkEntryKind::File));
    assert_eq!(entries[0].relative, PathBuf::from("a.txt"));
    assert_eq!(entries[1].relative, PathBuf::from("b.txt"));
}

#[test]
fn includes_hidden_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".hidden"), "data").unwrap();
    std::fs::write(tmp.path().join("visible"), "data").unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());
    assert_eq!(entries.len(), 2);
    let names: Vec<_> = entries.iter().map(|e| &e.relative).collect();
    assert!(names.contains(&&PathBuf::from(".hidden")));
    assert!(names.contains(&&PathBuf::from("visible")));
}

#[test]
fn detects_executable_files() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("run.sh");
    std::fs::write(&script, "#!/bin/bash").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, WalkEntryKind::ExecutableFile);
}

#[test]
fn preserves_symlinks_without_following() {
    let tmp = tempfile::tempdir().unwrap();
    // Create a symlink pointing to a nonexistent target — must still be found.
    std::os::unix::fs::symlink("/nonexistent/target", tmp.path().join("broken-link")).unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, WalkEntryKind::Symlink);
    assert_eq!(entries[0].relative, PathBuf::from("broken-link"));
}

#[test]
fn does_not_follow_symlink_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let real_dir = tmp.path().join("real");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::write(real_dir.join("inside.txt"), "data").unwrap();

    // Symlink to a directory — walker must not enter it.
    std::os::unix::fs::symlink(&real_dir, tmp.path().join("link-dir")).unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());
    // Should have: real/inside.txt and link-dir (as symlink)
    let symlinks: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == WalkEntryKind::Symlink)
        .collect();
    assert_eq!(symlinks.len(), 1);
    assert_eq!(symlinks[0].relative, PathBuf::from("link-dir"));

    // Must NOT contain any entries beneath link-dir/
    assert!(
        !entries
            .iter()
            .any(|e| { e.relative.starts_with("link-dir") && e.kind != WalkEntryKind::Symlink })
    );
}

#[test]
fn skips_nested_git_directories() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git/objects")).unwrap();
    std::fs::write(tmp.path().join(".git/HEAD"), "ref").unwrap();
    std::fs::write(tmp.path().join("file.txt"), "data").unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());
    // Should have file.txt and .git (as GitDirectory)
    let git_entries: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == WalkEntryKind::GitDirectory)
        .collect();
    assert_eq!(git_entries.len(), 1);
    assert_eq!(git_entries[0].relative, PathBuf::from(".git"));

    // Must NOT contain any files inside .git (only the .git entry itself)
    let files_inside_git: Vec<_> = entries
        .iter()
        .filter(|e| e.relative.starts_with(".git") && e.kind != WalkEntryKind::GitDirectory)
        .collect();
    assert!(files_inside_git.is_empty());
}

#[test]
fn detects_special_files() {
    let tmp = tempfile::tempdir().unwrap();
    let sock_path = tmp.path().join("test.sock");

    // Create a Unix socket
    let listener = std::os::unix::net::UnixListener::bind(&sock_path).unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();
    drop(listener);

    assert!(errors.is_empty());
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        &entries[0].kind,
        WalkEntryKind::SpecialFile { file_type } if file_type == "socket"
    ));
}

#[test]
fn recurses_into_subdirectories() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("a/b/c")).unwrap();
    std::fs::write(tmp.path().join("a/b/c/deep.txt"), "deep").unwrap();
    std::fs::write(tmp.path().join("a/top.txt"), "top").unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());
    let paths: Vec<_> = entries.iter().map(|e| &e.relative).collect();
    assert!(paths.contains(&&PathBuf::from("a/b/c/deep.txt")));
    assert!(paths.contains(&&PathBuf::from("a/top.txt")));
}

#[test]
fn single_file_source_root() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("single.txt");
    std::fs::write(&file, "content").unwrap();

    let (entries, errors) = walk_source(&file).unwrap();

    assert!(errors.is_empty());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, file);
    assert_eq!(entries[0].relative, PathBuf::new());
    assert_eq!(entries[0].kind, WalkEntryKind::File);
}

#[test]
fn single_symlink_source_root() {
    let tmp = tempfile::tempdir().unwrap();
    let link = tmp.path().join("my-link");
    std::os::unix::fs::symlink("/some/target", &link).unwrap();

    let (entries, errors) = walk_source(&link).unwrap();

    assert!(errors.is_empty());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, WalkEntryKind::Symlink);
}

#[test]
fn output_is_sorted_by_relative_path() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("z.txt"), "z").unwrap();
    std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
    std::fs::create_dir_all(tmp.path().join("m")).unwrap();
    std::fs::write(tmp.path().join("m/file.txt"), "m").unwrap();

    let (entries, _) = walk_source(tmp.path()).unwrap();

    let paths: Vec<_> = entries.iter().map(|e| e.relative.clone()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}

#[test]
fn full_test_tree() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_tree(tmp.path());

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());

    // Verify expected entries exist
    let has_file = entries
        .iter()
        .any(|e| e.relative == *"file.txt" && e.kind == WalkEntryKind::File);
    let has_hidden = entries
        .iter()
        .any(|e| e.relative == *".hidden" && e.kind == WalkEntryKind::File);
    let has_executable = entries
        .iter()
        .any(|e| e.relative == *"script.sh" && e.kind == WalkEntryKind::ExecutableFile);
    let has_symlink = entries
        .iter()
        .any(|e| e.relative == *"link" && e.kind == WalkEntryKind::Symlink);
    let has_git = entries
        .iter()
        .any(|e| e.relative == *".git" && e.kind == WalkEntryKind::GitDirectory);
    let has_nested = entries.iter().any(|e| e.relative == *"subdir/nested.txt");
    let has_config = entries.iter().any(|e| e.relative == *".config/settings");

    assert!(has_file, "missing file.txt");
    assert!(has_hidden, "missing .hidden");
    assert!(has_executable, "missing script.sh");
    assert!(has_symlink, "missing link");
    assert!(has_git, "missing .git");
    assert!(has_nested, "missing subdir/nested.txt");
    assert!(has_config, "missing .config/settings");

    // Verify .git contents are NOT present (only the .git entry itself)
    assert!(
        !entries
            .iter()
            .any(|e| { e.relative.starts_with(".git") && e.kind != WalkEntryKind::GitDirectory })
    );
}

#[test]
fn nonexistent_source_returns_error() {
    let result = walk_source(Path::new("/nonexistent/path/for/testing"));
    assert!(result.is_err());
}

#[test]
fn empty_directory_returns_empty_entries() {
    let tmp = tempfile::tempdir().unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(entries.is_empty());
    assert!(errors.is_empty());
}

#[test]
fn walk_entry_kind_predicates() {
    assert!(WalkEntryKind::File.is_file());
    assert!(WalkEntryKind::ExecutableFile.is_file());
    assert!(!WalkEntryKind::Symlink.is_file());

    assert!(WalkEntryKind::Symlink.is_symlink());
    assert!(!WalkEntryKind::File.is_symlink());

    assert!(WalkEntryKind::File.is_backupable());
    assert!(WalkEntryKind::ExecutableFile.is_backupable());
    assert!(WalkEntryKind::Symlink.is_backupable());
    assert!(!WalkEntryKind::Directory.is_backupable());
    assert!(!WalkEntryKind::GitDirectory.is_backupable());
    assert!(
        !(WalkEntryKind::SpecialFile {
            file_type: "socket".to_string()
        })
        .is_backupable()
    );
}

#[test]
fn hidden_directories_are_traversed() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".hidden-dir/sub")).unwrap();
    std::fs::write(tmp.path().join(".hidden-dir/sub/file.txt"), "data").unwrap();

    let (entries, errors) = walk_source(tmp.path()).unwrap();

    assert!(errors.is_empty());
    assert!(
        entries
            .iter()
            .any(|e| e.relative == *".hidden-dir/sub/file.txt")
    );
}
