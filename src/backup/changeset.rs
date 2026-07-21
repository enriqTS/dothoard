//! Change-set model for backup planning.
//!
//! A [`ChangeSet`] represents the complete set of operations that a backup run
//! would perform. It is produced by the planner without modifying the
//! filesystem or invoking Git, making it safe for previews and dry runs.
//!
//! The model represents:
//! - Additions: new files or symlinks not yet in the destination.
//! - Modifications: existing files whose content, type, symlink target,
//!   or executable bit changed.
//! - Deletions: destination files whose source was removed or newly ignored.
//! - Exclusions: source files matched by ignore rules.
//! - Warnings: problems that do not prevent backup but require attention.

use std::path::PathBuf;

/// The complete set of planned changes for one backup run.
///
/// A change-set is deterministic: the same source state and configuration
/// always produce the same change-set in the same order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    /// Files and symlinks that would be added to the destination.
    pub additions: Vec<Addition>,

    /// Files and symlinks that would be updated in the destination.
    pub modifications: Vec<Modification>,

    /// Files and symlinks that would be removed from the destination.
    pub deletions: Vec<Deletion>,

    /// Source paths that matched ignore rules and were excluded.
    pub exclusions: Vec<Exclusion>,

    /// Non-fatal problems detected during planning.
    pub warnings: Vec<PlanWarning>,
}

impl ChangeSet {
    /// Create an empty change-set.
    pub fn new() -> Self {
        Self {
            additions: Vec::new(),
            modifications: Vec::new(),
            deletions: Vec::new(),
            exclusions: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Returns `true` if the change-set contains no operations.
    pub fn is_empty(&self) -> bool {
        self.additions.is_empty() && self.modifications.is_empty() && self.deletions.is_empty()
    }

    /// Total number of filesystem operations planned (additions + modifications + deletions).
    pub fn operation_count(&self) -> usize {
        self.additions.len() + self.modifications.len() + self.deletions.len()
    }

    /// Sort all entries for deterministic output ordering.
    ///
    /// Entries within each category are sorted by their destination path.
    pub fn sort(&mut self) {
        self.additions
            .sort_by(|a, b| a.destination.cmp(&b.destination));
        self.modifications
            .sort_by(|a, b| a.destination.cmp(&b.destination));
        self.deletions
            .sort_by(|a, b| a.destination.cmp(&b.destination));
        self.exclusions.sort_by(|a, b| a.source.cmp(&b.source));
        self.warnings.sort_by(|a, b| a.path.cmp(&b.path));
    }
}

impl Default for ChangeSet {
    fn default() -> Self {
        Self::new()
    }
}

/// A file or symlink that would be added to the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addition {
    /// Absolute path in the source.
    pub source: PathBuf,

    /// Absolute path in the destination (under `repository/home/`).
    pub destination: PathBuf,

    /// The type of entry being added.
    pub entry_type: EntryType,
}

/// A file or symlink that would be updated in the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modification {
    /// Absolute path in the source.
    pub source: PathBuf,

    /// Absolute path in the destination (under `repository/home/`).
    pub destination: PathBuf,

    /// What kind of change was detected.
    pub change: ChangeKind,
}

/// A file or symlink that would be removed from the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deletion {
    /// Absolute path in the destination that would be removed.
    pub destination: PathBuf,

    /// Why this entry is being deleted.
    pub reason: DeletionReason,
}

/// A source path that was excluded by an ignore rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exclusion {
    /// Absolute source path that was excluded.
    pub source: PathBuf,

    /// The type of entry that was excluded.
    pub entry_type: EntryType,

    /// The reason for exclusion.
    pub reason: ExclusionReason,
}

/// A non-fatal problem detected during planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanWarning {
    /// The path associated with the warning.
    pub path: PathBuf,

    /// What the warning is about.
    pub kind: WarningKind,
}

/// Classification of a filesystem entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryType {
    /// A regular file.
    RegularFile,

    /// A regular file with the executable bit set.
    ExecutableFile,

    /// A symbolic link (target is not followed).
    Symlink,
}

