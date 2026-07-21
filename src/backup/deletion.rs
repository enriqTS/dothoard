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
mod tests {
    use super::*;
    use crate::backup::changeset::EntryType;
    use crate::backup::ignore::IgnoreMatcher;
    use std::path::Path;

    fn make_source_entry(relative: &str) -> EntryMeta {
        EntryMeta {
            source_path: PathBuf::from(format!("/home/user/{relative}")),
            relative_path: PathBuf::from(relative),
            entry_type: EntryType::RegularFile,
            size: 100,
            mtime_secs: 1000,
            symlink_target: None,
        }
    }

    fn make_dest_entry(relative: &str, dest_root: &str) -> DestinationMeta {
        DestinationMeta {
            destination_path: PathBuf::from(format!("{dest_root}/{relative}")),
            relative_path: PathBuf::from(relative),
            entry_type: EntryType::RegularFile,
            size: 100,
            mtime_secs: 1000,
            symlink_target: None,
        }
    }

    fn empty_matcher() -> IgnoreMatcher {
        let (m, _) = IgnoreMatcher::new(Path::new("/home/user/source"), &[]);
        m
    }

    fn matcher_with(patterns: &[&str]) -> IgnoreMatcher {
        let patterns: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
        let (m, _) = IgnoreMatcher::new(Path::new("/home/user/source"), &patterns);
        m
    }

    #[test]
    fn no_deletions_when_source_and_dest_match() {
        let source = vec![make_source_entry("a.txt"), make_source_entry("b.txt")];
        let dest = vec![
            make_dest_entry("a.txt", "/repo/home"),
            make_dest_entry("b.txt", "/repo/home"),
        ];

        let (deletions, warnings) = plan_deletions(&source, &dest, &empty_matcher());

        assert!(deletions.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn detects_source_removed_deletion() {
        let source = vec![make_source_entry("a.txt")];
        let dest = vec![
            make_dest_entry("a.txt", "/repo/home"),
            make_dest_entry("removed.txt", "/repo/home"),
        ];

        let (deletions, warnings) = plan_deletions(&source, &dest, &empty_matcher());

        assert_eq!(deletions.len(), 1);
        assert_eq!(
            deletions[0].destination,
            PathBuf::from("/repo/home/removed.txt")
        );
        assert_eq!(deletions[0].reason, DeletionReason::SourceRemoved);
        assert!(warnings.is_empty());
    }

    #[test]
    fn detects_newly_ignored_deletion() {
        // Source has "secret.key" but it's now ignored,
        // so it won't be in source_entries (filtered out).
        // The dest still has it.
        let source = vec![make_source_entry("config.toml")];
        let dest = vec![
            make_dest_entry("config.toml", "/repo/home"),
            make_dest_entry("secret.key", "/repo/home"),
        ];

        let m = matcher_with(&["*.key"]);
        let (deletions, warnings) = plan_deletions(&source, &dest, &m);

        assert_eq!(deletions.len(), 1);
        assert_eq!(
            deletions[0].destination,
            PathBuf::from("/repo/home/secret.key")
        );
        assert_eq!(deletions[0].reason, DeletionReason::NewlyIgnored);

        // Should warn about the newly ignored file being tracked
        assert_eq!(warnings.len(), 1);
        assert!(matches!(&warnings[0].kind, WarningKind::IgnoredButTracked));
    }

    #[test]
    fn multiple_deletions_mixed_reasons() {
        let source = vec![make_source_entry("keep.txt")];
        let dest = vec![
            make_dest_entry("keep.txt", "/repo/home"),
            make_dest_entry("deleted.txt", "/repo/home"),
            make_dest_entry("now_ignored.log", "/repo/home"),
        ];

        let m = matcher_with(&["*.log"]);
        let (deletions, warnings) = plan_deletions(&source, &dest, &m);

        assert_eq!(deletions.len(), 2);

        let removed: Vec<_> = deletions
            .iter()
            .filter(|d| d.reason == DeletionReason::SourceRemoved)
            .collect();
        let ignored: Vec<_> = deletions
            .iter()
            .filter(|d| d.reason == DeletionReason::NewlyIgnored)
            .collect();

        assert_eq!(removed.len(), 1);
        assert_eq!(ignored.len(), 1);
        assert_eq!(warnings.len(), 1); // Only the newly-ignored one warns
    }

    #[test]
    fn empty_destination_produces_no_deletions() {
        let source = vec![make_source_entry("new.txt")];
        let dest: Vec<DestinationMeta> = vec![];

        let (deletions, warnings) = plan_deletions(&source, &dest, &empty_matcher());

        assert!(deletions.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn empty_source_deletes_all_destination_entries() {
        let source: Vec<EntryMeta> = vec![];
        let dest = vec![
            make_dest_entry("a.txt", "/repo/home"),
            make_dest_entry("b.txt", "/repo/home"),
        ];

        let (deletions, _) = plan_deletions(&source, &dest, &empty_matcher());

        assert_eq!(deletions.len(), 2);
        assert!(
            deletions
                .iter()
                .all(|d| d.reason == DeletionReason::SourceRemoved)
        );
    }

    #[test]
    fn nested_paths_handled_correctly() {
        let source = vec![make_source_entry("sub/keep.txt")];
        let dest = vec![
            make_dest_entry("sub/keep.txt", "/repo/home"),
            make_dest_entry("sub/removed.txt", "/repo/home"),
            make_dest_entry("other/gone.txt", "/repo/home"),
        ];

        let (deletions, _) = plan_deletions(&source, &dest, &empty_matcher());

        assert_eq!(deletions.len(), 2);
        let paths: Vec<_> = deletions.iter().map(|d| &d.destination).collect();
        assert!(paths.contains(&&PathBuf::from("/repo/home/sub/removed.txt")));
        assert!(paths.contains(&&PathBuf::from("/repo/home/other/gone.txt")));
    }

    // --- Missing source root tests ---

    #[test]
    fn missing_source_root_returns_warning() {
        let warning = check_missing_source_root(Path::new("/nonexistent/path"), ".config/missing");

        assert!(warning.is_some());
        let w = warning.unwrap();
        assert!(matches!(
            &w.kind,
            WarningKind::MissingSourceRoot { source_path } if source_path == ".config/missing"
        ));
    }

    #[test]
    fn existing_source_root_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("source")).unwrap();

        let warning = check_missing_source_root(&tmp.path().join("source"), ".config/fish");

        assert!(warning.is_none());
    }

    #[test]
    fn symlink_source_root_does_not_report_missing() {
        let tmp = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/some/target", tmp.path().join("link")).unwrap();

        // A source-root symlink exists (even if target is invalid) — not "missing"
        let warning = check_missing_source_root(&tmp.path().join("link"), ".config/link");

        assert!(warning.is_none());
    }
}
