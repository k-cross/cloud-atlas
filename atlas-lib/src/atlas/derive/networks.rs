use crate::atlas::definition::{Edge, Node};
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Connector {
    Hub(NodeIndex),
    Link { naming: NodeIndex, named: NodeIndex },
}

impl Connector {
    fn partner(self) -> Connector {
        match self {
            Connector::Hub(hub) => Connector::Hub(hub),
            Connector::Link { naming, named } => Connector::Link {
                naming: named,
                named: naming,
            },
        }
    }
}

struct Step {
    edge: Edge,
    toward: Direction,
    kind: &'static str,
}

const fn to(edge: Edge, kind: &'static str) -> Step {
    Step {
        edge,
        toward: Direction::Outgoing,
        kind,
    }
}

const fn from(edge: Edge, kind: &'static str) -> Step {
    Step {
        edge,
        toward: Direction::Incoming,
        kind,
    }
}

enum Join {
    Through {
        network: &'static str,
        connector: &'static str,
    },
    Direct {
        network: &'static str,
    },
    Link {
        network: &'static str,
        ends: &'static [&'static [Step]],
        holds: &'static [&'static [Step]],
        names: &'static [&'static [Step]],
    },
}

const BGP_HOLDS: &[&[Step]] = &[
    &[to(Edge::ConnectsTo, "GenericPrivateIpAddress")],
    &[to(Edge::ConnectsTo, "GenericIpAddress")],
];

const BGP_NAMES: &[&[Step]] = &[
    &[to(Edge::RoutesTo, "GenericPrivateIpAddress")],
    &[to(Edge::RoutesTo, "GenericIpAddress")],
];

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
    Join::Link {
        network: "AwsEc2Vpc",
        ends: &[
            &[
                from(Edge::AttachedTo, "AwsEc2VpnGateway"),
                from(Edge::AttachedTo, "AwsEc2VpnConnection"),
            ],
            &[
                from(Edge::AttachedTo, "AwsEc2TransitGateway"),
                from(Edge::AttachedTo, "AwsEc2VpnConnection"),
            ],
        ],
        holds: &[&[to(Edge::ConnectsTo, "GenericIpAddress")]],
        names: &[&[
            to(Edge::AttachedTo, "AwsEc2CustomerGateway"),
            to(Edge::RoutesTo, "GenericIpAddress"),
        ]],
    },
    Join::Link {
        network: "AzureVirtualNetwork",
        ends: &[
            &[from(Edge::AttachedTo, "AzureVirtualNetworkGateway")],
            &[
                to(Edge::RoutesTo, "AzureVirtualNetwork"),
                from(Edge::AttachedTo, "AzureVirtualNetworkGateway"),
            ],
        ],
        holds: &[&[
            to(Edge::HasIp, "AzurePublicIpAddress"),
            to(Edge::ConnectsTo, "GenericIpAddress"),
        ]],
        names: &[&[
            from(Edge::AttachedTo, "AzureGatewayConnection"),
            to(Edge::AttachedTo, "AzureLocalNetworkGateway"),
            to(Edge::RoutesTo, "GenericIpAddress"),
        ]],
    },
    Join::Link {
        network: "GcpComputeNetwork",
        ends: &[&[from(Edge::AttachedTo, "GcpComputeVpnGateway")]],
        holds: &[&[to(Edge::ConnectsTo, "GenericIpAddress")]],
        names: &[&[
            from(Edge::AttachedTo, "GcpComputeVpnTunnel"),
            to(Edge::AttachedTo, "GcpComputeExternalVpnGateway"),
            to(Edge::RoutesTo, "GenericIpAddress"),
        ]],
    },
    Join::Link {
        network: "AwsEc2Vpc",
        ends: &[
            &[
                from(Edge::AttachedTo, "AwsEc2VpnGateway"),
                from(Edge::AttachedTo, "AwsDirectConnectVirtualInterface"),
            ],
            &[
                from(Edge::AttachedTo, "AwsEc2VpnGateway"),
                to(Edge::AttachedTo, "AwsDirectConnectGateway"),
                from(Edge::AttachedTo, "AwsDirectConnectVirtualInterface"),
            ],
            &[
                from(Edge::AttachedTo, "AwsEc2TransitGateway"),
                to(Edge::AttachedTo, "AwsDirectConnectGateway"),
                from(Edge::AttachedTo, "AwsDirectConnectVirtualInterface"),
            ],
        ],
        holds: BGP_HOLDS,
        names: BGP_NAMES,
    },
    Join::Link {
        network: "AzureVirtualNetwork",
        ends: &[
            &[
                from(Edge::AttachedTo, "AzureVirtualNetworkGateway"),
                from(Edge::AttachedTo, "AzureGatewayConnection"),
                to(Edge::AttachedTo, "AzureExpressRouteCircuit"),
            ],
            &[
                to(Edge::RoutesTo, "AzureVirtualNetwork"),
                from(Edge::AttachedTo, "AzureVirtualNetworkGateway"),
                from(Edge::AttachedTo, "AzureGatewayConnection"),
                to(Edge::AttachedTo, "AzureExpressRouteCircuit"),
            ],
        ],
        holds: BGP_HOLDS,
        names: BGP_NAMES,
    },
    Join::Link {
        network: "GcpComputeNetwork",
        ends: &[&[
            from(Edge::AttachedTo, "GcpComputeRouter"),
            from(Edge::AttachedTo, "GcpComputeInterconnectAttachment"),
        ]],
        holds: BGP_HOLDS,
        names: BGP_NAMES,
    },
];

