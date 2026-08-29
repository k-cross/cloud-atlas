//! One-shot tasks and the shared command-running plumbing.
//!
//! Everything here shells out to the commands a developer would type by hand
//! (`cargo test`, `bun test`, `bun run wasm`, …) — xtask orchestrates, it never
//! reimplements.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::SystemTime;

/// Repo root, derived from this crate's location (`<root>/xtask`), so xtask
/// works no matter which directory `cargo xtask` is invoked from.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level under the repo root")
        .to_path_buf()
}

pub fn strip_inherited_cargo_env(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        let injected = key.starts_with("CARGO_PKG_")
            || matches!(
                key.as_str(),
                "CARGO"
                    | "CARGO_BIN_NAME"
                    | "CARGO_CRATE_NAME"
                    | "CARGO_MAKEFLAGS"
                    | "CARGO_MANIFEST_DIR"
                    | "CARGO_MANIFEST_LINKS"
                    | "CARGO_MANIFEST_PATH"
                    | "CARGO_PRIMARY_PACKAGE"
            );
        if injected {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// Run `program args…` in `dir`, streaming output, failing loudly on non-zero.
pub fn run(dir: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    println!("\n▶ {} {} (in {})", program, args.join(" "), dir.display());
    let status = strip_inherited_cargo_env(Command::new(program).args(args).current_dir(dir))
        .status()
        .map_err(|e| format!("failed to start {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed: {status}", args.join(" ")))
    }
}

fn nextest_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("cargo")
            .args(["nextest", "--version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

pub fn cargo_test(dir: &Path, scope: &[&str]) -> Result<(), String> {
    let mut args = if nextest_available() {
        vec!["nextest", "run"]
    } else {
        vec!["test"]
    };
    args.push("--all-targets");
    args.extend_from_slice(scope);
    run(dir, "cargo", &args)
}

fn web_dir() -> PathBuf {
    repo_root().join("atlas-render/atlas-web")
}

fn render_dir() -> PathBuf {
    repo_root().join("atlas-render")
}

/// Newest mtime of any file under `path` (recursively), or None if empty/absent.
fn newest_mtime(path: &Path) -> Option<SystemTime> {
    if path.is_file() {
        return path.metadata().and_then(|m| m.modified()).ok();
    }
    let mut newest = None;
    for entry in std::fs::read_dir(path).ok()?.flatten() {
        if let Some(t) = newest_mtime(&entry.path()) {
            newest = Some(newest.map_or(t, |n: SystemTime| n.max(t)));
        }
    }
    newest
}

/// Rebuild the wasm layout engine (`pkg/`) when its Rust sources are newer than
/// the built artifact — the guard against the stale-wasm class of bug where a
/// `SNAPSHOT_VERSION` bump in atlas-layout silently isn't reflected in the
/// engine the browser loads.
pub fn ensure_wasm(force: bool) -> Result<(), String> {
    let root = repo_root();
    let artifact = web_dir().join("static/pkg/atlas_layout_wasm_bg.wasm");

    let stale = force || !artifact.exists() || {
        let built = artifact.metadata().and_then(|m| m.modified()).ok();
        let sources = [
            root.join("atlas-render/atlas-layout/src"),
            root.join("atlas-render/atlas-layout/Cargo.toml"),
            root.join("atlas-render/atlas-layout-wasm/src"),
            root.join("atlas-render/atlas-layout-wasm/Cargo.toml"),
        ];
        let newest = sources.iter().filter_map(|p| newest_mtime(p)).max();
        match (newest, built) {
            (Some(src), Some(art)) => src > art,
            _ => true,
        }
    };

    if stale {
        println!("wasm layout engine is stale — rebuilding pkg/");
        run(&web_dir(), "bun", &["run", "wasm"])
    } else {
        println!("wasm layout engine is up to date (use `cargo xtask wasm --force` to rebuild)");
        Ok(())
    }
}

/// Generate the credential-free Globex demo snapshot if it's missing.
pub fn ensure_demo_snapshot() -> Result<(), String> {
    let root = repo_root();
    if root.join("multi_cloud_demo.json").exists() {
        return Ok(());
    }
    println!("multi_cloud_demo.json missing — generating from fixtures");
    run(
        &root,
        "cargo",
        &["run", "-p", "atlas-lib", "--example", "demo"],
    )
}

pub fn demo_snapshot() -> Result<(), String> {
    run(
        &repo_root(),
        "cargo",
        &["run", "-p", "atlas-lib", "--example", "demo"],
    )
}

/// Every test suite, in dependency order, fail-fast. All credential-free.
pub fn test(e2e: bool) -> Result<(), String> {
    let root = repo_root();
    cargo_test(&root, &["--workspace"])?;
    cargo_test(&render_dir(), &[])?;
    // Biome (format + lint) for JS/TS/JSON/CSS, svelte-check for types/.svelte,
    // then the frontend unit tests (pure graph/style logic).
    run(&web_dir(), "bun", &["run", "lint"])?;
    run(&web_dir(), "bun", &["run", "check"])?;
    run(&web_dir(), "bun", &["run", "test:unit"])?;
    if e2e {
        // Playwright drives the real wasm + demo snapshot; make sure both exist
        // and are fresh before spending browser time.
        ensure_wasm(false)?;
        ensure_demo_snapshot()?;
        run(&web_dir(), "bun", &["run", "test:e2e"])?;
    }
    println!(
        "\nall test suites passed{}",
        if e2e { " (including e2e)" } else { "" }
    );
    Ok(())
}
