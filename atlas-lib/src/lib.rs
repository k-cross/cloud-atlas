use crate::atlas::projector;
use crate::cloud::amazon::provider;

pub mod atlas;
pub mod cloud;

#[derive(Debug)]
pub struct Settings {
    /// The AWS Region.
    regions: Vec<String>,

    /// Include all mappings by default
    all: bool,

    /// Whether to display additional information.
    verbose: bool,
}

pub async fn graph(opts: Settings) -> Result<petgraph::DiGraphMap<&'a str, u8>, Box<dyn std::error::Error>> {
    if opts.verbose {
        tracing_subscriber::fmt::init();
    }

    let aws_provider = provider::build_aws(opts.verbose, &opts).await?;

    // TODO: log output
    if opts.verbose {
        dbg!(&aws_provider);
    }

    let g = projector::build(&aws_provider, &opts);

    Ok(g)
}
