use crate::Settings;
use crate::atlas::collection::CollectionReport;
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::projector;
use crate::cloud::amazon::provider::build_aws;
use crate::cloud::azure::provider::build_azure;
use crate::cloud::cloudflare::provider::build_cloudflare;
use crate::cloud::google::provider::build_gcp;
use petgraph::dot::Dot;
use std::time::Duration;

/// One collection pass: the projected graph plus what could not be read while
/// producing it.
pub struct Scan {
    pub builder: GraphBuilder,
    pub report: CollectionReport,
}

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
    ///
    /// The returned [`Scan`] carries a [`CollectionReport`] alongside the graph.
    /// A source that errors is recorded there rather than silently yielding an
    /// empty collection, so callers can tell "this cloud has no such resources"
    /// apart from "we could not read this cloud" — the distinction the live
    /// differ needs to avoid deleting resources on a transient API failure.
    pub async fn collect(&self) -> Scan {
        let mut builder = GraphBuilder::new();
        let mut report = CollectionReport::default();

        let aws_future = async {
            if !self.settings.regions.is_empty() {
                Some(build_aws(self.settings.verbose, &self.settings).await)
            } else {
                None
            }
        };

        let gcp_future = async {
            if let Some(projects) = &self.settings.gcp_projects
                && !projects.is_empty()
            {
                return Some(build_gcp(self.settings.verbose, &self.settings).await);
            }
            None
        };

        let azure_future = async {
            if let Some(subs) = &self.settings.azure_subscriptions
                && !subs.is_empty()
            {
                return Some(build_azure(self.settings.verbose, &self.settings).await);
            }
            None
        };

        let cloudflare_future = async {
            if self.settings.cloudflare {
                Some(build_cloudflare(self.settings.verbose, &self.settings).await)
            } else {
                None
            }
        };

        let (aws_res, gcp_res, azure_res, cloudflare_res) =
            tokio::join!(aws_future, gcp_future, azure_future, cloudflare_future);

        for scan in [aws_res, gcp_res, azure_res, cloudflare_res]
            .into_iter()
            .flatten()
        {
            projector::build(&mut builder, &scan.provider, &self.settings);
            report.merge(scan.report);
        }

        Scan { builder, report }
    }

    /// Full point-in-time refresh used by the CLI: re-collect and export to
    /// disk. Still a wipe-and-rebuild (no diffing) — the live server is the
    /// incremental path.
    pub async fn update_graph(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let scan = self.collect().await;
        if !scan.report.is_complete() {
            eprintln!(
                "Warning: collection was incomplete, the exported graph is partial -- {}",
                scan.report.summary()
            );
        }
        self.builder = scan.builder;
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
