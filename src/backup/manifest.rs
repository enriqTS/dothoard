//! Repository manifest definition and serialization.
//!
//! Each manifest (`.dothoard-manifest.toml`) lives in its machine namespace
//! and serves as:
//! - An ownership marker identifying the repository as managed by this application.
//! - A portable description of the backed-up sources and their ignore rules.
//! - A format-versioned schema for forward compatibility.
//!
//! The local configuration remains authoritative for operation. The manifest
//! is not applied without review — it describes what was last backed up.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::app;

/// The format identifier embedded in every manifest to make it recognizable.
pub const FORMAT_IDENTIFIER: &str = "dothoard-manifest";

/// Top-level repository manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// A fixed string that identifies this file as a dothoard manifest.
    pub format: String,

    /// Schema version for forward-compatible evolution.
    pub version: u32,

    /// The portable machine namespace this manifest authorizes.
    pub namespace: String,

    /// The sources that are backed up into this repository, recorded at
    /// the time of the last successful backup.
    #[serde(default)]
    pub sources: Vec<ManifestSource>,
}

/// A source entry in the manifest, recording what was backed up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestSource {
    /// Home-relative path to the source.
    pub path: String,

    /// Ignore patterns that were active for this source.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Errors from manifest I/O operations.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest not found at {path}")]
    NotFound { path: PathBuf },

    #[error("failed to read manifest from {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse manifest from {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid manifest format identifier: expected \"{expected}\", found \"{found}\"")]
    InvalidFormat { expected: String, found: String },

    #[error("unsupported manifest version {found} (supported: {supported})")]
    UnsupportedVersion { found: u32, supported: u32 },

    #[error("manifest declares invalid namespace {namespace:?}: {reason}")]
    InvalidNamespace { namespace: String, reason: String },

    #[error("manifest namespace {declared:?} does not match directory namespace {expected:?}")]
    NamespaceMismatch { declared: String, expected: String },

    #[error("manifest directory {path} does not have a valid UTF-8 namespace name")]
    InvalidNamespaceDirectory { path: PathBuf },

    #[error("failed to serialize manifest")]
    Serialize(#[from] toml::ser::Error),

    #[error("failed to write manifest atomically to {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to persist temporary manifest to {path}")]
    Persist {
        path: PathBuf,
        #[source]
        source: tempfile::PersistError,
    },
}

impl Manifest {
    /// Current manifest schema version.
    pub const CURRENT_VERSION: u32 = 2;

    /// Create a new manifest for `namespace` from the given source configuration.
    pub fn from_sources(
        namespace: impl Into<String>,
        sources: &[crate::config::SourceConfig],
    ) -> Self {
        Self {
            format: FORMAT_IDENTIFIER.to_string(),
            version: Self::CURRENT_VERSION,
            namespace: namespace.into(),
            sources: sources
                .iter()
                .map(|s| ManifestSource {
                    path: s.path.clone(),
                    ignore: s.ignore.clone(),
                })
                .collect(),
        }
    }

    /// Deserialize a manifest from TOML text.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialize the manifest to TOML text.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Validate the format identifier and version of this manifest.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.format != FORMAT_IDENTIFIER {
            return Err(ManifestError::InvalidFormat {
                expected: FORMAT_IDENTIFIER.to_string(),
                found: self.format.clone(),
            });
        }

        if self.version != Self::CURRENT_VERSION {
            return Err(ManifestError::UnsupportedVersion {
                found: self.version,
                supported: Self::CURRENT_VERSION,
            });
        }

        crate::config::validate_namespace(&self.namespace).map_err(|error| {
            ManifestError::InvalidNamespace {
                namespace: self.namespace.clone(),
                reason: error.to_string(),
            }
        })
    }

    /// Validate that this manifest authorizes exactly `namespace`.
    pub fn validate_for_namespace(&self, namespace: &str) -> Result<(), ManifestError> {
        self.validate()?;
        if self.namespace != namespace {
            return Err(ManifestError::NamespaceMismatch {
                declared: self.namespace.clone(),
                expected: namespace.to_string(),
            });
        }
        Ok(())
    }

    /// Load and validate a manifest from its namespace directory.
    pub fn load(namespace_directory: &Path) -> Result<Self, ManifestError> {
        Self::load_from_directory(namespace_directory)
    }

    /// Load and validate a manifest from a directory containing a namespace.
    ///
    /// The caller chooses the directory deliberately; this is used by
    /// ownership classification to inspect only the active namespace.
    pub fn load_from_directory(directory: &Path) -> Result<Self, ManifestError> {
        let path = Self::path_in(directory);

        if !path.exists() {
            return Err(ManifestError::NotFound { path: path.clone() });
        }

        let text = std::fs::read_to_string(&path).map_err(|source| ManifestError::Read {
            path: path.clone(),
            source,
        })?;

        let manifest = Self::from_toml(&text).map_err(|source| ManifestError::Parse {
            path: path.clone(),
            source,
        })?;

        let expected_namespace = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| ManifestError::InvalidNamespaceDirectory {
                path: directory.to_path_buf(),
            })?;
        manifest.validate_for_namespace(expected_namespace)?;

        Ok(manifest)
    }

    /// Save the manifest atomically to its namespace directory.
    pub fn save(&self, namespace_directory: &Path) -> Result<(), ManifestError> {
        let path = namespace_directory.join(app::MANIFEST_FILE_NAME);
        let text = self.to_toml()?;

        let parent = path.parent().unwrap_or(Path::new("."));
        let mut tmp =
            tempfile::NamedTempFile::new_in(parent).map_err(|source| ManifestError::Write {
                path: path.clone(),
                source,
            })?;

        tmp.write_all(text.as_bytes())
            .map_err(|source| ManifestError::Write {
                path: path.clone(),
                source,
            })?;

        tmp.flush().map_err(|source| ManifestError::Write {
            path: path.clone(),
            source,
        })?;

        tmp.persist(&path)
            .map_err(|source| ManifestError::Persist {
                path: path.clone(),
                source,
            })?;

        Ok(())
    }

    /// Return the manifest file path for a namespace directory.
    pub fn path_in(namespace_directory: &Path) -> PathBuf {
        namespace_directory.join(app::MANIFEST_FILE_NAME)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/backup/manifest.rs"]
mod tests;
