use atlas_lib::atlas::collection::{CollectionReport, CollectionSource, FailureKind};
use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::derive;
use atlas_lib::atlas::event::{ChangeEvent, ChangeOp, EventApplier};
use atlas_lib::atlas::export::node_key;
use atlas_lib::atlas::flow::{FlowAction, FlowIndex, FlowObservation, Orientation, orientation};
use atlas_lib::atlas::graph_builder::GraphBuilder;
use atlas_lib::atlas::patch::{GraphPatch, Retention, carry_forward, diff};
use atlas_lib::atlas::util::{
    canonical_address, canonical_hostname, mask_to_prefix, parse_address,
};
use atlas_lib::cloud::amazon::flow_logs;
use petgraph::graph::Graph;
use proptest::prelude::*;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;

const T0: i64 = 1_788_436_800_000;

fn source() -> impl Strategy<Value = CollectionSource> {
    prop_oneof![
        Just(CollectionSource::Aws),
        Just(CollectionSource::Gcp),
        Just(CollectionSource::Azure),
        Just(CollectionSource::Cloudflare),
    ]
}

fn failure_kind() -> impl Strategy<Value = FailureKind> {
    prop_oneof![
        Just(FailureKind::Unavailable),
        Just(FailureKind::Unauthorized),
        Just(FailureKind::Malformed),
    ]
}

// ---------------------------------------------------------------- collection

proptest! {
    #[test]
    fn unreadable_kind_is_unauthorized_exactly_when_every_unreadable_failure_was_a_refusal(
        failures in prop::collection::vec((source(), failure_kind()), 0..12),
        asked in source(),
    ) {
        let mut report = CollectionReport::default();
        for (source, kind) in &failures {
            report.note(*source, *kind, "scope", "message");
        }

        let unreadable: Vec<FailureKind> = failures
            .iter()
            .filter(|(s, k)| *s == asked && k.is_unreadable())
            .map(|(_, k)| *k)
            .collect();

        let expected = if unreadable.is_empty() {
            None
        } else if unreadable.iter().all(|k| *k == FailureKind::Unauthorized) {
            Some(FailureKind::Unauthorized)
        } else {
            Some(FailureKind::Unavailable)
        };

        prop_assert_eq!(report.unreadable_kind(asked), expected);
        prop_assert_eq!(
            report.unreadable_sources().contains(&asked),
            expected.is_some()
        );
        prop_assert_eq!(report.is_complete(), failures.is_empty());
    }
}

// ----------------------------------------------------------------- retention

proptest! {
    #[test]
    fn a_source_is_held_for_exactly_the_budget_its_evidence_earns(
        budget in 0u32..6,
        evidence in prop::collection::vec(failure_kind(), 1..4),
        run in 1usize..12,
    ) {
        // The short budget only exists to be shorter; if that ever stopped
        // being true a refusal would pin a provider for a full outage.
        prop_assert!(Retention::AUTH_BUDGET < Retention::DEFAULT_BUDGET);

        let mut retention = Retention::new(budget);

        // A drifted row is not an unreadable source, so evidence made only of
        // `Malformed` holds nothing at all; mixed evidence gets the longer
        // budget, because only an unbroken run of refusals diagnoses auth.
        let unreadable: Vec<FailureKind> =
            evidence.iter().copied().filter(|k| k.is_unreadable()).collect();
        let effective = if unreadable.is_empty() {
            0
        } else if unreadable.iter().all(|k| *k == FailureKind::Unauthorized) {
            Retention::AUTH_BUDGET.min(budget)
        } else {
            budget
        };

        let mut report = CollectionReport::default();
        for kind in &evidence {
            report.note(CollectionSource::Aws, *kind, "scope", "boom");
        }

        for attempt in 1..=run as u32 {
            let held = retention.hold(&report);
            prop_assert_eq!(
                held.contains(&CollectionSource::Aws),
                attempt <= effective,
                "failure {} against an effective budget of {}",
                attempt,
                effective
            );
        }

        // Recovery resets the streak, so a source that fails again later is
        // protected again rather than being released immediately.
        retention.hold(&CollectionReport::default());
        prop_assert_eq!(retention.streak(CollectionSource::Aws), 0);
        prop_assert_eq!(
            retention.hold(&report).contains(&CollectionSource::Aws),
            effective >= 1
        );
    }
}

// --------------------------------------------------------------- orientation

// The property below checks shape (antisymmetry, ports-required) that holds
// regardless of where the ephemeral floor sits; it deliberately does not
// restate orientation()'s own branch logic, since a copy of that logic would
// drift together with a bug introduced in the original. The floor's exact
// value and the tie-break it implies are pinned by concrete cases instead,
// each hand-computed against the doc comment on orientation() rather than
// against the implementation.
#[test]
fn the_non_ephemeral_end_is_the_service() {
    assert_eq!(
        orientation(Some(43122), Some(8080)),
        Some(Orientation::Forward)
    );
    assert_eq!(
        orientation(Some(8080), Some(43122)),
        Some(Orientation::Reverse)
    );
}

