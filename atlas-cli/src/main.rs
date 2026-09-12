use atlas_lib::atlas::engine::AtlasEngine;
use clap::Parser;

#[derive(Debug, Parser)]
#[clap(about, version, long_about = None)]
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
        short,
        long,
        hide(true),
        help = "Whether to exclude non-explicitly defined values by default."
    )]
    exclude: bool,

    #[clap(
        short,
        long,
        help = "Run as a long-running daemon that updates continuously."
    )]
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
