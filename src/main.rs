use std::process::ExitCode;

use clap::Parser;
use dothoard::{app, cli, diagnostics};

fn main() -> ExitCode {
    // Parse CLI first to determine if we're in TUI mode
    let cli = cli::Cli::parse();

    // Initialize diagnostics based on mode.
    // - TUI mode: the TUI handles its own log initialization.
    // - Backup command: uses per-run log file (initialized in execute_backup).
    // - Other CLI commands: use stderr.
    let init_result = match &cli.command {
        None => Ok(()),                       // TUI handles it
        Some(cli::Command::Backup) => Ok(()), // Per-run log handles it
        Some(_) => diagnostics::init(),       // stderr for check/service
    };

    if let Err(error) = init_result {
        eprintln!("error: failed to initialize diagnostics: {error}");
        return ExitCode::FAILURE;
    }

    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            let exit_code = error.exit_code();
            let rendered = format!("{error:#}");
            let redacted = diagnostics::redact_sensitive_text(&rendered);
            tracing::error!(error = %redacted, "command failed");
            exit_code
        }
    }
}

fn run(cli: cli::Cli) -> Result<ExitCode, cli::CliError> {
    app::trace_identifiers();

    tracing::debug!(command = ?cli.command, "parsed command");
    cli::execute(cli)
}
