//! Shared repo-list/path resolution, backing every `brigid repos` subcommand.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct ReposFile {
    repos: Vec<String>,
}

/// Absolute path to this repo's (`cli`) own root (baked in at compile time).
pub fn cli_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Absolute path to the directory containing every sibling repo.
pub fn workspace_root() -> PathBuf {
    cli_root()
        .parent()
        .expect("`cli` must have a parent directory")
        .to_path_buf()
}

/// Sibling repo names from `repos.json`, in declaration order (`cli` itself is
/// excluded — callers that want it included too should use [`all_repos`]).
pub fn sibling_repo_names() -> Result<Vec<String>> {
    let path = cli_root().join("repos.json");
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let parsed: ReposFile =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    Ok(parsed.repos)
}

pub struct Repo {
    pub name: String,
    pub path: PathBuf,
}

pub enum ProjectKind {
    Cargo,
    Pnpm,
    None,
}

impl Repo {
    pub fn kind(&self) -> ProjectKind {
        if self.path.join("Cargo.toml").is_file() {
            ProjectKind::Cargo
        } else if self.path.join("package.json").is_file() {
            ProjectKind::Pnpm
        } else {
            ProjectKind::None
        }
    }

    pub fn is_git_repo(&self) -> bool {
        self.path.join(".git").exists()
    }

    /// Whether this repo's `package.json` declares a `scripts.<name>` entry.
    pub fn has_pnpm_script(&self, name: &str) -> bool {
        let Ok(raw) = std::fs::read_to_string(self.path.join("package.json")) else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return false;
        };
        value
            .get("scripts")
            .and_then(|scripts| scripts.get(name))
            .is_some()
    }
}

/// Every repo brigid operates over: `cli` itself, then each sibling in `repos.json`.
pub fn all_repos() -> Result<Vec<Repo>> {
    let mut repos = vec![Repo {
        name: "cli".to_string(),
        path: cli_root(),
    }];
    for name in sibling_repo_names()? {
        let path = workspace_root().join(&name);
        repos.push(Repo { name, path });
    }
    Ok(repos)
}
