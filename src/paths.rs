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
    /// Application configuration directory (e.g. `~/.config/config-sync/`).
    config_dir: PathBuf,
    /// Application configuration file path.
    config_file: PathBuf,
    /// Application state directory (e.g. `~/.local/state/config-sync/`).
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
/// Default: `$XDG_CONFIG_HOME/config-sync/` or `~/.config/config-sync/`.
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
/// Default: `$XDG_STATE_HOME/config-sync/` or `~/.local/state/config-sync/`.
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

#[cfg(test)]
mod tests {
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
}
