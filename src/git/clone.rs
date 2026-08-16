//! Safe cloning of an existing remote into a new local destination.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use super::{GitCommand, GitError, GitRunner};

/// Errors returned before or during repository cloning.
#[derive(Debug, Error)]
pub enum CloneError {
    #[error("Git URL cannot be empty")]
    EmptyUrl,
    #[error("clone destination must be an absolute path: {0}")]
    RelativeDestination(PathBuf),
    #[error("clone destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("clone destination has no parent directory: {0}")]
    MissingParent(PathBuf),
    #[error("clone destination parent is not a directory: {0}")]
    ParentNotDirectory(PathBuf),
    #[error("clone destination parent does not exist: {0}")]
    ParentMissing(PathBuf),
    #[error("clone destination parent contains a symbolic link: {0}")]
    SymlinkParent(PathBuf),
    #[error("failed to inspect clone path {path}: {source}")]
    Inspect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("repository clone failed: {0}")]
    Git(#[from] GitError),
}

/// Clone `url` into a destination that does not yet exist.
///
/// Git runs with the same bounded, noninteractive and redacting environment as
/// every other network operation. Existing paths are never adopted or
/// overwritten, and symlinked destination parents are refused before Git runs.
pub fn clone_repository(
    url: &str,
    destination: &Path,
    timeout: Duration,
) -> Result<PathBuf, CloneError> {
    if url.trim().is_empty() {
        return Err(CloneError::EmptyUrl);
    }
    if !destination.is_absolute() {
        return Err(CloneError::RelativeDestination(destination.to_path_buf()));
    }
    match std::fs::symlink_metadata(destination) {
        Ok(_) => return Err(CloneError::DestinationExists(destination.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(CloneError::Inspect {
                path: destination.to_path_buf(),
                source,
            });
        }
    }

    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| CloneError::MissingParent(destination.to_path_buf()))?;
    verify_existing_directory_without_symlinks(parent)?;

    let runner = GitRunner::new(timeout);
    let command = GitCommand::new(parent)
        .args(["clone", "--origin", "origin", "--"])
        .arg(url)
        .arg(destination.as_os_str().to_string_lossy().into_owned())
        .network();
    runner.run(&command)?;
    Ok(destination.to_path_buf())
}

fn verify_existing_directory_without_symlinks(path: &Path) -> Result<(), CloneError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => current.push(component.as_os_str()),
            Component::Normal(part) => current.push(part),
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(CloneError::ParentMissing(path.to_path_buf()));
            }
        }
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(CloneError::ParentMissing(path.to_path_buf()));
            }
            Err(source) => {
                return Err(CloneError::Inspect {
                    path: current,
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(CloneError::SymlinkParent(current));
        }
    }

    let metadata = std::fs::symlink_metadata(path).map_err(|source| CloneError::Inspect {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(CloneError::ParentNotDirectory(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/git/clone.rs"]
mod tests;
