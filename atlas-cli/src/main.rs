use atlas_lib::atlas::graph_builder::GraphBuilder;
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

    /// The Azure Subscriptions.
    #[clap(long, value_parser, num_args = 1..)]
    azure_subscriptions: Option<Vec<String>>,

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
        all: opts.all,
        verbose: opts.verbose,
        exclude_by_default: opts.exclude,
    };

    if opts.daemon {
        println!("Starting in daemon mode. Polling for changes every 60 seconds...");
        loop {
            let mut builder = GraphBuilder::new();

            let aws_future = async {
                if !settings.regions.is_empty() {
                    provider::build_aws(settings.verbose, &settings).await.ok()
                } else {
                    None
                }
            };

            let gcp_future = async {
                if let Some(projects) = &settings.gcp_projects
                    && !projects.is_empty()
                {
                    return atlas_lib::cloud::google::provider::build_gcp(
                        settings.verbose,
                        &settings,
                    )
                    .await
                    .ok();
                }
                None
            };

            let azure_future = async {
                if let Some(subs) = &settings.azure_subscriptions
                    && !subs.is_empty()
                {
                    return atlas_lib::cloud::azure::provider::build_azure(
                        settings.verbose,
                        &settings,
                    )
                    .await
                    .ok();
                }
                None
            };

            let (aws_opt, gcp_opt, azure_opt) = tokio::join!(aws_future, gcp_future, azure_future);

            if let Some(aws_provider) = aws_opt {
                projector::build(&mut builder, &aws_provider, &settings);
            }
            if let Some(gcp_provider) = gcp_opt {
                projector::build(&mut builder, &gcp_provider, &settings);
            }
            if let Some(azure_provider) = azure_opt {
                projector::build(&mut builder, &azure_provider, &settings);
            }

            let s = format!("{}", Dot::with_config(&builder.graph, &[]));
            fs::write("atlas.dot", s)?;
            println!("Graph updated successfully at atlas.dot");
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    } else {
        let mut builder = GraphBuilder::new();

        let aws_future = async {
            if !settings.regions.is_empty() {
                provider::build_aws(settings.verbose, &settings).await.ok()
            } else {
                None
            }
        };

        let gcp_future = async {
            if let Some(projects) = &settings.gcp_projects
                && !projects.is_empty()
            {
                return atlas_lib::cloud::google::provider::build_gcp(settings.verbose, &settings)
                    .await
                    .ok();
            }
            None
        };

        let azure_future = async {
            if let Some(subs) = &settings.azure_subscriptions
                && !subs.is_empty()
            {
                return atlas_lib::cloud::azure::provider::build_azure(settings.verbose, &settings)
                    .await
                    .ok();
            }
            None
        };

        let (aws_opt, gcp_opt, azure_opt) = tokio::join!(aws_future, gcp_future, azure_future);

        if let Some(aws_provider) = aws_opt {
            projector::build(&mut builder, &aws_provider, &settings);
        }
        if let Some(gcp_provider) = gcp_opt {
            projector::build(&mut builder, &gcp_provider, &settings);
        }
        if let Some(azure_provider) = azure_opt {
            projector::build(&mut builder, &azure_provider, &settings);
        }

        let s = format!("{}", Dot::with_config(&builder.graph, &[]));
        fs::write("atlas.dot", s)?;
    }

    Ok(())
}
