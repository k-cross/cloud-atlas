use crate::atlas::definition::{Edge, Node};
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Side {
    Mesh,
    Holds,
    Names,
}

impl Side {
    fn partner(self) -> Side {
        match self {
            Side::Mesh => Side::Mesh,
            Side::Holds => Side::Names,
            Side::Names => Side::Holds,
        }
    }
}

struct Step {
    edge: Edge,
    kind: &'static str,
}

const fn step(edge: Edge, kind: &'static str) -> Step {
    Step { edge, kind }
}

enum Join {
    Through {
        network: &'static str,
        connector: &'static str,
    },
    Direct {
        network: &'static str,
    },
    Tunnel {
        network: &'static str,
        gateways: &'static [&'static [Step]],
        holds: &'static [Step],
        names: &'static [Step],
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
    Join::Tunnel {
        network: "AwsEc2Vpc",
        gateways: &[
            &[step(Edge::AttachedTo, "AwsEc2VpnGateway")],
            &[step(Edge::AttachedTo, "AwsEc2TransitGateway")],
        ],
        holds: &[
            step(Edge::AttachedTo, "AwsEc2VpnConnection"),
            step(Edge::ConnectsTo, "GenericIpAddress"),
        ],
        names: &[
            step(Edge::AttachedTo, "AwsEc2VpnConnection"),
            step(Edge::AttachedTo, "AwsEc2CustomerGateway"),
            step(Edge::RoutesTo, "GenericIpAddress"),
        ],
    },
    Join::Tunnel {
        network: "AzureVirtualNetwork",
        gateways: &[
            &[step(Edge::AttachedTo, "AzureVirtualNetworkGateway")],
            &[
                step(Edge::RoutesTo, "AzureVirtualNetwork"),
                step(Edge::AttachedTo, "AzureVirtualNetworkGateway"),
            ],
        ],
        holds: &[
            step(Edge::HasIp, "AzurePublicIpAddress"),
            step(Edge::ConnectsTo, "GenericIpAddress"),
        ],
        names: &[
            step(Edge::AttachedTo, "AzureVpnConnection"),
            step(Edge::AttachedTo, "AzureLocalNetworkGateway"),
            step(Edge::RoutesTo, "GenericIpAddress"),
        ],
    },
    Join::Tunnel {
        network: "GcpComputeNetwork",
        gateways: &[&[step(Edge::AttachedTo, "GcpComputeVpnGateway")]],
        holds: &[step(Edge::ConnectsTo, "GenericIpAddress")],
        names: &[
            step(Edge::AttachedTo, "GcpComputeVpnTunnel"),
            step(Edge::AttachedTo, "GcpComputeExternalVpnGateway"),
            step(Edge::RoutesTo, "GenericIpAddress"),
        ],
    },
];

#[derive(Default)]
pub(super) struct Joins<'g> {
    members: HashMap<&'g str, HashSet<(NodeIndex, Side)>>,
    peers: HashMap<&'g str, HashSet<&'g str>>,
}

impl<'g> Joins<'g> {
    pub(super) fn of(graph: &'g Graph<Node, Edge>) -> Self {
        let mut by_kind: HashMap<&str, Vec<NodeIndex>> = HashMap::new();
        for index in graph.node_indices() {
            by_kind.entry(graph[index].kind()).or_default().push(index);
        }
        let nodes_of = |kind: &str| by_kind.get(kind).map_or(&[][..], Vec::as_slice);

        let mut joins = Joins::default();
        for join in JOINS {
            match join {
                Join::Through { network, connector } => {
                    for &hub in nodes_of(connector) {
                        for member in scopes_beside(graph, hub, network) {
                            joins
                                .members
                                .entry(member)
                                .or_default()
                                .insert((hub, Side::Mesh));
                        }
                    }
                }
                Join::Direct { network } => {
                    for &start in nodes_of(network) {
                        let Some(from) = graph[start].network_scope() else {
                            continue;
                        };
                        for to in scopes_beside(graph, start, network).filter(|to| *to != from) {
                            joins.peers.entry(from).or_default().insert(to);
                            joins.peers.entry(to).or_default().insert(from);
                        }
                    }
                }
                Join::Tunnel {
                    network,
                    gateways,
                    holds,
                    names,
                } => {
                    for &start in nodes_of(network) {
                        let Some(scope) = graph[start].network_scope() else {
                            continue;
                        };
                        let mut reached = Vec::new();
                        for path in *gateways {
                            follow(graph, start, path, &mut reached);
                        }
                        for (side, path) in [(Side::Holds, holds), (Side::Names, names)] {
                            let mut pivots = Vec::new();
                            for &gateway in &reached {
                                follow(graph, gateway, path, &mut pivots);
                            }
                            for pivot in pivots {
                                joins
                                    .members
                                    .entry(scope)
                                    .or_default()
                                    .insert((pivot, side));
                            }
                        }
                    }
                }
            }
        }
        joins
    }

