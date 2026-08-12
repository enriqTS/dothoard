use super::*;
use tempfile::TempDir;

fn make_test_dirs() -> (TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let home = root.join("home");
    let config = root.join("config");
    let state = root.join("state");
    let runtime = root.join("runtime");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    (tmp, home, config, state, runtime)
}

#[test]
fn resolves_from_injected_inputs() {
    let (_tmp, home, config, state, runtime) = make_test_dirs();

    let paths = AppPaths::resolve(PathInputs {
        home: Some(home.clone()),
        config_dir: Some(config.clone()),
        state_dir: Some(state.clone()),
        runtime_dir: Some(runtime.clone()),
        use_environment: false,
    })
    .unwrap();

    assert_eq!(paths.home(), home);
    assert_eq!(paths.config_dir(), config);
    assert_eq!(paths.config_file(), config.join(app::CONFIG_FILE_NAME));
    assert_eq!(paths.state_dir(), state);
    assert_eq!(paths.runtime_dir(), runtime);
}

#[test]
fn rejects_relative_home() {
    let (_tmp, _home, config, state, runtime) = make_test_dirs();

    let result = AppPaths::resolve(PathInputs {
        home: Some(PathBuf::from("relative/home")),
        config_dir: Some(config),
        state_dir: Some(state),
        runtime_dir: Some(runtime),
        use_environment: false,
    });

    let error = result.unwrap_err();
    assert!(matches!(error, PathError::NotAbsolute { name: "home", .. }));
}

#[test]
fn rejects_nonexistent_home() {
    let (_tmp, _home, config, state, runtime) = make_test_dirs();
    let missing = PathBuf::from("/nonexistent/path/for/testing");

    let result = AppPaths::resolve(PathInputs {
        home: Some(missing),
        config_dir: Some(config),
        state_dir: Some(state),
        runtime_dir: Some(runtime),
        use_environment: false,
    });

    let error = result.unwrap_err();
    assert!(matches!(error, PathError::NotFound { name: "home", .. }));
}

#[test]
fn rejects_relative_config_dir() {
    let (_tmp, home, _config, state, runtime) = make_test_dirs();

    let result = AppPaths::resolve(PathInputs {
        home: Some(home),
        config_dir: Some(PathBuf::from("relative/config")),
        state_dir: Some(state),
        runtime_dir: Some(runtime),
        use_environment: false,
    });

    let error = result.unwrap_err();
    assert!(matches!(
        error,
        PathError::NotAbsolute { name: "config", .. }
    ));
}

#[test]
fn rejects_relative_state_dir() {
    let (_tmp, home, config, _state, runtime) = make_test_dirs();

    let result = AppPaths::resolve(PathInputs {
        home: Some(home),
        config_dir: Some(config),
        state_dir: Some(PathBuf::from("relative/state")),
        runtime_dir: Some(runtime),
        use_environment: false,
    });

    let error = result.unwrap_err();
    assert!(matches!(
        error,
        PathError::NotAbsolute { name: "state", .. }
    ));
}

#[test]
fn rejects_relative_runtime_dir() {
    let (_tmp, home, config, state, _runtime) = make_test_dirs();

    let result = AppPaths::resolve(PathInputs {
        home: Some(home),
        config_dir: Some(config),
        state_dir: Some(state),
        runtime_dir: Some(PathBuf::from("relative/runtime")),
        use_environment: false,
    });

    let error = result.unwrap_err();
    assert!(matches!(
        error,
        PathError::NotAbsolute {
            name: "runtime",
            ..
        }
    ));
}

#[test]
fn rejects_nonexistent_runtime_dir() {
    let (_tmp, home, config, state, _runtime) = make_test_dirs();

    let result = AppPaths::resolve(PathInputs {
        home: Some(home),
        config_dir: Some(config),
        state_dir: Some(state),
        runtime_dir: Some(PathBuf::from("/nonexistent/runtime")),
        use_environment: false,
    });

    let error = result.unwrap_err();
    assert!(matches!(
        error,
        PathError::NotFound {
            name: "runtime",
            ..
        }
    ));
}

#[test]
fn config_dir_need_not_exist_yet() {
    let (_tmp, home, _config, state, runtime) = make_test_dirs();
    let nonexistent_config = home.join("nonexistent-config");

    let paths = AppPaths::resolve(PathInputs {
        home: Some(home),
        config_dir: Some(nonexistent_config.clone()),
        state_dir: Some(state),
        runtime_dir: Some(runtime),
        use_environment: false,
    })
    .unwrap();

    assert_eq!(paths.config_dir(), nonexistent_config);
}

