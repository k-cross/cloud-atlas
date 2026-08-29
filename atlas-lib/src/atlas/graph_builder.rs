use crate::atlas::definition::{Edge, Node};
use petgraph::graph::{Graph, NodeIndex};
use std::collections::HashMap;

pub struct GraphBuilder {
    pub graph: Graph<Node, Edge>,
    pub node_map: HashMap<Node, NodeIndex>,
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
            node_map: HashMap::new(),
        }
    }

    pub fn get_or_add_node(&mut self, node: Node) -> NodeIndex {
        if let Some(&idx) = self.node_map.get(&node) {
            idx
        } else {
            let idx = self.graph.add_node(node.clone());
            self.node_map.insert(node, idx);
            idx
        }
    }

    /// [`get_or_add_node`](Self::get_or_add_node) for a node you only have by
    /// reference: it clones only when the node is genuinely new. This is the
    /// shape `merge` folds with, where most nodes are already present.
    pub fn get_or_add_ref(&mut self, node: &Node) -> NodeIndex {
        match self.node_map.get(node) {
            Some(&idx) => idx,
            None => self.get_or_add_node(node.clone()),
        }
    }

    /// Add an edge unless an identical one already connects the two nodes,
    /// keeping the exported .dot output free of duplicates.
    pub fn add_edge(&mut self, a: NodeIndex, b: NodeIndex, edge: Edge) {
        let exists = self
            .graph
            .edges_connecting(a, b)
            .any(|e| e.weight() == &edge);
        if !exists {
            self.graph.add_edge(a, b, edge);
        }
    }

    /// Fold another graph's nodes and edges into this one, translating
    /// endpoints by node identity so cross-graph dedup is preserved. This is
    /// how sub-graphs produced in parallel are stitched back together, and how
    /// `patch::carry_forward` folds the live graph into an incomplete scan;
    /// merging in a fixed input order keeps the result deterministic.
    pub fn merge(&mut self, other: &Graph<Node, Edge>) {
        self.merge_where(other, |_| true);
    }

    /// `merge`, restricted to the nodes `keep` accepts. An edge crosses over
    /// only when at least one endpoint was kept on purpose *and* both endpoints
    /// are present here — a rejected node is never resurrected as the endpoint
    /// of an edge, and no edge is left dangling.
    pub fn merge_where(&mut self, other: &Graph<Node, Edge>, keep: impl Fn(&Node) -> bool) {
        // Carry over every node first — this covers standalone nodes that never
        // appear as an edge endpoint.
        for node in other.node_weights() {
            if keep(node) {
                self.get_or_add_ref(node);
            }
        }
        for edge_idx in other.edge_indices() {
            if let Some((a, b)) = other.edge_endpoints(edge_idx) {
                let (source, target) = (&other[a], &other[b]);
                if !keep(source) && !keep(target) {
                    continue;
                }
                if let (Some(&a_idx), Some(&b_idx)) =
                    (self.node_map.get(source), self.node_map.get(target))
                {
                    self.add_edge(a_idx, b_idx, other[edge_idx].clone());
                }
            }
        }
    }
}
