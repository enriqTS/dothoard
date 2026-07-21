//! Source and destination inventory collection.
//!
//! An inventory is a snapshot of filesystem state collected without modifying
//! anything. It provides the comparison metadata needed by the planner to
//! determine additions, modifications, and deletions.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::changeset::{EntryType, Exclusion, ExclusionReason, PlanWarning, WarningKind};
use super::ignore::{IgnoreMatcher, is_hard_excluded_git, is_hard_excluded_special};
use super::walker::{WalkEntry, WalkEntryKind, WalkError, walk_source};

/// Metadata for a single inventoried entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMeta {
    /// Absolute path to the entry in the source.
    pub source_path: PathBuf,

    /// Path relative to the source root.
    pub relative_path: PathBuf,

    /// Classification of the entry.
    pub entry_type: EntryType,

    /// Size in bytes (0 for symlinks).
    pub size: u64,

    /// Modification time as seconds since epoch (for quick-skip comparison).
    pub mtime_secs: i64,

    /// Symlink target (only set for symlinks).
    pub symlink_target: Option<PathBuf>,
}

/// Metadata for a destination entry (existing backup content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationMeta {
    /// Absolute path in the destination (under repository/home/).
    pub destination_path: PathBuf,

    /// Path relative to the managed home directory.
    pub relative_path: PathBuf,

    /// Classification of the entry.
    pub entry_type: EntryType,

    /// Size in bytes (0 for symlinks).
    pub size: u64,

    /// Modification time as seconds since epoch.
    pub mtime_secs: i64,

    /// Symlink target (only set for symlinks).
    pub symlink_target: Option<PathBuf>,
}

/// Result of collecting a source inventory.
#[derive(Debug)]
pub struct SourceInventory {
    /// Entries that passed filtering and should be considered for backup.
    pub entries: Vec<EntryMeta>,

    /// Entries that were excluded by ignore rules or hard exclusions.
    pub exclusions: Vec<Exclusion>,

    /// Non-fatal warnings encountered during inventory.
    pub warnings: Vec<PlanWarning>,

    /// Non-fatal walk errors (e.g., permission denied on a subdirectory).
    pub walk_errors: Vec<WalkError>,
}

/// Result of collecting a destination inventory.
#[derive(Debug)]
pub struct DestinationInventory {
    /// Existing entries in the destination directory.
    pub entries: Vec<DestinationMeta>,

    /// Non-fatal warnings encountered during inventory.
    pub warnings: Vec<PlanWarning>,
}

/// Errors that prevent inventory collection.
#[derive(Debug, Error)]
pub enum InventoryError {
    #[error("failed to walk source at {path}")]
    WalkFailed {
        path: PathBuf,
        #[source]
        source: WalkError,
    },

