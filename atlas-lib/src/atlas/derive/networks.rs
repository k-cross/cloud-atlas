use crate::atlas::definition::{Edge, Node};
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

enum Join {
    Through {
        network: &'static str,
        connector: &'static str,
    },
    Direct {
        network: &'static str,
    },
}

const JOINS: &[Join] = &[
    Join::Through {
        network: "AwsEc2Vpc",
        connector: "AwsEc2VpcPeeringConnection",
    },
    Join::Through {
        network: "AwsEc2Vpc",
        connector: "AwsEc2TransitGateway",
    },
    Join::Direct {
        network: "AzureVirtualNetwork",
    },
    Join::Direct {
        network: "GcpComputeNetwork",
    },
];

#[derive(Default)]
pub(super) struct Joins<'g> {
    connectors: HashMap<&'g str, HashSet<NodeIndex>>,
    peers: HashMap<&'g str, HashSet<&'g str>>,
}

impl<'g> Joins<'g> {
    pub(super) fn of(graph: &'g Graph<Node, Edge>) -> Self {
        let mut joins = Joins::default();
        for join in JOINS {
            match join {
                Join::Through { network, connector } => {
                    for hub in nodes_of(graph, connector) {
                        for member in scopes_beside(graph, hub, network) {
                            joins.connectors.entry(member).or_default().insert(hub);
                        }
                    }
                }
                Join::Direct { network } => {
                    for start in nodes_of(graph, network) {
                        let Some(from) = graph[start].network_scope() else {
                            continue;
                        };
                        for to in scopes_beside(graph, start, network).filter(|to| *to != from) {
                            joins.peers.entry(from).or_default().insert(to);
                            joins.peers.entry(to).or_default().insert(from);
                        }
                    }
                }
            }
        }
        joins
    }

    pub(super) fn is_empty(&self) -> bool {
        self.connectors.is_empty() && self.peers.is_empty()
    }

    pub(super) fn joined(&self, a: &str, b: &str) -> bool {
        if a == b {
            return false;
        }
        if self.peers.get(a).is_some_and(|peers| peers.contains(b)) {
            return true;
        }
        match (self.connectors.get(a), self.connectors.get(b)) {
            (Some(left), Some(right)) => !left.is_disjoint(right),
            _ => false,
        }
    }
}

pub(crate) struct HeldAddresses<'g> {
    joins: Joins<'g>,
    held: HashMap<&'g str, Vec<(&'g str, NodeIndex)>>,
}

impl<'g> HeldAddresses<'g> {
    pub(super) fn new(
        graph: &'g Graph<Node, Edge>,
        joins: Joins<'g>,
        held_pivots: impl IntoIterator<Item = NodeIndex>,
    ) -> Self {
        let mut held: HashMap<&str, Vec<(&str, NodeIndex)>> = HashMap::new();
        for pivot in held_pivots {
            if let Node::GenericPrivateIpAddress { network, address } = &graph[pivot] {
                held.entry(address).or_default().push((network, pivot));
            }
        }
        HeldAddresses { joins, held }
    }

    pub(crate) fn holder(&self, network: &str, address: &str) -> Option<NodeIndex> {
        let holders = self.held.get(address)?;
        if let Some((_, local)) = holders.iter().find(|(held_in, _)| *held_in == network) {
            return Some(*local);
        }
        let mut joined = holders
            .iter()
            .filter(|(held_in, _)| self.joins.joined(network, held_in));
        match (joined.next(), joined.next()) {
            (Some((_, only)), None) => Some(*only),
            _ => None,
        }
    }
}

fn nodes_of<'g>(
    graph: &'g Graph<Node, Edge>,
    kind: &'g str,
) -> impl Iterator<Item = NodeIndex> + 'g {
    graph
        .node_indices()
        .filter(move |index| graph[*index].kind() == kind)
}

fn scopes_beside<'g>(
    graph: &'g Graph<Node, Edge>,
    at: NodeIndex,
    kind: &'g str,
) -> impl Iterator<Item = &'g str> + 'g {
    neighbours(graph, at)
        .filter(move |next| graph[*next].kind() == kind)
        .filter_map(|next| graph[next].network_scope())
}

