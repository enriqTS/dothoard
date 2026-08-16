//! Command-line parsing and dispatch.

use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use thiserror::Error;

use crate::app::BINARY_NAME;
use crate::automation;
use crate::backup::coordinator::{self, BackupOutcome, CoordinatorError};
use crate::config::Config;
use crate::paths::AppPaths;

/// Exit codes for the backup command.
pub mod exit_code {
    use std::process::ExitCode;

    /// Backup completed successfully (commit created and pushed, or no changes).
    pub const SUCCESS: ExitCode = ExitCode::SUCCESS;
    /// Backup failed.
    pub const FAILURE: ExitCode = ExitCode::FAILURE;
    /// Another backup is already running.
    pub fn already_running() -> ExitCode {
        ExitCode::from(2)
    }
    /// Configuration is invalid or missing.
    pub fn config_error() -> ExitCode {
        ExitCode::from(3)
    }
}

#[derive(Debug, Parser)]
#[command(name = BINARY_NAME, version, about = "Back up selected home-directory configuration to Git")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run one backup immediately.
    Backup,
    /// Validate configuration and repository state.
    Check,
    /// Manage background backup automation.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    /// Select the automation backend.
    Select {
        #[arg(value_enum)]
        backend: BackendArg,
    },
    /// Install and enable managed backup automation.
    Install,
    /// Disable and remove managed backup automation.
    Remove,
    /// Show automation status.
    Status,
    /// Print the command to schedule with an external automation provider.
    PrintCommand,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BackendArg {
    Systemd,
    Cron,
    External,
}

