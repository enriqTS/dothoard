//! Home, XDG, source, and repository path handling.
//!
//! All path resolution is performed through [`AppPaths`], which accepts
//! injectable directory roots so that tests never touch the real home
//! directory or XDG locations.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::app;

/// Resolved application directory layout.
///
/// Every field is an absolute, validated path. The struct is cheap to clone
/// and safe to share across backend layers.
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// The user's home directory.
    home: PathBuf,
    /// Application configuration directory (e.g. `~/.config/dothoard/`).
    config_dir: PathBuf,
    /// Application configuration file path.
    config_file: PathBuf,
    /// Application state directory (e.g. `~/.local/state/dothoard/`).
    state_dir: PathBuf,
    /// Runtime directory for the exclusive lock (e.g. `$XDG_RUNTIME_DIR`).
    runtime_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum PathError {
    #[error("home directory is not set or could not be determined")]
    HomeNotFound,

    #[error("{name} directory is not absolute: {path}")]
    NotAbsolute { name: &'static str, path: PathBuf },

    #[error("{name} directory does not exist: {path}")]
    NotFound { name: &'static str, path: PathBuf },

    #[error("XDG base directories could not be determined")]
    XdgResolutionFailed,
}

/// Inputs for path resolution that can be injected by tests.
///
/// When `None`, the corresponding value is resolved from the environment
/// using the `directories` crate or standard environment variables.
///
/// Set `use_environment` to `false` in tests to prevent reading real XDG
/// environment variables. When disabled, unset directories are derived
/// purely from the provided `home`.
#[derive(Debug)]
pub struct PathInputs {
    pub home: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub runtime_dir: Option<PathBuf>,
    /// Whether to consult real environment variables for XDG fallbacks.
    /// Defaults to `true` for production use.
    pub use_environment: bool,
}

impl Default for PathInputs {
    fn default() -> Self {
        Self {
            home: None,
            config_dir: None,
            state_dir: None,
            runtime_dir: None,
            use_environment: true,
        }
    }
}

impl AppPaths {
    /// Resolve all application paths from the given inputs.
    ///
    /// Falls back to XDG and environment detection for any input that is
    /// `None`. Validates that each resolved path is absolute and exists.
    pub fn resolve(inputs: PathInputs) -> Result<Self, PathError> {
        let use_env = inputs.use_environment;
        let home = resolve_home(inputs.home, use_env)?;
        let config_dir = resolve_config_dir(inputs.config_dir, &home, use_env)?;
        let config_file = config_dir.join(app::CONFIG_FILE_NAME);
        let state_dir = resolve_state_dir(inputs.state_dir, &home, use_env)?;
        let runtime_dir = resolve_runtime_dir(inputs.runtime_dir, use_env)?;

        Ok(Self {
            home,
            config_dir,
            config_file,
            state_dir,
            runtime_dir,
        })
    }

    /// Resolve paths from the real environment.
    pub fn from_environment() -> Result<Self, PathError> {
        Self::resolve(PathInputs::default())
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }
}

/// Resolve the home directory, preferring the injected value.
fn resolve_home(injected: Option<PathBuf>, use_env: bool) -> Result<PathBuf, PathError> {
    let home = match injected {
        Some(path) => path,
        None => {
            if !use_env {
                return Err(PathError::HomeNotFound);
            }
            let base_dirs = directories::BaseDirs::new().ok_or(PathError::HomeNotFound)?;
            base_dirs.home_dir().to_path_buf()
        }
    };

    validate_directory(&home, "home")?;
    Ok(home)
}

/// Resolve the configuration directory.
///
/// Default: `$XDG_CONFIG_HOME/dothoard/` or `~/.config/dothoard/`.
fn resolve_config_dir(
    injected: Option<PathBuf>,
    home: &Path,
    use_env: bool,
) -> Result<PathBuf, PathError> {
    let dir = match injected {
        Some(path) => path,
        None => {
            let xdg_config = if use_env {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
            } else {
                None
            };
            let base = xdg_config.unwrap_or_else(|| home.join(".config"));
            base.join(app::CONFIG_DIR_NAME)
        }
    };

    validate_absolute(&dir, "config")?;
    Ok(dir)
}

/// Resolve the state directory.
///
/// Default: `$XDG_STATE_HOME/dothoard/` or `~/.local/state/dothoard/`.
fn resolve_state_dir(
    injected: Option<PathBuf>,
    home: &Path,
    use_env: bool,
) -> Result<PathBuf, PathError> {
    let dir = match injected {
        Some(path) => path,
        None => {
            let xdg_state = if use_env {
                std::env::var_os("XDG_STATE_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
            } else {
                None
            };
            let base = xdg_state.unwrap_or_else(|| home.join(".local").join("state"));
            base.join(app::STATE_DIR_NAME)
        }
    };

    validate_absolute(&dir, "state")?;
    Ok(dir)
}

/// Resolve the runtime directory for the exclusive lock.
///
/// Default: `$XDG_RUNTIME_DIR`. Falls back to `/tmp` if unset.
fn resolve_runtime_dir(injected: Option<PathBuf>, use_env: bool) -> Result<PathBuf, PathError> {
    let dir = match injected {
        Some(path) => path,
        None => {
            if use_env {
                std::env::var_os("XDG_RUNTIME_DIR")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
            } else {
                return Err(PathError::NotFound {
                    name: "runtime",
                    path: PathBuf::from("<not provided>"),
                });
            }
        }
    };

    validate_directory(&dir, "runtime")?;
    Ok(dir)
}

/// Validate that a path is absolute.
fn validate_absolute(path: &Path, name: &'static str) -> Result<(), PathError> {
    if !path.is_absolute() {
        return Err(PathError::NotAbsolute {
            name,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// Validate that a path is absolute and exists as a directory.
fn validate_directory(path: &Path, name: &'static str) -> Result<(), PathError> {
    validate_absolute(path, name)?;
    if !path.is_dir() {
        return Err(PathError::NotFound {
            name,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// Errors from source path filesystem validation.
#[derive(Debug, Error)]
pub enum SourcePathError {
    /// A parent component between `$HOME` and the source root is a symlink.
    #[error("symlink in parent path at {symlink_at}: source \"{relative_source}\" rejected")]
    SymlinkedParent {
        relative_source: String,
        symlink_at: PathBuf,
    },

    /// The source root does not exist.
    #[error("source root does not exist: {path}")]
    SourceNotFound { path: PathBuf },

    /// An I/O error occurred while checking a path component.
    #[error("failed to inspect path component {path}")]
    Inspect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Validate a source path on the filesystem.
///
/// Checks that no parent component between `home` and the source root is a
/// symlink. The source root itself is permitted to be a symlink (it will be
/// backed up as a link without being followed).
///
/// The `relative_source` must be a home-relative path that has already passed
/// string-level validation (not empty, not absolute, no `..`).
///
/// Returns the absolute path to the source root on success.
pub fn validate_source_path(
    home: &Path,
    relative_source: &str,
) -> Result<PathBuf, SourcePathError> {
    let full_path = home.join(relative_source);

    // Check that the source root exists (as any file type including symlink).
    // Use symlink_metadata to detect existence without following links.
    let source_exists = std::fs::symlink_metadata(&full_path).is_ok();
    if !source_exists {
        return Err(SourcePathError::SourceNotFound {
            path: full_path.clone(),
        });
    }

    // Walk each intermediate component between home and the source root.
    // We check every prefix of the relative path EXCEPT the final component
    // (the source root itself, which is allowed to be a symlink).
    let rel_path = Path::new(relative_source);
    let components: Vec<_> = rel_path.components().collect();

    // Check all parent prefixes (all but the last component).
    if components.len() > 1 {
        let mut current = home.to_path_buf();
        for component in &components[..components.len() - 1] {
            current.push(component);

            let metadata =
                std::fs::symlink_metadata(&current).map_err(|source| SourcePathError::Inspect {
                    path: current.clone(),
                    source,
                })?;

            if metadata.file_type().is_symlink() {
                return Err(SourcePathError::SymlinkedParent {
                    relative_source: relative_source.to_string(),
                    symlink_at: current,
                });
            }
        }
    }

    Ok(full_path)
}

/// Result of an overlap or containment check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlapError {
    /// Two sources overlap: one is an ancestor of or equal to the other.
    SourceOverlap {
        /// Index of the first source in the configuration.
        first: usize,
        /// Index of the second source in the configuration.
        second: usize,
        /// The path of the ancestor source.
        ancestor: String,
        /// The path of the descendant source.
        descendant: String,
    },

    /// A source contains the repository or the repository contains a source.
    RepositoryContainment {
        /// Index of the source in the configuration.
        source_index: usize,
        /// The source path.
        source_path: String,
        /// Human-readable description of the containment direction.
        description: String,
    },
}

impl std::fmt::Display for OverlapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceOverlap {
                first,
                second,
                ancestor,
                descendant,
            } => {
                write!(
                    f,
                    "sources [{first}] \"{ancestor}\" and [{second}] \"{descendant}\" overlap"
                )
            }
            Self::RepositoryContainment {
                source_index,
                source_path,
                description,
            } => {
                write!(
                    f,
                    "source [{source_index}] \"{source_path}\": {description}"
                )
            }
        }
    }
}

/// Check whether `ancestor` is a prefix of or equal to `descendant`.
///
/// Uses component-wise comparison to avoid false matches on partial directory
/// names (e.g. `/home/user/.config` is not a prefix of `/home/user/.config2`).
fn is_path_prefix_or_equal(ancestor: &Path, descendant: &Path) -> bool {
    let mut ancestor_components = ancestor.components();
    let mut descendant_components = descendant.components();

    loop {
        match (ancestor_components.next(), descendant_components.next()) {
            (Some(a), Some(d)) => {
                if a != d {
                    return false;
                }
            }
            // Ancestor exhausted first or both exhausted together — prefix or equal.
            (None, _) => return true,
            // Descendant exhausted first — ancestor is longer.
            (Some(_), None) => return false,
        }
    }
}

/// Detect overlapping sources and source-repository containment.
///
/// `source_paths` are the absolute resolved paths for each source (in the
/// same order as the configuration). `repository` is the absolute path to the
/// Git repository.
///
/// Returns all detected problems.
pub fn check_overlaps(source_paths: &[PathBuf], repository: &Path) -> Vec<OverlapError> {
    let mut errors = Vec::new();

    // Check pairwise source overlap.
    for i in 0..source_paths.len() {
        for j in (i + 1)..source_paths.len() {
            let a = &source_paths[i];
            let b = &source_paths[j];

            if is_path_prefix_or_equal(a, b) {
                errors.push(OverlapError::SourceOverlap {
                    first: i,
                    second: j,
                    ancestor: a.to_string_lossy().into_owned(),
                    descendant: b.to_string_lossy().into_owned(),
                });
            } else if is_path_prefix_or_equal(b, a) {
                errors.push(OverlapError::SourceOverlap {
                    first: j,
                    second: i,
                    ancestor: b.to_string_lossy().into_owned(),
                    descendant: a.to_string_lossy().into_owned(),
                });
            }
        }
    }

    // Check source-repository containment.
    for (index, source) in source_paths.iter().enumerate() {
        if is_path_prefix_or_equal(source, repository) {
            errors.push(OverlapError::RepositoryContainment {
                source_index: index,
                source_path: source.to_string_lossy().into_owned(),
                description: "source contains the repository (recursive backup)".to_string(),
            });
        } else if is_path_prefix_or_equal(repository, source) {
            errors.push(OverlapError::RepositoryContainment {
                source_index: index,
                source_path: source.to_string_lossy().into_owned(),
                description: "repository contains the source".to_string(),
            });
        }
    }

    errors
}

#[cfg(test)]
#[path = "../tests/unit/paths.rs"]
mod tests;
