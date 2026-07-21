//! Source-to-destination path mapping.
//!
//! Every configured source is a home-relative path. The backup maps each
//! source into the repository under `home/`, preserving its relative position
//! beneath `$HOME`. This module provides the deterministic mapping functions
//! used by the planner and the mirror executor.
//!
//! # Example
//!
//! ```text
//! home:       /home/user
//! repository: /home/user/dotfiles
//! source:     .config/fish
//!
//! absolute source: /home/user/.config/fish
//! destination:     /home/user/dotfiles/home/.config/fish
//! ```

use std::path::{Path, PathBuf};

/// The name of the managed namespace directory inside the repository.
pub const HOME_DIR_NAME: &str = "home";

/// Maps a home-relative source path to its absolute source path.
///
/// # Arguments
///
/// * `home` - Absolute path to the user's home directory.
/// * `relative_source` - Home-relative source path (e.g. `.config/fish`).
pub fn source_absolute(home: &Path, relative_source: &str) -> PathBuf {
    home.join(relative_source)
}

/// Maps a home-relative source path to its destination path in the repository.
///
/// The destination preserves the source's position beneath `$HOME`, placed
/// under the repository's `home/` directory.
///
/// # Arguments
///
/// * `repository` - Absolute path to the repository root.
/// * `relative_source` - Home-relative source path (e.g. `.config/fish`).
pub fn destination_root(repository: &Path, relative_source: &str) -> PathBuf {
    repository.join(HOME_DIR_NAME).join(relative_source)
}

/// Maps an absolute source file path to its corresponding destination path.
///
/// Given an absolute source path and the home directory, strips the home
/// prefix and places the result under `repository/home/`.
///
/// Returns `None` if the source path is not beneath the home directory.
///
/// # Arguments
///
/// * `home` - Absolute path to the user's home directory.
/// * `repository` - Absolute path to the repository root.
/// * `source_path` - Absolute path to a file within a source.
pub fn map_source_to_destination(
    home: &Path,
    repository: &Path,
    source_path: &Path,
) -> Option<PathBuf> {
    let relative = source_path.strip_prefix(home).ok()?;
    Some(repository.join(HOME_DIR_NAME).join(relative))
}

/// Maps an absolute destination path back to its home-relative path.
///
/// Given a path beneath `repository/home/`, strips the repository and `home/`
/// prefix to recover the home-relative path.
///
/// Returns `None` if the destination path is not beneath `repository/home/`.
///
/// # Arguments
///
/// * `repository` - Absolute path to the repository root.
/// * `destination_path` - Absolute path within the managed namespace.
pub fn map_destination_to_relative(repository: &Path, destination_path: &Path) -> Option<PathBuf> {
    let home_root = repository.join(HOME_DIR_NAME);
    destination_path
        .strip_prefix(&home_root)
        .ok()
        .map(PathBuf::from)
}

/// Returns the absolute path to the managed `home/` directory in the repository.
pub fn managed_home_dir(repository: &Path) -> PathBuf {
    repository.join(HOME_DIR_NAME)
}