fn neighbours(graph: &Graph<Node, Edge>, at: NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
    let outgoing = graph
        .edges_directed(at, Direction::Outgoing)
        .filter(|edge| edge.weight().is_projected())
        .map(|edge| edge.target());
    let incoming = graph
        .edges_directed(at, Direction::Incoming)
        .filter(|edge| edge.weight().is_projected())
        .map(|edge| edge.source());
    outgoing.chain(incoming)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::graph_builder::GraphBuilder;

    fn vpc(id: &str) -> Node {
        Node::AwsEc2Vpc(id.into())
    }

    fn attach(builder: &mut GraphBuilder, connector: Node, networks: &[&str]) {
        let index = builder.get_or_add_node(connector);
        for network in networks {
            builder.link_to(index, vpc(network), Edge::AttachedTo);
        }
    }

    #[test]
    fn every_join_names_a_kind_that_exists() {
        for join in JOINS {
            let (network, connector) = match join {
                Join::Through { network, connector } => (*network, Some(*connector)),
                Join::Direct { network } => (*network, None),
            };
            for kind in std::iter::once(network).chain(connector) {
                assert!(
                    Node::ALL_KINDS.contains(&kind),
                    "no Node variant is called {kind}"
                );
            }
            assert!(
                ["AwsEc2Vpc", "AzureVirtualNetwork", "GcpComputeNetwork"].contains(&network),
                "{network} is not a network, so it has no scope to join"
            );
        }
    }

    #[test]
    fn a_peering_joins_its_two_networks_and_nothing_else() {
        let mut builder = GraphBuilder::new();
        attach(
            &mut builder,
            Node::AwsEc2VpcPeeringConnection("pcx-ab".into()),
            &["vpc-a", "vpc-b"],
        );
        attach(
            &mut builder,
            Node::AwsEc2VpcPeeringConnection("pcx-bc".into()),
            &["vpc-b", "vpc-c"],
        );

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vpc-a", "vpc-b"));
        assert!(joins.joined("vpc-c", "vpc-b"));
        assert!(
            !joins.joined("vpc-a", "vpc-c"),
            "peering is not transitive: a and c share a peer, not a join"
        );
    }

    #[test]
    fn a_transit_gateway_joins_every_network_attached_to_it() {
        let mut builder = GraphBuilder::new();
        attach(
            &mut builder,
            Node::AwsEc2TransitGateway("tgw-1".into()),
            &["vpc-a", "vpc-b", "vpc-c"],
        );
        attach(
            &mut builder,
            Node::AwsEc2TransitGateway("tgw-2".into()),
            &["vpc-c", "vpc-d"],
        );

        let joins = Joins::of(&builder.graph);

        for (a, b) in [("vpc-a", "vpc-b"), ("vpc-a", "vpc-c"), ("vpc-b", "vpc-c")] {
            assert!(joins.joined(a, b) && joins.joined(b, a), "{a} and {b}");
        }
        assert!(joins.joined("vpc-c", "vpc-d"));
        assert!(
            !joins.joined("vpc-a", "vpc-d"),
            "two gateways that share a network are not one gateway"
        );
        assert!(!joins.joined("vpc-a", "vpc-a"));
    }

    #[test]
    fn networks_that_share_a_region_are_not_joined() {
        let mut builder = GraphBuilder::new();
        let region = builder.get_or_add_node(Node::AwsRegion("us-east-1".into()));
        builder.link_to(region, vpc("vpc-a"), Edge::Contains);
        builder.link_to(region, vpc("vpc-b"), Edge::Contains);

        assert!(Joins::of(&builder.graph).is_empty());
    }

    #[test]
    fn a_direct_peering_joins_in_either_direction() {
        let mut builder = GraphBuilder::new();
        let a = builder.get_or_add_node(Node::AzureVirtualNetwork("vnet-a".into()));
        builder.link_to(
            a,
            Node::AzureVirtualNetwork("vnet-b".into()),
            Edge::ConnectsTo,
        );

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vnet-a", "vnet-b"));
        assert!(joins.joined("vnet-b", "vnet-a"));
    }
}
