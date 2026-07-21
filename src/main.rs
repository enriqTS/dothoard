use std::process::ExitCode;

use clap::Parser;
use config_sync::cli;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();

    if let Err(error) = cli::execute(cli) {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
