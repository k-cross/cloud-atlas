mod demo;
mod http;
mod poll;
mod state;
mod stream;
mod ws;

use crate::poll::Source;
use crate::state::AppState;
use atlas_lib::atlas::collection::CollectionReport;
use atlas_lib::atlas::engine::AtlasEngine;
use atlas_lib::atlas::flow::FlowIndex;
use atlas_lib::atlas::patch::Retention;
use clap::Parser;
use std::time::Duration;

#[derive(Debug, Parser)]
#[clap(about = "Cloud Atlas live backend server", version, long_about = None)]
pub struct Opt {
    #[clap(short, long, value_parser, num_args = 1.., default_values = vec!["us-east-1"])]
    regions: Vec<String>,

    #[clap(long, value_parser, num_args = 1..)]
    gcp_projects: Option<Vec<String>>,

    #[clap(long, value_parser, num_args = 1..)]
    azure_subscriptions: Option<Vec<String>>,

    #[clap(long)]
    cloudflare: bool,

    #[clap(short, long)]
    verbose: bool,

    #[clap(long)]
    demo: bool,

    #[clap(long, default_value_t = 4681)]
    port: u16,

    #[clap(long, default_value_t = 60)]
    poll_secs: u64,

    #[clap(long, default_value_t = Retention::DEFAULT_BUDGET)]
    retain_scans: u32,

    #[clap(long)]
    aws_event_queue: Option<String>,

    #[clap(long)]
    aws_flow_log_queue: Option<String>,

    #[clap(long)]
    flow_ttl_secs: Option<u64>,
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

    let event_region = opt.regions.first().cloned().unwrap_or_default();

    let (mut initial, initial_report, source) = if opt.demo {
        tracing::info!("demo mode: serving credential-free Globex fixtures");
        (demo::graph(0), CollectionReport::default(), Source::Demo)
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
            tracing::warn!(
                failures = scan.report.failures.len(),
                "initial collection incomplete, serving a partial baseline: {}",
                scan.report.summary()
            );
        }
        (scan.builder, scan.report, Source::Live(Box::new(engine)))
    };

    let flows = match (&opt.aws_flow_log_queue, opt.demo) {
        (Some(queue_url), false) => {
            tracing::info!(region = %event_region, "consuming AWS VPC flow logs from {queue_url}");
            stream::FlowSource::aws(&event_region, queue_url).await
        }

        (Some(_), true) => {
            tracing::warn!("--aws-flow-log-queue is ignored in --demo mode");
            stream::FlowSource::Disabled
        }
        (None, _) => stream::FlowSource::Disabled,
    };

    let events = match (&opt.aws_event_queue, opt.demo) {
        (Some(queue_url), false) => {
            tracing::info!(region = %event_region, "consuming AWS change events from {queue_url}");
            stream::Source::aws(&event_region, queue_url, false).await
        }

        (Some(_), true) => {
            tracing::warn!("--aws-event-queue is ignored in --demo mode");
            stream::Source::Disabled
        }
        (None, _) => stream::Source::Disabled,
    };

    let poll_interval = Duration::from_secs(opt.poll_secs);
    let flow_ttl = opt.flow_ttl_secs.map_or_else(
        || source.default_flow_ttl(poll_interval),
        Duration::from_secs,
    );
    tracing::info!(ttl = ?flow_ttl, "observed flows count as current for");
    let mut flow_index = FlowIndex::new(flow_ttl, FlowIndex::DEFAULT_CAPACITY);
    source.seed_flows(&mut flow_index);
    flow_index.overlay(&mut initial);
    let state = AppState::new(initial, initial_report, flow_index);
    let app = http::router(state.clone());
    let addr = format!("0.0.0.0:{}", opt.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("atlas-server listening on http://{addr} (ws://{addr}/ws)");

    let poller = poll::run(
        state,
        source,
        events,
        flows,
        poll_interval,
        Retention::new(opt.retain_scans),
    );
    tokio::select! {
        result = axum::serve(listener, app) => result?,
        _ = poller => {}
    }
    Ok(())
}
