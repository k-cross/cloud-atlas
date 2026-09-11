use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

pub const SNAPSHOT_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub version: u32,
    pub nodes: Vec<SnapshotNode>,
    pub edges: Vec<SnapshotEdge>,

    #[serde(default)]
    pub observations: Vec<SnapshotObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotObservation {
    pub key: String,
    pub last_seen: i64,

    #[serde(default)]
    pub packets: Option<u64>,
    #[serde(default)]
    pub bytes: Option<u64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotNode {
    pub id: u32,

    #[serde(default)]
    pub key: String,
    pub label: String,
    pub kind: String,

    #[serde(default)]
    pub x: Option<f32>,
    #[serde(default)]
    pub y: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEdge {
    pub source: u32,
    pub target: u32,

    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub source_key: String,
    #[serde(default)]
    pub target_key: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GraphError {
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
    UnknownNodeId {
        id: u32,
    },
    EdgeOutOfBounds {
        source: u32,
        target: u32,
        node_count: usize,
    },
    Json(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::UnsupportedVersion { found, supported } => write!(
                f,
                "unsupported snapshot version {found} (this build supports {supported})"
            ),
            GraphError::UnknownNodeId { id } => {
                write!(f, "edge references node id {id} absent from the node list")
            }
            GraphError::EdgeOutOfBounds {
                source,
                target,
                node_count,
            } => write!(
                f,
                "edge ({source} -> {target}) out of bounds for {node_count} nodes"
            ),
            GraphError::Json(msg) => write!(f, "invalid snapshot JSON: {msg}"),
        }
    }
}

impl std::error::Error for GraphError {}

#[derive(Debug, Clone)]
pub struct LayoutGraph {
    node_count: usize,
    edges: Vec<(u32, u32)>,
    degrees: Vec<u32>,
    initial_positions: Vec<f32>,
}

impl LayoutGraph {
    pub fn new(node_count: usize, edges: Vec<(u32, u32)>) -> Result<Self, GraphError> {
        let mut degrees = vec![0u32; node_count];
        for &(source, target) in &edges {
            if source as usize >= node_count || target as usize >= node_count {
                return Err(GraphError::EdgeOutOfBounds {
                    source,
                    target,
                    node_count,
                });
            }
            degrees[source as usize] += 1;
            degrees[target as usize] += 1;
        }
        Ok(Self {
            node_count,
            edges,
            degrees,
            initial_positions: Vec::new(),
        })
    }

    pub fn initial_positions(&self) -> &[f32] {
        &self.initial_positions
    }

    pub fn from_snapshot(snapshot: &GraphSnapshot) -> Result<Self, GraphError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(GraphError::UnsupportedVersion {
                found: snapshot.version,
                supported: SNAPSHOT_VERSION,
            });
        }
        let index_of: HashMap<u32, u32> = snapshot
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i as u32))
            .collect();
        let lookup = |id: u32| {
            index_of
                .get(&id)
                .copied()
                .ok_or(GraphError::UnknownNodeId { id })
        };
        let edges = snapshot
            .edges
            .iter()
            .map(|e| Ok((lookup(e.source)?, lookup(e.target)?)))
            .collect::<Result<Vec<_>, GraphError>>()?;
        let mut graph = Self::new(snapshot.nodes.len(), edges)?;

        if snapshot
            .nodes
            .iter()
            .any(|n| n.x.is_some() && n.y.is_some())
        {
            graph.initial_positions = snapshot
                .nodes
                .iter()
                .flat_map(|n| match (n.x, n.y) {
                    (Some(x), Some(y)) => [x, y],
                    _ => [f32::NAN, f32::NAN],
                })
                .collect();
        }
        Ok(graph)
    }

    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        let snapshot: GraphSnapshot =
            serde_json::from_str(json).map_err(|e| GraphError::Json(e.to_string()))?;
        Self::from_snapshot(&snapshot)
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn edges(&self) -> &[(u32, u32)] {
        &self.edges
    }

    pub fn degree(&self, node: usize) -> u32 {
        self.degrees[node]
    }

    pub fn mass(&self, node: usize) -> f32 {
        (self.degrees[node] + 1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "version": 3,
        "nodes": [
            {"id": 0, "key": "AwsEc2Instance#a", "label": "Instance(i-1)", "kind": "AwsEc2Instance"},
            {"id": 1, "key": "AwsEc2Eni#b", "label": "Eni(eni-1)", "kind": "AwsEc2Eni"},
            {"id": 2, "key": "AwsEc2Subnet#c", "label": "Subnet(subnet-1)", "kind": "AwsEc2Subnet"}
        ],
        "edges": [
            {"source": 0, "target": 1, "key": "HasIp|a->b", "source_key": "AwsEc2Instance#a", "target_key": "AwsEc2Eni#b", "kind": "HasIp"},
            {"source": 1, "target": 2, "key": "AttachedTo|b->c", "source_key": "AwsEc2Eni#b", "target_key": "AwsEc2Subnet#c", "kind": "AttachedTo"}
        ],
        "observations": [
            {"key": "TrafficFlow|a->b", "last_seen": 1788436800000, "packets": 24, "bytes": 4800, "status": "accepted"},
            {"key": "AwsEc2Instance#a", "last_seen": 1788436800000, "status": "accepted"}
        ]
    }"#;

    #[test]
    fn parses_snapshot_json() {
        let graph = LayoutGraph::from_json(SAMPLE).unwrap();
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
        assert_eq!(graph.degree(1), 2);
        assert_eq!(graph.mass(1), 3.0);
    }

    #[test]
    fn remaps_sparse_node_ids() {
        let json = r#"{
            "version": 3,
            "nodes": [
                {"id": 10, "label": "a", "kind": "GenericIpAddress"},
                {"id": 99, "label": "b", "kind": "GenericHostname"}
            ],
            "edges": [{"source": 99, "target": 10, "kind": "ResolvesTo"}]
        }"#;
        let graph = LayoutGraph::from_json(json).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edges(), &[(1, 0)]);
    }

    #[test]
    fn rejects_unknown_edge_endpoint() {
        let json = r#"{
            "version": 3,
            "nodes": [{"id": 0, "label": "a", "kind": "GenericIpAddress"}],
            "edges": [{"source": 0, "target": 5, "kind": "RoutesTo"}]
        }"#;
        assert_eq!(
            LayoutGraph::from_json(json).unwrap_err(),
            GraphError::UnknownNodeId { id: 5 }
        );
    }

    #[test]
    fn rejects_future_version() {
        let json = r#"{"version": 4, "nodes": [], "edges": []}"#;
        assert!(matches!(
            LayoutGraph::from_json(json).unwrap_err(),
            GraphError::UnsupportedVersion { found: 4, .. }
        ));
    }

    #[test]
    fn a_snapshot_without_observations_still_parses() {
        let json = r#"{
            "version": 3,
            "nodes": [{"id": 0, "label": "a", "kind": "GenericIpAddress"}],
            "edges": []
        }"#;
        assert_eq!(LayoutGraph::from_json(json).unwrap().node_count(), 1);
    }

    #[test]
    fn rejects_out_of_bounds_edge() {
        assert!(matches!(
            LayoutGraph::new(2, vec![(0, 2)]).unwrap_err(),
            GraphError::EdgeOutOfBounds { .. }
        ));
    }
}
