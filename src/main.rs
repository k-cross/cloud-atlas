use crate::atlas::projector;
use crate::cloud::amazon::provider;
use clap::Parser;
use petgraph::dot::{Config, Dot};
use std::fs;

pub mod atlas;
pub mod cloud;

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opt::parse();

    if opts.verbose {
        tracing_subscriber::fmt::init();
    }

    let aws_provider = provider::build_aws(opts.verbose, &opts).await?;

    // TODO: log output
    if opts.verbose {
        dbg!(&aws_provider);
    }

    let g = projector::build(&aws_provider, &opts);

    // TODO: log output
    if opts.verbose {
        dbg!(&g);
    }

    let s = format!("{:?}", Dot::with_config(&g, &[Config::EdgeNoLabel]));
    fs::write("atlas.dot", s)?;

    Ok(())
}
