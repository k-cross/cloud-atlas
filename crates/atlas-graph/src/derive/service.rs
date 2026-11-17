use super::networks::{HeldAddresses, Joins};
use crate::definition::{Edge, Node};
use crate::export::{RenderObservation, edge_key, node_key};
use crate::flow::{FlowIndex, Orientation};
use crate::graph_builder::GraphBuilder;
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

pub const CONFIRMED: &str = "confirmed";
pub const INFERRED: &str = "inferred";

// The control-plane chains that mean "wired up", as sequences of `Node::kind()`.
// A provider's own topology cannot be inferred generically, so this table is the
// one provider-specific thing in the pass; the matcher below knows nothing about
// any cloud. `every_pattern_names_a_kind_that_exists` holds it to `ALL_KINDS`,
// so a renamed variant fails a test rather than silently matching nothing.
//
// GCP and Azure rows wait on node kinds that do not exist yet (a backend
// service, and a backend pool to sit under `AzureLoadBalancer`), so until those
// land their `Serves` edges come from observed traffic alone, which phase 1d
// made possible by giving both clouds' resources their addresses. Asymmetric on
// purpose.
const CHAINS: &[&[&str]] = &[
    &["AwsElbLoadBalancer", "AwsElbTargetGroup", "AwsEc2Instance"],
    &[
        "AwsElbLoadBalancer",
        "AwsElbTargetGroup",
        "GenericPrivateIpAddress",
    ],
    &[
        "AwsElbLoadBalancer",
        "AwsElbTargetGroup",
        "GenericIpAddress",
    ],
];

pub(super) fn link(builder: &mut GraphBuilder, flows: &FlowIndex) {
    for serving in served(&builder.graph, flows) {
        builder.add_edge(serving.source, serving.target, Edge::Serves);
    }
}

pub(super) fn observations(graph: &Graph<Node, Edge>, flows: &FlowIndex) -> Vec<RenderObservation> {
    served(graph, flows)
        .into_iter()
        // Between reconciliations a flow can arrive whose `Serves` edge the
        // graph does not have yet, and an observation keyed to an edge no
        // client holds is a claim about nothing.
        .filter(|serving| {
            graph
                .edges_connecting(serving.source, serving.target)
                .any(|e| e.weight() == &Edge::Serves)
        })
        .map(|serving| RenderObservation {
            key: edge_key(
                &node_key(&graph[serving.source]),
                &node_key(&graph[serving.target]),
                &Edge::Serves,
            ),
            // An inferred edge has never been observed, and 0 says so rather
            // than dressing "wired up" as "seen just now".
            last_seen: serving.last_seen,
            packets: None,
            bytes: None,
            status: serving.status(),
        })
        .collect()
}

pub(super) struct Serving {
    source: NodeIndex,
    target: NodeIndex,
    confirmations: usize,
    last_seen: i64,
}

impl Serving {
    // The two provenances are one edge on purpose: a flow that lapses drops a
    // still-registered target back to `inferred` instead of deleting it, and
    // "wired up and receiving nothing" is the more useful of the two readings.
    fn status(&self) -> &'static str {
        if self.confirmations == 0 {
            INFERRED
        } else {
            CONFIRMED
        }
    }
}

// Every typed pair in a serving relationship, with how many flows confirm it
// and the newest of them — none, for a pair only the control plane vouches for.
// Both halves of the pass read this one answer, so an edge and its observation
// can never disagree about which pairs exist or how they got there.
fn served(graph: &Graph<Node, Edge>, flows: &FlowIndex) -> Vec<Serving> {
    let owners = address_owners(graph);
    let mut pairs: HashMap<(NodeIndex, NodeIndex), (usize, i64)> = HashMap::new();

    for flow in graph
        .edge_references()
        .filter(|e| e.weight() == &Edge::TrafficFlow)
    {
        // Confirmation is read from the index, not from the edge: between scans
        // an evicted flow's edge outlives its record, and a status backed by an
        // edge with nothing behind it would be "confirmed, never seen".
        let Some(stats) = flows.stats(&graph[flow.source()], &graph[flow.target()]) else {
            continue;
        };
        // A reply travels server -> client, so taken at packet direction it
        // would draw every relationship backwards beside the forwards one.
        let (client, server) = match stats.orientation() {
            Some(Orientation::Reverse) => (flow.target(), flow.source()),
            Some(Orientation::Forward) | None => (flow.source(), flow.target()),
        };
        let (Some(sources), Some(targets)) = (owners.get(&client), owners.get(&server)) else {
            continue;
        };
        for source in sources {
            for target in targets {
                // A resource talking to itself across two of its own interfaces
                // is not a service relationship, and a self-loop renders as a
                // smudge on the node rather than as a path.
                if source != target {
                    let (confirmations, last_seen) = pairs.entry((*source, *target)).or_default();
                    *confirmations += 1;
                    *last_seen = (*last_seen).max(stats.last_seen);
                }
            }
        }
    }

    for (source, target) in wired(graph, &owners) {
        pairs.entry((source, target)).or_default();
    }

    let mut served: Vec<Serving> = pairs
        .into_iter()
        .map(|((source, target), (confirmations, last_seen))| Serving {
            source,
            target,
            confirmations,
            last_seen,
        })
        .collect();
    served.sort_by_key(|serving| (serving.source.index(), serving.target.index()));
    served
}