impl EntryType {
    /// Returns `true` if this is any kind of regular file (executable or not).
    pub fn is_file(self) -> bool {
        matches!(self, Self::RegularFile | Self::ExecutableFile)
    }

    /// Returns `true` if this is a symlink.
    pub fn is_symlink(self) -> bool {
        matches!(self, Self::Symlink)
    }
}

impl std::fmt::Display for EntryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RegularFile => write!(f, "file"),
            Self::ExecutableFile => write!(f, "executable"),
            Self::Symlink => write!(f, "symlink"),
        }
    }
}

/// The kind of change detected for a modification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// File content changed (same type, different bytes).
    ContentChanged,

    /// The executable bit changed (file gained or lost the executable flag).
    ExecutableBitChanged {
        /// Whether the file is now executable.
        now_executable: bool,
    },

    /// A symlink's target path changed.
    SymlinkTargetChanged {
        /// The old target path.
        old_target: PathBuf,
        /// The new target path.
        new_target: PathBuf,
    },

    /// The entry type changed (e.g., regular file became symlink or vice versa).
    TypeChanged {
        /// What the entry was in the destination.
        old_type: EntryType,
        /// What the entry is now in the source.
        new_type: EntryType,
    },

    /// Both content and the executable bit changed.
    ContentAndExecutableBitChanged {
        /// Whether the file is now executable.
        now_executable: bool,
    },
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContentChanged => write!(f, "content changed"),
            Self::ExecutableBitChanged { now_executable } => {
                if *now_executable {
                    write!(f, "became executable")
                } else {
                    write!(f, "lost executable bit")
                }
            }
            Self::SymlinkTargetChanged {
                old_target,
                new_target,
            } => {
                write!(
                    f,
                    "symlink target changed: {} -> {}",
                    old_target.display(),
                    new_target.display()
                )
            }
            Self::TypeChanged { old_type, new_type } => {
                write!(f, "type changed: {old_type} -> {new_type}")
            }
            Self::ContentAndExecutableBitChanged { now_executable } => {
                if *now_executable {
                    write!(f, "content changed, became executable")
                } else {
                    write!(f, "content changed, lost executable bit")
                }
            }
        }
    }
}

/// Why a destination file is scheduled for deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeletionReason {
    /// The source file no longer exists (removed from the source directory).
    SourceRemoved,

    /// The source file now matches an ignore rule.
    NewlyIgnored,
}

impl std::fmt::Display for DeletionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceRemoved => write!(f, "source removed"),
            Self::NewlyIgnored => write!(f, "newly ignored"),
        }
    }
}

/// Why a source path was excluded from the backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExclusionReason {
    /// Matched a user-configured ignore pattern.
    IgnorePattern {
        /// The pattern that matched.
        pattern: String,
    },

    /// A nested `.git` directory (hard exclusion, cannot be negated).
    NestedGitDirectory,

    /// An unsupported special file (socket, device, FIFO, etc.).
    UnsupportedSpecialFile,
}

impl std::fmt::Display for ExclusionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IgnorePattern { pattern } => write!(f, "matched ignore pattern: {pattern}"),
            Self::NestedGitDirectory => write!(f, "nested .git directory"),
            Self::UnsupportedSpecialFile => write!(f, "unsupported special file"),
        }
    }
}

/// Classification of non-fatal warnings during planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// A file that looks like it might contain secrets.
    PossibleSecret {
        /// Why it was flagged (e.g., filename pattern).
        reason: String,
    },

    /// A source root is missing (backup is preserved, not deleted).
    MissingSourceRoot {
        /// The home-relative source path that is missing.
        source_path: String,
    },

    /// An ignored file is already tracked in the destination.
    IgnoredButTracked,

    /// An unsupported special file was encountered and skipped.
    SkippedSpecialFile {
        /// Description of the file type.
        file_type: String,
    },

    /// A symlink destination escape attempt was detected.
    DestinationSymlinkEscape,
}

