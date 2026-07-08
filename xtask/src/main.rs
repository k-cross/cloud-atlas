//! Workspace orchestration (`cargo xtask …`), the unified mechanism for
//! running the server + renderer + frontend together and for the cross-
//! workspace build/test chores that otherwise live in tribal knowledge.
//!
//! Invoked through the alias in `.cargo/config.toml`. Everything shells out to
//! the same commands a developer would run by hand — see `tasks.rs` — and the
//! `dev` supervisor lives in `dev.rs`.

mod dev;
mod tasks;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[clap(about = "Cloud Atlas workspace orchestration", long_about = None)]
struct Opt {
    #[clap(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the full dev stack: wasm engine (rebuilt if stale) → atlas-server →
    /// frontend dev server, supervised together. Real collection by default;
    /// --demo for the credential-free fixtures.
    Dev {
        /// Serve the credential-free Globex fixtures instead of real clouds.
        #[clap(long)]
        demo: bool,

        /// AWS regions to collect from (server default: us-east-1).
        #[clap(long, num_args = 1..)]
        regions: Vec<String>,

        /// GCP projects to collect from.
        #[clap(long, num_args = 1..)]
        gcp_projects: Vec<String>,

        /// Azure subscriptions to collect from.
        #[clap(long, num_args = 1..)]
        azure_subscriptions: Vec<String>,

        /// Include Cloudflare resources (needs CLOUDFLARE_API_TOKEN).
        #[clap(long)]
        cloudflare: bool,

        /// atlas-server port (the frontend's default WebSocket target).
        #[clap(long, default_value_t = 4681)]
        port: u16,

        /// Seconds between server reconciliation scans.
        #[clap(long, default_value_t = 60)]
        poll_secs: u64,

        /// Frontend dev-server port.
        #[clap(long, default_value_t = 4680)]
        web_port: u16,

        /// Skip the wasm staleness check/rebuild.
        #[clap(long)]
        skip_wasm: bool,
    },

    /// Rebuild the wasm layout engine (pkg/) if its sources are newer than the
    /// built artifact.
    Wasm {
        /// Rebuild unconditionally.
        #[clap(long)]
        force: bool,
    },

    /// Generate the credential-free Globex demo snapshot (multi_cloud_demo.json).
    Demo,

    /// Run every test suite in order: cargo (root workspace), cargo
    /// (atlas-render), bun unit tests, typecheck — and optionally Playwright.
    Test {
        /// Also run the browser end-to-end suite (static + live WebSocket).
        #[clap(long)]
        e2e: bool,
    },
}

fn main() {
    let result = match Opt::parse().command {
        Cmd::Dev {
            demo,
            regions,
            gcp_projects,
            azure_subscriptions,
            cloudflare,
            port,
            poll_secs,
            web_port,
            skip_wasm,
        } => dev::dev(dev::DevOpts {
            demo,
            regions,
            gcp_projects,
            azure_subscriptions,
            cloudflare,
            port,
            poll_secs,
            web_port,
            skip_wasm,
        }),
        Cmd::Wasm { force } => tasks::ensure_wasm(force),
        Cmd::Demo => tasks::demo_snapshot(),
        Cmd::Test { e2e } => tasks::test(e2e),
    };

    if let Err(e) = result {
        eprintln!("\nerror: {e}");
        std::process::exit(1);
    }
}