// A NAT gateway's translated source ports span 1024-65535 — ordinary, non-
// ephemeral numbers — so with neither port ephemeral the lower one is taken
// to be the service, keeping the gateway from reading as the server of a 443
// it is only relaying.
#[test]
fn with_no_ephemeral_port_the_lower_one_is_the_service() {
    assert_eq!(
        orientation(Some(20000), Some(443)),
        Some(Orientation::Forward)
    );
    assert_eq!(
        orientation(Some(443), Some(20000)),
        Some(Orientation::Reverse)
    );
}

// A single ephemeral port and a magnitude comparison always agree on which
// end is the service (the floor guarantees an ephemeral port outranks any
// non-ephemeral one), so the floor only changes anything once both ends sit
// at or above it — the point where a decisive answer becomes an ambiguous
// pair of clients.
#[test]
fn the_ephemeral_floor_is_exactly_32768() {
    assert_eq!(
        orientation(Some(32_767), Some(40_000)),
        Some(Orientation::Reverse),
        "32767 is not yet ephemeral, so it is still weighed against 40000"
    );
    assert_eq!(
        orientation(Some(32_768), Some(40_000)),
        None,
        "32768 is ephemeral, so both ends are ephemeral and the pair is ambiguous"
    );
}

#[test]
fn what_cannot_be_told_apart_is_left_to_packet_direction() {
    assert_eq!(
        orientation(Some(50000), Some(60000)),
        None,
        "two high ports"
    );
    assert_eq!(orientation(Some(5060), Some(5060)), None, "identical ports");
    assert_eq!(orientation(Some(0), Some(0)), None, "ICMP carries no ports");
    assert_eq!(orientation(None, Some(443)), None, "a format without ports");
}

proptest! {
    #[test]
    fn orientation_is_antisymmetric_and_needs_both_ports(
        a in 0u16..=u16::MAX,
        b in 0u16..=u16::MAX,
    ) {
        let forward = orientation(Some(a), Some(b));
        let reverse = orientation(Some(b), Some(a));

        // Swapping the record swaps which end is the service, always — true
        // for any floor and any tie-break, so it holds without restating them.
        let mirrored = match forward {
            Some(Orientation::Forward) => Some(Orientation::Reverse),
            Some(Orientation::Reverse) => Some(Orientation::Forward),
            None => None,
        };
        prop_assert_eq!(reverse, mirrored);

        // A port is never read out of a record that has none.
        prop_assert_eq!(orientation(None, Some(b)), None);
        prop_assert_eq!(orientation(Some(a), None), None);

        // Identical ports are always a tie, whatever the floor.
        if a == b {
            prop_assert_eq!(forward, None);
        }
    }
}

// ------------------------------------------------------------ canonicalising

fn address_text() -> impl Strategy<Value = String> {
    prop_oneof![
        (any::<u32>(), 0u8..=32).prop_map(|(bits, prefix)| {
            format!(
                "{}/{}",
                IpAddr::from(std::net::Ipv4Addr::from(bits)),
                prefix
            )
        }),
        any::<u32>().prop_map(|bits| std::net::Ipv4Addr::from(bits).to_string()),
        (any::<u128>(), 0u8..=128).prop_map(|(bits, prefix)| {
            format!("{}/{}", std::net::Ipv6Addr::from(bits), prefix)
        }),
        any::<u128>().prop_map(|bits| {
            // The long form, so canonicalisation has something to collapse.
            let octets = std::net::Ipv6Addr::from(bits).segments();
            octets
                .iter()
                .map(|s| format!("{s:04x}"))
                .collect::<Vec<_>>()
                .join(":")
        }),
        "[a-zA-Z0-9.\\-/]{1,20}",
    ]
}

proptest! {
    #[test]
    fn canonicalising_an_address_is_idempotent_and_reparses_to_the_same_value(
        text in address_text(),
    ) {
        let Some(once) = canonical_address(&text) else {
            prop_assert_eq!(parse_address(&text), None, "unparseable stays unparseable");
            return Ok(());
        };

        let twice = canonical_address(&once);
        prop_assert_eq!(twice.as_deref(), Some(once.as_str()), "not idempotent");
        prop_assert_eq!(parse_address(&once), parse_address(&text));
    }
}

proptest! {
    #[test]
    fn two_spellings_of_one_address_land_on_one_pivot(
        bits in any::<u128>(),
    ) {
        let addr = std::net::Ipv6Addr::from(bits);
        let long: String = addr
            .segments()
            .iter()
            .map(|s| format!("{s:04x}"))
            .collect::<Vec<_>>()
            .join(":");

        prop_assert_eq!(Node::ip(&long), Node::ip(&addr.to_string()));
        prop_assert_eq!(
            Node::ip(&addr.to_string().to_uppercase()),
            Node::ip(&addr.to_string())
        );
    }
}

proptest! {
    // address_text()'s v6 strategy draws from any::<u128>() and essentially
    // never lands on an IPv4-mapped address, so it cannot exercise the one
    // branch in parse_address that treats a mapped address specially. Here
    // the mapped form is built directly, with the prefix drawn independently
    // of the address, so both sides of the /96 boundary get real coverage.
    #[test]
    fn a_mapped_v6_prefix_collapses_to_ipv4_only_at_or_past_the_mapped_block(
        bits in any::<u32>(),
        width in 0u8..=128,
    ) {
        let v4 = std::net::Ipv4Addr::from(bits);
        let mapped = format!("::ffff:{v4}/{width}");

        let once = canonical_address(&mapped).expect("a mapped address always parses");

        if width >= 96 {
            prop_assert_eq!(&once, &format!("{v4}/{}", width - 96));
        } else {
            // Narrower than the ::ffff:0:0/96 block spans more than the v4
            // range it looks like, so it must not be read as that range.
            prop_assert_eq!(&once, &mapped, "a prefix under /96 must stay IPv6");
        }

        let twice = canonical_address(&once);
        prop_assert_eq!(twice.as_deref(), Some(once.as_str()));
    }
}

