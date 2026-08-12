//! Repository ownership classification.
//!
//! Determines the ownership state of a selected machine namespace by
//! inspecting only its `home/` directory and `.dothoard-manifest.toml` file.
//! Root-level V1 paths and sibling namespaces are unmanaged content.
//!
//! The classification distinguishes four states that determine how the
//! application should proceed:
//!
//! - **New**: Neither the managed namespace nor a manifest exists. The
//!   repository can be initialized after user confirmation.
//! - **Owned**: A valid manifest exists. The application previously initialized
//!   this repository and can attach to it after review and confirmation.
//! - **InvalidManifest**: A manifest file exists but is malformed or has an
//!   unsupported version. Refuse to use the repository.
//! - **Ambiguous**: The `home/` directory contains data but no valid manifest
//!   establishes ownership. Refuse to adopt this content silently.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::app;
use crate::backup::manifest::{Manifest, ManifestError};
use crate::backup::mapping;

/// The ownership state of a repository's managed namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipState {
    /// No managed namespace and no manifest. Safe to initialize.
    New,

    /// A valid manifest exists. The application owns this repository.
    Owned {
        /// The loaded and validated manifest.
        manifest: OwnedManifest,
    },

    /// A manifest file exists but is invalid (parse error, wrong format,
    /// or unsupported version).
    InvalidManifest {
        /// Description of why the manifest is invalid.
        reason: String,
    },

    /// The `home/` directory contains content but no valid manifest
    /// establishes ownership. Refusing to adopt.
    Ambiguous {
        /// Description of the ambiguous state.
        reason: String,
    },
}

/// A validated manifest with its metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedManifest {
    /// The sources recorded in the manifest.
    pub sources: Vec<ManifestSourceInfo>,
}

/// Summary of a source entry from the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestSourceInfo {
    /// Home-relative source path.
    pub path: String,
    /// Number of ignore patterns configured.
    pub ignore_count: usize,
}

/// Errors that prevent ownership classification.
#[derive(Debug, Error)]
pub enum OwnershipError {
    /// Failed to check the filesystem state.
    #[error("failed to inspect repository at {path}: {source}")]
    Inspect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Classify the ownership state of one selected namespace in the repository.
///
/// Inspects only `repository/namespace/home/` and
/// `repository/namespace/.dothoard-manifest.toml`. It intentionally ignores
/// root-level V1 paths and every sibling namespace, which remain unmanaged.
pub fn classify_ownership(
    repository: &Path,
    namespace: &str,
) -> Result<OwnershipState, OwnershipError> {
    let namespace_dir = repository.join(namespace);
    let manifest_path = namespace_dir.join(app::MANIFEST_FILE_NAME);
    let home_dir = namespace_dir.join(mapping::HOME_DIR_NAME);

    let manifest_exists = manifest_path.exists();
    let home_exists = home_dir.exists();
    let home_has_content = home_exists && directory_has_content(&home_dir)?;

    match (manifest_exists, home_has_content) {
        // No manifest, no home content → new namespace.
        (false, false) => Ok(OwnershipState::New),

        // Manifest exists → try to load and validate it.
        (true, _) => classify_with_manifest(&namespace_dir),

        // Home has content but no manifest → ambiguous.
        (false, true) => Ok(OwnershipState::Ambiguous {
            reason: format!(
                "directory {} contains files but no manifest ({}) exists",
                home_dir.display(),
                app::MANIFEST_FILE_NAME
            ),
        }),
    }
}

/// Attempt to load and validate the manifest, returning the appropriate state.
fn classify_with_manifest(namespace_dir: &Path) -> Result<OwnershipState, OwnershipError> {
    match Manifest::load_from_directory(namespace_dir) {
        Ok(manifest) => {
            let sources = manifest
                .sources
                .iter()
                .map(|s| ManifestSourceInfo {
                    path: s.path.clone(),
                    ignore_count: s.ignore.len(),
                })
                .collect();

            Ok(OwnershipState::Owned {
                manifest: OwnedManifest { sources },
            })
        }
        Err(ManifestError::NotFound { .. }) => {
            // Race condition: file disappeared between exists check and load.
            Ok(OwnershipState::New)
        }
        Err(ManifestError::Parse { source, .. }) => Ok(OwnershipState::InvalidManifest {
            reason: format!("manifest could not be parsed: {source}"),
        }),
        Err(ManifestError::InvalidFormat { expected, found }) => {
            Ok(OwnershipState::InvalidManifest {
                reason: format!(
                    "manifest has wrong format identifier: expected \"{expected}\", found \"{found}\""
                ),
            })
        }
        Err(ManifestError::UnsupportedVersion { found, supported }) => {
            Ok(OwnershipState::InvalidManifest {
                reason: format!(
                    "manifest version {found} is not supported (supported: {supported})"
                ),
            })
        }
        Err(
            ManifestError::InvalidNamespace { namespace, reason }
            | ManifestError::NamespaceMismatch {
                declared: namespace,
                expected: reason,
            },
        ) => Ok(OwnershipState::InvalidManifest {
            reason: format!("manifest namespace is invalid: {namespace}: {reason}"),
        }),
        Err(ManifestError::InvalidNamespaceDirectory { path }) => {
            Ok(OwnershipState::InvalidManifest {
                reason: format!(
                    "manifest is in an invalid namespace directory: {}",
                    path.display()
                ),
            })
        }
        Err(ManifestError::Read { path, source }) => Err(OwnershipError::Inspect { path, source }),
        // These variants are for write operations and shouldn't occur during load.
        Err(
            ManifestError::Serialize(_)
            | ManifestError::Write { .. }
            | ManifestError::Persist { .. },
        ) => {
            unreachable!("write errors should not occur during manifest load")
        }
    }
}

/// Check if a directory has any entries (files, directories, or symlinks).
fn directory_has_content(dir: &Path) -> Result<bool, OwnershipError> {
    let entries = std::fs::read_dir(dir).map_err(|source| OwnershipError::Inspect {
        path: dir.to_path_buf(),
        source,
    })?;

    Ok(entries.into_iter().next().is_some())
}

impl std::fmt::Display for OwnershipState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::New => write!(f, "new (no managed namespace or manifest)"),
            Self::Owned { manifest } => {
                write!(f, "owned ({} sources in manifest)", manifest.sources.len())
            }
            Self::InvalidManifest { reason } => {
                write!(f, "invalid manifest: {reason}")
            }
            Self::Ambiguous { reason } => {
                write!(f, "ambiguous: {reason}")
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git/ownership.rs"]
mod tests;
