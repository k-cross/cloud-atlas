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

    pub fn add_edge(&mut self, a: NodeIndex, b: NodeIndex, edge: Edge) {
        self.graph.add_edge(a, b, edge);
    }
}
