//! Mirror executor: applies a planned change-set to the filesystem.
//!
//! The executor takes a [`ChangeSet`] produced by the planner and performs the
//! actual filesystem operations: copying files, creating symlinks, and deleting
//! entries. Every operation is guarded by destination boundary checks that ensure
//! writes and deletions remain beneath the repository root and that no parent
//! component in the destination path is a symbolic link.
//!
//! # Recovery from interrupted runs
//!
//! The mirror is self-healing by design. If a run is interrupted (crash,
//! timeout, signal), the managed namespace may contain:
//! - Partially updated files (the old version remains because atomic rename
//!   never happened).
//! - Stale temporary files with random names (left by `NamedTempFile`).
//! - Files that should have been deleted but weren't yet.
//!
//! On the next run, the planner re-reads source and destination state from
//! scratch, detects all discrepancies, and the executor applies the correct
//! operations to normalize the namespace. No special recovery logic is needed
//! because:
//! - `copy_file_atomic` replaces any existing destination atomically.
//! - `copy_symlink` removes and recreates the destination.
//! - `delete_entry` is idempotent for already-removed paths.
//! - The planner is stateless and compares source truth to destination state.
//!
//! Stale temporary files (`.tmpXXXXXX` pattern) in the destination directory
//! are harmless — they have random names that don't match source paths and
//! are not staged by Git (the Git layer stages only managed paths).
//!
//! Safety invariants enforced by this module:
//! - Every destination write and deletion must remain beneath the repository.
//! - No existing parent component in the managed namespace may be a symlink.
//! - Destination symlinks are never followed.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Errors from the mirror executor.
#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("destination path escapes the repository boundary: {path}")]
    BoundaryEscape { path: PathBuf },

    #[error("destination parent component is a symlink: {symlink} in path {path}")]
    SymlinkedParent { symlink: PathBuf, path: PathBuf },

    #[error("failed to copy file from {source} to {destination}")]
    Copy {
        source: PathBuf,
        destination: PathBuf,
        #[source]
        source_err: std::io::Error,
    },

    #[error("failed to create symlink at {destination}")]
    Symlink {
        destination: PathBuf,
        #[source]
        source_err: std::io::Error,
    },

    #[error("failed to delete {path}")]
    Delete {
        path: PathBuf,
        #[source]
        source_err: std::io::Error,
    },

    #[error("failed to create directory {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source_err: std::io::Error,
    },

    #[error("failed to set permissions on {path}")]
    SetPermissions {
        path: PathBuf,
        #[source]
        source_err: std::io::Error,
    },

    #[error("source preflight failed for \"{source_path}\": {reason}")]
    Preflight { source_path: String, reason: String },

    #[error("manifest update failed")]
    Manifest(#[source] super::manifest::ManifestError),
}

/// Result type for executor operations.
pub type ExecutorResult<T> = Result<T, ExecutorError>;

/// Validate that a destination path is lexically contained within the repository root.
///
/// This performs a purely lexical check: it normalizes both paths by resolving
/// `.` and `..` components without touching the filesystem, then verifies the
/// destination starts with the repository prefix.
///
/// This check must be performed before any filesystem write or deletion.
pub fn validate_boundary(repository: &Path, destination: &Path) -> ExecutorResult<()> {
    let normalized_repo = normalize_lexical(repository);
    let normalized_dest = normalize_lexical(destination);

    if !normalized_dest.starts_with(&normalized_repo) {
        return Err(ExecutorError::BoundaryEscape {
            path: destination.to_path_buf(),
        });
    }

    // The destination must not be the repository root itself.
    if normalized_dest == normalized_repo {
        return Err(ExecutorError::BoundaryEscape {
            path: destination.to_path_buf(),
        });
    }

    Ok(())
}

