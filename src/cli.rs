//! Command-line parsing and dispatch.

use clap::{Parser, Subcommand};
use thiserror::Error;

use crate::app::BINARY_NAME;

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
    /// Install and enable the user timer.
    Install,
    /// Disable and remove the user timer.
    Remove,
    /// Show automation status.
    Status,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{operation} is not implemented yet")]
    NotImplemented { operation: &'static str },
}

pub fn execute(cli: Cli) -> Result<(), CliError> {
    let operation = match cli.command {
        None => "the TUI",
        Some(Command::Backup) => "backup",
        Some(Command::Check) => "check",
        Some(Command::Service { command }) => match command {
            ServiceCommand::Install => "service install",
            ServiceCommand::Remove => "service remove",
            ServiceCommand::Status => "service status",
        },
    };

    Err(CliError::NotImplemented { operation })
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::*;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_every_planned_command() {
        for arguments in [
            vec![BINARY_NAME],
            vec![BINARY_NAME, "backup"],
            vec![BINARY_NAME, "check"],
            vec![BINARY_NAME, "service", "install"],
            vec![BINARY_NAME, "service", "remove"],
            vec![BINARY_NAME, "service", "status"],
        ] {
            assert!(Cli::try_parse_from(arguments).is_ok());
        }
    }

    #[test]
    fn exposes_the_planned_command_hierarchy() {
        let command = Cli::command();
        let command_names = command
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        let service = command
            .find_subcommand("service")
            .expect("service command should exist");
        let service_command_names = service
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();

        assert_eq!(command_names, ["backup", "check", "service"]);
        assert_eq!(service_command_names, ["install", "remove", "status"]);
    }

    #[test]
    fn reports_every_unimplemented_operation_clearly() {
        let cases = [
            (Cli { command: None }, "the TUI is not implemented yet"),
            (
                Cli {
                    command: Some(Command::Backup),
                },
                "backup is not implemented yet",
            ),
            (
                Cli {
                    command: Some(Command::Check),
                },
                "check is not implemented yet",
            ),
            (
                Cli {
                    command: Some(Command::Service {
                        command: ServiceCommand::Install,
                    }),
                },
                "service install is not implemented yet",
            ),
            (
                Cli {
                    command: Some(Command::Service {
                        command: ServiceCommand::Remove,
                    }),
                },
                "service remove is not implemented yet",
            ),
            (
                Cli {
                    command: Some(Command::Service {
                        command: ServiceCommand::Status,
                    }),
                },
                "service status is not implemented yet",
            ),
        ];

        for (cli, expected) in cases {
            let error = execute(cli).expect_err("bootstrap command must remain a stub");

            assert_eq!(error.to_string(), expected);
        }
    }
}
