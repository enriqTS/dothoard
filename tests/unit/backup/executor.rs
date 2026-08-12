use super::*;

// --- normalize_lexical ---

#[test]
fn normalize_removes_current_dir() {
    let path = Path::new("/repo/./home/./file");
    assert_eq!(normalize_lexical(path), PathBuf::from("/repo/home/file"));
}

#[test]
fn normalize_resolves_parent_dir() {
    let path = Path::new("/repo/home/../home/file");
    assert_eq!(normalize_lexical(path), PathBuf::from("/repo/home/file"));
}

#[test]
fn normalize_no_change_for_clean_path() {
    let path = Path::new("/repo/desktop/home/.config/fish");
    assert_eq!(
        normalize_lexical(path),
        PathBuf::from("/repo/desktop/home/.config/fish")
    );
}

#[test]
fn normalize_preserves_leading_dotfiles() {
    let path = Path::new("/repo/home/.bashrc");
    assert_eq!(normalize_lexical(path), PathBuf::from("/repo/home/.bashrc"));
}

// --- validate_boundary ---

#[test]
fn boundary_accepts_path_inside_repository() {
    let repo = Path::new("/home/user/dotfiles");
    let dest = Path::new("/home/user/dotfiles/home/.bashrc");
    assert!(validate_boundary(repo, dest).is_ok());
}

#[test]
fn boundary_accepts_deeply_nested_path() {
    let repo = Path::new("/home/user/dotfiles");
    let dest = Path::new("/home/user/dotfiles/home/.config/fish/functions/hello.fish");
    assert!(validate_boundary(repo, dest).is_ok());
}

#[test]
fn boundary_rejects_path_outside_repository() {
    let repo = Path::new("/home/user/dotfiles");
    let dest = Path::new("/home/user/other/file.txt");
    let result = validate_boundary(repo, dest);
    assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
}

#[test]
fn boundary_rejects_path_that_is_repository_root() {
    let repo = Path::new("/home/user/dotfiles");
    let dest = Path::new("/home/user/dotfiles");
    let result = validate_boundary(repo, dest);
    assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
}

#[test]
fn boundary_rejects_traversal_escape() {
    let repo = Path::new("/home/user/dotfiles");
    let dest = Path::new("/home/user/dotfiles/home/../../etc/passwd");
    let result = validate_boundary(repo, dest);
    assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
}

#[test]
fn boundary_rejects_sibling_with_prefix() {
    // "dotfiles-evil" starts with "dotfiles" as a string but is not beneath it.
    let repo = Path::new("/home/user/dotfiles");
    let dest = Path::new("/home/user/dotfiles-evil/file.txt");
    let result = validate_boundary(repo, dest);
    assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
}

#[test]
fn boundary_accepts_manifest_path() {
    let repo = Path::new("/home/user/dotfiles");
    let dest = Path::new("/home/user/dotfiles/.dothoard-manifest.toml");
    assert!(validate_boundary(repo, dest).is_ok());
}

// --- validate_no_symlinked_parents ---

#[test]
fn symlink_check_passes_when_no_parents_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let dest = repo
        .join("desktop/home")
        .join(".config")
        .join("fish")
        .join("config.fish");
    assert!(validate_no_symlinked_parents(&repo, &dest).is_ok());
}

#[test]
fn symlink_check_passes_with_real_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home").join(".config")).unwrap();

    let dest = repo.join("desktop/home").join(".config").join("file.txt");
    assert!(validate_no_symlinked_parents(&repo, &dest).is_ok());
}

#[test]
fn symlink_check_rejects_symlinked_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let escape_target = tmp.path().join("escape");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();
    std::fs::create_dir_all(&escape_target).unwrap();

    // Create a symlink at repo/home/.config -> /tmp/.../escape
    std::os::unix::fs::symlink(&escape_target, repo.join("desktop/home").join(".config")).unwrap();

    let dest = repo.join("desktop/home").join(".config").join("file.txt");
    let result = validate_no_symlinked_parents(&repo, &dest);
    assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
}

