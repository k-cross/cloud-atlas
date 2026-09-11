use crate::demo;
use crate::state::AppState;
use crate::stream;
use atlas_lib::atlas::collection::{CollectionReport, CollectionSource, FailureKind};
use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::engine::AtlasEngine;
use atlas_lib::atlas::event::{ChangeEvent, EventApplier};
use atlas_lib::atlas::flow::{FlowIndex, FlowObservation};
use atlas_lib::atlas::graph_builder::GraphBuilder;
use atlas_lib::atlas::patch::{GraphPatch, Retention, carry_forward, diff, merge_additions};
use petgraph::graph::Graph;
use std::collections::HashSet;
use std::time::Duration;

pub enum Source {
    Live(Box<AtlasEngine>),
    Demo,
}

impl Source {
    async fn scan(&self, tick: u64) -> (GraphBuilder, CollectionReport) {
        match self {
            Source::Live(engine) => {
                let scan = engine.collect().await;
                (scan.builder, scan.report)
            }
            Source::Demo => (demo::graph(tick), CollectionReport::default()),
        }
    }

    fn observations(&self, tick: u64) -> Vec<FlowObservation> {
        match self {
            Source::Live(_) => Vec::new(),
            Source::Demo => demo::observations(tick),
        }
    }

    pub fn seed_flows(&self, flows: &mut FlowIndex) {
        for observation in self.observations(0) {
            flows.observe(&observation);
        }
    }

    pub fn default_flow_ttl(&self, poll: Duration) -> Duration {
        match self {
            Source::Live(_) => FlowIndex::DEFAULT_TTL,
            Source::Demo => demo::flow_ttl(poll),
        }
    }
}

fn reconcile(
    live: &Graph<Node, Edge>,
    next: &mut GraphBuilder,
    held: &HashSet<CollectionSource>,
    flows: &FlowIndex,
) -> GraphPatch {
    if !held.is_empty() {
        carry_forward(next, live, held);
    }
    flows.overlay(next);
    diff(live, &next.graph)
}

fn apply_events(
    live: &mut GraphBuilder,
    applier: &mut EventApplier,
    events: &[ChangeEvent],
) -> GraphPatch {
    let mut patch = GraphPatch::empty();
    for event in events {
        let change = applier.apply(live, event);
        if change.is_empty() {
            tracing::debug!(
                event = %event.id,
                op = %event.op,
                node = %event.node,
                "event told us nothing new",
            );
            continue;
        }
        tracing::info!(
            event = %event.id,
            op = %event.op,
            node = %event.node,
            scope = %event.scope,
            "applied change event",
        );
        patch.extend(change);
    }
    patch
}

fn publish(state: &AppState, patch: GraphPatch, tier: &'static str) {
    if patch.is_empty() {
        return;
    }
    tracing::info!(
        tier,
        added_nodes = patch.added_nodes.len(),
        removed_nodes = patch.removed_nodes.len(),
        added_edges = patch.added_edges.len(),
        removed_edges = patch.removed_edges.len(),
        observations = patch.observations.len(),
        expired = patch.expired.len(),
        "graph changed",
    );

    let _ = state.patches.send(patch);
}

pub async fn run(
    state: AppState,
    source: Source,
    events: stream::Source,
    flows: stream::FlowSource,
    interval: Duration,
    retention: Retention,
) {
    let mut retention = retention;
    let mut applier = EventApplier::new();
    let mut backoff = stream::Backoff::new();
    let mut flow_backoff = stream::Backoff::new();
    let mut tick: u64 = 0;

    if events.is_enabled() {
        tracing::info!("tier-1 event feed enabled; reconciling every {interval:?}");
    }
    if flows.is_enabled() {
        tracing::info!("tier-2 flow feed enabled");
    }

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;

    let mut drain = Box::pin(events.next(backoff.delay()));
    let mut flow_drain = Box::pin(flows.next(flow_backoff.delay()));

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                tick += 1;
                reconcile_tick(&state, &source, &mut retention, tick).await;
            }
            batch = &mut drain => {
                backoff.record(healthy(&batch.report));
                ingest(&state, &mut applier, batch).await;
                drain = Box::pin(events.next(backoff.delay()));
            }
            batch = &mut flow_drain => {
                flow_backoff.record(healthy(&batch.report));
                ingest_flows(&state, batch).await;
                flow_drain = Box::pin(flows.next(flow_backoff.delay()));
            }
        }
    }
}

