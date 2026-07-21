use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use config_sync::{app, cli, diagnostics};

fn main() -> ExitCode {
    if let Err(error) = diagnostics::init() {
        eprintln!("error: failed to initialize diagnostics: {error}");
        return ExitCode::FAILURE;
    }

    if let Err(error) = run() {
        let rendered = format!("{error:#}");
        let redacted = diagnostics::redact_sensitive_text(&rendered);
        tracing::error!(error = %redacted, "command failed");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    app::trace_identifiers();

    let cli = cli::Cli::parse();
    tracing::debug!(command = ?cli.command, "parsed command");
    cli::execute(cli).context("unable to execute command")
}
