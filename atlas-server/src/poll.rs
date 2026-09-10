//! The graph's single writer, driving both live tiers.
//!
//! **Tier 3** is the reconciliation scan: periodically re-derive the whole
//! graph, diff it against the live one, and broadcast the change set. It is
//! authoritative — the only thing that can conclude a resource is gone because
//! nothing mentioned it.
//!
//! **Tier 1** is the event feed (`stream::Source`): normalized `ChangeEvent`s
//! applied the moment they arrive, seconds after the change instead of up to a
//! poll interval later. It only ever says what it was told, so it can add a
//! resource, and delete the one an event named, but never garbage-collect.
//!
//! **Tier 2** is the flow feed (`stream::FlowSource`): observed traffic, folded
//! into `AppState::flows` and laid over the graph as `Edge::TrafficFlow` plus
//! per-key freshness. It is the only tier that can say a resource is *doing*
//! something, and the only one that never decides anything exists. Its edges
//! are re-folded onto every scan before the diff, so an observation that lapses
//! is removed by the ordinary differ rather than by a special path — see
//! `atlas::flow`.
//!
//! All three run on *this* task, chosen by `select!`, which is what keeps
//! mutation serialized (the graph-actor intent of
//! `docs/change_monitoring_design.md` §7) without any tier taking a lock
//! another is waiting on.
//!
//! Tiers 1 and 3 do race, and the resolution is deliberate: a reconciliation scan
//! installs the estate as it looked when the scan *started*, so a resource
//! created by an event mid-scan is removed by that tick's diff and re-added by
//! the next one. Clients are never inconsistent — the patch always describes
//! the graph that was installed — only briefly behind. Fixing it properly means
//! replaying post-scan events over the scan result, which needs per-node
//! provenance the graph does not carry yet.

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

/// Where each reconciliation tick's graph comes from. Everything the
/// credential-free variant actually *does* lives in [`crate::demo`]; this enum
/// only chooses between the two, so no demo cadence, sentinel or timing leaks
/// into the reconciliation path or into the flags that configure a real run.
pub enum Source {
    /// Real collection from configured cloud providers.
    Live(Box<AtlasEngine>),
    /// Credential-free fixtures, for local development and demos.
    Demo,
}

impl Source {
    /// Produce the graph for tick `n`, together with what could not be read
    /// while producing it. The demo source is always complete -- it never
    /// leaves the process.
    async fn scan(&self, tick: u64) -> (GraphBuilder, CollectionReport) {
        match self {
            Source::Live(engine) => {
                let scan = engine.collect().await;
                (scan.builder, scan.report)
            }
            Source::Demo => (demo::graph(tick), CollectionReport::default()),
        }
    }

    /// Traffic this source observes without a feed. Only the demo has any: it
    /// re-observes the fixtures' flows every tick so a credential-free run
    /// carries live-looking liveness, exactly as the real path does off
    /// `stream::FlowSource`. A real source observes nothing here -- its traffic
    /// arrives on the Tier-2 feed or not at all.
    fn observations(&self, tick: u64) -> Vec<FlowObservation> {
        match self {
            Source::Live(_) => Vec::new(),
            Source::Demo => demo::observations(tick),
        }
    }

    /// Prime a fresh index with whatever this source already knows it has seen,
    /// so the very first snapshot carries the same overlay a reconciled one
    /// would. A real source knows nothing until its feed delivers, and seeds
    /// nothing.
    pub fn seed_flows(&self, flows: &mut FlowIndex) {
        for observation in self.observations(0) {
            flows.observe(&observation);
        }
    }

    /// How long an observation counts as current when the operator has not said.
    /// A real feed's default is generous relative to flow logs' own delivery lag
    /// (`FlowIndex::DEFAULT_TTL`); the demo answers for itself, since its
    /// traffic is synthesised on the reconciliation tick rather than delivered.
    pub fn default_flow_ttl(&self, poll: Duration) -> Duration {
        match self {
            Source::Live(_) => FlowIndex::DEFAULT_TTL,
            Source::Demo => demo::flow_ttl(poll),
        }
    }
}

/// Turn a scan into the patch to broadcast. A complete scan is authoritative
/// and diffs straight through, removals included. For each source in `held`,
/// that source's live resources are folded forward first, so the tick is
/// additive-only *for that source* while every other provider keeps deleting
/// normally -- `next` is left as the exact graph the caller should install.
///
/// `held` is what [`Retention`] decided, not simply what failed: a source that
/// has been unreadable for too long is no longer held, so the graph converges
/// instead of waiting forever on a collector that never recovers.
///
/// The flow overlay is folded on *after* carry-forward and before the diff, so
/// the scanned graph is compared against the live one with the same decoration
/// on both sides. That ordering is what makes Tier-2 expiry work through the
/// ordinary differ: a flow the overlay no longer believes in is simply absent
/// from `next`, and the diff removes its edge like any other.
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

