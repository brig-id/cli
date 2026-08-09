//! Minimal read/write for the gitignored `cli/.env` file that persists
//! `BRIGID_MASTER_KEY` across shells — dev-only, never used in production
//! (production reads `BRIGID_MASTER_KEY`/`BRIGID_MASTER_KEY_FILE` directly from
//! the deployment environment, per `server-leaf/AGENTS.md`'s hard security
//! constraints).

use std::path::{Path, PathBuf};

use crate::repos::cli_root;

const MASTER_KEY_VAR: &str = "BRIGID_MASTER_KEY";

pub fn path() -> PathBuf {
    cli_root().join(".env")
}

/// Reads `BRIGID_MASTER_KEY` out of `.env`, if present.
pub fn read_master_key(env_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(env_path).ok()?;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix(&format!("{MASTER_KEY_VAR}=")) {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Writes/replaces `BRIGID_MASTER_KEY` in `.env`, preserving any other lines.
pub fn write_master_key(env_path: &Path, key: &str) -> anyhow::Result<()> {
    let mut lines: Vec<String> = std::fs::read_to_string(env_path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.starts_with(&format!("{MASTER_KEY_VAR}=")))
        .map(str::to_string)
        .collect();
    lines.push(format!("{MASTER_KEY_VAR}={key}"));
    std::fs::write(env_path, lines.join("\n") + "\n")?;
    Ok(())
}

/// 32 bytes read from `/dev/urandom`, hex-encoded — same shape as
/// `openssl rand -hex 32`. Errors out rather than falling back to anything
/// weaker: this seeds a real (if dev-only) secret, not cosmetic randomness.
pub fn generate_master_key() -> anyhow::Result<String> {
    use std::io::Read;

    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|err| anyhow::anyhow!("reading /dev/urandom: {err}"))?;

    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
