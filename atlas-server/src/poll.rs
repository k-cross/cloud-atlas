//! Tier-3 reconciliation loop: periodically re-derive the whole graph, diff it
//! against the live one, and broadcast the change set. This is the same
//! full-scan we do today, minus the wipe — the incremental primary feeds (event
//! streams, flow logs) plug in on top of this later.

use crate::state::AppState;
use atlas_lib::atlas::collection::{CollectionReport, CollectionSource};
use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::engine::AtlasEngine;
use atlas_lib::atlas::graph_builder::GraphBuilder;
use atlas_lib::atlas::patch::{GraphPatch, Retention, carry_forward, diff};
use atlas_lib::fixtures;
use petgraph::graph::Graph;
use std::collections::HashSet;
use std::time::Duration;

/// Where each reconciliation tick's graph comes from.
pub enum Source {
    /// Real collection from configured cloud providers.
    Live(Box<AtlasEngine>),
    /// Credential-free fixtures with a sentinel that flips in and out every
    /// other tick, so the live-patch path is exercised without any cloud calls.
    Demo,
}

impl Source {
    /// Produce the graph for tick `n`, together with what could not be read
    /// while producing it. The demo source is always complete -- it never
    /// leaves the process.
    async fn scan(&self, tick: u64) -> (GraphBuilder, CollectionReport) {
        match self {
            Source::Live(engine) => {
                let scan = engine.collect().await;
                (scan.builder, scan.report)
            }
            Source::Demo => (demo_graph(tick), CollectionReport::default()),
        }
    }
}

/// The fixtures graph, plus a small connected sentinel pair on odd ticks. The
/// resulting alternation (add on odd, remove on even) makes every kind of patch
/// — added/removed nodes and edges — flow past a connected frontend.
fn demo_graph(tick: u64) -> GraphBuilder {
    let mut builder = fixtures::build_graph();
    if tick % 2 == 1 {
        let host = builder.get_or_add_node(Node::GenericHostname("live-demo.internal".into()));
        let ip = builder.get_or_add_node(Node::GenericIpAddress("198.51.100.42".into()));
        builder.add_edge(host, ip, Edge::ResolvesTo);
    }
    builder
}

/// Turn a scan into the patch to broadcast. A complete scan is authoritative
/// and diffs straight through, removals included. For each source in `held`,
/// that source's live resources are folded forward first, so the tick is
/// additive-only *for that source* while every other provider keeps deleting
/// normally -- `next` is left as the exact graph the caller should install.
///
/// `held` is what [`Retention`] decided, not simply what failed: a source that
/// has been unreadable for too long is no longer held, so the graph converges
/// instead of waiting forever on a collector that never recovers.
fn reconcile(
    live: &Graph<Node, Edge>,
    next: &mut GraphBuilder,
    held: &HashSet<CollectionSource>,
) -> GraphPatch {
    if !held.is_empty() {
        carry_forward(next, live, held);
    }
    diff(live, &next.graph)
}

