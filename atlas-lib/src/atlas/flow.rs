//! Observed traffic — Tier 2 of `docs/change_monitoring_design.md`.
//!
//! Tier 1 and Tier 3 both read a cloud's *control plane*: what exists and how
//! it is configured. Neither can say whether any of it is doing anything. Flow
//! logs are the data plane, and they answer exactly one question the control
//! plane cannot — "is this thing alive, and who is it talking to" — while being
//! useless for the questions the control plane answers well. They are sampled,
//! aggregated, and minutes late; a provisioned-but-silent resource never
//! appears in them at all, and absence is indistinguishable from idle. So this
//! tier is strictly an *overlay*: it decorates the graph the other two tiers
//! build, and it is never the reason a typed resource node exists or stops
//! existing.
//!
//! Three rules keep the overlay from fighting the tiers underneath it:
//!
//! 1. **Metrics live beside the graph, not inside it.** `Node` and `Edge` are
//!    the graph's identity types — `Hash + Eq`, deduplicated on insert, diffed
//!    by value. A packet counter inside [`Edge::TrafficFlow`] would make every
//!    metric update a *different* edge: duplicates past `add_edge`'s dedup, and
//!    a remove-then-add of the same `edge_key` in every reconciliation patch.
//!    So [`FlowIndex`] holds the numbers, keyed by the same stable
//!    `node_key`/`edge_key` the wire uses, and the graph holds only the
//!    payload-free fact that traffic was seen.
//! 2. **An observation may create only the nodes no provider owns.** A flow
//!    record carries an IP and an interface id, not a resource's type, tags,
//!    subnet or security groups — reconstructing a typed node from that would
//!    be building topology out of shadows, and the next full scan would
//!    disagree. The one exception is [`Node::GenericIpAddress`] and its
//!    siblings ([`Node::owner`] returns `None` for them): they are the
//!    cross-cloud pivots the projectors already emit, so a flow to an
//!    unrecognised address becomes a generic endpoint and nothing more. That is
//!    also what makes a flow between two clouds land as a real edge between
//!    their estates.
//! 3. **Expiry is what deletes, and Tier 3 is what applies it.** Each
//!    reconciliation folds the *current* overlay into the freshly scanned graph
//!    before diffing ([`FlowIndex::overlay`]), so a flow that has gone quiet
//!    simply stops being folded in and the ordinary differ removes its edge.
//!    Tier 2 never removes anything itself, exactly as Tier 1 never
//!    garbage-collects.

use crate::atlas::collection::CollectionSource;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::export::{RenderObservation, edge_key, node_key};
use crate::atlas::graph_builder::GraphBuilder;
use petgraph::graph::Graph;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Whether the observed traffic got through. A security group rule says traffic
/// *could* flow; this is the only thing in the graph that says whether it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowAction {
    Accepted,
    Rejected,
}

/// One normalized flow record, whatever cloud produced it — Tier 2's
/// counterpart to [`ChangeEvent`](crate::atlas::event::ChangeEvent).
///
/// `src`/`dst` are the graph nodes the traffic ran between, chosen by the
/// adapter under rule 2 above: in practice a pair of [`Node::GenericIpAddress`]
/// values, which is what lets an AWS instance's flow to an Azure public IP
/// stitch the two estates together through the pivot node they already share.
#[derive(Debug, Clone)]
pub struct FlowObservation {
    pub source: CollectionSource,
    /// Region/project the records were read from, for reporting — the same
    /// granularity a collection failure is attributed at.
    pub scope: String,
    pub src: Node,
    pub dst: Node,
    /// Typed resources the record named outright (an instance id in the record,
    /// not an IP we guessed from). These get freshness and nothing else — never
    /// a node, since the record does not carry enough to build one.
    pub resources: Vec<Node>,
    pub packets: u64,
    pub bytes: u64,
    /// `None` when the record could not say. Flow-log formats are chosen field
    /// by field, and one that omits the verdict still proves the traffic
    /// happened — which is the liveness signal. Recording it as accepted would
    /// be claiming something the record did not say.
    pub action: Option<FlowAction>,
    /// Epoch milliseconds at the *end* of the record's aggregation window —
    /// the latest moment this traffic is known to have been happening.
    pub observed_at: i64,
}

