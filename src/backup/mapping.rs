//! Source-to-destination path mapping.
//!
//! Each configured source maps beneath the selected machine namespace's
//! `home/` directory. Mapping functions in this module deliberately require a
//! namespace so callers cannot accidentally use the legacy root-level layout.

use std::path::{Component, Path, PathBuf};

/// The name of the managed home directory inside a namespace.
pub const HOME_DIR_NAME: &str = "home";

/// Maps a home-relative source path to its absolute source path.
pub fn source_absolute(home: &Path, relative_source: &str) -> PathBuf {
    home.join(relative_source)
}

/// Returns the absolute path to the selected namespace directory.
pub fn namespace_dir(repository: &Path, namespace: &str) -> PathBuf {
    repository.join(namespace)
}

/// Returns the absolute path to the selected namespace's managed `home/` directory.
pub fn managed_home_dir(repository: &Path, namespace: &str) -> PathBuf {
    namespace_dir(repository, namespace).join(HOME_DIR_NAME)
}

/// Maps a home-relative source path into the selected namespace.
pub fn destination_root(repository: &Path, namespace: &str, relative_source: &str) -> PathBuf {
    managed_home_dir(repository, namespace).join(relative_source)
}

/// Maps an absolute source file path to its corresponding namespaced destination.
pub fn map_source_to_destination(
    home: &Path,
    repository: &Path,
    namespace: &str,
    source_path: &Path,
) -> Option<PathBuf> {
    let relative = source_path.strip_prefix(home).ok()?;
    Some(managed_home_dir(repository, namespace).join(relative))
}

/// Maps a namespaced destination path back to its home-relative path.
pub fn map_destination_to_relative(
    repository: &Path,
    namespace: &str,
    destination_path: &Path,
) -> Option<PathBuf> {
    destination_path
        .strip_prefix(managed_home_dir(repository, namespace))
        .ok()
        .map(PathBuf::from)
}

/// Checks whether a path is inside the selected namespace's `home/` directory.
pub fn is_managed_path(repository: &Path, namespace: &str, path: &Path) -> bool {
    normalize_lexical(path).starts_with(normalize_lexical(&managed_home_dir(repository, namespace)))
}

/// Normalize components without resolving symlinks so containment checks cannot
/// be bypassed with lexical `..` segments.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

#[cfg(test)]
mod tests {
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
}
