//! Configuration models and persistence.
//!
//! The configuration file lives at `~/.config/dothoard/config.toml` and
//! describes the repository location, remote, schedule, and source mappings.
//! This module defines the schema and serialization; validation logic lives
//! in dedicated functions that operate on the deserialized model.
//!
//! Writes use atomic replacement (write to a temporary file in the same
//! directory, then rename) so an interrupted save never leaves a partially
//! written configuration.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during configuration I/O.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration file not found: {path}")]
    NotFound { path: PathBuf },

    #[error("failed to read configuration from {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse configuration from {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to serialize configuration")]
    Serialize(#[from] toml::ser::Error),

    #[error("failed to create parent directory {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write configuration atomically to {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to persist temporary file to {path}")]
    Persist {
        path: PathBuf,
        #[source]
        source: tempfile::PersistError,
    },
}

/// A single validation problem found in a configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// The schema version is not supported.
    UnsupportedVersion { found: u32, supported: u32 },
    /// The repository path is empty.
    EmptyRepository,
    /// The remote name is empty.
    EmptyRemote,
    /// The configured machine namespace is empty.
    EmptyNamespace,
    /// The configured machine namespace is an absolute path.
    AbsoluteNamespace { namespace: String },
    /// The configured machine namespace contains a path separator.
    NamespaceContainsSeparator { namespace: String },
    /// The configured machine namespace is reserved as a path component.
    ReservedNamespace { namespace: String },
    /// The configured machine namespace contains a non-portable character.
    InvalidNamespaceCharacter { namespace: String },
    /// The backup interval is zero.
    ZeroInterval,
    /// The network timeout is zero.
    ZeroTimeout,
    /// A source path is empty.
    EmptySourcePath { index: usize },
    /// A source path is absolute (must be home-relative).
    AbsoluteSourcePath { index: usize, path: String },
    /// A source path contains parent traversal (`..`).
    ParentTraversal { index: usize, path: String },
    /// Duplicate source paths detected.
    DuplicateSource { index: usize, path: String },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { found, supported } => {
                write!(
                    f,
                    "unsupported configuration version {found} (supported: {supported})"
                )
            }
            Self::EmptyRepository => write!(f, "repository path is empty"),
            Self::EmptyRemote => write!(f, "remote name is empty"),
            Self::EmptyNamespace => write!(f, "machine namespace is empty"),
            Self::AbsoluteNamespace { namespace } => {
                write!(f, "machine namespace must not be absolute: \"{namespace}\"")
            }
            Self::NamespaceContainsSeparator { namespace } => {
                write!(
                    f,
                    "machine namespace must be one path component: \"{namespace}\""
                )
            }
            Self::ReservedNamespace { namespace } => {
                write!(f, "machine namespace is reserved: \"{namespace}\"")
            }
            Self::InvalidNamespaceCharacter { namespace } => write!(
                f,
                "machine namespace contains non-portable characters: \"{namespace}\""
            ),
            Self::ZeroInterval => write!(f, "interval_minutes must be at least 1"),
            Self::ZeroTimeout => write!(f, "network_timeout_seconds must be at least 1"),
            Self::EmptySourcePath { index } => {
                write!(f, "source [{index}]: path is empty")
            }
            Self::AbsoluteSourcePath { index, path } => {
                write!(f, "source [{index}]: path must be relative, got \"{path}\"")
            }
            Self::ParentTraversal { index, path } => {
                write!(
                    f,
                    "source [{index}]: path contains parent traversal (..): \"{path}\""
                )
            }
            Self::DuplicateSource { index, path } => {
                write!(f, "source [{index}]: duplicate path \"{path}\"")
            }
        }
    }
}

/// Scheduler backend selected for managed backup automation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationBackend {
    /// A `systemd --user` service and timer.
    #[default]
    Systemd,
    /// A managed block in the user's crontab.
    Cron,
}

/// Top-level application configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Schema version for forward-compatible migrations.
    pub version: u32,

    /// Path to the dedicated Git repository clone.
    /// Stored as-is from the file; tilde expansion and validation happen
    /// at use time, not at deserialization.
    pub repository: String,

    /// Git remote name used for push and pull. Defaults to `"origin"`.
    #[serde(default = "default_remote")]
    pub remote: String,

    /// User-selected directory that exclusively contains this machine's backup.
    ///
    /// It is validated as a portable, single path component before use. The
    /// default only permits old configuration files to deserialize far enough
    /// to report actionable validation errors; it is never operational.
    #[serde(default)]
    pub namespace: String,

    /// Backup automation interval in minutes. Defaults to 5.
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u32,

    /// Scheduler used by managed backup automation. Defaults to systemd.
    #[serde(default)]
    pub automation_backend: AutomationBackend,

    /// Network timeout in seconds for Git transport commands. Defaults to 120.
    #[serde(default = "default_network_timeout_seconds")]
    pub network_timeout_seconds: u32,

    /// Configured source directories to back up.
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

/// A single source directory beneath `$HOME` to be backed up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Home-relative path to the source. Must not be absolute, must not
    /// contain parent traversal, and must not be empty.
    pub path: String,

    /// Per-source ignore patterns using `.gitignore` semantics.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Default remote name.
fn default_remote() -> String {
    "origin".to_string()
}

/// Default backup interval in minutes.
fn default_interval_minutes() -> u32 {
    5
}

