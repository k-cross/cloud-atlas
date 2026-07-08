//! Tier-3 reconciliation loop: periodically re-derive the whole graph, diff it
//! against the live one, and broadcast the change set. This is the same
//! full-scan we do today, minus the wipe — the incremental primary feeds (event
//! streams, flow logs) plug in on top of this later.

use crate::state::AppState;
use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::engine::AtlasEngine;
use atlas_lib::atlas::patch::diff;
use atlas_lib::fixtures;
use petgraph::graph::Graph;
use std::time::Duration;

/// Where each reconciliation tick's graph comes from.
pub enum Source {
    /// Real collection from configured cloud providers.
    Live(AtlasEngine),
    /// Credential-free fixtures with a sentinel that flips in and out every
    /// other tick, so the live-patch path is exercised without any cloud calls.
    Demo,
}

impl Source {
    /// Produce the graph for tick `n`.
    async fn scan(&self, tick: u64) -> Graph<Node, Edge> {
        match self {
            Source::Live(engine) => engine.collect().await.graph,
            Source::Demo => demo_graph(tick),
        }
    }
}

/// The fixtures graph, plus a small connected sentinel pair on odd ticks. The
/// resulting alternation (add on odd, remove on even) makes every kind of patch
/// — added/removed nodes and edges — flow past a connected frontend.
fn demo_graph(tick: u64) -> Graph<Node, Edge> {
    let mut builder = fixtures::build_graph();
    if tick % 2 == 1 {
        let host = builder.get_or_add_node(Node::GenericHostname("live-demo.internal".into()));
        let ip = builder.get_or_add_node(Node::GenericIpAddress("198.51.100.42".into()));
        builder.add_edge(host, ip, Edge::ResolvesTo);
    }
    builder.graph
}

/// Run forever, reconciling every `interval`. Only non-empty diffs mutate the
/// live graph or hit the broadcast channel.
pub async fn run(state: AppState, source: Source, interval: Duration) {
    let mut tick: u64 = 0;
    loop {
        tokio::time::sleep(interval).await;
        tick += 1;

        let next = source.scan(tick).await;
        // Diff under the read lock — no full-graph clone, and the critical
        // section is just the comparison. WebSocket readers share the lock.
        let patch = {
            let live = state.live.read().await;
            diff(&live, &next)
        };
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
        *state.live.write().await = next;
        // Err only means no subscribers are connected — nothing to do.
        let _ = state.patches.send(patch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_graph_toggles_sentinel_by_parity() {
        let even = demo_graph(2);
        let odd = demo_graph(3);
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