/// Run forever, reconciling every `interval`. Only non-empty diffs mutate the
/// live graph or hit the broadcast channel.
pub async fn run(state: AppState, source: Source, interval: Duration, retention: Retention) {
    let mut retention = retention;
    let mut tick: u64 = 0;
    loop {
        tokio::time::sleep(interval).await;
        tick += 1;

        let (mut next, report) = source.scan(tick).await;
        let held = retention.hold(&report);

        if !report.is_complete() {
            tracing::warn!(
                tick,
                failures = report.failures.len(),
                "collection incomplete, holding unconfirmed resources: {}",
                report.summary()
            );
            for released in report.unreadable_sources().difference(&held) {
                tracing::warn!(
                    tick,
                    source = %released,
                    scans = retention.streak(*released),
                    "source unreadable for too many consecutive scans; releasing its \
                     unconfirmed resources to the differ",
                );
            }
        }

        // Diff under the read lock — no full-graph clone, and the critical
        // section is just the comparison. WebSocket readers share the lock.
        let patch = {
            let live = state.live.read().await;
            reconcile(&live, &mut next, &held)
        };

        // Published even when the graph is unchanged: a provider going dark
        // changes what the snapshot *means* without changing a single node.
        *state.report.write().await = report;

        if patch.is_empty() {
            continue;
        }

        tracing::info!(
            tick,
            added_nodes = patch.added_nodes.len(),
            removed_nodes = patch.removed_nodes.len(),
            added_edges = patch.added_edges.len(),
            removed_edges = patch.removed_edges.len(),
            "graph changed",
        );
        *state.live.write().await = next.graph;
        // Err only means no subscribers are connected — nothing to do.
        let _ = state.patches.send(patch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use atlas_lib::atlas::collection::{CollectionReport, CollectionSource};

    fn without_kind(graph: &Graph<Node, Edge>, kind: &str) -> GraphBuilder {
        let mut trimmed = graph.clone();
        trimmed.retain_nodes(|g, i| g[i].kind() != kind);
        let mut builder = GraphBuilder::new();
        builder.merge(&trimmed);
        builder
    }

    fn nothing_held() -> HashSet<CollectionSource> {
        HashSet::new()
    }

    /// AWS held back -- it owns the kind these tests drop.
    fn holding_aws() -> HashSet<CollectionSource> {
        HashSet::from([CollectionSource::Aws])
    }

    fn aws_throttled() -> CollectionReport {
        let mut report = CollectionReport::default();
        report.record(CollectionSource::Aws, "us-east-1/ec2", "throttled");
        report
    }

    #[test]
    fn an_incomplete_scan_never_removes() {
        let live = demo_graph(1).graph;
        let dropped = "AwsEc2Instance";

        let mut next = without_kind(&live, dropped);
        assert!(
            next.graph.node_count() < live.node_count(),
            "fixture must contain the kind this test drops"
        );

        let complete_patch = reconcile(&live, &mut without_kind(&live, dropped), &nothing_held());
        assert!(
            !complete_patch.removed_nodes.is_empty(),
            "a complete scan is authoritative and must still delete"
        );

        let patch = reconcile(&live, &mut next, &holding_aws());
        assert!(
            patch.removed_nodes.is_empty() && patch.removed_edges.is_empty(),
            "an incomplete scan deleted {} nodes / {} edges",
            patch.removed_nodes.len(),
            patch.removed_edges.len()
        );
        assert_eq!(
            next.graph.node_count(),
            live.node_count(),
            "unconfirmed resources must be carried into the installed graph"
        );
    }

    /// Retention protects against a *transient* failure. A collector that fails
    /// on every tick must not pin its resources in the graph forever, or
    /// deletions never converge for that provider.
    #[test]
    fn a_source_that_never_recovers_stops_blocking_removals() {
        let live = demo_graph(1).graph;
        let mut retention = Retention::new(2);

        for scan in 1..=2 {
            let held = retention.hold(&aws_throttled());
            let patch = reconcile(&live, &mut without_kind(&live, "AwsEc2Instance"), &held);
            assert!(
                patch.removed_nodes.is_empty(),
                "scan {scan} is still within budget and must not delete"
            );
        }

        let held = retention.hold(&aws_throttled());
        assert!(
            held.is_empty(),
            "the budget is spent, AWS is no longer held"
        );

        let patch = reconcile(&live, &mut without_kind(&live, "AwsEc2Instance"), &held);
        assert!(
            !patch.removed_nodes.is_empty(),
            "past its budget, an unreadable source must stop blocking removals"
        );
    }

    #[test]
    fn an_incomplete_scan_still_applies_additions() {
        let live = demo_graph(2).graph;
        let mut next = without_kind(&demo_graph(3).graph, "AwsEc2Instance");

        let patch = reconcile(&live, &mut next, &holding_aws());

        assert_eq!(
            patch.added_nodes.len(),
            2,
            "sentinel additions from the sources that did respond must land"
        );
        assert_eq!(patch.added_edges.len(), 1);
        assert!(patch.removed_nodes.is_empty() && patch.removed_edges.is_empty());
    }

    #[test]
    fn a_complete_scan_is_unaffected_by_carry_forward() {
        let live = demo_graph(2).graph;
        let mut next = demo_graph(3);
        let same = next.graph.clone();

        let with_policy = reconcile(&live, &mut next, &nothing_held());
        let plain = diff(&live, &same);

        assert_eq!(with_policy.added_nodes.len(), plain.added_nodes.len());
        assert_eq!(with_policy.removed_nodes.len(), plain.removed_nodes.len());
        assert_eq!(with_policy.added_edges.len(), plain.added_edges.len());
        assert_eq!(with_policy.removed_edges.len(), plain.removed_edges.len());
    }

    #[test]
    fn a_report_distinguishes_failure_from_absence() {
        let clean = CollectionReport::default();
        assert!(clean.is_complete());

        let mut partial = CollectionReport::default();
        partial.record(CollectionSource::Aws, "us-east-1/dynamodb", "throttled");
        assert!(!partial.is_complete());
        assert!(partial.summary().contains("AWS"));
        assert!(partial.summary().contains("us-east-1/dynamodb"));
    }

    #[test]
    fn demo_graph_toggles_sentinel_by_parity() {
        let even = demo_graph(2).graph;
        let odd = demo_graph(3).graph;
        // The sentinel host+ip and their edge are present only on odd ticks.
        assert_eq!(odd.node_count(), even.node_count() + 2);

        // Diffing an even→odd transition yields exactly the sentinel additions,
        // and the reverse yields the removals — the live path the demo drives.
        let added = diff(&even, &odd);
        assert_eq!(added.added_nodes.len(), 2);
        assert_eq!(added.added_edges.len(), 1);
        assert!(added.removed_nodes.is_empty() && added.removed_edges.is_empty());

        let removed = diff(&odd, &even);
        assert_eq!(removed.removed_nodes.len(), 2);
        assert_eq!(removed.removed_edges.len(), 1);
        assert!(removed.added_nodes.is_empty() && removed.added_edges.is_empty());
    }
}
