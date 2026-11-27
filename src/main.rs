//! `brigid` — brig·id dev orchestrator: prerequisite checks, local HTTPS /
//! master-key setup, cross-repo git/build commands, a split-pane process
//! launcher (via `mprocs`) for local development, and the releases of the
//! servers.

mod commands;
mod env_file;
mod repos;

use clap::{Parser, Subcommand};

use commands::release::ReleaseAction;
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
    /// What a release of each server would hold; `cut` starts one.
    ///
    /// Without a subcommand: for seedling and grove, what was merged since
    /// their last release (in the server, crypto, the core and the
    /// interface), and whether a person running the server would notice.
    Release {
        #[command(subcommand)]
        action: Option<ReleaseAction>,
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
        Command::Release { action } => commands::release::run(action),
    }
}
