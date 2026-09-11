use crate::atlas::collection::CollectionSource;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::export::{RenderObservation, edge_key, node_key};
use crate::atlas::graph_builder::GraphBuilder;
use petgraph::graph::Graph;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowAction {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct FlowObservation {
    pub source: CollectionSource,
    pub scope: String,
    pub src: Node,
    pub dst: Node,
    pub resources: Vec<Node>,
    pub packets: u64,
    pub bytes: u64,
    pub action: Option<FlowAction>,
    pub observed_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlowStats {
    pub last_seen: i64,
    pub packets: u64,
    pub bytes: u64,
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

fn status_of(accepted: u64, rejected: u64) -> &'static str {
    match (accepted > 0, rejected > 0) {
        (true, true) => "mixed",
        (false, true) => "rejected",
        (true, false) => "accepted",
        (false, false) => "observed",
    }
}

pub struct FlowIndex {
    ttl_ms: i64,
    capacity: usize,
    flows: HashMap<(Node, Node), FlowStats>,
    resources: HashMap<Node, Liveness>,
    dirty_flows: HashSet<(Node, Node)>,
    dirty_resources: HashSet<Node>,
    lapsed: Vec<String>,
}

impl Default for FlowIndex {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TTL, Self::DEFAULT_CAPACITY)
    }
}

impl FlowIndex {
    pub const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);

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

    pub fn overlay(&self, builder: &mut GraphBuilder) {
        let mut pairs: Vec<&(Node, Node)> = self.flows.keys().collect();

        pairs.sort_by_cached_key(|pair| flow_key(pair));
        let context = subgraph(builder, pairs.into_iter().map(|(a, b)| (a, b)));
        builder.merge(&context);
    }

    pub fn context(
        &self,
        live: &GraphBuilder,
        observations: &[FlowObservation],
    ) -> Graph<Node, Edge> {
        let retained: HashSet<(&Node, &Node)> = self.flows.keys().map(|(a, b)| (a, b)).collect();
        subgraph(
            live,
            observations
                .iter()
                .map(|o| (&o.src, &o.dst))
                .filter(|pair| retained.contains(pair)),
        )
    }

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

    pub fn drain_lapsed(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lapsed)
    }

    fn evict(&mut self) {
        let low_water = self.capacity - self.capacity / 4;
        if self.flows.len() > self.capacity {
            let (cutoff, mut ties) =
                eviction_cut(self.flows.values().map(|s| s.last_seen), low_water);
            let lapsed = &mut self.lapsed;
            let dirty = &mut self.dirty_flows;
            self.flows.retain(|pair, stats| {
                let keep = survives(stats.last_seen, cutoff, &mut ties);
                if !keep {
                    lapsed.push(flow_key(pair));
                    dirty.remove(pair);
                }
                keep
            });
        }
        if self.resources.len() > self.capacity {
            let (cutoff, mut ties) =
                eviction_cut(self.resources.values().map(|s| s.last_seen), low_water);
            let lapsed = &mut self.lapsed;
            let dirty = &mut self.dirty_resources;
            self.resources.retain(|node, stats| {
                let keep = survives(stats.last_seen, cutoff, &mut ties);
                if !keep {
                    lapsed.push(node_key(node));
                    dirty.remove(node);
                }
                keep
            });
        }
    }
}

fn eviction_cut(last_seen: impl Iterator<Item = i64> + Clone, keep: usize) -> (i64, usize) {
    let cutoff = nth_oldest(last_seen.clone(), keep);
    let newer = last_seen.filter(|seen| *seen > cutoff).count();
    (cutoff, keep.saturating_sub(newer))
}

fn survives(last_seen: i64, cutoff: i64, ties: &mut usize) -> bool {
    if last_seen > cutoff {
        return true;
    }
    if last_seen == cutoff && *ties > 0 {
        *ties -= 1;
        return true;
    }
    false
}

fn nth_oldest(last_seen: impl Iterator<Item = i64>, keep: usize) -> i64 {
    if keep == 0 {
        return i64::MAX;
    }
    let mut times: Vec<i64> = last_seen.collect();
    times.sort_unstable();
    times[times.len() - keep]
}

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

    #[test]
    fn an_unknown_remote_address_becomes_a_generic_pivot_node() {
        let mut index = FlowIndex::default();
        index.observe(&flow("10.0.0.1", "203.0.113.7", T0));

        let mut graph = estate();
        index.overlay(&mut graph);

        assert!(graph.contains(&ip("203.0.113.7")));
    }

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

    #[test]
    fn a_zero_budget_keeps_nothing() {
        let mut index = FlowIndex::new(Duration::from_secs(600), 0);

        index.observe(&flow("10.0.0.1", "198.51.100.10", T0));

        assert_eq!(index.flow_count(), 0);
        assert_eq!(index.resource_count(), 0);
    }

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

    #[test]
    fn eviction_survives_a_batch_that_shares_one_timestamp() {
        let capacity = 100;
        let mut index = FlowIndex::new(Duration::from_secs(3600), capacity);

        for i in 0..=capacity as i64 {
            index.observe(&flow("10.0.0.1", &format!("203.0.113.{i}"), T0));
        }

        let low_water = capacity - capacity / 4;
        assert_eq!(
            index.flow_count(),
            low_water,
            "an all-ties batch should trim to the low-water mark, not empty the index"
        );
    }

    #[test]
    fn the_merge_context_never_exceeds_what_the_index_kept() {
        let capacity = 20;
        let mut index = FlowIndex::new(Duration::from_secs(3600), capacity);
        let observations: Vec<FlowObservation> = (0..capacity as i64 * 5)
            .map(|i| flow("10.0.0.1", &format!("203.0.113.{i}"), T0 + i))
            .collect();
        for observation in &observations {
            index.observe(observation);
        }

        let context = index.context(&estate(), &observations);
        let flow_edges = context
            .edge_weights()
            .filter(|e| **e == Edge::TrafficFlow)
            .count();
        assert_eq!(flow_edges, index.flow_count());
        assert!(flow_edges <= capacity);
    }

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

    #[test]
    fn a_batch_context_admits_the_same_endpoints_the_overlay_does() {
        let live = estate();
        let mut typed = flow("10.0.0.1", "203.0.113.7", T0);
        typed.dst = Node::AwsEc2Eni("i-nowhere".into());

        let mut index = FlowIndex::default();
        let observations = [flow("10.0.0.1", "198.51.100.10", T0), typed];
        for observation in &observations {
            index.observe(observation);
        }
        let context = index.context(&live, &observations);

        assert_eq!(context.edge_count(), 1, "only the admissible flow crosses");
    }
}
