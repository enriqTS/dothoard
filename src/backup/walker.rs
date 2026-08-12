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
    dir_entries.sort_by_key(|e| e.file_name());

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
#[path = "../../tests/unit/backup/walker.rs"]
mod tests;