    #[error("failed to read symlink target at {path}")]
    ReadLink {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Collect the source inventory for a single source directory.
///
/// Walks the source, applies ignore rules, collects metadata for passing
/// entries, and records exclusions and warnings.
///
/// The `source_root` must be an absolute path (either a directory or file).
pub fn collect_source_inventory(
    source_root: &Path,
    ignore_matcher: &IgnoreMatcher,
) -> Result<SourceInventory, InventoryError> {
    let (walk_entries, walk_errors) =
        walk_source(source_root).map_err(|source| InventoryError::WalkFailed {
            path: source_root.to_path_buf(),
            source,
        })?;

    let mut entries = Vec::new();
    let mut exclusions = Vec::new();
    let mut warnings = Vec::new();

    for walk_entry in walk_entries {
        match classify_and_filter(&walk_entry, ignore_matcher) {
            FilterResult::Include => {
                match collect_entry_meta(&walk_entry, source_root) {
                    Ok(meta) => entries.push(meta),
                    Err(InventoryError::ReadLink { path, source: _ }) => {
                        // Non-fatal: record a warning and skip the entry.
                        warnings.push(PlanWarning {
                            path,
                            kind: WarningKind::SkippedSpecialFile {
                                file_type: "unreadable symlink".to_string(),
                            },
                        });
                    }
                    Err(e) => return Err(e),
                }
            }
            FilterResult::Exclude(exclusion) => {
                exclusions.push(exclusion);
            }
            FilterResult::Warn(warning) => {
                warnings.push(warning);
            }
        }
    }

    Ok(SourceInventory {
        entries,
        exclusions,
        warnings,
        walk_errors,
    })
}

/// Collect the destination inventory for an existing backup directory.
///
/// Walks the destination directory (under `repository/home/`) without following
/// symlinks, collecting metadata for comparison against the source inventory.
///
/// Returns an empty inventory if the destination directory does not exist.
pub fn collect_destination_inventory(
    destination_root: &Path,
) -> Result<DestinationInventory, InventoryError> {
    if !destination_root.exists() {
        return Ok(DestinationInventory {
            entries: Vec::new(),
            warnings: Vec::new(),
        });
    }

    let (walk_entries, _walk_errors) =
        walk_source(destination_root).map_err(|source| InventoryError::WalkFailed {
            path: destination_root.to_path_buf(),
            source,
        })?;

    let mut entries = Vec::new();
    let mut warnings = Vec::new();

    for walk_entry in walk_entries {
        // In the destination, we don't apply ignore rules — we just collect what's there.
        // Skip .git and special files with warnings.
        if matches!(walk_entry.kind, WalkEntryKind::GitDirectory) {
            continue;
        }
        if matches!(walk_entry.kind, WalkEntryKind::SpecialFile { .. }) {
            warnings.push(PlanWarning {
                path: walk_entry.path.clone(),
                kind: WarningKind::SkippedSpecialFile {
                    file_type: match &walk_entry.kind {
                        WalkEntryKind::SpecialFile { file_type } => file_type.clone(),
                        _ => unreachable!(),
                    },
                },
            });
            continue;
        }

        match collect_destination_meta(&walk_entry, destination_root) {
            Ok(meta) => entries.push(meta),
            Err(InventoryError::ReadLink { path, source: _ }) => {
                warnings.push(PlanWarning {
                    path,
                    kind: WarningKind::SkippedSpecialFile {
                        file_type: "unreadable symlink".to_string(),
                    },
                });
            }
            Err(e) => return Err(e),
        }
    }

    Ok(DestinationInventory { entries, warnings })
}

/// Outcome of classifying and filtering a walked entry.
enum FilterResult {
    /// The entry passes all filters and should be inventoried.
    Include,
    /// The entry is excluded and should be recorded as an exclusion.
    Exclude(Exclusion),
    /// The entry generates a warning (e.g., special file skipped).
    Warn(PlanWarning),
}

/// Classify a walk entry and determine whether it should be included.
fn classify_and_filter(entry: &WalkEntry, ignore_matcher: &IgnoreMatcher) -> FilterResult {
    // Hard exclusion: nested .git directories (cannot be negated).
    if matches!(entry.kind, WalkEntryKind::GitDirectory) {
        return FilterResult::Exclude(Exclusion {
            source: entry.path.clone(),
            entry_type: EntryType::RegularFile, // Directories aren't backed up
            reason: ExclusionReason::NestedGitDirectory,
        });
    }

    // Hard exclusion: unsupported special files (cannot be negated).
    if is_hard_excluded_special(&entry.kind) {
        let file_type = match &entry.kind {
            WalkEntryKind::SpecialFile { file_type } => file_type.clone(),
            _ => "unknown".to_string(),
        };
        return FilterResult::Warn(PlanWarning {
            path: entry.path.clone(),
            kind: WarningKind::SkippedSpecialFile { file_type },
        });
    }

    // Hard exclusion check via path components (catches .git in deeper paths).
    if is_hard_excluded_git(&entry.relative) {
        return FilterResult::Exclude(Exclusion {
            source: entry.path.clone(),
            entry_type: walk_kind_to_entry_type(&entry.kind),
            reason: ExclusionReason::NestedGitDirectory,
        });
    }

    // User-configured ignore patterns.
    let is_dir = matches!(entry.kind, WalkEntryKind::Directory);
    if ignore_matcher.is_ignored(&entry.relative, is_dir) {
        let result = ignore_matcher.matches(&entry.relative, is_dir);
        let pattern = match result {
            super::ignore::MatchResult::Ignored { pattern } => pattern,
            _ => String::new(),
        };
        return FilterResult::Exclude(Exclusion {
            source: entry.path.clone(),
            entry_type: walk_kind_to_entry_type(&entry.kind),
            reason: ExclusionReason::IgnorePattern { pattern },
        });
    }

    FilterResult::Include
}

/// Convert a WalkEntryKind to an EntryType for the changeset model.
fn walk_kind_to_entry_type(kind: &WalkEntryKind) -> EntryType {
    match kind {
        WalkEntryKind::File => EntryType::RegularFile,
        WalkEntryKind::ExecutableFile => EntryType::ExecutableFile,
        WalkEntryKind::Symlink => EntryType::Symlink,
        _ => EntryType::RegularFile,
    }
}

/// Collect metadata for a source entry.
fn collect_entry_meta(entry: &WalkEntry, source_root: &Path) -> Result<EntryMeta, InventoryError> {
    let entry_type = walk_kind_to_entry_type(&entry.kind);

    let (size, mtime_secs) = if entry.kind.is_symlink() {
        // For symlinks, use symlink_metadata (already done in walker).
        let meta =
            std::fs::symlink_metadata(&entry.path).map_err(|source| InventoryError::ReadLink {
                path: entry.path.clone(),
                source,
            })?;
        (0, meta.mtime())
    } else {
        let meta =
            std::fs::symlink_metadata(&entry.path).map_err(|source| InventoryError::ReadLink {
                path: entry.path.clone(),
                source,
            })?;
        (meta.size(), meta.mtime())
    };

    let symlink_target = if entry.kind.is_symlink() {
        let target =
            std::fs::read_link(&entry.path).map_err(|source| InventoryError::ReadLink {
                path: entry.path.clone(),
                source,
            })?;
        Some(target)
    } else {
        None
    };

    // Compute relative path: for single-file sources, use an empty relative path.
    let relative_path = if entry.relative.as_os_str().is_empty() {
        // Single-file source root — use the file name.
        source_root
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_default()
    } else {
        entry.relative.clone()
    };

    Ok(EntryMeta {
        source_path: entry.path.clone(),
        relative_path,
        entry_type,
        size,
        mtime_secs,
        symlink_target,
    })
}

/// Collect metadata for a destination entry.
fn collect_destination_meta(
    entry: &WalkEntry,
    destination_root: &Path,
) -> Result<DestinationMeta, InventoryError> {
    let entry_type = walk_kind_to_entry_type(&entry.kind);

    let (size, mtime_secs) = if entry.kind.is_symlink() {
        let meta =
            std::fs::symlink_metadata(&entry.path).map_err(|source| InventoryError::ReadLink {
                path: entry.path.clone(),
                source,
            })?;
        (0, meta.mtime())
    } else {
        let meta =
            std::fs::symlink_metadata(&entry.path).map_err(|source| InventoryError::ReadLink {
                path: entry.path.clone(),
                source,
            })?;
        (meta.size(), meta.mtime())
    };

    let symlink_target = if entry.kind.is_symlink() {
        let target =
            std::fs::read_link(&entry.path).map_err(|source| InventoryError::ReadLink {
                path: entry.path.clone(),
                source,
            })?;
        Some(target)
    } else {
        None
    };

    // Relative path within the destination root.
    let relative_path = if entry.relative.as_os_str().is_empty() {
        destination_root
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_default()
    } else {
        entry.relative.clone()
    };

    Ok(DestinationMeta {
        destination_path: entry.path.clone(),
        relative_path,
        entry_type,
        size,
        mtime_secs,
        symlink_target,
    })
}

#[cfg(test)]
mod tests {
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
}
