//! Mirror executor: applies a planned change-set to the filesystem.
//!
//! The executor takes a [`ChangeSet`] produced by the planner and performs the
//! actual filesystem operations: copying files, creating symlinks, and deleting
//! entries. Every operation is guarded by destination boundary checks that ensure
//! writes and deletions remain beneath the repository root and that no parent
//! component in the destination path is a symbolic link.
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
}
