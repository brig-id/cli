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
            "no runnable server found (server-seedling/server-grove/server-forest all missing a Cargo.toml)"
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
    std::fs::write(&config_path, serde_norway::to_string(&config)?)
        .with_context(|| format!("writing {}", config_path.display()))?;

    println!("\n→ launching mprocs ({})...\n", config_path.display());
    let status = std::process::Command::new("mprocs")
        .arg("--config")
        .arg(&config_path)
        .status()
        .context("failed to spawn mprocs — is it on PATH? (`brigid check`)")?;

    std::process::exit(status.code().unwrap_or(1));
}

/// The servers, one per edition, that have an actual `Cargo.toml` today.
/// `server-leaf` is the name `server-seedling` had: a workspace cloned
/// before the rename still holds it under that folder.
const SERVERS: [&str; 4] = [
    "server-seedling",
    "server-leaf",
    "server-grove",
    "server-forest",
];

fn available_servers() -> Vec<String> {
    SERVERS
        .into_iter()
        .filter(|name| workspace_root().join(name).join("Cargo.toml").is_file())
        .map(str::to_string)
        .collect()
}

fn server_proc(name: &str, master_key: &str) -> MprocsProc {
    let mut env = BTreeMap::new();
    env.insert("BRIGID_MASTER_KEY".to_string(), master_key.to_string());

    // The three editions read their settings under one prefix, `BRIGID_`
    // (see `server-seedling/AGENTS.md`, "Local dev without Docker").
    for (key, value) in dev_settings() {
        env.insert(key.to_string(), value.to_string());
    }

    MprocsProc {
        // Each server repository holds one binary: no package to name.
        shell: "cargo run".to_string(),
        cwd: workspace_root().join(name).display().to_string(),
        env,
    }
}

/// What a server needs to serve the dev interface on `brigid.localhost`.
fn dev_settings() -> [(&'static str, &'static str); 3] {
    [
        ("BRIGID_DATABASE__PATH", "./brigid-dev.db"),
        ("BRIGID_SERVER__DOMAIN", "brigid.localhost"),
        ("BRIGID_SERVER__PUBLIC_URL", "https://brigid.localhost:5173"),
    ]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_gets_the_master_key_and_its_settings_under_one_prefix() {
        let proc = server_proc("server-seedling", "00ff");
        assert_eq!(proc.shell, "cargo run");
        assert!(proc.cwd.ends_with("server-seedling"));
        assert_eq!(proc.env["BRIGID_MASTER_KEY"], "00ff");
        assert_eq!(proc.env["BRIGID_SERVER__DOMAIN"], "brigid.localhost");
        assert!(proc.env.keys().all(|key| key.starts_with("BRIGID_")));
    }

    #[test]
    fn the_renamed_server_comes_first() {
        assert_eq!(SERVERS[0], "server-seedling");
        assert!(SERVERS.contains(&"server-leaf"));
    }
}
