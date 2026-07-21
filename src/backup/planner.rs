//! Deterministic backup planner.
//!
//! The planner orchestrates all backup sub-components to produce a complete
//! [`ChangeSet`] representing what a backup run would do — without modifying
//! the filesystem or invoking Git.
//!
//! The same inputs always produce the same ordered output, making the planner
//! suitable for previews, dry runs, and testing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::SourceConfig;

use super::changeset::ChangeSet;
use super::compare::{compare_entries, make_addition, make_modification};
use super::deletion::{check_missing_source_root, plan_deletions};
use super::ignore::IgnoreMatcher;
use super::inventory::{InventoryError, collect_destination_inventory, collect_source_inventory};
use super::mapping;
use super::secrets::{detect_secret, make_secret_warning};

/// Errors that prevent the planner from producing a change-set.
#[derive(Debug, Error)]
pub enum PlanError {
    #[error("failed to inventory source \"{source}\": {source_err}")]
    SourceInventory {
        source: String,
        #[source]
        source_err: InventoryError,
    },

    #[error("failed to inventory destination for \"{source}\": {source_err}")]
    DestinationInventory {
        source: String,
        #[source]
        source_err: InventoryError,
    },
}

/// Configuration inputs for the planner.
#[derive(Debug)]
pub struct PlanInputs<'a> {
    /// Absolute path to the user's home directory.
    pub home: &'a Path,

    /// Absolute path to the repository root.
    pub repository: &'a Path,

    /// Configured sources to back up.
    pub sources: &'a [SourceConfig],
}

/// Plan a complete backup, producing a deterministic change-set.
///
/// For each configured source:
/// 1. Check if the source root exists (missing → warning, skip deletions).
/// 2. Collect source inventory (walk + ignore filter).
/// 3. Collect destination inventory (existing backup content).
/// 4. Compare entries to find additions and modifications.
/// 5. Plan deletions for missing/newly-ignored files.
/// 6. Detect secret warnings for included files.
///
/// The resulting change-set is sorted for deterministic output.
pub fn plan_backup(inputs: &PlanInputs<'_>) -> Result<ChangeSet, PlanError> {
    let mut changeset = ChangeSet::new();

    for source_config in inputs.sources {
        plan_source(inputs, source_config, &mut changeset)?;
    }

    // Sort for deterministic output.
    changeset.sort();

    Ok(changeset)
}

