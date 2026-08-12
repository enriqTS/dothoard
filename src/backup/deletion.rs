//! Deletion planning for backup destinations.
//!
//! Determines which destination files should be deleted based on:
//!
//! - **Source removed:** A file exists in the destination but no longer exists
//!   in the source (child was deleted from the source directory).
//! - **Newly ignored:** A file exists in the destination but now matches an
//!   ignore rule (the file should be removed from the backup).
//!
//! The planner also enforces the safety rule that a missing source root never
//! causes deletion of its entire backup — the backup is preserved and an error
//! is reported instead.

use std::collections::HashSet;
use std::path::PathBuf;

use super::changeset::{Deletion, DeletionReason, PlanWarning, WarningKind};
use super::ignore::IgnoreMatcher;
use super::inventory::{DestinationMeta, EntryMeta};

/// Plan deletions for a single source's destination directory.
///
/// Compares the destination inventory against the source inventory to find
/// entries that should be removed. A destination entry is scheduled for
/// deletion when:
///
/// 1. It has no corresponding source entry (the source file was removed).
/// 2. Its corresponding source path now matches an ignore rule (newly ignored).
///
/// # Arguments
///
/// * `source_entries` - The filtered source inventory (entries that passed
///   ignore matching).
/// * `destination_entries` - The existing destination inventory.
/// * `ignore_matcher` - The ignore matcher for this source (used to detect
///   newly ignored files).
///
/// # Returns
///
/// A list of planned deletions and any associated warnings.
pub fn plan_deletions(
    source_entries: &[EntryMeta],
    destination_entries: &[DestinationMeta],
    ignore_matcher: &IgnoreMatcher,
) -> (Vec<Deletion>, Vec<PlanWarning>) {
    let mut deletions = Vec::new();
    let mut warnings = Vec::new();

    // Build a set of relative paths present in the source for quick lookup.
    let source_paths: HashSet<&PathBuf> = source_entries.iter().map(|e| &e.relative_path).collect();

    for dest_entry in destination_entries {
        if source_paths.contains(&dest_entry.relative_path) {
            // Source still has this file — not a deletion candidate.
            continue;
        }

        // Destination file has no corresponding source. Determine why.
        let is_dir = false; // Destination entries collected by walker are files/symlinks.
        let reason = if ignore_matcher.is_ignored(&dest_entry.relative_path, is_dir) {
            DeletionReason::NewlyIgnored
        } else {
            DeletionReason::SourceRemoved
        };

        // Warn about newly ignored files that were tracked (they exist in dest).
        if matches!(reason, DeletionReason::NewlyIgnored) {
            warnings.push(PlanWarning {
                path: dest_entry.destination_path.clone(),
                kind: WarningKind::IgnoredButTracked,
            });
        }

        deletions.push(Deletion {
            destination: dest_entry.destination_path.clone(),
            reason,
        });
    }

    (deletions, warnings)
}

/// Check whether a source root is missing and return a warning if so.
///
/// When a configured source root does not exist, the backup for that source
/// is preserved (no deletions) and an error/warning is generated. This
/// prevents accidental deletion of an entire backup when a source is
/// temporarily unavailable.
///
/// Returns `Some(warning)` if the source root is missing, `None` otherwise.
pub fn check_missing_source_root(
    source_root: &std::path::Path,
    relative_source: &str,
) -> Option<PlanWarning> {
    // Use symlink_metadata to detect existence without following links.
    if std::fs::symlink_metadata(source_root).is_err() {
        return Some(PlanWarning {
            path: source_root.to_path_buf(),
            kind: WarningKind::MissingSourceRoot {
                source_path: relative_source.to_string(),
            },
        });
    }
    None
}

#[cfg(test)]
#[path = "../../tests/unit/backup/deletion.rs"]
mod tests;
