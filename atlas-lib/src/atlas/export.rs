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
/// stable). v3 added `observations`, the Tier-2 liveness overlay. Bump all
/// three sides together when the shape changes.
pub const SNAPSHOT_VERSION: u32 = 3;

#[derive(Serialize)]
pub struct RenderSnapshot {
    pub version: u32,
    pub nodes: Vec<RenderNode>,
    pub edges: Vec<RenderEdge>,
    /// Liveness for the nodes and edges traffic was observed on (v3). Keyed by
    /// the same stable `key` the node/edge carries, and deliberately a separate
    /// list rather than fields on `RenderNode`/`RenderEdge`: freshness changes
    /// far more often than topology does, so it has to be patchable on its own
    /// without re-announcing the resource.
    pub observations: Vec<RenderObservation>,
}

/// What the flow overlay has observed about one node or edge.
///
/// The `key` is a `node_key` or an `edge_key` — whichever the observation is
/// about — so a consumer looks it up in the graph it already holds. A key that
/// names nothing is simply ignored: an observation can name a resource no scan
/// has found yet, and inventing a node for it is Tier 3's job, not Tier 2's.
#[derive(Clone, Debug, Serialize)]
pub struct RenderObservation {
    pub key: String,
    /// Epoch milliseconds of the newest flow record that mentioned it — the
    /// freshness the health overlay reads. Present on nodes and edges alike,
    /// because it composes as a maximum: recording it against every key one
    /// record names is idempotent.
    pub last_seen: i64,
    /// Volume, on flow *edges* only — absent on a node, where it would be an
    /// undirected sum over every flow that touched it, and where recording it
    /// would count one record's traffic once per key the record names. Derive a
    /// node's throughput from its incident `TrafficFlow` edges instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packets: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// `accepted`, `rejected`, `mixed`, or `observed` — whether the traffic got
    /// through. A security group says traffic *could* flow; this says whether
    /// it did.
    pub status: &'static str,
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

impl RenderNode {
    /// The one place a `Node` becomes wire format. `id` is positional and the
    /// caller owns it: a full snapshot uses the petgraph index, while a
    /// subgraph payload renumbers from zero so its edges can index its own
    /// node array.
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
    /// The one place an `Edge` becomes wire format; `source_id`/`target_id`
    /// index whichever node array this edge is being emitted alongside.
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

/// A snapshot of topology alone. The batch CLI path has no flow overlay, so
/// its `observations` are empty — which reads as "nothing observed", not as
/// "everything is dark", because a client that never sees an observation for
/// any key has no liveness information to overlay in the first place.
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
