use std::process::ExitCode;

use clap::Parser;
use config_sync::{app, cli, diagnostics};

fn main() -> ExitCode {
    if let Err(error) = diagnostics::init() {
        eprintln!("error: failed to initialize diagnostics: {error}");
        return ExitCode::FAILURE;
    }

    match run() {
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

fn run() -> Result<ExitCode, cli::CliError> {
    app::trace_identifiers();

    let cli = cli::Cli::parse();
    tracing::debug!(command = ?cli.command, "parsed command");
    cli::execute(cli)
}
