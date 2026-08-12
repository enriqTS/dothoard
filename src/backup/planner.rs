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

    /// Selected machine namespace inside the repository.
    pub namespace: &'a str,

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
    let destination_root =
        mapping::destination_root(inputs.repository, inputs.namespace, &source_config.path);

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
#[path = "../../tests/unit/backup/planner.rs"]
mod tests;
