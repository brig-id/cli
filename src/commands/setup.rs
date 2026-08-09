//! `brigid setup` — idempotently fixes whatever `brigid check` finds missing:
//! trusts the local mkcert CA, generates the `brigid.localhost` dev cert, and
//! persists a `BRIGID_MASTER_KEY` in a gitignored `cli/.env`.

use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{env_file, repos::workspace_root};

pub fn run() -> Result<()> {
    ensure_mkcert_present()?;
    ensure_ca_installed()?;
    ensure_app_cert()?;
    ensure_master_key()?;
    println!("\n✓ setup complete — run `brigid dev` to launch.");
    Ok(())
}

fn ensure_mkcert_present() -> Result<()> {
    if Command::new("mkcert").arg("-version").output().is_ok() {
        return Ok(());
    }
    bail!(
        "mkcert is not on PATH. It's installed by roots/.devcontainer/setup-container.sh — \
         rebuild the devcontainer, or install it manually: https://github.com/FiloSottile/mkcert"
    );
}

fn ensure_ca_installed() -> Result<()> {
    let root = String::from_utf8(Command::new("mkcert").arg("-CAROOT").output()?.stdout)
        .unwrap_or_default();
    let root = root.trim();
    let already_installed =
        !root.is_empty() && std::path::Path::new(root).join("rootCA.pem").is_file();
    if already_installed {
        println!("✓ mkcert CA already installed at {root}");
        return Ok(());
    }

    println!("→ running `mkcert -install`...");
    let status = Command::new("mkcert").arg("-install").status()?;
    if !status.success() {
        bail!("`mkcert -install` failed");
    }
    println!("✓ mkcert CA installed");
    Ok(())
}

fn ensure_app_cert() -> Result<()> {
    let cert_dir = workspace_root().join("app").join(".cert");
    let cert = cert_dir.join("brigid.localhost.pem");
    let key = cert_dir.join("brigid.localhost-key.pem");

    if cert.is_file() && key.is_file() {
        println!("✓ app/.cert/brigid.localhost already present");
        return Ok(());
    }

    std::fs::create_dir_all(&cert_dir)
        .with_context(|| format!("creating {}", cert_dir.display()))?;

    println!("→ generating app/.cert/brigid.localhost cert...");
    let status = Command::new("mkcert")
        .args([
            "-cert-file",
            "brigid.localhost.pem",
            "-key-file",
            "brigid.localhost-key.pem",
            "brigid.localhost",
            "*.brigid.localhost",
            "localhost",
            "127.0.0.1",
            "::1",
        ])
        .current_dir(&cert_dir)
        .status()?;
    if !status.success() {
        bail!("mkcert cert generation failed");
    }
    println!("✓ app/.cert/brigid.localhost generated");
    Ok(())
}

fn ensure_master_key() -> Result<()> {
    let env_path = env_file::path();
    if let Some(key) = env_file::read_master_key(&env_path) {
        if key.len() == 64 && key.chars().all(|c| c.is_ascii_hexdigit()) {
            println!("✓ cli/.env BRIGID_MASTER_KEY already present");
            return Ok(());
        }
    }

    println!("→ generating BRIGID_MASTER_KEY...");
    let key = env_file::generate_master_key()?;
    env_file::write_master_key(&env_path, &key)?;
    println!(
        "✓ BRIGID_MASTER_KEY written to cli/.env\n\
         ⚠️  dev only — never reuse this key, or a `.env`-style file, in production. \
         See server-leaf/AGENTS.md's hard security constraints."
    );
    Ok(())
}
