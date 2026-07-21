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
}
