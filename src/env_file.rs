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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_master_key_is_64_lowercase_hex_chars() {
        let key = generate_master_key().unwrap();
        assert_eq!(key.len(), 64);
        assert!(
            key.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn generate_master_key_is_not_deterministic() {
        assert_ne!(
            generate_master_key().unwrap(),
            generate_master_key().unwrap()
        );
    }

    #[test]
    fn read_master_key_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_master_key(&dir.path().join("nope.env")).is_none());
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        write_master_key(&env_path, "abc123").unwrap();
        assert_eq!(read_master_key(&env_path).as_deref(), Some("abc123"));
    }

    #[test]
    fn write_master_key_replaces_existing_value_and_preserves_other_lines() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        std::fs::write(
            &env_path,
            format!("OTHER_VAR=keep-me\n{MASTER_KEY_VAR}=old\n"),
        )
        .unwrap();

        write_master_key(&env_path, "new-value").unwrap();

        let content = std::fs::read_to_string(&env_path).unwrap();
        assert!(content.contains("OTHER_VAR=keep-me"));
        assert_eq!(read_master_key(&env_path).as_deref(), Some("new-value"));
        assert_eq!(content.matches(MASTER_KEY_VAR).count(), 1);
    }
}
