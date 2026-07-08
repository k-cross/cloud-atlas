//! Render snapshot export.
//!
//! Rendering (layout + drawing) lives entirely in the separate
//! `atlas-render` workspace; this versioned JSON document (plus the
//! `atlas::patch::GraphPatch` delta of the same shape) is the only contract
//! between graph building and rendering. Keep the shape and `SNAPSHOT_VERSION`
//! in sync across all three consumers — `atlas-layout`'s `graph` module (whose
//! `parses_snapshot_json` test pins the wire format) and `atlas-web`'s
//! `graph.ts` — and rebuild the wasm (`bun run wasm`) after a bump, since the
//! compiled layout engine bakes in the version.

use crate::atlas::definition::{Edge, Node};
use petgraph::graph::Graph;
use petgraph::visit::EdgeRef;
use serde::Serialize;

/// Wire-format version shared with `atlas-layout` and `atlas-web`. v2 added a
/// stable `key` to every node and edge so the live backend can reference a
/// specific resource across full-scan rebuilds (petgraph indices are not
/// stable). Bump all three sides together when the shape changes.
pub const SNAPSHOT_VERSION: u32 = 2;

#[derive(Serialize)]
pub struct RenderSnapshot {
    pub version: u32,
    pub nodes: Vec<RenderNode>,
    pub edges: Vec<RenderEdge>,
}

#[derive(Clone, Serialize)]
pub struct RenderNode {
    /// petgraph node index — dense, but consumers must not rely on it across
    /// rebuilds; it exists only so the layout engine can position by index.
    pub id: u32,
    /// Stable identity derived from the typed `Node` (`node_key`). Survives
    /// rebuilds, so patches and the frontend key nodes by this.
    pub key: String,
    /// Human-readable `Display` form, for tooltips and search.
    pub label: String,
    /// Enum variant name (`Node::kind()`), for styling by resource type.
    pub kind: &'static str,
}

#[derive(Clone, Serialize)]
pub struct RenderEdge {
    pub source: u32,
    pub target: u32,
    /// Stable identity derived from endpoint keys + edge kind (`edge_key`).
    pub key: String,
    /// Stable key of the source node, for referencing across rebuilds.
    pub source_key: String,
    /// Stable key of the target node.
    pub target_key: String,
    pub kind: &'static str,
}

/// Stable, human-debuggable identity for a node. `kind` disambiguates variants
/// whose `Display` forms could otherwise coincide; `Display` carries the
/// resource id (the `Type::SubType(id)` convention from CLAUDE.md).
pub fn node_key(node: &Node) -> String {
    format!("{}#{}", node.kind(), node)
}

/// Stable identity for an edge: its endpoints' keys plus its kind. Matches the
/// dedup guarantee in `GraphBuilder::add_edge` (no two identical edges between
/// the same pair), so this is unique within a graph.
pub fn edge_key(source_key: &str, target_key: &str, edge: &Edge) -> String {
    format!("{}|{}->{}", edge.kind(), source_key, target_key)
}

pub fn render_snapshot(graph: &Graph<Node, Edge>) -> RenderSnapshot {
    let nodes = graph
        .node_indices()
        .map(|i| RenderNode {
            id: i.index() as u32,
            key: node_key(&graph[i]),
            label: graph[i].to_string(),
            kind: graph[i].kind(),
        })
        .collect();
    let edges = graph
        .edge_references()
        .map(|e| {
            let source_key = node_key(&graph[e.source()]);
            let target_key = node_key(&graph[e.target()]);
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
    RenderSnapshot {
        version: SNAPSHOT_VERSION,
        nodes,
        edges,
    }
}

pub fn snapshot_json(graph: &Graph<Node, Edge>) -> serde_json::Result<String> {
    serde_json::to_string(&render_snapshot(graph))
}