fn healthy(report: &CollectionReport) -> bool {
    report.unreadable_sources().is_empty()
}

async fn reconcile_tick(state: &AppState, source: &Source, retention: &mut Retention, tick: u64) {
    let (mut next, report) = source.scan(tick).await;
    let held = retention.hold(&report);

    {
        let mut flows = state.flows.write().await;
        for observation in &source.observations(tick) {
            flows.observe(observation);
        }
        flows.expire(now_millis());
    }

    if !report.is_complete() {
        tracing::warn!(
            tick,
            failures = report.failures.len(),
            held = held.len(),
            "collection incomplete: {}",
            report.summary()
        );
        for released in report.unreadable_sources().difference(&held) {
            tracing::warn!(
                tick,
                source = %released,
                kind = %report.unreadable_kind(*released).unwrap_or(FailureKind::Unavailable),
                scans = retention.streak(*released),
                "source unreadable for too many consecutive scans; releasing its \
                 unconfirmed resources to the differ",
            );
        }
    }

    let mut patch = {
        let live = state.live.read().await;
        let flows = state.flows.read().await;
        reconcile(&live.graph, &mut next, &held, &flows)
    };

    *state.report.write().await = report;

    let topology_changed = !patch.is_empty();
    {
        let mut flows = state.flows.write().await;
        patch.observations = flows.drain_observations();
        patch.expired = flows.drain_lapsed();
    }

    if topology_changed {
        *state.live.write().await = next;
    }
    publish(state, patch, "reconcile");
}

