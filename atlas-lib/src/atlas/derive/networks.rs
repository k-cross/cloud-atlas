use super::routing::Routing;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::util::parse_address;
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Connector {
    Hub(NodeIndex),
    Edge(NodeIndex),
    Link { naming: NodeIndex, named: NodeIndex },
}

impl Connector {
    fn meets(self) -> impl Iterator<Item = Connector> {
        let partners = match self {
            Connector::Hub(hub) => [Some(Connector::Hub(hub)), Some(Connector::Edge(hub))],
            Connector::Edge(hub) => [Some(Connector::Hub(hub)), None],
            Connector::Link { naming, named } => [
                Some(Connector::Link {
                    naming: named,
                    named: naming,
                }),
                None,
            ],
        };
        partners.into_iter().flatten()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Approach {
    via: NodeIndex,
    beyond: Option<NodeIndex>,
}

impl Approach {
    fn along(trail: &[NodeIndex]) -> Option<Approach> {
        Some(Approach {
            via: *trail.get(1)?,
            beyond: trail.get(2).copied(),
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Crossing {
    pub(super) via: NodeIndex,
    pub(super) entrant: NodeIndex,
    pub(super) toward: NodeIndex,
    pub(super) onward: Option<Onward>,
}

#[derive(Clone, Copy)]
pub(super) struct Onward {
    pub(super) hub: NodeIndex,
    pub(super) entrant: NodeIndex,
    pub(super) toward: NodeIndex,
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
    Hub {
        network: &'static str,
        path: &'static [Step],
        edge: Option<fn(&Node) -> bool>,
    },
    HubPeering {
        hub: &'static str,
        peering: &'static str,
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

fn is_edge_group(node: &Node) -> bool {
    matches!(node, Node::GcpNetworkConnectivityGroup(name) if name.ends_with("/groups/edge"))
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
    Join::Hub {
        network: "AwsEc2Vpc",
        path: &[from(Edge::AttachedTo, "AwsEc2VpcPeeringConnection")],
        edge: None,
    },
    Join::Hub {
        network: "AwsEc2Vpc",
        path: &[from(Edge::AttachedTo, "AwsEc2TransitGateway")],
        edge: None,
    },
    Join::HubPeering {
        hub: "AwsEc2TransitGateway",
        peering: "AwsEc2TransitGatewayPeeringAttachment",
    },
    Join::Hub {
        network: "AzureVirtualNetwork",
        path: &[
            from(Edge::AttachedTo, "AzureVirtualHub"),
            to(Edge::AttachedTo, "AzureVirtualWan"),
        ],
        edge: None,
    },
    Join::Hub {
        network: "GcpComputeNetwork",
        path: &[
            from(Edge::AttachedTo, "GcpNetworkConnectivitySpoke"),
            to(Edge::AttachedTo, "GcpNetworkConnectivityGroup"),
            from(Edge::Contains, "GcpNetworkConnectivityHub"),
        ],
        edge: Some(is_edge_group),
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

type Members = HashMap<Connector, HashSet<Approach>>;

#[derive(Default)]
pub(super) struct Joins<'g> {
    members: HashMap<&'g str, Members>,
    peers: HashMap<&'g str, HashSet<&'g str>>,
    peered: HashMap<NodeIndex, Vec<(NodeIndex, NodeIndex)>>,
    networks: HashMap<&'g str, NodeIndex>,
}

struct LinkEnd<'g> {
    networks: HashMap<&'g str, HashSet<Approach>>,
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
            networks: HashMap::new(),
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
                Join::Hub {
                    network,
                    path,
                    edge,
                } => {
                    let Some(hub_kind) = path.last().map(|step| step.kind) else {
                        continue;
                    };
                    for &hub in nodes_of(hub_kind) {
                        for trail in trails_back(graph, hub, path, network) {
                            let (Some(scope), Some(approach)) =
                                (graph[trail[0]].network_scope(), Approach::along(&trail))
                            else {
                                continue;
                            };
                            let at_edge = edge.is_some_and(|is_edge| {
                                trail.iter().any(|&node| is_edge(&graph[node]))
                            });
                            let connector = if at_edge {
                                Connector::Edge(hub)
                            } else {
                                Connector::Hub(hub)
                            };
                            joins
                                .members
                                .entry(scope)
                                .or_default()
                                .entry(connector)
                                .or_default()
                                .insert(approach);
                        }
                    }
                }
                Join::HubPeering { hub, peering } => {
                    for &attachment in nodes_of(peering) {
                        let hubs: Vec<NodeIndex> = neighbours(graph, attachment)
                            .filter(|next| graph[*next].kind() == *hub)
                            .collect();
                        for &near in &hubs {
                            for &far in hubs.iter().filter(|far| **far != near) {
                                joins
                                    .peered
                                    .entry(near)
                                    .or_default()
                                    .push((far, attachment));
                            }
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
                            let mut reached: Vec<(&str, Approach)> =
                                trails_back(graph, end, path, network)
                                    .into_iter()
                                    .filter_map(|trail| {
                                        Some((
                                            graph[trail[0]].network_scope()?,
                                            Approach::along(&trail)?,
                                        ))
                                    })
                                    .collect();
                            if reached.is_empty() {
                                continue;
                            }
                            let link_end = link_ends
                                .entry(end)
                                .or_insert_with(|| LinkEnd::of(graph, end, holds, names));
                            for (scope, approach) in reached.drain(..) {
                                link_end.networks.entry(scope).or_default().insert(approach);
                            }
                        }
                    }
                }
            }
        }
        joins.pair(&link_ends);
        if !joins.is_empty() {
            joins.networks = graph
                .node_indices()
                .filter_map(|index| Some((graph[index].network_scope()?, index)))
                .collect();
        }
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
                for (&network, approaches) in &naming_end.networks {
                    self.members
                        .entry(network)
                        .or_default()
                        .entry(Connector::Link { naming, named })
                        .or_default()
                        .extend(approaches.iter().copied());
                }
            }
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.members.is_empty() && self.peers.is_empty()
    }

    pub(super) fn crossings(&self, from: &str, to: &str) -> Vec<Crossing> {
        let mut crossings = Vec::new();
        if from == to {
            return crossings;
        }
        let (Some(&entrant), Some(&arrival)) = (self.networks.get(from), self.networks.get(to))
        else {
            return crossings;
        };
        if self.peers.get(from).is_some_and(|peers| peers.contains(to)) {
            crossings.push(Crossing {
                via: arrival,
                entrant,
                toward: arrival,
                onward: None,
            });
        }
        let (Some(mine), Some(theirs)) = (self.members.get(from), self.members.get(to)) else {
            return crossings;
        };
        for (connector, approaches) in mine {
            let met = connector
                .meets()
                .any(|partner| theirs.contains_key(&partner));
            for approach in approaches {
                if met {
                    let toward = match connector {
                        Connector::Link { .. } => approach.beyond.unwrap_or(approach.via),
                        Connector::Hub(_) | Connector::Edge(_) => arrival,
                    };
                    crossings.push(Crossing {
                        via: approach.via,
                        entrant,
                        toward,
                        onward: None,
                    });
                }
                let Connector::Hub(hub) = connector else {
                    continue;
                };
                for &(far, attachment) in self.peered.get(hub).into_iter().flatten() {
                    if theirs.contains_key(&Connector::Hub(far)) {
                        crossings.push(Crossing {
                            via: approach.via,
                            entrant,
                            toward: attachment,
                            onward: Some(Onward {
                                hub: far,
                                entrant: attachment,
                                toward: arrival,
                            }),
                        });
                    }
                }
            }
        }
        crossings
    }

    #[cfg(test)]
    pub(super) fn joined(&self, a: &str, b: &str) -> bool {
        !self.crossings(a, b).is_empty()
    }
}

pub(crate) struct HeldAddresses<'g> {
    joins: Joins<'g>,
    routing: Routing<'g>,
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
        let routing = if joins.is_empty() {
            Routing::default()
        } else {
            Routing::of(graph)
        };
        HeldAddresses {
            joins,
            routing,
            held,
        }
    }

    pub(crate) fn holder(&self, network: &str, address: &str) -> Option<NodeIndex> {
        let holders = self.held.get(address)?;
        if let Some((_, local)) = holders.iter().find(|(held_in, _)| *held_in == network) {
            return Some(*local);
        }
        let host = parse_address(address).and_then(|(addr, width)| width.is_none().then_some(addr));
        if host.is_some_and(|addr| self.routing.is_local(network, &addr)) {
            return None;
        }
        let mut reachable = holders.iter().filter(|(held_in, _)| {
            self.joins
                .crossings(network, held_in)
                .iter()
                .any(|crossing| {
                    host.is_none_or(|addr| self.routing.allows(network, &addr, crossing))
                })
        });
        match (reachable.next(), reachable.next()) {
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

fn trails_back(
    graph: &Graph<Node, Edge>,
    end: NodeIndex,
    path: &[Step],
    origin: &str,
) -> Vec<Vec<NodeIndex>> {
    fn walk(
        graph: &Graph<Node, Edge>,
        at: NodeIndex,
        path: &[Step],
        origin: &str,
        trail: &mut Vec<NodeIndex>,
        trails: &mut Vec<Vec<NodeIndex>>,
    ) {
        trail.push(at);
        match path.split_last() {
            None => trails.push(trail.iter().rev().copied().collect()),
            Some((step, before)) => {
                let kind = before.last().map_or(origin, |previous| previous.kind);
                for next in stepped(graph, at, &step.edge, step.toward.opposite(), kind) {
                    walk(graph, next, before, origin, trail, trails);
                }
            }
        }
        trail.pop();
    }
    let mut trails = Vec::new();
    walk(graph, end, path, origin, &mut Vec::new(), &mut trails);
    trails
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
                Join::Hub { network, path, .. } => {
                    (*network, path.iter().map(|step| step.kind).collect())
                }
                Join::HubPeering { hub, peering } => ("AwsEc2Vpc", vec![*hub, *peering]),
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

    fn hold(builder: &mut GraphBuilder, vpc_id: &str, address: &str) -> Node {
        let interface =
            builder.get_or_add_node(Node::AwsEc2Eni(format!("eni-{vpc_id}-{address}").into()));
        let pivot = Node::ip(address, vpc_id);
        builder.link_to(interface, pivot.clone(), Edge::ConnectsTo);
        pivot
    }

    fn address_space(builder: &mut GraphBuilder, vpc_id: &str, cidr: &str) {
        let network = builder.get_or_add_node(vpc(vpc_id));
        builder.link_to(network, Node::ip(cidr, vpc_id), Edge::ConnectsTo);
    }

    fn route_table(builder: &mut GraphBuilder, vpc_id: &str, routes: &[(&str, Option<Node>)]) {
        let table_id = format!("rtb-{vpc_id}");
        let table = builder.link_to(
            builder.index_of(&vpc(vpc_id)),
            Node::AwsEc2RouteTable(table_id.as_str().into()),
            Edge::Contains,
        );
        for (destination, target) in routes {
            let route = builder.link_to(
                table,
                Node::AwsEc2Route {
                    table: table_id.as_str().into(),
                    destination: (*destination).into(),
                },
                Edge::Contains,
            );
            if let Some(target) = target {
                builder.link_to(route, target.clone(), Edge::RoutesTo);
            }
        }
    }

    fn hub_table(
        builder: &mut GraphBuilder,
        hub: &Node,
        table_id: &str,
        associated: &[Node],
        propagated: &[Node],
        routes: &[(&str, Option<Node>)],
    ) {
        let hub = builder.get_or_add_node(hub.clone());
        let table = builder.link_to(
            hub,
            Node::AwsEc2TransitGatewayRouteTable(table_id.into()),
            Edge::Contains,
        );
        for entrant in associated {
            builder.link_from(table, entrant.clone(), Edge::AttachedTo);
        }
        for resource in propagated {
            builder.link_to(table, resource.clone(), Edge::RoutesTo);
        }
        for (destination, target) in routes {
            let route = builder.link_to(
                table,
                Node::AwsEc2TransitGatewayRoute {
                    table: table_id.into(),
                    destination: (*destination).into(),
                },
                Edge::Contains,
            );
            if let Some(target) = target {
                builder.link_to(route, target.clone(), Edge::RoutesTo);
            }
        }
    }

    fn resolved(builder: &GraphBuilder, network: &str, address: &str) -> Option<Node> {
        crate::atlas::derive::held_addresses(&builder.graph)
            .holder(network, address)
            .map(|index| builder.graph[index].clone())
    }

    #[test]
    fn an_unheld_address_inside_the_reading_network_stays_local() {
        let mut builder = GraphBuilder::new();
        attach(
            &mut builder,
            Node::AwsEc2TransitGateway("tgw-1".into()),
            &["vpc-a", "vpc-b"],
        );
        let far = hold(&mut builder, "vpc-b", "10.10.9.9");
        assert_eq!(
            resolved(&builder, "vpc-a", "10.10.9.9"),
            Some(far),
            "without its address space, the reading network looks across the join"
        );

        address_space(&mut builder, "vpc-a", "10.10.0.0/16");

        assert_eq!(
            resolved(&builder, "vpc-a", "10.10.9.9"),
            None,
            "an address inside vpc-a's own range is vpc-a's, held or not"
        );
    }

    #[test]
    fn a_route_table_chooses_which_join_an_address_crosses() {
        let mut builder = GraphBuilder::new();
        let peering = Node::AwsEc2VpcPeeringConnection("pcx-ab".into());
        let transit = Node::AwsEc2TransitGateway("tgw-1".into());
        attach(&mut builder, peering.clone(), &["vpc-a", "vpc-b"]);
        attach(&mut builder, transit.clone(), &["vpc-a", "vpc-c"]);
        let peered = hold(&mut builder, "vpc-b", "10.40.1.1");
        hold(&mut builder, "vpc-c", "10.40.1.1");
        assert_eq!(
            resolved(&builder, "vpc-a", "10.40.1.1"),
            None,
            "two holders"
        );

        route_table(
            &mut builder,
            "vpc-a",
            &[
                ("10.40.0.0/16", Some(peering)),
                ("10.0.0.0/8", Some(transit)),
            ],
        );

        assert_eq!(resolved(&builder, "vpc-a", "10.40.1.1"), Some(peered));
    }

    #[test]
    fn a_network_with_no_route_toward_a_join_does_not_cross_it() {
        let mut builder = GraphBuilder::new();
        attach(
            &mut builder,
            Node::AwsEc2VpcPeeringConnection("pcx-ab".into()),
            &["vpc-a", "vpc-b"],
        );
        hold(&mut builder, "vpc-b", "10.40.1.1");
        route_table(
            &mut builder,
            "vpc-a",
            &[(
                "0.0.0.0/0",
                Some(Node::AwsEc2InternetGateway("igw-a".into())),
            )],
        );

        assert_eq!(resolved(&builder, "vpc-a", "10.40.1.1"), None);
    }

    #[test]
    fn a_more_specific_blackhole_route_stops_the_crossing() {
        let mut builder = GraphBuilder::new();
        let peering = Node::AwsEc2VpcPeeringConnection("pcx-ab".into());
        attach(&mut builder, peering.clone(), &["vpc-a", "vpc-b"]);
        hold(&mut builder, "vpc-b", "10.40.1.1");
        let reachable = hold(&mut builder, "vpc-b", "10.40.2.1");
        route_table(
            &mut builder,
            "vpc-a",
            &[("10.40.0.0/16", Some(peering)), ("10.40.1.0/24", None)],
        );

        assert_eq!(resolved(&builder, "vpc-a", "10.40.1.1"), None);
        assert_eq!(resolved(&builder, "vpc-a", "10.40.2.1"), Some(reachable));
    }

    #[test]
    fn a_route_to_a_prefix_list_may_carry_any_address() {
        let mut builder = GraphBuilder::new();
        let transit = Node::AwsEc2TransitGateway("tgw-1".into());
        attach(&mut builder, transit.clone(), &["vpc-a", "vpc-b"]);
        let far = hold(&mut builder, "vpc-b", "10.40.1.1");
        route_table(
            &mut builder,
            "vpc-a",
            &[
                ("10.40.1.0/24", None),
                ("pl-0shared", Some(transit.clone())),
            ],
        );
        hub_table(
            &mut builder,
            &transit,
            "tgw-rtb-1",
            &[vpc("vpc-a")],
            &[],
            &[("pl-0shared", Some(vpc("vpc-b")))],
        );

        assert_eq!(
            resolved(&builder, "vpc-a", "10.40.1.1"),
            Some(far),
            "a prefix list's entries are unread, so its route may be the longest match on both tables"
        );
    }

    #[test]
    fn a_transit_gateway_route_table_segments_its_attachments() {
        let mut builder = GraphBuilder::new();
        let transit = Node::AwsEc2TransitGateway("tgw-1".into());
        attach(&mut builder, transit.clone(), &["vpc-a", "vpc-b", "vpc-c"]);
        for (vpc_id, cidr) in [
            ("vpc-a", "10.1.0.0/16"),
            ("vpc-b", "10.2.0.0/16"),
            ("vpc-c", "10.3.0.0/16"),
        ] {
            address_space(&mut builder, vpc_id, cidr);
        }
        let shared = hold(&mut builder, "vpc-b", "10.2.0.9");
        hold(&mut builder, "vpc-c", "10.3.0.9");
        hold(&mut builder, "vpc-a", "10.1.0.9");
        hub_table(
            &mut builder,
            &transit,
            "tgw-rtb-shared",
            &[vpc("vpc-a"), vpc("vpc-b")],
            &[vpc("vpc-a"), vpc("vpc-b")],
            &[],
        );
        hub_table(
            &mut builder,
            &transit,
            "tgw-rtb-isolated",
            &[vpc("vpc-c")],
            &[vpc("vpc-c")],
            &[],
        );

        assert_eq!(resolved(&builder, "vpc-a", "10.2.0.9"), Some(shared));
        assert_eq!(
            resolved(&builder, "vpc-a", "10.3.0.9"),
            None,
            "c propagates elsewhere"
        );
        assert_eq!(
            resolved(&builder, "vpc-c", "10.1.0.9"),
            None,
            "c's table reaches only c"
        );
    }

    #[test]
    fn a_vpn_on_a_transit_gateway_needs_its_table_to_carry_the_far_side() {
        let build = |propagates_vpn: bool| {
            let mut builder = GraphBuilder::new();
            let transit = Node::AwsEc2TransitGateway("tgw-1".into());
            attach(&mut builder, transit.clone(), &["vpc-a"]);
            vpn_connection(
                &mut builder,
                transit.clone(),
                "tgw",
                "192.0.2.1",
                "203.0.113.20",
            );
            azure_gateway(&mut builder, "vnet-far", "203.0.113.20");
            azure_connection(&mut builder, "vnet-far", "tgw", "192.0.2.1");
            hold(&mut builder, "vnet-far", "10.20.0.5");
            let propagated: Vec<Node> = if propagates_vpn {
                vec![Node::AwsEc2VpnConnection("vpn-tgw".into())]
            } else {
                Vec::new()
            };
            hub_table(
                &mut builder,
                &transit,
                "tgw-rtb-1",
                &[vpc("vpc-a")],
                &propagated,
                &[],
            );
            builder
        };

        assert!(resolved(&build(true), "vpc-a", "10.20.0.5").is_some());
        assert_eq!(resolved(&build(false), "vpc-a", "10.20.0.5"), None);
    }

    #[test]
    fn transit_gateway_peering_needs_a_route_on_both_gateways_and_goes_one_hop() {
        let build = |east_routes: bool, west_carries: bool| {
            let mut builder = GraphBuilder::new();
            let east = Node::AwsEc2TransitGateway("tgw-east".into());
            let west = Node::AwsEc2TransitGateway("tgw-west".into());
            let south = Node::AwsEc2TransitGateway("tgw-south".into());
            attach(&mut builder, east.clone(), &["vpc-a"]);
            attach(&mut builder, west.clone(), &["vpc-b"]);
            attach(&mut builder, south.clone(), &["vpc-c"]);
            address_space(&mut builder, "vpc-b", "10.2.0.0/16");
            address_space(&mut builder, "vpc-c", "10.3.0.0/16");
            hold(&mut builder, "vpc-b", "10.2.0.9");
            hold(&mut builder, "vpc-c", "10.3.0.9");
            let peer = |builder: &mut GraphBuilder, id: &str, sides: [&Node; 2]| {
                let attachment = Node::AwsEc2TransitGatewayPeeringAttachment(id.into());
                let index = builder.get_or_add_node(attachment.clone());
                for side in sides {
                    builder.link_to(index, side.clone(), Edge::AttachedTo);
                }
                attachment
            };
            let east_west = peer(&mut builder, "tgw-attach-ew", [&east, &west]);
            let west_south = peer(&mut builder, "tgw-attach-ws", [&west, &south]);
            let routes: Vec<(&str, Option<Node>)> = if east_routes {
                vec![
                    ("10.2.0.0/16", Some(east_west.clone())),
                    ("10.3.0.0/16", Some(east_west.clone())),
                ]
            } else {
                Vec::new()
            };
            hub_table(
                &mut builder,
                &east,
                "tgw-rtb-east",
                &[vpc("vpc-a")],
                &[],
                &routes,
            );
            let propagated = if west_carries {
                vec![vpc("vpc-b")]
            } else {
                Vec::new()
            };
            hub_table(
                &mut builder,
                &west,
                "tgw-rtb-west",
                &[east_west, vpc("vpc-b")],
                &propagated,
                &[("10.3.0.0/16", Some(west_south))],
            );
            builder
        };

        let routed = build(true, true);
        assert!(resolved(&routed, "vpc-a", "10.2.0.9").is_some());
        assert_eq!(
            resolved(&routed, "vpc-a", "10.3.0.9"),
            None,
            "transit gateway peering is not transitive"
        );
        assert_eq!(resolved(&build(false, true), "vpc-a", "10.2.0.9"), None);
        assert_eq!(
            resolved(&build(true, false), "vpc-a", "10.2.0.9"),
            None,
            "the far gateway's table must route the address on"
        );
    }

    fn ncc_spoke(builder: &mut GraphBuilder, network: &str, group: &str) {
        let spoke = builder.get_or_add_node(Node::GcpNetworkConnectivitySpoke(
            format!("spokes/{network}").into(),
        ));
        builder.link_to(
            spoke,
            Node::GcpComputeNetwork(network.into()),
            Edge::AttachedTo,
        );
        let group = builder.link_to(
            spoke,
            Node::GcpNetworkConnectivityGroup(format!("hubs/hub-1/groups/{group}").into()),
            Edge::AttachedTo,
        );
        builder.link_from(
            group,
            Node::GcpNetworkConnectivityHub("hubs/hub-1".into()),
            Edge::Contains,
        );
    }

    #[test]
    fn a_mesh_hub_joins_every_spoke_and_a_star_hub_keeps_edges_apart() {
        let mut mesh = GraphBuilder::new();
        for network in ["net-a", "net-b", "net-c"] {
            ncc_spoke(&mut mesh, network, "default");
        }
        let joins = Joins::of(&mesh.graph);
        assert!(joins.joined("net-a", "net-b") && joins.joined("net-b", "net-c"));

        let mut star = GraphBuilder::new();
        ncc_spoke(&mut star, "net-center", "center");
        ncc_spoke(&mut star, "net-edge-1", "edge");
        ncc_spoke(&mut star, "net-edge-2", "edge");
        let joins = Joins::of(&star.graph);
        assert!(joins.joined("net-edge-1", "net-center"));
        assert!(joins.joined("net-center", "net-edge-2"));
        assert!(
            !joins.joined("net-edge-1", "net-edge-2"),
            "edge spokes of a star reach only the centre"
        );
    }

    #[test]
    fn virtual_networks_on_one_virtual_wan_are_joined_across_its_hubs() {
        let mut builder = GraphBuilder::new();
        for (hub, wan, vnets) in [
            ("vhub-east", "vwan-1", ["vnet-a", "vnet-b"]),
            ("vhub-west", "vwan-1", ["vnet-c", "vnet-d"]),
            ("vhub-other", "vwan-2", ["vnet-e", "vnet-f"]),
        ] {
            let hub = builder.get_or_add_node(Node::AzureVirtualHub(hub.into()));
            builder.link_to(hub, Node::AzureVirtualWan(wan.into()), Edge::AttachedTo);
            for vnet in vnets {
                builder.link_to(
                    hub,
                    Node::AzureVirtualNetwork(vnet.into()),
                    Edge::AttachedTo,
                );
            }
        }

        let joins = Joins::of(&builder.graph);

        assert!(joins.joined("vnet-a", "vnet-b"));
        assert!(
            joins.joined("vnet-a", "vnet-d"),
            "hubs of one WAN route to each other"
        );
        assert!(!joins.joined("vnet-a", "vnet-e"));
    }

    #[derive(Clone, Copy, Debug)]
    enum Toward {
        Peering,
        Transit,
        Nowhere,
    }

    proptest::proptest! {
        #[test]
        fn the_longest_matching_route_decides_the_crossing(
            routes in proptest::collection::vec(
                (8u8..=32, proptest::bool::ANY, proptest::prop_oneof![
                    proptest::strategy::Just(Toward::Peering),
                    proptest::strategy::Just(Toward::Transit),
                    proptest::strategy::Just(Toward::Nowhere),
                ]),
                0..6,
            ),
        ) {
            use crate::atlas::util::{format_address, mask_to_prefix};
            use std::net::{IpAddr, Ipv4Addr};

            let address = IpAddr::V4(Ipv4Addr::new(10, 40, 1, 1));
            let mut builder = GraphBuilder::new();
            let peering = Node::AwsEc2VpcPeeringConnection("pcx-ab".into());
            let transit = Node::AwsEc2TransitGateway("tgw-1".into());
            attach(&mut builder, peering.clone(), &["vpc-a", "vpc-b"]);
            attach(&mut builder, transit.clone(), &["vpc-a", "vpc-c"]);
            let peered = hold(&mut builder, "vpc-b", "10.40.1.1");
            let transited = hold(&mut builder, "vpc-c", "10.40.1.1");

            let mut seen = HashSet::new();
            let mut table = Vec::new();
            let mut best: Option<(u8, Toward)> = None;
            for (width, covers, toward) in routes {
                let base = if covers {
                    address
                } else {
                    IpAddr::V4(Ipv4Addr::from(u32::from(Ipv4Addr::new(10, 40, 1, 1)) ^ (1 << (32 - width))))
                };
                let network = mask_to_prefix(&base, width).expect("a width inside v4");
                let destination = format_address(&network, Some(width));
                if !seen.insert(destination.clone()) {
                    continue;
                }
                if covers && best.is_none_or(|(longest, _)| width > longest) {
                    best = Some((width, toward));
                }
                let target = match toward {
                    Toward::Peering => Some(peering.clone()),
                    Toward::Transit => Some(transit.clone()),
                    Toward::Nowhere => None,
                };
                table.push((destination, target));
            }
            let borrowed: Vec<(&str, Option<Node>)> = table
                .iter()
                .map(|(destination, target)| (destination.as_str(), target.clone()))
                .collect();
            route_table(&mut builder, "vpc-a", &borrowed);

            let expected = match best {
                Some((_, Toward::Peering)) => Some(peered),
                Some((_, Toward::Transit)) => Some(transited),
                Some((_, Toward::Nowhere)) | None => None,
            };
            proptest::prop_assert_eq!(resolved(&builder, "vpc-a", "10.40.1.1"), expected);
        }
    }
}
