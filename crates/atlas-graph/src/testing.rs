use crate::collection::CollectionSource;
use crate::definition::{Edge, Node};
use crate::graph_builder::GraphBuilder;
use petgraph::visit::EdgeRef;
use std::collections::HashSet;

pub fn public(address: &str) -> Node {
    Node::public_ip(address).expect("a public address")
}

pub fn link(address: &str) -> Node {
    Node::link_ip(address).expect("an address with its link prefix")
}

pub fn assert_edge(builder: &GraphBuilder, a: &Node, b: &Node, edge: &Edge) {
    let a_idx = builder
        .node_map
        .get(a)
        .unwrap_or_else(|| panic!("Node not found: {:?}", a));
    let b_idx = builder
        .node_map
        .get(b)
        .unwrap_or_else(|| panic!("Node not found: {:?}", b));

    let found = builder
        .graph
        .edges_connecting(*a_idx, *b_idx)
        .any(|e| e.weight() == edge);
    assert!(
        found,
        "Edge {:?} -> {:?} with weight {:?} not found",
        a, b, edge
    );
}

pub fn assert_has_node(builder: &GraphBuilder, node: &Node) {
    assert!(
        builder.node_map.contains_key(node),
        "Node not found: {:?}",
        node
    );
}

pub fn assert_resolves(builder: &GraphBuilder, name: &str, record: &Node, target: &Node) {
    assert_edge(builder, &Node::hostname(name), record, &Edge::ResolvesTo);
    assert_edge(builder, record, target, &Edge::ResolvesTo);
}

pub fn serves(builder: &GraphBuilder, source: &Node, target: &Node) -> bool {
    builder.has_edge(source, target, &Edge::Serves)
}

pub fn serves_any(builder: &GraphBuilder, pick: impl Fn(&Node, &Node) -> bool) -> bool {
    builder
        .graph
        .edge_references()
        .filter(|e| e.weight() == &Edge::Serves)
        .any(|e| pick(&builder.graph[e.source()], &builder.graph[e.target()]))
}

pub fn all_sources() -> HashSet<CollectionSource> {
    HashSet::from([
        CollectionSource::Aws,
        CollectionSource::Gcp,
        CollectionSource::Azure,
        CollectionSource::Cloudflare,
    ])
}