/// Validate that no parent component of the destination path (between the
/// repository root and the file itself) is a symbolic link.
///
/// This prevents symlink-based escape attacks where a symlinked directory
/// inside the managed namespace could redirect writes outside the repository.
///
/// The repository root itself is not checked — only components beneath it
/// leading to the destination are inspected.
pub fn validate_no_symlinked_parents(repository: &Path, destination: &Path) -> ExecutorResult<()> {
    let normalized_repo = normalize_lexical(repository);
    let normalized_dest = normalize_lexical(destination);

    // Get the relative path from repository to destination.
    let relative = match normalized_dest.strip_prefix(&normalized_repo) {
        Ok(r) => r,
        Err(_) => {
            return Err(ExecutorError::BoundaryEscape {
                path: destination.to_path_buf(),
            });
        }
    };

    // Walk each parent component (excluding the final filename) and check
    // if any existing component is a symlink.
    let mut current = normalized_repo.clone();
    let components: Vec<_> = relative.components().collect();

    // Check all components except the last one (the file itself).
    // The file itself may be a symlink that we are about to replace.
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current = current.join(component.as_os_str());

        // Only check components that actually exist on disk.
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ExecutorError::SymlinkedParent {
                        symlink: current,
                        path: destination.to_path_buf(),
                    });
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Component doesn't exist yet — safe; it will be created
                // as a real directory. Stop checking further components
                // since none of them can exist either.
                break;
            }
            Err(_) => {
                // Other errors (permission denied, etc.) — be conservative
                // and allow the operation to proceed. The actual write will
                // fail with a more specific error if there's a real problem.
                break;
            }
        }
    }

    Ok(())
}

/// Perform both boundary and symlink-parent validation for a destination path.
///
/// This is the standard entry point for validating any destination before
/// performing a write or deletion.
pub fn validate_destination(
    repository: &Path,
    namespace: &str,
    destination: &Path,
) -> ExecutorResult<()> {
    validate_boundary(repository, destination)?;
    if !super::mapping::is_managed_path(repository, namespace, destination) {
        return Err(ExecutorError::BoundaryEscape {
            path: destination.to_path_buf(),
        });
    }
    validate_no_symlinked_parents(repository, destination)?;
    Ok(())
}

/// Normalize a path lexically without touching the filesystem.
///
/// Resolves `.` and `..` components, collapses redundant separators, and
/// produces an absolute-looking path. This is intentionally not using
/// `canonicalize()` to avoid following symlinks.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => {
                result.push(other.as_os_str());
            }
        }
    }

    result
}

/// Ensure all parent directories of a destination path exist.
///
/// Creates directories as needed. Validates that no existing parent component
/// is a symlink before creating missing directories.
fn ensure_parent_dirs(
    repository: &Path,
    namespace: &str,
    destination: &Path,
) -> ExecutorResult<()> {
    if let Some(parent) = destination.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(|source_err| ExecutorError::CreateDir {
            path: parent.to_path_buf(),
            source_err,
        })?;

        // Re-validate after creation to ensure no symlink was injected
        // (TOCTOU mitigation — we re-check after directory creation).
        validate_destination(repository, namespace, destination)?;
    }
    Ok(())
}