#[test]
fn symlink_check_allows_symlink_as_final_component() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    // The final component (the file itself) can be a symlink — we'll replace it.
    std::os::unix::fs::symlink("/some/target", repo.join("desktop/home").join("my-link")).unwrap();

    let dest = repo.join("desktop/home").join("my-link");
    assert!(validate_no_symlinked_parents(&repo, &dest).is_ok());
}

#[test]
fn symlink_check_rejects_intermediate_symlink_deep() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let escape_target = tmp.path().join("elsewhere");
    std::fs::create_dir_all(repo.join("desktop/home").join(".config")).unwrap();
    std::fs::create_dir_all(&escape_target).unwrap();

    // Create symlink at repo/home/.config/fish -> elsewhere
    std::os::unix::fs::symlink(
        &escape_target,
        repo.join("desktop/home").join(".config").join("fish"),
    )
    .unwrap();

    let dest = repo
        .join("desktop/home")
        .join(".config")
        .join("fish")
        .join("config.fish");
    let result = validate_no_symlinked_parents(&repo, &dest);
    assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
}

// --- validate_destination (combined) ---

#[test]
fn validate_destination_passes_for_valid_path() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let dest = repo.join("desktop/home").join(".bashrc");
    assert!(validate_destination(&repo, "desktop", &dest).is_ok());
}

#[test]
fn validate_destination_fails_for_escape() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let dest = tmp.path().join("outside").join("file.txt");
    let result = validate_destination(&repo, "desktop", &dest);
    assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
}

#[test]
fn validate_destination_fails_for_symlinked_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let escape_target = tmp.path().join("escape");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();
    std::fs::create_dir_all(&escape_target).unwrap();

    std::os::unix::fs::symlink(&escape_target, repo.join("desktop/home").join("evil")).unwrap();

    let dest = repo.join("desktop/home").join("evil").join("file.txt");
    let result = validate_destination(&repo, "desktop", &dest);
    assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
}

// --- copy_file_atomic ---

#[test]
fn copy_file_creates_destination_with_content() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("source.txt");
    std::fs::write(&source, "hello world").unwrap();

    let dest = repo.join("desktop/home").join("file.txt");
    copy_file_atomic(&repo, "desktop", &source, &dest, false).unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello world");
}

#[test]
fn copy_file_preserves_executable_bit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("script.sh");
    std::fs::write(&source, "#!/bin/bash\necho hi").unwrap();

    let dest = repo.join("desktop/home").join("script.sh");
    copy_file_atomic(&repo, "desktop", &source, &dest, true).unwrap();

    let meta = std::fs::metadata(&dest).unwrap();
    assert!(meta.permissions().mode() & 0o111 != 0);
}

#[test]
fn copy_file_sets_non_executable_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("data.txt");
    std::fs::write(&source, "data").unwrap();

    let dest = repo.join("desktop/home").join("data.txt");
    copy_file_atomic(&repo, "desktop", &source, &dest, false).unwrap();

    let meta = std::fs::metadata(&dest).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o644);
}

#[test]
fn copy_file_creates_parent_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let source = tmp.path().join("src.txt");
    std::fs::write(&source, "content").unwrap();

    let dest = repo
        .join("desktop/home")
        .join(".config")
        .join("fish")
        .join("config.fish");
    copy_file_atomic(&repo, "desktop", &source, &dest, false).unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "content");
}

#[test]
fn copy_file_replaces_existing_file_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("new.txt");
    std::fs::write(&source, "new content").unwrap();

    let dest = repo.join("desktop/home").join("file.txt");
    std::fs::write(&dest, "old content").unwrap();

    copy_file_atomic(&repo, "desktop", &source, &dest, false).unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new content");
}