// The ends of every control-plane chain the table describes. Only the node
// kinds discriminate; the walk follows any *projected* edge, since a chain is
// the scan's own evidence — matching through a derived edge would let the pass
// feed on its own output.
fn wired(
    graph: &Graph<Node, Edge>,
    owners: &HashMap<NodeIndex, Vec<NodeIndex>>,
) -> Vec<(NodeIndex, NodeIndex)> {
    let mut ends = Vec::new();
    for pattern in CHAINS {
        let Some((first, rest)) = pattern.split_first() else {
            continue;
        };
        for start in graph
            .node_indices()
            .filter(|index| graph[*index].kind() == *first)
        {
            walk(graph, owners, start, rest, start, &mut ends);
        }
    }
    ends
}

fn walk(
    graph: &Graph<Node, Edge>,
    owners: &HashMap<NodeIndex, Vec<NodeIndex>>,
    at: NodeIndex,
    remaining: &[&str],
    start: NodeIndex,
    ends: &mut Vec<(NodeIndex, NodeIndex)>,
) {
    let Some((kind, rest)) = remaining.split_first() else {
        let targets = if graph[at].owner().is_none() {
            owners.get(&at).map_or(&[][..], Vec::as_slice)
        } else {
            std::slice::from_ref(&at)
        };
        for &target in targets {
            if target != start {
                ends.push((start, target));
            }
        }
        return;
    };
    for edge in graph
        .edges_directed(at, Direction::Outgoing)
        .filter(|edge| edge.weight().is_projected())
    {
        if graph[edge.target()].kind() == *kind {
            walk(graph, owners, edge.target(), rest, start, ends);
        }
    }
}

// Which typed resource a flow endpoint belongs to. A pivot is claimed by the
// resource that `ConnectsTo` it, but the claimer is usually an interface or an
// address object, and neither is what anyone wants to read off the picture: the
// address on an ALB's ENI is the ALB's, and an Elastic IP associated with an
// instance's interface is the instance's. So a claimer resolves up through
// `HasIp` to whatever ultimately holds it.
//
// Transitively, because the holders stack — an associated EIP is held by an
// interface that is held by an instance — and stopping one hop short leaves the
// address with two owners, the object and the resource. A visited set keeps a
// cycle, which no producer should emit, from being a hang rather than a no-op.
// A claimer nothing holds — a Lambda, VPC endpoint or RDS interface (§7 of the
// design doc) — is its own owner, which is why those stay nodes with addresses.
//
// Fan-out is deliberately uncapped: an address with several owners is a real
// statement about the estate, not a budget to protect.
pub fn address_owners(graph: &Graph<Node, Edge>) -> HashMap<NodeIndex, Vec<NodeIndex>> {
    let mut owners = local_owners(graph);
    let reached = across_joins(graph, &owners);
    owners.extend(reached);
    owners
}

pub fn held_addresses(graph: &Graph<Node, Edge>) -> HeldAddresses<'_> {
    HeldAddresses::new(graph, Joins::of(graph), local_owners(graph).into_keys())
}

fn local_owners(graph: &Graph<Node, Edge>) -> HashMap<NodeIndex, Vec<NodeIndex>> {
    let mut held_by: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
    for edge in graph
        .edge_references()
        .filter(|e| e.weight() == &Edge::HasIp)
    {
        held_by
            .entry(edge.target())
            .or_default()
            .push(edge.source());
    }

    let mut owners: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
    for edge in graph
        .edge_references()
        .filter(|e| e.weight() == &Edge::ConnectsTo)
    {
        let (claimer, pivot) = (edge.source(), edge.target());
        if graph[pivot].owner().is_some() || graph[claimer].owner().is_none() {
            continue;
        }
        holders(claimer, &held_by, owners.entry(pivot).or_default());
    }

    for claimers in owners.values_mut() {
        claimers.sort_by_key(|index| index.index());
        claimers.dedup();
    }
    owners.retain(|_, claimers| !claimers.is_empty());
    owners
}

fn across_joins(
    graph: &Graph<Node, Edge>,
    owners: &HashMap<NodeIndex, Vec<NodeIndex>>,
) -> Vec<(NodeIndex, Vec<NodeIndex>)> {
    let joins = Joins::of(graph);
    if joins.is_empty() {
        return Vec::new();
    }
    let held = HeldAddresses::new(graph, joins, owners.keys().copied());

    graph
        .node_indices()
        .filter(|pivot| !owners.contains_key(pivot))
        .filter_map(|pivot| match &graph[pivot] {
            Node::GenericPrivateIpAddress { network, address } => held
                .holder(network, address)
                .map(|holder| (pivot, owners[&holder].clone())),
            _ => None,
        })
        .collect()
}

