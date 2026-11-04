use super::networks::Crossing;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::util::{mask_to_prefix, parse_address};
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::net::IpAddr;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Range {
    network: IpAddr,
    width: u8,
}

impl Range {
    fn parse(value: &str) -> Option<Range> {
        let (addr, Some(width)) = parse_address(value)? else {
            return None;
        };
        Some(Range {
            network: mask_to_prefix(&addr, width)?,
            width,
        })
    }

    fn covers(&self, addr: &IpAddr) -> bool {
        mask_to_prefix(addr, self.width) == Some(self.network)
    }
}

fn range_of(node: &Node) -> Option<Range> {
    match node {
        Node::GenericPrivateIpAddress { address, .. } | Node::GenericIpAddress(address) => {
            Range::parse(address)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Destination {
    Range(Range),
    Learned,
    PrefixList,
}

impl Destination {
    fn parse(value: &str) -> Option<Destination> {
        if value.starts_with("pl-") {
            return Some(Destination::PrefixList);
        }
        Range::parse(value).map(Destination::Range)
    }
}

struct Route {
    destination: Destination,
    configured: bool,
    target: Option<NodeIndex>,
}

impl Route {
    fn rank(&self, addr: &IpAddr) -> Option<(i16, bool)> {
        match self.destination {
            Destination::Range(range) if range.covers(addr) => {
                Some((i16::from(range.width), self.configured))
            }
            Destination::Learned => Some((-1, self.configured)),
            Destination::Range(_) | Destination::PrefixList => None,
        }
    }
}

fn chosen(routes: &[Route], addr: &IpAddr) -> impl Iterator<Item = Option<NodeIndex>> {
    let best = routes.iter().filter_map(|route| route.rank(addr)).max();
    routes
        .iter()
        .filter(move |route| {
            route.destination == Destination::PrefixList
                || (best.is_some() && route.rank(addr) == best)
        })
        .map(|route| route.target)
}

#[derive(Default)]
struct HubTables {
    routes: HashMap<NodeIndex, Vec<Route>>,
    associations: HashMap<NodeIndex, Vec<NodeIndex>>,
}

#[derive(Default)]
pub(super) struct Routing<'g> {
    spaces: HashMap<&'g str, Vec<Range>>,
    network_tables: HashMap<&'g str, Vec<Vec<Route>>>,
    hubs: HashMap<NodeIndex, HubTables>,
}

impl<'g> Routing<'g> {
    pub(super) fn of(graph: &'g Graph<Node, Edge>) -> Self {
        let mut routing = Routing::default();
        for edge in graph
            .edge_references()
            .filter(|edge| edge.weight() == &Edge::ConnectsTo)
        {
            if let (Some(scope), Some(range)) = (
                graph[edge.source()].network_scope(),
                range_of(&graph[edge.target()]),
            ) {
                routing.spaces.entry(scope).or_default().push(range);
            }
        }

        for table in graph.node_indices() {
            match &graph[table] {
                Node::AwsEc2RouteTable(_) => {
                    if let Some(scope) =
                        containers(graph, table).find_map(|parent| match &graph[parent] {
                            Node::AwsEc2Vpc(id) => Some(id.as_ref()),
                            _ => None,
                        })
                    {
                        let routes = configured_routes(graph, table).collect();
                        routing
                            .network_tables
                            .entry(scope)
                            .or_default()
                            .push(routes);
                    }
                }
                Node::AwsEc2TransitGatewayRouteTable(_) => {
                    let Some(hub) = containers(graph, table)
                        .find(|parent| matches!(graph[*parent], Node::AwsEc2TransitGateway(_)))
                    else {
                        continue;
                    };
                    let mut routes: Vec<Route> = configured_routes(graph, table).collect();
                    for propagated in linked(graph, table, Direction::Outgoing, &Edge::RoutesTo) {
                        let space = graph[propagated]
                            .network_scope()
                            .and_then(|scope| routing.spaces.get(scope));
                        match space {
                            Some(ranges) => routes.extend(ranges.iter().map(|range| Route {
                                destination: Destination::Range(*range),
                                configured: false,
                                target: Some(propagated),
                            })),
                            None => routes.push(Route {
                                destination: Destination::Learned,
                                configured: false,
                                target: Some(propagated),
                            }),
                        }
                    }
                    let tables = routing.hubs.entry(hub).or_default();
                    tables.routes.insert(table, routes);
                    for entrant in linked(graph, table, Direction::Incoming, &Edge::AttachedTo) {
                        tables.associations.entry(entrant).or_default().push(table);
                    }
                }
                _ => {}
            }
        }
        routing
    }

    pub(super) fn is_local(&self, scope: &str, addr: &IpAddr) -> bool {
        self.spaces
            .get(scope)
            .is_some_and(|ranges| ranges.iter().any(|range| range.covers(addr)))
    }

    pub(super) fn allows(&self, scope: &str, addr: &IpAddr, crossing: &Crossing) -> bool {
        self.leaves(scope, addr, crossing.via)
            && self.hub_carries(crossing.via, crossing.entrant, addr, crossing.toward)
            && crossing.onward.is_none_or(|onward| {
                self.hub_carries(onward.hub, onward.entrant, addr, onward.toward)
            })
    }

    fn leaves(&self, scope: &str, addr: &IpAddr, via: NodeIndex) -> bool {
        self.network_tables.get(scope).is_none_or(|tables| {
            tables
                .iter()
                .any(|routes| chosen(routes, addr).any(|target| target == Some(via)))
        })
    }

    fn hub_carries(
        &self,
        hub: NodeIndex,
        entrant: NodeIndex,
        addr: &IpAddr,
        toward: NodeIndex,
    ) -> bool {
        let Some(tables) = self.hubs.get(&hub) else {
            return true;
        };
        tables
            .associations
            .get(&entrant)
            .into_iter()
            .flatten()
            .filter_map(|table| tables.routes.get(table))
            .any(|routes| chosen(routes, addr).any(|target| target == Some(toward)))
    }
}

fn containers(graph: &Graph<Node, Edge>, at: NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
    linked(graph, at, Direction::Incoming, &Edge::Contains)
}

fn linked<'g>(
    graph: &'g Graph<Node, Edge>,
    at: NodeIndex,
    toward: Direction,
    edge: &'g Edge,
) -> impl Iterator<Item = NodeIndex> + 'g {
    graph
        .edges_directed(at, toward)
        .filter(move |link| link.weight() == edge)
        .map(move |link| match toward {
            Direction::Outgoing => link.target(),
            Direction::Incoming => link.source(),
        })
}

fn configured_routes<'g>(
    graph: &'g Graph<Node, Edge>,
    table: NodeIndex,
) -> impl Iterator<Item = Route> + 'g {
    linked(graph, table, Direction::Outgoing, &Edge::Contains).filter_map(move |route| {
        let destination = match &graph[route] {
            Node::AwsEc2Route { destination, .. }
            | Node::AwsEc2TransitGatewayRoute { destination, .. } => destination,
            _ => return None,
        };
        Some(Route {
            destination: Destination::parse(destination)?,
            configured: true,
            target: linked(graph, route, Direction::Outgoing, &Edge::RoutesTo).next(),
        })
    })
}
