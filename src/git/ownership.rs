//! Repository ownership classification.
//!
//! Determines the ownership state of a repository's managed namespace by
//! inspecting the `home/` directory and `.config-sync-manifest.toml` file.
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

/// Classify the ownership state of the managed namespace in the repository.
///
/// Inspects:
/// - Whether `repository/home/` exists and has any content.
/// - Whether `.config-sync-manifest.toml` exists and is valid.
///
/// Returns one of the four ownership states.
pub fn classify_ownership(repository: &Path) -> Result<OwnershipState, OwnershipError> {
    let manifest_path = repository.join(app::MANIFEST_FILE_NAME);
    let home_dir = mapping::managed_home_dir(repository);

    let manifest_exists = manifest_path.exists();
    let home_exists = home_dir.exists();
    let home_has_content = home_exists && directory_has_content(&home_dir)?;

    match (manifest_exists, home_has_content) {
        // No manifest, no home content → new namespace.
        (false, false) => Ok(OwnershipState::New),

        // Manifest exists → try to load and validate it.
        (true, _) => classify_with_manifest(repository),

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
fn classify_with_manifest(repository: &Path) -> Result<OwnershipState, OwnershipError> {
    match Manifest::load(repository) {
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
mod tests {
    use std::fs;

    use super::*;
    use crate::backup::manifest::FORMAT_IDENTIFIER;
    use crate::config::SourceConfig;

    #[test]
    fn empty_repository_is_new() {
        let tmp = tempfile::tempdir().unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        assert_eq!(state, OwnershipState::New);
    }

    #[test]
    fn empty_home_directory_is_new() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("home")).unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        assert_eq!(state, OwnershipState::New);
    }

    #[test]
    fn valid_manifest_classifies_as_owned() {
        let tmp = tempfile::tempdir().unwrap();

        let manifest = Manifest::from_sources(&[
            SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec!["*.log".to_string()],
            },
            SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
        ]);
        manifest.save(tmp.path()).unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        match state {
            OwnershipState::Owned { manifest } => {
                assert_eq!(manifest.sources.len(), 2);
                assert_eq!(manifest.sources[0].path, ".config/fish");
                assert_eq!(manifest.sources[0].ignore_count, 1);
                assert_eq!(manifest.sources[1].path, ".bashrc");
                assert_eq!(manifest.sources[1].ignore_count, 0);
            }
            other => panic!("expected Owned, got: {other}"),
        }
    }

    #[test]
    fn valid_manifest_with_home_content_classifies_as_owned() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".config/fish")).unwrap();
        fs::write(home.join(".config/fish/config.fish"), "# fish").unwrap();

        let manifest = Manifest::from_sources(&[SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }]);
        manifest.save(tmp.path()).unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        assert!(matches!(state, OwnershipState::Owned { .. }));
    }

    #[test]
    fn home_content_without_manifest_is_ambiguous() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".bashrc"), "# bashrc").unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        match state {
            OwnershipState::Ambiguous { reason } => {
                assert!(reason.contains("no manifest"));
            }
            other => panic!("expected Ambiguous, got: {other}"),
        }
    }

    #[test]
    fn invalid_manifest_format_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join(app::MANIFEST_FILE_NAME);
        fs::write(
            &manifest_path,
            "format = \"wrong-format\"\nversion = 1\nsources = []\n",
        )
        .unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        match state {
            OwnershipState::InvalidManifest { reason } => {
                assert!(reason.contains("wrong format identifier"));
            }
            other => panic!("expected InvalidManifest, got: {other}"),
        }
    }

    #[test]
    fn unsupported_manifest_version_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join(app::MANIFEST_FILE_NAME);
        let content = format!("format = \"{FORMAT_IDENTIFIER}\"\nversion = 99\nsources = []\n");
        fs::write(&manifest_path, content).unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        match state {
            OwnershipState::InvalidManifest { reason } => {
                assert!(reason.contains("not supported"));
            }
            other => panic!("expected InvalidManifest, got: {other}"),
        }
    }

    #[test]
    fn unparseable_manifest_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join(app::MANIFEST_FILE_NAME);
        fs::write(&manifest_path, "this is not valid [[[toml").unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        match state {
            OwnershipState::InvalidManifest { reason } => {
                assert!(reason.contains("could not be parsed"));
            }
            other => panic!("expected InvalidManifest, got: {other}"),
        }
    }

    #[test]
    fn invalid_manifest_with_home_content_is_still_invalid_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("file.txt"), "data").unwrap();

        let manifest_path = tmp.path().join(app::MANIFEST_FILE_NAME);
        fs::write(&manifest_path, "broken toml {{{{").unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        // Invalid manifest takes precedence over ambiguous home content.
        assert!(matches!(state, OwnershipState::InvalidManifest { .. }));
    }

    #[test]
    fn ownership_state_display() {
        let new = OwnershipState::New;
        assert!(new.to_string().contains("new"));

        let owned = OwnershipState::Owned {
            manifest: OwnedManifest {
                sources: vec![ManifestSourceInfo {
                    path: ".bashrc".to_string(),
                    ignore_count: 0,
                }],
            },
        };
        assert!(owned.to_string().contains("1 sources"));

        let invalid = OwnershipState::InvalidManifest {
            reason: "bad version".to_string(),
        };
        assert!(invalid.to_string().contains("bad version"));

        let ambiguous = OwnershipState::Ambiguous {
            reason: "orphaned data".to_string(),
        };
        assert!(ambiguous.to_string().contains("orphaned data"));
    }

    #[test]
    fn nested_home_content_is_ambiguous() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".config/nvim")).unwrap();
        fs::write(home.join(".config/nvim/init.lua"), "-- nvim").unwrap();

        let state = classify_ownership(tmp.path()).unwrap();

        assert!(matches!(state, OwnershipState::Ambiguous { .. }));
    }
}