    pub(super) fn is_empty(&self) -> bool {
        self.members.is_empty() && self.peers.is_empty()
    }

    pub(super) fn joined(&self, a: &str, b: &str) -> bool {
        if a == b {
            return false;
        }
        if self.peers.get(a).is_some_and(|peers| peers.contains(b)) {
            return true;
        }
        match (self.members.get(a), self.members.get(b)) {
            (Some(left), Some(right)) => left
                .iter()
                .any(|(connector, side)| right.contains(&(*connector, side.partner()))),
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

fn scopes_beside<'g>(
    graph: &'g Graph<Node, Edge>,
    at: NodeIndex,
    kind: &'g str,
) -> impl Iterator<Item = &'g str> + 'g {
    neighbours(graph, at)
        .filter(move |next| graph[*next].kind() == kind)
        .filter_map(|next| graph[next].network_scope())
}

fn linked(
    graph: &Graph<Node, Edge>,
    at: NodeIndex,
) -> impl Iterator<Item = (NodeIndex, &Edge)> + '_ {
    let outgoing = graph
        .edges_directed(at, Direction::Outgoing)
        .map(|edge| (edge.target(), edge.weight()));
    let incoming = graph
        .edges_directed(at, Direction::Incoming)
        .map(|edge| (edge.source(), edge.weight()));
    outgoing
        .chain(incoming)
        .filter(|(_, edge)| edge.is_projected())
}

fn neighbours(graph: &Graph<Node, Edge>, at: NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
    linked(graph, at).map(|(next, _)| next)
}

fn follow(graph: &Graph<Node, Edge>, at: NodeIndex, path: &[Step], ends: &mut Vec<NodeIndex>) {
    let Some((step, rest)) = path.split_first() else {
        ends.push(at);
        return;
    };
    for (next, edge) in linked(graph, at) {
        if *edge == step.edge && graph[next].kind() == step.kind {
            follow(graph, next, rest, ends);
        }
    }
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
        let networks = ["AwsEc2Vpc", "AzureVirtualNetwork", "GcpComputeNetwork"];
        for join in JOINS {
            let (network, kinds): (&str, Vec<&str>) = match join {
                Join::Through { network, connector } => (*network, vec![*connector]),
                Join::Direct { network } => (*network, Vec::new()),
                Join::Tunnel {
                    network,
                    gateways,
                    holds,
                    names,
                } => {
                    for path in [holds, names] {
                        assert_eq!(
                            path.last().map(|step| step.kind),
                            Some("GenericIpAddress"),
                            "a tunnel meets at a public address"
                        );
                    }
                    let steps = gateways
                        .iter()
                        .copied()
                        .flatten()
                        .chain(*holds)
                        .chain(*names);
                    (*network, steps.map(|step| step.kind).collect())
                }
            };
            assert!(
                networks.contains(&network),
                "{network} is not a network, so it has no scope to join"
            );
            for kind in std::iter::once(network).chain(kinds) {
                assert!(
                    Node::ALL_KINDS.contains(&kind),
                    "no Node variant is called {kind}"
                );
            }
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

    fn public(address: &str) -> Node {
        Node::public_ip(address).expect("a public address")
    }

    fn aws_names(builder: &mut GraphBuilder, vpc_id: &str, tag: &str, far: &str) {
        let vgw = builder.get_or_add_node(Node::AwsEc2VpnGateway(format!("vgw-{tag}").into()));
        builder.link_to(vgw, vpc(vpc_id), Edge::AttachedTo);
        let connection =
            builder.get_or_add_node(Node::AwsEc2VpnConnection(format!("vpn-{tag}").into()));
        builder.link_to(
            connection,
            Node::AwsEc2VpnGateway(format!("vgw-{tag}").into()),
            Edge::AttachedTo,
        );
        let cgw = builder.get_or_add_node(Node::AwsEc2CustomerGateway(format!("cgw-{tag}").into()));
        builder.link_to(
            connection,
            Node::AwsEc2CustomerGateway(format!("cgw-{tag}").into()),
            Edge::AttachedTo,
        );
        builder.link_to(cgw, public(far), Edge::RoutesTo);
    }

    fn azure_holds(builder: &mut GraphBuilder, vnet: &str, address: &str) -> NodeIndex {
        let gateway = builder.get_or_add_node(Node::AzureVirtualNetworkGateway(
            format!("vng-{vnet}").into(),
        ));
        builder.link_to(
            gateway,
            Node::AzureVirtualNetwork(vnet.into()),
            Edge::AttachedTo,
        );
        let pip = builder.link_to(
            gateway,
            Node::AzurePublicIpAddress(format!("pip-{vnet}").into()),
            Edge::HasIp,
        );
        builder.link_to(pip, public(address), Edge::ConnectsTo);
        gateway
    }

    #[test]
    fn a_tunnel_joins_the_network_that_holds_the_address_to_the_one_that_names_it() {
        let mut builder = GraphBuilder::new();
        aws_names(&mut builder, "vpc-a", "a", "198.51.100.9");
        azure_holds(&mut builder, "vnet-hub", "198.51.100.9");

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vpc-a", "vnet-hub"));
        assert!(joins.joined("vnet-hub", "vpc-a"));
    }

    #[test]
    fn two_networks_naming_the_same_gateway_are_not_joined_to_each_other() {
        let mut builder = GraphBuilder::new();
        aws_names(&mut builder, "vpc-a", "a", "198.51.100.9");
        aws_names(&mut builder, "vpc-b", "b", "198.51.100.9");
        azure_holds(&mut builder, "vnet-hub", "198.51.100.9");

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vpc-a", "vnet-hub") && joins.joined("vpc-b", "vnet-hub"));
        assert!(!joins.joined("vpc-a", "vpc-b"));
    }

    #[test]
    fn a_tunnel_to_a_gateway_the_scan_does_not_model_joins_nothing() {
        let mut builder = GraphBuilder::new();
        aws_names(&mut builder, "vpc-a", "a", "198.51.100.9");

        let joins = Joins::of(&builder.graph);

        assert!(!joins.joined("vpc-a", "vnet-hub"));
        assert!(!joins.joined("vpc-a", "vpc-a"));
    }

    #[test]
    fn only_a_peering_that_uses_remote_gateways_rides_the_hub_gateway() {
        let mut builder = GraphBuilder::new();
        aws_names(&mut builder, "vpc-a", "a", "198.51.100.9");
        azure_holds(&mut builder, "vnet-hub", "198.51.100.9");
        let hub = builder.get_or_add_node(Node::AzureVirtualNetwork("vnet-hub".into()));
        builder.link_to(
            hub,
            Node::AzureVirtualNetwork("vnet-plain".into()),
            Edge::ConnectsTo,
        );
        let transit = builder.get_or_add_node(Node::AzureVirtualNetwork("vnet-transit".into()));
        builder.link_to(
            transit,
            Node::AzureVirtualNetwork("vnet-hub".into()),
            Edge::ConnectsTo,
        );
        builder.link_to(
            transit,
            Node::AzureVirtualNetwork("vnet-hub".into()),
            Edge::RoutesTo,
        );

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vnet-transit", "vpc-a"));
        assert!(!joins.joined("vnet-plain", "vpc-a"));
    }

    #[test]
    fn a_vpn_on_a_transit_gateway_joins_every_network_attached_to_it() {
        let mut builder = GraphBuilder::new();
        attach(
            &mut builder,
            Node::AwsEc2TransitGateway("tgw-1".into()),
            &["vpc-a", "vpc-b"],
        );
        let connection = builder.get_or_add_node(Node::AwsEc2VpnConnection("vpn-1".into()));
        builder.link_to(
            connection,
            Node::AwsEc2TransitGateway("tgw-1".into()),
            Edge::AttachedTo,
        );
        builder.link_to(connection, public("203.0.113.9"), Edge::ConnectsTo);
        let gcp = builder.get_or_add_node(Node::GcpComputeVpnGateway("ha-1".into()));
        builder.link_to(
            gcp,
            Node::GcpComputeNetwork("net-gcp".into()),
            Edge::AttachedTo,
        );
        let external = builder.get_or_add_node(Node::GcpComputeExternalVpnGateway("peer".into()));
        builder.link_to(external, public("203.0.113.9"), Edge::RoutesTo);
        let tunnel = builder.get_or_add_node(Node::GcpComputeVpnTunnel("tun-1".into()));
        builder.link_to(
            tunnel,
            Node::GcpComputeVpnGateway("ha-1".into()),
            Edge::AttachedTo,
        );
        builder.link_to(
            tunnel,
            Node::GcpComputeExternalVpnGateway("peer".into()),
            Edge::AttachedTo,
        );

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("net-gcp", "vpc-a"));
        assert!(joins.joined("net-gcp", "vpc-b"));
    }
}
