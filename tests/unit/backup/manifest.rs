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

    let manifest = Manifest::from_sources("desktop", &sources);

    assert_eq!(manifest.format, FORMAT_IDENTIFIER);
    assert_eq!(manifest.version, Manifest::CURRENT_VERSION);
    assert_eq!(manifest.namespace, "desktop");
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
        version: Manifest::CURRENT_VERSION,
        namespace: "desktop".to_string(),
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
    let manifest = Manifest::from_sources("desktop", &[]);

    assert!(manifest.validate().is_ok());
}

#[test]
fn rejects_wrong_format_identifier() {
    let manifest = Manifest {
        format: "something-else".to_string(),
        version: Manifest::CURRENT_VERSION,
        namespace: "desktop".to_string(),
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
        namespace: "desktop".to_string(),
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
    let repo = tmp.path().join("desktop");
    std::fs::create_dir(&repo).unwrap();

    let manifest = Manifest::from_sources(
        "desktop",
        &[SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string()],
        }],
    );

    manifest.save(&repo).unwrap();

    let loaded = Manifest::load_from_directory(&repo).unwrap();
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
    std::fs::write(
        &path,
        "format = \"wrong\"\nversion = 2\nnamespace = \"desktop\"\nsources = []\n",
    )
    .unwrap();

    let result = Manifest::load(tmp.path());

    assert!(matches!(result, Err(ManifestError::InvalidFormat { .. })));
}

#[test]
fn load_rejects_unsupported_version_in_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(app::MANIFEST_FILE_NAME);
    let content = format!(
        "format = \"{FORMAT_IDENTIFIER}\"\nversion = 99\nnamespace = \"desktop\"\nsources = []\n"
    );
    std::fs::write(&path, content).unwrap();

    let result = Manifest::load(tmp.path());

    assert!(matches!(
        result,
        Err(ManifestError::UnsupportedVersion { .. })
    ));
}

#[test]
fn load_rejects_manifest_substituted_from_another_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    let desktop = tmp.path().join("desktop");
    std::fs::create_dir(&desktop).unwrap();
    Manifest::from_sources("notebook", &[])
        .save(&desktop)
        .unwrap();

    let result = Manifest::load_from_directory(&desktop);

    assert!(matches!(
        result,
        Err(ManifestError::NamespaceMismatch { .. })
    ));
}

#[test]
fn rejects_malformed_manifest_namespace() {
    let manifest = Manifest::from_sources("desktop/../../notebook", &[]);

    assert!(matches!(
        manifest.validate(),
        Err(ManifestError::InvalidNamespace { .. })
    ));
}

#[test]
fn manifest_file_name_matches_app_constant() {
    let repo = Path::new("/home/user/dotfiles");

    assert_eq!(Manifest::path_in(repo), repo.join(app::MANIFEST_FILE_NAME));
}

#[test]
fn serialized_manifest_contains_format_identifier() {
    let manifest = Manifest::from_sources("desktop", &[]);
    let text = manifest.to_toml().unwrap();

    assert!(text.contains(FORMAT_IDENTIFIER));
}
