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
