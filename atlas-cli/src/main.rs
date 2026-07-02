use atlas_lib::atlas::engine::AtlasEngine;
use clap::Parser;

#[derive(Debug, Parser)]
#[clap(about, version, long_about = None)]
pub struct Opt {
    /// The AWS Region.
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

    /// Include all mappings by default
    #[clap(short, long)]
    all: bool,

    /// Whether to display additional information.
    #[clap(short, long)]
    verbose: bool,

    /// Whether to exclude non-explicitly defined values by default
    #[clap(short, long, hide(true))]
    exclude: bool,

    /// Run as a long-running daemon that updates continuously
    #[clap(short, long)]
    daemon: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opt::parse();

    if opts.verbose {
        tracing_subscriber::fmt::init();
    }

    let settings = atlas_lib::Settings {
        regions: opts.regions,
        gcp_projects: opts.gcp_projects,
        azure_subscriptions: opts.azure_subscriptions,
        cloudflare: opts.cloudflare,
        all: opts.all,
        verbose: opts.verbose,
        exclude_by_default: opts.exclude,
    };

    let mut engine = AtlasEngine::new(settings);

    if opts.daemon {
        engine.run_daemon(60).await?;
    } else {
        engine.run_once().await?;
    }

    Ok(())
}