#[test]
fn state_dir_need_not_exist_yet() {
    let (_tmp, home, config, _state, runtime) = make_test_dirs();
    let nonexistent_state = home.join("nonexistent-state");

    let paths = AppPaths::resolve(PathInputs {
        home: Some(home),
        config_dir: Some(config),
        state_dir: Some(nonexistent_state.clone()),
        runtime_dir: Some(runtime),
        use_environment: false,
    })
    .unwrap();

    assert_eq!(paths.state_dir(), nonexistent_state);
}

#[test]
fn config_file_is_derived_from_config_dir() {
    let (_tmp, home, config, state, runtime) = make_test_dirs();

    let paths = AppPaths::resolve(PathInputs {
        home: Some(home),
        config_dir: Some(config.clone()),
        state_dir: Some(state),
        runtime_dir: Some(runtime),
        use_environment: false,
    })
    .unwrap();

    assert_eq!(paths.config_file(), config.join(app::CONFIG_FILE_NAME));
}

#[test]
fn fallback_config_derives_from_home() {
    let (_tmp, home, _config, state, runtime) = make_test_dirs();

    // With use_environment=false, config_dir=None derives from home.
    let paths = AppPaths::resolve(PathInputs {
        home: Some(home.clone()),
        config_dir: None,
        state_dir: Some(state),
        runtime_dir: Some(runtime),
        use_environment: false,
    })
    .unwrap();

    let expected = home.join(".config").join(app::CONFIG_DIR_NAME);
    assert_eq!(paths.config_dir(), expected);
}

#[test]
fn fallback_state_derives_from_home() {
    let (_tmp, home, config, _state, runtime) = make_test_dirs();

    let paths = AppPaths::resolve(PathInputs {
        home: Some(home.clone()),
        config_dir: Some(config),
        state_dir: None,
        runtime_dir: Some(runtime),
        use_environment: false,
    })
    .unwrap();

    let expected = home.join(".local").join("state").join(app::STATE_DIR_NAME);
    assert_eq!(paths.state_dir(), expected);
}

// --- Source path validation tests (C05) ---

#[test]
fn accepts_regular_directory_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".config/fish")).unwrap();

    let result = validate_source_path(&home, ".config/fish");

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), home.join(".config/fish"));
}

#[test]
fn accepts_regular_file_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".bashrc"), "# bash").unwrap();

    let result = validate_source_path(&home, ".bashrc");

    assert!(result.is_ok());
}

#[test]
fn accepts_source_root_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let target = tmp.path().join("real-config");
    std::fs::create_dir_all(&target).unwrap();

    // The source root itself is a symlink — allowed.
    std::os::unix::fs::symlink(&target, home.join(".config-link")).unwrap();

    let result = validate_source_path(&home, ".config-link");

    assert!(result.is_ok());
}

#[test]
fn rejects_symlinked_parent_component() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    // Create: home/.config -> /tmp/.../real-config (symlink parent)
    let real_config = tmp.path().join("real-config");
    std::fs::create_dir_all(real_config.join("fish")).unwrap();
    std::os::unix::fs::symlink(&real_config, home.join(".config")).unwrap();

    let result = validate_source_path(&home, ".config/fish");

    assert!(matches!(
        result,
        Err(SourcePathError::SymlinkedParent { .. })
    ));
    if let Err(SourcePathError::SymlinkedParent { symlink_at, .. }) = &result {
        assert_eq!(symlink_at, &home.join(".config"));
    }
}

#[test]
fn rejects_deeply_nested_symlinked_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".local")).unwrap();

    // .local/share is a symlink
    let real_share = tmp.path().join("real-share");
    std::fs::create_dir_all(real_share.join("nvim")).unwrap();
    std::os::unix::fs::symlink(&real_share, home.join(".local/share")).unwrap();

    let result = validate_source_path(&home, ".local/share/nvim");

    assert!(matches!(
        result,
        Err(SourcePathError::SymlinkedParent { .. })
    ));
    if let Err(SourcePathError::SymlinkedParent { symlink_at, .. }) = &result {
        assert_eq!(symlink_at, &home.join(".local/share"));
    }
}

#[test]
fn rejects_nonexistent_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let result = validate_source_path(&home, ".config/nonexistent");

    assert!(matches!(
        result,
        Err(SourcePathError::SourceNotFound { .. })
    ));
}

#[test]
fn accepts_single_component_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();

    // Single component — no parent to check for symlinks.
    let result = validate_source_path(&home, ".ssh");

    assert!(result.is_ok());
}