#[derive(Default)]
pub(super) struct Joins<'g> {
    members: HashMap<&'g str, HashSet<Connector>>,
    peers: HashMap<&'g str, HashSet<&'g str>>,
}

struct LinkEnd<'g> {
    networks: HashSet<&'g str>,
    holds: Vec<NodeIndex>,
    names: Vec<NodeIndex>,
}

impl LinkEnd<'_> {
    fn of(graph: &Graph<Node, Edge>, end: NodeIndex, holds: &[&[Step]], names: &[&[Step]]) -> Self {
        let reach = |paths: &[&[Step]]| {
            let mut addresses = Vec::new();
            for path in paths {
                follow(graph, end, path, &mut addresses);
            }
            addresses
        };
        LinkEnd {
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
        let mut link_ends: HashMap<NodeIndex, LinkEnd> = HashMap::new();
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
                Join::Link {
                    network,
                    ends,
                    holds,
                    names,
                } => {
                    for path in *ends {
                        let Some(end_kind) = path.last().map(|step| step.kind) else {
                            continue;
                        };
                        for &end in nodes_of(end_kind) {
                            let mut starts = Vec::new();
                            follow_back(graph, end, path, network, &mut starts);
                            let mut scopes = starts
                                .into_iter()
                                .filter_map(|start| graph[start].network_scope())
                                .peekable();
                            if scopes.peek().is_none() {
                                continue;
                            }
                            link_ends
                                .entry(end)
                                .or_insert_with(|| LinkEnd::of(graph, end, holds, names))
                                .networks
                                .extend(scopes);
                        }
                    }
                }
            }
        }
        joins.pair(&link_ends);
        joins
    }

    fn pair(&mut self, link_ends: &HashMap<NodeIndex, LinkEnd<'g>>) {
        let mut holders: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        for (&end, link_end) in link_ends {
            for &address in &link_end.holds {
                holders.entry(address).or_default().push(end);
            }
        }
        for (&naming, naming_end) in link_ends {
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
                        .insert(Connector::Link { naming, named });
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

fn stepped<'g>(
    graph: &'g Graph<Node, Edge>,
    at: NodeIndex,
    edge: &'g Edge,
    toward: Direction,
    kind: &'g str,
) -> impl Iterator<Item = NodeIndex> + 'g {
    graph
        .edges_directed(at, toward)
        .filter(move |link| link.weight() == edge)
        .map(move |link| match toward {
            Direction::Outgoing => link.target(),
            Direction::Incoming => link.source(),
        })
        .filter(move |next| graph[*next].kind() == kind)
}

fn follow(graph: &Graph<Node, Edge>, at: NodeIndex, path: &[Step], ends: &mut Vec<NodeIndex>) {
    let Some((step, rest)) = path.split_first() else {
        ends.push(at);
        return;
    };
    for next in stepped(graph, at, &step.edge, step.toward, step.kind) {
        follow(graph, next, rest, ends);
    }
}

