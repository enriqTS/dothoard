//! Configuration models and persistence.
//!
//! The configuration file lives at `~/.config/config-sync/config.toml` and
//! describes the repository location, remote, schedule, and source mappings.
//! This module defines the schema and serialization; validation logic lives
//! in dedicated functions that operate on the deserialized model.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

    /// Backup interval in minutes for the systemd timer. Defaults to 5.
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u32,

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
    pub const CURRENT_VERSION: u32 = 1;

    /// Deserialize a configuration from TOML text.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Serialize the configuration to TOML text.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Create a minimal default configuration pointing at the given repository.
    pub fn new(repository: impl Into<String>) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            repository: repository.into(),
            remote: default_remote(),
            interval_minutes: default_interval_minutes(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_TOML: &str = r#"
version = 1
repository = "~/pessoal/example-repo"
remote = "origin"
interval_minutes = 5
network_timeout_seconds = 120

[[sources]]
path = ".config/fish"
ignore = [
  "*.log",
  "fish_variables",
]

[[sources]]
path = ".config/waybar"
ignore = [
  "cache/",
  "*token*",
]
"#;

    #[test]
    fn deserializes_complete_config() {
        let config = Config::from_toml(EXAMPLE_TOML).unwrap();

        assert_eq!(config.version, 1);
        assert_eq!(config.repository, "~/pessoal/example-repo");
        assert_eq!(config.remote, "origin");
        assert_eq!(config.interval_minutes, 5);
        assert_eq!(config.network_timeout_seconds, 120);
        assert_eq!(config.sources.len(), 2);
        assert_eq!(config.sources[0].path, ".config/fish");
        assert_eq!(config.sources[0].ignore, vec!["*.log", "fish_variables"]);
        assert_eq!(config.sources[1].path, ".config/waybar");
        assert_eq!(config.sources[1].ignore, vec!["cache/", "*token*"]);
    }

    #[test]
    fn applies_defaults_for_omitted_fields() {
        let minimal = r#"
version = 1
repository = "~/repo"
"#;
        let config = Config::from_toml(minimal).unwrap();

        assert_eq!(config.remote, "origin");
        assert_eq!(config.interval_minutes, 5);
        assert_eq!(config.network_timeout_seconds, 120);
        assert!(config.sources.is_empty());
    }

    #[test]
    fn round_trips_through_toml() {
        let original = Config {
            version: 1,
            repository: "~/pessoal/dotfiles".to_string(),
            remote: "upstream".to_string(),
            interval_minutes: 10,
            network_timeout_seconds: 60,
            sources: vec![
                SourceConfig {
                    path: ".bashrc".to_string(),
                    ignore: vec![],
                },
                SourceConfig {
                    path: ".config/nvim".to_string(),
                    ignore: vec!["plugin/".to_string(), "*.swp".to_string()],
                },
            ],
        };

        let text = original.to_toml().unwrap();
        let restored = Config::from_toml(&text).unwrap();

        assert_eq!(original, restored);
    }

    #[test]
    fn new_creates_minimal_config() {
        let config = Config::new("~/pessoal/sync");

        assert_eq!(config.version, Config::CURRENT_VERSION);
        assert_eq!(config.repository, "~/pessoal/sync");
        assert_eq!(config.remote, "origin");
        assert_eq!(config.interval_minutes, 5);
        assert_eq!(config.network_timeout_seconds, 120);
        assert!(config.sources.is_empty());
    }

    #[test]
    fn expands_tilde_in_repository_path() {
        let config = Config::new("~/pessoal/dotfiles");
        let home = std::path::Path::new("/home/user");

        assert_eq!(
            config.repository_path(home),
            PathBuf::from("/home/user/pessoal/dotfiles")
        );
    }

    #[test]
    fn preserves_absolute_repository_path() {
        let config = Config {
            repository: "/opt/backups/dotfiles".to_string(),
            ..Config::new("")
        };
        let home = std::path::Path::new("/home/user");

        assert_eq!(
            config.repository_path(home),
            PathBuf::from("/opt/backups/dotfiles")
        );
    }

    #[test]
    fn handles_bare_tilde_repository_path() {
        let config = Config {
            repository: "~".to_string(),
            ..Config::new("")
        };
        let home = std::path::Path::new("/home/user");

        assert_eq!(config.repository_path(home), PathBuf::from("/home/user"));
    }

    #[test]
    fn rejects_missing_required_fields() {
        let missing_version = r#"
repository = "~/repo"
"#;
        assert!(Config::from_toml(missing_version).is_err());

        let missing_repository = r#"
version = 1
"#;
        assert!(Config::from_toml(missing_repository).is_err());
    }

    #[test]
    fn source_with_empty_ignore_list() {
        let text = r#"
version = 1
repository = "~/repo"

[[sources]]
path = ".bashrc"
"#;
        let config = Config::from_toml(text).unwrap();

        assert_eq!(config.sources.len(), 1);
        assert_eq!(config.sources[0].path, ".bashrc");
        assert!(config.sources[0].ignore.is_empty());
    }
}
