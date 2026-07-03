//! Render snapshot export.
//!
//! Rendering (layout + drawing) lives entirely in the separate
//! `atlas-render` workspace; this versioned JSON document is the only
//! contract between graph building and rendering. Keep the shape and
//! `SNAPSHOT_VERSION` in sync with `atlas-layout`'s `graph` module — its
//! `parses_snapshot_json` test pins the same wire format from the consumer
//! side.

use crate::atlas::definition::{Edge, Node};
use petgraph::graph::Graph;
use petgraph::visit::EdgeRef;
use serde::Serialize;

pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct RenderSnapshot {
    pub version: u32,
    pub nodes: Vec<RenderNode>,
    pub edges: Vec<RenderEdge>,
}

#[derive(Serialize)]
pub struct RenderNode {
    /// petgraph node index — dense, but consumers must not rely on that.
    pub id: u32,
    /// Human-readable `Display` form, for tooltips and search.
    pub label: String,
    /// Enum variant name (`Node::kind()`), for styling by resource type.
    pub kind: &'static str,
}

#[derive(Serialize)]
pub struct RenderEdge {
    pub source: u32,
    pub target: u32,
    pub kind: &'static str,
}

pub fn render_snapshot(graph: &Graph<Node, Edge>) -> RenderSnapshot {
    let nodes = graph
        .node_indices()
        .map(|i| RenderNode {
            id: i.index() as u32,
            label: graph[i].to_string(),
            kind: graph[i].kind(),
        })
        .collect();
    let edges = graph
        .edge_references()
        .map(|e| RenderEdge {
            source: e.source().index() as u32,
            target: e.target().index() as u32,
            kind: e.weight().kind(),
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
