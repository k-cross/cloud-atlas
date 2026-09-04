//! Cloud Atlas live backend.
//!
//! Owns the in-memory graph, keeps it current from two directions — the
//! Tier-1 event feed (`stream`) and the Tier-3 reconciliation scan, both driven
//! by the single writer in `poll` — and pushes incremental patches to the
//! frontend over WebSocket (`ws`). See `docs/change_monitoring_design.md` §7.

mod http;
mod poll;
mod state;
mod stream;
mod ws;

use crate::poll::Source;
use crate::state::AppState;
use atlas_lib::atlas::collection::CollectionReport;
use atlas_lib::atlas::engine::AtlasEngine;
use atlas_lib::atlas::patch::Retention;
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

    /// How many consecutive incomplete scans a provider's resources are held
    /// through before the graph stops waiting and deletes what it cannot
    /// confirm. 0 deletes unconfirmed resources immediately; a large value
    /// holds them effectively forever.
    #[clap(long, default_value_t = Retention::DEFAULT_BUDGET)]
    retain_scans: u32,

    /// URL of an SQS queue fed by an EventBridge rule, to consume as the Tier-1
    /// live change feed (AWS Config items, EC2 state changes, CloudTrail
    /// management events).
    ///
    /// Optional: without it the graph is still correct, just at poll-interval
    /// latency instead of seconds. The queue is read in the first `--regions`
    /// region, which is where its EventBridge rule lives.
    #[clap(long)]
    aws_event_queue: Option<String>,
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

    // The Tier-1 feed is read in the region whose EventBridge rule targets the
    // queue: the first configured region, before `regions` is moved into the
    // engine's settings.
    let event_region = opt.regions.first().cloned().unwrap_or_default();

    // Seed the graph once up front so the very first client gets a populated
    // snapshot, and choose the reconciliation source.
    let (initial, initial_report, source) = if opt.demo {
        tracing::info!("demo mode: serving credential-free Globex fixtures");
        (
            fixtures::build_graph(),
            CollectionReport::default(),
            Source::Demo,
        )
    } else {
        let settings = atlas_lib::Settings {
            regions: opt.regions,
            gcp_projects: opt.gcp_projects,
            azure_subscriptions: opt.azure_subscriptions,
            cloudflare: opt.cloudflare,
            verbose: opt.verbose,
            exclude_by_default: false,
        };
        let engine = AtlasEngine::new(settings);
        let scan = engine.collect().await;
        if !scan.report.is_complete() {
            // The baseline every client starts from is this graph, so say
            // loudly that it is partial and keep the report on the state --
            // GET /collection.json is how a client tells "nothing there" from
            // "we could not look".
            tracing::warn!(
                failures = scan.report.failures.len(),
                "initial collection incomplete, serving a partial baseline: {}",
                scan.report.summary()
            );
        }
        (scan.builder, scan.report, Source::Live(Box::new(engine)))
    };

    let events = match (&opt.aws_event_queue, opt.demo) {
        (Some(queue_url), false) => {
            tracing::info!(region = %event_region, "consuming AWS change events from {queue_url}");
            stream::Source::aws(&event_region, queue_url, false).await
        }
        // A queue is meaningless in demo mode, where nothing reaches AWS at
        // all; say so rather than silently ignoring the flag.
        (Some(_), true) => {
            tracing::warn!("--aws-event-queue is ignored in --demo mode");
            stream::Source::Disabled
        }
        (None, _) => stream::Source::Disabled,
    };

    let state = AppState::new(initial, initial_report);
    let app = http::router(state.clone());
    let addr = format!("0.0.0.0:{}", opt.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("atlas-server listening on http://{addr} (ws://{addr}/ws)");

    // The reconciliation loop is the sole writer of the live graph. It runs on
    // this task (not `tokio::spawn`) because provider collection carries
    // non-`Send` errors; `select!` still drives it concurrently with the
    // server, and either finishing tears the process down.
    let poller = poll::run(
        state,
        source,
        events,
        Duration::from_secs(opt.poll_secs),
        Retention::new(opt.retain_scans),
    );
    tokio::select! {
        result = axum::serve(listener, app) => result?,
        _ = poller => {}
    }
    Ok(())
}
