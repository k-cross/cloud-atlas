use crate::amazon::{instance, resource};
use crate::atlas::projector;
use clap::Parser;

pub mod amazon;
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
    // TODO remove once provider is built
    let configs = resource::collector::runner(verbose, region.as_str()).await?;
    let (running_insts, _offline_insts) = instance::collector::runner(region.as_str()).await?;

    println!("AWS Config: {:#?}", configs);
    //println!("AWS Instances: {:#?}", running_insts);
    let g = projector::build(provider, region);
    dbg!(g);

    Ok(())
}
