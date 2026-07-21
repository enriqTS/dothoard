//! No-follow source directory walker.
//!
//! Recursively walks a source directory collecting entries without following
//! symlinks. The walker:
//!
//! - Includes hidden files and directories (dotfiles).
//! - Preserves symlinks as entries without reading their targets.
//! - Rejects unsupported special files (sockets, devices, FIFOs) with warnings.
//! - Never enters nested `.git` directories.
//! - Never follows symlinks during traversal (uses `symlink_metadata`).
//!
//! The output is a flat list of [`WalkEntry`] values representing every
//! discovered filesystem object, suitable for filtering and inventory.

use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// A single entry discovered by the source walker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkEntry {
    /// Absolute path to the entry.
    pub path: PathBuf,

    /// Path relative to the source root.
    pub relative: PathBuf,

    /// Classification of the entry.
    pub kind: WalkEntryKind,
}

/// Classification of a walked filesystem entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkEntryKind {
    /// A regular file (not executable).
    File,

    /// A regular file with the executable bit set.
    ExecutableFile,

    /// A symbolic link (target not followed or read during walk).
    Symlink,

    /// A directory (used internally for recursion but not emitted in results).
    Directory,

    /// A nested `.git` directory (hard exclusion).
    GitDirectory,

    /// An unsupported special file (socket, device, FIFO, etc.).
    SpecialFile {
        /// Human-readable description of the file type.
        file_type: String,
    },
}

impl WalkEntryKind {
    /// Returns `true` if this entry is a regular or executable file.
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File | Self::ExecutableFile)
    }

    /// Returns `true` if this entry is a symlink.
    pub fn is_symlink(&self) -> bool {
        matches!(self, Self::Symlink)
    }

    /// Returns `true` if this entry should be backed up (file or symlink).
    pub fn is_backupable(&self) -> bool {
        matches!(self, Self::File | Self::ExecutableFile | Self::Symlink)
    }
}

/// Errors that can occur during a source walk.
#[derive(Debug, Error)]
pub enum WalkError {
    #[error("failed to read directory {path}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read directory entry in {parent}")]
    ReadEntry {
        parent: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read metadata for {path}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Walk a source root directory recursively without following symlinks.
///
/// Returns a flat list of all discovered entries. Directories themselves are
/// not included in the output — only their contents. Nested `.git` directories
/// and special files are included as entries with their respective kinds so
/// callers can handle them (e.g., emit exclusions or warnings).
///
/// If `source_root` is a file or symlink (not a directory), returns a single
/// entry for that item.
///
/// # Errors
///
/// Returns an error if the source root cannot be read or if a critical I/O
/// error occurs. Individual entry errors within a directory are collected
/// and returned as part of the error list rather than aborting the entire walk.
pub fn walk_source(source_root: &Path) -> Result<(Vec<WalkEntry>, Vec<WalkError>), WalkError> {
    let meta = std::fs::symlink_metadata(source_root).map_err(|source| WalkError::Metadata {
        path: source_root.to_path_buf(),
        source,
    })?;

    // If the source root is a file or symlink, return it as a single entry.
    if !meta.is_dir() {
        let kind = classify_metadata(&meta, source_root);
        let entry = WalkEntry {
            path: source_root.to_path_buf(),
            relative: PathBuf::new(),
            kind,
        };
        return Ok((vec![entry], Vec::new()));
    }

    let mut entries = Vec::new();
    let mut errors = Vec::new();

    walk_recursive(source_root, source_root, &mut entries, &mut errors);

    // Sort for deterministic output.
    entries.sort_by(|a, b| a.relative.cmp(&b.relative));

    Ok((entries, errors))
}

/// Recursively walk a directory, collecting entries.
fn walk_recursive(
    root: &Path,
    current: &Path,
    entries: &mut Vec<WalkEntry>,
    errors: &mut Vec<WalkError>,
) {
    let read_dir = match std::fs::read_dir(current) {
        Ok(rd) => rd,
        Err(source) => {
            errors.push(WalkError::ReadDir {
                path: current.to_path_buf(),
                source,
            });
            return;
        }
    };

    // Collect and sort directory entries for deterministic ordering.
    let mut dir_entries: Vec<_> = Vec::new();
    for entry_result in read_dir {
        match entry_result {
            Ok(entry) => dir_entries.push(entry),
            Err(source) => {
                errors.push(WalkError::ReadEntry {
                    parent: current.to_path_buf(),
                    source,
                });
            }
        }
    }
    dir_entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    for dir_entry in dir_entries {
        let path = dir_entry.path();
        let relative = match path.strip_prefix(root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };

        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(source) => {
                errors.push(WalkError::Metadata {
                    path: path.clone(),
                    source,
                });
                continue;
            }
        };

        if meta.is_symlink() {
            // Symlinks are never followed during traversal.
            entries.push(WalkEntry {
                path,
                relative,
                kind: WalkEntryKind::Symlink,
            });
        } else if meta.is_dir() {
            // Check for nested .git directory (hard exclusion).
            if is_git_directory(&path) {
                entries.push(WalkEntry {
                    path,
                    relative,
                    kind: WalkEntryKind::GitDirectory,
                });
                // Do not recurse into .git directories.
            } else {
                // Recurse into regular directories.
                walk_recursive(root, &path, entries, errors);
            }
        } else if meta.is_file() {
            let kind = if is_executable(&meta) {
                WalkEntryKind::ExecutableFile
            } else {
                WalkEntryKind::File
            };
            entries.push(WalkEntry {
                path,
                relative,
                kind,
            });
        } else {
            // Special file (socket, device, FIFO, etc.)
            let file_type = describe_special_file(&meta);
            entries.push(WalkEntry {
                path,
                relative,
                kind: WalkEntryKind::SpecialFile { file_type },
            });
        }
    }
}

/// Classify metadata into a WalkEntryKind (for single-file source roots).
fn classify_metadata(meta: &Metadata, _path: &Path) -> WalkEntryKind {
    if meta.is_symlink() {
        WalkEntryKind::Symlink
    } else if meta.is_file() {
        if is_executable(meta) {
            WalkEntryKind::ExecutableFile
        } else {
            WalkEntryKind::File
        }
    } else if meta.is_dir() {
        WalkEntryKind::Directory
    } else {
        WalkEntryKind::SpecialFile {
            file_type: describe_special_file(meta),
        }
    }
}

/// Check whether a directory is a `.git` directory.
fn is_git_directory(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == ".git")
}

