//! `brigid release` — the release train, in the order it has to be merged.
//!
//! Each repository releases on its own: release-please keeps one release pull
//! request open per repository, and Renovate brings a new release of crypto,
//! core or the interface into the repositories that pin it, in one pull
//! request labelled `brig-id`. A release of everything is therefore a chain:
//! crypto, then the core, then the interface, then the servers, each one's
//! release becoming the next one's update.
//!
//! This command reads that chain from GitHub (through `gh`) and prints it:
//! for each repository, the pull requests to merge, with their checks, and
//! what it pins that is older than the latest release (an update Renovate has
//! yet to propose). It never merges: merging is how a release is reviewed.

use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

const ORG: &str = "brig-id";

/// The repositories of the train, in the order a release flows through them.
const TRAIN: [&str; 5] = ["crypto", "core", "app", "server-seedling", "server-grove"];

/// What each repository pins, and where: (dependency, file on `main`).
fn pinned(repo: &str) -> &'static [(&'static str, &'static str)] {
    match repo {
        "core" => &[("crypto", "Cargo.toml")],
        "server-seedling" => &[
            ("crypto", "Cargo.toml"),
            ("core", "Cargo.toml"),
            ("app", ".github/workflows/ci.yml"),
        ],
        "server-grove" => &[("core", "Cargo.toml"), ("app", ".github/workflows/ci.yml")],
        _ => &[],
    }
}

/// The label release-please puts on its open pull request.
const RELEASE_LABEL: &str = "autorelease: pending";
/// The label the organisation's Renovate preset puts on an update to crypto,
/// core or the interface.
const UPDATE_LABEL: &str = "brig-id";

