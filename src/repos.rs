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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_root_points_at_this_crate() {
        assert!(cli_root().join("Cargo.toml").is_file());
    }

    #[test]
    fn workspace_root_is_cli_roots_parent() {
        assert_eq!(workspace_root(), cli_root().parent().unwrap());
    }

    #[test]
    fn sibling_repo_names_reads_real_repos_json() {
        // Exercises the real repos.json shipped in this crate — catches a
        // malformed file at test time rather than only when a `brigid repos`
        // subcommand happens to be run.
        let names = sibling_repo_names().unwrap();
        assert!(!names.is_empty());
        assert!(
            !names.contains(&"cli".to_string()),
            "cli must not list itself in repos.json — all_repos() adds it implicitly"
        );
    }

    #[test]
    fn all_repos_puts_cli_first_and_includes_every_sibling() {
        let repos = all_repos().unwrap();
        assert_eq!(repos[0].name, "cli");
        assert_eq!(repos[0].path, cli_root());
        assert_eq!(repos.len(), sibling_repo_names().unwrap().len() + 1);
    }

    #[test]
    fn kind_detects_cargo_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let repo = Repo {
            name: "x".to_string(),
            path: dir.path().to_path_buf(),
        };
        assert!(matches!(repo.kind(), ProjectKind::Cargo));
    }

    #[test]
    fn kind_detects_pnpm_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let repo = Repo {
            name: "x".to_string(),
            path: dir.path().to_path_buf(),
        };
        assert!(matches!(repo.kind(), ProjectKind::Pnpm));
    }

    #[test]
    fn kind_is_none_without_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo {
            name: "x".to_string(),
            path: dir.path().to_path_buf(),
        };
        assert!(matches!(repo.kind(), ProjectKind::None));
    }

    #[test]
    fn is_git_repo_checks_for_dot_git() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo {
            name: "x".to_string(),
            path: dir.path().to_path_buf(),
        };
        assert!(!repo.is_git_repo());
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(repo.is_git_repo());
    }

    #[test]
    fn has_pnpm_script_finds_declared_scripts_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"build": "vite build"}}"#,
        )
        .unwrap();
        let repo = Repo {
            name: "x".to_string(),
            path: dir.path().to_path_buf(),
        };
        assert!(repo.has_pnpm_script("build"));
        assert!(!repo.has_pnpm_script("test"));
    }

    #[test]
    fn has_pnpm_script_false_without_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo {
            name: "x".to_string(),
            path: dir.path().to_path_buf(),
        };
        assert!(!repo.has_pnpm_script("build"));
    }
}