/// Apply a batch of Tier-1 events to the live graph, folding what each one
/// changed into a single patch.
///
/// One patch rather than one per event because a burst — a deploy, an
/// autoscaling event — is one thing happening to the estate, and fanning it out
/// as a hundred frames makes every connected client re-layout a hundred times.
fn apply_events(
    live: &mut GraphBuilder,
    applier: &mut EventApplier,
    events: &[ChangeEvent],
) -> GraphPatch {
    let mut patch = GraphPatch::empty();
    for event in events {
        let change = applier.apply(live, event);
        if change.is_empty() {
            // Routine: a redelivery, or a create we already have. Worth seeing
            // at debug level, not worth a broadcast.
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

/// Broadcast a patch and log it. Returns nothing to do for an empty patch,
/// which is the common case on both tiers.
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
    // Err only means no subscribers are connected — nothing to do.
    let _ = state.patches.send(patch);
}

/// Run forever: reconcile every `interval`, and apply events from `events` as
/// they arrive. Only non-empty changes mutate the live graph or hit the
/// broadcast channel.
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

    // `interval` fires immediately on its first tick; the graph was just
    // collected at start-up, so skip that one and keep the original cadence of
    // "sleep, then scan".
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;

    // Pinned outside the loop, and deliberately not recreated per iteration.
    // A drain deletes the messages it processed before returning them, so a
    // receive dropped in that window loses those events for good — SQS has
    // already forgotten them. Holding one future across iterations means a
    // reconciliation tick winning the `select!` merely stops polling the drain;
    // it resumes untouched next time round.
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

/// Whether a feed is worth going straight back to, as opposed to backing off.
///
/// Deliberately *not* `is_complete()`, which is false for any failure at all
/// including [`FailureKind::Malformed`] — and a malformed message means the
/// feed was read perfectly well and one thing on it could not be understood.
/// Backing off there punishes a healthy queue for its contents: a poison
/// message, a partial `DeleteMessageBatch`, or — worst — a flow-log bucket
/// configured for Parquet, where *every* object is malformed and the feed
/// would sit pinned at the backoff ceiling while being entirely readable.
fn healthy(report: &CollectionReport) -> bool {
    report.unreadable_sources().is_empty()
}

/// One Tier-3 pass: scan, decide what is held, diff, install, broadcast.
async fn reconcile_tick(state: &AppState, source: &Source, retention: &mut Retention, tick: u64) {
    let (mut next, report) = source.scan(tick).await;
    let held = retention.hold(&report);

    // One instant for the whole tick, and the only place the overlay ages.
    // Expiry is deliberately driven from the reconciliation clock rather than
    // from the flow feed: a feed that has gone silent must still let its
    // observations lapse, or a dead flow pipeline would pin the last traffic it
    // ever saw as permanently current.
    {
        let mut flows = state.flows.write().await;
        for observation in &source.observations(tick) {
            flows.observe(observation);
        }
        flows.expire(now_millis());
    }

    if !report.is_complete() {
        // `held` rather than "holding": a scan can be incomplete without
        // anything being held, when every failure was a malformed record
        // in a response we did read.
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

    // Diff under the read lock — no full-graph clone, and the critical
    // section is just the comparison. WebSocket readers share the lock.
    let mut patch = {
        let live = state.live.read().await;
        let flows = state.flows.read().await;
        reconcile(&live.graph, &mut next, &held, &flows)
    };

    // Published even when the graph is unchanged: a provider going dark
    // changes what the snapshot *means* without changing a single node.
    *state.report.write().await = report;

    // Whether to install `next` is a question about topology alone. Freshness
    // rides on the same patch but changes nothing in the graph, so a tick that
    // only refreshed liveness must not churn the installed node indices.
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

/// One Tier-2 batch: record what was observed, add the traffic edges it implies,
/// and publish the feed's own health.
///
/// The edges go in immediately rather than waiting for the next reconciliation,
/// for the same reason Tier 1 does not wait: a flow that has already been
/// observed is not news a minute later. The next scan folds the same overlay in
/// and agrees.
async fn ingest_flows(state: &AppState, batch: stream::FlowBatch) {
    if !batch.report.is_complete() {
        tracing::warn!(
            failures = batch.report.failures.len(),
            "flow feed degraded: {}",
            batch.report.summary()
        );
    }
    // Third report, third question. A flow feed we cannot read leaves the
    // topology entirely correct and only the liveness stale, so it must not
    // suspend a removal any more than a dead event feed may.
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
        let context = FlowIndex::context(&live, &batch.observations);
        let mut patch = merge_additions(&mut live, &context);
        patch.observations = flows.drain_observations();
        // Drained together with the observations, never left for the next
        // reconciliation. `evict` retires keys as a side effect of `observe`,
        // and a lapse that outlives the patch it belongs to can be contradicted
        // before it is sent: re-observe the evicted key in a later batch and
        // the tick would announce an expiry for a flow that is live again,
        // darkening it on every client until it next happens to be seen.
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

/// One Tier-1 batch: apply, broadcast, and publish the feed's own health.
async fn ingest(state: &AppState, applier: &mut EventApplier, batch: stream::Batch) {
    if !batch.report.is_complete() {
        tracing::warn!(
            failures = batch.report.failures.len(),
            "event feed degraded: {}",
            batch.report.summary()
        );
    }
    // Kept apart from the scan report on purpose: a feed we cannot read makes
    // the graph slow, not wrong, and must not suspend Tier-3 removals.
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

    /// A reconciliation with no observed traffic to lay over it. The overlay's
    /// own behaviour is covered in `atlas::flow`; these tests are about the
    /// retention policy, which must be unaffected by it.
    fn no_flows() -> FlowIndex {
        FlowIndex::default()
    }

    /// AWS held back -- it owns the kind these tests drop.
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

    /// A row ARG returned that would not deserialize. The query itself
    /// succeeded, so Azure was read.
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

    /// The failure that is *not* a read failure. ARG answers for the entire
    /// tenant in one response, so treating an unmappable row as an unreadable
    /// source froze deletions across every Azure resource for the whole
    /// retention budget — because one resource drifted from its model.
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

    /// Waiting out a throttle is sensible; waiting out a rejected credential
    /// just claims resources nobody can verify, since polling will not fix it.
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

    /// Releasing early is only safe when the diagnosis is unambiguous. One
    /// region forbidden while another is merely throttled may still recover.
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

    /// `--retain-scans 0` means retain nothing. An auth failure is not an
    /// exception that quietly lengthens it.
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

    /// Retention protects against a *transient* failure. A collector that fails
    /// on every tick must not pin its resources in the graph forever, or
    /// deletions never converge for that provider.
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
        // Whichever endpoint the burst tick adds over a steady one -- the
        // address itself is the demo module's business, not this test's.
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

    /// The point of Tier 1: a change reaches the graph without waiting for a
    /// scan, and the patch says exactly what it did.
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

    /// A whole burst is one patch, not one per event: a deploy that touches
    /// fifty resources must not make every connected client re-layout fifty
    /// times.
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

    /// Tier 1 is not authoritative about absence. An event says what it was
    /// told and nothing more, so a batch must never remove a resource no event
    /// mentioned — that is Tier 3's job alone.
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

    /// A deletion event takes the resource out immediately, along with the
    /// edges that died with it — the latency win that justifies the tier.
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

    // ------------------------------------------------------------------
    // Tier 2
    // ------------------------------------------------------------------

    /// Traffic to somewhere the fixtures never mention, so the overlay's
    /// contribution is unambiguous.
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

    /// The latency win that justifies the tier: observed traffic reaches the
    /// graph — and a connected client — without waiting for a reconciliation.
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

    /// Tier 2 sees only what crossed the network, so it can never conclude
    /// anything is gone. Removal stays Tier 3's alone.
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

    /// A flow feed we cannot read makes liveness stale and nothing else. Rolling
    /// it into the scan report would make an unreachable bucket suspend
    /// deletions across all of AWS — the same mistake, one tier along.
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

    /// A message we could not parse means the queue was read perfectly well and
    /// one thing on it was not understood. Backing off there punishes a healthy
    /// feed for its contents — and a bucket configured for Parquet makes *every*
    /// object malformed, which would pin a fully readable feed at the ceiling
    /// for as long as the server runs.
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

    /// `evict` retires keys as a side effect of `observe`, so a lapse that
    /// outlived its patch could be contradicted before it was ever sent:
    /// re-observe the evicted key in a later batch and the next tick would
    /// announce an expiry for a flow that is live again, darkening it on every
    /// client until it happened to be seen once more.
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

    /// The scan graph is rebuilt from scratch every tick and knows nothing about
    /// traffic, so without the overlay being folded back on, every
    /// reconciliation would delete every flow edge and the next batch would put
    /// them back — flapping forever.
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

    /// And the other half: expiry works *through* the differ. Nothing in Tier 2
    /// removes anything itself; the overlay simply stops claiming the flow.
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

    /// Retention holds what an *unreadable provider* could not confirm. A
    /// traffic edge was never confirmed by a provider in the first place, so
    /// holding it would suspend the overlay's own expiry for as long as any
    /// collector is unhealthy — liveness frozen by an unrelated outage.
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

    /// The documented race, pinned so it stays a known trade and not a
    /// surprise: the next reconciliation installs the estate as the scan saw
    /// it, which puts back a resource an event deleted mid-scan. The graph is
    /// briefly behind, never inconsistent — the patch always describes the
    /// graph that was installed.
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
