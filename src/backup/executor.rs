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
pub fn validate_destination(repository: &Path, destination: &Path) -> ExecutorResult<()> {
    validate_boundary(repository, destination)?;
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
fn ensure_parent_dirs(repository: &Path, destination: &Path) -> ExecutorResult<()> {
    if let Some(parent) = destination.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|source_err| ExecutorError::CreateDir {
                path: parent.to_path_buf(),
                source_err,
            })?;

            // Re-validate after creation to ensure no symlink was injected
            // (TOCTOU mitigation — we re-check after directory creation).
            validate_no_symlinked_parents(repository, destination)?;
        }
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
    source: &Path,
    destination: &Path,
    executable: bool,
) -> ExecutorResult<()> {
    validate_destination(repository, destination)?;
    ensure_parent_dirs(repository, destination)?;

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
pub fn copy_symlink(repository: &Path, source: &Path, destination: &Path) -> ExecutorResult<()> {
    validate_destination(repository, destination)?;
    ensure_parent_dirs(repository, destination)?;

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
pub fn delete_entry(repository: &Path, path: &Path) -> ExecutorResult<()> {
    validate_destination(repository, path)?;

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
    sources: &[crate::config::SourceConfig],
) -> ExecutorResult<()> {
    use super::manifest::Manifest;

    let manifest = Manifest::from_sources(sources);
    let manifest_path = Manifest::path_in(repository);

    // Validate that the manifest path is within the repository.
    validate_boundary(repository, &manifest_path)?;

    // Save atomically (uses tempfile internally).
    manifest.save(repository).map_err(ExecutorError::Manifest)?;

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
    sources: &[crate::config::SourceConfig],
) -> PreflightResult {
    use super::mapping;

    let mut statuses = Vec::with_capacity(sources.len());
    let mut errors = Vec::new();

    for source_config in sources {
        let source_root = mapping::source_absolute(home, &source_config.path);
        let destination_root = mapping::destination_root(repository, &source_config.path);

        // Check if source root exists (symlink_metadata to not follow links).
        match fs::symlink_metadata(&source_root) {
            Ok(_) => {
                // Source exists — validate destination path.
                if let Err(e) = validate_destination(repository, &destination_root) {
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
    let manifest_path = repository.join(crate::app::MANIFEST_FILE_NAME);
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
    sources: &[crate::config::SourceConfig],
    changeset: &super::changeset::ChangeSet,
) -> Result<MirrorResult, ExecutorError> {
    use super::changeset::EntryType;

    // --- Preflight ---
    let preflight = preflight_sources(home, repository, sources);
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
            EntryType::Symlink => copy_symlink(repository, &addition.source, &addition.destination),
            EntryType::RegularFile => {
                copy_file_atomic(repository, &addition.source, &addition.destination, false)
            }
            EntryType::ExecutableFile => {
                copy_file_atomic(repository, &addition.source, &addition.destination, true)
            }
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
            } => copy_symlink(repository, &modification.source, &modification.destination),

            super::changeset::ChangeKind::ExecutableBitChanged { now_executable } => {
                copy_file_atomic(
                    repository,
                    &modification.source,
                    &modification.destination,
                    *now_executable,
                )
            }

            super::changeset::ChangeKind::ContentAndExecutableBitChanged { now_executable } => {
                copy_file_atomic(
                    repository,
                    &modification.source,
                    &modification.destination,
                    *now_executable,
                )
            }

            super::changeset::ChangeKind::ContentChanged => {
                // Determine if executable from source metadata.
                let executable = fs::metadata(&modification.source)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false);
                copy_file_atomic(
                    repository,
                    &modification.source,
                    &modification.destination,
                    executable,
                )
            }

            super::changeset::ChangeKind::TypeChanged { new_type, .. } => match new_type {
                EntryType::RegularFile => copy_file_atomic(
                    repository,
                    &modification.source,
                    &modification.destination,
                    false,
                ),
                EntryType::ExecutableFile => copy_file_atomic(
                    repository,
                    &modification.source,
                    &modification.destination,
                    true,
                ),
                EntryType::Symlink => {
                    copy_symlink(repository, &modification.source, &modification.destination)
                }
            },
        };
        match result {
            Ok(()) => copies_completed += 1,
            Err(e) => errors.push(e),
        }
    }

    // --- Apply deletions ---
    for deletion in &changeset.deletions {
        match delete_entry(repository, &deletion.destination) {
            Ok(()) => deletions_completed += 1,
            Err(e) => errors.push(e),
        }
    }

    // --- Update manifest ---
    let manifest_ok = match update_manifest(repository, sources) {
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
mod tests {
    use super::*;

    // --- normalize_lexical ---

    #[test]
    fn normalize_removes_current_dir() {
        let path = Path::new("/repo/./home/./file");
        assert_eq!(normalize_lexical(path), PathBuf::from("/repo/home/file"));
    }

    #[test]
    fn normalize_resolves_parent_dir() {
        let path = Path::new("/repo/home/../home/file");
        assert_eq!(normalize_lexical(path), PathBuf::from("/repo/home/file"));
    }

    #[test]
    fn normalize_no_change_for_clean_path() {
        let path = Path::new("/repo/home/.config/fish");
        assert_eq!(
            normalize_lexical(path),
            PathBuf::from("/repo/home/.config/fish")
        );
    }

    #[test]
    fn normalize_preserves_leading_dotfiles() {
        let path = Path::new("/repo/home/.bashrc");
        assert_eq!(normalize_lexical(path), PathBuf::from("/repo/home/.bashrc"));
    }

    // --- validate_boundary ---

    #[test]
    fn boundary_accepts_path_inside_repository() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles/home/.bashrc");
        assert!(validate_boundary(repo, dest).is_ok());
    }

    #[test]
    fn boundary_accepts_deeply_nested_path() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles/home/.config/fish/functions/hello.fish");
        assert!(validate_boundary(repo, dest).is_ok());
    }

    #[test]
    fn boundary_rejects_path_outside_repository() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/other/file.txt");
        let result = validate_boundary(repo, dest);
        assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
    }

    #[test]
    fn boundary_rejects_path_that_is_repository_root() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles");
        let result = validate_boundary(repo, dest);
        assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
    }

    #[test]
    fn boundary_rejects_traversal_escape() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles/home/../../etc/passwd");
        let result = validate_boundary(repo, dest);
        assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
    }

    #[test]
    fn boundary_rejects_sibling_with_prefix() {
        // "dotfiles-evil" starts with "dotfiles" as a string but is not beneath it.
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles-evil/file.txt");
        let result = validate_boundary(repo, dest);
        assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
    }

    #[test]
    fn boundary_accepts_manifest_path() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles/.config-sync-manifest.toml");
        assert!(validate_boundary(repo, dest).is_ok());
    }

    // --- validate_no_symlinked_parents ---

    #[test]
    fn symlink_check_passes_when_no_parents_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let dest = repo
            .join("home")
            .join(".config")
            .join("fish")
            .join("config.fish");
        assert!(validate_no_symlinked_parents(&repo, &dest).is_ok());
    }

    #[test]
    fn symlink_check_passes_with_real_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home").join(".config")).unwrap();

        let dest = repo.join("home").join(".config").join("file.txt");
        assert!(validate_no_symlinked_parents(&repo, &dest).is_ok());
    }

    #[test]
    fn symlink_check_rejects_symlinked_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let escape_target = tmp.path().join("escape");
        std::fs::create_dir_all(repo.join("home")).unwrap();
        std::fs::create_dir_all(&escape_target).unwrap();

        // Create a symlink at repo/home/.config -> /tmp/.../escape
        std::os::unix::fs::symlink(&escape_target, repo.join("home").join(".config")).unwrap();

        let dest = repo.join("home").join(".config").join("file.txt");
        let result = validate_no_symlinked_parents(&repo, &dest);
        assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
    }

    #[test]
    fn symlink_check_allows_symlink_as_final_component() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        // The final component (the file itself) can be a symlink — we'll replace it.
        std::os::unix::fs::symlink("/some/target", repo.join("home").join("my-link")).unwrap();

        let dest = repo.join("home").join("my-link");
        assert!(validate_no_symlinked_parents(&repo, &dest).is_ok());
    }

    #[test]
    fn symlink_check_rejects_intermediate_symlink_deep() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let escape_target = tmp.path().join("elsewhere");
        std::fs::create_dir_all(repo.join("home").join(".config")).unwrap();
        std::fs::create_dir_all(&escape_target).unwrap();

        // Create symlink at repo/home/.config/fish -> elsewhere
        std::os::unix::fs::symlink(
            &escape_target,
            repo.join("home").join(".config").join("fish"),
        )
        .unwrap();

        let dest = repo
            .join("home")
            .join(".config")
            .join("fish")
            .join("config.fish");
        let result = validate_no_symlinked_parents(&repo, &dest);
        assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
    }

    // --- validate_destination (combined) ---

    #[test]
    fn validate_destination_passes_for_valid_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let dest = repo.join("home").join(".bashrc");
        assert!(validate_destination(&repo, &dest).is_ok());
    }

    #[test]
    fn validate_destination_fails_for_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let dest = tmp.path().join("outside").join("file.txt");
        let result = validate_destination(&repo, &dest);
        assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
    }

    #[test]
    fn validate_destination_fails_for_symlinked_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let escape_target = tmp.path().join("escape");
        std::fs::create_dir_all(repo.join("home")).unwrap();
        std::fs::create_dir_all(&escape_target).unwrap();

        std::os::unix::fs::symlink(&escape_target, repo.join("home").join("evil")).unwrap();

        let dest = repo.join("home").join("evil").join("file.txt");
        let result = validate_destination(&repo, &dest);
        assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
    }

    // --- copy_file_atomic ---

    #[test]
    fn copy_file_creates_destination_with_content() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("source.txt");
        std::fs::write(&source, "hello world").unwrap();

        let dest = repo.join("home").join("file.txt");
        copy_file_atomic(&repo, &source, &dest, false).unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello world");
    }

    #[test]
    fn copy_file_preserves_executable_bit() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("script.sh");
        std::fs::write(&source, "#!/bin/bash\necho hi").unwrap();

        let dest = repo.join("home").join("script.sh");
        copy_file_atomic(&repo, &source, &dest, true).unwrap();

        let meta = std::fs::metadata(&dest).unwrap();
        assert!(meta.permissions().mode() & 0o111 != 0);
    }

    #[test]
    fn copy_file_sets_non_executable_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("data.txt");
        std::fs::write(&source, "data").unwrap();

        let dest = repo.join("home").join("data.txt");
        copy_file_atomic(&repo, &source, &dest, false).unwrap();

        let meta = std::fs::metadata(&dest).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o644);
    }

    #[test]
    fn copy_file_creates_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let source = tmp.path().join("src.txt");
        std::fs::write(&source, "content").unwrap();

        let dest = repo
            .join("home")
            .join(".config")
            .join("fish")
            .join("config.fish");
        copy_file_atomic(&repo, &source, &dest, false).unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "content");
    }

    #[test]
    fn copy_file_replaces_existing_file_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("new.txt");
        std::fs::write(&source, "new content").unwrap();

        let dest = repo.join("home").join("file.txt");
        std::fs::write(&dest, "old content").unwrap();

        copy_file_atomic(&repo, &source, &dest, false).unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new content");
    }

    #[test]
    fn copy_file_replaces_existing_symlink_with_regular_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("real.txt");
        std::fs::write(&source, "real content").unwrap();

        // Destination is currently a symlink.
        let dest = repo.join("home").join("link");
        std::os::unix::fs::symlink("/some/target", &dest).unwrap();

        copy_file_atomic(&repo, &source, &dest, false).unwrap();

        // After copy, destination is a regular file, not a symlink.
        let meta = std::fs::symlink_metadata(&dest).unwrap();
        assert!(meta.file_type().is_file());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "real content");
    }

    #[test]
    fn copy_file_rejects_boundary_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let source = tmp.path().join("source.txt");
        std::fs::write(&source, "evil").unwrap();

        let dest = tmp.path().join("outside").join("file.txt");
        let result = copy_file_atomic(&repo, &source, &dest, false);
        assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
    }

    #[test]
    fn copy_file_rejects_symlinked_parent_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let escape = tmp.path().join("escape");
        std::fs::create_dir_all(repo.join("home")).unwrap();
        std::fs::create_dir_all(&escape).unwrap();

        // Create symlink escape at repo/home/evil -> escape dir
        std::os::unix::fs::symlink(&escape, repo.join("home").join("evil")).unwrap();

        let source = tmp.path().join("source.txt");
        std::fs::write(&source, "data").unwrap();

        let dest = repo.join("home").join("evil").join("file.txt");
        let result = copy_file_atomic(&repo, &source, &dest, false);
        assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
    }

    #[test]
    fn copy_file_handles_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("empty.txt");
        std::fs::write(&source, "").unwrap();

        let dest = repo.join("home").join("empty.txt");
        copy_file_atomic(&repo, &source, &dest, false).unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "");
    }

    #[test]
    fn copy_file_handles_large_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        // Create a file larger than the 8KB buffer.
        let content = "x".repeat(32 * 1024);
        let source = tmp.path().join("large.txt");
        std::fs::write(&source, &content).unwrap();

        let dest = repo.join("home").join("large.txt");
        copy_file_atomic(&repo, &source, &dest, false).unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), content);
    }

    // --- copy_symlink ---

    #[test]
    fn copy_symlink_preserves_relative_target() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("link");
        std::os::unix::fs::symlink("../other/file", &source).unwrap();

        let dest = repo.join("home").join("link");
        copy_symlink(&repo, &source, &dest).unwrap();

        let target = std::fs::read_link(&dest).unwrap();
        assert_eq!(target, PathBuf::from("../other/file"));
    }

    #[test]
    fn copy_symlink_preserves_absolute_target() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("link");
        std::os::unix::fs::symlink("/usr/bin/bash", &source).unwrap();

        let dest = repo.join("home").join("link");
        copy_symlink(&repo, &source, &dest).unwrap();

        let target = std::fs::read_link(&dest).unwrap();
        assert_eq!(target, PathBuf::from("/usr/bin/bash"));
    }

    #[test]
    fn copy_symlink_preserves_dangling_target() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let source = tmp.path().join("link");
        std::os::unix::fs::symlink("/nonexistent/path/that/does/not/exist", &source).unwrap();

        let dest = repo.join("home").join("link");
        copy_symlink(&repo, &source, &dest).unwrap();

        let target = std::fs::read_link(&dest).unwrap();
        assert_eq!(
            target,
            PathBuf::from("/nonexistent/path/that/does/not/exist")
        );
    }

    #[test]
    fn copy_symlink_replaces_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        // Existing regular file at destination.
        let dest = repo.join("home").join("entry");
        std::fs::write(&dest, "old file content").unwrap();

        let source = tmp.path().join("link");
        std::os::unix::fs::symlink("/target", &source).unwrap();

        copy_symlink(&repo, &source, &dest).unwrap();

        let meta = std::fs::symlink_metadata(&dest).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(std::fs::read_link(&dest).unwrap(), PathBuf::from("/target"));
    }

    #[test]
    fn copy_symlink_replaces_existing_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        // Existing symlink at destination.
        let dest = repo.join("home").join("link");
        std::os::unix::fs::symlink("/old/target", &dest).unwrap();

        let source = tmp.path().join("link");
        std::os::unix::fs::symlink("/new/target", &source).unwrap();

        copy_symlink(&repo, &source, &dest).unwrap();

        assert_eq!(
            std::fs::read_link(&dest).unwrap(),
            PathBuf::from("/new/target")
        );
    }

    #[test]
    fn copy_symlink_creates_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let source = tmp.path().join("link");
        std::os::unix::fs::symlink("/target", &source).unwrap();

        let dest = repo.join("home").join("deep").join("nested").join("link");
        copy_symlink(&repo, &source, &dest).unwrap();

        assert_eq!(std::fs::read_link(&dest).unwrap(), PathBuf::from("/target"));
    }

    #[test]
    fn copy_symlink_rejects_boundary_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let source = tmp.path().join("link");
        std::os::unix::fs::symlink("/target", &source).unwrap();

        let dest = tmp.path().join("outside").join("link");
        let result = copy_symlink(&repo, &source, &dest);
        assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
    }

    #[test]
    fn copy_symlink_rejects_symlinked_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let escape = tmp.path().join("escape");
        std::fs::create_dir_all(repo.join("home")).unwrap();
        std::fs::create_dir_all(&escape).unwrap();

        std::os::unix::fs::symlink(&escape, repo.join("home").join("evil")).unwrap();

        let source = tmp.path().join("link");
        std::os::unix::fs::symlink("/target", &source).unwrap();

        let dest = repo.join("home").join("evil").join("link");
        let result = copy_symlink(&repo, &source, &dest);
        assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
    }

    // --- delete_entry ---

    #[test]
    fn delete_entry_removes_regular_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let file = repo.join("home").join("old.txt");
        std::fs::write(&file, "content").unwrap();

        delete_entry(&repo, &file).unwrap();

        assert!(!file.exists());
    }

    #[test]
    fn delete_entry_removes_symlink_without_following() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        // Create a symlink pointing to a real file outside the repo.
        let outside_file = tmp.path().join("outside.txt");
        std::fs::write(&outside_file, "should not be deleted").unwrap();

        let link = repo.join("home").join("link");
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();

        delete_entry(&repo, &link).unwrap();

        // Symlink is gone.
        assert!(!link.exists());
        // Target file is untouched.
        assert!(outside_file.exists());
        assert_eq!(
            std::fs::read_to_string(&outside_file).unwrap(),
            "should not be deleted"
        );
    }

    #[test]
    fn delete_entry_is_idempotent_for_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let file = repo.join("home").join("nonexistent.txt");
        // Should succeed without error even though file doesn't exist.
        delete_entry(&repo, &file).unwrap();
    }

    #[test]
    fn delete_entry_cleans_up_empty_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let deep = repo.join("home").join(".config").join("fish");
        std::fs::create_dir_all(&deep).unwrap();

        let file = deep.join("config.fish");
        std::fs::write(&file, "content").unwrap();

        delete_entry(&repo, &file).unwrap();

        // File is gone.
        assert!(!file.exists());
        // Empty parents cleaned up.
        assert!(!deep.exists());
        assert!(!repo.join("home").join(".config").exists());
        // But repo/home stays if it's the managed root (not repo itself).
        // Actually, home/ is also empty so it gets cleaned too.
        assert!(!repo.join("home").exists());
        // Repository root itself is never removed.
        assert!(repo.exists());
    }

    #[test]
    fn delete_entry_stops_cleanup_at_non_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let fish_dir = repo.join("home").join(".config").join("fish");
        std::fs::create_dir_all(&fish_dir).unwrap();

        // Two files in the directory.
        std::fs::write(fish_dir.join("config.fish"), "content").unwrap();
        std::fs::write(fish_dir.join("functions.fish"), "other").unwrap();

        // Delete only one.
        delete_entry(&repo, &fish_dir.join("config.fish")).unwrap();

        // Directory still exists because it has another file.
        assert!(fish_dir.exists());
        assert!(fish_dir.join("functions.fish").exists());
    }

    #[test]
    fn delete_entry_rejects_boundary_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let outside = tmp.path().join("outside.txt");
        std::fs::write(&outside, "data").unwrap();

        let result = delete_entry(&repo, &outside);
        assert!(matches!(result, Err(ExecutorError::BoundaryEscape { .. })));
        // File still exists.
        assert!(outside.exists());
    }

    #[test]
    fn delete_entry_rejects_symlinked_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let escape = tmp.path().join("escape");
        std::fs::create_dir_all(repo.join("home")).unwrap();
        std::fs::create_dir_all(&escape).unwrap();
        std::fs::write(escape.join("file.txt"), "data").unwrap();

        // Symlink inside managed namespace pointing outside.
        std::os::unix::fs::symlink(&escape, repo.join("home").join("evil")).unwrap();

        let target_file = repo.join("home").join("evil").join("file.txt");
        let result = delete_entry(&repo, &target_file);
        assert!(matches!(result, Err(ExecutorError::SymlinkedParent { .. })));
        // Original file is untouched.
        assert!(escape.join("file.txt").exists());
    }

    #[test]
    fn delete_entry_removes_dangling_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("home")).unwrap();

        let link = repo.join("home").join("dangling");
        std::os::unix::fs::symlink("/nonexistent/path", &link).unwrap();

        delete_entry(&repo, &link).unwrap();

        // The symlink entry itself should be gone.
        assert!(!link.symlink_metadata().is_ok());
    }

    // --- update_manifest ---

    #[test]
    fn update_manifest_creates_manifest_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string()],
        }];

        update_manifest(&repo, &sources).unwrap();

        let manifest_path = repo.join(crate::app::MANIFEST_FILE_NAME);
        assert!(manifest_path.exists());

        let content = std::fs::read_to_string(&manifest_path).unwrap();
        assert!(content.contains("config-sync-manifest"));
        assert!(content.contains(".config/fish"));
        assert!(content.contains("*.log"));
    }

    #[test]
    fn update_manifest_overwrites_existing_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        // Write initial manifest.
        let sources_v1 = vec![crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        }];
        update_manifest(&repo, &sources_v1).unwrap();

        // Overwrite with new sources.
        let sources_v2 = vec![
            crate::config::SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec!["*.log".to_string()],
            },
            crate::config::SourceConfig {
                path: ".config/waybar".to_string(),
                ignore: vec![],
            },
        ];
        update_manifest(&repo, &sources_v2).unwrap();

        let manifest_path = repo.join(crate::app::MANIFEST_FILE_NAME);
        let content = std::fs::read_to_string(&manifest_path).unwrap();
        assert!(content.contains(".config/fish"));
        assert!(content.contains(".config/waybar"));
        assert!(!content.contains(".bashrc"));
    }

    #[test]
    fn update_manifest_with_empty_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        update_manifest(&repo, &[]).unwrap();

        let manifest_path = repo.join(crate::app::MANIFEST_FILE_NAME);
        assert!(manifest_path.exists());

        // Should be loadable and valid.
        let loaded = super::super::manifest::Manifest::load(&repo).unwrap();
        assert!(loaded.sources.is_empty());
    }

    #[test]
    fn update_manifest_produces_valid_loadable_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let sources = vec![
            crate::config::SourceConfig {
                path: ".ssh/config".to_string(),
                ignore: vec!["id_*".to_string()],
            },
            crate::config::SourceConfig {
                path: ".config/waybar".to_string(),
                ignore: vec!["cache/".to_string(), "*token*".to_string()],
            },
        ];

        update_manifest(&repo, &sources).unwrap();

        let loaded = super::super::manifest::Manifest::load(&repo).unwrap();
        assert_eq!(loaded.sources.len(), 2);
        assert_eq!(loaded.sources[0].path, ".ssh/config");
        assert_eq!(loaded.sources[0].ignore, vec!["id_*"]);
        assert_eq!(loaded.sources[1].path, ".config/waybar");
        assert_eq!(loaded.sources[1].ignore, vec!["cache/", "*token*"]);
    }

    // --- preflight_sources ---

    #[test]
    fn preflight_passes_with_existing_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        let result = preflight_sources(&home, &repo, &sources);
        assert!(result.is_ok());
        assert!(result.source_is_ready(0));
    }

    #[test]
    fn preflight_marks_missing_source_as_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/nonexistent".to_string(),
            ignore: vec![],
        }];

        let result = preflight_sources(&home, &repo, &sources);
        // Missing source is non-fatal.
        assert!(result.is_ok());
        assert!(!result.source_is_ready(0));
    }

    #[test]
    fn preflight_multiple_sources_mixed() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        // .bashrc exists as a file.
        std::fs::write(home.join(".bashrc"), "# bash").unwrap();

        let sources = vec![
            crate::config::SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".config/missing".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
        ];

        let result = preflight_sources(&home, &repo, &sources);
        assert!(result.is_ok());
        assert!(result.source_is_ready(0)); // .config/fish exists
        assert!(!result.source_is_ready(1)); // .config/missing is missing
        assert!(result.source_is_ready(2)); // .bashrc exists
    }

    #[test]
    fn preflight_detects_symlinked_destination_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        let escape = tmp.path().join("escape");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(repo.join("home")).unwrap();
        std::fs::create_dir_all(&escape).unwrap();

        // Create a symlink at repo/home/.config -> escape directory
        std::os::unix::fs::symlink(&escape, repo.join("home").join(".config")).unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        let result = preflight_sources(&home, &repo, &sources);
        // Symlinked parent is a hard error.
        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn preflight_accepts_single_file_source() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(home.join(".bashrc"), "content").unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        }];

        let result = preflight_sources(&home, &repo, &sources);
        assert!(result.is_ok());
        assert!(result.source_is_ready(0));
    }

    #[test]
    fn preflight_with_no_sources_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        let result = preflight_sources(&home, &repo, &[]);
        assert!(result.is_ok());
        assert_eq!(result.statuses.len(), 0);
    }

    // --- execute_mirror ---

    #[test]
    fn execute_mirror_empty_changeset_publishes() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        let sources: Vec<crate::config::SourceConfig> = vec![];
        let changeset = super::super::changeset::ChangeSet::new();

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        assert_eq!(result.copies_completed, 0);
        assert_eq!(result.deletions_completed, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn execute_mirror_applies_additions_and_publishes() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        std::fs::write(home.join(".config/fish/config.fish"), "set PATH").unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset.additions.push(super::super::changeset::Addition {
            source: home.join(".config/fish/config.fish"),
            destination: repo.join("home/.config/fish/config.fish"),
            entry_type: super::super::changeset::EntryType::RegularFile,
        });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        assert_eq!(result.copies_completed, 1);
        assert_eq!(
            std::fs::read_to_string(repo.join("home/.config/fish/config.fish")).unwrap(),
            "set PATH"
        );
        // Manifest was created.
        assert!(repo.join(crate::app::MANIFEST_FILE_NAME).exists());
    }

    #[test]
    fn execute_mirror_applies_deletions() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(repo.join("home/.config/fish")).unwrap();
        std::fs::write(repo.join("home/.config/fish/old.fish"), "old").unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset.deletions.push(super::super::changeset::Deletion {
            destination: repo.join("home/.config/fish/old.fish"),
            reason: super::super::changeset::DeletionReason::SourceRemoved,
        });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        assert_eq!(result.deletions_completed, 1);
        assert!(!repo.join("home/.config/fish/old.fish").exists());
    }

    #[test]
    fn execute_mirror_blocks_publication_on_copy_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        // Source file referenced in changeset does not exist.
        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset.additions.push(super::super::changeset::Addition {
            source: home.join(".config/fish/nonexistent.fish"),
            destination: repo.join("home/.config/fish/nonexistent.fish"),
            entry_type: super::super::changeset::EntryType::RegularFile,
        });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(!result.may_publish);
        assert_eq!(result.copies_completed, 0);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn execute_mirror_fails_on_preflight_hard_error() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        let escape = tmp.path().join("escape");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(repo.join("home")).unwrap();
        std::fs::create_dir_all(&escape).unwrap();

        // Symlink inside repo that would escape.
        std::os::unix::fs::symlink(&escape, repo.join("home").join(".config")).unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        let changeset = super::super::changeset::ChangeSet::new();
        let result = execute_mirror(&home, &repo, &sources, &changeset);

        // Preflight hard error → Err, not Ok with may_publish=false.
        assert!(result.is_err());
    }

    #[test]
    fn execute_mirror_handles_symlink_addition() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/links")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        std::os::unix::fs::symlink("/usr/bin/bash", home.join(".config/links/bash")).unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/links".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset.additions.push(super::super::changeset::Addition {
            source: home.join(".config/links/bash"),
            destination: repo.join("home/.config/links/bash"),
            entry_type: super::super::changeset::EntryType::Symlink,
        });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        assert_eq!(result.copies_completed, 1);
        let target = std::fs::read_link(repo.join("home/.config/links/bash")).unwrap();
        assert_eq!(target, PathBuf::from("/usr/bin/bash"));
    }

    #[test]
    fn execute_mirror_handles_executable_addition() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        std::fs::write(home.join("bin/script.sh"), "#!/bin/bash").unwrap();
        std::fs::set_permissions(
            home.join("bin/script.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: "bin".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset.additions.push(super::super::changeset::Addition {
            source: home.join("bin/script.sh"),
            destination: repo.join("home/bin/script.sh"),
            entry_type: super::super::changeset::EntryType::ExecutableFile,
        });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        let meta = std::fs::metadata(repo.join("home/bin/script.sh")).unwrap();
        assert!(meta.permissions().mode() & 0o111 != 0);
    }

    // --- Interrupted-run recovery ---

    #[test]
    fn recovery_stale_destination_is_overwritten() {
        // Simulates an interrupted run that left an outdated file at the
        // destination. A subsequent mirror corrects it.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(repo.join("home/.config/fish")).unwrap();

        // Source has the correct content.
        std::fs::write(home.join(".config/fish/config.fish"), "correct content").unwrap();
        // Destination has stale content from a previous interrupted copy.
        std::fs::write(
            repo.join("home/.config/fish/config.fish"),
            "stale from interrupted run",
        )
        .unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        // The planner would detect this as a modification.
        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset
            .modifications
            .push(super::super::changeset::Modification {
                source: home.join(".config/fish/config.fish"),
                destination: repo.join("home/.config/fish/config.fish"),
                change: super::super::changeset::ChangeKind::ContentChanged,
            });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        assert_eq!(result.copies_completed, 1);
        assert_eq!(
            std::fs::read_to_string(repo.join("home/.config/fish/config.fish")).unwrap(),
            "correct content"
        );
    }

    #[test]
    fn recovery_pending_deletion_completes() {
        // Simulates a file that should have been deleted in a previous run
        // but the run was interrupted before the deletion happened.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(repo.join("home/.config/fish")).unwrap();

        // File exists in destination but not in source — deletion was pending.
        std::fs::write(repo.join("home/.config/fish/stale.fish"), "should be gone").unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset.deletions.push(super::super::changeset::Deletion {
            destination: repo.join("home/.config/fish/stale.fish"),
            reason: super::super::changeset::DeletionReason::SourceRemoved,
        });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        assert_eq!(result.deletions_completed, 1);
        assert!(!repo.join("home/.config/fish/stale.fish").exists());
    }

    #[test]
    fn recovery_symlink_replaced_with_file() {
        // Simulates a type change: destination has a symlink from a previous
        // run, but source is now a regular file.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(repo.join("home/.config")).unwrap();

        // Source is now a file.
        std::fs::write(home.join(".config/entry"), "file content").unwrap();
        // Destination has a symlink from before.
        std::os::unix::fs::symlink("/old/target", repo.join("home/.config/entry")).unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset
            .modifications
            .push(super::super::changeset::Modification {
                source: home.join(".config/entry"),
                destination: repo.join("home/.config/entry"),
                change: super::super::changeset::ChangeKind::TypeChanged {
                    old_type: super::super::changeset::EntryType::Symlink,
                    new_type: super::super::changeset::EntryType::RegularFile,
                },
            });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        assert_eq!(result.copies_completed, 1);
        let meta = std::fs::symlink_metadata(repo.join("home/.config/entry")).unwrap();
        assert!(meta.file_type().is_file());
        assert_eq!(
            std::fs::read_to_string(repo.join("home/.config/entry")).unwrap(),
            "file content"
        );
    }

    #[test]
    fn recovery_file_replaced_with_symlink() {
        // Simulates a type change: destination has a regular file, source is
        // now a symlink.
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(repo.join("home/.config")).unwrap();

        // Source is now a symlink.
        std::os::unix::fs::symlink("/new/target", home.join(".config/entry")).unwrap();
        // Destination has a regular file from before.
        std::fs::write(repo.join("home/.config/entry"), "old file").unwrap();

        let sources = vec![crate::config::SourceConfig {
            path: ".config".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset
            .modifications
            .push(super::super::changeset::Modification {
                source: home.join(".config/entry"),
                destination: repo.join("home/.config/entry"),
                change: super::super::changeset::ChangeKind::TypeChanged {
                    old_type: super::super::changeset::EntryType::RegularFile,
                    new_type: super::super::changeset::EntryType::Symlink,
                },
            });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        assert!(result.may_publish);
        let meta = std::fs::symlink_metadata(repo.join("home/.config/entry")).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(
            std::fs::read_link(repo.join("home/.config/entry")).unwrap(),
            PathBuf::from("/new/target")
        );
    }

    #[test]
    fn recovery_already_deleted_file_is_idempotent() {
        // Simulates a deletion that already happened (e.g., partial previous
        // run deleted the file but crashed before finishing other operations).
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::create_dir_all(repo.join("home/.config/fish")).unwrap();

        // The file to delete doesn't exist — already cleaned up.
        let sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];

        let mut changeset = super::super::changeset::ChangeSet::new();
        changeset.deletions.push(super::super::changeset::Deletion {
            destination: repo.join("home/.config/fish/already-gone.fish"),
            reason: super::super::changeset::DeletionReason::SourceRemoved,
        });

        let result = execute_mirror(&home, &repo, &sources, &changeset).unwrap();

        // Idempotent: deletion of missing file succeeds.
        assert!(result.may_publish);
        assert_eq!(result.deletions_completed, 1);
    }
}