impl From<BackendArg> for crate::config::AutomationBackend {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::Systemd => Self::Systemd,
            BackendArg::Cron => Self::Cron,
            BackendArg::External => Self::External,
        }
    }
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{operation} is not implemented yet")]
    NotImplemented { operation: &'static str },

    #[error(transparent)]
    Backup(#[from] CoordinatorError),

    #[error("path resolution failed: {0}")]
    Paths(#[from] crate::paths::PathError),

    #[error("automation operation failed: {0}")]
    Automation(#[from] automation::AutomationError),

    #[error("configuration error: {0}")]
    Config(#[from] Box<crate::config::ConfigError>),

    #[error("configuration is invalid: {0}")]
    Validation(String),

    #[error("remove the installed {backend} automation before selecting another backend")]
    BackendInUse { backend: automation::Backend },

    #[error("TUI error: {0}")]
    Tui(String),
}

impl CliError {
    /// Map the error to an appropriate exit code.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::NotImplemented { .. } => exit_code::FAILURE,
            Self::Backup(CoordinatorError::Lock(crate::locking::LockError::AlreadyRunning {
                ..
            })) => exit_code::already_running(),
            Self::Backup(CoordinatorError::Config(_))
            | Self::Backup(CoordinatorError::Validation(_)) => exit_code::config_error(),
            Self::Backup(_) => exit_code::FAILURE,
            Self::Paths(_) => exit_code::config_error(),
            Self::Automation(_) => exit_code::FAILURE,
            Self::Config(_) | Self::Validation(_) => exit_code::config_error(),
            Self::BackendInUse { .. } => exit_code::FAILURE,
            Self::Tui(_) => exit_code::FAILURE,
        }
    }
}

/// Execute the parsed CLI command.
///
/// Returns `Ok(ExitCode)` on success (including "no changes" which is still
/// a successful run), or `Err(CliError)` for failures.
pub fn execute(cli: Cli) -> Result<ExitCode, CliError> {
    match cli.command {
        None => execute_tui(),
        Some(Command::Backup) => execute_backup(),
        Some(Command::Check) => execute_check(),
        Some(Command::Service { command }) => match command {
            ServiceCommand::Select { backend } => execute_service_select(backend),
            ServiceCommand::Install => execute_service_install(),
            ServiceCommand::Remove => execute_service_remove(),
            ServiceCommand::Status => execute_service_status(),
            ServiceCommand::PrintCommand => execute_service_print_command(),
        },
    }
}

/// Launch the interactive TUI.
fn execute_tui() -> Result<ExitCode, CliError> {
    crate::tui::run().map_err(|e| CliError::Tui(e.to_string()))?;
    Ok(exit_code::SUCCESS)
}

/// Execute the `backup` command.
fn execute_backup() -> Result<ExitCode, CliError> {
    let paths = AppPaths::from_environment()?;

    // Initialize per-run logging for CLI mode.
    let started_at = chrono::Utc::now();
    let _log_guard = crate::diagnostics::init_for_run(paths.state_dir(), &started_at)
        .map_err(|e| CliError::Tui(format!("failed to initialize logging: {e}")))?;

    let outcome = coordinator::run_backup_at(&paths, started_at)?;
    report_outcome(&outcome);

    if outcome.success {
        Ok(exit_code::SUCCESS)
    } else {
        Ok(exit_code::FAILURE)
    }
}

/// Execute the `check` command.
fn execute_check() -> Result<ExitCode, CliError> {
    use crate::backup::check;

    let paths = AppPaths::from_environment()?;

    let report = check::run_check(&paths);
    check::print_report(&report);

    if report.is_healthy() {
        Ok(exit_code::SUCCESS)
    } else {
        Ok(exit_code::FAILURE)
    }
}

/// Execute the `service select` command.
fn execute_service_select(backend: BackendArg) -> Result<ExitCode, CliError> {
    let paths = AppPaths::from_environment()?;
    let mut config = load_and_validate_config(&paths)?;
    let current = automation::selected_backend(&config);
    let selected = backend.into();

    if current == selected {
        return Ok(exit_code::SUCCESS);
    }
    if automation::is_installed(&config, &paths)? {
        return Err(CliError::BackendInUse { backend: current });
    }

    config.automation_backend = selected;
    automation::validate(&config, &paths)?;
    config
        .save(paths.config_file())
        .map_err(|error| CliError::Config(Box::new(error)))?;
    tracing::info!(backend = %selected, "automation backend selected");
    Ok(exit_code::SUCCESS)
}

/// Execute the `service install` command.
fn execute_service_install() -> Result<ExitCode, CliError> {
    let paths = AppPaths::from_environment()?;
    let config = load_and_validate_config(&paths)?;
    automation::install(&config, &paths)?;

    tracing::info!(
        backend = %automation::selected_backend(&config),
        interval_minutes = config.interval_minutes,
        "automation installed and started"
    );

    Ok(exit_code::SUCCESS)
}

/// Execute the `service remove` command.
fn execute_service_remove() -> Result<ExitCode, CliError> {
    let paths = AppPaths::from_environment()?;
    let config = load_and_validate_config(&paths)?;
    automation::remove(&config, &paths)?;

    tracing::info!(backend = %automation::selected_backend(&config), "automation removed");

    Ok(exit_code::SUCCESS)
}

/// Execute the `service status` command.
fn execute_service_status() -> Result<ExitCode, CliError> {
    let paths = AppPaths::from_environment()?;
    let config = load_and_validate_config(&paths)?;
    let automation_status = automation::status(&config, &paths)?;

    tracing::info!(status = %automation_status, "automation status");

    match automation_status {
        automation::Status::Active { .. } => Ok(exit_code::SUCCESS),
        automation::Status::Installed { .. } | automation::Status::External { .. } => {
            Ok(exit_code::SUCCESS)
        }
        automation::Status::Failed { .. } => Ok(exit_code::FAILURE),
        automation::Status::NotInstalled => Ok(exit_code::FAILURE),
    }
}

/// Print the direct backup invocation for an externally managed scheduler.
fn execute_service_print_command() -> Result<ExitCode, CliError> {
    let paths = AppPaths::from_environment()?;
    load_and_validate_config(&paths)?;
    println!("{}", automation::external_command(&paths)?);
    Ok(exit_code::SUCCESS)
}

/// Load and validate configuration, returning a CLI-friendly error.
fn load_and_validate_config(paths: &AppPaths) -> Result<Config, CliError> {
    let config = Config::load(paths.config_file()).map_err(|e| CliError::Config(Box::new(e)))?;
    let errors = config.validate();
    if !errors.is_empty() {
        let messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        return Err(CliError::Validation(messages.join("; ")));
    }
    Ok(config)
}

/// Print a human-readable summary of the backup outcome.
fn report_outcome(outcome: &BackupOutcome) {
    if outcome.success {
        if let Some(ref sha) = outcome.commit {
            let push_status = if outcome.pushed {
                "pushed"
            } else {
                "pending push"
            };
            tracing::info!(
                namespace = %outcome.namespace,
                commit = %sha,
                copies = outcome.copies,
                deletions = outcome.deletions,
                push = push_status,
                "backup complete"
            );
        } else {
            tracing::info!(namespace = %outcome.namespace, "backup complete: no changes");
        }
    } else if let Some(ref error) = outcome.error {
        tracing::error!(namespace = %outcome.namespace, error = %error, "backup failed");
    }

    for warning in &outcome.warnings {
        tracing::warn!(warning = %warning);
    }
}

#[cfg(test)]
#[path = "../tests/unit/cli.rs"]
mod tests;