async fn ingest_flows(state: &AppState, batch: stream::FlowBatch) {
    if !batch.report.is_complete() {
        tracing::warn!(
            failures = batch.report.failures.len(),
            "flow feed degraded: {}",
            batch.report.summary()
        );
    }

    *state.flow_report.write().await = batch.report;

    if batch.observations.is_empty() {
        return;
    }

    let patch = {
        let mut live = state.live.write().await;
        let mut flows = state.flows.write().await;
        for observation in &batch.observations {
            flows.observe(observation);
        }
        let context = flows.context(&live, &batch.observations);
        let mut patch = merge_additions(&mut live, &context);
        patch.observations = flows.drain_observations();

        patch.expired = flows.drain_lapsed();
        patch
    };
    publish(state, patch, "flow");
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

async fn ingest(state: &AppState, applier: &mut EventApplier, batch: stream::Batch) {
    if !batch.report.is_complete() {
        tracing::warn!(
            failures = batch.report.failures.len(),
            "event feed degraded: {}",
            batch.report.summary()
        );
    }

    *state.stream_report.write().await = batch.report;

    if batch.events.is_empty() {
        return;
    }

    let patch = {
        let mut live = state.live.write().await;
        apply_events(&mut live, applier, &batch.events)
    };
    publish(state, patch, "event");
}

#[cfg(test)]
mod tests {
    use super::*;

    use atlas_lib::atlas::collection::{CollectionReport, CollectionSource, FailureKind};
    use atlas_lib::atlas::event::ChangeOp;
    use atlas_lib::atlas::flow::FlowAction;

    const T0: i64 = 1_788_436_800_000;

    fn change(op: ChangeOp, node: Node, at: i64) -> ChangeEvent {
        ChangeEvent::new(CollectionSource::Aws, "us-east-1", "evt", at, op, node)
    }

    fn created(node: Node, at: i64) -> ChangeEvent {
        let mut context = GraphBuilder::new();
        context.get_or_add_node(node.clone());
        change(ChangeOp::Created, node, at).with_context(context.graph)
    }

    fn some_fixture_instance(graph: &Graph<Node, Edge>) -> Node {
        graph
            .node_weights()
            .find(|node| matches!(node, Node::AwsEc2Instance(_)))
            .expect("the fixtures contain an EC2 instance")
            .clone()
    }

    fn without_kind(graph: &Graph<Node, Edge>, kind: &str) -> GraphBuilder {
        let mut trimmed = graph.clone();
        trimmed.retain_nodes(|g, i| g[i].kind() != kind);
        let mut builder = GraphBuilder::new();
        builder.merge(&trimmed);
        builder
    }

    fn nothing_held() -> HashSet<CollectionSource> {
        HashSet::new()
    }

    fn no_flows() -> FlowIndex {
        FlowIndex::default()
    }

    fn holding_aws() -> HashSet<CollectionSource> {
        HashSet::from([CollectionSource::Aws])
    }

    fn aws_throttled() -> CollectionReport {
        let mut report = CollectionReport::default();
        report.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/ec2",
            "throttled",
        );
        report
    }

    fn aws_refused() -> CollectionReport {
        let mut report = CollectionReport::default();
        report.record(
            CollectionSource::Aws,
            FailureKind::Unauthorized,
            "us-east-1/credentials",
            "the security token included in the request is expired",
        );
        report
    }

    fn azure_drifted_row() -> CollectionReport {
        let mut report = CollectionReport::default();
        report.note(
            CollectionSource::Azure,
            FailureKind::Malformed,
            "resource_graph/row /subscriptions/s/vm-1",
            "unknown variant",
        );
        report
    }

    #[test]
    fn a_malformed_row_does_not_suspend_removals() {
        let live = demo::graph(1).graph;
        let report = azure_drifted_row();

        assert!(
            !report.is_complete(),
            "the row was still lost and must be reported"
        );
        assert!(
            report.unreadable_sources().is_empty(),
            "a row that would not map does not make the source unreadable"
        );

        let mut retention = Retention::default();
        let held = retention.hold(&report);
        assert!(held.is_empty(), "nothing to hold: Azure was read");

        let dropped = "AzureVirtualMachine";
        let mut next = without_kind(&live, dropped);
        assert!(
            next.graph.node_count() < live.node_count(),
            "fixture must contain the kind this test drops"
        );

        let patch = reconcile(&live, &mut next, &held, &no_flows());
        assert!(
            !patch.removed_nodes.is_empty(),
            "a scan that was read stays authoritative about what is gone"
        );
    }

    #[test]
    fn an_unauthorized_source_is_released_sooner_than_an_unavailable_one() {
        let budget = Retention::DEFAULT_BUDGET;
        assert!(Retention::AUTH_BUDGET < budget);

        let mut throttled = Retention::new(budget);
        for scan in 1..=budget {
            assert!(
                !throttled.hold(&aws_throttled()).is_empty(),
                "scan {scan} is within the outage budget"
            );
        }
        assert!(throttled.hold(&aws_throttled()).is_empty());

        let mut refused = Retention::new(budget);
        for scan in 1..=Retention::AUTH_BUDGET {
            assert!(
                !refused.hold(&aws_refused()).is_empty(),
                "scan {scan} is within the auth budget"
            );
        }
        assert!(
            refused.hold(&aws_refused()).is_empty(),
            "a credential failure must not pin the provider for the full outage budget"
        );
    }

    #[test]
    fn mixed_evidence_keeps_the_longer_budget() {
        let mut report = aws_refused();
        report.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-west-2/ec2",
            "throttled",
        );

        let mut retention = Retention::new(Retention::DEFAULT_BUDGET);
        for scan in 1..=Retention::AUTH_BUDGET + 1 {
            assert!(
                !retention.hold(&report).is_empty(),
                "scan {scan}: mixed evidence must not spend the short auth budget"
            );
        }
    }

    #[test]
    fn a_zero_budget_holds_nothing_regardless_of_kind() {
        let mut retention = Retention::new(0);
        assert!(retention.hold(&aws_refused()).is_empty());
        assert!(retention.hold(&aws_throttled()).is_empty());
    }

    #[test]
    fn an_incomplete_scan_never_removes() {
        let live = demo::graph(1).graph;
        let dropped = "AwsEc2Instance";

        let mut next = without_kind(&live, dropped);
        assert!(
            next.graph.node_count() < live.node_count(),
            "fixture must contain the kind this test drops"
        );

        let complete_patch = reconcile(
            &live,
            &mut without_kind(&live, dropped),
            &nothing_held(),
            &no_flows(),
        );
        assert!(
            !complete_patch.removed_nodes.is_empty(),
            "a complete scan is authoritative and must still delete"
        );

        let patch = reconcile(&live, &mut next, &holding_aws(), &no_flows());
        assert!(
            patch.removed_nodes.is_empty() && patch.removed_edges.is_empty(),
            "an incomplete scan deleted {} nodes / {} edges",
            patch.removed_nodes.len(),
            patch.removed_edges.len()
        );
        assert_eq!(
            next.graph.node_count(),
            live.node_count(),
            "unconfirmed resources must be carried into the installed graph"
        );
    }

    #[test]
    fn a_source_that_never_recovers_stops_blocking_removals() {
        let live = demo::graph(1).graph;
        let mut retention = Retention::new(2);

        for scan in 1..=2 {
            let held = retention.hold(&aws_throttled());
            let patch = reconcile(
                &live,
                &mut without_kind(&live, "AwsEc2Instance"),
                &held,
                &no_flows(),
            );
            assert!(
                patch.removed_nodes.is_empty(),
                "scan {scan} is still within budget and must not delete"
            );
        }

        let held = retention.hold(&aws_throttled());
        assert!(
            held.is_empty(),
            "the budget is spent, AWS is no longer held"
        );

        let patch = reconcile(
            &live,
            &mut without_kind(&live, "AwsEc2Instance"),
            &held,
            &no_flows(),
        );
        assert!(
            !patch.removed_nodes.is_empty(),
            "past its budget, an unreadable source must stop blocking removals"
        );
    }

    #[test]
    fn an_incomplete_scan_still_applies_additions() {
        let live = demo::graph(2).graph;
        let mut next = without_kind(&demo::graph(3).graph, "AwsEc2Instance");

        let patch = reconcile(&live, &mut next, &holding_aws(), &no_flows());

        assert_eq!(
            patch.added_nodes.len(),
            2,
            "sentinel additions from the sources that did respond must land"
        );
        assert_eq!(patch.added_edges.len(), 1);
        assert!(patch.removed_nodes.is_empty() && patch.removed_edges.is_empty());
    }

    #[test]
    fn a_complete_scan_is_unaffected_by_carry_forward() {
        let live = demo::graph(2).graph;
        let mut next = demo::graph(3);
        let same = next.graph.clone();

        let with_policy = reconcile(&live, &mut next, &nothing_held(), &no_flows());
        let plain = diff(&live, &same);

        assert_eq!(with_policy.added_nodes.len(), plain.added_nodes.len());
        assert_eq!(with_policy.removed_nodes.len(), plain.removed_nodes.len());
        assert_eq!(with_policy.added_edges.len(), plain.added_edges.len());
        assert_eq!(with_policy.removed_edges.len(), plain.removed_edges.len());
    }

    #[test]
    fn a_report_distinguishes_failure_from_absence() {
        let clean = CollectionReport::default();
        assert!(clean.is_complete());

        let mut partial = CollectionReport::default();
        partial.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/dynamodb",
            "throttled",
        );
        assert!(!partial.is_complete());
        assert!(partial.summary().contains("AWS"));
        assert!(partial.summary().contains("us-east-1/dynamodb"));
    }

    #[tokio::test]
    async fn a_burst_flow_arrives_then_lapses_back_out_of_the_graph() {
        let ttl = Duration::from_millis(50);
        let state = AppState::new(
            demo::graph(1),
            CollectionReport::default(),
            FlowIndex::new(ttl, FlowIndex::DEFAULT_CAPACITY),
        );
        let mut retention = Retention::new(Retention::DEFAULT_BUDGET);

        reconcile_tick(&state, &Source::Demo, &mut retention, 1).await;

        let steady: HashSet<Node> = demo::observations(2).into_iter().map(|o| o.src).collect();
        let burst = demo::observations(1)
            .into_iter()
            .map(|o| o.src)
            .find(|src| !steady.contains(src))
            .expect("the burst tick observes an endpoint the steady tick does not");
        assert!(
            state.live.read().await.node_map.contains_key(&burst),
            "the burst's endpoint should be pulled in by the overlay"
        );

        tokio::time::sleep(ttl * 2).await;
        reconcile_tick(&state, &Source::Demo, &mut retention, 2).await;
        assert!(
            !state.live.read().await.node_map.contains_key(&burst),
            "a lapsed flow should be removed by the ordinary differ"
        );
    }

    #[test]
    fn an_event_batch_changes_the_graph_and_reports_what_it_changed() {
        let mut live = demo::graph(2);
        let mut applier = EventApplier::new();
        let new_instance = Node::AwsEc2Instance("i-brand-new".into());

        let patch = apply_events(
            &mut live,
            &mut applier,
            &[created(new_instance.clone(), T0)],
        );

        assert_eq!(patch.added_nodes.len(), 1);
        assert!(live.contains(&new_instance));
    }

    #[test]
    fn a_burst_of_events_produces_a_single_patch() {
        let mut live = demo::graph(2);
        let mut applier = EventApplier::new();
        let events: Vec<_> = (0..5)
            .map(|i| created(Node::AwsEc2Instance(format!("i-burst-{i}").into()), T0 + i))
            .collect();

        let patch = apply_events(&mut live, &mut applier, &events);

        assert_eq!(patch.added_nodes.len(), 5);
    }

    #[test]
    fn events_never_garbage_collect() {
        let mut live = demo::graph(2);
        let before = live.graph.node_count();
        let mut applier = EventApplier::new();

        let patch = apply_events(
            &mut live,
            &mut applier,
            &[created(Node::AwsEc2Instance("i-brand-new".into()), T0)],
        );

        assert!(patch.removed_nodes.is_empty() && patch.removed_edges.is_empty());
        assert_eq!(live.graph.node_count(), before + 1);
    }

    #[test]
    fn a_deletion_event_removes_a_resource_the_scan_still_believes_in() {
        let mut live = demo::graph(2);
        let doomed = some_fixture_instance(&live.graph);
        let mut applier = EventApplier::new();

        let patch = apply_events(
            &mut live,
            &mut applier,
            &[change(ChangeOp::Deleted, doomed.clone(), T0)],
        );

        assert_eq!(patch.removed_nodes.len(), 1);
        assert!(!live.contains(&doomed));
    }

    fn observed(src: &str, dst: &str, at: i64) -> FlowObservation {
        FlowObservation {
            source: CollectionSource::Aws,
            scope: "us-east-1".to_owned(),
            src: Node::GenericIpAddress(src.into()),
            dst: Node::GenericIpAddress(dst.into()),
            resources: Vec::new(),
            packets: 12,
            bytes: 900,
            action: Some(FlowAction::Accepted),
            observed_at: at,
        }
    }

    fn state_with(graph: GraphBuilder) -> AppState {
        AppState::new(graph, CollectionReport::default(), FlowIndex::default())
    }

    #[tokio::test]
    async fn a_flow_batch_adds_traffic_without_waiting_for_a_scan() {
        let state = state_with(demo::graph(2));
        let mut patches = state.patches.subscribe();

        ingest_flows(
            &state,
            stream::FlowBatch {
                observations: vec![observed("10.10.1.10", "192.0.2.5", T0)],
                report: CollectionReport::default(),
            },
        )
        .await;

        let patch = patches.try_recv().expect("a patch was broadcast");
        assert!(patch.added_edges.iter().any(|e| e.kind == "TrafficFlow"));
        assert!(
            !patch.observations.is_empty(),
            "the freshness rides along with the edge"
        );
        assert!(state.live.read().await.has_edge(
            &Node::GenericIpAddress("10.10.1.10".into()),
            &Node::GenericIpAddress("192.0.2.5".into()),
            &Edge::TrafficFlow
        ));
    }

    #[tokio::test]
    async fn a_flow_batch_never_removes_anything() {
        let state = state_with(demo::graph(2));
        let before = state.live.read().await.graph.node_count();
        let mut patches = state.patches.subscribe();

        ingest_flows(
            &state,
            stream::FlowBatch {
                observations: vec![observed("10.10.1.10", "192.0.2.5", T0)],
                report: CollectionReport::default(),
            },
        )
        .await;

        let patch = patches.try_recv().expect("a patch was broadcast");
        assert!(patch.removed_nodes.is_empty() && patch.removed_edges.is_empty());
        assert!(state.live.read().await.graph.node_count() > before);
    }

    #[tokio::test]
    async fn a_broken_flow_feed_is_reported_without_touching_scan_health() {
        let state = state_with(demo::graph(2));
        let mut broken = CollectionReport::default();
        broken.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/flow-logs",
            "bucket unreachable",
        );

        ingest_flows(
            &state,
            stream::FlowBatch {
                observations: Vec::new(),
                report: broken,
            },
        )
        .await;

        assert!(!state.flow_report.read().await.is_complete());
        assert!(
            state.report.read().await.is_complete(),
            "the scan read every provider end to end"
        );
        assert!(
            state
                .flow_report
                .read()
                .await
                .unreadable_sources()
                .contains(&CollectionSource::Aws),
            "the feed's own health still names what it could not read"
        );
    }

    #[test]
    fn a_malformed_message_does_not_back_the_feed_off() {
        let mut malformed = CollectionReport::default();
        malformed.note(
            CollectionSource::Aws,
            FailureKind::Malformed,
            "us-east-1/flow-logs",
            "Parquet-formatted flow logs are not supported",
        );

        assert!(!malformed.is_complete(), "something was still lost");
        assert!(healthy(&malformed), "but the feed itself was readable");

        let mut unreachable = CollectionReport::default();
        unreachable.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/flow-logs",
            "timed out",
        );
        assert!(!healthy(&unreachable), "this is what backoff is for");
    }

    #[tokio::test]
    async fn a_batch_that_evicts_announces_the_lapse_in_its_own_patch() {
        let state = AppState::new(
            demo::graph(2),
            CollectionReport::default(),
            FlowIndex::new(Duration::from_secs(3600), 4),
        );
        let mut patches = state.patches.subscribe();
        let observations = (0..12)
            .map(|i| observed("10.10.1.10", &format!("192.0.2.{i}"), T0 + i))
            .collect();

        ingest_flows(
            &state,
            stream::FlowBatch {
                observations,
                report: CollectionReport::default(),
            },
        )
        .await;

        let patch = patches.try_recv().expect("a patch was broadcast");
        assert!(
            !patch.expired.is_empty(),
            "the batch evicted flows and must say so in the same patch"
        );
    }

    #[test]
    fn a_reconciliation_does_not_wipe_traffic_the_scan_cannot_see() {
        let mut flows = FlowIndex::default();
        flows.observe(&observed("10.10.1.10", "192.0.2.5", T0));

        let mut live = demo::graph(2);
        flows.overlay(&mut live);
        let mut next = demo::graph(2);

        let patch = reconcile(&live.graph, &mut next, &nothing_held(), &flows);

        assert!(patch.is_empty(), "a settled tick must change nothing");
    }

    #[test]
    fn an_observation_that_lapses_is_removed_by_the_next_reconciliation() {
        let mut flows = FlowIndex::new(Duration::from_secs(600), 100);
        flows.observe(&observed("10.10.1.10", "192.0.2.5", T0));

        let mut live = demo::graph(2);
        flows.overlay(&mut live);
        flows.expire(T0 + 11 * 60 * 1000);

        let mut next = demo::graph(2);
        let patch = reconcile(&live.graph, &mut next, &nothing_held(), &flows);

        assert!(
            patch
                .removed_edges
                .iter()
                .any(|key| key.starts_with("TrafficFlow|")),
            "the lapsed flow's edge must go"
        );
    }

    #[test]
    fn an_incomplete_scan_does_not_hold_traffic_edges() {
        let mut flows = FlowIndex::new(Duration::from_secs(600), 100);
        flows.observe(&observed("10.10.1.10", "192.0.2.5", T0));

        let mut live = demo::graph(2);
        flows.overlay(&mut live);
        flows.expire(T0 + 11 * 60 * 1000);

        let mut next = demo::graph(2);
        let patch = reconcile(&live.graph, &mut next, &holding_aws(), &flows);

        assert!(
            patch
                .removed_edges
                .iter()
                .any(|key| key.starts_with("TrafficFlow|")),
            "an AWS outage says nothing about whether traffic is still flowing"
        );
    }

    #[test]
    fn a_reconciliation_scan_overrules_an_event_it_could_not_have_seen() {
        let mut live = demo::graph(2);
        let doomed = some_fixture_instance(&live.graph);
        let mut applier = EventApplier::new();
        apply_events(
            &mut live,
            &mut applier,
            &[change(ChangeOp::Deleted, doomed.clone(), T0)],
        );

        let mut next = demo::graph(2);
        let patch = reconcile(&live.graph, &mut next, &nothing_held(), &no_flows());

        assert!(
            patch
                .added_nodes
                .iter()
                .any(|n| n.key == atlas_lib::atlas::export::node_key(&doomed)),
            "the scan re-adds what its snapshot still contained"
        );
    }
}