/// What the overlay knows about one observed *flow*. Volume is recorded here
/// and nowhere else, because a flow is the only thing it is well defined for:
/// one record describes traffic between one pair of endpoints, in one
/// direction.
///
/// Counters accumulate for as long as the entry stays alive and reset when it
/// lapses, so they read as "traffic during this run of activity" rather than
/// "since the process started".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowStats {
    pub last_seen: i64,
    pub packets: u64,
    pub bytes: u64,
    /// Records seen, by verdict — not packets. A single rejected probe against
    /// a busy accepted conversation should register as `mixed`, not vanish
    /// into the packet totals. A record that carried no verdict counts towards
    /// neither.
    pub accepted: u64,
    pub rejected: u64,
}

impl FlowStats {
    fn record(&mut self, observation: &FlowObservation) {
        self.last_seen = self.last_seen.max(observation.observed_at);
        self.packets = self.packets.saturating_add(observation.packets);
        self.bytes = self.bytes.saturating_add(observation.bytes);
        verdict(&mut self.accepted, &mut self.rejected, observation);
    }

    pub fn status(&self) -> &'static str {
        status_of(self.accepted, self.rejected)
    }

    fn render(&self, key: String) -> RenderObservation {
        RenderObservation {
            key,
            last_seen: self.last_seen,
            packets: Some(self.packets),
            bytes: Some(self.bytes),
            status: self.status(),
        }
    }
}

/// What the overlay knows about one *node*: that it was heard from, and whether
/// what it carried got through.
///
/// Deliberately no volume. One record names up to four nodes — both endpoints,
/// and the instance and interface it came from — so stamping its packet count
/// on each would record the same traffic four times, and summing `packets`
/// across a snapshot would report several times the real figure. Worse, the
/// number would not mean anything even alone: a node's "packets" is a sum over
/// every flow that touched it, inbound and outbound together. A node's
/// throughput is derived from its incident [`Edge::TrafficFlow`] edges, which
/// are the things that know a direction.
///
/// `last_seen` survives the same treatment because it composes as a *maximum*
/// rather than a sum: recording it against every node a record names is
/// idempotent, and it is the signal the health overlay actually wants.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Liveness {
    pub last_seen: i64,
    pub accepted: u64,
    pub rejected: u64,
}

impl Liveness {
    fn record(&mut self, observation: &FlowObservation) {
        self.last_seen = self.last_seen.max(observation.observed_at);
        verdict(&mut self.accepted, &mut self.rejected, observation);
    }

    pub fn status(&self) -> &'static str {
        status_of(self.accepted, self.rejected)
    }

    fn render(&self, key: String) -> RenderObservation {
        RenderObservation {
            key,
            last_seen: self.last_seen,
            packets: None,
            bytes: None,
            status: self.status(),
        }
    }
}

fn verdict(accepted: &mut u64, rejected: &mut u64, observation: &FlowObservation) {
    match observation.action {
        Some(FlowAction::Accepted) => *accepted = accepted.saturating_add(1),
        Some(FlowAction::Rejected) => *rejected = rejected.saturating_add(1),
        None => {}
    }
}

/// `accepted`, `rejected`, `mixed`, or `observed` when every record that
/// contributed declined to say.
fn status_of(accepted: u64, rejected: u64) -> &'static str {
    match (accepted > 0, rejected > 0) {
        (true, true) => "mixed",
        (false, true) => "rejected",
        (true, false) => "accepted",
        (false, false) => "observed",
    }
}

