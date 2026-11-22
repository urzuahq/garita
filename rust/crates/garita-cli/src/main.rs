//! The `garita` command line.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "garita", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the gate
    Run,
    /// Check the local setup
    Doctor,
    /// Manage git hooks
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
}

#[derive(Subcommand)]
enum HooksAction {
    /// Install the git hooks
    Install,
}

fn main() -> ExitCode {
    let name = match Cli::parse().command {
        Command::Run => "run",
        Command::Doctor => "doctor",
        Command::Hooks {
            action: HooksAction::Install,
        } => "hooks install",
    };
    eprintln!("garita: {name} is not implemented");
    ExitCode::FAILURE
}
