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
    #[clap(short, long, value_parser, num_args = 1.., default_values = vec!["us-east-1"], help = "The AWS Regions to collect from.")]
    regions: Vec<String>,

    #[clap(long, value_parser, num_args = 1.., help = "The GCP Projects.")]
    gcp_projects: Option<Vec<String>>,

    #[clap(long, value_parser, num_args = 1.., help = "The Azure Subscriptions.")]
    azure_subscriptions: Option<Vec<String>>,

    #[clap(long, help = "Whether to include Cloudflare resources.")]
    cloudflare: bool,

    #[clap(short, long, help = "Whether to display additional information.")]
    verbose: bool,

    #[clap(
        long,
        help = "Serve the credential-free \"Globex\" fixtures instead of collecting from real clouds.",
        long_help = "Serve the credential-free \"Globex\" fixtures with a live-changing sentinel \
                     instead of collecting from real clouds. For local development and demos."
    )]
    demo: bool,

    #[clap(long, default_value_t = 4681, help = "TCP port to listen on.")]
    port: u16,

    #[clap(
        long,
        default_value_t = 60,
        help = "Seconds between reconciliation scans."
    )]
    poll_secs: u64,

    #[clap(
        long,
        default_value_t = Retention::DEFAULT_BUDGET,
        help = "How many consecutive incomplete scans a provider's resources are held through.",
        long_help = "How many consecutive incomplete scans a provider's resources are held \
                     through before the graph stops waiting and deletes what it cannot confirm. \
                     0 deletes unconfirmed resources immediately; a large value holds them \
                     effectively forever."
    )]
    retain_scans: u32,

    #[clap(
        long,
        help = "SQS queue URL fed by an EventBridge rule, consumed as the Tier-1 live change feed.",
        long_help = "URL of an SQS queue fed by an EventBridge rule, to consume as the Tier-1 \
                     live change feed (AWS Config items, EC2 state changes, CloudTrail management \
                     events).\n\nOptional: without it the graph is still correct, just at \
                     poll-interval latency instead of seconds. The queue is read in the first \
                     --regions region, which is where its EventBridge rule lives."
    )]
    aws_event_queue: Option<String>,

    #[clap(
        long,
        help = "SQS queue URL subscribed to a VPC Flow Logs bucket, consumed as the Tier-2 liveness feed.",
        long_help = "URL of an SQS queue subscribed to a VPC Flow Logs bucket's S3 event \
                     notifications, to consume as the Tier-2 liveness feed.\n\nOptional, and \
                     orthogonal to the other two tiers: without it the graph is complete and \
                     correct but carries no liveness, so nothing can say whether any of it is \
                     actually passing traffic."
    )]
    aws_flow_log_queue: Option<String>,

    #[clap(
        long,
        help = "How long an observed flow counts as current.",
        long_help = "How long an observed flow counts as current. Past this, the traffic edge is \
                     removed and the resources it touched stop reporting as live.\n\nUnset, the \
                     collection source answers: fifteen minutes for a real flow feed, generous on \
                     purpose relative to flow logs' own aggregation and delivery lag, which is \
                     minutes."
    )]
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
