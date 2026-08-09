//! `brigid` — brig·id dev orchestrator: prerequisite checks, local HTTPS /
//! master-key setup, cross-repo git/build commands, and a split-pane process
//! launcher (via `mprocs`) for local development.

mod commands;
mod env_file;
mod repos;

use clap::{Parser, Subcommand};

use commands::repos::ReposAction;

#[derive(Parser)]
#[command(name = "brigid", about = "brig·id dev orchestrator", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Cross-repo git/build commands (replaces the old scripts/*.mjs).
    Repos {
        #[command(subcommand)]
        action: ReposAction,
    },
    /// Verify local-dev prerequisites are in place.
    Check,
    /// Fix what `check` finds missing (mkcert CA, dev cert, MASTER_KEY).
    Setup,
    /// Interactively launch dev processes side by side.
    Dev,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Repos { action } => commands::repos::run(action),
        Command::Check => {
            if commands::check::run() {
                Ok(())
            } else {
                anyhow::bail!("one or more checks failed — run `brigid setup`")
            }
        }
        Command::Setup => commands::setup::run(),
        Command::Dev => commands::dev::run(),
    }
}
