use crate::cloud::amazon::provider;
use crate::atlas::projector;
use clap::Parser;
use petgraph::dot::{Config, Dot};
use std::fs;

pub mod cloud;
pub mod atlas;
pub mod neo4j_client;

#[derive(Debug, Parser)]
#[clap(about, version, long_about = None)]
struct Opt {
    /// The AWS Region.
    #[clap(short, long, default_value = "us-east-1")]
    region: String,

    /// The Neo4J Username.
    #[clap(short, long, default_value = "neo4j")]
    user: String,

    /// The Neo4J Password.
    #[clap(short, long, default_value = "password")]
    pass: String,

    /// The Neo4J URI.
    #[clap(long, default_value = "127.0.0.1:7687")]
    uri: String,

    /// Whether to display additional information.
    #[clap(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Opt {
        region,
        verbose,
        user,
        pass,
        uri,
    } = Opt::parse();

    if verbose {
        tracing_subscriber::fmt::init();
    }

    let _graph = neo4j_client::graph_client::setup_client(user, pass, uri).await?;
    let aws_provider = provider::build_aws(verbose, region.clone()).await?;


    // println!("AWS Config: {:#?}", aws_provider);
    let g = projector::build(&aws_provider, region.as_str());
    //dbg!(g);
    let s = format!("{:?}", Dot::with_config(&g, &[Config::EdgeNoLabel]));
    fs::write("atlas.dot", s)?;

    Ok(())
}