#[test]
fn copy_file_replaces_existing_symlink_with_regular_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("real.txt");
    std::fs::write(&source, "real content").unwrap();

    // Destination is currently a symlink.
    let dest = repo.join("desktop/home").join("link");
    std::os::unix::fs::symlink("/some/target", &dest).unwrap();

    copy_file_atomic(&repo, "desktop", &source, &dest, false).unwrap();

    // After copy, destination is a regular file, not a symlink.
    let meta = std::fs::symlink_metadata(&dest).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "real content");
}

#[test]
fn copy_file_rejects_boundary_escape() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let source = tmp.path().join("source.txt");
    std::fs::write(&source, "evil").unwrap();

    let dest = tmp.path().join("outside").join("file.txt");
    let result = copy_file_atomic(&repo, "desktop", &source, &dest, false);
    assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
}

#[test]
fn copy_file_rejects_symlinked_parent_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let escape = tmp.path().join("escape");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();
    std::fs::create_dir_all(&escape).unwrap();

    // Create symlink escape at repo/home/evil -> escape dir
    std::os::unix::fs::symlink(&escape, repo.join("desktop/home").join("evil")).unwrap();

    let source = tmp.path().join("source.txt");
    std::fs::write(&source, "data").unwrap();

    let dest = repo.join("desktop/home").join("evil").join("file.txt");
    let result = copy_file_atomic(&repo, "desktop", &source, &dest, false);
    assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
}

#[test]
fn copy_file_handles_empty_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("empty.txt");
    std::fs::write(&source, "").unwrap();

    let dest = repo.join("desktop/home").join("empty.txt");
    copy_file_atomic(&repo, "desktop", &source, &dest, false).unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "");
}

#[test]
fn copy_file_handles_large_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    // Create a file larger than the 8KB buffer.
    let content = "x".repeat(32 * 1024);
    let source = tmp.path().join("large.txt");
    std::fs::write(&source, &content).unwrap();

    let dest = repo.join("desktop/home").join("large.txt");
    copy_file_atomic(&repo, "desktop", &source, &dest, false).unwrap();

    assert_eq!(std::fs::read_to_string(&dest).unwrap(), content);
}

// --- copy_symlink ---

#[test]
fn copy_symlink_preserves_relative_target() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("link");
    std::os::unix::fs::symlink("../other/file", &source).unwrap();

    let dest = repo.join("desktop/home").join("link");
    copy_symlink(&repo, "desktop", &source, &dest).unwrap();

    let target = std::fs::read_link(&dest).unwrap();
    assert_eq!(target, PathBuf::from("../other/file"));
}

#[test]
fn copy_symlink_preserves_absolute_target() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("link");
    std::os::unix::fs::symlink("/usr/bin/bash", &source).unwrap();

    let dest = repo.join("desktop/home").join("link");
    copy_symlink(&repo, "desktop", &source, &dest).unwrap();

    let target = std::fs::read_link(&dest).unwrap();
    assert_eq!(target, PathBuf::from("/usr/bin/bash"));
}

#[test]
fn copy_symlink_preserves_dangling_target() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let source = tmp.path().join("link");
    std::os::unix::fs::symlink("/nonexistent/path/that/does/not/exist", &source).unwrap();

    let dest = repo.join("desktop/home").join("link");
    copy_symlink(&repo, "desktop", &source, &dest).unwrap();

    let target = std::fs::read_link(&dest).unwrap();
    assert_eq!(
        target,
        PathBuf::from("/nonexistent/path/that/does/not/exist")
    );
}

#[test]
fn copy_symlink_replaces_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    // Existing regular file at destination.
    let dest = repo.join("desktop/home").join("entry");
    std::fs::write(&dest, "old file content").unwrap();

    let source = tmp.path().join("link");
    std::os::unix::fs::symlink("/target", &source).unwrap();

    copy_symlink(&repo, "desktop", &source, &dest).unwrap();

    let meta = std::fs::symlink_metadata(&dest).unwrap();
    assert!(meta.file_type().is_symlink());
    assert_eq!(std::fs::read_link(&dest).unwrap(), PathBuf::from("/target"));
}

