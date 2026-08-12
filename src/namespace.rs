//! Safe lifecycle operations for machine namespaces.
//!
//! These operations mutate only a selected namespace directory. They never
//! adopt root-level V1 paths or sibling namespace content, and configuration
//! changes use [`Config::save`] for atomic replacement.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use crate::app;
use crate::backup::manifest::{Manifest, ManifestError};
use crate::backup::mapping;
use crate::config::{Config, ConfigError, validate_namespace};
use crate::git::{
    GitRunner, OwnershipError, OwnershipState, classify_namespace_worktree, classify_ownership,
};

/// Errors from namespace lifecycle operations.
#[derive(Debug, Error)]
pub enum NamespaceError {
    #[error("invalid namespace {namespace:?}: {reason}")]
    InvalidName { namespace: String, reason: String },

    #[error("namespace lifecycle operation requires explicit confirmation")]
    ConfirmationRequired,

    #[error("namespace {namespace:?} already contains unmanaged content")]
    Collision { namespace: String },

    #[error("namespace {namespace:?} is a symbolic link")]
    Symlink { namespace: String },

    #[error("namespace {namespace:?} cannot be used: {reason}")]
    Ownership { namespace: String, reason: String },

    #[error("namespace lifecycle requires a clean worktree: {paths}")]
    DirtyWorktree { paths: String },

    #[error("cannot delete the active namespace without selecting a different replacement")]
    ReplacementRequired,

