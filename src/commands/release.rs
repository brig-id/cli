//! `brigid release` — what a release of a server would hold, and the gesture
//! that makes one.
//!
//! Only the servers are released (seedling, grove). crypto, the core and the
//! interface are what a server is made of: a server follows each at a commit
//! of its `main` (`Cargo.lock` for crypto and the core, `APP_REF` in its
//! `ci.yml` for the interface), and a release of the server says what moved
//! in all four since the release before.
//!
//! Nothing here releases on its own. `brigid release` reads GitHub (through
//! `gh`) and says, for each server, whether a release would hold anything a
//! person running it could notice. `brigid release cut` starts the server's
//! `Release` workflow, which builds, publishes, and sets the tag last. That
//! workflow calls `brigid release plan` for the version and the notes, so
//! what is announced here is what it does.

use std::fmt::Write as _;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};
use serde_json::Value;

const ORG: &str = "brig-id";

/// A server, and the repository it lives in.
struct Product {
    name: &'static str,
    repo: &'static str,
}

const PRODUCTS: [Product; 2] = [
    Product {
        name: "seedling",
        repo: "server-seedling",
    },
    Product {
        name: "grove",
        repo: "server-grove",
    },
];

/// What a server is made of, besides its own repository, and where it writes
/// the commit it follows.
const COMPONENTS: [(&str, Pin); 3] = [
    ("crypto", Pin::CargoLock),
    ("core", Pin::CargoLock),
    ("app", Pin::AppRef),
];

#[derive(Clone, Copy)]
enum Pin {
    /// `source = "git+https://github.com/brig-id/<name>?…#<commit>"` in `Cargo.lock`.
    CargoLock,
    /// `APP_REF: <commit>` in `.github/workflows/ci.yml`.
    AppRef,
}

impl Pin {
    fn file(self) -> &'static str {
        match self {
            Pin::CargoLock => "Cargo.lock",
            Pin::AppRef => ".github/workflows/ci.yml",
        }
    }

    /// How a server moves to a newer commit of the component.
    fn how_to_move(self) -> &'static str {
        match self {
            Pin::CargoLock => "its Cargo.lock: cargo update -p <crate>",
            Pin::AppRef => "APP_REF in its ci.yml",
        }
    }
}