#[test]
fn copy_symlink_replaces_existing_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    // Existing symlink at destination.
    let dest = repo.join("desktop/home").join("link");
    std::os::unix::fs::symlink("/old/target", &dest).unwrap();

    let source = tmp.path().join("link");
    std::os::unix::fs::symlink("/new/target", &source).unwrap();

    copy_symlink(&repo, "desktop", &source, &dest).unwrap();

    assert_eq!(
        std::fs::read_link(&dest).unwrap(),
        PathBuf::from("/new/target")
    );
}

#[test]
fn copy_symlink_creates_parent_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let source = tmp.path().join("link");
    std::os::unix::fs::symlink("/target", &source).unwrap();

    let dest = repo
        .join("desktop/home")
        .join("deep")
        .join("nested")
        .join("link");
    copy_symlink(&repo, "desktop", &source, &dest).unwrap();

    assert_eq!(std::fs::read_link(&dest).unwrap(), PathBuf::from("/target"));
}

#[test]
fn copy_symlink_rejects_boundary_escape() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let source = tmp.path().join("link");
    std::os::unix::fs::symlink("/target", &source).unwrap();

    let dest = tmp.path().join("outside").join("link");
    let result = copy_symlink(&repo, "desktop", &source, &dest);
    assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
}

#[test]
fn copy_symlink_rejects_symlinked_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let escape = tmp.path().join("escape");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();
    std::fs::create_dir_all(&escape).unwrap();

    std::os::unix::fs::symlink(&escape, repo.join("desktop/home").join("evil")).unwrap();

    let source = tmp.path().join("link");
    std::os::unix::fs::symlink("/target", &source).unwrap();

    let dest = repo.join("desktop/home").join("evil").join("link");
    let result = copy_symlink(&repo, "desktop", &source, &dest);
    assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
}

// --- delete_entry ---

#[test]
fn delete_entry_removes_regular_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let file = repo.join("desktop/home").join("old.txt");
    std::fs::write(&file, "content").unwrap();

    delete_entry(&repo, "desktop", &file).unwrap();

    assert!(!file.exists());
}

#[test]
fn delete_entry_removes_symlink_without_following() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    // Create a symlink pointing to a real file outside the repo.
    let outside_file = tmp.path().join("outside.txt");
    std::fs::write(&outside_file, "should not be deleted").unwrap();

    let link = repo.join("desktop/home").join("link");
    std::os::unix::fs::symlink(&outside_file, &link).unwrap();

    delete_entry(&repo, "desktop", &link).unwrap();

    // Symlink is gone.
    assert!(!link.exists());
    // Target file is untouched.
    assert!(outside_file.exists());
    assert_eq!(
        std::fs::read_to_string(&outside_file).unwrap(),
        "should not be deleted"
    );
}

#[test]
fn delete_entry_is_idempotent_for_missing_path() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let file = repo.join("desktop/home").join("nonexistent.txt");
    // Should succeed without error even though file doesn't exist.
    delete_entry(&repo, "desktop", &file).unwrap();
}

#[test]
fn delete_entry_cleans_up_empty_parent_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let deep = repo.join("desktop/home").join(".config").join("fish");
    std::fs::create_dir_all(&deep).unwrap();

    let file = deep.join("config.fish");
    std::fs::write(&file, "content").unwrap();

    delete_entry(&repo, "desktop", &file).unwrap();

    // File is gone.
    assert!(!file.exists());
    // Empty parents cleaned up.
    assert!(!deep.exists());
    assert!(!repo.join("desktop/home").join(".config").exists());
    // But repo/home stays if it's the managed root (not repo itself).
    // Actually, home/ is also empty so it gets cleaned too.
    assert!(!repo.join("desktop/home").exists());
    // Repository root itself is never removed.
    assert!(repo.exists());
}

