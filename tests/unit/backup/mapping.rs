use super::*;

const NAMESPACE: &str = "desktop";

#[test]
fn source_absolute_joins_home_and_relative() {
    assert_eq!(
        source_absolute(Path::new("/home/user"), ".config/fish"),
        PathBuf::from("/home/user/.config/fish")
    );
}

#[test]
fn mappings_are_confined_to_the_selected_namespace() {
    let home = Path::new("/home/user");
    let repo = Path::new("/home/user/dotfiles");
    let source = Path::new("/home/user/.config/fish/config.fish");
    let expected = PathBuf::from("/home/user/dotfiles/desktop/home/.config/fish/config.fish");

    assert_eq!(
        destination_root(repo, NAMESPACE, ".config/fish"),
        expected.parent().unwrap()
    );
    assert_eq!(
        map_source_to_destination(home, repo, NAMESPACE, source),
        Some(expected.clone())
    );
    assert_eq!(
        map_destination_to_relative(repo, NAMESPACE, &expected),
        Some(PathBuf::from(".config/fish/config.fish"))
    );
}

#[test]
fn reverse_mapping_and_managed_path_reject_siblings_and_legacy_paths() {
    let repo = Path::new("/home/user/dotfiles");
    let sibling = repo.join("notebook/home/.bashrc");
    let legacy = repo.join("home/.bashrc");

    assert_eq!(map_destination_to_relative(repo, NAMESPACE, &sibling), None);
    assert_eq!(map_destination_to_relative(repo, NAMESPACE, &legacy), None);
    assert!(!is_managed_path(repo, NAMESPACE, &sibling));
    assert!(!is_managed_path(repo, NAMESPACE, &legacy));
    assert!(is_managed_path(
        repo,
        NAMESPACE,
        &repo.join("desktop/home/.bashrc")
    ));
}

#[test]
fn namespace_and_home_paths_are_deterministic() {
    let repo = Path::new("/home/user/dotfiles");
    assert_eq!(
        namespace_dir(repo, NAMESPACE),
        PathBuf::from("/home/user/dotfiles/desktop")
    );
    assert_eq!(
        managed_home_dir(repo, NAMESPACE),
        PathBuf::from("/home/user/dotfiles/desktop/home")
    );
}