fn holders(
    start: NodeIndex,
    held_by: &HashMap<NodeIndex, Vec<NodeIndex>>,
    out: &mut Vec<NodeIndex>,
) {
    let mut visited = HashSet::new();
    let mut pending = vec![start];
    while let Some(at) = pending.pop() {
        if !visited.insert(at) {
            continue;
        }
        match held_by.get(&at) {
            Some(above) => pending.extend(above),
            None => out.push(at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{serves, serves_any};

    fn connect(builder: &mut GraphBuilder, connector: Node, networks: &[&str]) {
        let index = builder.get_or_add_node(connector);
        for network in networks {
            builder.link_to(index, Node::AwsEc2Vpc((*network).into()), Edge::AttachedTo);
        }
    }

    fn hold(builder: &mut GraphBuilder, instance: &str, address: &str, network: &str) {
        let index = builder.get_or_add_node(Node::AwsEc2Instance(instance.into()));
        let eni = builder.link_to(
            index,
            Node::AwsEc2Eni(format!("eni-{instance}").into()),
            Edge::HasIp,
        );
        builder.link_to(eni, Node::ip(address, network), Edge::ConnectsTo);
    }

    fn balancer_targeting(builder: &mut GraphBuilder, address: &str, network: &str) -> Node {
        let balancer = Node::AwsElbLoadBalancer(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/peer/1".into(),
        );
        let balancer_idx = builder.get_or_add_node(balancer.clone());
        let group = builder.link_to(
            balancer_idx,
            Node::AwsElbTargetGroup(
                "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/peer/1".into(),
            ),
            Edge::ConnectsTo,
        );
        builder.link_to(group, Node::ip(address, network), Edge::RoutesTo);
        balancer
    }

    fn derived(mut builder: GraphBuilder) -> GraphBuilder {
        super::super::all(&mut builder, &FlowIndex::default());
        builder
    }

    #[test]
    fn an_ip_target_in_a_peered_vpc_is_inferred() {
        let mut builder = GraphBuilder::new();
        connect(
            &mut builder,
            Node::AwsEc2VpcPeeringConnection("pcx-ab".into()),
            &["vpc-a", "vpc-b"],
        );
        hold(&mut builder, "i-b", "10.0.1.5", "vpc-b");
        let balancer = balancer_targeting(&mut builder, "10.0.1.5", "vpc-a");
        let builder = derived(builder);

        assert!(serves(
            &builder,
            &balancer,
            &Node::AwsEc2Instance("i-b".into())
        ));
        assert!(
            observations(&builder.graph, &FlowIndex::default())
                .iter()
                .all(|o| o.status == INFERRED)
        );
    }

    #[test]
    fn peering_does_not_chain() {
        let mut builder = GraphBuilder::new();
        connect(
            &mut builder,
            Node::AwsEc2VpcPeeringConnection("pcx-ab".into()),
            &["vpc-a", "vpc-b"],
        );
        connect(
            &mut builder,
            Node::AwsEc2VpcPeeringConnection("pcx-bc".into()),
            &["vpc-b", "vpc-c"],
        );
        hold(&mut builder, "i-c", "10.0.1.5", "vpc-c");
        let balancer = balancer_targeting(&mut builder, "10.0.1.5", "vpc-a");
        let builder = derived(builder);

        assert!(!serves_any(&builder, |source, _| source == &balancer));
    }

    #[test]
    fn an_address_two_joined_networks_hold_resolves_to_neither() {
        let mut builder = GraphBuilder::new();
        connect(
            &mut builder,
            Node::AwsEc2TransitGateway("tgw-1".into()),
            &["vpc-a", "vpc-b", "vpc-c"],
        );
        hold(&mut builder, "i-b", "10.0.1.5", "vpc-b");
        hold(&mut builder, "i-c", "10.0.1.5", "vpc-c");
        let balancer = balancer_targeting(&mut builder, "10.0.1.5", "vpc-a");
        let builder = derived(builder);

        assert!(!serves_any(&builder, |source, _| source == &balancer));
    }

    #[test]
    fn a_holder_in_the_reading_network_wins_over_any_joined_one() {
        let mut builder = GraphBuilder::new();
        connect(
            &mut builder,
            Node::AwsEc2TransitGateway("tgw-1".into()),
            &["vpc-a", "vpc-b"],
        );
        hold(&mut builder, "i-a", "10.0.1.5", "vpc-a");
        hold(&mut builder, "i-b", "10.0.1.5", "vpc-b");
        let balancer = balancer_targeting(&mut builder, "10.0.1.5", "vpc-a");
        let builder = derived(builder);

        assert!(serves(
            &builder,
            &balancer,
            &Node::AwsEc2Instance("i-a".into())
        ));
        assert!(!serves(
            &builder,
            &balancer,
            &Node::AwsEc2Instance("i-b".into())
        ));
    }

    #[test]
    fn every_pattern_names_a_kind_that_exists() {
        for pattern in CHAINS {
            assert!(pattern.len() >= 2, "a chain needs two ends");
            for kind in *pattern {
                assert!(
                    Node::ALL_KINDS.contains(kind),
                    "no Node variant is called {kind}"
                );
            }
        }
    }
}