#[test]
fn delete_entry_stops_cleanup_at_non_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let fish_dir = repo.join("desktop/home").join(".config").join("fish");
    std::fs::create_dir_all(&fish_dir).unwrap();

    // Two files in the directory.
    std::fs::write(fish_dir.join("config.fish"), "content").unwrap();
    std::fs::write(fish_dir.join("functions.fish"), "other").unwrap();

    // Delete only one.
    delete_entry(&repo, "desktop", &fish_dir.join("config.fish")).unwrap();

    // Directory still exists because it has another file.
    assert!(fish_dir.exists());
    assert!(fish_dir.join("functions.fish").exists());
}

#[test]
fn delete_entry_rejects_boundary_escape() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "data").unwrap();

    let result = delete_entry(&repo, "desktop", &outside);
    assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
    // File still exists.
    assert!(outside.exists());
}

#[test]
fn delete_entry_rejects_symlinked_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let escape = tmp.path().join("escape");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();
    std::fs::create_dir_all(&escape).unwrap();
    std::fs::write(escape.join("file.txt"), "data").unwrap();

    // Symlink inside managed namespace pointing outside.
    std::os::unix::fs::symlink(&escape, repo.join("desktop/home").join("evil")).unwrap();

    let target_file = repo.join("desktop/home").join("evil").join("file.txt");
    let result = delete_entry(&repo, "desktop", &target_file);
    assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
    // Original file is untouched.
    assert!(escape.join("file.txt").exists());
}

#[test]
fn delete_entry_removes_dangling_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();

    let link = repo.join("desktop/home").join("dangling");
    std::os::unix::fs::symlink("/nonexistent/path", &link).unwrap();

    delete_entry(&repo, "desktop", &link).unwrap();

    // The symlink entry itself should be gone.
    assert!(!link.symlink_metadata().is_ok());
}

// --- update_manifest ---

#[test]
fn update_manifest_creates_manifest_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec!["*.log".to_string()],
    }];

    update_manifest(&repo, "desktop", &sources).unwrap();

    let manifest_path = repo.join("desktop").join(crate::app::MANIFEST_FILE_NAME);
    assert!(manifest_path.exists());

    let content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(content.contains("dothoard-manifest"));
    assert!(content.contains(".config/fish"));
    assert!(content.contains("*.log"));
}

#[test]
fn update_manifest_overwrites_existing_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    // Write initial manifest.
    let sources_v1 = vec![crate::config::SourceConfig {
        path: ".bashrc".to_string(),
        ignore: vec![],
    }];
    update_manifest(&repo, "desktop", &sources_v1).unwrap();

    // Overwrite with new sources.
    let sources_v2 = vec![
        crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string()],
        },
        crate::config::SourceConfig {
            path: ".config/waybar".to_string(),
            ignore: vec![],
        },
    ];
    update_manifest(&repo, "desktop", &sources_v2).unwrap();

    let manifest_path = repo.join("desktop").join(crate::app::MANIFEST_FILE_NAME);
    let content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(content.contains(".config/fish"));
    assert!(content.contains(".config/waybar"));
    assert!(!content.contains(".bashrc"));
}

#[test]
fn update_manifest_with_empty_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    update_manifest(&repo, "desktop", &[]).unwrap();

    let manifest_path = repo.join("desktop").join(crate::app::MANIFEST_FILE_NAME);
    assert!(manifest_path.exists());

    // Should be loadable and valid.
    let loaded = super::super::manifest::Manifest::load(&repo.join("desktop")).unwrap();
    assert_eq!(loaded.namespace, "desktop");
    assert!(loaded.sources.is_empty());
}

