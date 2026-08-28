//! Incremental graph diffs (Phase 1 of `docs/change_monitoring_design.md`).
//!
//! The live backend keeps a persistent graph and, on each reconciliation scan,
//! projects into a scratch graph and diffs it against the live one. The result
//! is a [`GraphPatch`] describing exactly which nodes/edges were added or
//! removed, keyed by the stable [`node_key`]/[`edge_key`] identity so a
//! consumer can apply it without a full rebuild.
//!
//! Node *property updates* are represented as a remove + add of the same key
//! (the typed `Node` encodes its config, so a changed resource is a different
//! value). Richer update semantics are deferred to the liveness work.

use crate::atlas::definition::{Edge, Node};
use crate::atlas::export::{RenderEdge, RenderNode, SNAPSHOT_VERSION, edge_key, node_key};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A minimal set of changes between two graph states. Added items carry their
/// full render info so the frontend can materialize them; removed items are
/// referenced by stable key.
#[derive(Clone, Serialize)]
pub struct GraphPatch {
    pub version: u32,
    pub added_nodes: Vec<RenderNode>,
    pub removed_nodes: Vec<String>,
    pub added_edges: Vec<RenderEdge>,
    pub removed_edges: Vec<String>,
}

impl GraphPatch {
    /// A patch that touches nothing — the common case between polls, and the
    /// signal the poll loop uses to skip a broadcast.
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
    }
}

/// Identity of an edge: its endpoint values plus its weight. Endpoints are
/// compared by `Node` value (not petgraph index), so this survives rebuilds.
type EdgeRefId<'a> = (&'a Node, &'a Node, &'a Edge);

fn edge_ref_ids(graph: &Graph<Node, Edge>) -> HashSet<EdgeRefId<'_>> {
    graph
        .edge_references()
        .map(|e| (&graph[e.source()], &graph[e.target()], e.weight()))
        .collect()
}

/// Diff `new` against `old`, producing the change set that turns `old` into
/// `new`. Identity is the typed `Node`/`Edge` value (both `Hash + Eq`), so this
/// is independent of petgraph index churn and — crucially for the poll loop
/// that runs this every tick — allocates strings only for items that actually
/// changed, not for the whole graph.
pub fn diff(old: &Graph<Node, Edge>, new: &Graph<Node, Edge>) -> GraphPatch {
    let old_nodes: HashSet<&Node> = old.node_weights().collect();
    let new_nodes: HashSet<&Node> = new.node_weights().collect();
    let old_edges = edge_ref_ids(old);
    let new_edges = edge_ref_ids(new);

    let added_nodes = new
        .node_indices()
        .filter(|&i| !old_nodes.contains(&new[i]))
        .map(|i| RenderNode {
            id: i.index() as u32,
            key: node_key(&new[i]),
            label: new[i].to_string(),
            kind: new[i].kind(),
        })
        .collect();
    let removed_nodes = old
        .node_weights()
        .filter(|n| !new_nodes.contains(n))
        .map(node_key)
        .collect();

    let added_edges = new
        .edge_references()
        .filter(|e| !old_edges.contains(&(&new[e.source()], &new[e.target()], e.weight())))
        .map(|e| {
            let source_key = node_key(&new[e.source()]);
            let target_key = node_key(&new[e.target()]);
            RenderEdge {
                source: e.source().index() as u32,
                target: e.target().index() as u32,
                key: edge_key(&source_key, &target_key, e.weight()),
                source_key,
                target_key,
                kind: e.weight().kind(),
            }
        })
        .collect();
    let removed_edges = old
        .edge_references()
        .filter(|e| !new_edges.contains(&(&old[e.source()], &old[e.target()], e.weight())))
        .map(|e| {
            let source_key = node_key(&old[e.source()]);
            let target_key = node_key(&old[e.target()]);
            edge_key(&source_key, &target_key, e.weight())
        })
        .collect();

    GraphPatch {
        version: SNAPSHOT_VERSION,
        added_nodes,
        removed_nodes,
        added_edges,
        removed_edges,
    }
}

/// Fold everything in `previous` that is missing from `next` back into `next`,
/// so a diff against `previous` can only add.
///
/// This is the reconciliation policy for an *incomplete* scan. When a source
/// could not be read, its resources are absent from `next` for a reason that
/// has nothing to do with them being gone, and diffing as-is would broadcast
/// spurious removals. Carrying the previous state forward makes the tick
/// purely additive: genuinely new resources from the sources that *did* respond
/// still land, and anything unconfirmed is retained until a complete scan can
/// speak to it. Stale resources therefore linger rather than flicker, which is
/// the safer failure for a graph that is meant to be authoritative.
pub fn carry_forward(next: &mut Graph<Node, Edge>, previous: &Graph<Node, Edge>) {
    let mut index: HashMap<Node, NodeIndex> =
        next.node_indices().map(|i| (next[i].clone(), i)).collect();
    let mut present: HashSet<(Node, Node, Edge)> = next
        .edge_references()
        .map(|e| {
            (
                next[e.source()].clone(),
                next[e.target()].clone(),
                e.weight().clone(),
            )
        })
        .collect();

    for i in previous.node_indices() {
        let node = &previous[i];
        if !index.contains_key(node) {
            let added = next.add_node(node.clone());
            index.insert(node.clone(), added);
        }
    }

    for e in previous.edge_references() {
        let (source, target, weight) = (&previous[e.source()], &previous[e.target()], e.weight());
        if present.insert((source.clone(), target.clone(), weight.clone())) {
            next.add_edge(index[source], index[target], weight.clone());
        }
    }
}