#[test]
fn accepts_non_symlink_parents_with_symlink_source_root() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".config")).unwrap();

    // .config is a real dir, fish is a symlink (the source root)
    let real_fish = tmp.path().join("real-fish");
    std::fs::create_dir_all(&real_fish).unwrap();
    std::os::unix::fs::symlink(&real_fish, home.join(".config/fish")).unwrap();

    let result = validate_source_path(&home, ".config/fish");

    assert!(result.is_ok());
}

// --- Overlap and recursion validation tests (C06) ---

#[test]
fn no_overlaps_for_disjoint_sources() {
    let sources = vec![
        PathBuf::from("/home/user/.config/fish"),
        PathBuf::from("/home/user/.config/waybar"),
        PathBuf::from("/home/user/.bashrc"),
    ];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    assert!(errors.is_empty());
}

#[test]
fn detects_ancestor_descendant_source_overlap() {
    let sources = vec![
        PathBuf::from("/home/user/.config"),
        PathBuf::from("/home/user/.config/fish"),
    ];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        OverlapError::SourceOverlap {
            first: 0,
            second: 1,
            ..
        }
    ));
}

#[test]
fn detects_descendant_ancestor_source_overlap() {
    // Same as above but reversed order in configuration.
    let sources = vec![
        PathBuf::from("/home/user/.config/fish"),
        PathBuf::from("/home/user/.config"),
    ];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        OverlapError::SourceOverlap {
            first: 1,
            second: 0,
            ..
        }
    ));
}

#[test]
fn detects_identical_source_paths_as_overlap() {
    let sources = vec![
        PathBuf::from("/home/user/.config/fish"),
        PathBuf::from("/home/user/.config/fish"),
    ];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    assert_eq!(errors.len(), 1);
    assert!(matches!(&errors[0], OverlapError::SourceOverlap { .. }));
}

#[test]
fn no_false_overlap_on_partial_name_match() {
    // .config and .config2 should NOT overlap.
    let sources = vec![
        PathBuf::from("/home/user/.config"),
        PathBuf::from("/home/user/.config2"),
    ];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    assert!(errors.is_empty());
}

#[test]
fn detects_source_contains_repository() {
    // Source is an ancestor of the repository — recursive backup risk.
    let sources = vec![PathBuf::from("/home/user/pessoal")];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        OverlapError::RepositoryContainment {
            source_index: 0,
            ..
        }
    ));
    if let OverlapError::RepositoryContainment { description, .. } = &errors[0] {
        assert!(description.contains("source contains the repository"));
    }
}

#[test]
fn detects_repository_contains_source() {
    // Repository is an ancestor of the source.
    let sources = vec![PathBuf::from("/home/user/pessoal/dotfiles/home/.config")];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        OverlapError::RepositoryContainment {
            source_index: 0,
            ..
        }
    ));
    if let OverlapError::RepositoryContainment { description, .. } = &errors[0] {
        assert!(description.contains("repository contains the source"));
    }
}

#[test]
fn detects_source_equals_repository() {
    let sources = vec![PathBuf::from("/home/user/pessoal/dotfiles")];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    // Equal paths mean both directions of containment — at least one error.
    assert!(!errors.is_empty());
}

#[test]
fn detects_multiple_overlap_errors() {
    let sources = vec![
        PathBuf::from("/home/user/.config"),
        PathBuf::from("/home/user/.config/fish"),
        PathBuf::from("/home/user/pessoal"),
    ];
    let repo = Path::new("/home/user/pessoal/dotfiles");

    let errors = check_overlaps(&sources, repo);

    // Should have source overlap (0,1) + repository containment (2).
    assert!(errors.len() >= 2);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, OverlapError::SourceOverlap { .. }))
    );
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, OverlapError::RepositoryContainment { .. }))
    );
}

#[test]
fn is_path_prefix_or_equal_basic_cases() {
    assert!(is_path_prefix_or_equal(
        Path::new("/a/b"),
        Path::new("/a/b/c")
    ));
    assert!(is_path_prefix_or_equal(
        Path::new("/a/b"),
        Path::new("/a/b")
    ));
    assert!(!is_path_prefix_or_equal(
        Path::new("/a/b/c"),
        Path::new("/a/b")
    ));
    assert!(!is_path_prefix_or_equal(
        Path::new("/a/b"),
        Path::new("/a/bc")
    ));
    assert!(is_path_prefix_or_equal(
        Path::new("/"),
        Path::new("/anything")
    ));
}
