use crate::atlas::definition::{Edge, Node};
use petgraph::graph::Graph;
use petgraph::visit::EdgeRef;
use serde::Serialize;

pub const SNAPSHOT_VERSION: u32 = 3;

#[derive(Serialize)]
pub struct RenderSnapshot {
    pub version: u32,
    pub nodes: Vec<RenderNode>,
    pub edges: Vec<RenderEdge>,
    pub observations: Vec<RenderObservation>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenderObservation {
    pub key: String,
    pub last_seen: i64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub packets: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    pub status: &'static str,
}

#[derive(Clone, Serialize)]
pub struct RenderNode {
    pub id: u32,
    pub key: String,
    pub label: String,
    pub kind: &'static str,
}

#[derive(Clone, Serialize)]
pub struct RenderEdge {
    pub source: u32,
    pub target: u32,
    pub key: String,
    pub source_key: String,
    pub target_key: String,
    pub kind: &'static str,
}

impl RenderNode {
    pub fn new(node: &Node, id: u32) -> Self {
        Self {
            id,
            key: node_key(node),
            label: node.to_string(),
            kind: node.kind(),
        }
    }
}

impl RenderEdge {
    pub fn new(source: &Node, target: &Node, edge: &Edge, source_id: u32, target_id: u32) -> Self {
        let source_key = node_key(source);
        let target_key = node_key(target);
        Self {
            source: source_id,
            target: target_id,
            key: edge_key(&source_key, &target_key, edge),
            source_key,
            target_key,
            kind: edge.kind(),
        }
    }
}

pub fn node_key(node: &Node) -> String {
    format!("{}#{}", node.kind(), node)
}

pub fn edge_key(source_key: &str, target_key: &str, edge: &Edge) -> String {
    format!("{}|{}->{}", edge.kind(), source_key, target_key)
}

pub fn render_snapshot(graph: &Graph<Node, Edge>) -> RenderSnapshot {
    render_snapshot_with(graph, Vec::new())
}

pub fn render_snapshot_with(
    graph: &Graph<Node, Edge>,
    observations: Vec<RenderObservation>,
) -> RenderSnapshot {
    let nodes = graph
        .node_indices()
        .map(|i| RenderNode::new(&graph[i], i.index() as u32))
        .collect();
    let edges = graph
        .edge_references()
        .map(|e| {
            RenderEdge::new(
                &graph[e.source()],
                &graph[e.target()],
                e.weight(),
                e.source().index() as u32,
                e.target().index() as u32,
            )
        })
        .collect();
    RenderSnapshot {
        version: SNAPSHOT_VERSION,
        nodes,
        edges,
        observations,
    }
}

pub fn snapshot_json(graph: &Graph<Node, Edge>) -> serde_json::Result<String> {
    serde_json::to_string(&render_snapshot(graph))
}
