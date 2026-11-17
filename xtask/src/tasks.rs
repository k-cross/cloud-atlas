use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::SystemTime;

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
    repo_root().join("atlas-web")
}

fn older_than_sources(artifact: &Path, sources: &[PathBuf]) -> bool {
    let built = artifact.metadata().and_then(|m| m.modified()).ok();
    let newest = sources.iter().filter_map(|p| newest_mtime(p)).max();
    match (newest, built) {
        (Some(src), Some(art)) => src > art,
        _ => true,
    }
}

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

pub fn ensure_wasm(force: bool) -> Result<(), String> {
    let artifact = web_dir().join("static/pkg/atlas_layout_wasm_bg.wasm");

    let stale = force
        || older_than_sources(
            &artifact,
            &crate_sources(&["atlas-layout", "atlas-layout-wasm"]),
        );

    if stale {
        println!("wasm layout engine is stale — rebuilding pkg/");
        run(&web_dir(), "bun", &["run", "wasm"])
    } else {
        println!("wasm layout engine is up to date (use `cargo xtask wasm --force` to rebuild)");
        Ok(())
    }
}

fn demo_json() -> PathBuf {
    repo_root().join("multi_cloud_demo.json")
}

const DEMO_SOURCE_CRATES: &[&str] = &[
    "atlas-graph",
    "atlas-provider",
    "atlas-aws",
    "atlas-azure",
    "atlas-cloudflare",
    "atlas-gcp",
    "atlas-engine",
];

fn crate_dir(name: &str) -> PathBuf {
    repo_root().join("crates").join(name)
}

fn crate_sources(names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .flat_map(|name| {
            [
                crate_dir(name).join("src"),
                crate_dir(name).join("Cargo.toml"),
            ]
        })
        .collect()
}

pub fn ensure_demo_snapshot() -> Result<(), String> {
    let mut sources = crate_sources(DEMO_SOURCE_CRATES);
    sources.push(crate_dir("atlas-engine").join("examples"));
    if older_than_sources(&demo_json(), &sources) {
        println!("multi_cloud_demo.json is missing or older than its source crates — regenerating");
        demo_snapshot()
    } else {
        publish_demo_snapshot()
    }
}

pub fn demo_snapshot() -> Result<(), String> {
    run(
        &repo_root(),
        "cargo",
        &["run", "-p", "atlas-engine", "--example", "demo"],
    )?;
    publish_demo_snapshot()
}

fn publish_demo_snapshot() -> Result<(), String> {
    let target = web_dir().join("static/snapshot.json");
    if !older_than_sources(&target, &[demo_json()]) {
        return Ok(());
    }
    std::fs::copy(demo_json(), &target).map_err(|e| {
        format!(
            "failed to copy the demo snapshot to {}: {e}",
            target.display()
        )
    })?;
    println!("refreshed {} (the UI's offline snapshot)", target.display());
    Ok(())
}

pub fn test(e2e: bool) -> Result<(), String> {
    let root = repo_root();
    cargo_test(&root, &["--workspace"])?;

    run(&web_dir(), "bun", &["run", "lint"])?;
    run(&web_dir(), "bun", &["run", "check"])?;
    run(&web_dir(), "bun", &["run", "test:unit"])?;
    if e2e {
        ensure_wasm(false)?;
        ensure_demo_snapshot()?;
        run(&root, "cargo", &["build", "-p", "atlas-server"])?;
        run(&web_dir(), "bun", &["run", "test:e2e"])?;
    }
    println!(
        "\nall test suites passed{}",
        if e2e { " (including e2e)" } else { "" }
    );
    Ok(())
}
