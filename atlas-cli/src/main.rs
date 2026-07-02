use atlas_lib::atlas::projector;
use atlas_lib::cloud::amazon::provider;
use clap::Parser;
use petgraph::dot::Dot;
use std::fs;

#[derive(Debug, Parser)]
#[clap(about, version, long_about = None)]
pub struct Opt {
    /// The AWS Region.
    #[clap(short, long, value_parser, num_args = 1.., default_values = vec!["us-east-1"])]
    regions: Vec<String>,

    /// The GCP Projects.
    #[clap(long, value_parser, num_args = 1..)]
    gcp_projects: Option<Vec<String>>,

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
        all: opts.all,
        verbose: opts.verbose,
        exclude_by_default: opts.exclude,
    };

    if opts.daemon {
        println!("Starting in daemon mode. Polling for changes every 60 seconds...");
        loop {
            // As per requirements: only fetch AWS if regions are specified (not empty).
            // When GCP/Azure are added, we would check for their respective flags here.
            let mut graph = petgraph::graph::Graph::new();

            if !settings.regions.is_empty() {
                let aws_provider = provider::build_aws(settings.verbose, &settings).await?;
                let g = projector::build(&aws_provider, &settings);
                // We'd ideally merge this if there are multiple providers
                graph = g;
            }

            if let Some(projects) = &settings.gcp_projects
                && !projects.is_empty()
            {
                let gcp_provider =
                    atlas_lib::cloud::google::provider::build_gcp(settings.verbose, &settings)
                        .await?;
                let g = projector::build(&gcp_provider, &settings);
                // Just replace graph for now if AWS isn't merged
                graph = g;
            }

            let s = format!("{}", Dot::with_config(&graph, &[]));
            fs::write("atlas.dot", s)?;
            println!("Graph updated successfully at atlas.dot");
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    } else {
        let mut graph = petgraph::graph::Graph::new();

        if !settings.regions.is_empty() {
            let aws_provider = provider::build_aws(settings.verbose, &settings).await?;
            let g = projector::build(&aws_provider, &settings);
            graph = g;
        }

        if let Some(projects) = &settings.gcp_projects
            && !projects.is_empty()
        {
            let gcp_provider =
                atlas_lib::cloud::google::provider::build_gcp(settings.verbose, &settings).await?;
            let g = projector::build(&gcp_provider, &settings);
            graph = g;
        }

        let s = format!("{}", Dot::with_config(&graph, &[]));
        fs::write("atlas.dot", s)?;
    }

    Ok(())
}
