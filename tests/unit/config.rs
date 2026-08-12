use super::*;

const EXAMPLE_TOML: &str = r#"
version = 2
repository = "~/pessoal/example-repo"
remote = "origin"
namespace = "desktop"
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

    assert_eq!(config.version, Config::CURRENT_VERSION);
    assert_eq!(config.repository, "~/pessoal/example-repo");
    assert_eq!(config.remote, "origin");
    assert_eq!(config.namespace, "desktop");
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
version = 2
repository = "~/repo"
namespace = "notebook"
"#;
    let config = Config::from_toml(minimal).unwrap();

    assert_eq!(config.remote, "origin");
    assert_eq!(config.namespace, "notebook");
    assert_eq!(config.interval_minutes, 5);
    assert_eq!(config.network_timeout_seconds, 120);
    assert!(config.sources.is_empty());
}

#[test]
fn round_trips_through_toml() {
    let original = Config {
        version: Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
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
    let config = Config::new("~/pessoal/sync", "desktop");

    assert_eq!(config.version, Config::CURRENT_VERSION);
    assert_eq!(config.repository, "~/pessoal/sync");
    assert_eq!(config.remote, "origin");
    assert_eq!(config.namespace, "desktop");
    assert_eq!(config.interval_minutes, 5);
    assert_eq!(config.network_timeout_seconds, 120);
    assert!(config.sources.is_empty());
}

#[test]
fn expands_tilde_in_repository_path() {
    let config = Config::new("~/pessoal/dotfiles", "desktop");
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
        ..Config::new("", "test-machine")
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
        ..Config::new("", "test-machine")
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
version = 2
repository = "~/repo"
namespace = "desktop"

[[sources]]
path = ".bashrc"
"#;
    let config = Config::from_toml(text).unwrap();

    assert_eq!(config.sources.len(), 1);
    assert_eq!(config.sources[0].path, ".bashrc");
    assert!(config.sources[0].ignore.is_empty());
}

#[test]
fn save_creates_parent_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nested").join("dir").join("config.toml");
    let config = Config::new("~/repo", "test-machine");

    config.save(&path).unwrap();

    assert!(path.exists());
    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded, config);
}

#[test]
fn save_and_load_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let config = Config {
        version: Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/dotfiles".to_string(),
        remote: "upstream".to_string(),
        interval_minutes: 10,
        network_timeout_seconds: 60,
        sources: vec![SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string()],
        }],
    };

    config.save(&path).unwrap();
    let loaded = Config::load(&path).unwrap();

    assert_eq!(loaded, config);
}

#[test]
fn save_overwrites_existing_file_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");

    // Write initial config.
    let first = Config::new("~/first", "test-machine");
    first.save(&path).unwrap();

    // Overwrite with different config.
    let second = Config::new("~/second", "test-machine");
    second.save(&path).unwrap();

    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded.repository, "~/second");
}

#[test]
fn load_returns_not_found_for_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nonexistent.toml");

    let result = Config::load(&path);

    assert!(matches!(result, Err(ConfigError::NotFound { .. })));
}

#[test]
fn load_returns_parse_error_for_invalid_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.toml");
    std::fs::write(&path, "this is not valid toml [[[").unwrap();

    let result = Config::load(&path);

    assert!(matches!(result, Err(ConfigError::Parse { .. })));
}

// --- Validation tests (C04) ---

#[test]
fn valid_config_produces_no_errors() {
    let config = Config {
        version: Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/dotfiles".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        network_timeout_seconds: 120,
        sources: vec![SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }],
    };

    assert!(config.validate().is_empty());
}

#[test]
fn rejects_unsupported_version() {
    let config = Config {
        version: 99,
        namespace: "test-machine".to_string(),
        ..Config::new("~/repo", "test-machine")
    };

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::UnsupportedVersion {
        found: 99,
        supported: Config::CURRENT_VERSION,
    }));
}

#[test]
fn rejects_empty_repository() {
    let config = Config {
        repository: "  ".to_string(),
        ..Config::new("", "test-machine")
    };

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::EmptyRepository));
}

#[test]
fn rejects_empty_remote() {
    let config = Config {
        remote: "".to_string(),
        ..Config::new("~/repo", "test-machine")
    };

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::EmptyRemote));
}

#[test]
fn rejects_empty_namespace() {
    let config = Config::new("~/repo", "");

    assert!(config.validate().contains(&ValidationError::EmptyNamespace));
}

#[test]
fn rejects_absolute_namespace() {
    let config = Config::new("~/repo", "/desktop");

    assert!(
        config
            .validate()
            .contains(&ValidationError::AbsoluteNamespace {
                namespace: "/desktop".to_string(),
            })
    );
}

#[test]
fn rejects_namespace_with_path_separators() {
    for namespace in ["desktop/notebook", r"desktop\\notebook"] {
        let config = Config::new("~/repo", namespace);

        assert!(
            config
                .validate()
                .contains(&ValidationError::NamespaceContainsSeparator {
                    namespace: namespace.to_string(),
                })
        );
    }
}