/// The live overlay: which pairs of endpoints have been seen talking, and how
/// recently each node was heard from.
///
/// Bounded and expiring, because this is a daemon and flow logs are the highest
/// -volume feed of the three. A busy VPC talks to a practically unlimited
/// number of external addresses, so an unbounded index would grow for the
/// lifetime of the process and drag a `GenericIpAddress` node into the graph
/// for every one of them.
pub struct FlowIndex {
    ttl_ms: i64,
    capacity: usize,
    flows: HashMap<(Node, Node), FlowStats>,
    resources: HashMap<Node, Liveness>,
    /// Entries touched since the last drain, so a patch carries the freshness
    /// that actually changed instead of re-sending the whole overlay every
    /// tick.
    dirty_flows: HashSet<(Node, Node)>,
    dirty_resources: HashSet<Node>,
    /// Keys that have lapsed (expired or been evicted) and are owed to clients,
    /// which would otherwise keep showing a resource as live forever.
    lapsed: Vec<String>,
}

impl Default for FlowIndex {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TTL, Self::DEFAULT_CAPACITY)
    }
}

impl FlowIndex {
    /// How long an observation counts as current. Generous relative to the
    /// feed's own latency on purpose: AWS aggregates flows over one- to
    /// ten-minute windows and then takes minutes more to deliver them, so a
    /// shorter window would mark healthy resources dark purely because of
    /// pipeline lag.
    pub const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);

    /// Flows and resources tracked before the oldest are dropped. Each tracked
    /// flow can pull a `GenericIpAddress` node into the graph, so this is also
    /// the ceiling on how far the overlay can inflate the twin.
    pub const DEFAULT_CAPACITY: usize = 10_000;

    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl_ms: ttl.as_millis().min(i64::MAX as u128) as i64,
            capacity,
            flows: HashMap::new(),
            resources: HashMap::new(),
            dirty_flows: HashSet::new(),
            dirty_resources: HashSet::new(),
            lapsed: Vec::new(),
        }
    }

    pub fn flow_count(&self) -> usize {
        self.flows.len()
    }

    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    /// Fold one record in. Both endpoints get freshness as well as the flow
    /// itself: "this address was talking" is the liveness signal, and the
    /// resource holding that address is one `ConnectsTo` edge away in the graph
    /// the projectors already built.
    pub fn observe(&mut self, observation: &FlowObservation) {
        let pair = (observation.src.clone(), observation.dst.clone());
        self.flows
            .entry(pair.clone())
            .or_default()
            .record(observation);
        self.dirty_flows.insert(pair);

        for node in [&observation.src, &observation.dst]
            .into_iter()
            .chain(observation.resources.iter())
        {
            self.resources
                .entry(node.clone())
                .or_default()
                .record(observation);
            self.dirty_resources.insert(node.clone());
        }

        self.evict();
    }

    /// Drop everything not heard from within the TTL, so the next
    /// reconciliation stops folding those flows in and the differ removes their
    /// edges. `now_ms` is passed rather than read from the clock so the policy
    /// is testable and so a caller can drive it from one consistent instant.
    pub fn expire(&mut self, now_ms: i64) {
        let horizon = now_ms.saturating_sub(self.ttl_ms);
        let mut lapsed = Vec::new();
        let mut dirty_flows = std::mem::take(&mut self.dirty_flows);
        let mut dirty_resources = std::mem::take(&mut self.dirty_resources);

        self.flows.retain(|pair, stats| {
            let alive = stats.last_seen >= horizon;
            if !alive {
                lapsed.push(flow_key(pair));
                dirty_flows.remove(pair);
            }
            alive
        });
        self.resources.retain(|node, stats| {
            let alive = stats.last_seen >= horizon;
            if !alive {
                lapsed.push(node_key(node));
                dirty_resources.remove(node);
            }
            alive
        });

        self.dirty_flows = dirty_flows;
        self.dirty_resources = dirty_resources;
        self.lapsed.extend(lapsed);
    }

    /// Fold the whole overlay into a graph. Called on the scan graph before it
    /// is diffed, which is what makes the overlay survive a full rebuild and,
    /// on expiry, disappear through the ordinary differ instead of a special
    /// removal path.
    pub fn overlay(&self, builder: &mut GraphBuilder) {
        let mut pairs: Vec<&(Node, Node)> = self.flows.keys().collect();
        // Merge order decides node indices, and the snapshot is compared
        // byte-for-byte by the frontend's tests; keep it deterministic.
        pairs.sort_by_cached_key(|pair| flow_key(pair));
        let context = subgraph(builder, pairs.into_iter().map(|(a, b)| (a, b)));
        builder.merge(&context);
    }

    /// The flow edges from `observations` that `live` can accept, as a subgraph
    /// ready to be merged. This is the between-scans path: a batch arrives, its
    /// edges go straight into the live graph, and clients see the traffic
    /// without waiting for the next reconciliation.
    pub fn context(live: &GraphBuilder, observations: &[FlowObservation]) -> Graph<Node, Edge> {
        subgraph(live, observations.iter().map(|o| (&o.src, &o.dst)))
    }

    /// Everything currently observed, for a full snapshot.
    pub fn observations(&self) -> Vec<RenderObservation> {
        let mut all: Vec<RenderObservation> = self
            .flows
            .iter()
            .map(|(pair, stats)| stats.render(flow_key(pair)))
            .chain(
                self.resources
                    .iter()
                    .map(|(node, stats)| stats.render(node_key(node))),
            )
            .collect();
        all.sort_by(|a, b| a.key.cmp(&b.key));
        all
    }

    /// Only what changed since the last drain — what a patch should carry.
    pub fn drain_observations(&mut self) -> Vec<RenderObservation> {
        let mut changed: Vec<RenderObservation> = self
            .dirty_flows
            .drain()
            .filter_map(|pair| {
                self.flows
                    .get(&pair)
                    .map(|stats| stats.render(flow_key(&pair)))
            })
            .chain(self.dirty_resources.drain().filter_map(|node| {
                self.resources
                    .get(&node)
                    .map(|stats| stats.render(node_key(&node)))
            }))
            .collect();
        changed.sort_by(|a, b| a.key.cmp(&b.key));
        changed
    }

    /// Keys whose observation has lapsed and which clients should stop
    /// treating as live.
    pub fn drain_lapsed(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lapsed)
    }

    /// Hold the index to its budget by dropping the least recently observed.
    /// Age alone cannot do it: a scan of a /16 can add tens of thousands of
    /// endpoints inside one TTL window, and the point of the budget is that the
    /// overlay's contribution to the graph has a ceiling regardless.
    ///
    /// Trimming to a *low-water mark* rather than to the budget is what keeps
    /// this off the hot path. Cutting back to exactly `capacity` leaves the
    /// index one observation below the threshold, so it crosses again almost
    /// immediately and every subsequent record pays for a full sort and a full
    /// `retain` — twice over, once per map — while `ingest_flows` holds both
    /// the graph and the index write locks. Dropping a quarter at a time makes
    /// that cost amortized instead of per-record. Same reasoning, same shape,
    /// as `EventApplier::prune`.
    fn evict(&mut self) {
        let low_water = self.capacity - self.capacity / 4;
        if self.flows.len() > self.capacity {
            let cutoff = nth_oldest(self.flows.values().map(|s| s.last_seen), low_water);
            let lapsed = &mut self.lapsed;
            let dirty = &mut self.dirty_flows;
            self.flows.retain(|pair, stats| {
                let keep = stats.last_seen > cutoff;
                if !keep {
                    lapsed.push(flow_key(pair));
                    dirty.remove(pair);
                }
                keep
            });
        }
        if self.resources.len() > self.capacity {
            let cutoff = nth_oldest(self.resources.values().map(|s| s.last_seen), low_water);
            let lapsed = &mut self.lapsed;
            let dirty = &mut self.dirty_resources;
            self.resources.retain(|node, stats| {
                let keep = stats.last_seen > cutoff;
                if !keep {
                    lapsed.push(node_key(node));
                    dirty.remove(node);
                }
                keep
            });
        }
    }
}