/// Check whether a path is within the managed namespace (beneath `repository/home/`).
pub fn is_managed_path(repository: &Path, path: &Path) -> bool {
    let home_root = repository.join(HOME_DIR_NAME);
    path.starts_with(&home_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_absolute_joins_home_and_relative() {
        let home = Path::new("/home/user");
        assert_eq!(
            source_absolute(home, ".config/fish"),
            PathBuf::from("/home/user/.config/fish")
        );
    }

    #[test]
    fn source_absolute_single_file() {
        let home = Path::new("/home/user");
        assert_eq!(
            source_absolute(home, ".bashrc"),
            PathBuf::from("/home/user/.bashrc")
        );
    }

    #[test]
    fn destination_root_maps_under_repository_home() {
        let repo = Path::new("/home/user/dotfiles");
        assert_eq!(
            destination_root(repo, ".config/fish"),
            PathBuf::from("/home/user/dotfiles/home/.config/fish")
        );
    }

    #[test]
    fn destination_root_single_file() {
        let repo = Path::new("/home/user/dotfiles");
        assert_eq!(
            destination_root(repo, ".bashrc"),
            PathBuf::from("/home/user/dotfiles/home/.bashrc")
        );
    }

    #[test]
    fn destination_root_deeply_nested() {
        let repo = Path::new("/home/user/dotfiles");
        assert_eq!(
            destination_root(repo, ".local/share/nvim/site"),
            PathBuf::from("/home/user/dotfiles/home/.local/share/nvim/site")
        );
    }

    #[test]
    fn map_source_to_destination_regular_file() {
        let home = Path::new("/home/user");
        let repo = Path::new("/home/user/dotfiles");
        let source = Path::new("/home/user/.config/fish/config.fish");

        assert_eq!(
            map_source_to_destination(home, repo, source),
            Some(PathBuf::from(
                "/home/user/dotfiles/home/.config/fish/config.fish"
            ))
        );
    }

    #[test]
    fn map_source_to_destination_returns_none_if_not_under_home() {
        let home = Path::new("/home/user");
        let repo = Path::new("/home/user/dotfiles");
        let source = Path::new("/etc/passwd");

        assert_eq!(map_source_to_destination(home, repo, source), None);
    }

    #[test]
    fn map_source_to_destination_at_home_root() {
        let home = Path::new("/home/user");
        let repo = Path::new("/home/user/dotfiles");
        let source = Path::new("/home/user/.bashrc");

        assert_eq!(
            map_source_to_destination(home, repo, source),
            Some(PathBuf::from("/home/user/dotfiles/home/.bashrc"))
        );
    }

    #[test]
    fn map_destination_to_relative_recovers_path() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles/home/.config/fish/config.fish");

        assert_eq!(
            map_destination_to_relative(repo, dest),
            Some(PathBuf::from(".config/fish/config.fish"))
        );
    }

    #[test]
    fn map_destination_to_relative_returns_none_outside_managed() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles/README.md");

        assert_eq!(map_destination_to_relative(repo, dest), None);
    }

    #[test]
    fn map_destination_to_relative_root_file() {
        let repo = Path::new("/home/user/dotfiles");
        let dest = Path::new("/home/user/dotfiles/home/.bashrc");

        assert_eq!(
            map_destination_to_relative(repo, dest),
            Some(PathBuf::from(".bashrc"))
        );
    }

    #[test]
    fn managed_home_dir_returns_correct_path() {
        let repo = Path::new("/home/user/dotfiles");
        assert_eq!(
            managed_home_dir(repo),
            PathBuf::from("/home/user/dotfiles/home")
        );
    }

    #[test]
    fn is_managed_path_inside() {
        let repo = Path::new("/home/user/dotfiles");
        let path = Path::new("/home/user/dotfiles/home/.config/fish");
        assert!(is_managed_path(repo, path));
    }

    #[test]
    fn is_managed_path_outside() {
        let repo = Path::new("/home/user/dotfiles");
        let path = Path::new("/home/user/dotfiles/README.md");
        assert!(!is_managed_path(repo, path));
    }

    #[test]
    fn is_managed_path_manifest_is_not_managed() {
        let repo = Path::new("/home/user/dotfiles");
        let path = Path::new("/home/user/dotfiles/.config-sync-manifest.toml");
        assert!(!is_managed_path(repo, path));
    }

    #[test]
    fn round_trip_source_to_destination_and_back() {
        let home = Path::new("/home/user");
        let repo = Path::new("/home/user/dotfiles");
        let relative = ".config/waybar/config";

        let abs_source = source_absolute(home, relative);
        let dest = map_source_to_destination(home, repo, &abs_source).unwrap();
        let recovered = map_destination_to_relative(repo, &dest).unwrap();

        assert_eq!(recovered, PathBuf::from(relative));
    }

    #[test]
    fn mapping_is_deterministic() {
        let home = Path::new("/home/user");
        let repo = Path::new("/home/user/dotfiles");
        let source = Path::new("/home/user/.config/fish/functions/hello.fish");

        let first = map_source_to_destination(home, repo, source);
        let second = map_source_to_destination(home, repo, source);

        assert_eq!(first, second);
    }

    #[test]
    fn different_sources_produce_different_destinations() {
        let repo = Path::new("/home/user/dotfiles");

        let dest_a = destination_root(repo, ".config/fish");
        let dest_b = destination_root(repo, ".config/waybar");

        assert_ne!(dest_a, dest_b);
    }
}
