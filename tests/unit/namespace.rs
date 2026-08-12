use super::*;
use crate::config::SourceConfig;

fn config(repository: &Path, namespace: &str) -> Config {
    Config::new(repository.display().to_string(), namespace)
}

fn owned(repository: &Path, namespace: &str) {
    let directory = repository.join(namespace);
    fs::create_dir_all(directory.join("home")).unwrap();
    Manifest::from_sources(
        namespace,
        &[SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        }],
    )
    .save(&directory)
    .unwrap();
}

#[test]
fn create_requires_confirmation_and_rejects_collisions() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        create(tmp.path(), "desktop", false),
        Err(NamespaceError::ConfirmationRequired)
    ));
    create(tmp.path(), "desktop", true).unwrap();
    assert!(tmp.path().join("desktop/home").is_dir());
    fs::write(tmp.path().join("desktop/unmanaged"), "x").unwrap();
    assert!(matches!(
        create(tmp.path(), "desktop", true),
        Err(NamespaceError::Collision { .. })
    ));
}

#[test]
fn select_refuses_ambiguous_and_persists_usable_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("config.toml");
    let mut config = config(tmp.path(), "desktop");
    config.save(&config_path).unwrap();
    let state = select(&config_path, &mut config, tmp.path(), "notebook").unwrap();
    assert!(matches!(state, OwnershipState::New));
    assert_eq!(Config::load(&config_path).unwrap().namespace, "notebook");
    fs::create_dir_all(tmp.path().join("broken/home")).unwrap();
    fs::write(tmp.path().join("broken/home/file"), "x").unwrap();
    assert!(matches!(
        select(&config_path, &mut config, tmp.path(), "broken"),
        Err(NamespaceError::Ownership { .. })
    ));
}

#[test]
fn rename_updates_manifest_and_leaves_siblings_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    owned(tmp.path(), "desktop");
    owned(tmp.path(), "notebook");
    let config_path = tmp.path().join("config.toml");
    let mut config = config(tmp.path(), "desktop");
    rename(&config_path, &mut config, tmp.path(), "server", true).unwrap();
    assert_eq!(config.namespace, "server");
    assert!(!tmp.path().join("desktop").exists());
    assert_eq!(
        Manifest::load_from_directory(&tmp.path().join("server"))
            .unwrap()
            .namespace,
        "server"
    );
    assert!(tmp.path().join("notebook/.dothoard-manifest.toml").exists());
}

#[test]
fn rename_cancellation_and_collision_leave_source_intact() {
    let tmp = tempfile::tempdir().unwrap();
    owned(tmp.path(), "desktop");
    owned(tmp.path(), "notebook");
    let config_path = tmp.path().join("config.toml");
    let mut config = config(tmp.path(), "desktop");
    assert!(matches!(
        rename(&config_path, &mut config, tmp.path(), "server", false),
        Err(NamespaceError::ConfirmationRequired)
    ));
    assert!(matches!(
        rename(&config_path, &mut config, tmp.path(), "notebook", true),
        Err(NamespaceError::Collision { .. })
    ));
    assert!(tmp.path().join("desktop/.dothoard-manifest.toml").exists());
}

#[test]
fn lifecycle_refuses_dirty_or_staged_sibling_worktree_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = crate::git::GitRunner::new(Duration::from_secs(10));
    runner
        .run(&crate::git::GitCommand::new(tmp.path()).args(["init", "--initial-branch=main"]))
        .unwrap();
    runner
        .run(&crate::git::GitCommand::new(tmp.path()).args([
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ]))
        .unwrap();
    owned(tmp.path(), "desktop");
    runner
        .run(&crate::git::GitCommand::new(tmp.path()).args(["add", "--", "desktop"]))
        .unwrap();
    runner
        .run(&crate::git::GitCommand::new(tmp.path()).args(["commit", "-m", "desktop"]))
        .unwrap();
    fs::create_dir_all(tmp.path().join("notebook/home")).unwrap();
    fs::write(tmp.path().join("notebook/home/file"), "dirty sibling").unwrap();

    let config_path = tmp.path().join("config.toml");
    let mut config = config(tmp.path(), "desktop");
    assert!(matches!(
        rename(&config_path, &mut config, tmp.path(), "server", true),
        Err(NamespaceError::DirtyWorktree { .. })
    ));
    assert!(tmp.path().join("desktop/.dothoard-manifest.toml").exists());
}

#[test]
fn delete_requires_replacement_and_only_removes_owned_paths() {
    let tmp = tempfile::tempdir().unwrap();
    owned(tmp.path(), "desktop");
    owned(tmp.path(), "notebook");
    fs::create_dir_all(tmp.path().join("home")).unwrap();
    fs::write(tmp.path().join("home/legacy"), "legacy").unwrap();
    let config_path = tmp.path().join("config.toml");
    let mut config = config(tmp.path(), "desktop");
    assert!(matches!(
        delete(&config_path, &mut config, tmp.path(), "desktop", true),
        Err(NamespaceError::ReplacementRequired)
    ));
    delete(&config_path, &mut config, tmp.path(), "notebook", true).unwrap();
    assert_eq!(config.namespace, "notebook");
    assert!(!tmp.path().join("desktop/home").exists());
    assert!(!tmp.path().join("desktop/.dothoard-manifest.toml").exists());
    assert_eq!(
        fs::read_to_string(tmp.path().join("home/legacy")).unwrap(),
        "legacy"
    );
    assert!(tmp.path().join("notebook/.dothoard-manifest.toml").exists());
}