/// Copy a regular file atomically to a destination within the repository.
///
/// The file is first written to a temporary file in the same directory as the
/// destination, then permissions are set, and finally the temporary file is
/// atomically renamed to the destination path. This ensures:
/// - Partially written files are never visible at the destination.
/// - The executable bit is set before the file becomes visible.
/// - An existing file at the destination is replaced atomically.
///
/// If a symlink or other non-regular-file exists at the destination, it is
/// removed before the atomic rename.
///
/// # Safety
///
/// Validates destination boundaries before any write. The destination must be
/// beneath the repository root and no parent component may be a symlink.
pub fn copy_file_atomic(
    repository: &Path,
    namespace: &str,
    source: &Path,
    destination: &Path,
    executable: bool,
) -> ExecutorResult<()> {
    validate_destination(repository, namespace, destination)?;
    ensure_parent_dirs(repository, namespace, destination)?;

    let parent = destination.parent().unwrap_or(Path::new("."));

    // Open the source file for reading (do not follow symlinks — caller is
    // responsible for distinguishing files from symlinks).
    let mut src_file = fs::File::open(source).map_err(|source_err| ExecutorError::Copy {
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        source_err,
    })?;

    // Create a temporary file in the same directory for atomic rename.
    let mut tmp =
        tempfile::NamedTempFile::new_in(parent).map_err(|source_err| ExecutorError::Copy {
            source: source.to_path_buf(),
            destination: destination.to_path_buf(),
            source_err,
        })?;

    // Copy content in chunks.
    let mut buf = [0u8; 8192];
    loop {
        let n = src_file
            .read(&mut buf)
            .map_err(|source_err| ExecutorError::Copy {
                source: source.to_path_buf(),
                destination: destination.to_path_buf(),
                source_err,
            })?;
        if n == 0 {
            break;
        }
        tmp.write_all(&buf[..n])
            .map_err(|source_err| ExecutorError::Copy {
                source: source.to_path_buf(),
                destination: destination.to_path_buf(),
                source_err,
            })?;
    }

    tmp.flush().map_err(|source_err| ExecutorError::Copy {
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        source_err,
    })?;

    // Set permissions before persisting so the file is never visible with
    // wrong permissions.
    let mode = if executable { 0o755 } else { 0o644 };
    let perms = fs::Permissions::from_mode(mode);
    tmp.as_file()
        .set_permissions(perms)
        .map_err(|source_err| ExecutorError::SetPermissions {
            path: destination.to_path_buf(),
            source_err,
        })?;

    // If there's an existing symlink at the destination, remove it first.
    // NamedTempFile::persist does a rename which would fail on a symlink target.
    remove_destination_if_different_type(destination)?;

    // Atomically move the temp file to the destination.
    tmp.persist(destination).map_err(|e| ExecutorError::Copy {
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        source_err: e.error,
    })?;

    Ok(())
}

/// Remove an existing destination entry if it's a symlink (since we're about
/// to replace it with a regular file via rename). Regular files don't need
/// removal since rename replaces them atomically.
fn remove_destination_if_different_type(destination: &Path) -> ExecutorResult<()> {
    match fs::symlink_metadata(destination) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                fs::remove_file(destination).map_err(|source_err| ExecutorError::Delete {
                    path: destination.to_path_buf(),
                    source_err,
                })?;
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Ok(()), // If we can't stat, let the persist fail with a better error.
    }
}

/// Copy a symbolic link to a destination within the repository.
///
/// Reads the raw link target from the source path and recreates the same
/// symlink at the destination. The target is preserved exactly as-is — it is
/// never resolved, followed, or validated. This means:
/// - Relative targets remain relative.
/// - Absolute targets remain absolute.
/// - Dangling targets are preserved without error.
///
/// If the destination already exists (as a file or symlink), it is removed
/// before creating the new symlink.
///
/// # Safety
///
/// Validates destination boundaries before any write. The destination must be
/// beneath the repository root and no parent component may be a symlink.
pub fn copy_symlink(
    repository: &Path,
    namespace: &str,
    source: &Path,
    destination: &Path,
) -> ExecutorResult<()> {
    validate_destination(repository, namespace, destination)?;
    ensure_parent_dirs(repository, namespace, destination)?;

    // Read the raw link target without following it.
    let target = fs::read_link(source).map_err(|source_err| ExecutorError::Symlink {
        destination: destination.to_path_buf(),
        source_err,
    })?;

    // Remove any existing entry at the destination (file or symlink).
    remove_destination_entry(destination)?;

    // Create the symlink with the same target.
    std::os::unix::fs::symlink(&target, destination).map_err(|source_err| {
        ExecutorError::Symlink {
            destination: destination.to_path_buf(),
            source_err,
        }
    })?;

    Ok(())
}

