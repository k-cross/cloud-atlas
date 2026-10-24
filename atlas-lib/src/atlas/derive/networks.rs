use crate::atlas::definition::{Edge, Node};
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Connector {
    Hub(NodeIndex),
    Tunnel { naming: NodeIndex, named: NodeIndex },
}

impl Connector {
    fn partner(self) -> Connector {
        match self {
            Connector::Hub(hub) => Connector::Hub(hub),
            Connector::Tunnel { naming, named } => Connector::Tunnel {
                naming: named,
                named: naming,
            },
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
        ends: &'static [&'static [Step]],
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
        ends: &[
            &[
                step(Edge::AttachedTo, "AwsEc2VpnGateway"),
                step(Edge::AttachedTo, "AwsEc2VpnConnection"),
            ],
            &[
                step(Edge::AttachedTo, "AwsEc2TransitGateway"),
                step(Edge::AttachedTo, "AwsEc2VpnConnection"),
            ],
        ],
        holds: &[step(Edge::ConnectsTo, "GenericIpAddress")],
        names: &[
            step(Edge::AttachedTo, "AwsEc2CustomerGateway"),
            step(Edge::RoutesTo, "GenericIpAddress"),
        ],
    },
    Join::Tunnel {
        network: "AzureVirtualNetwork",
        ends: &[
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
        ends: &[&[step(Edge::AttachedTo, "GcpComputeVpnGateway")]],
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
    members: HashMap<&'g str, HashSet<Connector>>,
    peers: HashMap<&'g str, HashSet<&'g str>>,
}

struct TunnelEnd<'g> {
    networks: HashSet<&'g str>,
    holds: Vec<NodeIndex>,
    names: Vec<NodeIndex>,
}

impl TunnelEnd<'_> {
    fn of(graph: &Graph<Node, Edge>, end: NodeIndex, holds: &[Step], names: &[Step]) -> Self {
        let reach = |path: &[Step]| {
            let mut addresses = Vec::new();
            follow(graph, end, path, &mut addresses);
            addresses
        };
        TunnelEnd {
            networks: HashSet::new(),
            holds: reach(holds),
            names: reach(names),
        }
    }
}

