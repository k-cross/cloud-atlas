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

    /// Whether this node is already in the graph, without inserting it.
    pub fn contains(&self, node: &Node) -> bool {
        self.node_map.contains_key(node)
    }

    /// Where a node lives, if it is here. The index is only valid until the
    /// next [`remove_node`](Self::remove_node).
    pub fn index_of(&self, node: &Node) -> Option<NodeIndex> {
        self.node_map.get(node).copied()
    }

    /// Whether this exact edge already connects these two nodes. The
    /// by-value counterpart of the dedup check inside
    /// [`add_edge`](Self::add_edge), for callers holding `Node`s rather than
    /// indices.
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

    /// Take a node out of the graph, along with every edge touching it, and
    /// report exactly what went — the caller needs that to describe the change
    /// downstream, and re-deriving it after the fact is impossible.
    ///
    /// This is the only correct way to remove a node here: petgraph's
    /// `remove_node` swap-removes, so the node that was last takes the removed
    /// index and every other index above it stays put. Left alone, `node_map`
    /// would then point that moved node at a stale index — the reason removal
    /// belongs on the builder rather than at each call site.
    pub fn remove_node(&mut self, node: &Node) -> Option<Removal> {
        let idx = self.index_of(node)?;

        // Self-loops would otherwise be collected twice, once per direction.
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
        // Drop the mapping only once the graph has actually let go, so a stale
        // index — the very bug this method exists to prevent — cannot leave a
        // node present in the graph but unreachable by identity, where the next
        // `get_or_add_node` would silently duplicate it.
        let removed = self.graph.remove_node(idx)?;
        self.node_map.remove(node);
        if last != idx {
            // The node that was at `last` now lives at `idx`.
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

/// What a [`GraphBuilder::remove_node`] took out: the node itself plus every
/// edge that died with it, by value, so the caller can name them after the
/// graph no longer holds them.
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

    /// petgraph swap-removes, so the last node lands on the removed index. If
    /// the map is not repaired, the moved node's edges get attached to whatever
    /// now occupies its old index.
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