proptest! {
    #[test]
    fn hostnames_canonicalise_to_one_spelling_and_an_empty_one_is_no_pivot(
        labels in prop::collection::vec("[a-zA-Z0-9-]{1,12}", 1..5),
        trailing in 0usize..4,
        upper in any::<bool>(),
    ) {
        let name = labels.join(".");
        let spelled = if upper { name.to_uppercase() } else { name.clone() };
        let dotted = format!("{}{}", spelled, ".".repeat(trailing));

        let Some(once) = canonical_hostname(&dotted) else {
            prop_assert!(false, "{dotted} is a hostname");
            unreachable!()
        };
        prop_assert_eq!(&once, &name.to_ascii_lowercase());
        let twice = canonical_hostname(&once);
        prop_assert_eq!(twice.as_deref(), Some(once.as_str()));
        prop_assert_eq!(Node::hostname(&dotted), Node::hostname(&name));
    }
}

proptest! {
    #[test]
    fn a_hostname_of_nothing_but_dots_is_not_a_hostname(dots in 1usize..6) {
        prop_assert_eq!(canonical_hostname(&".".repeat(dots)), None);
    }
}

// --------------------------------------------------------------- containment

fn cidr() -> impl Strategy<Value = (IpAddr, u8)> {
    prop_oneof![
        (any::<u32>(), 8u8..=32)
            .prop_map(|(bits, p)| (IpAddr::from(std::net::Ipv4Addr::from(bits)), p)),
        (any::<u128>(), 16u8..=128)
            .prop_map(|(bits, p)| (IpAddr::from(std::net::Ipv6Addr::from(bits)), p)),
    ]
}

fn bare_address() -> impl Strategy<Value = IpAddr> {
    prop_oneof![
        any::<u32>().prop_map(|b| IpAddr::from(std::net::Ipv4Addr::from(b))),
        any::<u128>().prop_map(|b| IpAddr::from(std::net::Ipv6Addr::from(b))),
    ]
}

proptest! {
    #[test]
    fn a_range_covers_exactly_the_addresses_inside_it(
        ranges in prop::collection::vec(cidr(), 1..5),
        addresses in prop::collection::vec(bare_address(), 1..5),
        junk in prop::collection::vec("pl-[0-9a-f]{8}|[A-Za-z]{2,8}", 0..3),
    ) {
        let mut builder = GraphBuilder::new();
        for (addr, prefix) in &ranges {
            builder.get_or_add_node(Node::ip(&format!("{addr}/{prefix}")));
        }
        for addr in &addresses {
            builder.get_or_add_node(Node::ip(&addr.to_string()));
        }
        // Prefix-list ids and service tags travel the same pivot; containment
        // has to walk past them rather than guess at them.
        for value in &junk {
            builder.get_or_add_node(Node::ip(value));
        }

        derive::all(&mut builder, &FlowIndex::default());

        // A rule that subsumes another rule says nothing about traffic, and an
        // unparseable value is nobody's address: every `Covers` edge runs from
        // a CIDR to a bare address, both of which parse.
        for edge in builder.graph.edge_indices() {
            if builder.graph[edge] != Edge::Covers {
                continue;
            }
            let (from, to) = builder.graph.edge_endpoints(edge).expect("endpoints");
            let (Node::GenericIpAddress(from), Node::GenericIpAddress(to)) =
                (&builder.graph[from], &builder.graph[to])
            else {
                prop_assert!(false, "Covers left the pivots");
                unreachable!()
            };
            prop_assert!(
                matches!(parse_address(from), Some((_, Some(_)))),
                "{from} is not a range"
            );
            prop_assert!(
                matches!(parse_address(to), Some((_, None))),
                "{to} is not a bare address"
            );
        }

        for (range_addr, prefix) in &ranges {
            let range = Node::ip(&format!("{range_addr}/{prefix}"));
            for addr in &addresses {
                let inside = range_addr.is_ipv4() == addr.is_ipv4()
                    && mask_to_prefix(addr, *prefix) == mask_to_prefix(range_addr, *prefix);

                prop_assert_eq!(
                    builder.has_edge(&range, &Node::ip(&addr.to_string()), &Edge::Covers),
                    inside,
                    "{} vs {}/{}",
                    addr,
                    range_addr,
                    prefix
                );
            }
        }
    }
}

proptest! {
    #[test]
    fn derivation_is_idempotent_however_many_times_a_tick_runs_it(
        ranges in prop::collection::vec(cidr(), 0..4),
        addresses in prop::collection::vec(bare_address(), 0..4),
        runs in 2usize..5,
    ) {
        let mut builder = GraphBuilder::new();
        for (addr, prefix) in &ranges {
            builder.get_or_add_node(Node::ip(&format!("{addr}/{prefix}")));
        }
        for addr in &addresses {
            builder.get_or_add_node(Node::ip(&addr.to_string()));
        }

        let flows = FlowIndex::default();
        derive::all(&mut builder, &flows);
        let settled = builder.graph.clone();

        for _ in 1..runs {
            derive::all(&mut builder, &flows);
        }

        prop_assert!(diff(&settled, &builder.graph).is_empty());
    }
}