#[test]
fn update_manifest_produces_valid_loadable_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();

    let sources = vec![
        crate::config::SourceConfig {
            path: ".ssh/config".to_string(),
            ignore: vec!["id_*".to_string()],
        },
        crate::config::SourceConfig {
            path: ".config/waybar".to_string(),
            ignore: vec!["cache/".to_string(), "*token*".to_string()],
        },
    ];

    update_manifest(&repo, "desktop", &sources).unwrap();

    let loaded = super::super::manifest::Manifest::load(&repo.join("desktop")).unwrap();
    assert_eq!(loaded.namespace, "desktop");
    assert_eq!(loaded.sources.len(), 2);
    assert_eq!(loaded.sources[0].path, ".ssh/config");
    assert_eq!(loaded.sources[0].ignore, vec!["id_*"]);
    assert_eq!(loaded.sources[1].path, ".config/waybar");
    assert_eq!(loaded.sources[1].ignore, vec!["cache/", "*token*"]);
}

// --- preflight_sources ---

#[test]
fn preflight_passes_with_existing_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let result = preflight_sources(&home, &repo, "desktop", &sources);
    assert!(result.is_ok());
    assert!(result.source_is_ready(0));
}

#[test]
fn preflight_marks_missing_source_as_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/nonexistent".to_string(),
        ignore: vec![],
    }];

    let result = preflight_sources(&home, &repo, "desktop", &sources);
    // Missing source is non-fatal.
    assert!(result.is_ok());
    assert!(!result.source_is_ready(0));
}

#[test]
fn preflight_multiple_sources_mixed() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    // .bashrc exists as a file.
    std::fs::write(home.join(".bashrc"), "# bash").unwrap();

    let sources = vec![
        crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        },
        crate::config::SourceConfig {
            path: ".config/missing".to_string(),
            ignore: vec![],
        },
        crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        },
    ];

    let result = preflight_sources(&home, &repo, "desktop", &sources);
    assert!(result.is_ok());
    assert!(result.source_is_ready(0)); // .config/fish exists
    assert!(!result.source_is_ready(1)); // .config/missing is missing
    assert!(result.source_is_ready(2)); // .bashrc exists
}

#[test]
fn preflight_detects_symlinked_destination_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    let escape = tmp.path().join("escape");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();
    std::fs::create_dir_all(&escape).unwrap();

    // Create a symlink at repo/home/.config -> escape directory
    std::os::unix::fs::symlink(&escape, repo.join("desktop/home").join(".config")).unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let result = preflight_sources(&home, &repo, "desktop", &sources);
    // Symlinked parent is a hard error.
    assert!(!result.is_ok());
    assert_eq!(result.errors.len(), 1);
}

#[test]
fn preflight_accepts_single_file_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(home.join(".bashrc"), "content").unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".bashrc".to_string(),
        ignore: vec![],
    }];

    let result = preflight_sources(&home, &repo, "desktop", &sources);
    assert!(result.is_ok());
    assert!(result.source_is_ready(0));
}

#[test]
fn preflight_with_no_sources_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();

    let result = preflight_sources(&home, &repo, "desktop", &[]);
    assert!(result.is_ok());
    assert_eq!(result.statuses.len(), 0);
}

// --- execute_mirror ---

#[test]
fn execute_mirror_empty_changeset_publishes() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();

    let sources: Vec<crate::config::SourceConfig> = vec![];
    let changeset = super::super::changeset::ChangeSet::new();

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 0);
    assert_eq!(result.deletions_completed, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn execute_mirror_applies_additions_and_publishes() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();

    std::fs::write(home.join(".config/fish/config.fish"), "set PATH").unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset.additions.push(super::super::changeset::Addition {
        source: home.join(".config/fish/config.fish"),
        destination: repo.join("desktop/home/.config/fish/config.fish"),
        entry_type: super::super::changeset::EntryType::RegularFile,
    });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);
    assert_eq!(
        std::fs::read_to_string(repo.join("desktop/home/.config/fish/config.fish")).unwrap(),
        "set PATH"
    );
    // Manifest was created.
    assert!(
        repo.join("desktop")
            .join(crate::app::MANIFEST_FILE_NAME)
            .exists()
    );
}