    #[error("failed to inspect namespace path {path}")]
    Inspect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to modify namespace path {path}")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("manifest operation failed")]
    Manifest(#[from] ManifestError),

    #[error("configuration update failed")]
    Config(#[from] ConfigError),

    #[error("ownership inspection failed")]
    OwnershipInspect(#[from] OwnershipError),
}

/// Create an empty namespace after explicit confirmation.
///
/// Creation only creates `<namespace>/home`; it does not create a manifest or
/// claim any existing content. An existing empty directory (or empty `home/`)
/// is accepted, while every other entry is a collision.
pub fn create(repository: &Path, namespace: &str, confirmed: bool) -> Result<(), NamespaceError> {
    validate_name(namespace)?;
    if !confirmed {
        return Err(NamespaceError::ConfirmationRequired);
    }
    let directory = mapping::namespace_dir(repository, namespace);
    ensure_new_namespace_is_empty(&directory, namespace)?;
    fs::create_dir_all(directory.join(mapping::HOME_DIR_NAME)).map_err(|source| {
        NamespaceError::Filesystem {
            path: directory.join(mapping::HOME_DIR_NAME),
            source,
        }
    })
}

/// Select a usable namespace and atomically persist it in local configuration.
///
/// A new namespace is selectable so the first headless backup can initialize
/// it. Invalid and ambiguous namespaces are never selected.
pub fn select(
    config_path: &Path,
    config: &mut Config,
    repository: &Path,
    namespace: &str,
) -> Result<OwnershipState, NamespaceError> {
    validate_name(namespace)?;
    reject_namespace_symlink(repository, namespace)?;
    let state = classify_ownership(repository, namespace)?;
    require_usable(namespace, &state)?;
    let mut updated = config.clone();
    updated.namespace = namespace.to_string();
    updated.save(config_path)?;
    *config = updated;
    Ok(state)
}

/// Rename the active owned namespace and atomically update local configuration.
///
/// The manifest is rewritten after the directory move. If either manifest or
/// config persistence fails, the directory is moved back and the original
/// manifest remains authoritative whenever rollback succeeds.
pub fn rename(
    config_path: &Path,
    config: &mut Config,
    repository: &Path,
    new_namespace: &str,
    confirmed: bool,
) -> Result<(), NamespaceError> {
    let old_namespace = config.namespace.clone();
    validate_name(&old_namespace)?;
    validate_name(new_namespace)?;
    if !confirmed {
        return Err(NamespaceError::ConfirmationRequired);
    }
    if old_namespace == new_namespace {
        return Ok(());
    }
    reject_namespace_symlink(repository, &old_namespace)?;
    ensure_clean_worktree(repository, &old_namespace)?;
    require_owned(
        &old_namespace,
        &classify_ownership(repository, &old_namespace)?,
    )?;
    let destination = mapping::namespace_dir(repository, new_namespace);
    ensure_new_namespace_is_empty(&destination, new_namespace)?;

    let source = mapping::namespace_dir(repository, &old_namespace);
    let mut manifest = Manifest::load_from_directory(&source)?;
    fs::rename(&source, &destination).map_err(|source_error| NamespaceError::Filesystem {
        path: source.clone(),
        source: source_error,
    })?;

    manifest.namespace = new_namespace.to_string();
    if let Err(error) = manifest.save(&destination) {
        let _ = fs::rename(&destination, &source);
        return Err(NamespaceError::Manifest(error));
    }

    let mut updated = config.clone();
    updated.namespace = new_namespace.to_string();
    if let Err(error) = updated.save(config_path) {
        // Restore the prior manifest before moving the directory back.
        manifest.namespace = old_namespace.clone();
        let _ = manifest.save(&destination);
        let _ = fs::rename(&destination, &source);
        return Err(NamespaceError::Config(error));
    }
    *config = updated;
    Ok(())
}

/// Delete only the owned `home/` directory and manifest of the active namespace.
///
/// The caller must choose another usable namespace first. Configuration is
/// updated before deletion; if deletion fails, it remains pointed at the safe
/// replacement rather than at partially removed content.
pub fn delete(
    config_path: &Path,
    config: &mut Config,
    repository: &Path,
    replacement_namespace: &str,
    confirmed: bool,
) -> Result<(), NamespaceError> {
    let namespace = config.namespace.clone();
    validate_name(&namespace)?;
    validate_name(replacement_namespace)?;
    if !confirmed {
        return Err(NamespaceError::ConfirmationRequired);
    }
    if namespace == replacement_namespace {
        return Err(NamespaceError::ReplacementRequired);
    }
    reject_namespace_symlink(repository, &namespace)?;
    ensure_clean_worktree(repository, &namespace)?;
    require_owned(&namespace, &classify_ownership(repository, &namespace)?)?;
    let replacement = classify_ownership(repository, replacement_namespace)?;
    require_usable(replacement_namespace, &replacement)?;

    let mut updated = config.clone();
    updated.namespace = replacement_namespace.to_string();
    updated.save(config_path)?;
    *config = updated;

    let directory = mapping::namespace_dir(repository, &namespace);
    remove_owned_path(&directory.join(mapping::HOME_DIR_NAME), &namespace)?;
    remove_owned_path(&directory.join(app::MANIFEST_FILE_NAME), &namespace)
}

fn validate_name(namespace: &str) -> Result<(), NamespaceError> {
    validate_namespace(namespace).map_err(|error| NamespaceError::InvalidName {
        namespace: namespace.to_string(),
        reason: error.to_string(),
    })
}

fn require_usable(namespace: &str, state: &OwnershipState) -> Result<(), NamespaceError> {
    match state {
        OwnershipState::New | OwnershipState::Owned { .. } => Ok(()),
        OwnershipState::InvalidManifest { reason } | OwnershipState::Ambiguous { reason } => {
            Err(NamespaceError::Ownership {
                namespace: namespace.to_string(),
                reason: reason.clone(),
            })
        }
    }
}

fn require_owned(namespace: &str, state: &OwnershipState) -> Result<(), NamespaceError> {
    if matches!(state, OwnershipState::Owned { .. }) {
        Ok(())
    } else {
        require_usable(namespace, state)?;
        Err(NamespaceError::Ownership {
            namespace: namespace.to_string(),
            reason: "a valid ownership manifest is required".to_string(),
        })
    }
}

fn reject_namespace_symlink(repository: &Path, namespace: &str) -> Result<(), NamespaceError> {
    let path = mapping::namespace_dir(repository, namespace);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(NamespaceError::Symlink {
            namespace: namespace.to_string(),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(NamespaceError::Inspect { path, source }),
    }
}

fn ensure_new_namespace_is_empty(directory: &Path, namespace: &str) -> Result<(), NamespaceError> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(NamespaceError::Symlink {
                namespace: namespace.to_string(),
            });
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(NamespaceError::Collision {
                namespace: namespace.to_string(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(NamespaceError::Inspect {
                path: directory.to_path_buf(),
                source,
            });
        }
    }

    let mut entries = fs::read_dir(directory).map_err(|source| NamespaceError::Inspect {
        path: directory.to_path_buf(),
        source,
    })?;
    match (entries.next(), entries.next()) {
        (None, _) => Ok(()),
        (Some(Ok(entry)), None) if entry.file_name() == mapping::HOME_DIR_NAME => {
            let home = entry.path();
            let mut home_entries = fs::read_dir(&home)
                .map_err(|source| NamespaceError::Inspect { path: home, source })?;
            if home_entries.next().is_none() {
                Ok(())
            } else {
                Err(NamespaceError::Collision {
                    namespace: namespace.to_string(),
                })
            }
        }
        _ => Err(NamespaceError::Collision {
            namespace: namespace.to_string(),
        }),
    }
}

fn ensure_clean_worktree(repository: &Path, namespace: &str) -> Result<(), NamespaceError> {
    // Before a Git worktree exists this module remains usable for initial
    // setup. Once one exists, lifecycle changes must not carry staged or dirty
    // active/sibling paths into a later publication.
    if !repository.join(".git").exists() {
        return Ok(());
    }
    let runner = GitRunner::new(Duration::from_secs(10));
    let status = classify_namespace_worktree(&runner, repository, namespace).map_err(|error| {
        NamespaceError::Ownership {
            namespace: namespace.to_string(),
            reason: format!("cannot inspect Git worktree: {error}"),
        }
    })?;
    if status.is_clean() {
        return Ok(());
    }
    let mut paths = status.managed_dirty;
    paths.extend(status.unmanaged_dirty);
    paths.sort();
    Err(NamespaceError::DirtyWorktree {
        paths: paths.join(", "),
    })
}

fn remove_owned_path(path: &Path, namespace: &str) -> Result<(), NamespaceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(NamespaceError::Inspect {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(NamespaceError::Symlink {
            namespace: namespace.to_string(),
        });
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|source| NamespaceError::Filesystem {
            path: path.to_path_buf(),
            source,
        })
    } else {
        fs::remove_file(path).map_err(|source| NamespaceError::Filesystem {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/namespace.rs"]
mod tests;
