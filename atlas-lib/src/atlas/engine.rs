use crate::Settings;
use crate::atlas::collection::CollectionReport;
use crate::atlas::containment;
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::patch::{Retention, carry_forward};
use crate::atlas::projector;
use crate::cloud::amazon::provider::build_aws;
use crate::cloud::azure::provider::build_azure;
use crate::cloud::cloudflare::provider::build_cloudflare;
use crate::cloud::google::provider::build_gcp;
use petgraph::dot::Dot;
use std::time::Duration;

pub struct Scan {
    pub builder: GraphBuilder,
    pub report: CollectionReport,
}

pub struct AtlasEngine {
    settings: Settings,
    builder: GraphBuilder,
    retention: Retention,
}

impl AtlasEngine {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            builder: GraphBuilder::new(),
            retention: Retention::default(),
        }
    }

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

    pub async fn update_graph(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let scan = self.collect().await;
        self.install(scan);
        self.export_graph().await?;
        Ok(())
    }

    fn install(&mut self, mut scan: Scan) {
        let held = self.retention.hold(&scan.report);
        if !scan.report.is_complete() {
            eprintln!(
                "Warning: collection was incomplete ({} source(s) retained) -- {}",
                held.len(),
                scan.report.summary()
            );
            for released in scan.report.unreadable_sources().difference(&held) {
                eprintln!(
                    "Warning: {released} unreadable for {} consecutive scans, dropping its \
                     unconfirmed resources",
                    self.retention.streak(*released)
                );
            }
        }
        if !held.is_empty() {
            carry_forward(&mut scan.builder, &self.builder.graph, &held);
        }
        containment::link(&mut scan.builder);

        self.builder = scan.builder;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::collection::{CollectionSource, FailureKind};
    use crate::atlas::definition::Node;

    fn seeded(nodes: [Node; 2]) -> GraphBuilder {
        let mut builder = GraphBuilder::new();
        for node in nodes {
            builder.get_or_add_node(node);
        }
        builder
    }

    fn instance() -> Node {
        Node::AwsEc2Instance("i-daemon".into())
    }

    fn zone() -> Node {
        Node::CloudflareZone("zone-daemon".into())
    }

    #[test]
    fn an_incomplete_scan_does_not_export_phantom_deletions() {
        let mut engine = AtlasEngine::new(Settings::default());
        engine.builder = seeded([instance(), zone()]);

        let mut report = CollectionReport::default();
        report.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/ec2",
            "throttled",
        );
        let scan = Scan {
            builder: GraphBuilder::new(),
            report,
        };

        engine.install(scan);

        let kept: Vec<&Node> = engine.builder.graph.node_weights().collect();
        assert_eq!(
            kept,
            vec![&instance()],
            "AWS was unreadable so its instance is retained; Cloudflare was read \
             and its zone is genuinely gone"
        );
    }

    #[test]
    fn a_complete_scan_replaces_the_graph_wholesale() {
        let mut engine = AtlasEngine::new(Settings::default());
        engine.builder = seeded([instance(), zone()]);

        engine.install(Scan {
            builder: GraphBuilder::new(),
            report: CollectionReport::default(),
        });

        assert_eq!(engine.builder.graph.node_count(), 0);
    }
}