/// The `last_seen` below which entries are dropped to bring a map back to
/// `keep` entries. Ties are dropped together, so this can undershoot the
/// target — which is the safe direction, and keeps eviction from re-firing on
/// the very next observation.
///
/// A budget of zero keeps nothing, which is how an operator turns the overlay
/// off outright.
fn nth_oldest(last_seen: impl Iterator<Item = i64>, keep: usize) -> i64 {
    if keep == 0 {
        return i64::MAX;
    }
    let mut times: Vec<i64> = last_seen.collect();
    times.sort_unstable();
    times[times.len() - keep]
}

/// Build the [`Edge::TrafficFlow`] edges for `pairs` as a standalone graph,
/// admitting only endpoints `target` may legitimately gain.
///
/// This is rule 2 in code. An endpoint already in the graph is decorated; an
/// endpoint no provider owns (the `GenericIpAddress`/`GenericHostname`/
/// `ExternalService` pivots) is created, because that is the shape a projector
/// would give an address it did not recognise either. A *typed* resource the
/// scan has not found is skipped outright rather than invented from an IP.
fn subgraph<'a>(
    target: &GraphBuilder,
    pairs: impl Iterator<Item = (&'a Node, &'a Node)>,
) -> Graph<Node, Edge> {
    let mut context = GraphBuilder::new();
    for (src, dst) in pairs {
        if !admissible(target, src) || !admissible(target, dst) {
            continue;
        }
        let a = context.get_or_add_ref(src);
        context.link_to(a, dst.clone(), Edge::TrafficFlow);
    }
    context.graph
}

