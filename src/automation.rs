//! Scheduler-neutral backup automation lifecycle.
//!
//! The backup workflow itself remains independent of this module. Automation
//! providers only arrange for the short-lived `dothoard backup` command to run
//! and expose install, removal, status, refresh, and staleness operations.

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use crate::config::AutomationBackend as Backend;
use crate::config::Config;
use crate::paths::AppPaths;
use crate::{cron, systemd};

impl Backend {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::Cron => "cron",
            Self::External => "external",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Systemd => "systemd user timer",
            Self::Cron => "user crontab",
            Self::External => "externally managed scheduler",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Systemd => Self::Cron,
            Self::Cron => Self::External,
            Self::External => Self::Systemd,
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

pub const fn selected_backend(config: &Config) -> Backend {
    config.automation_backend
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Active {
        stale: bool,
    },
    Installed {
        stale: bool,
        activity: ActivityStatus,
    },
    Failed {
        reason: String,
    },
    External {
        command: String,
    },
    NotInstalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityStatus {
    Inactive,
    NotInspected,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active { stale: false } => write!(f, "active"),
            Self::Active { stale: true } => write!(f, "active (stale configuration)"),
            Self::Installed {
                stale: false,
                activity: ActivityStatus::Inactive,
            } => write!(f, "installed but inactive"),
            Self::Installed {
                stale: true,
                activity: ActivityStatus::Inactive,
            } => write!(f, "installed but inactive (stale configuration)"),
            Self::Installed {
                stale: false,
                activity: ActivityStatus::NotInspected,
            } => write!(f, "installed (scheduler activity not inspected)"),
            Self::Installed {
                stale: true,
                activity: ActivityStatus::NotInspected,
            } => write!(
                f,
                "installed (stale configuration; scheduler activity not inspected)"
            ),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
            Self::External { command } => {
                write!(f, "externally managed; schedule `{command}`")
            }
            Self::NotInstalled => write!(f, "not installed"),
        }
    }
}