/// Remove any existing filesystem entry at a path (file, symlink, or empty directory).
///
/// Used before creating a symlink at a destination that might already have content.
/// Does not follow symlinks — uses `remove_file` which operates on the link itself.
fn remove_destination_entry(destination: &Path) -> ExecutorResult<()> {
    match fs::symlink_metadata(destination) {
        Ok(meta) => {
            if meta.is_dir() {
                // Only remove if empty — a non-empty directory indicates a
                // type change from directory source to symlink, which requires
                // the directory contents to be cleaned up first.
                fs::remove_dir(destination).map_err(|source_err| ExecutorError::Delete {
                    path: destination.to_path_buf(),
                    source_err,
                })?;
            } else {
                // File or symlink — remove directly.
                fs::remove_file(destination).map_err(|source_err| ExecutorError::Delete {
                    path: destination.to_path_buf(),
                    source_err,
                })?;
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source_err) => Err(ExecutorError::Delete {
            path: destination.to_path_buf(),
            source_err,
        }),
    }
}

/// Safely delete a file or symlink from the managed namespace.
///
/// Performs boundary validation to ensure the path is within the repository
/// and that no parent component is a symlink. Uses `remove_file` which
/// operates on the link entry itself without following symlinks.
///
/// After removing the file, cleans up any empty parent directories up to
/// (but not including) the repository root. This keeps the managed namespace
/// tidy when entire source directories are removed.
///
/// # Safety
///
/// - Never follows symlinks during deletion.
/// - Never deletes outside the repository boundary.
/// - Never removes the repository root itself.
pub fn delete_entry(repository: &Path, namespace: &str, path: &Path) -> ExecutorResult<()> {
    validate_destination(repository, namespace, path)?;

    // Check what exists at the path (without following symlinks).
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.is_dir() {
                // Directories in the managed namespace should only be removed
                // when empty (their contents should be deleted individually first).
                fs::remove_dir(path).map_err(|source_err| ExecutorError::Delete {
                    path: path.to_path_buf(),
                    source_err,
                })?;
            } else {
                // Regular file or symlink — remove directly.
                fs::remove_file(path).map_err(|source_err| ExecutorError::Delete {
                    path: path.to_path_buf(),
                    source_err,
                })?;
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Already gone — idempotent success.
            return Ok(());
        }
        Err(source_err) => {
            return Err(ExecutorError::Delete {
                path: path.to_path_buf(),
                source_err,
            });
        }
    }

    // Clean up empty parent directories toward the repository root.
    cleanup_empty_parents(repository, path);

    Ok(())
}

/// Remove empty parent directories between a deleted file and the repository root.
///
/// Walks up from the deleted file's parent, removing each directory if it is
/// empty. Stops at the repository root or at the first non-empty directory.
/// Errors are silently ignored — cleanup is best-effort and non-critical.
fn cleanup_empty_parents(repository: &Path, deleted_path: &Path) {
    let mut current = deleted_path.parent();

    while let Some(dir) = current {
        // Stop at or above the repository root.
        if dir == repository || !dir.starts_with(repository) {
            break;
        }

        // Try to remove — only succeeds if empty.
        if fs::remove_dir(dir).is_err() {
            break;
        }

        current = dir.parent();
    }
}

/// Generate and atomically update the repository manifest from configuration.
///
/// The manifest records the current source configuration as an ownership marker
/// and portable description of what was backed up. It is written atomically
/// to avoid partially written manifests.
///
/// # Safety
///
/// Validates that the manifest path is within the repository boundary before
/// writing.
pub fn update_manifest(
    repository: &Path,
    namespace: &str,
    sources: &[crate::config::SourceConfig],
) -> ExecutorResult<()> {
    use super::manifest::Manifest;

    let manifest = Manifest::from_sources(namespace, sources);
    let namespace_root = super::mapping::namespace_dir(repository, namespace);
    fs::create_dir_all(&namespace_root).map_err(|source_err| ExecutorError::CreateDir {
        path: namespace_root.clone(),
        source_err,
    })?;
    let manifest_path = Manifest::path_in(&namespace_root);

    // Validate that the manifest path is within the repository.
    validate_boundary(repository, &manifest_path)?;
    validate_no_symlinked_parents(repository, &manifest_path)?;

    // Save atomically (uses tempfile internally).
    manifest
        .save(&namespace_root)
        .map_err(ExecutorError::Manifest)?;

    Ok(())
}

/// Result of preflighting a single source.
#[derive(Debug)]
pub enum PreflightStatus {
    /// Source root exists and its destination path is valid.
    Ready,