#[derive(Subcommand)]
pub enum ReleaseAction {
    /// What a release would hold: its notes, as they would be published.
    Notes {
        /// seedling or grove.
        product: String,
    },
    /// Start the server's Release workflow (it builds, publishes, tags last).
    Cut {
        /// seedling or grove.
        product: String,
        /// The size of the step; `auto` reads it from the commits.
        #[arg(long, value_enum, default_value_t = Bump::Auto)]
        bump: Bump,
        /// Release even when nothing changed for the people who run the server.
        #[arg(long)]
        force: bool,
        /// Build and check everything, publish nothing.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask before starting.
        #[arg(long, short)]
        yes: bool,
    },
    /// For the Release workflow: the version and the notes of the release at
    /// a commit, written to `$GITHUB_OUTPUT` when it is set.
    Plan {
        /// seedling or grove (or the repository's name).
        product: String,
        #[arg(long, value_enum, default_value_t = Bump::Auto)]
        bump: Bump,
        #[arg(long)]
        force: bool,
        /// The commit to release (default: `$GITHUB_SHA`, else `main`).
        #[arg(long)]
        at: Option<String>,
        /// Where to write the notes.
        #[arg(long)]
        notes: Option<std::path::PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum Bump {
    Auto,
    Patch,
    Minor,
    Major,
}

pub fn run(action: Option<ReleaseAction>) -> Result<()> {
    match action {
        None => status(),
        Some(ReleaseAction::Notes { product }) => {
            let plan = plan(product_named(&product)?, "main", Bump::Auto, true)?;
            print!("{}", plan.notes);
            Ok(())
        }
        Some(ReleaseAction::Cut {
            product,
            bump,
            force,
            dry_run,
            yes,
        }) => cut(product_named(&product)?, bump, force, dry_run, yes),
        Some(ReleaseAction::Plan {
            product,
            bump,
            force,
            at,
            notes,
        }) => {
            let at = at
                .or_else(|| std::env::var("GITHUB_SHA").ok())
                .unwrap_or_else(|| "main".into());
            let plan = plan(product_named(&product)?, &at, bump, force)?;
            if let Some(path) = notes {
                std::fs::write(&path, &plan.notes)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            let outputs = plan.outputs();
            match std::env::var("GITHUB_OUTPUT") {
                Ok(file) if !file.is_empty() => {
                    use std::io::Write as _;
                    let mut out = std::fs::OpenOptions::new().append(true).open(&file)?;
                    out.write_all(outputs.as_bytes())?;
                }
                _ => {}
            }
            print!("{outputs}");
            Ok(())
        }
    }
}

fn product_named(name: &str) -> Result<&'static Product> {
    PRODUCTS
        .iter()
        .find(|product| product.name == name || product.repo == name)
        .with_context(|| format!("`{name}` is not a server that is released: seedling or grove"))
}

// -- What a release would hold ------------------------------------------------

/// One commit, as far as a release is concerned.
#[derive(Debug, Clone, PartialEq)]
struct Change {
    repo: String,
    sha: String,
    kind: Kind,
    breaking: bool,
    subject: String,
}

/// What a commit is to a person who runs the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Feature,
    Fix,
    Performance,
    Revert,
    /// Documentation, tests, the pipeline, updates of what the build uses:
    /// part of the release, and no reason for one.
    Unseen,
}

/// Scopes of commits that never reach a built server, whatever their type.
const NEVER_SHIPPED: [&str; 3] = ["ci", "e2e", "ai"];

impl Change {
    /// Reads a commit written as `type(scope)!: subject`. Anything else is
    /// part of the release without being a reason for one.
    fn parse(repo: &str, sha: &str, message: &str) -> Self {
        let first = message.lines().next().unwrap_or("").trim();
        let (head, subject) = match first.split_once(": ") {
            Some((head, subject)) if !head.contains(' ') => (head, subject),
            _ => ("", first),
        };
        let bang = head.ends_with('!');
        let head = head.trim_end_matches('!');
        let (kind, scope) = match head.split_once('(') {
            Some((kind, scope)) => (kind, scope.trim_end_matches(')')),
            None => (head, ""),
        };
        let kind = if NEVER_SHIPPED.contains(&scope) {
            Kind::Unseen
        } else {
            match kind {
                "feat" => Kind::Feature,
                "fix" => Kind::Fix,
                "perf" => Kind::Performance,
                "revert" => Kind::Revert,
                _ => Kind::Unseen,
            }
        };
        let breaking = bang
            || message.lines().any(|line| {
                line.starts_with("BREAKING CHANGE:") || line.starts_with("BREAKING-CHANGE:")
            });
        Change {
            repo: repo.to_string(),
            sha: sha.to_string(),
            kind,
            breaking,
            subject: without_leading_emoji(subject).to_string(),
        }
    }