// --------------------------------------------------------------- event tiers

fn instance(id: u8) -> Node {
    Node::AwsEc2Instance(format!("i-{id}").into())
}

fn context_for(id: u8) -> Graph<Node, Edge> {
    let mut builder = GraphBuilder::new();
    let vpc = builder.get_or_add_node(Node::AwsEc2Vpc("vpc-1".into()));
    let subnet = builder.link_to(vpc, Node::AwsEc2Subnet("subnet-1".into()), Edge::Contains);
    let inst = builder.get_or_add_node(instance(id));
    let eni = builder.link_to(
        inst,
        Node::AwsEc2Eni(format!("eni-{id}").into()),
        Edge::HasIp,
    );
    builder.add_edge(eni, subnet, Edge::AttachedTo);
    builder.graph
}

#[derive(Debug, Clone, Copy)]
struct Statement {
    id: u8,
    op: ChangeOp,
    at: i64,
}

fn statement() -> impl Strategy<Value = Statement> {
    (
        0u8..3,
        prop_oneof![
            Just(ChangeOp::Created),
            Just(ChangeOp::Modified),
            Just(ChangeOp::Deleted)
        ],
        0i64..6,
    )
        .prop_map(|(id, op, tick)| Statement {
            id,
            op,
            at: T0 + tick * 1_000,
        })
}

// Last-writer-wins can only be order-independent where the record times it
// compares are distinct: two statements about one resource stamped the same
// millisecond are a tie, and a tie is settled by whichever arrives last. The
// clouds stamp to the millisecond, so this narrows the generator rather than
// the guarantee.
fn distinctly_timed(size: std::ops::Range<usize>) -> impl Strategy<Value = Vec<Statement>> {
    prop::collection::vec(statement(), size).prop_map(|statements| {
        let mut seen = HashSet::new();
        statements
            .into_iter()
            .filter(|s| seen.insert((s.id, s.at)))
            .collect()
    })
}

fn event_of(statement: Statement) -> ChangeEvent {
    let event = ChangeEvent::new(
        CollectionSource::Aws,
        "us-east-1",
        "evt",
        statement.at,
        statement.op,
        instance(statement.id),
    );
    match statement.op {
        ChangeOp::Deleted => event,
        _ => event.with_context(context_for(statement.id)),
    }
}

fn fold(statements: &[Statement]) -> GraphBuilder {
    let mut live = GraphBuilder::new();
    let mut applier = EventApplier::new();
    for statement in statements {
        applier.apply(&mut live, &event_of(*statement));
    }
    live
}

proptest! {
    #[test]
    fn an_unordered_at_least_once_feed_converges_on_the_same_graph(
        statements in distinctly_timed(1..10),
        shuffle in prop::collection::vec(0usize..1_000, 0..20),
        duplicates in prop::collection::vec(0usize..1_000, 0..10),
    ) {
        // Timestamps are the cloud's record time, so an ordered delivery is the
        // ground truth every permutation has to agree with. Only for the
        // resources events *name*, though: a delete speaks for its own node,
        // so whether a create's neighbourhood ever landed depends on arrival
        // order, and reconciling that is Tier 3's job.
        let mut ordered = statements.clone();
        ordered.sort_by_key(|s| s.at);
        let settled = fold(&ordered);

        let mut delivered = statements.clone();
        for (round, pick) in shuffle.iter().enumerate() {
            let len = delivered.len();
            delivered.swap(pick % len, round % len);
        }
        for pick in &duplicates {
            let choice = delivered[pick % delivered.len()];
            delivered.push(choice);
        }
        let live = fold(&delivered);

        for statement in &statements {
            let named = instance(statement.id);
            prop_assert_eq!(
                live.contains(&named),
                settled.contains(&named),
                "{} disagrees with an in-order delivery",
                named
            );
        }
    }
}

proptest! {
    #[test]
    fn events_only_ever_add_or_remove_the_node_they_name(
        statements in prop::collection::vec(statement(), 1..8),
    ) {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        for statement in &statements {
            let before: HashSet<String> = live.graph.node_weights().map(node_key).collect();
            let event = event_of(*statement);
            applier.apply(&mut live, &event);
            let after: HashSet<String> = live.graph.node_weights().map(node_key).collect();

            let removed: Vec<&String> = before.difference(&after).collect();
            prop_assert!(
                removed.len() <= 1,
                "an event removed more than its own resource: {:?}",
                removed
            );
            if let Some(gone) = removed.first() {
                prop_assert_eq!(*gone, &node_key(&event.node));
            }
        }
    }
}

// ------------------------------------------------------------------ flow index

fn observation(dst: u8, at: i64, action: Option<FlowAction>) -> FlowObservation {
    FlowObservation {
        source: CollectionSource::Aws,
        scope: "us-east-1".to_owned(),
        src: Node::ip("10.0.0.1"),
        dst: Node::ip(&format!("203.0.113.{dst}")),
        src_port: None,
        dst_port: None,
        resources: Vec::new(),
        packets: 10,
        bytes: 840,
        action,
        observed_at: at,
    }
}