fn follow_back(
    graph: &Graph<Node, Edge>,
    at: NodeIndex,
    path: &[Step],
    origin: &str,
    starts: &mut Vec<NodeIndex>,
) {
    let Some((step, before)) = path.split_last() else {
        starts.push(at);
        return;
    };
    let kind = before.last().map_or(origin, |previous| previous.kind);
    for next in stepped(graph, at, &step.edge, step.toward.opposite(), kind) {
        follow_back(graph, next, before, origin, starts);
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
                Join::Link {
                    network,
                    ends,
                    holds,
                    names,
                } => {
                    for path in holds.iter().chain(*names) {
                        assert!(
                            matches!(
                                path.last().map(|step| step.kind),
                                Some("GenericIpAddress" | "GenericPrivateIpAddress")
                            ),
                            "a link meets at an address"
                        );
                    }
                    let steps = [*ends, *holds, *names]
                        .into_iter()
                        .flatten()
                        .copied()
                        .flatten();
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
    fn every_link_end_kind_belongs_to_one_row() {
        let mut rows_by_end: HashMap<&str, usize> = HashMap::new();
        for (row, join) in JOINS.iter().enumerate() {
            let Join::Link { ends, .. } = join else {
                continue;
            };
            for path in *ends {
                let end = path.last().expect("a path reaches its end").kind;
                let first = *rows_by_end.entry(end).or_insert(row);
                assert_eq!(
                    first, row,
                    "{end} ends two Link rows, but an end keeps the holds and names of whichever \
                     row reaches it first"
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
            builder.get_or_add_node(Node::AzureGatewayConnection(format!("conn-{tag}").into()));
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
    fn a_hub_never_rides_the_gateway_of_a_network_that_routes_through_it() {
        let mut builder = GraphBuilder::new();
        aws_end(&mut builder, "vpc-a", "192.0.2.1", "198.51.100.9");
        azure_gateway(&mut builder, "vnet-spoke", "198.51.100.9");
        azure_connection(&mut builder, "vnet-spoke", "a", "192.0.2.1");
        let spoke = builder.get_or_add_node(Node::AzureVirtualNetwork("vnet-spoke".into()));
        builder.link_to(
            spoke,
            Node::AzureVirtualNetwork("vnet-hub".into()),
            Edge::RoutesTo,
        );

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vnet-spoke", "vpc-a"));
        assert!(
            !joins.joined("vnet-hub", "vpc-a"),
            "gateway transit runs from the network that uses the remote gateway, never back"
        );
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

    fn link(address: &str) -> Node {
        Node::link_ip(address).expect("an address with its link prefix")
    }

    fn dx_interface(builder: &mut GraphBuilder, on: Node, tag: &str, holds: &str, names: &str) {
        let interface = builder.get_or_add_node(Node::AwsDirectConnectVirtualInterface(
            format!("dxvif-{tag}").into(),
        ));
        builder.link_to(interface, on, Edge::AttachedTo);
        builder.link_to(interface, link(holds), Edge::ConnectsTo);
        builder.link_to(interface, link(names), Edge::RoutesTo);
    }

    fn gcp_attachment(builder: &mut GraphBuilder, network: &str, holds: &str, names: &str) {
        let router =
            builder.get_or_add_node(Node::GcpComputeRouter(format!("rtr-{network}").into()));
        builder.link_to(
            router,
            Node::GcpComputeNetwork(network.into()),
            Edge::AttachedTo,
        );
        let attachment = builder.get_or_add_node(Node::GcpComputeInterconnectAttachment(
            format!("att-{network}-{holds}").into(),
        ));
        builder.link_to(
            attachment,
            Node::GcpComputeRouter(format!("rtr-{network}").into()),
            Edge::AttachedTo,
        );
        builder.link_to(attachment, link(holds), Edge::ConnectsTo);
        builder.link_to(attachment, link(names), Edge::RoutesTo);
    }

    #[test]
    fn a_direct_connect_interface_and_an_interconnect_attachment_join_their_networks() {
        let mut builder = GraphBuilder::new();
        let gateway = vpn_gateway(&mut builder, "vpc-a");
        dx_interface(
            &mut builder,
            gateway,
            "a",
            "169.254.1.1/30",
            "169.254.1.2/30",
        );
        gcp_attachment(&mut builder, "net-gcp", "169.254.1.2/30", "169.254.1.1/30");

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vpc-a", "net-gcp"));
        assert!(joins.joined("net-gcp", "vpc-a"));
    }

    #[test]
    fn networks_behind_one_direct_connect_gateway_reach_its_circuit_but_not_each_other() {
        let mut builder = GraphBuilder::new();
        let dx_gateway = Node::AwsDirectConnectGateway("dxgw-1".into());
        for vpc_id in ["vpc-a", "vpc-b"] {
            let gateway = vpn_gateway(&mut builder, vpc_id);
            let gateway = builder.get_or_add_node(gateway);
            builder.link_to(gateway, dx_gateway.clone(), Edge::AttachedTo);
        }
        attach(
            &mut builder,
            Node::AwsEc2TransitGateway("tgw-1".into()),
            &["vpc-c", "vpc-d"],
        );
        let transit = builder.get_or_add_node(Node::AwsEc2TransitGateway("tgw-1".into()));
        builder.link_to(transit, dx_gateway.clone(), Edge::AttachedTo);
        dx_interface(
            &mut builder,
            dx_gateway,
            "shared",
            "169.254.1.1/30",
            "169.254.1.2/30",
        );
        gcp_attachment(&mut builder, "net-gcp", "169.254.1.2/30", "169.254.1.1/30");

        let joins = Joins::of(&builder.graph);

        for vpc_id in ["vpc-a", "vpc-b", "vpc-c", "vpc-d"] {
            assert!(joins.joined(vpc_id, "net-gcp"), "{vpc_id}");
        }
        assert!(
            !joins.joined("vpc-a", "vpc-b"),
            "a Direct Connect gateway does not route between the gateways on it"
        );
    }

    #[test]
    fn an_expressroute_circuit_joins_the_hub_and_the_spokes_that_use_its_gateway() {
        let mut builder = GraphBuilder::new();
        let gateway = builder.get_or_add_node(Node::AzureVirtualNetworkGateway("ergw-hub".into()));
        builder.link_to(
            gateway,
            Node::AzureVirtualNetwork("vnet-hub".into()),
            Edge::AttachedTo,
        );
        let connection = builder.get_or_add_node(Node::AzureGatewayConnection("conn-er".into()));
        builder.link_to(
            connection,
            Node::AzureVirtualNetworkGateway("ergw-hub".into()),
            Edge::AttachedTo,
        );
        let circuit = builder.link_to(
            connection,
            Node::AzureExpressRouteCircuit("er-1".into()),
            Edge::AttachedTo,
        );
        builder.link_to(circuit, link("169.254.70.2/30"), Edge::ConnectsTo);
        builder.link_to(circuit, link("169.254.70.1/30"), Edge::RoutesTo);
        gcp_attachment(
            &mut builder,
            "net-gcp",
            "169.254.70.1/30",
            "169.254.70.2/30",
        );
        let hub = Node::AzureVirtualNetwork("vnet-hub".into());
        let spoke = builder.get_or_add_node(Node::AzureVirtualNetwork("vnet-spoke".into()));
        builder.link_to(spoke, hub.clone(), Edge::ConnectsTo);
        builder.link_to(spoke, hub.clone(), Edge::RoutesTo);
        let plain = builder.get_or_add_node(Node::AzureVirtualNetwork("vnet-plain".into()));
        builder.link_to(plain, hub, Edge::ConnectsTo);

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vnet-hub", "net-gcp"));
        assert!(joins.joined("vnet-spoke", "net-gcp"));
        assert!(!joins.joined("vnet-plain", "net-gcp"));
    }

    #[test]
    fn circuits_that_end_at_routers_the_scan_does_not_model_join_nothing() {
        let mut builder = GraphBuilder::new();
        let gateway = vpn_gateway(&mut builder, "vpc-a");
        dx_interface(
            &mut builder,
            gateway,
            "office",
            "169.254.1.1/30",
            "169.254.1.2/30",
        );
        gcp_attachment(
            &mut builder,
            "net-office",
            "169.254.2.1/30",
            "169.254.2.2/30",
        );
        gcp_attachment(
            &mut builder,
            "net-same-side",
            "169.254.1.1/30",
            "169.254.1.2/30",
        );

        let joins = Joins::of(&builder.graph);

        assert!(!joins.joined("vpc-a", "net-office"));
        assert!(
            !joins.joined("vpc-a", "net-same-side"),
            "two ends that both hold the cloud address of a link are not its two ends"
        );
    }
}