#[test]
fn execute_mirror_applies_deletions() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(repo.join("desktop/home/.config/fish")).unwrap();
    std::fs::write(repo.join("desktop/home/.config/fish/old.fish"), "old").unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset.deletions.push(super::super::changeset::Deletion {
        destination: repo.join("desktop/home/.config/fish/old.fish"),
        reason: super::super::changeset::DeletionReason::SourceRemoved,
    });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    assert_eq!(result.deletions_completed, 1);
    assert!(!repo.join("desktop/home/.config/fish/old.fish").exists());
}

#[test]
fn execute_mirror_blocks_publication_on_copy_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();

    // Source file referenced in changeset does not exist.
    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset.additions.push(super::super::changeset::Addition {
        source: home.join(".config/fish/nonexistent.fish"),
        destination: repo.join("desktop/home/.config/fish/nonexistent.fish"),
        entry_type: super::super::changeset::EntryType::RegularFile,
    });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(!result.may_publish);
    assert_eq!(result.copies_completed, 0);
    assert!(!result.errors.is_empty());
}

#[test]
fn execute_mirror_fails_on_preflight_hard_error() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    let escape = tmp.path().join("escape");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(repo.join("desktop/home")).unwrap();
    std::fs::create_dir_all(&escape).unwrap();

    // Symlink inside repo that would escape.
    std::os::unix::fs::symlink(&escape, repo.join("desktop/home").join(".config")).unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let changeset = super::super::changeset::ChangeSet::new();
    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset);

    // Preflight hard error → Err, not Ok with may_publish=false.
    assert!(result.is_err());
}

#[test]
fn execute_mirror_handles_symlink_addition() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/links")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();

    std::os::unix::fs::symlink("/usr/bin/bash", home.join(".config/links/bash")).unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/links".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset.additions.push(super::super::changeset::Addition {
        source: home.join(".config/links/bash"),
        destination: repo.join("desktop/home/.config/links/bash"),
        entry_type: super::super::changeset::EntryType::Symlink,
    });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);
    let target = std::fs::read_link(repo.join("desktop/home/.config/links/bash")).unwrap();
    assert_eq!(target, PathBuf::from("/usr/bin/bash"));
}

#[test]
fn execute_mirror_handles_executable_addition() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();

    std::fs::write(home.join("bin/script.sh"), "#!/bin/bash").unwrap();
    std::fs::set_permissions(
        home.join("bin/script.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: "bin".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset.additions.push(super::super::changeset::Addition {
        source: home.join("bin/script.sh"),
        destination: repo.join("desktop/home/bin/script.sh"),
        entry_type: super::super::changeset::EntryType::ExecutableFile,
    });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    let meta = std::fs::metadata(repo.join("desktop/home/bin/script.sh")).unwrap();
    assert!(meta.permissions().mode() & 0o111 != 0);
}

// --- Interrupted-run recovery ---

#[test]
fn recovery_stale_destination_is_overwritten() {
    // Simulates an interrupted run that left an outdated file at the
    // destination. A subsequent mirror corrects it.
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(repo.join("desktop/home/.config/fish")).unwrap();

    // Source has the correct content.
    std::fs::write(home.join(".config/fish/config.fish"), "correct content").unwrap();
    // Destination has stale content from a previous interrupted copy.
    std::fs::write(
        repo.join("desktop/home/.config/fish/config.fish"),
        "stale from interrupted run",
    )
    .unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    // The planner would detect this as a modification.
    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset
        .modifications
        .push(super::super::changeset::Modification {
            source: home.join(".config/fish/config.fish"),
            destination: repo.join("desktop/home/.config/fish/config.fish"),
            change: super::super::changeset::ChangeKind::ContentChanged,
        });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);
    assert_eq!(
        std::fs::read_to_string(repo.join("desktop/home/.config/fish/config.fish")).unwrap(),
        "correct content"
    );
}