    fn seen(&self) -> bool {
        self.breaking || self.kind != Kind::Unseen
    }
}

/// `✨ read the limits` → `read the limits`: the section's title says the kind.
fn without_leading_emoji(subject: &str) -> &str {
    match subject.split_once(' ') {
        Some((first, rest)) if !first.chars().any(|c| c.is_ascii_alphanumeric()) => {
            rest.trim_start()
        }
        _ => subject,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

impl Version {
    fn parse(tag: &str) -> Option<Self> {
        let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.');
        let version = Version(
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        parts.next().is_none().then_some(version)
    }

    fn bumped(self, bump: Bump) -> Self {
        match bump {
            Bump::Major => Version(self.0 + 1, 0, 0),
            Bump::Minor => Version(self.0, self.1 + 1, 0),
            Bump::Patch | Bump::Auto => Version(self.0, self.1, self.2 + 1),
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The step the changes ask for, or `None` when they ask for no release.
///
/// Before 1.0 a release is a patch, and only a change that breaks what worked
/// moves the minor. From 1.0 on: a break is a major, a feature a minor.
fn step_for(changes: &[Change], current: Version) -> Option<Bump> {
    let stable = current.0 >= 1;
    if changes.iter().any(|change| change.breaking) {
        Some(if stable { Bump::Major } else { Bump::Minor })
    } else if changes.iter().any(|change| change.kind == Kind::Feature) {
        Some(if stable { Bump::Minor } else { Bump::Patch })
    } else if changes.iter().any(Change::seen) {
        Some(Bump::Patch)
    } else {
        None
    }
}

/// The version to release, or `None` when there is no release to make.
fn next_version(
    previous: Option<Version>,
    asked: Option<Bump>,
    chosen: Bump,
    force: bool,
) -> Option<Version> {
    let Some(previous) = previous else {
        // A first release is asked for by hand, and is 0.1.0.
        return Some(Version(0, 1, 0));
    };
    match (asked, chosen) {
        (None, _) if !force => None,
        (None, Bump::Auto) => Some(previous.bumped(Bump::Patch)),
        (Some(step), Bump::Auto) => Some(previous.bumped(step)),
        (_, chosen) => Some(previous.bumped(chosen)),
    }
}

/// A repository between the last release and the commit to release.
struct Moved {
    repo: &'static str,
    from: Option<String>,
    to: String,
    changes: Vec<Change>,
    /// For a component: how the server moves to a newer commit of it.
    pin: Option<Pin>,
}

struct Plan {
    previous: Option<Version>,
    /// The step the commits ask for (`None`: nothing a person would notice).
    asked: Option<Bump>,
    /// The version this release would be, when there is one to make.
    next: Option<Version>,
    moved: Vec<Moved>,
    notes: String,
}

impl Plan {
    fn outputs(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "needed={}", self.next.is_some());
        if let Some(next) = self.next {
            let _ = writeln!(out, "version={next}");
            let _ = writeln!(out, "tag=v{next}");
        }
        if let Some(previous) = self.previous {
            let _ = writeln!(out, "previous=v{previous}");
        }
        out
    }
}

fn plan(product: &'static Product, at: &str, bump: Bump, force: bool) -> Result<Plan> {
    let previous = latest_version(product.repo)?;
    let mut moved = Vec::new();

    // The server's own repository.
    let to = commit_of(product.repo, at)?;
    let changes = match previous {
        Some(version) => changes_between(product.repo, &format!("v{version}"), &to)?,
        None => Vec::new(),
    };
    moved.push(Moved {
        repo: product.repo,
        from: previous.map(|version| format!("v{version}")),
        to: to.clone(),
        changes,
        pin: None,
    });

    // What it is made of: each between the commit the last release followed
    // and the one this commit follows.
    for (component, pin) in COMPONENTS {
        let Some(now) = pin_at(product.repo, &to, component, pin)? else {
            continue;
        };
        let before = match previous {
            Some(version) => pin_at(product.repo, &format!("v{version}"), component, pin)?,
            None => None,
        };
        let changes = match &before {
            Some(before) if *before != now => changes_between(component, before, &now)?,
            _ => Vec::new(),
        };
        moved.push(Moved {
            repo: component,
            from: before,
            to: now,
            changes,
            pin: Some(pin),
        });
    }

    let all: Vec<Change> = moved.iter().flat_map(|m| m.changes.clone()).collect();
    let asked = step_for(&all, previous.unwrap_or(Version(0, 0, 0)));
    let next = next_version(previous, asked, bump, force);
    let notes = notes(product, previous, &moved);
    Ok(Plan {
        previous,
        asked,
        next,
        moved,
        notes,
    })
}

/// The notes of a release: what changed for the people who run the server,
/// then the commits it is built from.
fn notes(product: &Product, previous: Option<Version>, moved: &[Moved]) -> String {
    let mut out = String::new();
    let seen: Vec<&Change> = moved
        .iter()
        .flat_map(|m| m.changes.iter())
        .filter(|change| change.seen())
        .collect();

    if previous.is_none() {
        let _ = writeln!(out, "The first release of {}.\n", product.name);
    } else if seen.is_empty() {
        let _ = writeln!(
            out,
            "Nothing changes for the people who run {}: this release only carries \
             maintenance (documentation, tests, updates of what it is built with).\n",
            product.name
        );
    }

    type Belongs = fn(&Change) -> bool;
    let sections: [(&str, Belongs); 5] = [
        ("⚠️ Changes that need attention", |c| c.breaking),
        ("Features", |c| !c.breaking && c.kind == Kind::Feature),
        ("Fixes", |c| !c.breaking && c.kind == Kind::Fix),
        ("Performance", |c| {
            !c.breaking && c.kind == Kind::Performance
        }),
        ("Reverted", |c| !c.breaking && c.kind == Kind::Revert),
    ];
    for (title, belongs) in sections {
        let lines: Vec<&&Change> = seen.iter().filter(|change| belongs(change)).collect();
        if lines.is_empty() {
            continue;
        }
        let _ = writeln!(out, "### {title}\n");
        for change in lines {
            let _ = writeln!(
                out,
                "- {} ([{}@{}](https://github.com/{ORG}/{}/commit/{}))",
                change.subject,
                change.repo,
                short(&change.sha),
                change.repo,
                change.sha
            );
        }
        out.push('\n');
    }

    let _ = writeln!(out, "### Built from\n");
    for m in moved {
        let link = format!(
            "[{}@{}](https://github.com/{ORG}/{}/commit/{})",
            m.repo,
            short(&m.to),
            m.repo,
            m.to
        );
        let since = match &m.from {
            Some(from) if *from == m.to || m.changes.is_empty() => "unchanged".to_string(),
            Some(from) => format!(
                "[{} commits](https://github.com/{ORG}/{}/compare/{}...{}) since {}",
                m.changes.len(),
                m.repo,
                from,
                m.to,
                short(from)
            ),
            None => "new".to_string(),
        };
        let _ = writeln!(out, "- {link}: {since}");
    }
    out
}

fn short(reference: &str) -> &str {
    if is_commit(reference) {
        &reference[..7]
    } else {
        reference
    }
}

// -- `brigid release` ---------------------------------------------------------

fn status() -> Result<()> {
    let mut to_release = Vec::new();
    for product in &PRODUCTS {
        let plan = plan(product, "main", Bump::Auto, false)?;
        let previous = plan
            .previous
            .map_or_else(|| "no release yet".to_string(), |v| format!("v{v}"));
        match (plan.asked, plan.next) {
            (Some(_), Some(next)) => {
                to_release.push(product.name);
                println!("{}  {previous} → v{next}", product.name);
            }
            _ => println!("{}  {previous}: nothing to release", product.name),
        }
        let mut merged = 0;
        for moved in &plan.moved {
            if moved.changes.is_empty() {
                continue;
            }
            merged += moved.changes.len();
            let seen: Vec<&Change> = moved.changes.iter().filter(|c| c.seen()).collect();
            println!(
                "  {:<16} {} commit(s), {} a person would notice",
                moved.repo,
                moved.changes.len(),
                seen.len()
            );
            for change in seen {
                let mark = if change.breaking { "⚠️ " } else { "" };
                println!("    - {mark}{}", change.subject);
            }
        }
        if merged == 0 && plan.previous.is_some() {
            println!("  nothing merged since {previous}");
        }
        // What is merged in a component and not followed by the server yet.
        for moved in &plan.moved {
            let Some(pin) = moved.pin else { continue };
            let ahead = changes_between(moved.repo, &moved.to, "main")?;
            let seen = ahead.iter().filter(|c| c.seen()).count();
            if seen > 0 {
                println!(
                    "  {:<16} its main has {seen} change(s) {} does not follow yet ({})",
                    moved.repo,
                    product.name,
                    pin.how_to_move()
                );
            }
        }
        println!();
    }
    if to_release.is_empty() {
        println!("No release to make.");
    } else {
        for name in to_release {
            println!(
                "To release: brigid release cut {name}   (its notes: brigid release notes {name})"
            );
        }
    }
    Ok(())
}

// -- `brigid release cut` -----------------------------------------------------

fn cut(product: &'static Product, bump: Bump, force: bool, dry_run: bool, yes: bool) -> Result<()> {
    let plan = plan(product, "main", bump, force)?;
    let Some(next) = plan.next else {
        bail!(
            "nothing changed for the people who run {} since v{}: no release \
             (`--force` makes one anyway)",
            product.name,
            plan.previous.map(|v| v.to_string()).unwrap_or_default()
        );
    };
    println!("{} v{next}\n\n{}", product.name, plan.notes);
    if !yes {
        let question = if dry_run {
            format!("Build {} v{next} without publishing it?", product.name)
        } else {
            format!("Release {} v{next}?", product.name)
        };
        if !inquire::Confirm::new(&question)
            .with_default(false)
            .prompt()?
        {
            println!("Nothing started.");
            return Ok(());
        }
    }
    let full = format!("{ORG}/{}", product.repo);
    let bump = match bump {
        Bump::Auto => "auto",
        Bump::Patch => "patch",
        Bump::Minor => "minor",
        Bump::Major => "major",
    };
    gh(&[
        "workflow",
        "run",
        "release.yml",
        "-R",
        &full,
        "--ref",
        "main",
        "-f",
        &format!("bump={bump}"),
        "-f",
        &format!("force={force}"),
        "-f",
        &format!("dry-run={dry_run}"),
    ])?;
    println!("Started: https://github.com/{full}/actions/workflows/release.yml");
    Ok(())
}

// -- GitHub, through `gh` -----------------------------------------------------

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

/// The highest `vX.Y.Z` among the repository's releases.
fn latest_version(repo: &str) -> Result<Option<Version>> {
    let raw = gh(&[
        "api",
        "--paginate",
        &format!("repos/{ORG}/{repo}/releases?per_page=100"),
        "--jq",
        ".[] | select(.draft | not) | .tag_name",
    ])?;
    Ok(raw.lines().filter_map(Version::parse).max())
}

/// The commit a branch, a tag or a commit names.
fn commit_of(repo: &str, reference: &str) -> Result<String> {
    Ok(gh(&[
        "api",
        &format!("repos/{ORG}/{repo}/commits/{reference}"),
        "--jq",
        ".sha",
    ])?
    .trim()
    .to_string())
}

/// The commits of `repo` that are in `to` and not in `from`, oldest first.
fn changes_between(repo: &str, from: &str, to: &str) -> Result<Vec<Change>> {
    let mut changes = Vec::new();
    for page in 1.. {
        let raw = gh(&[
            "api",
            &format!("repos/{ORG}/{repo}/compare/{from}...{to}?per_page=100&page={page}"),
        ])?;
        let compare: Value = serde_json::from_str(&raw)?;
        let commits = compare["commits"].as_array().map_or(&[][..], Vec::as_slice);
        for commit in commits {
            changes.push(Change::parse(
                repo,
                commit["sha"].as_str().unwrap_or(""),
                commit["commit"]["message"].as_str().unwrap_or(""),
            ));
        }
        let total = compare["total_commits"].as_u64().unwrap_or(0) as usize;
        if commits.is_empty() || changes.len() >= total {
            break;
        }
    }
    Ok(changes)
}

/// The commit of `component` that `repo` follows at `reference`.
fn pin_at(repo: &str, reference: &str, component: &str, pin: Pin) -> Result<Option<String>> {
    let path = format!("repos/{ORG}/{repo}/contents/{}?ref={reference}", pin.file());
    let text = gh(&["api", "-H", "Accept: application/vnd.github.raw", &path])?;
    Ok(match pin {
        Pin::CargoLock => lock_pin(&text, component),
        Pin::AppRef => app_ref(&text),
    })
}

/// `source = "git+https://github.com/brig-id/<component>?branch=main#<commit>"`
fn lock_pin(cargo_lock: &str, component: &str) -> Option<String> {
    let source = format!("git+https://github.com/{ORG}/{component}");
    cargo_lock.lines().find_map(|line| {
        let rest = &line[line.find(&source)? + source.len()..];
        if !rest.starts_with(['?', '#']) {
            return None; // a longer name, `core-extra`
        }
        let commit = rest.rsplit_once('#')?.1.trim_end_matches('"');
        is_commit(commit).then(|| commit.to_string())
    })
}

/// `APP_REF: <commit>`, whatever follows it on the line.
fn app_ref(workflow: &str) -> Option<String> {
    workflow.lines().find_map(|line| {
        let commit = line
            .trim()
            .strip_prefix("APP_REF:")?
            .split_whitespace()
            .next()?;
        is_commit(commit).then(|| commit.to_string())
    })
}

fn is_commit(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn change(message: &str) -> Change {
        Change::parse("core", SHA, message)
    }

    #[test]
    fn reads_a_commit() {
        let c = change("feat(api): ✨ read and set the limits\n\nbody");
        assert_eq!(c.kind, Kind::Feature);
        assert!(!c.breaking);
        assert_eq!(c.subject, "read and set the limits");
        assert_eq!(change("fix: 🐛 no scope").kind, Kind::Fix);
        assert_eq!(change("perf(store): ⚡️ faster").kind, Kind::Performance);
        assert_eq!(
            change("revert(deps): ⏪ core by its tag").kind,
            Kind::Revert
        );
    }

    #[test]
    fn what_a_person_would_not_notice() {
        for message in [
            "docs(docs): 📝 the guide",
            "chore(release): 🔖 release v0.3.2",
            "build(deps): ⬆️ update dependencies",
            "ci(ci): 👷 a job",
            "test(api): ✅ slack",
            "Merge branch 'x'",
            "feat(ci): ✨ a new job is not a feature of the server",
            "fix(e2e): 🐛 a flaky test",
        ] {
            assert!(!change(message).seen(), "{message}");
        }
    }

    #[test]
    fn a_break_is_seen_whatever_its_type() {
        assert!(change("refactor(api)!: ♻️ rename the route").breaking);
        assert!(change("feat(api): ✨ x\n\nBREAKING CHANGE: the route moved").breaking);
        assert!(change("chore(api)!: 🔧 drop the old column").seen());
    }

    #[test]
    fn a_subject_without_emoji_is_kept_whole() {
        assert_eq!(change("fix(api): refuse it").subject, "refuse it");
        assert_eq!(change("no convention here").subject, "no convention here");
    }

    #[test]
    fn versions() {
        assert_eq!(Version::parse("v0.12.2"), Some(Version(0, 12, 2)));
        assert_eq!(Version::parse("0.1.0"), Some(Version(0, 1, 0)));
        assert_eq!(Version::parse("v1.2"), None);
        assert_eq!(Version::parse("v1.2.3-rc.1"), None);
        assert_eq!(Version::parse("seedling-v1.2.3"), None);
        assert!(Version(0, 12, 2) > Version(0, 9, 10));
        assert_eq!(Version(0, 12, 2).bumped(Bump::Patch).to_string(), "0.12.3");
        assert_eq!(Version(0, 12, 2).bumped(Bump::Minor).to_string(), "0.13.0");
        assert_eq!(Version(0, 12, 2).bumped(Bump::Major).to_string(), "1.0.0");
    }

    #[test]
    fn before_one_point_zero_a_feature_is_a_patch() {
        let young = Version(0, 12, 2);
        let feature = [change("feat(api): ✨ x")];
        let fix = [change("fix(api): 🐛 x")];
        let breaking = [change("feat(api)!: 💥 x")];
        let unseen = [change("docs(docs): 📝 x"), change("build(deps): ⬆️ x")];
        assert_eq!(step_for(&feature, young), Some(Bump::Patch));
        assert_eq!(step_for(&fix, young), Some(Bump::Patch));
        assert_eq!(step_for(&breaking, young), Some(Bump::Minor));
        assert_eq!(step_for(&unseen, young), None);
        assert_eq!(step_for(&[], young), None);

        let stable = Version(1, 4, 0);
        assert_eq!(step_for(&feature, stable), Some(Bump::Minor));
        assert_eq!(step_for(&fix, stable), Some(Bump::Patch));
        assert_eq!(step_for(&breaking, stable), Some(Bump::Major));
    }

    #[test]
    fn no_release_without_a_reason_unless_forced() {
        let last = Some(Version(0, 12, 2));
        assert_eq!(next_version(last, None, Bump::Auto, false), None);
        assert_eq!(next_version(last, None, Bump::Minor, false), None);
        assert_eq!(
            next_version(last, None, Bump::Auto, true),
            Some(Version(0, 12, 3))
        );
        assert_eq!(
            next_version(last, None, Bump::Minor, true),
            Some(Version(0, 13, 0))
        );
        assert_eq!(
            next_version(last, Some(Bump::Patch), Bump::Auto, false),
            Some(Version(0, 12, 3))
        );
        // The step chosen by hand wins over the one the commits ask for.
        assert_eq!(
            next_version(last, Some(Bump::Patch), Bump::Minor, false),
            Some(Version(0, 13, 0))
        );
        assert_eq!(
            next_version(None, None, Bump::Auto, false),
            Some(Version(0, 1, 0))
        );
    }

    #[test]
    fn reads_the_commit_a_lock_file_follows() {
        let lock = r#"
[[package]]
name = "brigid-api"
version = "0.3.1"
source = "git+https://github.com/brig-id/core?tag=v0.3.1#074c008922d0015962dc39a65a912654a14d2c44"

[[package]]
name = "brigid-crypto"
version = "0.2.0"
source = "git+https://github.com/brig-id/crypto?branch=main#0f78de89cba0b302b6d5c7bfb328226c3bf1bdf8"

[[package]]
name = "other"
source = "git+https://github.com/brig-id/core-extra#1111111111111111111111111111111111111111"
"#;
        assert_eq!(
            lock_pin(lock, "core").as_deref(),
            Some("074c008922d0015962dc39a65a912654a14d2c44")
        );
        assert_eq!(
            lock_pin(lock, "crypto").as_deref(),
            Some("0f78de89cba0b302b6d5c7bfb328226c3bf1bdf8")
        );
        assert_eq!(lock_pin(lock, "app"), None);
    }

    #[test]
    fn reads_the_interface_a_server_follows() {
        let with_tag = "env:\n  APP_REF: a6fbc98615825c8830ad9af36471acc3126e87c9 # v0.11.1\n";
        let bare = "env:\n  APP_REF: a6fbc98615825c8830ad9af36471acc3126e87c9\n";
        for text in [with_tag, bare] {
            assert_eq!(
                app_ref(text).as_deref(),
                Some("a6fbc98615825c8830ad9af36471acc3126e87c9")
            );
        }
        assert_eq!(app_ref("ref: ${{ env.APP_REF }}"), None);
    }

    fn moved(repo: &'static str, from: &str, to: &str, messages: &[&str]) -> Moved {
        Moved {
            repo,
            from: Some(from.to_string()),
            to: to.to_string(),
            changes: messages
                .iter()
                .map(|message| Change::parse(repo, SHA, message))
                .collect(),
            pin: None,
        }
    }

    #[test]
    fn the_notes_say_what_changed_in_every_repository() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let notes = notes(
            &PRODUCTS[0],
            Some(Version(0, 12, 2)),
            &[
                moved(
                    "server-seedling",
                    "v0.12.2",
                    &a,
                    &["build(deps): ⬆️ the core at its main"],
                ),
                moved(
                    "core",
                    &b,
                    &a,
                    &[
                        "feat(api): ✨ read the limits",
                        "fix(store)!: 🐛 the column is renamed",
                    ],
                ),
                moved("app", &a, &a, &[]),
            ],
        );
        assert!(notes.contains(
            "### ⚠️ Changes that need attention\n\n- the column is renamed ([core@0123456]"
        ));
        assert!(notes.contains("### Features\n\n- read the limits ([core@0123456]"));
        assert!(!notes.contains("the core at its main"));
        assert!(notes.contains("- [app@aaaaaaa](https://github.com/brig-id/app/commit/"));
        assert!(notes.contains("unchanged"));
        assert!(notes.contains("[2 commits](https://github.com/brig-id/core/compare/"));
    }

    #[test]
    fn the_notes_of_a_release_nobody_would_notice_say_so() {
        let a = "a".repeat(40);
        let notes = notes(
            &PRODUCTS[1],
            Some(Version(0, 5, 2)),
            &[moved(
                "server-grove",
                "v0.5.2",
                &a,
                &["docs(docs): 📝 the guide"],
            )],
        );
        assert!(notes.starts_with("Nothing changes for the people who run grove"));
    }
}