    /// Source root is missing. The backup for this source is preserved (not
    /// deleted) and a warning is emitted, but mirroring can still proceed
    /// for other sources.
    Missing,
}

/// Result of preflighting all sources.
#[derive(Debug)]
pub struct PreflightResult {
    /// Per-source statuses in the same order as the input sources.
    pub statuses: Vec<PreflightStatus>,

    /// Hard errors that prevent the mirror from proceeding at all.
    /// If this is non-empty, no mutation should occur.
    pub errors: Vec<ExecutorError>,
}

impl PreflightResult {
    /// Returns `true` if the preflight passed — no hard errors prevent mirroring.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns `true` if a specific source (by index) is ready for mirroring.
    pub fn source_is_ready(&self, index: usize) -> bool {
        matches!(self.statuses.get(index), Some(PreflightStatus::Ready))
    }
}

/// Validate all source roots and their destination paths before any mutation.
///
/// Preflight checks performed for each source:
/// 1. Whether the source root exists (missing is non-fatal — backup preserved).
/// 2. Whether the destination root path is within the repository boundary.
/// 3. Whether existing destination parent components contain symlinks.
///
/// A missing source root is recorded as [`PreflightStatus::Missing`] and does
/// not block mirroring of other sources. A boundary or symlink violation is a
/// hard error that prevents all mirroring for that run.
///
/// The preflight also validates the manifest destination.
pub fn preflight_sources(
    home: &Path,
    repository: &Path,
    namespace: &str,
    sources: &[crate::config::SourceConfig],
) -> PreflightResult {
    use super::mapping;

    let mut statuses = Vec::with_capacity(sources.len());
    let mut errors = Vec::new();

    for source_config in sources {
        let source_root = mapping::source_absolute(home, &source_config.path);
        let destination_root =
            mapping::destination_root(repository, namespace, &source_config.path);

        // Check if source root exists (symlink_metadata to not follow links).
        match fs::symlink_metadata(&source_root) {
            Ok(_) => {
                // Source exists — validate destination path.
                if let Err(e) = validate_destination(repository, namespace, &destination_root) {
                    errors.push(e);
                    statuses.push(PreflightStatus::Missing); // Mark as not ready.
                } else {
                    statuses.push(PreflightStatus::Ready);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Missing source root — non-fatal, backup is preserved.
                statuses.push(PreflightStatus::Missing);
            }
            Err(_) => {
                // Permission denied or other OS error accessing source root.
                errors.push(ExecutorError::Preflight {
                    source_path: source_config.path.clone(),
                    reason: format!("cannot access source root: {}", source_root.display()),
                });
                statuses.push(PreflightStatus::Missing);
            }
        }
    }

    // Also validate the manifest path is within bounds.
    let manifest_path =
        super::mapping::namespace_dir(repository, namespace).join(crate::app::MANIFEST_FILE_NAME);
    if let Err(e) = validate_boundary(repository, &manifest_path) {
        errors.push(e);
    }

    PreflightResult { statuses, errors }
}

/// The outcome of a complete mirror execution.
///
/// This result signals whether Git publication (staging, committing, pulling,
/// pushing) may proceed. If `may_publish` is false, the caller must not
/// perform any Git operations for this run.
#[derive(Debug)]
pub struct MirrorResult {
    /// Whether all mirror and manifest operations succeeded.
    /// When false, Git publication must be blocked for this run.
    pub may_publish: bool,

    /// Number of files/symlinks successfully copied or updated.
    pub copies_completed: usize,

    /// Number of files/symlinks successfully deleted.
    pub deletions_completed: usize,

    /// Errors encountered during mirroring. These do not include preflight
    /// errors (which prevent execution entirely).
    pub errors: Vec<ExecutorError>,
}

/// Execute the complete mirror operation from a planned change-set.
///
/// This is the top-level orchestrator that:
/// 1. Runs preflight validation on all sources and destinations.
/// 2. Applies additions and modifications from the change-set.
/// 3. Applies deletions from the change-set.
/// 4. Updates the repository manifest.
///
/// # Publication boundary
///
/// If any mirror operation or manifest update fails, the returned
/// [`MirrorResult`] has `may_publish = false`, which signals to the caller
/// that no Git staging, committing, pulling, or pushing should occur for
/// this run. Changes already written to the worktree remain and will be
/// corrected by a later run.
///
/// A preflight failure (hard error) prevents execution entirely and returns
/// an error immediately.
pub fn execute_mirror(
    home: &Path,
    repository: &Path,
    namespace: &str,
    sources: &[crate::config::SourceConfig],
    changeset: &super::changeset::ChangeSet,
) -> Result<MirrorResult, ExecutorError> {
    use super::changeset::EntryType;

    // --- Preflight ---
    let preflight = preflight_sources(home, repository, namespace, sources);
    if !preflight.is_ok() {
        // Return the first hard error. Preflight failures prevent all mutation.
        return Err(preflight.errors.into_iter().next().unwrap());
    }

    let mut copies_completed: usize = 0;
    let mut deletions_completed: usize = 0;
    let mut errors: Vec<ExecutorError> = Vec::new();

    // --- Apply additions ---
    for addition in &changeset.additions {
        let result = match addition.entry_type {
            EntryType::Symlink => copy_symlink(
                repository,
                namespace,
                &addition.source,
                &addition.destination,
            ),
            EntryType::RegularFile => copy_file_atomic(
                repository,
                namespace,
                &addition.source,
                &addition.destination,
                false,
            ),
            EntryType::ExecutableFile => copy_file_atomic(
                repository,
                namespace,
                &addition.source,
                &addition.destination,
                true,
            ),
        };
        match result {
            Ok(()) => copies_completed += 1,
            Err(e) => errors.push(e),
        }
    }

    // --- Apply modifications ---
    for modification in &changeset.modifications {
        let result = match &modification.change {
            super::changeset::ChangeKind::SymlinkTargetChanged { .. }
            | super::changeset::ChangeKind::TypeChanged {
                new_type: EntryType::Symlink,
                ..
            } => copy_symlink(
                repository,
                namespace,
                &modification.source,
                &modification.destination,
            ),

            super::changeset::ChangeKind::ExecutableBitChanged { now_executable }
            | super::changeset::ChangeKind::ContentAndExecutableBitChanged { now_executable } => {
                copy_file_atomic(
                    repository,
                    namespace,
                    &modification.source,
                    &modification.destination,
                    *now_executable,
                )
            }

            super::changeset::ChangeKind::ContentChanged => {
                let executable = fs::metadata(&modification.source)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false);
                copy_file_atomic(
                    repository,
                    namespace,
                    &modification.source,
                    &modification.destination,
                    executable,
                )
            }

            super::changeset::ChangeKind::TypeChanged { new_type, .. } => match new_type {
                EntryType::RegularFile => copy_file_atomic(
                    repository,
                    namespace,
                    &modification.source,
                    &modification.destination,
                    false,
                ),
                EntryType::ExecutableFile => copy_file_atomic(
                    repository,
                    namespace,
                    &modification.source,
                    &modification.destination,
                    true,
                ),
                EntryType::Symlink => copy_symlink(
                    repository,
                    namespace,
                    &modification.source,
                    &modification.destination,
                ),
            },
        };
        match result {
            Ok(()) => copies_completed += 1,
            Err(e) => errors.push(e),
        }
    }

    // --- Apply deletions ---
    for deletion in &changeset.deletions {
        match delete_entry(repository, namespace, &deletion.destination) {
            Ok(()) => deletions_completed += 1,
            Err(e) => errors.push(e),
        }
    }

    // --- Update manifest ---
    let manifest_ok = match update_manifest(repository, namespace, sources) {
        Ok(()) => true,
        Err(e) => {
            errors.push(e);
            false
        }
    };

    // Publication is allowed only if there were zero errors.
    let may_publish = errors.is_empty() && manifest_ok;

    Ok(MirrorResult {
        may_publish,
        copies_completed,
        deletions_completed,
        errors,
    })
}

#[cfg(test)]
#[path = "../../tests/unit/backup/executor.rs"]
mod tests;
