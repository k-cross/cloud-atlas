use clap::Parser;
use petgraph::dot::{Config, Dot};
use std::fs;
use atlas_lib::atlas::projector;
use atlas_lib::cloud::amazon::provider;

#[derive(Debug, Parser)]
#[clap(about, version, long_about = None)]
pub struct Opt {
    /// The AWS Region.
    #[clap(short, long, value_parser, num_args = 1.., default_values = vec!["us-east-1"])]
    regions: Vec<String>,

    /// Include all mappings by default
    #[clap(short, long)]
    all: bool,

    /// Whether to display additional information.
    #[clap(short, long)]
    verbose: bool,

    /// Whether to exclude non-explicitly defined values by default
    #[clap(short, long, hide(true))]
    exclude: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opt::parse();

    if opts.verbose {
        tracing_subscriber::fmt::init();
    }

    let settings = atlas_lib::Settings {
        regions: opts.regions,
        all: opts.all,
        verbose: opts.verbose,
        exclude_by_default: opts.exclude,
    };

    let aws_provider = provider::build_aws(settings.verbose, &settings).await?;
    let g = projector::build(&aws_provider, &settings);
    let s = format!("{:?}", Dot::with_config(&g, &[Config::EdgeNoLabel]));
    fs::write("atlas.dot", s)?;

    Ok(())
}