/// Plan a single source's contribution to the change-set.
fn plan_source(
    inputs: &PlanInputs<'_>,
    source_config: &SourceConfig,
    changeset: &mut ChangeSet,
) -> Result<(), PlanError> {
    let source_root = mapping::source_absolute(inputs.home, &source_config.path);
    let destination_root = mapping::destination_root(inputs.repository, &source_config.path);

    // Check for missing source root — preserve backup, emit warning.
    if let Some(warning) = check_missing_source_root(&source_root, &source_config.path) {
        changeset.warnings.push(warning);
        // Do NOT plan any deletions for a missing source — preserve the backup.
        return Ok(());
    }

    // Build the ignore matcher for this source.
    let (ignore_matcher, pattern_errors) = IgnoreMatcher::new(&source_root, &source_config.ignore);
    // Pattern parse errors are non-fatal — log them as warnings if needed.
    // For now we silently ignore them since the matcher is still functional.
    let _ = pattern_errors;

    // Collect source inventory.
    let source_inventory =
        collect_source_inventory(&source_root, &ignore_matcher).map_err(|source_err| {
            PlanError::SourceInventory {
                source: source_config.path.clone(),
                source_err,
            }
        })?;

    // Transfer exclusions and warnings from source inventory.
    changeset.exclusions.extend(source_inventory.exclusions);
    changeset.warnings.extend(source_inventory.warnings);

    // Collect destination inventory.
    let dest_inventory =
        collect_destination_inventory(&destination_root).map_err(|source_err| {
            PlanError::DestinationInventory {
                source: source_config.path.clone(),
                source_err,
            }
        })?;

    // Build a lookup of destination entries by relative path for comparison.
    let dest_by_relative: HashMap<&PathBuf, &_> = dest_inventory
        .entries
        .iter()
        .map(|e| (&e.relative_path, e))
        .collect();

    changeset.warnings.extend(dest_inventory.warnings);

    // Compare source entries against destination entries.
    // Determine if this is a single-file source. When the source root is a
    // file or symlink (not a directory), destination_root already IS the final
    // file path — we don't join relative paths onto it.
    let is_single_file_source = std::fs::symlink_metadata(&source_root)
        .map(|m| !m.is_dir())
        .unwrap_or(false);

    for source_entry in &source_inventory.entries {
        let dest_path = if is_single_file_source {
            destination_root.clone()
        } else {
            destination_root.join(&source_entry.relative_path)
        };

        if let Some(dest_entry) = dest_by_relative.get(&source_entry.relative_path) {
            // Entry exists in both — check for modifications.
            if let Some(change) = compare_entries(source_entry, dest_entry) {
                changeset
                    .modifications
                    .push(make_modification(source_entry, dest_path, change));
            }
            // else: unchanged, no action needed.
        } else {
            // Entry exists in source but not destination — addition.
            changeset
                .additions
                .push(make_addition(source_entry, dest_path));
        }

        // Secret detection on included files.
        if let Some(reason) = detect_secret(&source_entry.relative_path) {
            changeset
                .warnings
                .push(make_secret_warning(&source_entry.source_path, reason));
        }
    }

    // Plan deletions (destination entries missing from source).
    let (deletions, deletion_warnings) = plan_deletions(
        &source_inventory.entries,
        &dest_inventory.entries,
        &ignore_matcher,
    );
    changeset.deletions.extend(deletions);
    changeset.warnings.extend(deletion_warnings);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Helper to set up a test environment with home and repository.
    struct TestEnv {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        repository: PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("home");
            let repository = tmp.path().join("repo");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(repository.join("home")).unwrap();
            Self {
                _tmp: tmp,
                home,
                repository,
            }
        }

        fn plan(&self, sources: &[SourceConfig]) -> ChangeSet {
            let inputs = PlanInputs {
                home: &self.home,
                repository: &self.repository,
                sources,
            };
            plan_backup(&inputs).unwrap()
        }
    }

    fn source(path: &str, ignore: &[&str]) -> SourceConfig {
        SourceConfig {
            path: path.to_string(),
            ignore: ignore.iter().map(|s| s.to_string()).collect(),
        }
    }

    // --- Basic planning ---

    #[test]
    fn empty_source_directory_produces_empty_changeset() {
        let env = TestEnv::new();
        std::fs::create_dir_all(env.home.join(".config/empty")).unwrap();

        let cs = env.plan(&[source(".config/empty", &[])]);

        assert!(cs.is_empty());
        assert!(cs.exclusions.is_empty());
    }

    #[test]
    fn new_files_appear_as_additions() {
        let env = TestEnv::new();
        let src = env.home.join(".config/fish");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.fish"), "set -x PATH").unwrap();
        std::fs::write(src.join("functions.fish"), "function hello").unwrap();

        let cs = env.plan(&[source(".config/fish", &[])]);

        assert_eq!(cs.additions.len(), 2);
        assert!(cs.modifications.is_empty());
        assert!(cs.deletions.is_empty());
    }

    #[test]
    fn identical_files_produce_no_changes() {
        let env = TestEnv::new();
        let src = env.home.join(".config/fish");
        let dst = env.repository.join("home/.config/fish");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        std::fs::write(src.join("config.fish"), "same content").unwrap();
        std::fs::write(dst.join("config.fish"), "same content").unwrap();

        let cs = env.plan(&[source(".config/fish", &[])]);

        assert!(cs.is_empty());
    }

    #[test]
    fn modified_files_appear_as_modifications() {
        let env = TestEnv::new();
        let src = env.home.join(".config/fish");
        let dst = env.repository.join("home/.config/fish");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        std::fs::write(src.join("config.fish"), "new content").unwrap();
        std::fs::write(dst.join("config.fish"), "old content").unwrap();

        let cs = env.plan(&[source(".config/fish", &[])]);

        assert!(cs.additions.is_empty());
        assert_eq!(cs.modifications.len(), 1);
        assert!(cs.deletions.is_empty());
    }

    #[test]
    fn removed_source_files_appear_as_deletions() {
        let env = TestEnv::new();
        let src = env.home.join(".config/fish");
        let dst = env.repository.join("home/.config/fish");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        // Source has only config.fish, but dest also has old.fish
        std::fs::write(src.join("config.fish"), "content").unwrap();
        std::fs::write(dst.join("config.fish"), "content").unwrap();
        std::fs::write(dst.join("old.fish"), "old content").unwrap();

        let cs = env.plan(&[source(".config/fish", &[])]);

        assert!(cs.additions.is_empty());
        assert!(cs.modifications.is_empty());
        assert_eq!(cs.deletions.len(), 1);
    }

    // --- Ignore rules ---

    #[test]
    fn ignored_files_appear_as_exclusions() {
        let env = TestEnv::new();
        let src = env.home.join(".config/fish");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.fish"), "content").unwrap();
        std::fs::write(src.join("debug.log"), "log data").unwrap();

        let cs = env.plan(&[source(".config/fish", &["*.log"])]);

        assert_eq!(cs.additions.len(), 1);
        assert_eq!(cs.exclusions.len(), 1);
    }

    #[test]
    fn newly_ignored_tracked_files_become_deletions_with_warning() {
        let env = TestEnv::new();
        let src = env.home.join(".config/fish");
        let dst = env.repository.join("home/.config/fish");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        // Source still has the file but it's now ignored
        std::fs::write(src.join("config.fish"), "content").unwrap();
        std::fs::write(src.join("secret.key"), "key data").unwrap();
        std::fs::write(dst.join("config.fish"), "content").unwrap();
        std::fs::write(dst.join("secret.key"), "key data").unwrap();

        let cs = env.plan(&[source(".config/fish", &["*.key"])]);

        // secret.key is now ignored → deleted from dest + warned
        assert_eq!(cs.deletions.len(), 1);
        assert!(cs.warnings.iter().any(|w| matches!(
            &w.kind,
            super::super::changeset::WarningKind::IgnoredButTracked
        )));
    }

    // --- Missing source root ---

    #[test]
    fn missing_source_root_emits_warning_without_deletions() {
        let env = TestEnv::new();
        let dst = env.repository.join("home/.config/gone");
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(dst.join("preserved.txt"), "data").unwrap();

        // Source doesn't exist — backup should be preserved
        let cs = env.plan(&[source(".config/gone", &[])]);

        assert!(cs.additions.is_empty());
        assert!(cs.modifications.is_empty());
        assert!(cs.deletions.is_empty()); // No deletions!
        assert!(cs.warnings.iter().any(|w| matches!(
            &w.kind,
            super::super::changeset::WarningKind::MissingSourceRoot { .. }
        )));
    }

    // --- Secret warnings ---

    #[test]
    fn secret_files_produce_warnings() {
        let env = TestEnv::new();
        let src = env.home.join(".ssh");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("id_rsa"), "private key").unwrap();
        std::fs::write(src.join("id_rsa.pub"), "public key").unwrap();
        std::fs::write(src.join("config"), "Host *").unwrap();

        let cs = env.plan(&[source(".ssh", &[])]);

        // id_rsa should trigger a secret warning
        assert!(cs.warnings.iter().any(|w| matches!(
            &w.kind,
            super::super::changeset::WarningKind::PossibleSecret { .. }
        )));
    }

    // --- Multiple sources ---

    #[test]
    fn multiple_sources_combined_in_changeset() {
        let env = TestEnv::new();
        let fish = env.home.join(".config/fish");
        let waybar = env.home.join(".config/waybar");
        std::fs::create_dir_all(&fish).unwrap();
        std::fs::create_dir_all(&waybar).unwrap();
        std::fs::write(fish.join("config.fish"), "fish").unwrap();
        std::fs::write(waybar.join("config"), "waybar").unwrap();

        let cs = env.plan(&[source(".config/fish", &[]), source(".config/waybar", &[])]);

        assert_eq!(cs.additions.len(), 2);
    }

    // --- Deterministic output ---

    #[test]
    fn output_is_deterministic() {
        let env = TestEnv::new();
        let src = env.home.join(".config/test");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("z.txt"), "z").unwrap();
        std::fs::write(src.join("a.txt"), "a").unwrap();
        std::fs::write(src.join("m.txt"), "m").unwrap();

        let cs1 = env.plan(&[source(".config/test", &[])]);
        let cs2 = env.plan(&[source(".config/test", &[])]);

        // Same inputs → same output.
        assert_eq!(cs1.additions.len(), cs2.additions.len());
        for (a, b) in cs1.additions.iter().zip(cs2.additions.iter()) {
            assert_eq!(a.destination, b.destination);
        }
    }

    #[test]
    fn additions_are_sorted_by_destination() {
        let env = TestEnv::new();
        let src = env.home.join(".config/test");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("z.txt"), "z").unwrap();
        std::fs::write(src.join("a.txt"), "a").unwrap();
        std::fs::write(src.join("m.txt"), "m").unwrap();

        let cs = env.plan(&[source(".config/test", &[])]);

        let dests: Vec<_> = cs.additions.iter().map(|a| &a.destination).collect();
        let mut sorted_dests = dests.clone();
        sorted_dests.sort();
        assert_eq!(dests, sorted_dests);
    }

    // --- Mixed operations ---

    #[test]
    fn full_scenario_with_additions_modifications_deletions() {
        let env = TestEnv::new();
        let src = env.home.join(".config/app");
        let dst = env.repository.join("home/.config/app");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        // Addition: new.txt exists in source only
        std::fs::write(src.join("new.txt"), "new").unwrap();
        // Modification: mod.txt has different content
        std::fs::write(src.join("mod.txt"), "modified").unwrap();
        std::fs::write(dst.join("mod.txt"), "original").unwrap();
        // Unchanged: same.txt is identical
        std::fs::write(src.join("same.txt"), "same").unwrap();
        std::fs::write(dst.join("same.txt"), "same").unwrap();
        // Deletion: old.txt exists only in destination
        std::fs::write(dst.join("old.txt"), "old").unwrap();

        let cs = env.plan(&[source(".config/app", &[])]);

        assert_eq!(cs.additions.len(), 1);
        assert_eq!(cs.modifications.len(), 1);
        assert_eq!(cs.deletions.len(), 1);
    }

    // --- Symlinks ---

    #[test]
    fn symlink_additions_detected() {
        let env = TestEnv::new();
        let src = env.home.join(".config/links");
        std::fs::create_dir_all(&src).unwrap();
        std::os::unix::fs::symlink("/some/target", src.join("my-link")).unwrap();

        let cs = env.plan(&[source(".config/links", &[])]);

        assert_eq!(cs.additions.len(), 1);
        assert_eq!(
            cs.additions[0].entry_type,
            super::super::changeset::EntryType::Symlink
        );
    }

    #[test]
    fn executable_bit_change_detected() {
        let env = TestEnv::new();
        let src = env.home.join(".config/scripts");
        let dst = env.repository.join("home/.config/scripts");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        // Source is executable, dest is not
        std::fs::write(src.join("run.sh"), "#!/bin/bash").unwrap();
        std::fs::set_permissions(src.join("run.sh"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::fs::write(dst.join("run.sh"), "#!/bin/bash").unwrap();
        // Dest is regular (0o644 by default)

        let cs = env.plan(&[source(".config/scripts", &[])]);

        assert_eq!(cs.modifications.len(), 1);
        assert!(matches!(
            &cs.modifications[0].change,
            super::super::changeset::ChangeKind::ExecutableBitChanged {
                now_executable: true
            }
        ));
    }

    // --- Single-file sources ---

    #[test]
    fn single_file_source_planned_correctly() {
        let env = TestEnv::new();
        std::fs::write(env.home.join(".bashrc"), "# bash config").unwrap();

        let cs = env.plan(&[source(".bashrc", &[])]);

        assert_eq!(cs.additions.len(), 1);
        assert!(
            cs.additions[0]
                .destination
                .to_string_lossy()
                .ends_with("home/.bashrc")
        );
    }
}