proptest! {
    #[test]
    fn the_index_never_exceeds_its_capacity_and_keeps_the_freshest(
        capacity in 1usize..40,
        records in prop::collection::vec((0u8..200, 0i64..500), 1..200),
    ) {
        let mut index = FlowIndex::new(Duration::from_secs(3_600), capacity);
        let mut newest: HashMap<u8, i64> = HashMap::new();

        for (dst, tick) in &records {
            let at = T0 + tick;
            index.observe(&observation(*dst, at, Some(FlowAction::Accepted)));
            let seen = newest.entry(*dst).or_insert(at);
            *seen = (*seen).max(at);
        }

        prop_assert!(index.flow_count() <= capacity, "{} > {}", index.flow_count(), capacity);

        // Eviction is by age, so the newest sighting is the one nothing can
        // take: which *pair* holds it is arbitrary when several share it.
        let retained = index
            .observations()
            .into_iter()
            .filter(|o| o.packets.is_some())
            .map(|o| o.last_seen)
            .max();

        prop_assert_eq!(
            retained,
            newest.values().copied().max(),
            "the most recently seen flow was evicted"
        );
    }
}

proptest! {
    #[test]
    fn a_status_reads_from_the_verdicts_it_actually_saw(
        verdicts in prop::collection::vec(
            prop_oneof![
                Just(Some(FlowAction::Accepted)),
                Just(Some(FlowAction::Rejected)),
                Just(None),
            ],
            1..10,
        ),
    ) {
        let mut index = FlowIndex::default();
        for (tick, action) in verdicts.iter().enumerate() {
            index.observe(&observation(1, T0 + tick as i64, *action));
        }

        let stats = index
            .stats(&Node::ip("10.0.0.1"), &Node::ip("203.0.113.1"))
            .expect("observed");

        let accepted = verdicts.contains(&Some(FlowAction::Accepted));
        let rejected = verdicts.contains(&Some(FlowAction::Rejected));
        let expected = match (accepted, rejected) {
            (true, true) => "mixed",
            (false, true) => "rejected",
            (true, false) => "accepted",
            (false, false) => "observed",
        };

        prop_assert_eq!(stats.status(), expected);
        prop_assert_eq!(stats.packets, 10 * verdicts.len() as u64);
        prop_assert_eq!(stats.last_seen, T0 + verdicts.len() as i64 - 1);
    }
}

proptest! {
    #[test]
    fn everything_older_than_the_ttl_lapses_and_nothing_younger_does(
        ttl_secs in 1u64..600,
        records in prop::collection::vec((0u8..30, 0i64..1_200), 1..40),
    ) {
        let ttl = Duration::from_secs(ttl_secs);
        let mut index = FlowIndex::new(ttl, 10_000);
        let mut newest: HashMap<u8, i64> = HashMap::new();

        for (dst, tick) in &records {
            let at = T0 + tick * 1_000;
            index.observe(&observation(*dst, at, None));
            let seen = newest.entry(*dst).or_insert(at);
            *seen = (*seen).max(at);
        }

        let now = T0 + 1_200 * 1_000;
        index.expire(now);
        let horizon = now - ttl.as_millis() as i64;

        let alive = newest.values().filter(|at| **at >= horizon).count();
        prop_assert_eq!(index.flow_count(), alive);
    }
}

// ---------------------------------------------------------------- graph patch

proptest! {
    #[test]
    fn folding_patches_says_the_same_thing_as_diffing_the_endpoints(
        steps in prop::collection::vec(prop::collection::vec(0u8..6, 0..5), 1..6),
    ) {
        let build = |ids: &[u8]| {
            let mut builder = GraphBuilder::new();
            for id in ids {
                let inst = builder.get_or_add_node(instance(*id));
                builder.link_to(inst, Node::AwsEc2Eni(format!("eni-{id}").into()), Edge::HasIp);
            }
            builder.graph
        };

        let graphs: Vec<Graph<Node, Edge>> = steps.iter().map(|ids| build(ids)).collect();

        let mut folded = GraphPatch::empty();
        for pair in graphs.windows(2) {
            folded.extend(diff(&pair[0], &pair[1]));
        }

        // A client applies removals before additions, so what has to match is
        // the state the fold lands a client in, not the patch's key sets.
        let mut client: HashSet<String> = graphs[0].node_weights().map(node_key).collect();
        for key in &folded.removed_nodes {
            client.remove(key);
        }
        for node in &folded.added_nodes {
            client.insert(node.key.clone());
        }

        let expected: HashSet<String> = graphs[graphs.len() - 1]
            .node_weights()
            .map(node_key)
            .collect();

        prop_assert_eq!(client, expected);
    }
}

// -------------------------------------------------------------- carry forward