pub fn run(watch: bool) -> Result<()> {
    if !watch {
        return print_train();
    }
    loop {
        // Clear the screen and go home, then print the train afresh.
        print!("\x1b[2J\x1b[H");
        print_train()?;
        println!("\n(refreshed every minute; Ctrl-C to stop)");
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

fn print_train() -> Result<()> {
    let latest: Vec<(&str, Option<String>)> = TRAIN
        .iter()
        .map(|repo| (*repo, latest_release(repo)))
        .collect();
    let latest_of = |repo: &str| {
        latest
            .iter()
            .find(|(name, _)| *name == repo)
            .and_then(|(_, tag)| tag.clone())
    };

    println!("brig·id release train: merge from top to bottom.\n");
    let mut step = 0;
    let mut idle = true;
    for repo in TRAIN {
        let current = latest_of(repo).unwrap_or_else(|| "no release".into());
        println!("{repo} ({current})");

        let updates = open_prs(repo, UPDATE_LABEL)?;
        let releases = open_prs(repo, RELEASE_LABEL)?;
        for pr in updates.iter().chain(releases.iter()) {
            step += 1;
            idle = false;
            println!("  {step}. merge #{} {}", pr.number, pr.title);
            println!("     {}  {}", pr.checks.describe(), pr.url);
        }

        if updates.is_empty() {
            for (dependency, file) in pinned(repo) {
                let Some(newest) = latest_of(dependency) else {
                    continue;
                };
                match pin_in(repo, dependency, file)? {
                    Some(pin) if pin != newest => {
                        idle = false;
                        println!(
                            "  … waiting for Renovate: {dependency} {pin} → {newest} \
                             (or tick it on the repository's Dependency Dashboard)"
                        );
                    }
                    Some(_) => {}
                    None => println!("  ? {dependency}: no tag pin found in {file}"),
                }
            }
        }
        if updates.is_empty() && releases.is_empty() {
            println!("  nothing to merge");
        }
    }
    if idle {
        println!("\nNothing to release: every repository is at its latest release.");
    }
    Ok(())
}

struct Pr {
    number: u64,
    title: String,
    url: String,
    checks: Checks,
}

#[derive(Debug, PartialEq)]
enum Checks {
    None,
    Passing(usize),
    Pending(usize),
    Failing(Vec<String>),
}

impl Checks {
    fn from_rollup(rollup: &[Value]) -> Self {
        let mut failing = Vec::new();
        let mut pending = 0;
        for check in rollup {
            // A check run has a status and a conclusion; a commit status has a state.
            let name = check["name"]
                .as_str()
                .or_else(|| check["context"].as_str())
                .unwrap_or("?");
            let conclusion = check["conclusion"].as_str().unwrap_or("");
            let state = check["state"].as_str().unwrap_or("");
            let status = check["status"].as_str().unwrap_or("");
            if matches!(
                conclusion,
                "FAILURE" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
            ) || matches!(state, "FAILURE" | "ERROR")
            {
                failing.push(name.to_string());
            } else if (!status.is_empty() && status != "COMPLETED") || state == "PENDING" {
                pending += 1;
            }
        }
        if !failing.is_empty() {
            Checks::Failing(failing)
        } else if pending > 0 {
            Checks::Pending(pending)
        } else if rollup.is_empty() {
            Checks::None
        } else {
            Checks::Passing(rollup.len())
        }
    }

    fn describe(&self) -> String {
        match self {
            Checks::None => "⚠️  no checks".into(),
            Checks::Passing(n) => format!("✅ {n} checks pass"),
            Checks::Pending(n) => format!("⏳ {n} checks running"),
            Checks::Failing(names) => format!("❌ failing: {}", names.join(", ")),
        }
    }
}

fn gh(args: &[&str]) -> Result<String> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .context("running `gh` (GitHub CLI): is it installed and logged in?")?;
    if !output.status.success() {
        bail!(
            "gh {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// The latest release's tag, or `None` when the repository has none.
fn latest_release(repo: &str) -> Option<String> {
    let full = format!("{ORG}/{repo}");
    gh(&[
        "release", "view", "-R", &full, "--json", "tagName", "--jq", ".tagName",
    ])
    .ok()
    .map(|tag| tag.trim().to_string())
    .filter(|tag| !tag.is_empty())
}

fn open_prs(repo: &str, label: &str) -> Result<Vec<Pr>> {
    let full = format!("{ORG}/{repo}");
    let raw = gh(&[
        "pr",
        "list",
        "-R",
        &full,
        "--state",
        "open",
        "--label",
        label,
        "--json",
        "number,title,url,statusCheckRollup",
    ])?;
    let list: Vec<Value> = serde_json::from_str(&raw)?;
    Ok(list
        .into_iter()
        .map(|pr| Pr {
            number: pr["number"].as_u64().unwrap_or(0),
            title: pr["title"].as_str().unwrap_or("").to_string(),
            url: pr["url"].as_str().unwrap_or("").to_string(),
            checks: Checks::from_rollup(
                pr["statusCheckRollup"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice),
            ),
        })
        .collect())
}

/// The tag `repo` pins `dependency` at, read from `file` on its `main`.
fn pin_in(repo: &str, dependency: &str, file: &str) -> Result<Option<String>> {
    let path = format!("repos/{ORG}/{repo}/contents/{file}");
    let text = gh(&["api", "-H", "Accept: application/vnd.github.raw", &path])?;
    Ok(if file.ends_with("Cargo.toml") {
        cargo_tag_pin(&text, dependency)
    } else {
        annotated_tag_pin(&text, dependency)
    })
}

/// `name = { git = "https://github.com/brig-id/<dependency>", tag = "vX.Y.Z", … }`
fn cargo_tag_pin(cargo_toml: &str, dependency: &str) -> Option<String> {
    let url = format!("github.com/{ORG}/{dependency}\"");
    cargo_toml
        .lines()
        .filter(|line| line.contains(&url))
        .find_map(|line| quoted_after(line, "tag = "))
}

/// The line after `# renovate: … depName=brig-id/<dependency>`:
/// `KEY: <commit> # vX.Y.Z`.
fn annotated_tag_pin(text: &str, dependency: &str) -> Option<String> {
    let marker = format!("depName={ORG}/{dependency}");
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.contains("# renovate:") && line.split_whitespace().any(|word| word == marker) {
            let pin = lines.next()?;
            let (_, comment) = pin.split_once(" # ")?;
            return comment.split_whitespace().next().map(str::to_string);
        }
    }
    None
}

fn quoted_after(line: &str, key: &str) -> Option<String> {
    let rest = &line[line.find(key)? + key.len()..];
    let rest = rest.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_cargo_tag_pin() {
        let toml = r#"
[dependencies]
brigid-server = { git = "https://github.com/brig-id/core", tag = "v0.2.0", package = "brigid-server" }
brigid-crypto = { git = "https://github.com/brig-id/crypto", tag = "v0.1.0" }
"#;
        assert_eq!(cargo_tag_pin(toml, "core").as_deref(), Some("v0.2.0"));
        assert_eq!(cargo_tag_pin(toml, "crypto").as_deref(), Some("v0.1.0"));
        assert_eq!(cargo_tag_pin(toml, "app"), None);
    }

    #[test]
    fn a_rev_pin_is_not_a_tag_pin() {
        let toml = r#"brigid-api = { git = "https://github.com/brig-id/core", rev = "27e30a9" }"#;
        assert_eq!(cargo_tag_pin(toml, "core"), None);
    }

    #[test]
    fn does_not_mistake_core_for_a_longer_name() {
        let toml = r#"x = { git = "https://github.com/brig-id/core-extra", tag = "v9.9.9" }"#;
        assert_eq!(cargo_tag_pin(toml, "core"), None);
    }

    #[test]
    fn reads_an_annotated_pin() {
        let yaml = "env:\n  # renovate: datasource=github-tags depName=brig-id/app\n  \
                    APP_REF: 9ee1110f49e93d705095d2aa2b1717581c400b1e # v0.11.1\n";
        assert_eq!(annotated_tag_pin(yaml, "app").as_deref(), Some("v0.11.1"));
        assert_eq!(annotated_tag_pin(yaml, "core"), None);
    }

    #[test]
    fn checks_summarise_the_rollup() {
        assert_eq!(Checks::from_rollup(&[]), Checks::None);
        let done = json!({"name": "CI", "status": "COMPLETED", "conclusion": "SUCCESS"});
        let running = json!({"name": "E2E", "status": "IN_PROGRESS", "conclusion": ""});
        let failed = json!({"name": "Clippy", "status": "COMPLETED", "conclusion": "FAILURE"});
        let status = json!({"context": "codecov", "state": "PENDING"});
        assert_eq!(
            Checks::from_rollup(std::slice::from_ref(&done)),
            Checks::Passing(1)
        );
        assert_eq!(
            Checks::from_rollup(&[done.clone(), running]),
            Checks::Pending(1)
        );
        assert_eq!(
            Checks::from_rollup(&[done.clone(), status]),
            Checks::Pending(1)
        );
        assert_eq!(
            Checks::from_rollup(&[done, failed]),
            Checks::Failing(vec!["Clippy".into()])
        );
    }
}
