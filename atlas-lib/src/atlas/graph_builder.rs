use crate::atlas::definition::{Edge, Node};
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
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

    pub fn contains(&self, node: &Node) -> bool {
        self.node_map.contains_key(node)
    }

    pub fn index_of(&self, node: &Node) -> Option<NodeIndex> {
        self.node_map.get(node).copied()
    }

    pub fn has_edge(&self, source: &Node, target: &Node, edge: &Edge) -> bool {
        match (self.node_map.get(source), self.node_map.get(target)) {
            (Some(&a), Some(&b)) => self
                .graph
                .edges_connecting(a, b)
                .any(|e| e.weight() == edge),
            _ => false,
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

    pub fn get_or_add_ref(&mut self, node: &Node) -> NodeIndex {
        match self.node_map.get(node) {
            Some(&idx) => idx,
            None => self.get_or_add_node(node.clone()),
        }
    }

    pub fn add_edge(&mut self, a: NodeIndex, b: NodeIndex, edge: Edge) {
        let exists = self
            .graph
            .edges_connecting(a, b)
            .any(|e| e.weight() == &edge);
        if !exists {
            self.graph.add_edge(a, b, edge);
        }
    }

    pub fn remove_node(&mut self, node: &Node) -> Option<Removal> {
        let idx = self.index_of(node)?;

        let edges: Vec<(Node, Node, Edge)> = self
            .graph
            .edges_directed(idx, Direction::Outgoing)
            .chain(
                self.graph
                    .edges_directed(idx, Direction::Incoming)
                    .filter(|e| e.source() != idx),
            )
            .map(|e| {
                (
                    self.graph[e.source()].clone(),
                    self.graph[e.target()].clone(),
                    e.weight().clone(),
                )
            })
            .collect();

        let last = NodeIndex::new(self.graph.node_count() - 1);

        let removed = self.graph.remove_node(idx)?;
        self.node_map.remove(node);
        if last != idx {
            self.node_map.insert(self.graph[idx].clone(), idx);
        }

        Some(Removal {
            node: removed,
            edges,
        })
    }

    pub fn link_to(
        &mut self,
        source: impl Into<Option<NodeIndex>>,
        node: Node,
        edge: Edge,
    ) -> NodeIndex {
        let target = self.get_or_add_node(node);
        if let Some(source) = source.into() {
            self.add_edge(source, target, edge);
        }
        target
    }

    pub fn link_from(
        &mut self,
        target: impl Into<Option<NodeIndex>>,
        node: Node,
        edge: Edge,
    ) -> NodeIndex {
        let source = self.get_or_add_node(node);
        if let Some(target) = target.into() {
            self.add_edge(source, target, edge);
        }
        source
    }

    pub fn merge(&mut self, other: &Graph<Node, Edge>) {
        self.merge_where(other, |_| true);
    }

    pub fn merge_where(&mut self, other: &Graph<Node, Edge>, keep: impl Fn(&Node) -> bool) {
        self.merge_selected(other, keep, |_, _, _| true);
    }

    pub fn merge_selected(
        &mut self,
        other: &Graph<Node, Edge>,
        keep: impl Fn(&Node) -> bool,
        keep_edge: impl Fn(&Node, &Node, &Edge) -> bool,
    ) {
        for node in other.node_weights() {
            if keep(node) {
                self.get_or_add_ref(node);
            }
        }
        for edge_idx in other.edge_indices() {
            if let Some((a, b)) = other.edge_endpoints(edge_idx) {
                let (source, target) = (&other[a], &other[b]);
                if !keep_edge(source, target, &other[edge_idx]) {
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

pub struct Removal {
    pub node: Node,
    pub edges: Vec<(Node, Node, Edge)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(id: &str) -> Node {
        Node::AwsEc2Instance(id.into())
    }

    #[test]
    fn removing_a_node_takes_its_edges_with_it() {
        let mut builder = GraphBuilder::new();
        let vpc = builder.get_or_add_node(Node::AwsEc2Vpc("vpc-1".into()));
        let subnet = builder.get_or_add_node(Node::AwsEc2Subnet("subnet-1".into()));
        let eni = builder.get_or_add_node(Node::AwsEc2Eni("i-1".into()));
        builder.add_edge(vpc, subnet, Edge::Contains);
        builder.add_edge(eni, subnet, Edge::AttachedTo);

        let removal = builder
            .remove_node(&Node::AwsEc2Subnet("subnet-1".into()))
            .expect("subnet was present");

        assert_eq!(removal.node, Node::AwsEc2Subnet("subnet-1".into()));
        assert_eq!(removal.edges.len(), 2, "both incident edges are reported");
        assert_eq!(builder.graph.edge_count(), 0);
        assert!(!builder.contains(&Node::AwsEc2Subnet("subnet-1".into())));
    }

    #[test]
    fn removal_repairs_the_index_of_the_node_that_moved() {
        let mut builder = GraphBuilder::new();
        for id in ["i-1", "i-2", "i-3"] {
            builder.get_or_add_node(instance(id));
        }

        builder.remove_node(&instance("i-1")).expect("present");

        for id in ["i-2", "i-3"] {
            let idx = builder.index_of(&instance(id)).expect("still mapped");
            assert_eq!(
                builder.graph[idx],
                instance(id),
                "{id} must still resolve to itself after the swap-remove"
            );
        }
    }

    #[test]
    fn removing_an_absent_node_reports_nothing() {
        let mut builder = GraphBuilder::new();
        builder.get_or_add_node(instance("i-1"));
        assert!(builder.remove_node(&instance("i-missing")).is_none());
        assert_eq!(builder.graph.node_count(), 1);
    }

    #[test]
    fn has_edge_matches_the_dedup_rule_add_edge_applies() {
        let mut builder = GraphBuilder::new();
        let a = builder.get_or_add_node(instance("i-1"));
        let b = builder.get_or_add_node(Node::AwsEc2Eni("i-1".into()));
        builder.add_edge(a, b, Edge::HasIp);

        assert!(builder.has_edge(
            &instance("i-1"),
            &Node::AwsEc2Eni("i-1".into()),
            &Edge::HasIp
        ));
        assert!(!builder.has_edge(
            &instance("i-1"),
            &Node::AwsEc2Eni("i-1".into()),
            &Edge::AttachedTo
        ));
        assert!(!builder.has_edge(
            &instance("i-2"),
            &Node::AwsEc2Eni("i-1".into()),
            &Edge::HasIp
        ));
    }
}
