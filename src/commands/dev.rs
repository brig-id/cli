//! `brigid dev` — interactively pick what to run, then hand off to `mprocs`
//! for the split-pane display.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use inquire::{Confirm, Select};
use serde::Serialize;

use crate::{
    commands::{check, setup},
    env_file,
    repos::{cli_root, workspace_root},
};

pub fn run() -> Result<()> {
    if !check::run() {
        println!();
        if Confirm::new("Some prerequisites are missing — run `brigid setup` now?")
            .with_default(true)
            .prompt()?
        {
            setup::run()?;
        } else {
            bail!("aborted — prerequisites not met");
        }
    }

    let servers = available_servers();
    if servers.is_empty() {
        bail!(
            "no runnable server found (server-leaf/server-grove/server-forest all missing a Cargo.toml)"
        );
    }
    let server = Select::new("Which server?", servers).prompt()?;

    let include_app = Confirm::new("Include app (Qwik UI)?")
        .with_default(true)
        .prompt()?;

    let master_key = env_file::read_master_key(&env_file::path())
        .context("BRIGID_MASTER_KEY missing after setup — this shouldn't happen")?;

    let mut procs = BTreeMap::new();
    procs.insert(server.clone(), server_proc(&server, &master_key));
    if include_app {
        procs.insert("app".to_string(), app_proc());
    }

    let config = MprocsConfig { procs };
    let config_path = cli_root().join("mprocs.yaml");
    std::fs::write(&config_path, serde_yaml::to_string(&config)?)
        .with_context(|| format!("writing {}", config_path.display()))?;

    println!("\n→ launching mprocs ({})...\n", config_path.display());
    let status = std::process::Command::new("mprocs")
        .arg("--config")
        .arg(&config_path)
        .status()
        .context("failed to spawn mprocs — is it on PATH? (`brigid check`)")?;

    std::process::exit(status.code().unwrap_or(1));
}

/// Server names that have an actual `Cargo.toml` today (only `server-leaf`,
/// until `server-grove`/`server-forest` grow past their empty-placeholder state).
fn available_servers() -> Vec<String> {
    ["server-leaf", "server-grove", "server-forest"]
        .into_iter()
        .filter(|name| workspace_root().join(name).join("Cargo.toml").is_file())
        .map(str::to_string)
        .collect()
}

fn server_proc(name: &str, master_key: &str) -> MprocsProc {
    let mut env = BTreeMap::new();
    env.insert("BRIGID_MASTER_KEY".to_string(), master_key.to_string());

    // `LEAF_*` config keys are specific to `server-leaf`'s figment setup (see
    // `server-leaf/AGENTS.md`'s "Local dev without Docker"). `server-grove`/
    // `server-forest` don't exist yet — when they do, they'll need their own
    // config-key scheme here rather than inheriting leaf's blindly.
    if name == "server-leaf" {
        env.insert(
            "LEAF_DATABASE__PATH".to_string(),
            "./brigid-dev.db".to_string(),
        );
        env.insert(
            "LEAF_SERVER__DOMAIN".to_string(),
            "brigid.localhost".to_string(),
        );
        env.insert(
            "LEAF_SERVER__PUBLIC_URL".to_string(),
            "https://brigid.localhost:5173".to_string(),
        );
    }

    let bin = name.strip_prefix("server-").unwrap_or(name);
    MprocsProc {
        shell: format!("cargo run -p {bin}"),
        cwd: workspace_root().join(name).display().to_string(),
        env,
    }
}

fn app_proc() -> MprocsProc {
    MprocsProc {
        shell: "corepack pnpm dev".to_string(),
        cwd: workspace_root().join("app").display().to_string(),
        env: BTreeMap::new(),
    }
}

#[derive(Serialize)]
struct MprocsConfig {
    procs: BTreeMap<String, MprocsProc>,
}

#[derive(Serialize)]
struct MprocsProc {
    shell: String,
    cwd: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
}