proptest! {
    #[test]
    fn carry_forward_restores_unreadable_territory_and_never_a_healthy_source_s(
        unreadable in prop::collection::hash_set(source(), 0..4),
        dropped in prop::collection::hash_set(source(), 0..4),
    ) {
        let live = atlas_lib::fixtures::build_graph().graph;

        let mut trimmed = live.clone();
        trimmed.retain_nodes(|g, i| {
            g[i].owner().is_none_or(|owner| !dropped.contains(&owner))
        });

        let mut next = GraphBuilder::new();
        next.merge(&trimmed);
        carry_forward(&mut next, &live, &unreadable);

        let owned = |graph: &Graph<Node, Edge>, source: CollectionSource| {
            graph
                .node_weights()
                .filter(|n| n.owner() == Some(source))
                .count()
        };

        for provider in [
            CollectionSource::Aws,
            CollectionSource::Gcp,
            CollectionSource::Azure,
            CollectionSource::Cloudflare,
        ] {
            if unreadable.contains(&provider) {
                prop_assert_eq!(
                    owned(&next.graph, provider),
                    owned(&live, provider),
                    "an unreadable source must keep everything it owned: {}",
                    provider
                );
            } else if dropped.contains(&provider) {
                prop_assert_eq!(
                    owned(&next.graph, provider),
                    0,
                    "a healthy source's deletions must go through: {}",
                    provider
                );
            }
        }
    }
}

// -------------------------------------------------------------- node removal

proptest! {
    #[test]
    fn every_surviving_node_still_resolves_to_itself_after_any_removals(
        present in prop::collection::vec(0u8..12, 1..15),
        removals in prop::collection::vec(0u8..12, 0..15),
    ) {
        let mut builder = GraphBuilder::new();
        for id in &present {
            let inst = builder.get_or_add_node(instance(*id));
            builder.link_to(inst, Node::AwsEc2Eni(format!("eni-{id}").into()), Edge::HasIp);
        }

        for id in &removals {
            let target = instance(*id);
            let incident = builder.index_of(&target).map(|at| {
                builder
                    .graph
                    .edges_directed(at, petgraph::Direction::Outgoing)
                    .count()
                    + builder
                        .graph
                        .edges_directed(at, petgraph::Direction::Incoming)
                        .count()
            });

            match builder.remove_node(&target) {
                // A caller repairing the render has to be told about every
                // edge that died, not just the node.
                Some(removal) => {
                    prop_assert_eq!(&removal.node, &target);
                    prop_assert_eq!(Some(removal.edges.len()), incident);
                }
                None => prop_assert_eq!(incident, None, "{} was present", target),
            }
        }

        // petgraph's remove_node is a swap-remove: the last node takes the
        // freed index, so a stale node_map entry would address the wrong node.
        for index in builder.graph.node_indices() {
            let node = builder.graph[index].clone();
            prop_assert_eq!(
                builder.index_of(&node),
                Some(index),
                "{} no longer addresses itself",
                node
            );
        }
        prop_assert_eq!(builder.node_map.len(), builder.graph.node_count());

        for id in &removals {
            prop_assert!(!builder.contains(&instance(*id)));
        }
    }
}

// ------------------------------------------------------------ flow log layout

const FIELDS: &[&str] = &[
    "version",
    "account-id",
    "interface-id",
    "instance-id",
    "srcaddr",
    "dstaddr",
    "srcport",
    "dstport",
    "protocol",
    "packets",
    "bytes",
    "start",
    "end",
    "action",
    "log-status",
];

proptest! {
    #[test]
    fn a_record_reads_the_same_whatever_order_its_own_header_declares(
        order in Just(FIELDS.to_vec()).prop_shuffle(),
        src in any::<u32>(),
        dst in any::<u32>(),
        src_port in 1u16..=u16::MAX,
        dst_port in 1u16..=u16::MAX,
        packets in 0u64..100_000,
        bytes in 0u64..10_000_000,
        end in 1i64..2_000_000_000,
        accepted in any::<bool>(),
    ) {
        let src = std::net::Ipv4Addr::from(src).to_string();
        let dst = std::net::Ipv4Addr::from(dst).to_string();
        let action = if accepted { "ACCEPT" } else { "REJECT" };

        let cell = |field: &str| match field {
            "version" => "2".to_owned(),
            "account-id" => "111111111111".to_owned(),
            "interface-id" => "eni-0abc".to_owned(),
            "instance-id" => "i-0abc".to_owned(),
            "srcaddr" => src.clone(),
            "dstaddr" => dst.clone(),
            "srcport" => src_port.to_string(),
            "dstport" => dst_port.to_string(),
            "protocol" => "6".to_owned(),
            "packets" => packets.to_string(),
            "bytes" => bytes.to_string(),
            "start" => (end - 1).to_string(),
            "end" => end.to_string(),
            "action" => action.to_owned(),
            "log-status" => "OK".to_owned(),
            other => panic!("unmodelled field {other}"),
        };

        let object = format!(
            "{}\n{}",
            order.join(" "),
            order.iter().map(|f| cell(f)).collect::<Vec<_>>().join(" ")
        );

        let parsed = flow_logs::parse(&object, "us-east-1", usize::MAX);

        prop_assert_eq!(parsed.unusable, 0);
        prop_assert!(!parsed.unusable_layout);
        let [flow] = &parsed.observations[..] else {
            prop_assert!(false, "expected one record from {:?}", order);
            unreachable!()
        };

        // Field order is the object's own business; what the projector reads
        // has to come out identical regardless.
        prop_assert_eq!(&flow.src, &Node::ip(&src));
        prop_assert_eq!(&flow.dst, &Node::ip(&dst));
        prop_assert_eq!(flow.src_port, Some(src_port));
        prop_assert_eq!(flow.dst_port, Some(dst_port));
        prop_assert_eq!(flow.packets, packets);
        prop_assert_eq!(flow.bytes, bytes);
        prop_assert_eq!(flow.observed_at, end * 1_000);
        prop_assert_eq!(
            flow.action,
            Some(if accepted { FlowAction::Accepted } else { FlowAction::Rejected })
        );
        prop_assert_eq!(
            &flow.resources,
            &vec![Node::AwsEc2Instance("i-0abc".into()), Node::AwsEc2Eni("eni-0abc".into())],
            "both identities, keyed the way the projector keys them"
        );
    }
}

