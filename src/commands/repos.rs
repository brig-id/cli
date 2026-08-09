//! `brigid repos <action>` — cross-repo git/build commands.
//!
//! Replaces the old `scripts/git-each.mjs` + `scripts/run-each.mjs` Node
//! scripts, and (unlike `run-each.mjs`, which only knew about `pnpm`) dispatches
//! per repo on whether it's a cargo crate or a pnpm project.

use clap::Subcommand;

use crate::repos::{ProjectKind, Repo, all_repos};

#[derive(Subcommand)]
pub enum ReposAction {
    /// `git status -sb` in every repo.
    Status,
    /// `git fetch --all --prune` in every repo.
    Fetch,
    /// `git pull --rebase --autostash` in every repo.
    Pull,
    /// `git branch --show-current` in every repo.
    Branch,
    /// Install dependencies (`cargo fetch` / `pnpm install`) in every repo.
    Install,
    /// Build (`cargo build` / `pnpm run build`) in every repo.
    Build,
    /// Test (`cargo test --workspace` / `pnpm run test`) in every repo.
    Test,
    /// Lint (`cargo clippy -D warnings` / `pnpm run lint`) in every repo.
    Lint,
    /// Run an arbitrary git command across every repo, e.g. `brigid repos git log -1`.
    Git {
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
}

pub fn run(action: ReposAction) -> anyhow::Result<()> {
    let repos = all_repos()?;

    let failed = match action {
        ReposAction::Status => run_git_each(&repos, &["--no-pager", "status", "-sb"]),
        ReposAction::Fetch => run_git_each(&repos, &["--no-pager", "fetch", "--all", "--prune"]),
        ReposAction::Pull => {
            run_git_each(&repos, &["--no-pager", "pull", "--rebase", "--autostash"])
        }
        ReposAction::Branch => run_git_each(&repos, &["--no-pager", "branch", "--show-current"]),
        ReposAction::Git { args } => {
            if args.is_empty() {
                anyhow::bail!("usage: brigid repos git <args...>");
            }
            let owned: Vec<&str> = args.iter().map(String::as_str).collect();
            run_git_each(&repos, &owned)
        }
        ReposAction::Install => run_script_each(&repos, Script::Install),
        ReposAction::Build => run_script_each(&repos, Script::Build),
        ReposAction::Test => run_script_each(&repos, Script::Test),
        ReposAction::Lint => run_script_each(&repos, Script::Lint),
    };

    if failed > 0 {
        anyhow::bail!("{failed} repo(s) failed");
    }
    Ok(())
}

fn run_git_each(repos: &[Repo], args: &[&str]) -> usize {
    let mut failed = 0;
    for repo in repos {
        if !repo.is_git_repo() {
            println!("⚠️  {}: not a git repo (skipped)", repo.name);
            continue;
        }
        println!("\n━━━ {} — git {} ━━━", repo.name, args.join(" "));
        match spawn(&repo.path, "git", args) {
            Ok(true) => {}
            Ok(false) => failed += 1,
            Err(err) => {
                println!("!  {}: failed to spawn git ({err})", repo.name);
                failed += 1;
            }
        }
    }
    failed
}

#[derive(Clone, Copy)]
enum Script {
    Install,
    Build,
    Test,
    Lint,
}

impl Script {
    fn name(self) -> &'static str {
        match self {
            Script::Install => "install",
            Script::Build => "build",
            Script::Test => "test",
            Script::Lint => "lint",
        }
    }

    fn cargo_args(self) -> &'static [&'static str] {
        match self {
            Script::Install => &["fetch"],
            Script::Build => &["build"],
            Script::Test => &["test", "--workspace"],
            Script::Lint => &[
                "clippy",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
        }
    }
}

fn run_script_each(repos: &[Repo], script: Script) -> usize {
    let mut failed = 0;
    for repo in repos {
        let (program, args): (&str, Vec<&str>) = match repo.kind() {
            ProjectKind::Cargo => ("cargo", script.cargo_args().to_vec()),
            ProjectKind::Pnpm => {
                if script.name() != "install" && !repo.has_pnpm_script(script.name()) {
                    println!(
                        "⚠️  {}: no \"{}\" script (skipped)",
                        repo.name,
                        script.name()
                    );
                    continue;
                }
                // Dispatched through corepack (not a bare "pnpm") so each repo's
                // own "packageManager" pin is what actually runs.
                let mut args = vec!["pnpm"];
                if script.name() == "install" {
                    args.push("install");
                } else {
                    args.extend(["run", script.name()]);
                }
                ("corepack", args)
            }
            ProjectKind::None => {
                println!("⚠️  {}: no Cargo.toml or package.json (skipped)", repo.name);
                continue;
            }
        };

        println!("\n━━━ {} — {} {} ━━━", repo.name, program, args.join(" "));
        match spawn(&repo.path, program, &args) {
            Ok(true) => {}
            Ok(false) => failed += 1,
            Err(err) => {
                println!("!  {}: failed to spawn {program} ({err})", repo.name);
                failed += 1;
            }
        }
    }
    failed
}

/// Spawns `program` with `args` in `cwd`, inheriting stdio. Returns `Ok(true)`
/// on a zero exit status, `Ok(false)` on a non-zero one.
fn spawn(cwd: &std::path::Path, program: &str, args: &[&str]) -> std::io::Result<bool> {
    let status = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()?;
    Ok(status.success())
}
