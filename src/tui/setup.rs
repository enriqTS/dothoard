//! First-run configuration flow state.

use std::path::PathBuf;

use super::task::LoadState;

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