impl<'g> Joins<'g> {
    pub(super) fn of(graph: &'g Graph<Node, Edge>) -> Self {
        let mut by_kind: HashMap<&str, Vec<NodeIndex>> = HashMap::new();
        for index in graph.node_indices() {
            by_kind.entry(graph[index].kind()).or_default().push(index);
        }
        let nodes_of = |kind: &str| by_kind.get(kind).map_or(&[][..], Vec::as_slice);

        let mut joins = Joins::default();
        let mut tunnel_ends: HashMap<NodeIndex, TunnelEnd> = HashMap::new();
        for join in JOINS {
            match join {
                Join::Through { network, connector } => {
                    for &hub in nodes_of(connector) {
                        for member in scopes_beside(graph, hub, network) {
                            joins
                                .members
                                .entry(member)
                                .or_default()
                                .insert(Connector::Hub(hub));
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
                    ends,
                    holds,
                    names,
                } => {
                    for &start in nodes_of(network) {
                        let Some(scope) = graph[start].network_scope() else {
                            continue;
                        };
                        let mut reached = Vec::new();
                        for path in *ends {
                            follow(graph, start, path, &mut reached);
                        }
                        for end in reached {
                            tunnel_ends
                                .entry(end)
                                .or_insert_with(|| TunnelEnd::of(graph, end, holds, names))
                                .networks
                                .insert(scope);
                        }
                    }
                }
            }
        }
        joins.pair(&tunnel_ends);
        joins
    }

    fn pair(&mut self, tunnel_ends: &HashMap<NodeIndex, TunnelEnd<'g>>) {
        let mut holders: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        for (&end, tunnel_end) in tunnel_ends {
            for &address in &tunnel_end.holds {
                holders.entry(address).or_default().push(end);
            }
        }
        for (&naming, naming_end) in tunnel_ends {
            let named_ends = naming_end
                .names
                .iter()
                .filter_map(|address| holders.get(address))
                .flatten()
                .filter(|&&named| named != naming);
            for &named in named_ends {
                for &network in &naming_end.networks {
                    self.members
                        .entry(network)
                        .or_default()
                        .insert(Connector::Tunnel { naming, named });
                }
            }
        }
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
                .any(|connector| right.contains(&connector.partner())),
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
                    ends,
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
                    let steps = ends.iter().copied().flatten().chain(*holds).chain(*names);
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

    fn vpn_gateway(builder: &mut GraphBuilder, vpc_id: &str) -> Node {
        let gateway = Node::AwsEc2VpnGateway(format!("vgw-{vpc_id}").into());
        let index = builder.get_or_add_node(gateway.clone());
        builder.link_to(index, vpc(vpc_id), Edge::AttachedTo);
        gateway
    }

    fn vpn_connection(builder: &mut GraphBuilder, on: Node, tag: &str, holds: &str, names: &str) {
        let connection =
            builder.get_or_add_node(Node::AwsEc2VpnConnection(format!("vpn-{tag}").into()));
        builder.link_to(connection, on, Edge::AttachedTo);
        builder.link_to(connection, public(holds), Edge::ConnectsTo);
        let customer_gateway = builder.link_to(
            connection,
            Node::AwsEc2CustomerGateway(format!("cgw-{tag}").into()),
            Edge::AttachedTo,
        );
        builder.link_to(customer_gateway, public(names), Edge::RoutesTo);
    }

    fn aws_end(builder: &mut GraphBuilder, vpc_id: &str, holds: &str, names: &str) {
        let gateway = vpn_gateway(builder, vpc_id);
        vpn_connection(builder, gateway, vpc_id, holds, names);
    }

    fn azure_gateway(builder: &mut GraphBuilder, vnet: &str, holds: &str) {
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
        builder.link_to(pip, public(holds), Edge::ConnectsTo);
    }

    fn azure_connection(builder: &mut GraphBuilder, vnet: &str, tag: &str, names: &str) {
        let connection =
            builder.get_or_add_node(Node::AzureVpnConnection(format!("conn-{tag}").into()));
        builder.link_to(
            connection,
            Node::AzureVirtualNetworkGateway(format!("vng-{vnet}").into()),
            Edge::AttachedTo,
        );
        let local_gateway = builder.link_to(
            connection,
            Node::AzureLocalNetworkGateway(format!("lng-{tag}").into()),
            Edge::AttachedTo,
        );
        builder.link_to(local_gateway, public(names), Edge::RoutesTo);
    }

    #[test]
    fn a_tunnel_joins_the_networks_at_its_two_ends() {
        let mut builder = GraphBuilder::new();
        aws_end(&mut builder, "vpc-a", "192.0.2.1", "198.51.100.9");
        azure_gateway(&mut builder, "vnet-hub", "198.51.100.9");
        azure_connection(&mut builder, "vnet-hub", "a", "192.0.2.1");

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vpc-a", "vnet-hub"));
        assert!(joins.joined("vnet-hub", "vpc-a"));
    }

    #[test]
    fn a_tunnel_configured_at_one_end_joins_nothing() {
        let mut builder = GraphBuilder::new();
        aws_end(&mut builder, "vpc-a", "192.0.2.1", "198.51.100.9");
        azure_gateway(&mut builder, "vnet-hub", "198.51.100.9");
        aws_end(&mut builder, "vpc-b", "192.0.2.2", "203.0.113.200");
        azure_gateway(&mut builder, "vnet-spoke", "198.51.100.10");
        azure_connection(&mut builder, "vnet-spoke", "b", "192.0.2.2");

        let joins = Joins::of(&builder.graph);

        assert!(
            !joins.joined("vpc-a", "vnet-hub"),
            "the Azure gateway names no AWS address"
        );
        assert!(
            !joins.joined("vpc-b", "vnet-spoke"),
            "the AWS connection names an address Azure does not hold"
        );
    }

    #[test]
    fn a_gateway_pairs_each_of_its_connections_on_its_own() {
        let mut builder = GraphBuilder::new();
        let gateway = vpn_gateway(&mut builder, "vpc-a");
        vpn_connection(
            &mut builder,
            gateway.clone(),
            "to-azure",
            "192.0.2.1",
            "198.51.100.9",
        );
        vpn_connection(
            &mut builder,
            gateway,
            "to-office",
            "192.0.2.2",
            "203.0.113.200",
        );
        azure_gateway(&mut builder, "vnet-hub", "198.51.100.9");
        azure_connection(&mut builder, "vnet-hub", "office", "192.0.2.2");

        let joins = Joins::of(&builder.graph);

        assert!(
            !joins.joined("vpc-a", "vnet-hub"),
            "the connection Azure names does not name Azure"
        );
    }

    #[test]
    fn two_networks_tunnelled_to_the_same_gateway_are_not_joined_to_each_other() {
        let mut builder = GraphBuilder::new();
        aws_end(&mut builder, "vpc-a", "192.0.2.1", "198.51.100.9");
        aws_end(&mut builder, "vpc-b", "192.0.2.2", "198.51.100.9");
        azure_gateway(&mut builder, "vnet-hub", "198.51.100.9");
        azure_connection(&mut builder, "vnet-hub", "a", "192.0.2.1");
        azure_connection(&mut builder, "vnet-hub", "b", "192.0.2.2");

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vpc-a", "vnet-hub") && joins.joined("vpc-b", "vnet-hub"));
        assert!(!joins.joined("vpc-a", "vpc-b"));
    }

    #[test]
    fn a_tunnel_to_a_gateway_the_scan_does_not_model_joins_nothing() {
        let mut builder = GraphBuilder::new();
        aws_end(&mut builder, "vpc-a", "192.0.2.1", "198.51.100.9");

        let joins = Joins::of(&builder.graph);

        assert!(!joins.joined("vpc-a", "vnet-hub"));
        assert!(!joins.joined("vpc-a", "vpc-a"));
    }

    #[test]
    fn only_a_peering_that_uses_remote_gateways_rides_the_hub_gateway() {
        let mut builder = GraphBuilder::new();
        aws_end(&mut builder, "vpc-a", "192.0.2.1", "198.51.100.9");
        azure_gateway(&mut builder, "vnet-hub", "198.51.100.9");
        azure_connection(&mut builder, "vnet-hub", "a", "192.0.2.1");
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
        vpn_connection(
            &mut builder,
            Node::AwsEc2TransitGateway("tgw-1".into()),
            "tgw",
            "192.0.2.1",
            "203.0.113.20",
        );
        let gcp = builder.get_or_add_node(Node::GcpComputeVpnGateway("ha-1".into()));
        builder.link_to(
            gcp,
            Node::GcpComputeNetwork("net-gcp".into()),
            Edge::AttachedTo,
        );
        builder.link_to(gcp, public("203.0.113.20"), Edge::ConnectsTo);
        let external = builder.get_or_add_node(Node::GcpComputeExternalVpnGateway("peer".into()));
        builder.link_to(external, public("192.0.2.1"), Edge::RoutesTo);
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
