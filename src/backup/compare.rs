//! Content comparison between source and destination inventories.
//!
//! Compares pairs of source and destination entries to determine:
//! - Additions: source entries with no matching destination.
//! - Modifications: source entries whose content, type, symlink target,
//!   or executable bit differs from the destination.
//! - Unchanged: entries that are identical and need no action.
//!
//! Comparison uses byte-level content equality for regular files and raw
//! target equality for symlinks. The executable bit is compared separately
//! from content so that mode-only changes are detected.

use std::fs;
use std::io::Read;
use std::path::Path;

use super::changeset::{Addition, ChangeKind, EntryType, Modification};
use super::inventory::{DestinationMeta, EntryMeta};

/// Compare a source entry against a destination entry to detect changes.
///
/// Returns `None` if the entries are identical (no change needed).
/// Returns `Some(ChangeKind)` describing what changed.
pub fn compare_entries(source: &EntryMeta, destination: &DestinationMeta) -> Option<ChangeKind> {
    // Type change: file became symlink or vice versa.
    if source.entry_type.is_symlink() != destination.entry_type.is_symlink() {
        return Some(ChangeKind::TypeChanged {
            old_type: destination.entry_type,
            new_type: source.entry_type,
        });
    }

    // Both are symlinks — compare targets.
    if source.entry_type.is_symlink() && destination.entry_type.is_symlink() {
        let source_target = source.symlink_target.as_ref();
        let dest_target = destination.symlink_target.as_ref();
        if source_target != dest_target {
            return Some(ChangeKind::SymlinkTargetChanged {
                old_target: dest_target.cloned().unwrap_or_default(),
                new_target: source_target.cloned().unwrap_or_default(),
            });
        }
        return None; // Symlinks with same target are unchanged.
    }

    // Both are regular files — compare content and executable bit.
    let exec_changed = is_executable(source.entry_type) != is_executable(destination.entry_type);
    let content_changed = is_content_different(source, destination);

    match (content_changed, exec_changed) {
        (true, true) => Some(ChangeKind::ContentAndExecutableBitChanged {
            now_executable: is_executable(source.entry_type),
        }),
        (true, false) => Some(ChangeKind::ContentChanged),
        (false, true) => Some(ChangeKind::ExecutableBitChanged {
            now_executable: is_executable(source.entry_type),
        }),
        (false, false) => None, // Unchanged.
    }
}

/// Quick check whether content is different between source and destination.
///
/// Uses a size check first (cheap) and falls back to byte-by-byte comparison
/// only when sizes match.
fn is_content_different(source: &EntryMeta, destination: &DestinationMeta) -> bool {
    // Different size means different content.
    if source.size != destination.size {
        return true;
    }

    // Same size — need byte comparison.
    files_differ(&source.source_path, &destination.destination_path)
}

/// Compare two files byte-by-byte.
///
/// Returns `true` if the files have different content or if either cannot be read.
/// Treats read errors as "different" to trigger a re-copy.
fn files_differ(path_a: &Path, path_b: &Path) -> bool {
    const BUFFER_SIZE: usize = 8192;

    let mut file_a = match fs::File::open(path_a) {
        Ok(f) => f,
        Err(_) => return true, // Can't read = treat as different.
    };
    let mut file_b = match fs::File::open(path_b) {
        Ok(f) => f,
        Err(_) => return true,
    };

    let mut buf_a = [0u8; BUFFER_SIZE];
    let mut buf_b = [0u8; BUFFER_SIZE];

    loop {
        let n_a = match file_a.read(&mut buf_a) {
            Ok(n) => n,
            Err(_) => return true,
        };
        let n_b = match file_b.read(&mut buf_b) {
            Ok(n) => n,
            Err(_) => return true,
        };

        if n_a != n_b {
            return true;
        }
        if n_a == 0 {
            return false; // Both reached EOF at the same position.
        }
        if buf_a[..n_a] != buf_b[..n_b] {
            return true;
        }
    }
}

/// Check if an EntryType represents an executable file.
fn is_executable(entry_type: EntryType) -> bool {
    matches!(entry_type, EntryType::ExecutableFile)
}

/// Create an Addition from a source entry that has no matching destination.
pub fn make_addition(source: &EntryMeta, destination_path: std::path::PathBuf) -> Addition {
    Addition {
        source: source.source_path.clone(),
        destination: destination_path,
        entry_type: source.entry_type,
    }
}

/// Create a Modification from a source entry that differs from its destination.
pub fn make_modification(
    source: &EntryMeta,
    destination_path: std::path::PathBuf,
    change: ChangeKind,
) -> Modification {
    Modification {
        source: source.source_path.clone(),
        destination: destination_path,
        change,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/backup/compare.rs"]
mod tests;
