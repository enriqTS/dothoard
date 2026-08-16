//! Scheduler-neutral backup automation lifecycle.
//!
//! The backup workflow itself remains independent of this module. Automation
//! providers only arrange for the short-lived `dothoard backup` command to run
//! and expose install, removal, status, refresh, and staleness operations.
//!
//! Systemd is the only managed provider today. Keeping provider-specific paths
//! and commands behind this facade lets callers remain neutral as additional
//! schedulers are added.

use std::path::Path;

use thiserror::Error;

use crate::config::Config;
use crate::systemd;

/// A scheduler backend managed by dothoard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Systemd,
}

impl Backend {
    /// Stable configuration-oriented backend name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
        }
    }

    /// Human-readable scheduler description.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Systemd => "systemd user timer",
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The currently selected managed backend.
///
/// Backend selection becomes configuration-driven when another managed
/// provider is introduced. Until then this preserves existing systemd behavior.
pub const fn selected_backend() -> Backend {
    Backend::Systemd
}

/// Scheduler-neutral automation status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Active { stale: bool },
    Installed { stale: bool },
    Failed { reason: String },
    NotInstalled,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active { stale: false } => write!(f, "active"),
            Self::Active { stale: true } => write!(f, "active (stale configuration)"),
            Self::Installed { stale: false } => write!(f, "installed but not running"),
            Self::Installed { stale: true } => {
                write!(f, "installed but not running (stale configuration)")
            }
            Self::Failed { reason } => write!(f, "failed: {reason}"),
            Self::NotInstalled => write!(f, "not installed"),
        }
    }
}

/// Errors from a managed automation backend.
#[derive(Debug, Error)]
pub enum AutomationError {
    #[error("systemd backend failed: {0}")]
    Systemd(#[from] systemd::SystemdError),
}

/// Install or reinstall the selected automation backend.
pub fn install(config: &Config, home: &Path) -> Result<(), AutomationError> {
    match selected_backend() {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            let unit_dir = systemd::user_unit_dir(home);
            systemd::install(&params, &unit_dir)?;
        }
    }
    Ok(())
}

/// Remove the selected automation backend.
pub fn remove(home: &Path) -> Result<(), AutomationError> {
    match selected_backend() {
        Backend::Systemd => systemd::remove(&systemd::user_unit_dir(home))?,
    }
    Ok(())
}

/// Inspect the selected automation backend.
pub fn status(config: &Config, home: &Path) -> Result<Status, AutomationError> {
    let status = match selected_backend() {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            let unit_dir = systemd::user_unit_dir(home);
            Status::from(systemd::status(&params, &unit_dir)?)
        }
    };
    Ok(status)
}

/// Refresh installed scheduler content after configuration changes.
pub fn refresh(config: &Config, home: &Path) -> Result<(), AutomationError> {
    match selected_backend() {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            let unit_dir = systemd::user_unit_dir(home);
            systemd::update_interval(&params, &unit_dir)?;
        }
    }
    Ok(())
}

/// Whether all files needed by the selected backend are installed.
pub fn is_installed(home: &Path) -> bool {
    match selected_backend() {
        Backend::Systemd => {
            let unit_dir = systemd::user_unit_dir(home);
            systemd::service_unit_path(&unit_dir).is_file()
                && systemd::timer_unit_path(&unit_dir).is_file()
        }
    }
}

/// Whether installed scheduler content differs from current configuration.
pub fn is_stale(config: &Config, home: &Path) -> Result<bool, AutomationError> {
    let stale = match selected_backend() {
        Backend::Systemd => {
            let params = systemd::params_from_config(config)?;
            systemd::is_stale(&params, &systemd::user_unit_dir(home))?
        }
    };
    Ok(stale)
}

impl From<systemd::AutomationStatus> for Status {
    fn from(value: systemd::AutomationStatus) -> Self {
        match value {
            systemd::AutomationStatus::Active { stale } => Self::Active { stale },
            systemd::AutomationStatus::Installed { stale } => Self::Installed { stale },
            systemd::AutomationStatus::Failed { reason } => Self::Failed { reason },
            systemd::AutomationStatus::NotInstalled => Self::NotInstalled,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/automation.rs"]
mod tests;
