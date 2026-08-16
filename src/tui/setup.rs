//! First-run configuration flow state.

use std::path::{Path, PathBuf};

use crate::config::AutomationBackend;

use super::task::LoadState;
use super::theme::ThemeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Repository,
    Namespace,
    Automation,
    Theme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryMethod {
    Existing,
    Clone,
}

impl RepositoryMethod {
    pub fn toggle(self) -> Self {
        match self {
            Self::Existing => Self::Clone,
            Self::Clone => Self::Existing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositorySetupMode {
    Choose,
    Existing,
    Clone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneField {
    Url,
    Destination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationField {
    Backend,
    Interval,
}

#[derive(Debug)]
pub struct SetupState {
    pub step: SetupStep,
    pub repository_method: RepositoryMethod,
    pub repository_mode: RepositorySetupMode,
    pub clone_field: CloneField,
    pub clone_url: String,
    pub clone_url_cursor: usize,
    pub clone_destination: String,
    pub clone_destination_cursor: usize,
    pub clone_state: LoadState<PathBuf>,
    pub automation_field: AutomationField,
    pub automation_backend: AutomationBackend,
    pub interval_input: String,
    pub interval_cursor: usize,
    pub automation_error: Option<String>,
    pub theme_selected: ThemeId,
    pub theme_previous: ThemeId,
    pub theme_error: Option<String>,
}

pub fn is_incomplete(config_dir: &Path) -> bool {
    config_dir
        .join(crate::app::SETUP_MARKER_FILE_NAME)
        .is_file()
}

pub fn mark_incomplete(config_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    std::fs::write(
        config_dir.join(crate::app::SETUP_MARKER_FILE_NAME),
        "Initial configuration is incomplete.\n",
    )
}

pub fn clear_incomplete(config_dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(config_dir.join(crate::app::SETUP_MARKER_FILE_NAME)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

impl Default for SetupState {
    fn default() -> Self {
        Self::new()
    }
}

impl SetupState {
    pub fn new() -> Self {
        Self {
            step: SetupStep::Repository,
            repository_method: RepositoryMethod::Existing,
            repository_mode: RepositorySetupMode::Choose,
            clone_field: CloneField::Url,
            clone_url: String::new(),
            clone_url_cursor: 0,
            clone_destination: String::new(),
            clone_destination_cursor: 0,
            clone_state: LoadState::NotLoaded,
            automation_field: AutomationField::Backend,
            automation_backend: AutomationBackend::Systemd,
            interval_input: "5".to_string(),
            interval_cursor: 1,
            automation_error: None,
            theme_selected: ThemeId::default(),
            theme_previous: ThemeId::default(),
            theme_error: None,
        }
    }

    pub fn resume(config: &crate::config::Config, theme: ThemeId) -> Self {
        let mut setup = Self::new();
        setup.step = SetupStep::Automation;
        setup.automation_backend = config.automation_backend;
        setup.interval_input = config.interval_minutes.to_string();
        setup.interval_cursor = setup.interval_input.len();
        setup.theme_selected = theme;
        setup.theme_previous = theme;
        setup
    }

    pub fn next_backend(&mut self) {
        self.automation_backend = match self.automation_backend {
            AutomationBackend::Systemd => AutomationBackend::Cron,
            AutomationBackend::Cron => AutomationBackend::External,
            AutomationBackend::External => AutomationBackend::Systemd,
        };
        self.automation_error = None;
    }

    pub fn previous_backend(&mut self) {
        self.automation_backend = match self.automation_backend {
            AutomationBackend::Systemd => AutomationBackend::External,
            AutomationBackend::Cron => AutomationBackend::Systemd,
            AutomationBackend::External => AutomationBackend::Cron,
        };
        self.automation_error = None;
    }

    pub fn backend_label(backend: AutomationBackend) -> &'static str {
        match backend {
            AutomationBackend::Systemd => "systemd",
            AutomationBackend::Cron => "cron",
            AutomationBackend::External => "external",
        }
    }

    pub fn clone_error(&self) -> Option<&str> {
        self.clone_state.error()
    }

    pub fn cloning(&self) -> bool {
        self.clone_state.is_loading()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/tui/setup.rs"]
mod tests;
