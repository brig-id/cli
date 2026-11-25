//! `brigid` — brig·id dev orchestrator: prerequisite checks, local HTTPS /
//! master-key setup, cross-repo git/build commands, a split-pane process
//! launcher (via `mprocs`) for local development, and the release train.

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
    /// Cross-repo git/build commands.
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
    /// Show the release train: the pull requests to merge, in order, with
    /// their checks, and the updates Renovate has yet to propose.
    Release {
        /// Refresh every minute until stopped.
        #[arg(long)]
        watch: bool,
    },
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
        Command::Release { watch } => commands::release::run(watch),
    }
}