impl std::fmt::Display for WarningKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PossibleSecret { reason } => write!(f, "possible secret: {reason}"),
            Self::MissingSourceRoot { source_path } => {
                write!(f, "source root missing: {source_path}")
            }
            Self::IgnoredButTracked => write!(f, "ignored but already tracked in destination"),
            Self::SkippedSpecialFile { file_type } => {
                write!(f, "skipped unsupported file type: {file_type}")
            }
            Self::DestinationSymlinkEscape => {
                write!(f, "destination path contains a symlink (escape attempt)")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_changeset_reports_empty() {
        let cs = ChangeSet::new();
        assert!(cs.is_empty());
        assert_eq!(cs.operation_count(), 0);
    }

    #[test]
    fn changeset_with_additions_is_not_empty() {
        let mut cs = ChangeSet::new();
        cs.additions.push(Addition {
            source: PathBuf::from("/home/user/.bashrc"),
            destination: PathBuf::from("/repo/home/.bashrc"),
            entry_type: EntryType::RegularFile,
        });
        assert!(!cs.is_empty());
        assert_eq!(cs.operation_count(), 1);
    }

    #[test]
    fn changeset_with_modifications_is_not_empty() {
        let mut cs = ChangeSet::new();
        cs.modifications.push(Modification {
            source: PathBuf::from("/home/user/.bashrc"),
            destination: PathBuf::from("/repo/home/.bashrc"),
            change: ChangeKind::ContentChanged,
        });
        assert!(!cs.is_empty());
        assert_eq!(cs.operation_count(), 1);
    }

    #[test]
    fn changeset_with_deletions_is_not_empty() {
        let mut cs = ChangeSet::new();
        cs.deletions.push(Deletion {
            destination: PathBuf::from("/repo/home/.old"),
            reason: DeletionReason::SourceRemoved,
        });
        assert!(!cs.is_empty());
        assert_eq!(cs.operation_count(), 1);
    }

    #[test]
    fn exclusions_and_warnings_do_not_count_as_operations() {
        let mut cs = ChangeSet::new();
        cs.exclusions.push(Exclusion {
            source: PathBuf::from("/home/user/.config/secret"),
            entry_type: EntryType::RegularFile,
            reason: ExclusionReason::IgnorePattern {
                pattern: "*secret*".to_string(),
            },
        });
        cs.warnings.push(PlanWarning {
            path: PathBuf::from("/home/user/.ssh/id_rsa"),
            kind: WarningKind::PossibleSecret {
                reason: "private key filename".to_string(),
            },
        });
        assert!(cs.is_empty());
        assert_eq!(cs.operation_count(), 0);
    }

    #[test]
    fn operation_count_sums_all_categories() {
        let mut cs = ChangeSet::new();
        cs.additions.push(Addition {
            source: PathBuf::from("/home/user/a"),
            destination: PathBuf::from("/repo/home/a"),
            entry_type: EntryType::RegularFile,
        });
        cs.additions.push(Addition {
            source: PathBuf::from("/home/user/b"),
            destination: PathBuf::from("/repo/home/b"),
            entry_type: EntryType::Symlink,
        });
        cs.modifications.push(Modification {
            source: PathBuf::from("/home/user/c"),
            destination: PathBuf::from("/repo/home/c"),
            change: ChangeKind::ContentChanged,
        });
        cs.deletions.push(Deletion {
            destination: PathBuf::from("/repo/home/d"),
            reason: DeletionReason::SourceRemoved,
        });
        assert_eq!(cs.operation_count(), 4);
    }

    #[test]
    fn sort_orders_by_destination_path() {
        let mut cs = ChangeSet::new();
        cs.additions.push(Addition {
            source: PathBuf::from("/home/user/z"),
            destination: PathBuf::from("/repo/home/z"),
            entry_type: EntryType::RegularFile,
        });
        cs.additions.push(Addition {
            source: PathBuf::from("/home/user/a"),
            destination: PathBuf::from("/repo/home/a"),
            entry_type: EntryType::RegularFile,
        });
        cs.deletions.push(Deletion {
            destination: PathBuf::from("/repo/home/m"),
            reason: DeletionReason::SourceRemoved,
        });
        cs.deletions.push(Deletion {
            destination: PathBuf::from("/repo/home/b"),
            reason: DeletionReason::NewlyIgnored,
        });

        cs.sort();

        assert_eq!(cs.additions[0].destination, PathBuf::from("/repo/home/a"));
        assert_eq!(cs.additions[1].destination, PathBuf::from("/repo/home/z"));
        assert_eq!(cs.deletions[0].destination, PathBuf::from("/repo/home/b"));
        assert_eq!(cs.deletions[1].destination, PathBuf::from("/repo/home/m"));
    }

    #[test]
    fn entry_type_display() {
        assert_eq!(format!("{}", EntryType::RegularFile), "file");
        assert_eq!(format!("{}", EntryType::ExecutableFile), "executable");
        assert_eq!(format!("{}", EntryType::Symlink), "symlink");
    }

    #[test]
    fn entry_type_predicates() {
        assert!(EntryType::RegularFile.is_file());
        assert!(EntryType::ExecutableFile.is_file());
        assert!(!EntryType::Symlink.is_file());
        assert!(EntryType::Symlink.is_symlink());
        assert!(!EntryType::RegularFile.is_symlink());
    }

    #[test]
    fn change_kind_display() {
        assert_eq!(format!("{}", ChangeKind::ContentChanged), "content changed");
        assert_eq!(
            format!(
                "{}",
                ChangeKind::ExecutableBitChanged {
                    now_executable: true
                }
            ),
            "became executable"
        );
        assert_eq!(
            format!(
                "{}",
                ChangeKind::ExecutableBitChanged {
                    now_executable: false
                }
            ),
            "lost executable bit"
        );
        assert_eq!(
            format!(
                "{}",
                ChangeKind::TypeChanged {
                    old_type: EntryType::RegularFile,
                    new_type: EntryType::Symlink,
                }
            ),
            "type changed: file -> symlink"
        );
    }

    #[test]
    fn deletion_reason_display() {
        assert_eq!(
            format!("{}", DeletionReason::SourceRemoved),
            "source removed"
        );
        assert_eq!(format!("{}", DeletionReason::NewlyIgnored), "newly ignored");
    }

    #[test]
    fn exclusion_reason_display() {
        assert_eq!(
            format!(
                "{}",
                ExclusionReason::IgnorePattern {
                    pattern: "*.log".to_string()
                }
            ),
            "matched ignore pattern: *.log"
        );
        assert_eq!(
            format!("{}", ExclusionReason::NestedGitDirectory),
            "nested .git directory"
        );
        assert_eq!(
            format!("{}", ExclusionReason::UnsupportedSpecialFile),
            "unsupported special file"
        );
    }

    #[test]
    fn warning_kind_display() {
        assert_eq!(
            format!(
                "{}",
                WarningKind::PossibleSecret {
                    reason: "private key".to_string()
                }
            ),
            "possible secret: private key"
        );
        assert_eq!(
            format!(
                "{}",
                WarningKind::MissingSourceRoot {
                    source_path: ".config/old".to_string()
                }
            ),
            "source root missing: .config/old"
        );
        assert_eq!(
            format!("{}", WarningKind::IgnoredButTracked),
            "ignored but already tracked in destination"
        );
    }

    #[test]
    fn default_creates_empty_changeset() {
        let cs = ChangeSet::default();
        assert!(cs.is_empty());
        assert_eq!(cs.operation_count(), 0);
        assert!(cs.exclusions.is_empty());
        assert!(cs.warnings.is_empty());
    }

    #[test]
    fn symlink_target_change_display() {
        let kind = ChangeKind::SymlinkTargetChanged {
            old_target: PathBuf::from("/old/target"),
            new_target: PathBuf::from("/new/target"),
        };
        assert_eq!(
            format!("{kind}"),
            "symlink target changed: /old/target -> /new/target"
        );
    }

    #[test]
    fn content_and_executable_bit_change_display() {
        let gained = ChangeKind::ContentAndExecutableBitChanged {
            now_executable: true,
        };
        assert_eq!(format!("{gained}"), "content changed, became executable");

        let lost = ChangeKind::ContentAndExecutableBitChanged {
            now_executable: false,
        };
        assert_eq!(format!("{lost}"), "content changed, lost executable bit");
    }
}