#[derive(Debug, Error)]
pub enum AutomationError {
    #[error("systemd backend failed: {0}")]
    Systemd(#[from] systemd::SystemdError),
    #[error("cron backend failed: {0}")]
    Cron(#[from] cron::CronError),
    #[error("failed to resolve the dothoard executable: {0}")]
    Executable(#[source] std::io::Error),
    #[error("{name} path is not valid UTF-8: {path}")]
    NonUtf8Path { name: &'static str, path: PathBuf },
    #[error(
        "{operation} is unavailable for externally managed automation; configure the scheduler outside dothoard and use `dothoard service print-command`"
    )]
    ExternallyManaged { operation: &'static str },
}

fn shell_quote(path: &Path, name: &'static str) -> Result<String, AutomationError> {
    let text = path.to_str().ok_or_else(|| AutomationError::NonUtf8Path {
        name,
        path: path.to_path_buf(),
    })?;
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

/// Render a copyable command for a scheduler managed outside dothoard.
pub fn external_command(paths: &AppPaths) -> Result<String, AutomationError> {
    let executable = std::env::current_exe().map_err(AutomationError::Executable)?;
    Ok(format!(
        "XDG_RUNTIME_DIR={} {} backup",
        shell_quote(paths.runtime_dir(), "runtime directory")?,
        shell_quote(&executable, "executable")?
    ))
}

fn systemd_unit_dir(paths: &AppPaths) -> std::path::PathBuf {
    let config_home = paths.config_dir().parent().unwrap_or_else(|| paths.home());
    systemd::user_unit_dir_from(config_home)
}

pub fn validate(config: &Config, paths: &AppPaths) -> Result<(), AutomationError> {
    match selected_backend(config) {
        Backend::Systemd => {
            systemd::params_from_config(config)?;
        }
        Backend::Cron => {
            let params = cron::params_from_config(config, paths.runtime_dir())?;
            cron::generate_managed_block(&params)?;
        }
        Backend::External => {
            external_command(paths)?;
        }
    }
    Ok(())
}

pub fn install(config: &Config, paths: &AppPaths) -> Result<(), AutomationError> {
    match selected_backend(config) {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            systemd::install(&params, &systemd_unit_dir(paths))?;
        }
        Backend::Cron => cron::install(&cron::params_from_config(config, paths.runtime_dir())?)?,
        Backend::External => {
            return Err(AutomationError::ExternallyManaged {
                operation: "installation",
            });
        }
    }
    Ok(())
}

pub fn remove(config: &Config, paths: &AppPaths) -> Result<(), AutomationError> {
    match selected_backend(config) {
        Backend::Systemd => systemd::remove(&systemd_unit_dir(paths))?,
        Backend::Cron => cron::remove()?,
        Backend::External => {
            return Err(AutomationError::ExternallyManaged {
                operation: "removal",
            });
        }
    }
    Ok(())
}

pub fn status(config: &Config, paths: &AppPaths) -> Result<Status, AutomationError> {
    let status = match selected_backend(config) {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            Status::from(systemd::status(&params, &systemd_unit_dir(paths))?)
        }
        Backend::Cron => Status::from(cron::status(&cron::params_from_config(
            config,
            paths.runtime_dir(),
        )?)?),
        Backend::External => Status::External {
            command: external_command(paths)?,
        },
    };
    Ok(status)
}

pub fn refresh(config: &Config, paths: &AppPaths) -> Result<(), AutomationError> {
    match selected_backend(config) {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            systemd::update_interval(&params, &systemd_unit_dir(paths))?;
        }
        Backend::Cron => cron::install(&cron::params_from_config(config, paths.runtime_dir())?)?,
        Backend::External => {
            return Err(AutomationError::ExternallyManaged {
                operation: "refresh",
            });
        }
    }
    Ok(())
}

pub fn is_installed(config: &Config, paths: &AppPaths) -> Result<bool, AutomationError> {
    let installed = match selected_backend(config) {
        Backend::Systemd => {
            let unit_dir = systemd_unit_dir(paths);
            systemd::service_unit_path(&unit_dir).is_file()
                && systemd::timer_unit_path(&unit_dir).is_file()
        }
        Backend::Cron => !matches!(
            cron::status(&cron::params_from_config(config, paths.runtime_dir())?)?,
            cron::CronStatus::NotInstalled
        ),
        Backend::External => false,
    };
    Ok(installed)
}

pub fn is_stale(config: &Config, paths: &AppPaths) -> Result<bool, AutomationError> {
    let stale = match selected_backend(config) {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            systemd::is_stale(&params, &systemd_unit_dir(paths))?
        }
        Backend::Cron => {
            match cron::status(&cron::params_from_config(config, paths.runtime_dir())?)? {
                cron::CronStatus::Installed { stale } => stale,
                cron::CronStatus::NotInstalled => false,
            }
        }
        Backend::External => false,
    };
    Ok(stale)
}

impl From<systemd::AutomationStatus> for Status {
    fn from(value: systemd::AutomationStatus) -> Self {
        match value {
            systemd::AutomationStatus::Active { stale } => Self::Active { stale },
            systemd::AutomationStatus::Installed { stale } => Self::Installed {
                stale,
                activity: ActivityStatus::Inactive,
            },
            systemd::AutomationStatus::Failed { reason } => Self::Failed { reason },
            systemd::AutomationStatus::NotInstalled => Self::NotInstalled,
        }
    }
}

impl From<cron::CronStatus> for Status {
    fn from(value: cron::CronStatus) -> Self {
        match value {
            cron::CronStatus::Installed { stale } => Self::Installed {
                stale,
                activity: ActivityStatus::NotInspected,
            },
            cron::CronStatus::NotInstalled => Self::NotInstalled,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/automation.rs"]
mod tests;