/// Default network timeout in seconds.
fn default_network_timeout_seconds() -> u32 {
    120
}

impl Config {
    /// The current schema version that new configurations are created with.
    pub const CURRENT_VERSION: u32 = 2;

    /// Deserialize a configuration from TOML text.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialize the configuration to TOML text.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Create a minimal configuration for the given repository and namespace.
    pub fn new(repository: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            repository: repository.into(),
            remote: default_remote(),
            namespace: namespace.into(),
            interval_minutes: default_interval_minutes(),
            automation_backend: AutomationBackend::default(),
            network_timeout_seconds: default_network_timeout_seconds(),
            sources: Vec::new(),
        }
    }

    /// Expand the repository path, resolving a leading `~` to the given home.
    pub fn repository_path(&self, home: &std::path::Path) -> PathBuf {
        if let Some(rest) = self.repository.strip_prefix("~/") {
            home.join(rest)
        } else if self.repository == "~" {
            home.to_path_buf()
        } else {
            PathBuf::from(&self.repository)
        }
    }

    /// Validate the configuration, collecting all problems found.
    ///
    /// Returns an empty vector when the configuration is valid.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        // Version check.
        if self.version != Self::CURRENT_VERSION {
            errors.push(ValidationError::UnsupportedVersion {
                found: self.version,
                supported: Self::CURRENT_VERSION,
            });
        }

        // Repository must not be empty.
        if self.repository.trim().is_empty() {
            errors.push(ValidationError::EmptyRepository);
        }

        // Remote must not be empty.
        if self.remote.trim().is_empty() {
            errors.push(ValidationError::EmptyRemote);
        }

        if let Err(error) = validate_namespace(&self.namespace) {
            errors.push(error);
        }

        // Interval must be positive.
        if self.interval_minutes == 0 {
            errors.push(ValidationError::ZeroInterval);
        }

        // Timeout must be positive.
        if self.network_timeout_seconds == 0 {
            errors.push(ValidationError::ZeroTimeout);
        }

        // Source path validation.
        let mut seen_paths = std::collections::HashSet::new();
        for (index, source) in self.sources.iter().enumerate() {
            let path = &source.path;

            if path.trim().is_empty() {
                errors.push(ValidationError::EmptySourcePath { index });
                continue;
            }

            if Path::new(path).is_absolute() {
                errors.push(ValidationError::AbsoluteSourcePath {
                    index,
                    path: path.clone(),
                });
            }

            if contains_parent_traversal(path) {
                errors.push(ValidationError::ParentTraversal {
                    index,
                    path: path.clone(),
                });
            }

            // Normalize for duplicate detection.
            let normalized = normalize_source_path(path);
            if !seen_paths.insert(normalized.clone()) {
                errors.push(ValidationError::DuplicateSource {
                    index,
                    path: path.clone(),
                });
            }
        }

        errors
    }

    /// Load configuration from the given file path.
    ///
    /// Returns `ConfigError::NotFound` if the file does not exist.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::NotFound {
                path: path.to_path_buf(),
            });
        }

        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;

        let config = Self::from_toml(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(config)
    }

    /// Save configuration atomically to the given file path.
    ///
    /// Creates the parent directory if it does not exist. Writes to a
    /// temporary file in the same directory and renames it into place so
    /// an interrupted write never corrupts the configuration.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = self.to_toml()?;

        // Ensure the parent directory exists.
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|source| ConfigError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        // Write to a temporary file in the same directory so that rename is
        // atomic on the same filesystem.
        let parent = path.parent().unwrap_or(Path::new("."));
        let mut tmp =
            tempfile::NamedTempFile::new_in(parent).map_err(|source| ConfigError::Write {
                path: path.to_path_buf(),
                source,
            })?;

        tmp.write_all(text.as_bytes())
            .map_err(|source| ConfigError::Write {
                path: path.to_path_buf(),
                source,
            })?;

        tmp.flush().map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;

        tmp.persist(path).map_err(|source| ConfigError::Persist {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(())
    }
}

/// Validate a user-selected machine namespace as a portable path component.
///
/// This is shared with manifest validation so a repository ownership marker
/// cannot declare a namespace that local configuration would refuse to use.
pub fn validate_namespace(namespace: &str) -> Result<(), ValidationError> {
    if namespace.is_empty() {
        return Err(ValidationError::EmptyNamespace);
    }

    if Path::new(namespace).is_absolute() {
        return Err(ValidationError::AbsoluteNamespace {
            namespace: namespace.to_string(),
        });
    }

    if namespace.contains(['/', '\\']) {
        return Err(ValidationError::NamespaceContainsSeparator {
            namespace: namespace.to_string(),
        });
    }

    if matches!(namespace, "." | "..") {
        return Err(ValidationError::ReservedNamespace {
            namespace: namespace.to_string(),
        });
    }

    if !namespace
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(ValidationError::InvalidNamespaceCharacter {
            namespace: namespace.to_string(),
        });
    }

    Ok(())
}

fn contains_parent_traversal(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Normalize a source path for duplicate detection by stripping trailing
/// slashes and collapsing redundant separators.
fn normalize_source_path(path: &str) -> String {
    let normalized: PathBuf = Path::new(path).components().collect();
    normalized.to_string_lossy().into_owned()
}

#[cfg(test)]
#[path = "../tests/unit/config.rs"]
mod tests;
