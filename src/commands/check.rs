//! `brigid check` — verifies the prerequisites for `brigid dev` are in place.

use std::path::Path;
use std::process::Command;

use crate::repos::workspace_root;

pub struct CheckResult {
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

/// Runs every check, printing a pass/fail line for each. Returns `true` iff
/// every hard-required check passed (soft/optional issues don't fail the run).
pub fn run() -> bool {
    let results = all_checks();
    let mut all_ok = true;

    for result in &results {
        let symbol = if result.ok { "✓" } else { "✗" };
        println!("{symbol} {}: {}", result.label, result.detail);
        all_ok &= result.ok;
    }

    all_ok
}

pub fn all_checks() -> Vec<CheckResult> {
    vec![
        command_check("rustc", "rustc", &["--version"]),
        command_check("cargo", "cargo", &["--version"]),
        command_check("node", "node", &["--version"]),
        command_check("corepack", "corepack", &["--version"]),
        command_check("mprocs", "mprocs", &["--version"]),
        mkcert_binary_check(),
        mkcert_ca_check(),
        app_cert_check(),
        master_key_check(),
    ]
}

fn command_check(label: &str, program: &str, args: &[&str]) -> CheckResult {
    match Command::new(program).args(args).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let detail = stdout.lines().next().unwrap_or("").trim().to_string();
            CheckResult {
                label: label.to_string(),
                ok: true,
                detail,
            }
        }
        _ => CheckResult {
            label: label.to_string(),
            ok: false,
            detail: format!(
                "`{program}` not found on PATH — see roots/.devcontainer/setup-container.sh"
            ),
        },
    }
}

fn mkcert_binary_check() -> CheckResult {
    command_check("mkcert", "mkcert", &["-version"])
}

fn mkcert_ca_check() -> CheckResult {
    let label = "mkcert CA".to_string();
    let Ok(out) = Command::new("mkcert").arg("-CAROOT").output() else {
        return CheckResult {
            label,
            ok: false,
            detail: "mkcert not installed — run `brigid setup`".to_string(),
        };
    };
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let installed = !root.is_empty() && Path::new(&root).join("rootCA.pem").is_file();
    CheckResult {
        label,
        ok: installed,
        detail: if installed {
            format!("installed at {root}")
        } else {
            "not installed — run `brigid setup` (or `mkcert -install`)".to_string()
        },
    }
}

fn app_cert_check() -> CheckResult {
    let cert_dir = workspace_root().join("app").join(".cert");
    let cert = cert_dir.join("brigid.localhost.pem");
    let key = cert_dir.join("brigid.localhost-key.pem");
    let ok = cert.is_file() && key.is_file();
    CheckResult {
        label: "app/.cert/brigid.localhost".to_string(),
        ok,
        detail: if ok {
            "present".to_string()
        } else {
            "missing — run `brigid setup`".to_string()
        },
    }
}

fn master_key_check() -> CheckResult {
    let env_path = crate::env_file::path();
    let label = "cli/.env BRIGID_MASTER_KEY".to_string();
    match crate::env_file::read_master_key(&env_path) {
        Some(key) if key.len() == 64 && key.chars().all(|c| c.is_ascii_hexdigit()) => CheckResult {
            label,
            ok: true,
            detail: "present (64 hex chars)".to_string(),
        },
        Some(_) => CheckResult {
            label,
            ok: false,
            detail: "present but not 64 hex chars — run `brigid setup` to regenerate".to_string(),
        },
        None => CheckResult {
            label,
            ok: false,
            detail: "missing — run `brigid setup`".to_string(),
        },
    }
}
