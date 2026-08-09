# AGENTS.md — brig·id `cli`

This repository contains `brigid` — the brig·id **dev orchestrator CLI**: prerequisite
checks, local HTTPS/master-key setup, a split-pane process launcher, and cross-repo
convenience commands. It is dev tooling that never ships as product code — the CLI
equivalent of the shell scripts it replaced.

## Language

**All content must be in English** — code, comments, doc-comments, commit messages,
issues, pull requests. No exceptions.

## Scope

- `check` — verify local-dev prerequisites are installed
- `setup` — fix what `check` finds missing (mkcert, dev cert, MASTER_KEY)
- `dev` — interactively launch dev processes side by side (via `mprocs`)
- `repos` — cross-repo convenience commands (`status`, `fetch`, `pull`, `branch`,
  `install`, `build`, `test`, `lint`) run against the sibling repos cloned by
  [`brig-id/roots`](https://github.com/brig-id/roots)'s devcontainer

This repo does not host product runtime code, orchestration config, or the
devcontainer itself — those live in `roots`. `cli` is cloned as a sibling alongside
the other product repos by `roots`'s `postCreateCommand`, and built with the same
shared Rust toolchain; it has no `.devcontainer/` of its own.

## Common commands

```bash
cargo build --release
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check

# Usage (once built / on PATH)
brigid check
brigid setup
brigid dev
brigid repos <status|fetch|pull|branch|install|build|test|lint>
```

## Roadmap & planning

TODOs, backlog ideas, and phase/release tracking live in the org's GitHub Project,
not in local files: **[brig-id Project 1](https://github.com/orgs/brig-id/projects/1)**.
Open a card there for new work instead of adding a local TODO/roadmap file.

## Commit conventions

Format: `type(scope): <emoji> description`

| Type | Emoji | When |
| --- | --- | --- |
| `feat` | ✨ | New feature or file |
| `fix` | 🐛 | Correction |
| `docs` | 📝 | Documentation only |
| `chore` | 🔧 | Maintenance, config |
| `ci` | 👷 | CI/CD |
| `revert` | ⏪ | Reverts a previous commit |

### Allowed scopes

Scopes are declared in `.vscode/settings.json` (`conventionalCommits.scopes`) — this
repo has no `scopes.json`, matching the other single-concern product repos
(`crypto`, `core`, `server-leaf`, `spec`) rather than `roots`, which is the only repo
covering multiple unrelated concerns.

| Scope | Maps to |
| --- | --- |
| `check` | `src/commands/check.rs` |
| `setup` | `src/commands/setup.rs` |
| `dev` | `src/commands/dev.rs` |
| `repos` | `src/commands/repos.rs`, `src/repos.rs` |
| `ci` | `.github/workflows/` |
| `deps` | Dependency bumps |

**Do not use a scope outside this list.** If a new top-level concern is added,
update this table and `.vscode/settings.json` together.

```text
feat(dev): ✨ add --profile flag to select which process set to launch
fix(setup): 🐛 handle missing mkcert on PATH with a clear error
ci(ci): 👷 add conventional commit check
```

## Git Workflow

brig·id ships to production, so branches go through an intermediate stage before `main`.
Every merge is **rebase + fast-forward only** — no merge commits, no squash merges, anywhere.

**Branches:**

| Branch | Purpose | Lifetime |
| --- | --- | --- |
| `main` | Production | Permanent |
| `dev/*` (e.g. `dev/ember`) | Internal/staging release train | One per cycle — deleted after merging into `main` |
| `hotfix/*` | Urgent production fix, bypasses `dev/*` | One per fix — deleted after merging into `main` |
| `feat/*`, `bug/*` | Regular work | One per change — deleted after merging into the current `dev/*` |

**Merging (always via PR, never a direct push to `main` or `dev/*`):**

- `feat/*` / `bug/*` → rebase onto the current `dev/*` tip, then fast-forward merge into `dev/*`.
- `dev/*` → rebase onto `main`'s tip, then fast-forward merge into `main`.
- `hotfix/*` → branched from `main`, rebase onto `main`'s tip, then fast-forward merge into `main`.
- If a `hotfix/*` lands on `main` while a `dev/*` is still in flight, rebase that `dev/*` onto the
  new `main` before its own merge — fast-forward tolerates no divergence.
- Releases are tracked with **tags on `main`** (there's no merge commit to mark them, since every
  merge is a fast-forward).

## Origin

Split out of [`brig-id/.dev`](https://github.com/brig-id/.dev) (retired), which
originally combined workspace/devcontainer orchestration (now `roots`) and this CLI
in one repo.
