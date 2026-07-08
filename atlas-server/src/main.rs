//! Cloud Atlas live backend.
//!
//! Owns the in-memory graph, reconciles it against the providers on an
//! interval (`poll`), and pushes incremental patches to the frontend over
//! WebSocket (`ws`). See `docs/change_monitoring_design.md` §7.

mod http;
mod poll;
mod state;
mod ws;

use crate::poll::Source;
use crate::state::AppState;
use atlas_lib::atlas::engine::AtlasEngine;
use atlas_lib::fixtures;
use clap::Parser;
use std::time::Duration;

#[derive(Debug, Parser)]
#[clap(about = "Cloud Atlas live backend server", version, long_about = None)]
pub struct Opt {
    /// The AWS Regions to collect from.
    #[clap(short, long, value_parser, num_args = 1.., default_values = vec!["us-east-1"])]
    regions: Vec<String>,

    /// The GCP Projects.
    #[clap(long, value_parser, num_args = 1..)]
    gcp_projects: Option<Vec<String>>,

    /// The Azure Subscriptions.
    #[clap(long, value_parser, num_args = 1..)]
    azure_subscriptions: Option<Vec<String>>,

    /// Whether to include Cloudflare resources.
    #[clap(long)]
    cloudflare: bool,

    /// Include all mappings by default.
    #[clap(short, long)]
    all: bool,

    /// Whether to display additional information.
    #[clap(short, long)]
    verbose: bool,

    /// Serve the credential-free "Globex" fixtures with a live-changing sentinel
    /// instead of collecting from real clouds. For local development and demos.
    #[clap(long)]
    demo: bool,

    /// TCP port to listen on.
    #[clap(long, default_value_t = 4681)]
    port: u16,

    /// Seconds between reconciliation scans.
    #[clap(long, default_value_t = 60)]
    poll_secs: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "atlas_server=info".into()),
        )
        .init();

    // Seed the graph once up front so the very first client gets a populated
    // snapshot, and choose the reconciliation source.
    let (initial, source) = if opt.demo {
        tracing::info!("demo mode: serving credential-free Globex fixtures");
        (fixtures::build_graph().graph, Source::Demo)
    } else {
        let settings = atlas_lib::Settings {
            regions: opt.regions,
            gcp_projects: opt.gcp_projects,
            azure_subscriptions: opt.azure_subscriptions,
            cloudflare: opt.cloudflare,
            all: opt.all,
            verbose: opt.verbose,
            exclude_by_default: false,
        };
        let engine = AtlasEngine::new(settings);
        let initial = engine.collect().await.graph;
        (initial, Source::Live(engine))
    };

    let state = AppState::new(initial);
    let app = http::router(state.clone());
    let addr = format!("0.0.0.0:{}", opt.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("atlas-server listening on http://{addr} (ws://{addr}/ws)");

    // The reconciliation loop is the sole writer of the live graph. It runs on
    // this task (not `tokio::spawn`) because provider collection carries
    // non-`Send` errors; `select!` still drives it concurrently with the
    // server, and either finishing tears the process down.
    let poller = poll::run(state, source, Duration::from_secs(opt.poll_secs));
    tokio::select! {
        result = axum::serve(listener, app) => result?,
        _ = poller => {}
    }
    Ok(())
}