#[test]
fn recovery_pending_deletion_completes() {
    // Simulates a file that should have been deleted in a previous run
    // but the run was interrupted before the deletion happened.
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(repo.join("desktop/home/.config/fish")).unwrap();

    // File exists in destination but not in source — deletion was pending.
    std::fs::write(
        repo.join("desktop/home/.config/fish/stale.fish"),
        "should be gone",
    )
    .unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset.deletions.push(super::super::changeset::Deletion {
        destination: repo.join("desktop/home/.config/fish/stale.fish"),
        reason: super::super::changeset::DeletionReason::SourceRemoved,
    });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    assert_eq!(result.deletions_completed, 1);
    assert!(!repo.join("desktop/home/.config/fish/stale.fish").exists());
}

#[test]
fn recovery_symlink_replaced_with_file() {
    // Simulates a type change: destination has a symlink from a previous
    // run, but source is now a regular file.
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(repo.join("desktop/home/.config")).unwrap();

    // Source is now a file.
    std::fs::write(home.join(".config/entry"), "file content").unwrap();
    // Destination has a symlink from before.
    std::os::unix::fs::symlink("/old/target", repo.join("desktop/home/.config/entry")).unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset
        .modifications
        .push(super::super::changeset::Modification {
            source: home.join(".config/entry"),
            destination: repo.join("desktop/home/.config/entry"),
            change: super::super::changeset::ChangeKind::TypeChanged {
                old_type: super::super::changeset::EntryType::Symlink,
                new_type: super::super::changeset::EntryType::RegularFile,
            },
        });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    assert_eq!(result.copies_completed, 1);
    let meta = std::fs::symlink_metadata(repo.join("desktop/home/.config/entry")).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(
        std::fs::read_to_string(repo.join("desktop/home/.config/entry")).unwrap(),
        "file content"
    );
}

#[test]
fn recovery_file_replaced_with_symlink() {
    // Simulates a type change: destination has a regular file, source is
    // now a symlink.
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(repo.join("desktop/home/.config")).unwrap();

    // Source is now a symlink.
    std::os::unix::fs::symlink("/new/target", home.join(".config/entry")).unwrap();
    // Destination has a regular file from before.
    std::fs::write(repo.join("desktop/home/.config/entry"), "old file").unwrap();

    let sources = vec![crate::config::SourceConfig {
        path: ".config".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset
        .modifications
        .push(super::super::changeset::Modification {
            source: home.join(".config/entry"),
            destination: repo.join("desktop/home/.config/entry"),
            change: super::super::changeset::ChangeKind::TypeChanged {
                old_type: super::super::changeset::EntryType::RegularFile,
                new_type: super::super::changeset::EntryType::Symlink,
            },
        });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    assert!(result.may_publish);
    let meta = std::fs::symlink_metadata(repo.join("desktop/home/.config/entry")).unwrap();
    assert!(meta.file_type().is_symlink());
    assert_eq!(
        std::fs::read_link(repo.join("desktop/home/.config/entry")).unwrap(),
        PathBuf::from("/new/target")
    );
}

#[test]
fn recovery_already_deleted_file_is_idempotent() {
    // Simulates a deletion that already happened (e.g., partial previous
    // run deleted the file but crashed before finishing other operations).
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();
    std::fs::create_dir_all(repo.join("desktop/home/.config/fish")).unwrap();

    // The file to delete doesn't exist — already cleaned up.
    let sources = vec![crate::config::SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let mut changeset = super::super::changeset::ChangeSet::new();
    changeset.deletions.push(super::super::changeset::Deletion {
        destination: repo.join("desktop/home/.config/fish/already-gone.fish"),
        reason: super::super::changeset::DeletionReason::SourceRemoved,
    });

    let result = execute_mirror(&home, &repo, "desktop", &sources, &changeset).unwrap();

    // Idempotent: deletion of missing file succeeds.
    assert!(result.may_publish);
    assert_eq!(result.deletions_completed, 1);
}
