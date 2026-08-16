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
        vec![BINARY_NAME, "service", "select", "cron"],
        vec![BINARY_NAME, "service", "select", "external"],
        vec![BINARY_NAME, "service", "print-command"],
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
    assert_eq!(
        service_command_names,
        ["select", "install", "remove", "status", "print-command"]
    );
}

#[test]
fn rejects_unknown_automation_backend() {
    assert!(Cli::try_parse_from([BINARY_NAME, "service", "select", "launchd"]).is_err());
}

#[test]
fn tui_error_exit_code() {
    let err = CliError::Tui("test error".to_string());
    assert_eq!(err.exit_code(), ExitCode::FAILURE);
}

#[test]
fn lock_already_running_exit_code() {
    let err = CliError::Backup(CoordinatorError::Lock(
        crate::locking::LockError::AlreadyRunning {
            path: std::path::PathBuf::from("/run/user/1000/dothoard.lock"),
        },
    ));

    // exit_code 2 for already running.
    assert_eq!(err.exit_code(), ExitCode::from(2));
}

#[test]
fn config_error_exit_code() {
    let err = CliError::Backup(CoordinatorError::Validation("empty repository".to_string()));

    assert_eq!(err.exit_code(), ExitCode::from(3));
}