#[test]
fn rejects_reserved_namespace_components() {
    for namespace in [".", ".."] {
        let config = Config::new("~/repo", namespace);

        assert!(
            config
                .validate()
                .contains(&ValidationError::ReservedNamespace {
                    namespace: namespace.to_string(),
                })
        );
    }
}

#[test]
fn rejects_nonportable_namespace_characters() {
    for namespace in ["desktop name", "desktop:pc", "notebooké"] {
        let config = Config::new("~/repo", namespace);

        assert!(
            config
                .validate()
                .contains(&ValidationError::InvalidNamespaceCharacter {
                    namespace: namespace.to_string(),
                })
        );
    }
}

#[test]
fn accepts_portable_namespace() {
    let config = Config::new("~/repo", "home-server_2.0");

    assert!(config.validate().is_empty());
}

#[test]
fn old_config_without_namespace_is_rejected_by_validation() {
    let config = Config::from_toml(
        r#"
version = 1
repository = "~/repo"
"#,
    )
    .unwrap();

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::UnsupportedVersion {
        found: 1,
        supported: Config::CURRENT_VERSION,
    }));
    assert!(errors.contains(&ValidationError::EmptyNamespace));
}

#[test]
fn rejects_zero_interval() {
    let config = Config {
        interval_minutes: 0,
        ..Config::new("~/repo", "test-machine")
    };

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::ZeroInterval));
}

#[test]
fn rejects_zero_timeout() {
    let config = Config {
        network_timeout_seconds: 0,
        ..Config::new("~/repo", "test-machine")
    };

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::ZeroTimeout));
}

#[test]
fn rejects_empty_source_path() {
    let config = Config {
        sources: vec![SourceConfig {
            path: "".to_string(),
            ignore: vec![],
        }],
        ..Config::new("~/repo", "test-machine")
    };

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::EmptySourcePath { index: 0 }));
}

#[test]
fn rejects_absolute_source_path() {
    let config = Config {
        sources: vec![SourceConfig {
            path: "/etc/passwd".to_string(),
            ignore: vec![],
        }],
        ..Config::new("~/repo", "test-machine")
    };

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::AbsoluteSourcePath {
        index: 0,
        path: "/etc/passwd".to_string(),
    }));
}

#[test]
fn rejects_parent_traversal_in_source_path() {
    let cases = vec![
        ".config/../secrets",
        "../outside",
        "a/b/../../c/../../../d",
        "..",
    ];

    for case in cases {
        let config = Config {
            sources: vec![SourceConfig {
                path: case.to_string(),
                ignore: vec![],
            }],
            ..Config::new("~/repo", "test-machine")
        };

        let errors = config.validate();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ValidationError::ParentTraversal { .. })),
            "expected ParentTraversal for path: {case}"
        );
    }
}

#[test]
fn accepts_dotfile_paths_without_traversal() {
    let config = Config {
        sources: vec![
            SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec![],
            },
            SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
            SourceConfig {
                path: ".local/share/nvim".to_string(),
                ignore: vec![],
            },
        ],
        ..Config::new("~/repo", "test-machine")
    };

    assert!(config.validate().is_empty());
}

#[test]
fn rejects_duplicate_source_paths() {
    let config = Config {
        sources: vec![
            SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec![],
            },
            SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec!["*.log".to_string()],
            },
        ],
        ..Config::new("~/repo", "test-machine")
    };

    let errors = config.validate();
    assert!(errors.contains(&ValidationError::DuplicateSource {
        index: 1,
        path: ".config/fish".to_string(),
    }));
}

#[test]
fn detects_duplicates_with_trailing_slash_difference() {
    let config = Config {
        sources: vec![
            SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec![],
            },
            SourceConfig {
                path: ".config/fish/".to_string(),
                ignore: vec![],
            },
        ],
        ..Config::new("~/repo", "test-machine")
    };

    let errors = config.validate();
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateSource { index: 1, .. }))
    );
}

#[test]
fn collects_multiple_errors() {
    let config = Config {
        version: 99,
        namespace: "test-machine".to_string(),
        repository: "".to_string(),
        remote: "".to_string(),
        interval_minutes: 0,
        network_timeout_seconds: 0,
        sources: vec![
            SourceConfig {
                path: "".to_string(),
                ignore: vec![],
            },
            SourceConfig {
                path: "/absolute".to_string(),
                ignore: vec![],
            },
            SourceConfig {
                path: "../traversal".to_string(),
                ignore: vec![],
            },
        ],
    };

    let errors = config.validate();
    // Should have at least: UnsupportedVersion, EmptyRepository, EmptyRemote,
    // ZeroInterval, ZeroTimeout, EmptySourcePath, AbsoluteSourcePath, ParentTraversal
    assert!(errors.len() >= 8);
}
