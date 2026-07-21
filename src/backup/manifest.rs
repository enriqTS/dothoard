//! Repository manifest definition and serialization.
//!
//! The manifest (`.dothoard-manifest.toml`) lives in the repository root
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
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new manifest from the given source configuration.
    pub fn from_sources(sources: &[crate::config::SourceConfig]) -> Self {
        Self {
            format: FORMAT_IDENTIFIER.to_string(),
            version: Self::CURRENT_VERSION,
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

        Ok(())
    }

    /// Load and validate a manifest from the given repository root.
    pub fn load(repository: &Path) -> Result<Self, ManifestError> {
        let path = repository.join(app::MANIFEST_FILE_NAME);

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

        manifest.validate()?;

        Ok(manifest)
    }

    /// Save the manifest atomically to the given repository root.
    pub fn save(&self, repository: &Path) -> Result<(), ManifestError> {
        let path = repository.join(app::MANIFEST_FILE_NAME);
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

    /// Return the manifest file path for a given repository root.
    pub fn path_in(repository: &Path) -> PathBuf {
        repository.join(app::MANIFEST_FILE_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SourceConfig;

    #[test]
    fn creates_manifest_from_sources() {
        let sources = vec![
            SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec!["*.log".to_string(), "fish_variables".to_string()],
            },
            SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
        ];

        let manifest = Manifest::from_sources(&sources);

        assert_eq!(manifest.format, FORMAT_IDENTIFIER);
        assert_eq!(manifest.version, Manifest::CURRENT_VERSION);
        assert_eq!(manifest.sources.len(), 2);
        assert_eq!(manifest.sources[0].path, ".config/fish");
        assert_eq!(manifest.sources[0].ignore, vec!["*.log", "fish_variables"]);
        assert_eq!(manifest.sources[1].path, ".bashrc");
        assert!(manifest.sources[1].ignore.is_empty());
    }

    #[test]
    fn round_trips_through_toml() {
        let manifest = Manifest {
            format: FORMAT_IDENTIFIER.to_string(),
            version: 1,
            sources: vec![
                ManifestSource {
                    path: ".config/waybar".to_string(),
                    ignore: vec!["cache/".to_string(), "*token*".to_string()],
                },
                ManifestSource {
                    path: ".ssh/config".to_string(),
                    ignore: vec![],
                },
            ],
        };

        let text = manifest.to_toml().unwrap();
        let restored = Manifest::from_toml(&text).unwrap();

        assert_eq!(manifest, restored);
    }

    #[test]
    fn validates_correct_manifest() {
        let manifest = Manifest::from_sources(&[]);

        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn rejects_wrong_format_identifier() {
        let manifest = Manifest {
            format: "something-else".to_string(),
            version: 1,
            sources: vec![],
        };

        let result = manifest.validate();
        assert!(matches!(result, Err(ManifestError::InvalidFormat { .. })));
    }

    #[test]
    fn rejects_unsupported_version() {
        let manifest = Manifest {
            format: FORMAT_IDENTIFIER.to_string(),
            version: 99,
            sources: vec![],
        };

        let result = manifest.validate();
        assert!(matches!(
            result,
            Err(ManifestError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();

        let manifest = Manifest::from_sources(&[SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string()],
        }]);

        manifest.save(repo).unwrap();

        let loaded = Manifest::load(repo).unwrap();
        assert_eq!(loaded, manifest);
    }

    #[test]
    fn load_returns_not_found_for_missing_manifest() {
        let tmp = tempfile::tempdir().unwrap();

        let result = Manifest::load(tmp.path());

        assert!(matches!(result, Err(ManifestError::NotFound { .. })));
    }

    #[test]
    fn load_rejects_invalid_format_in_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(app::MANIFEST_FILE_NAME);
        std::fs::write(&path, "format = \"wrong\"\nversion = 1\nsources = []\n").unwrap();

        let result = Manifest::load(tmp.path());

        assert!(matches!(result, Err(ManifestError::InvalidFormat { .. })));
    }

    #[test]
    fn load_rejects_unsupported_version_in_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(app::MANIFEST_FILE_NAME);
        let content = format!("format = \"{FORMAT_IDENTIFIER}\"\nversion = 99\nsources = []\n");
        std::fs::write(&path, content).unwrap();

        let result = Manifest::load(tmp.path());

        assert!(matches!(
            result,
            Err(ManifestError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn manifest_file_name_matches_app_constant() {
        let repo = Path::new("/home/user/dotfiles");

        assert_eq!(Manifest::path_in(repo), repo.join(app::MANIFEST_FILE_NAME));
    }

    #[test]
    fn serialized_manifest_contains_format_identifier() {
        let manifest = Manifest::from_sources(&[]);
        let text = manifest.to_toml().unwrap();

        assert!(text.contains(FORMAT_IDENTIFIER));
    }
}