/// Check whether a file has the executable bit set (any of user/group/other).
fn is_executable(meta: &Metadata) -> bool {
    meta.mode() & 0o111 != 0
}

/// Describe a special file type from its metadata.
fn describe_special_file(meta: &Metadata) -> String {
    let mode = meta.mode();
    let file_type = mode & 0o170000;
    match file_type {
        0o140000 => "socket".to_string(),
        0o060000 => "block device".to_string(),
        0o020000 => "character device".to_string(),
        0o010000 => "FIFO".to_string(),
        _ => format!("unknown (mode: {mode:#o})"),
    }
}

#[cfg(test)]
mod tests {
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
            !entries.iter().any(|e| {
                e.relative.starts_with("link-dir") && e.kind != WalkEntryKind::Symlink
            })
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
            .any(|e| e.relative == PathBuf::from("file.txt") && e.kind == WalkEntryKind::File);
        let has_hidden = entries
            .iter()
            .any(|e| e.relative == PathBuf::from(".hidden") && e.kind == WalkEntryKind::File);
        let has_executable = entries.iter().any(|e| {
            e.relative == PathBuf::from("script.sh") && e.kind == WalkEntryKind::ExecutableFile
        });
        let has_symlink = entries
            .iter()
            .any(|e| e.relative == PathBuf::from("link") && e.kind == WalkEntryKind::Symlink);
        let has_git = entries
            .iter()
            .any(|e| e.relative == PathBuf::from(".git") && e.kind == WalkEntryKind::GitDirectory);
        let has_nested = entries
            .iter()
            .any(|e| e.relative == PathBuf::from("subdir/nested.txt"));
        let has_config = entries
            .iter()
            .any(|e| e.relative == PathBuf::from(".config/settings"));

        assert!(has_file, "missing file.txt");
        assert!(has_hidden, "missing .hidden");
        assert!(has_executable, "missing script.sh");
        assert!(has_symlink, "missing link");
        assert!(has_git, "missing .git");
        assert!(has_nested, "missing subdir/nested.txt");
        assert!(has_config, "missing .config/settings");

        // Verify .git contents are NOT present (only the .git entry itself)
        assert!(
            !entries.iter().any(|e| {
                e.relative.starts_with(".git") && e.kind != WalkEntryKind::GitDirectory
            })
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
                .any(|e| e.relative == PathBuf::from(".hidden-dir/sub/file.txt"))
        );
    }
}
