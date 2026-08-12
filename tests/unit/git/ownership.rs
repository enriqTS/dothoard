use std::fs;

use super::*;
use crate::backup::manifest::FORMAT_IDENTIFIER;
use crate::config::SourceConfig;

const NAMESPACE: &str = "desktop";

fn namespace_dir(repository: &Path) -> PathBuf {
    repository.join(NAMESPACE)
}

#[test]
fn empty_repository_is_new() {
    let tmp = tempfile::tempdir().unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

    assert_eq!(state, OwnershipState::New);
}

#[test]
fn empty_home_directory_is_new() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(namespace_dir(tmp.path()).join("home")).unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

    assert_eq!(state, OwnershipState::New);
}

#[test]
fn valid_manifest_classifies_as_owned() {
    let tmp = tempfile::tempdir().unwrap();

    fs::create_dir(namespace_dir(tmp.path())).unwrap();
    let manifest = Manifest::from_sources(
        NAMESPACE,
        &[
            SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec!["*.log".to_string()],
            },
            SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
        ],
    );
    manifest.save(&namespace_dir(tmp.path())).unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

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
    let home = namespace_dir(tmp.path()).join("home");
    fs::create_dir_all(home.join(".config/fish")).unwrap();
    fs::write(home.join(".config/fish/config.fish"), "# fish").unwrap();

    let manifest = Manifest::from_sources(
        NAMESPACE,
        &[SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }],
    );
    manifest.save(&namespace_dir(tmp.path())).unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

    assert!(matches!(state, OwnershipState::Owned { .. }));
}

#[test]
fn home_content_without_manifest_is_ambiguous() {
    let tmp = tempfile::tempdir().unwrap();
    let home = namespace_dir(tmp.path()).join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "# bashrc").unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

    match state {
        OwnershipState::Ambiguous { reason } => {
            assert!(reason.contains("no manifest"));
        }
        other => panic!("expected Ambiguous, got: {other}"),
    }
}

#[test]
fn manifest_from_sibling_namespace_is_invalid() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(namespace_dir(tmp.path())).unwrap();
    Manifest::from_sources("notebook", &[])
        .save(&namespace_dir(tmp.path()))
        .unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

    assert!(matches!(state, OwnershipState::InvalidManifest { .. }));
}

#[test]
fn invalid_manifest_format_detected() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(namespace_dir(tmp.path())).unwrap();
    let manifest_path = namespace_dir(tmp.path()).join(app::MANIFEST_FILE_NAME);
    fs::write(
        &manifest_path,
        "format = \"wrong-format\"\nversion = 2\nnamespace = \"desktop\"\nsources = []\n",
    )
    .unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

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
    fs::create_dir(namespace_dir(tmp.path())).unwrap();
    let manifest_path = namespace_dir(tmp.path()).join(app::MANIFEST_FILE_NAME);
    let content = format!(
        "format = \"{FORMAT_IDENTIFIER}\"\nversion = 99\nnamespace = \"desktop\"\nsources = []\n"
    );
    fs::write(&manifest_path, content).unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

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
    fs::create_dir(namespace_dir(tmp.path())).unwrap();
    let manifest_path = namespace_dir(tmp.path()).join(app::MANIFEST_FILE_NAME);
    fs::write(&manifest_path, "this is not valid [[[toml").unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

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
    let home = namespace_dir(tmp.path()).join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("file.txt"), "data").unwrap();

    let manifest_path = namespace_dir(tmp.path()).join(app::MANIFEST_FILE_NAME);
    fs::write(&manifest_path, "broken toml {{{{").unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

    // Invalid manifest takes precedence over ambiguous home content.
    assert!(matches!(state, OwnershipState::InvalidManifest { .. }));
}

#[test]
fn root_level_v1_paths_are_unmanaged() {
    let tmp = tempfile::tempdir().unwrap();
    let root_home = tmp.path().join("home");
    fs::create_dir_all(&root_home).unwrap();
    fs::write(root_home.join(".bashrc"), "legacy").unwrap();
    fs::write(
        tmp.path().join(app::MANIFEST_FILE_NAME),
        "this root-level manifest is deliberately invalid",
    )
    .unwrap();

    assert_eq!(
        classify_ownership(tmp.path(), NAMESPACE).unwrap(),
        OwnershipState::New
    );
}

#[test]
fn sibling_namespace_is_unmanaged() {
    let tmp = tempfile::tempdir().unwrap();
    let sibling = tmp.path().join("notebook");
    fs::create_dir_all(sibling.join("home")).unwrap();
    fs::write(sibling.join("home/.bashrc"), "sibling").unwrap();
    fs::write(
        sibling.join(app::MANIFEST_FILE_NAME),
        "this sibling manifest is deliberately invalid",
    )
    .unwrap();

    assert_eq!(
        classify_ownership(tmp.path(), NAMESPACE).unwrap(),
        OwnershipState::New
    );
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
    let home = namespace_dir(tmp.path()).join("home");
    fs::create_dir_all(home.join(".config/nvim")).unwrap();
    fs::write(home.join(".config/nvim/init.lua"), "-- nvim").unwrap();

    let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

    assert!(matches!(state, OwnershipState::Ambiguous { .. }));
}
