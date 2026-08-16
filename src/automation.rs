//! Scheduler-neutral backup automation lifecycle.
//!
//! The backup workflow itself remains independent of this module. Automation
//! providers only arrange for the short-lived `dothoard backup` command to run
//! and expose install, removal, status, refresh, and staleness operations.

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
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Systemd => "systemd user timer",
            Self::Cron => "user crontab",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Systemd => Self::Cron,
            Self::Cron => Self::Systemd,
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
    }
    Ok(())
}

pub fn install(config: &Config, paths: &AppPaths) -> Result<(), AutomationError> {
    match selected_backend(config) {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            systemd::install(&params, &systemd::user_unit_dir(paths.home()))?;
        }
        Backend::Cron => cron::install(&cron::params_from_config(config, paths.runtime_dir())?)?,
    }
    Ok(())
}

pub fn remove(config: &Config, paths: &AppPaths) -> Result<(), AutomationError> {
    match selected_backend(config) {
        Backend::Systemd => systemd::remove(&systemd::user_unit_dir(paths.home()))?,
        Backend::Cron => cron::remove()?,
    }
    Ok(())
}

pub fn status(config: &Config, paths: &AppPaths) -> Result<Status, AutomationError> {
    let status = match selected_backend(config) {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            Status::from(systemd::status(
                &params,
                &systemd::user_unit_dir(paths.home()),
            )?)
        }
        Backend::Cron => Status::from(cron::status(&cron::params_from_config(
            config,
            paths.runtime_dir(),
        )?)?),
    };
    Ok(status)
}

pub fn refresh(config: &Config, paths: &AppPaths) -> Result<(), AutomationError> {
    match selected_backend(config) {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            systemd::update_interval(&params, &systemd::user_unit_dir(paths.home()))?;
        }
        Backend::Cron => cron::install(&cron::params_from_config(config, paths.runtime_dir())?)?,
    }
    Ok(())
}

pub fn is_installed(config: &Config, paths: &AppPaths) -> Result<bool, AutomationError> {
    let installed = match selected_backend(config) {
        Backend::Systemd => {
            let unit_dir = systemd::user_unit_dir(paths.home());
            systemd::service_unit_path(&unit_dir).is_file()
                && systemd::timer_unit_path(&unit_dir).is_file()
        }
        Backend::Cron => !matches!(
            cron::status(&cron::params_from_config(config, paths.runtime_dir())?)?,
            cron::CronStatus::NotInstalled
        ),
    };
    Ok(installed)
}

pub fn is_stale(config: &Config, paths: &AppPaths) -> Result<bool, AutomationError> {
    let stale = match selected_backend(config) {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            systemd::is_stale(&params, &systemd::user_unit_dir(paths.home()))?
        }
        Backend::Cron => {
            match cron::status(&cron::params_from_config(config, paths.runtime_dir())?)? {
                cron::CronStatus::Installed { stale } => stale,
                cron::CronStatus::NotInstalled => false,
            }
        }
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
