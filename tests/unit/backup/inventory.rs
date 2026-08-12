use super::*;
use std::os::unix::fs::PermissionsExt;

fn empty_matcher(root: &Path) -> IgnoreMatcher {
    let (m, _) = IgnoreMatcher::new(root, &[]);
    m
}

fn matcher_with(root: &Path, patterns: &[&str]) -> IgnoreMatcher {
    let patterns: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
    let (m, _) = IgnoreMatcher::new(root, &patterns);
    m
}

#[test]
fn collects_regular_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
    std::fs::write(tmp.path().join("b.txt"), "world").unwrap();

    let m = empty_matcher(tmp.path());
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 2);
    assert!(inv.exclusions.is_empty());
    assert!(inv.warnings.is_empty());

    let a = inv
        .entries
        .iter()
        .find(|e| e.relative_path == Path::new("a.txt"))
        .unwrap();
    assert_eq!(a.entry_type, EntryType::RegularFile);
    assert_eq!(a.size, 5);
    assert!(a.symlink_target.is_none());
}

#[test]
fn collects_executable_files() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("run.sh");
    std::fs::write(&script, "#!/bin/bash").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let m = empty_matcher(tmp.path());
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(inv.entries[0].entry_type, EntryType::ExecutableFile);
}

#[test]
fn collects_symlinks_with_target() {
    let tmp = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/some/target", tmp.path().join("link")).unwrap();

    let m = empty_matcher(tmp.path());
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(inv.entries[0].entry_type, EntryType::Symlink);
    assert_eq!(
        inv.entries[0].symlink_target,
        Some(PathBuf::from("/some/target"))
    );
    assert_eq!(inv.entries[0].size, 0);
}

#[test]
fn excludes_ignored_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("keep.txt"), "keep").unwrap();
    std::fs::write(tmp.path().join("debug.log"), "log").unwrap();

    let m = matcher_with(tmp.path(), &["*.log"]);
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(inv.entries[0].relative_path, PathBuf::from("keep.txt"));

    assert_eq!(inv.exclusions.len(), 1);
    assert!(matches!(
        &inv.exclusions[0].reason,
        ExclusionReason::IgnorePattern { pattern } if pattern == "*.log"
    ));
}

#[test]
fn excludes_nested_git_directories() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git/objects")).unwrap();
    std::fs::write(tmp.path().join(".git/HEAD"), "ref").unwrap();
    std::fs::write(tmp.path().join("file.txt"), "data").unwrap();

    let m = empty_matcher(tmp.path());
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(inv.entries[0].relative_path, PathBuf::from("file.txt"));

    assert!(
        inv.exclusions
            .iter()
            .any(|e| matches!(e.reason, ExclusionReason::NestedGitDirectory))
    );
}

#[test]
fn warns_on_special_files() {
    let tmp = tempfile::tempdir().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&sock_path).unwrap();
    std::fs::write(tmp.path().join("normal.txt"), "ok").unwrap();

    let m = empty_matcher(tmp.path());
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(inv.entries[0].relative_path, PathBuf::from("normal.txt"));

    assert!(
        inv.warnings
            .iter()
            .any(|w| matches!(&w.kind, WarningKind::SkippedSpecialFile { .. }))
    );
}

#[test]
fn collects_nested_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("sub/deep")).unwrap();
    std::fs::write(tmp.path().join("sub/deep/file.txt"), "deep").unwrap();
    std::fs::write(tmp.path().join("top.txt"), "top").unwrap();

    let m = empty_matcher(tmp.path());
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 2);
    let paths: Vec<_> = inv.entries.iter().map(|e| &e.relative_path).collect();
    assert!(paths.contains(&&PathBuf::from("sub/deep/file.txt")));
    assert!(paths.contains(&&PathBuf::from("top.txt")));
}

#[test]
fn ignore_patterns_apply_to_nested_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("sub/data.log"), "log").unwrap();
    std::fs::write(tmp.path().join("sub/config.toml"), "cfg").unwrap();

    let m = matcher_with(tmp.path(), &["*.log"]);
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(
        inv.entries[0].relative_path,
        PathBuf::from("sub/config.toml")
    );
    assert_eq!(inv.exclusions.len(), 1);
}

#[test]
fn nonexistent_source_returns_error() {
    let m = empty_matcher(Path::new("/nonexistent"));
    let result = collect_source_inventory(Path::new("/nonexistent/source"), &m);
    assert!(result.is_err());
}

#[test]
fn empty_directory_produces_empty_inventory() {
    let tmp = tempfile::tempdir().unwrap();

    let m = empty_matcher(tmp.path());
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert!(inv.entries.is_empty());
    assert!(inv.exclusions.is_empty());
    assert!(inv.warnings.is_empty());
}

#[test]
fn mtime_is_populated() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "data").unwrap();

    let m = empty_matcher(tmp.path());
    let inv = collect_source_inventory(tmp.path(), &m).unwrap();

    assert_eq!(inv.entries.len(), 1);
    // mtime should be a reasonable recent timestamp
    assert!(inv.entries[0].mtime_secs > 0);
}

// --- Destination inventory tests ---

#[test]
fn destination_inventory_nonexistent_returns_empty() {
    let result = collect_destination_inventory(Path::new("/nonexistent/dest")).unwrap();
    assert!(result.entries.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn destination_inventory_collects_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "data").unwrap();
    std::fs::write(tmp.path().join("other.txt"), "other").unwrap();

    let inv = collect_destination_inventory(tmp.path()).unwrap();

    assert_eq!(inv.entries.len(), 2);
    let paths: Vec<_> = inv.entries.iter().map(|e| &e.relative_path).collect();
    assert!(paths.contains(&&PathBuf::from("file.txt")));
    assert!(paths.contains(&&PathBuf::from("other.txt")));
}

#[test]
fn destination_inventory_collects_symlinks() {
    let tmp = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/target", tmp.path().join("link")).unwrap();

    let inv = collect_destination_inventory(tmp.path()).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(inv.entries[0].entry_type, EntryType::Symlink);
    assert_eq!(
        inv.entries[0].symlink_target,
        Some(PathBuf::from("/target"))
    );
}

#[test]
fn destination_inventory_skips_git_directory() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join(".git/HEAD"), "ref").unwrap();
    std::fs::write(tmp.path().join("file.txt"), "data").unwrap();

    let inv = collect_destination_inventory(tmp.path()).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(inv.entries[0].relative_path, PathBuf::from("file.txt"));
}

#[test]
fn destination_inventory_nested_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
    std::fs::write(tmp.path().join("a/b/deep.txt"), "deep").unwrap();

    let inv = collect_destination_inventory(tmp.path()).unwrap();

    assert_eq!(inv.entries.len(), 1);
    assert_eq!(inv.entries[0].relative_path, PathBuf::from("a/b/deep.txt"));
}
