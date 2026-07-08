//! One-shot tasks and the shared command-running plumbing.
//!
//! Everything here shells out to the commands a developer would type by hand
//! (`cargo test`, `bun test`, `bun run wasm`, …) — xtask orchestrates, it never
//! reimplements.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Repo root, derived from this crate's location (`<root>/xtask`), so xtask
/// works no matter which directory `cargo xtask` is invoked from.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level under the repo root")
        .to_path_buf()
}

/// Run `program args…` in `dir`, streaming output, failing loudly on non-zero.
pub fn run(dir: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    println!("\n▶ {} {} (in {})", program, args.join(" "), dir.display());
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| format!("failed to start {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed: {status}", args.join(" ")))
    }
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
    let artifact = web_dir().join("pkg/atlas_layout_wasm_bg.wasm");

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
    run(&root, "cargo", &["test", "--workspace"])?;
    run(&render_dir(), "cargo", &["test"])?;
    run(&web_dir(), "bun", &["test", "./test"])?;
    run(&web_dir(), "bun", &["run", "typecheck"])?;
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