// ----------------------------------------------------------- address owners

// A stack of holders over one address: `Instance -HasIp-> Eni -HasIp-> Eip`,
// with the bottom of the chain the thing that claims the pivot. Whatever the
// depth, the answer to "whose address is this" is the top of the stack.
#[derive(Debug, Clone, Copy)]
struct Stack {
    id: u8,
    depth: usize,
    owns: bool,
    diamond: bool,
    cycle: bool,
}

fn stack() -> impl Strategy<Value = Stack> {
    (
        0u8..8,
        1usize..5,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(id, depth, owns, diamond, cycle)| Stack {
            id,
            depth,
            owns,
            diamond,
            cycle,
        })
}

fn top_of(stack: Stack) -> Node {
    Node::AwsEc2Instance(format!("i-{}", stack.id).into())
}

fn pivot_of(stack: Stack) -> Node {
    Node::ip(&format!("10.{}.0.1", stack.id))
}

fn wire(builder: &mut GraphBuilder, stack: Stack) {
    let top = top_of(stack);
    let mut chain = vec![top.clone()];
    for step in 1..stack.depth {
        chain.push(Node::AwsEc2Eni(format!("eni-{}-{}", stack.id, step).into()));
    }

    for pair in chain.windows(2) {
        let holder = builder.get_or_add_ref(&pair[0]);
        builder.link_to(holder, pair[1].clone(), Edge::HasIp);
    }

    let claimer = chain.last().expect("a stack has a bottom").clone();

    // The same holder reached by two roads is still one owner.
    if stack.diamond && stack.depth >= 2 {
        let top_index = builder.get_or_add_ref(&top);
        builder.link_to(top_index, claimer.clone(), Edge::HasIp);
    }
    // A cycle below the owner must not hide it — or hang the walk.
    if stack.cycle && stack.depth >= 3 {
        let lower = builder.get_or_add_ref(&chain[2]);
        builder.link_to(lower, chain[1].clone(), Edge::HasIp);
    }

    // `ResolvesTo` names an address; only `ConnectsTo` holds one.
    let edge = if stack.owns {
        Edge::ConnectsTo
    } else {
        Edge::ResolvesTo
    };
    let claimer_index = builder.get_or_add_ref(&claimer);
    builder.link_to(claimer_index, pivot_of(stack), edge);
}

proptest! {
    #[test]
    fn an_address_is_served_by_whatever_ultimately_holds_it(
        stacks in prop::collection::vec(stack(), 2..5),
    ) {
        // One stack per id, or two would fight over the same pivot.
        let mut seen = HashSet::new();
        let stacks: Vec<Stack> = stacks.into_iter().filter(|s| seen.insert(s.id)).collect();
        prop_assume!(stacks.len() >= 2);

        let mut builder = GraphBuilder::new();
        for stack in &stacks {
            wire(&mut builder, *stack);
        }

        // An ephemeral source port against a service port orients every record
        // the same way: the destination is the service.
        let mut flows = FlowIndex::default();
        let mut expected: HashSet<(String, String)> = HashSet::new();
        for pair in stacks.windows(2) {
            let (client, server) = (pair[0], pair[1]);
            flows.observe(&FlowObservation {
                source: CollectionSource::Aws,
                scope: "us-east-1".to_owned(),
                src: pivot_of(client),
                dst: pivot_of(server),
                src_port: Some(43_122),
                dst_port: Some(443),
                resources: Vec::new(),
                packets: 10,
                bytes: 840,
                action: Some(FlowAction::Accepted),
                observed_at: T0,
            });
            if client.owns && server.owns {
                expected.insert((
                    node_key(&top_of(client)),
                    node_key(&top_of(server)),
                ));
            }
        }

        flows.overlay(&mut builder);
        derive::all(&mut builder, &flows);

        let served: HashSet<(String, String)> = builder
            .graph
            .edge_indices()
            .filter(|e| builder.graph[*e] == Edge::Serves)
            .filter_map(|e| {
                builder.graph.edge_endpoints(e).map(|(a, b)| {
                    (
                        node_key(&builder.graph[a]),
                        node_key(&builder.graph[b]),
                    )
                })
            })
            .collect();

        prop_assert_eq!(
            served,
            expected,
            "an address resolves to the top of its holder stack, and to nothing else"
        );
    }
}

// -------------------------------------------------------------- azure casing

// Resource Graph does not guarantee that a resource's own top-level `id` and
// the same id embedded inside another resource's `properties` share one
// casing. A VM names its NIC, and the NIC's `properties` name a public IP and
// a subnet — every one of those is a place two spellings of the same
// resource could disagree, so the projector has to fold each to one form
// before it becomes a node.
#[derive(Debug, Clone, Copy)]
enum Casing {
    AsGiven,
    Lower,
    Upper,
}

