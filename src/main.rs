use std::process::ExitCode;

use clap::Parser;
use dothoard::{app, cli, diagnostics};

fn main() -> ExitCode {
    // Parse CLI first to determine if we're in TUI mode
    let cli = cli::Cli::parse();

    // Initialize diagnostics based on mode
    let init_result = if cli.command.is_none() {
        // TUI mode: don't initialize here - the TUI will handle it
        Ok(())
    } else {
        // CLI mode: initialize with stderr
        diagnostics::init()
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
