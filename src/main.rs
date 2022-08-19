// dependencies
use clap::Parser;

pub mod neo4j_client;
pub mod cloud_config;
pub mod ec2;

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
async fn main() -> Result<(), aws_sdk_config::Error> {
    let Opt { region, verbose, user, pass, uri } = Opt::parse();

    if verbose {
        tracing_subscriber::fmt::init();
    }

    let _graph = neo4j_client::graph_client::setup_client(user, pass, uri).await;
    let _configs = cloud_config::collector::runner(verbose, region).await?;
    let (running_insts, offline_insts) = ec2::collector::runner(region).await?;

    Ok(())
}