fn casing() -> impl Strategy<Value = Casing> {
    prop_oneof![
        Just(Casing::AsGiven),
        Just(Casing::Lower),
        Just(Casing::Upper),
    ]
}

fn recase(id: &str, casing: Casing) -> String {
    match casing {
        Casing::AsGiven => id.to_owned(),
        Casing::Lower => id.to_ascii_lowercase(),
        Casing::Upper => id.to_ascii_uppercase(),
    }
}

fn azure_resource_json(id: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut value = serde_json::json!({ "id": id, "name": "r" });
    for (key, v) in extra.as_object().into_iter().flatten() {
        value[key] = v.clone();
    }
    value
}

proptest! {
    #[test]
    fn a_networking_resource_is_one_node_whatever_casing_names_it(
        vm_self in casing(), vm_ref in casing(),
        nic_self in casing(), pip_ref in casing(), subnet_ref in casing(),
        vnet_self in casing(), subnet_self in casing(), vnet_ref in casing(),
        nsg_self in casing(), nsg_ref in casing(),
        pip_self in casing(),
    ) {
        use atlas_lib::atlas::projector::azure::azure_projector;

        const VM: &str = "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Compute/virtualMachines/vm1";
        const NIC: &str = "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Network/networkInterfaces/nic1";
        const VNET: &str = "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet1";
        const SUBNET: &str = "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet1/subnets/default";
        const NSG: &str = "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Network/networkSecurityGroups/nsg1";
        const PIP: &str = "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Network/publicIPAddresses/pip1";

        let raw: Vec<serde_json::Value> = vec![
            azure_resource_json(
                &recase(VM, vm_self),
                serde_json::json!({
                    "type": "microsoft.compute/virtualmachines",
                    "properties": {
                        "networkProfile": {
                            "networkInterfaces": [{"id": recase(NIC, vm_ref)}]
                        }
                    }
                }),
            ),
            azure_resource_json(
                &recase(NIC, nic_self),
                serde_json::json!({
                    "type": "microsoft.network/networkinterfaces",
                    "properties": {
                        "ipConfigurations": [{
                            "properties": {
                                "publicIPAddress": {"id": recase(PIP, pip_ref)},
                                "subnet": {"id": recase(SUBNET, subnet_ref)}
                            }
                        }]
                    }
                }),
            ),
            azure_resource_json(
                &recase(VNET, vnet_self),
                serde_json::json!({
                    "type": "microsoft.network/virtualnetworks",
                    "properties": {
                        "subnets": [{
                            "id": recase(SUBNET, subnet_self),
                            "properties": {
                                "networkSecurityGroup": {"id": recase(NSG, nsg_ref)}
                            }
                        }]
                    }
                }),
            ),
            azure_resource_json(
                &recase(NSG, nsg_self),
                serde_json::json!({ "type": "microsoft.network/networksecuritygroups" }),
            ),
            azure_resource_json(
                &recase(PIP, pip_self),
                serde_json::json!({ "type": "microsoft.network/publicipaddresses" }),
            ),
        ];
        let _ = vnet_ref; // the vnet's own subnets list carries the ref; nothing else names the vnet by id.

        let (collections, report) = atlas_lib::cloud::azure::provider::map_resources(raw);
        prop_assert!(report.is_complete(), "every row here is well-formed");

        let mut builder = GraphBuilder::new();
        azure_projector(&mut builder, &collections);

        let count_of = |predicate: &dyn Fn(&Node) -> bool| {
            builder.graph.node_weights().filter(|n| predicate(n)).count()
        };
        prop_assert_eq!(count_of(&|n| matches!(n, Node::AzureVirtualMachine(_))), 1);
        prop_assert_eq!(count_of(&|n| matches!(n, Node::AzureNetworkInterface(_))), 1);
        prop_assert_eq!(count_of(&|n| matches!(n, Node::AzureVirtualNetwork(_))), 1);
        prop_assert_eq!(count_of(&|n| matches!(n, Node::AzureSubnet(_))), 1);
        prop_assert_eq!(count_of(&|n| matches!(n, Node::AzureNetworkSecurityGroup(_))), 1);
        prop_assert_eq!(count_of(&|n| matches!(n, Node::AzurePublicIpAddress(_))), 1);

        let vm = Node::AzureVirtualMachine(VM.to_ascii_lowercase().into());
        let nic = Node::AzureNetworkInterface(NIC.to_ascii_lowercase().into());
        let vnet = Node::AzureVirtualNetwork(VNET.to_ascii_lowercase().into());
        let subnet = Node::AzureSubnet(SUBNET.to_ascii_lowercase().into());
        let nsg = Node::AzureNetworkSecurityGroup(NSG.to_ascii_lowercase().into());
        let pip = Node::AzurePublicIpAddress(PIP.to_ascii_lowercase().into());

        prop_assert!(builder.has_edge(&vm, &nic, &Edge::HasIp));
        prop_assert!(builder.has_edge(&nic, &pip, &Edge::HasIp));
        prop_assert!(builder.has_edge(&nic, &subnet, &Edge::AttachedTo));
        prop_assert!(builder.has_edge(&vnet, &subnet, &Edge::Contains));
        prop_assert!(builder.has_edge(&subnet, &nsg, &Edge::ConnectsTo));
    }
}
