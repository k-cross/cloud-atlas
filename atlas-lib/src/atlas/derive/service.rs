use crate::atlas::definition::{Edge, Node};
use crate::atlas::export::{RenderObservation, edge_key, node_key};
use crate::atlas::flow::{FlowIndex, Orientation};
use crate::atlas::graph_builder::GraphBuilder;
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

pub(super) const CONFIRMED: &str = "confirmed";
pub(super) const INFERRED: &str = "inferred";

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
fn address_owners(graph: &Graph<Node, Edge>) -> HashMap<NodeIndex, Vec<NodeIndex>> {
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
    owners
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
    use crate::atlas::flow::{FlowIndex, FlowObservation};
    use crate::fixtures;

    fn serves(builder: &GraphBuilder, source: &Node, target: &Node) -> bool {
        builder.has_edge(source, target, &Edge::Serves)
    }

    fn alb() -> Node {
        Node::AwsElbLoadBalancer(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/globex/1".into(),
        )
    }

    // The projector folds a networking resource's id to one casing (Resource
    // Graph is not consistent about which casing it echoes an id back with),
    // so an expectation built from the fixture's own literal has to match.
    fn azure_networking_id(rest: &str) -> std::sync::Arc<str> {
        fixtures::azure_id(rest).to_ascii_lowercase().into()
    }

    fn serves_any(builder: &GraphBuilder, pick: impl Fn(&Node, &Node) -> bool) -> bool {
        builder
            .graph
            .edge_references()
            .filter(|e| e.weight() == &Edge::Serves)
            .any(|e| pick(&builder.graph[e.source()], &builder.graph[e.target()]))
    }

    // A flow log records the reply as its own record with the addresses swapped.
    // The fixture carries both halves of the ALB and RDS conversations.
    #[test]
    fn a_reply_does_not_reverse_the_relationship() {
        let builder = fixtures::build_graph();
        let web01 = Node::AwsEc2Instance("i-globex-web-01".into());
        let rds = Node::AwsEc2Eni("eni-globex-rds-1a".into());

        assert!(serves(&builder, &alb(), &web01));
        assert!(
            !serves(&builder, &web01, &alb()),
            "the target's reply is not the target serving the balancer"
        );
        assert!(serves(&builder, &web01, &rds));
        assert!(
            !serves(&builder, &rds, &web01),
            "the database's reply is not the database being served"
        );
    }

    // web-02's public address is an Elastic IP held by its interface. The
    // address object must resolve to the instance, not stand beside it.
    #[test]
    fn an_associated_address_resolves_to_what_holds_it() {
        let builder = fixtures::build_graph();

        assert!(serves(
            &builder,
            &Node::AwsEc2Instance("i-globex-web-01".into()),
            &Node::AwsEc2Instance("i-globex-web-02".into()),
        ));
        assert!(
            !serves_any(&builder, |source, target| {
                matches!(source, Node::AwsEc2Eip(_)) || matches!(target, Node::AwsEc2Eip(_))
            }),
            "an Elastic IP is an address, not a service"
        );
    }

    // The NAT gateway's address is held twice over — by the gateway directly,
    // and by the interface the gateway holds — and both roads end at one owner.
    #[test]
    fn a_holder_reached_two_ways_is_one_owner() {
        let builder = fixtures::build_graph();
        let owners = address_owners(&builder.graph);
        let public = builder
            .index_of(&Node::ip("203.0.113.50", fixtures::AWS_NETWORK))
            .unwrap();

        let names: Vec<&Node> = owners[&public].iter().map(|i| &builder.graph[*i]).collect();
        assert_eq!(names, vec![&Node::AwsEc2NatGateway("nat-globex".into())]);
    }

    // Between scans an evicted flow's edge outlives its record. The edge alone
    // must not confirm anything, or the status reads "confirmed, never seen".
    #[test]
    fn a_flow_the_index_no_longer_holds_confirms_nothing() {
        let builder = fixtures::build_graph();
        let observations = observations(&builder.graph, &FlowIndex::default());

        assert!(!observations.is_empty(), "the wired edges still report");
        assert!(observations.iter().all(|o| o.status == INFERRED));
    }

    #[test]
    fn traffic_collapses_the_whole_path_into_one_edge() {
        let builder = fixtures::build_graph();

        for instance in ["i-globex-web-01", "i-globex-web-02"] {
            assert!(
                serves(&builder, &alb(), &Node::AwsEc2Instance(instance.into())),
                "five hops through two interfaces and two pivots should read as one edge"
            );
        }
    }

    // The database cannot be named from a flow record, but its interface can,
    // and that is the answer the design settled on rather than a guess.
    #[test]
    fn an_unowned_interface_is_its_own_attribution() {
        let builder = fixtures::build_graph();

        assert!(serves(
            &builder,
            &Node::AwsEc2Instance("i-globex-web-01".into()),
            &Node::AwsEc2Eni("eni-globex-rds-1a".into()),
        ));
    }

    // Phase 1d: before the other two clouds carried addresses, a flow to a VM
    // or a GCE instance landed on a pivot nothing claimed, and the whole
    // relationship was invisible.
    #[test]
    fn the_other_clouds_compute_is_named_by_the_addresses_it_holds() {
        let builder = fixtures::build_graph();
        let web02 = Node::AwsEc2Instance("i-globex-web-02".into());
        let vm = Node::AzureVirtualMachine(azure_networking_id(
            "Microsoft.Compute/virtualMachines/vm-worker",
        ));

        let nic_address = builder
            .index_of(&Node::ip(
                "10.20.1.4",
                &azure_networking_id("Microsoft.Network/virtualNetworks/vnet-globex"),
            ))
            .expect("the NIC holds its address in its own network");
        let owners: Vec<&Node> = address_owners(&builder.graph)[&nic_address]
            .iter()
            .map(|i| &builder.graph[*i])
            .collect();
        assert_eq!(
            owners,
            vec![&vm],
            "the address is the NIC's, and the NIC is the machine's"
        );
        assert!(
            serves(
                &builder,
                &web02,
                &Node::GcpComputeInstance("7000000000000000001".into()),
            ),
            "a GCE instance holds its own NAT address, with no interface between"
        );
        assert!(
            !serves_any(&builder, |source, target| {
                matches!(source, Node::AzureNetworkInterface(_))
                    || matches!(target, Node::AzureNetworkInterface(_))
            }),
            "a NIC held by a machine is an address, not the service"
        );
    }

    // The balancer's frontend is a public address object, so without a kind to
    // hold it the hop read as `-> Azure::Network::PublicIpAddress(...)`.
    #[test]
    fn a_balancer_frontend_resolves_to_the_balancer() {
        let builder = fixtures::build_graph();

        assert!(serves(
            &builder,
            &Node::AwsEc2Instance("i-globex-web-01".into()),
            &Node::AzureLoadBalancer(azure_networking_id(
                "Microsoft.Network/loadBalancers/lb-globex",
            )),
        ));
        assert!(
            !serves_any(&builder, |source, target| {
                matches!(source, Node::AzurePublicIpAddress(_))
                    || matches!(target, Node::AzurePublicIpAddress(_))
            }),
            "a public address object is held by the resource behind it"
        );
    }

    // A pivot can never itself be a `Serves` endpoint — the pass only ever emits
    // resolved owners — so this asserts on the resolution: the address has no
    // owner, and the flow across it therefore yields no edge at all.
    #[test]
    fn an_endpoint_no_resource_holds_serves_nothing() {
        let builder = fixtures::build_graph();
        // A firewall rule names this address as a destination it allows, which
        // is not the same as holding it — nothing in the estate answers on it.
        let external = Node::ip("192.0.2.55", fixtures::AWS_NETWORK);
        let index = builder
            .index_of(&external)
            .expect("the fixture's internet endpoint is still a pivot");

        assert!(
            !address_owners(&builder.graph).contains_key(&index),
            "naming an address in a rule is not holding it"
        );
        // The fixture flows it into web-01, whose own end does resolve; an
        // unowned peer has to kill the pair, not fall back to the pivot.
        let web01 = builder
            .index_of(&Node::AwsEc2Instance("i-globex-web-01".into()))
            .expect("the other end of that flow");
        assert!(
            !builder
                .graph
                .edge_references()
                .filter(|e| e.weight() == &Edge::Serves)
                .any(|e| e.source() == index
                    || e.target() == index
                    || (e.target() == web01 && builder.graph[e.source()].owner().is_none())),
            "an address no resource claims is the external end, by design"
        );
    }

    // A record set names an address; it does not hold one. Treating it as an
    // owner would draw an arrow claiming a DNS record sent packets.
    #[test]
    fn naming_an_address_is_not_holding_it() {
        let builder = fixtures::build_graph();

        assert!(
            !builder
                .graph
                .edge_references()
                .filter(|e| e.weight() == &Edge::Serves)
                .any(|e| {
                    [e.source(), e.target()].iter().any(|end| {
                        matches!(
                            builder.graph[*end],
                            Node::AwsRoute53RecordSet(_) | Node::CloudflareDnsRecord(_)
                        )
                    })
                }),
            "DNS records reach the graph through ResolvesTo, which is not ownership"
        );
    }

    #[test]
    fn a_resource_never_serves_itself() {
        let builder = fixtures::build_graph();

        assert!(
            !builder
                .graph
                .edge_references()
                .filter(|e| e.weight() == &Edge::Serves)
                .any(|e| e.source() == e.target()),
        );
    }

    fn status_between(
        observations: &[RenderObservation],
        balancer: &str,
        target: &str,
    ) -> &'static str {
        observations
            .iter()
            .find(|o| o.key.contains(balancer) && o.key.ends_with(&format!("{target})")))
            .unwrap_or_else(|| panic!("{balancer} should serve {target}"))
            .status
    }

    fn status_of(observations: &[RenderObservation], target: &str) -> &'static str {
        status_between(observations, "app/globex/1", target)
    }

    #[test]
    fn a_derived_edge_carries_no_volume_of_its_own() {
        let builder = fixtures::build_graph();
        let observations = observations(&builder.graph, &fixtures::observed());

        assert!(!observations.is_empty());
        for observation in &observations {
            assert!(
                observation.packets.is_none() && observation.bytes.is_none(),
                "one edge summarises many flows, so their packets are not its own"
            );
            match observation.status {
                CONFIRMED => assert!(
                    observation.last_seen > 0,
                    "a confirmed edge is stamped with the newest flow that confirmed it"
                ),
                INFERRED => assert_eq!(
                    observation.last_seen, 0,
                    "an inferred edge has never been observed, and must not claim it was"
                ),
                other => panic!("unexpected status {other}"),
            }
        }
    }

    // The registration says the target is wired up; nothing says it is used.
    #[test]
    fn a_target_receiving_nothing_is_inferred_not_absent() {
        let builder = fixtures::build_graph();
        let silent = Node::AwsEc2Instance("i-globex-web-03".into());

        assert!(
            serves(&builder, &alb(), &silent),
            "a registered target is a real relationship even with no traffic"
        );
        let observations = observations(&builder.graph, &fixtures::observed());
        assert_eq!(status_of(&observations, "i-globex-web-03"), INFERRED);
        assert_eq!(status_of(&observations, "i-globex-web-01"), CONFIRMED);
    }

    #[test]
    fn an_ip_target_resolves_through_the_owner_of_its_address() {
        let builder = fixtures::build_graph();
        let nlb = Node::AwsElbLoadBalancer(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/globex-db/1".into(),
        );
        let rds = Node::AwsEc2Eni("eni-globex-rds-1a".into());

        assert!(serves(&builder, &nlb, &rds));
        let observations = observations(&builder.graph, &fixtures::observed());
        assert_eq!(
            status_between(&observations, "net/globex-db/1", "eni-globex-rds-1a"),
            INFERRED
        );
        assert!(
            !serves_any(&builder, |source, target| {
                source.owner().is_none() || target.owner().is_none()
            }),
            "a pivot at the end of a chain is resolved, never emitted as an endpoint"
        );
    }

    // The transition the two provenances exist to express: the wiring outlives
    // the traffic, so the edge stays and only its status moves.
    #[test]
    fn a_lapsed_flow_drops_a_wired_target_back_to_inferred() {
        let mut builder = fixtures::topology();
        super::super::all(&mut builder, &FlowIndex::default());
        let target = Node::AwsEc2Instance("i-globex-web-01".into());

        assert!(
            serves(&builder, &alb(), &target),
            "the control-plane chain alone is enough for the edge to exist"
        );
        assert_eq!(
            status_of(
                &observations(&builder.graph, &FlowIndex::default()),
                "i-globex-web-01"
            ),
            INFERRED
        );
    }

    // The chain is only as honest as the edges it walks. A stopped target keeps
    // its registration and its interface, and neither may bring it back.
    #[test]
    fn a_stopped_target_is_not_inferred() {
        use crate::cloud::definition::{AmazonCollection, Provider};

        let stopped = Node::AwsEc2Instance("i-globex-web-03".into());
        let Provider::AWS(collections) = fixtures::aws() else {
            unreachable!("fixtures::aws is the AWS provider");
        };
        let without_web03 = collections
            .into_iter()
            .map(|(region, collection)| match collection {
                AmazonCollection::AmazonInstances(instances) => (
                    region,
                    AmazonCollection::AmazonInstances(
                        instances
                            .into_iter()
                            .filter(|i| i.instance_id() != Some("i-globex-web-03"))
                            .collect(),
                    ),
                ),
                other => (region, other),
            })
            .collect();
        let mut builder = GraphBuilder::new();
        crate::atlas::projector::build(
            &mut builder,
            &Provider::AWS(without_web03),
            &crate::Settings::default(),
        );
        super::super::all(&mut builder, &FlowIndex::default());

        assert!(builder.index_of(&stopped).is_none());
        assert!(!serves_any(&builder, |_, target| target == &stopped));
    }

    #[test]
    fn a_private_address_seen_from_another_network_names_nothing_there() {
        let builder = fixtures::build_graph();
        let web01 = Node::AwsEc2Instance("i-globex-web-01".into());
        let web02 = Node::AwsEc2Instance("i-globex-web-02".into());

        assert!(!serves(
            &builder,
            &web01,
            &Node::AzureVirtualMachine(azure_networking_id(
                "Microsoft.Compute/virtualMachines/vm-worker",
            )),
        ));
        assert!(!serves(
            &builder,
            &web02,
            &Node::GcpSqlInstance("globex-analytics-db".into()),
        ));
    }

    fn overlapping_estate(target_running: bool) -> GraphBuilder {
        use crate::cloud::definition::{AWSLoadBalancing, AmazonCollection, Provider};
        use aws_sdk_ec2::types::{
            Instance, NetworkInterface, NetworkInterfaceAttachment,
            NetworkInterfacePrivateIpAddress,
        };
        use aws_sdk_elasticloadbalancingv2::types::{
            Action, Listener, LoadBalancer, TargetDescription, TargetGroup,
            TargetHealthDescription, TargetTypeEnum,
        };

        let lb_arn = "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/front/1";
        let tg_arn = "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/back/1";
        let interface = |id: &str, vpc: &str, instance: &str| {
            NetworkInterface::builder()
                .network_interface_id(id)
                .vpc_id(vpc)
                .subnet_id(format!("subnet-{vpc}"))
                .private_ip_addresses(
                    NetworkInterfacePrivateIpAddress::builder()
                        .private_ip_address("10.0.1.5")
                        .build(),
                )
                .attachment(
                    NetworkInterfaceAttachment::builder()
                        .instance_id(instance)
                        .build(),
                )
                .build()
        };
        let mut running = vec![Instance::builder().instance_id("i-other").build()];
        if target_running {
            running.push(Instance::builder().instance_id("i-target").build());
        }

        let mut builder = GraphBuilder::new();
        crate::atlas::projector::build(
            &mut builder,
            &Provider::AWS(
                [
                    AmazonCollection::AmazonNetworkInterfaces(vec![
                        interface("eni-target", "vpc-a", "i-target"),
                        interface("eni-other", "vpc-b", "i-other"),
                    ]),
                    AmazonCollection::AmazonInstances(running),
                    AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing {
                        load_balancers: vec![
                            LoadBalancer::builder()
                                .load_balancer_arn(lb_arn)
                                .vpc_id("vpc-a")
                                .build(),
                        ],
                        target_groups: vec![
                            TargetGroup::builder()
                                .target_group_arn(tg_arn)
                                .target_type(TargetTypeEnum::Ip)
                                .vpc_id("vpc-a")
                                .build(),
                        ],
                        listeners: vec![
                            Listener::builder()
                                .load_balancer_arn(lb_arn)
                                .default_actions(Action::builder().target_group_arn(tg_arn).build())
                                .build(),
                        ],
                        target_health: HashMap::from([(
                            tg_arn.to_owned(),
                            vec![
                                TargetHealthDescription::builder()
                                    .target(TargetDescription::builder().id("10.0.1.5").build())
                                    .build(),
                            ],
                        )]),
                    }),
                ]
                .into_iter()
                .map(|collection| ("us-east-1".to_owned(), collection))
                .collect(),
            ),
            &crate::Settings::default(),
        );
        super::super::all(&mut builder, &FlowIndex::default());
        builder
    }

    fn front() -> Node {
        Node::AwsElbLoadBalancer(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/front/1".into(),
        )
    }

    #[test]
    fn an_ip_target_is_served_only_by_the_holder_in_its_own_network() {
        let builder = overlapping_estate(true);

        assert!(serves(
            &builder,
            &front(),
            &Node::AwsEc2Instance("i-target".into())
        ));
        assert!(!serves(
            &builder,
            &front(),
            &Node::AwsEc2Instance("i-other".into())
        ));
    }

    #[test]
    fn a_stopped_ip_target_is_not_inferred() {
        let builder = overlapping_estate(false);

        assert!(
            !serves_any(&builder, |source, _| source == &front()),
            "the stopped instance's interface is not a service of its own"
        );
    }

    #[test]
    fn traffic_alone_never_claims_a_target_is_registered() {
        let builder = fixtures::build_graph();
        let observations = observations(&builder.graph, &fixtures::observed());

        // web-02 is not in the target group; only traffic puts it behind the
        // balancer, so it may never read as merely wired.
        assert_eq!(status_of(&observations, "i-globex-web-02"), CONFIRMED);
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

    // A chain must be matched out of the scan's own edges. Walking a derived
    // one would let the pass read its own output back as evidence.
    #[test]
    fn derivation_does_not_feed_on_its_own_edges() {
        let mut builder = fixtures::build_graph();
        let flows = fixtures::observed();
        let before = (builder.graph.node_count(), builder.graph.edge_count());

        super::super::all(&mut builder, &flows);
        super::super::all(&mut builder, &flows);

        assert_eq!(
            before,
            (builder.graph.node_count(), builder.graph.edge_count())
        );
    }

    // Both of the instance's interfaces carry the same relationship, so the
    // edge's freshness has to compose as a maximum over every flow behind it —
    // the property that lets one edge summarise many without double counting.
    #[test]
    fn freshness_is_the_newest_flow_behind_the_edge() {
        let mut builder = fixtures::topology();
        let mut flows = FlowIndex::default();
        for (dst, at) in [("10.10.1.10", 1_000), ("10.10.2.20", 9_000)] {
            flows.observe(&FlowObservation {
                source: crate::atlas::collection::CollectionSource::Aws,
                scope: "us-east-1".to_owned(),
                src: Node::ip("10.10.1.50", fixtures::AWS_NETWORK),
                dst: Node::ip(dst, fixtures::AWS_NETWORK),
                src_port: Some(43122),
                dst_port: Some(8080),
                resources: Vec::new(),
                packets: 1,
                bytes: 1,
                action: None,
                observed_at: at,
            });
        }

        flows.overlay(&mut builder);
        super::super::all(&mut builder, &flows);

        assert!(serves(
            &builder,
            &alb(),
            &Node::AwsEc2Instance("i-globex-web-01".into())
        ));
        let edge = observations(&builder.graph, &flows)
            .into_iter()
            .find(|o| o.key.contains("app/globex/1") && o.key.ends_with("i-globex-web-01)"))
            .expect("the balancer serves the instance it was seen talking to");
        assert_eq!(edge.last_seen, 9_000);
    }
}
