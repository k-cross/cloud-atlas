use crate::Settings;
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::projector;
use crate::cloud::amazon::provider::build_aws;
use crate::cloud::azure::provider::build_azure;
use crate::cloud::cloudflare::provider::build_cloudflare;
use crate::cloud::google::provider::build_gcp;
use petgraph::dot::Dot;
use std::time::Duration;

pub struct AtlasEngine {
    settings: Settings,
    builder: GraphBuilder,
}

impl AtlasEngine {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            builder: GraphBuilder::new(),
        }
    }

    /// Fetch from every configured provider and project into a fresh
    /// `GraphBuilder`, without mutating `self` or writing any files. This is the
    /// reusable core: the CLI wraps it with file export, and the live server
    /// diffs its output against the persistent graph.
    pub async fn collect(&self) -> GraphBuilder {
        let mut builder = GraphBuilder::new();

        let aws_future = async {
            if !self.settings.regions.is_empty() {
                build_aws(self.settings.verbose, &self.settings).await.ok()
            } else {
                None
            }
        };

        let gcp_future = async {
            if let Some(projects) = &self.settings.gcp_projects
                && !projects.is_empty()
            {
                return build_gcp(self.settings.verbose, &self.settings).await.ok();
            }
            None
        };

        let azure_future = async {
            if let Some(subs) = &self.settings.azure_subscriptions
                && !subs.is_empty()
            {
                return build_azure(self.settings.verbose, &self.settings)
                    .await
                    .ok();
            }
            None
        };

        let cloudflare_future = async {
            if self.settings.cloudflare {
                build_cloudflare(self.settings.verbose, &self.settings)
                    .await
                    .ok()
            } else {
                None
            }
        };

        let (aws_opt, gcp_opt, azure_opt, cloudflare_opt) =
            tokio::join!(aws_future, gcp_future, azure_future, cloudflare_future);

        if let Some(aws_provider) = aws_opt {
            projector::build(&mut builder, &aws_provider, &self.settings);
        }
        if let Some(gcp_provider) = gcp_opt {
            projector::build(&mut builder, &gcp_provider, &self.settings);
        }
        if let Some(azure_provider) = azure_opt {
            projector::build(&mut builder, &azure_provider, &self.settings);
        }
        if let Some(cloudflare_provider) = cloudflare_opt {
            projector::build(&mut builder, &cloudflare_provider, &self.settings);
        }

        builder
    }

    /// Full point-in-time refresh used by the CLI: re-collect and export to
    /// disk. Still a wipe-and-rebuild (no diffing) — the live server is the
    /// incremental path.
    pub async fn update_graph(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.builder = self.collect().await;
        self.export_graph().await?;
        Ok(())
    }

    pub async fn run_once(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.update_graph().await
    }

    pub async fn run_daemon(
        &mut self,
        interval_secs: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!(
            "Starting in daemon mode. Polling for changes every {} seconds...",
            interval_secs
        );
        loop {
            if let Err(e) = self.update_graph().await {
                eprintln!("Error updating graph: {:?}", e);
            }
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        }
    }

    async fn export_graph(&self) -> Result<(), Box<dyn std::error::Error>> {
        let s = format!("{}", Dot::with_config(&self.builder.graph, &[]));
        tokio::fs::write("atlas.dot", s).await?;
        let json = crate::atlas::export::snapshot_json(&self.builder.graph)?;
        tokio::fs::write("atlas.json", json).await?;
        println!("Graph updated successfully at atlas.dot (render snapshot: atlas.json)");
        Ok(())
    }
}