fn admissible(target: &GraphBuilder, node: &Node) -> bool {
    node.owner().is_none() || target.contains(node)
}

fn flow_key(pair: &(Node, Node)) -> String {
    edge_key(&node_key(&pair.0), &node_key(&pair.1), &Edge::TrafficFlow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::patch::diff;

    const T0: i64 = 1_788_436_800_000;
    const MINUTE: i64 = 60 * 1_000;

    fn ip(addr: &str) -> Node {
        Node::GenericIpAddress(addr.into())
    }

    fn flow(src: &str, dst: &str, at: i64) -> FlowObservation {
        FlowObservation {
            source: CollectionSource::Aws,
            scope: "us-east-1".to_owned(),
            src: ip(src),
            dst: ip(dst),
            resources: Vec::new(),
            packets: 10,
            bytes: 840,
            action: Some(FlowAction::Accepted),
            observed_at: at,
        }
    }

    /// A graph shaped like the projector's: an instance holding a private IP.
    fn estate() -> GraphBuilder {
        let mut builder = GraphBuilder::new();
        let instance = builder.get_or_add_node(Node::AwsEc2Instance("i-1".into()));
        builder.link_to(instance, ip("10.0.0.1"), Edge::ConnectsTo);
        builder
    }

    #[test]
    fn an_observed_flow_becomes_an_edge_between_its_endpoints() {
        let mut index = FlowIndex::default();
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));

        let mut graph = estate();
        index.overlay(&mut graph);

        assert!(graph.has_edge(&ip("10.0.0.1"), &ip("198.51.100.10"), &Edge::TrafficFlow));
    }

    /// The cross-cloud payoff: the remote end of a flow is a pivot node any
    /// other provider may also reference, so an unrecognised address is worth
    /// creating even though no scan reported it.
    #[test]
    fn an_unknown_remote_address_becomes_a_generic_pivot_node() {
        let mut index = FlowIndex::default();
        index.observe(&flow("10.0.0.1", "203.0.113.7", T0));

        let mut graph = estate();
        index.overlay(&mut graph);

        assert!(graph.contains(&ip("203.0.113.7")));
    }

    /// Rule 2. A flow record carries an IP and an interface id — not a type,
    /// tags, subnet or security groups — so a typed node built from one would
    /// be topology reconstructed from shadows, and the next full scan would
    /// disagree with it.
    #[test]
    fn a_typed_resource_the_scan_never_found_is_not_invented() {
        let unknown = Node::AwsEc2Eni("i-nowhere".into());
        let mut index = FlowIndex::default();
        let mut observation = flow("10.0.0.1", "203.0.113.7", T0);
        observation.dst = unknown.clone();
        index.observe(&observation);

        let mut graph = estate();
        index.overlay(&mut graph);

        assert!(!graph.contains(&unknown));
        assert_eq!(
            graph
                .graph
                .edge_weights()
                .filter(|e| **e == Edge::TrafficFlow)
                .count(),
            0,
            "no half-edge either"
        );
    }

    /// A typed resource the scan *did* find is decorated, not skipped — that is
    /// the difference between inventing topology and annotating it.
    #[test]
    fn a_typed_resource_the_scan_found_can_carry_a_flow() {
        let known = Node::AwsEc2Instance("i-1".into());
        let mut index = FlowIndex::default();
        let mut observation = flow("203.0.113.7", "10.0.0.1", T0);
        observation.dst = known.clone();
        index.observe(&observation);

        let mut graph = estate();
        index.overlay(&mut graph);

        assert!(graph.has_edge(&ip("203.0.113.7"), &known, &Edge::TrafficFlow));
    }

    /// Rule 3: expiry deletes through the ordinary differ. A quiet flow simply
    /// stops being folded into the scan graph, and reconciliation removes it —
    /// Tier 2 never removes anything itself.
    #[test]
    fn a_flow_that_goes_quiet_is_removed_by_the_next_reconciliation() {
        let mut index = FlowIndex::new(Duration::from_secs(600), 100);
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));

        let mut live = estate();
        index.overlay(&mut live);
        assert_eq!(live.graph.edge_count(), 2);

        index.expire(T0 + 11 * MINUTE);
        let mut next = estate();
        index.overlay(&mut next);

        let patch = diff(&live.graph, &next.graph);
        assert_eq!(patch.removed_edges.len(), 1);
        assert_eq!(
            patch.removed_nodes.len(),
            1,
            "the pivot node the flow brought in goes with it"
        );
    }

    /// Freshness inside the window is not a graph change: re-observing must
    /// leave the topology alone, or every flow record would churn the layout.
    #[test]
    fn refreshing_a_live_flow_changes_no_topology() {
        let mut index = FlowIndex::default();
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));
        let mut live = estate();
        index.overlay(&mut live);

        index.observe(&flow("10.0.0.1", "198.51.100.10", T0 + MINUTE));
        let mut next = estate();
        index.overlay(&mut next);

        assert!(diff(&live.graph, &next.graph).is_empty());
    }

    #[test]
    fn observations_accumulate_and_carry_the_newest_sighting() {
        let mut index = FlowIndex::default();
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0 + MINUTE));

        let key = flow_key(&(ip("10.0.0.1"), ip("198.51.100.10")));
        let observed = index
            .observations()
            .into_iter()
            .find(|o| o.key == key)
            .expect("the flow is observed");

        assert_eq!(observed.last_seen, T0 + MINUTE);
        assert_eq!(observed.packets, Some(20));
        assert_eq!(observed.status, "accepted");
    }

    /// A rejected probe against an otherwise healthy conversation must stay
    /// visible; it is the more interesting half of the pair.
    #[test]
    fn a_rejected_record_alongside_an_accepted_one_reads_as_mixed() {
        let mut index = FlowIndex::default();
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));
        let mut rejected = flow("10.0.0.1", "198.51.100.10", T0 + 1);
        rejected.action = Some(FlowAction::Rejected);
        index.observe(&rejected);

        let key = flow_key(&(ip("10.0.0.1"), ip("198.51.100.10")));
        let observed = index
            .observations()
            .into_iter()
            .find(|o| o.key == key)
            .expect("the flow is observed");

        assert_eq!(observed.status, "mixed");
    }

    /// A format that leaves the verdict out still proves the traffic happened,
    /// and that is the whole liveness signal. Filing it as accepted would claim
    /// something the record never said.
    #[test]
    fn a_record_with_no_verdict_is_observed_not_accepted() {
        let mut index = FlowIndex::default();
        let mut silent = flow("10.0.0.1", "198.51.100.10", T0);
        silent.action = None;
        index.observe(&silent);

        let key = flow_key(&(ip("10.0.0.1"), ip("198.51.100.10")));
        let observed = index
            .observations()
            .into_iter()
            .find(|o| o.key == key)
            .expect("the flow is observed");

        assert_eq!(observed.status, "observed");
        assert_eq!(observed.last_seen, T0, "liveness is still recorded");
    }

    /// One record names up to four keys — both endpoints plus the instance and
    /// interface it came from — so recording its volume against each would
    /// report the same traffic four times over. Volume belongs to the flow; the
    /// nodes carry only what composes idempotently.
    #[test]
    fn volume_is_counted_once_across_the_whole_overlay() {
        let mut index = FlowIndex::default();
        let mut observation = flow("10.0.0.1", "198.51.100.10", T0);
        observation.resources = vec![
            Node::AwsEc2Instance("i-1".into()),
            Node::AwsEc2Eni("eni-1".into()),
        ];
        index.observe(&observation);

        let observations = index.observations();
        assert_eq!(observations.len(), 5, "one flow plus four named keys");
        let total: u64 = observations.iter().filter_map(|o| o.packets).sum();

        assert_eq!(total, 10, "the record's packets, counted exactly once");
    }

    /// And a node reports no volume at all rather than a misleading one: on a
    /// node it would be an undirected sum over every flow that touched it.
    #[test]
    fn a_node_carries_freshness_without_volume() {
        let mut index = FlowIndex::default();
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));

        let observed = index
            .observations()
            .into_iter()
            .find(|o| o.key == node_key(&ip("10.0.0.1")))
            .expect("the endpoint is observed");

        assert_eq!(observed.last_seen, T0, "freshness is the point");
        assert_eq!(observed.packets, None);
        assert_eq!(observed.bytes, None);
        assert_eq!(observed.status, "accepted");
    }

    /// The record names an instance outright, so its freshness is a fact rather
    /// than an inference — but it still buys no node.
    #[test]
    fn a_named_resource_gets_freshness_without_a_node() {
        let instance = Node::AwsEc2Instance("i-1".into());
        let mut index = FlowIndex::default();
        let mut observation = flow("10.0.0.1", "198.51.100.10", T0);
        observation.resources = vec![instance.clone()];
        index.observe(&observation);

        assert!(
            index
                .observations()
                .iter()
                .any(|o| o.key == node_key(&instance))
        );

        let mut graph = GraphBuilder::new();
        index.overlay(&mut graph);
        assert!(!graph.contains(&instance), "freshness is not existence");
    }

    /// A patch should carry the freshness that changed, not the whole overlay:
    /// on a busy estate the overlay is far larger than the delta, and it
    /// changes on every single tick.
    #[test]
    fn a_drain_reports_only_what_changed_since_the_last_one() {
        let mut index = FlowIndex::default();
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));
        assert!(!index.drain_observations().is_empty());
        assert!(index.drain_observations().is_empty());

        index.observe(&flow("10.0.0.2", "198.51.100.10", T0 + MINUTE));
        let changed = index.drain_observations();

        assert!(
            changed.iter().all(|o| !o.key.contains("10.0.0.1")),
            "an untouched flow must not be re-sent: {changed:?}"
        );
    }

    /// A client that never hears otherwise keeps showing a lapsed resource as
    /// live, so expiry has to be announced as well as applied.
    #[test]
    fn expiry_is_reported_to_clients() {
        let mut index = FlowIndex::new(Duration::from_secs(600), 100);
        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));
        index.drain_observations();

        index.expire(T0 + 11 * MINUTE);
        let lapsed = index.drain_lapsed();

        assert!(lapsed.contains(&flow_key(&(ip("10.0.0.1"), ip("198.51.100.10")))));
        assert!(lapsed.contains(&node_key(&ip("198.51.100.10"))));
        assert!(index.drain_lapsed().is_empty(), "owed only once");
    }

    /// This is a daemon and flow logs are the highest-volume feed: a busy VPC
    /// talks to an unbounded number of external addresses, and every tracked
    /// flow can drag a node into the graph.
    #[test]
    fn the_index_stays_within_its_budget() {
        let capacity = 50;
        let mut index = FlowIndex::new(Duration::from_secs(3600), capacity);

        for i in 0..capacity as i64 * 4 {
            index.observe(&flow("10.0.0.1", &format!("203.0.113.{i}"), T0 + i));
        }

        assert!(index.flow_count() <= capacity, "{}", index.flow_count());
        assert!(index.resource_count() <= capacity);
    }

    /// A zero budget turns the overlay off rather than panicking on the first
    /// record — the shape `--retain-scans 0` already has elsewhere.
    #[test]
    fn a_zero_budget_keeps_nothing() {
        let mut index = FlowIndex::new(Duration::from_secs(600), 0);

        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));

        assert_eq!(index.flow_count(), 0);
        assert_eq!(index.resource_count(), 0);
    }

    /// Trimming back to exactly the budget would leave the index one
    /// observation below the threshold, so it would cross again on the very
    /// next record and every record after that would pay for a full sort and a
    /// full retain — on the ingest hot path, holding two write locks.
    #[test]
    fn eviction_leaves_headroom_rather_than_refiring_per_record() {
        let capacity = 100;
        let mut index = FlowIndex::new(Duration::from_secs(3600), capacity);

        for i in 0..=capacity as i64 {
            index.observe(&flow("10.0.0.1", &format!("203.0.113.{i}"), T0 + i));
        }

        let low_water = capacity - capacity / 4;
        assert!(
            index.flow_count() <= low_water,
            "trimmed to {} of {capacity}, leaving no headroom",
            index.flow_count()
        );
    }

    /// Eviction drops the *oldest*, so the traffic still happening survives a
    /// flood of one-off destinations.
    #[test]
    fn eviction_keeps_the_most_recently_seen() {
        let capacity = 10;
        let mut index = FlowIndex::new(Duration::from_secs(3600), capacity);
        for i in 0..capacity as i64 * 3 {
            index.observe(&flow("10.0.0.1", &format!("203.0.113.{i}"), T0 + i));
        }

        let newest = flow_key(&(ip("10.0.0.1"), ip("203.0.113.29")));
        assert!(index.observations().iter().any(|o| o.key == newest));
    }

    /// The overlay is folded into a graph the frontend compares byte-for-byte;
    /// HashMap iteration order must not leak into it.
    #[test]
    fn the_overlay_is_deterministic() {
        let mut index = FlowIndex::default();
        for i in 0..20 {
            index.observe(&flow("10.0.0.1", &format!("203.0.113.{i}"), T0 + i));
        }

        let mut first = estate();
        index.overlay(&mut first);
        let mut second = estate();
        index.overlay(&mut second);

        let keys =
            |b: &GraphBuilder| -> Vec<String> { b.graph.node_weights().map(node_key).collect() };
        assert_eq!(keys(&first), keys(&second));
    }

    /// The between-scans path: a batch that arrives mid-interval must be
    /// applicable to the live graph directly, under the same admission rule.
    #[test]
    fn a_batch_context_admits_the_same_endpoints_the_overlay_does() {
        let live = estate();
        let mut typed = flow("10.0.0.1", "203.0.113.7", T0);
        typed.dst = Node::AwsEc2Eni("i-nowhere".into());

        let context = FlowIndex::context(&live, &[flow("10.0.0.1", "198.51.100.10", T0), typed]);

        assert_eq!(context.edge_count(), 1, "only the admissible flow crosses");
    }
}
