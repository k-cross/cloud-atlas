//! Normalized change events — Tier 1 of `docs/change_monitoring_design.md`.
//!
//! A cloud's control-plane feed (AWS EventBridge/CloudTrail/Config, GCP asset
//! feeds, Azure Event Grid) says *what changed* seconds after it changed,
//! instead of being re-derived from a full scan a minute or ten later. Every
//! feed has its own wire format, so each provider's adapter translates into the
//! one shape here: a [`ChangeEvent`] naming the resource, what happened to it,
//! and the neighbourhood the event was able to tell us about.
//!
//! Two rules keep this tier from fighting the Tier-3 reconciliation scan, and
//! both are load-bearing:
//!
//! 1. **An adapter may only produce nodes and edges the full-scan projector
//!    would also produce.** Tier 3 is authoritative and diffs the whole graph:
//!    an edge invented here that no projector emits is deleted on the next
//!    reconciliation and re-added by the next event, flapping forever. Adapters
//!    therefore build their context through the *same* projector code where one
//!    exists (see `projector::aws::project_instance`), and skip resources whose
//!    identity they cannot key the way the projector keys it.
//! 2. **An event is a statement about one resource, not about the estate.** A
//!    `Created`/`Modified` event only ever adds; it never removes the edges it
//!    did not mention, because it does not know about them. Only `Deleted` and
//!    the Tier-3 differ remove anything. That asymmetry is what makes an
//!    at-least-once, occasionally-out-of-order feed safe to apply.

use crate::atlas::collection::CollectionSource;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::export::{edge_key, node_key};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::patch::{GraphPatch, merge_additions};
use petgraph::graph::Graph;
use std::collections::HashMap;
use std::fmt;

/// What happened to the resource.
///
/// `Created` and `Modified` are applied identically — the typed `Node` encodes
/// a resource's identity, not its mutable config, so "it exists" is the whole
/// content of both. They stay distinct because the distinction is real in the
/// feed and worth logging, and because richer property updates (the liveness
/// work) will need to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOp {
    Created,
    Modified,
    Deleted,
}

impl fmt::Display for ChangeOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ChangeOp::Created => "created",
            ChangeOp::Modified => "modified",
            ChangeOp::Deleted => "deleted",
        };
        f.write_str(name)
    }
}

/// One normalized change, whatever cloud it came from.
pub struct ChangeEvent {
    pub source: CollectionSource,
    /// Region/project/subscription the event was observed in, for reporting —
    /// the same granularity a collection failure is attributed at.
    pub scope: String,
    /// The provider's own event id. Kept for logging and for tracing a graph
    /// change back to the audit record that caused it.
    pub id: String,
    /// When the *cloud* recorded the change, in epoch milliseconds — not when
    /// we received it. Delivery order is not change order, so this is the only
    /// sound basis for deciding which of two events about one resource wins.
    pub observed_at: i64,
    pub op: ChangeOp,
    /// The resource this event is about. Its identity orders the event and, for
    /// [`ChangeOp::Deleted`], is exactly what leaves the graph.
    pub node: Node,
    /// Everything the event let us say about the resource's surroundings,
    /// already in graph form — the subject node plus whatever neighbours and
    /// edges the payload named. Built by the adapter through the projector, so
    /// it is a subgraph Tier 3 would agree with. Empty for a deletion, and for
    /// feeds that carry identity only.
    pub context: Graph<Node, Edge>,
}

impl ChangeEvent {
    pub fn new(
        source: CollectionSource,
        scope: impl Into<String>,
        id: impl Into<String>,
        observed_at: i64,
        op: ChangeOp,
        node: Node,
    ) -> Self {
        Self {
            source,
            scope: scope.into(),
            id: id.into(),
            observed_at,
            op,
            node,
            context: Graph::new(),
        }
    }

    /// Attach the neighbourhood the payload described. The subject node is
    /// expected to be in there too; a context that does not mention it would
    /// silently fail to create it.
    pub fn with_context(mut self, context: Graph<Node, Edge>) -> Self {
        self.context = context;
        self
    }

    /// Stable identity of the resource this event is about.
    pub fn key(&self) -> String {
        node_key(&self.node)
    }
}

impl fmt::Debug for ChangeEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangeEvent")
            .field("source", &self.source)
            .field("scope", &self.scope)
            .field("id", &self.id)
            .field("observed_at", &self.observed_at)
            .field("op", &self.op)
            .field("node", &self.node)
            .field("context_nodes", &self.context.node_count())
            .field("context_edges", &self.context.edge_count())
            .finish()
    }
}

/// What an event asserts about a resource's existence — the only thing two
/// events can genuinely contradict each other about.
///
/// `Created` and `Modified` both claim `Present` and are both purely additive,
/// which is what makes the ordering guard narrow enough to be safe: two
/// statements that agree can be applied in either order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    Present,
    Gone,
}

impl From<ChangeOp> for Claim {
    fn from(op: ChangeOp) -> Self {
        match op {
            ChangeOp::Deleted => Claim::Gone,
            ChangeOp::Created | ChangeOp::Modified => Claim::Present,
        }
    }
}

/// Applies [`ChangeEvent`]s to the live graph, in the face of a feed that
/// delivers at least once and does not promise order.
///
/// Idempotency is mostly free: `GraphBuilder` dedups nodes and edges, so
/// applying the same creation twice is a no-op and the second apply produces an
/// empty patch. Ordering is not free. A delete followed by a stale re-delivery
/// of the create that preceded it would resurrect a resource that is gone, and
/// nothing in the graph itself can tell that the create is the older statement.
/// So the applier remembers, per resource, the last statement it acted on and
/// ignores an older one that *contradicts* it.
///
/// The contradiction test is what keeps this from throwing away good data. The
/// feeds run at wildly different latencies: an EC2 state change arrives in
/// seconds, while the CloudTrail `RunInstances` record describing the same
/// launch — the one carrying the VPC, subnet, ENI and security groups — is
/// stamped *earlier* and lands minutes later. A blanket "older loses" would
/// discard exactly the event worth having, and the instance would sit as a
/// floating node until the next full scan. Since both claim the resource
/// exists, and creation is purely additive, the late-but-older event is applied
/// and only its timestamp is ignored.
pub struct EventApplier {
    applied: HashMap<String, (i64, Claim)>,
    newest: i64,
}

impl Default for EventApplier {
    fn default() -> Self {
        Self::new()
    }
}

impl EventApplier {
    /// Resources tracked before the applier starts forgetting the ones it has
    /// not heard about in a while. This is a long-running daemon: without a
    /// bound the ordering table grows for the lifetime of the process, one
    /// entry per resource ever mentioned by any event, including every
    /// short-lived instance in an autoscaling group.
    const MAX_TRACKED: usize = 100_000;

    /// How far back a forgotten resource's history has to be for forgetting it
    /// to be safe. An event older than this relative to the newest one we have
    /// seen is well past any plausible redelivery or reordering window, so
    /// dropping its bookkeeping cannot resurrect anything.
    const ORDERING_WINDOW_MS: i64 = 60 * 60 * 1000;

    /// What a forced eviction trims down to. Below [`Self::MAX_TRACKED`] so
    /// that a table pinned at the budget does not re-sort on every event.
    const LOW_WATER: usize = Self::MAX_TRACKED * 3 / 4;

    pub fn new() -> Self {
        Self {
            applied: HashMap::new(),
            newest: i64::MIN,
        }
    }

    /// Apply one event, returning exactly what changed. An event that tells us
    /// nothing new — a duplicate delivery, a creation we already have, a
    /// deletion of something already gone — returns an empty patch, which the
    /// caller uses to decide whether a broadcast is worth making.
    pub fn apply(&mut self, live: &mut GraphBuilder, event: &ChangeEvent) -> GraphPatch {
        let key = event.key();
        if self.is_stale(event) {
            return GraphPatch::empty();
        }

        // Only the newest statement is recorded. An older event that was
        // applied anyway (because it agreed) must not roll the guard back and
        // let the statement it disagrees with through next time.
        let claim = Claim::from(event.op);
        match self.applied.get(&key) {
            Some(&(at, _)) if at > event.observed_at => {}
            _ => {
                self.applied.insert(key, (event.observed_at, claim));
            }
        }
        self.newest = self.newest.max(event.observed_at);
        self.prune();

        match event.op {
            ChangeOp::Created | ChangeOp::Modified => merge_context(live, event),
            ChangeOp::Deleted => remove(live, &event.node),
        }
    }

    /// Whether this event is an older statement that contradicts one already
    /// applied for its resource, and so must be ignored. Exposed for reporting
    /// — the caller may want to count how lossy the feed's ordering is.
    pub fn is_stale(&self, event: &ChangeEvent) -> bool {
        self.applied
            .get(&event.key())
            .is_some_and(|&(at, claim)| event.observed_at < at && claim != Claim::from(event.op))
    }

    /// Keep the ordering table bounded. Age alone cannot do it: churn inside a
    /// single window (an autoscaling storm, a wide Config feed) can exceed the
    /// budget outright, and then `retain` would evict nothing while running a
    /// full scan of a growing map on every event — the leak the budget exists
    /// to prevent, plus a cost per event. So age first, and if that was not
    /// enough, drop the oldest entries down to a low-water mark, which also
    /// keeps this from re-firing on the very next event.
    fn prune(&mut self) {
        if self.applied.len() <= Self::MAX_TRACKED {
            return;
        }

        let horizon = self.newest.saturating_sub(Self::ORDERING_WINDOW_MS);
        self.applied.retain(|_, &mut (at, _)| at >= horizon);
        if self.applied.len() <= Self::MAX_TRACKED {
            return;
        }

        // Entries sharing the cutoff timestamp are dropped together, so this
        // can trim below the low-water mark. That is the safe direction and it
        // is deliberate: retaining them instead (`>=`) can leave the map above
        // `MAX_TRACKED`, which puts this sort back on *every* subsequent event
        // — the per-event cost the budget exists to avoid. Over-trimming only
        // forgets ordering for resources that have been quiet, which the
        // reconciliation scan corrects anyway.
        let mut times: Vec<i64> = self.applied.values().map(|&(at, _)| at).collect();
        times.sort_unstable();
        let cutoff = times[times.len() - Self::LOW_WATER];
        self.applied.retain(|_, &mut (at, _)| at > cutoff);
    }
}

/// A creation or modification: fold the event's context into the live graph and
/// report only what was genuinely new. The novelty bookkeeping is
/// [`merge_additions`], shared with the Tier-2 flow overlay — both tiers add a
/// subgraph to the live graph and must announce only the parts of it the graph
/// did not already hold.
fn merge_context(live: &mut GraphBuilder, event: &ChangeEvent) -> GraphPatch {
    merge_additions(live, &event.context)
}

/// A deletion: take the node out along with every edge that touched it. The
/// edges have to be named explicitly — a client applying the patch removes
/// exactly what the patch lists, and an edge left behind would point at a node
/// that no longer exists.
fn remove(live: &mut GraphBuilder, node: &Node) -> GraphPatch {
    let Some(removal) = live.remove_node(node) else {
        return GraphPatch::empty();
    };

    GraphPatch {
        removed_nodes: vec![node_key(&removal.node)],
        removed_edges: removal
            .edges
            .iter()
            .map(|(source, target, edge)| edge_key(&node_key(source), &node_key(target), edge))
            .collect(),
        ..GraphPatch::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_700_000_000_000;

    fn instance(id: &str) -> Node {
        Node::AwsEc2Instance(id.into())
    }

    fn event(op: ChangeOp, node: Node, at: i64) -> ChangeEvent {
        ChangeEvent::new(CollectionSource::Aws, "us-east-1", "evt", at, op, node)
    }

    /// An instance with the neighbourhood the projector gives it: the
    /// ENI pivot onto a subnet inside a VPC.
    fn instance_context(id: &str) -> Graph<Node, Edge> {
        let mut builder = GraphBuilder::new();
        let vpc = builder.get_or_add_node(Node::AwsEc2Vpc("vpc-1".into()));
        let subnet = builder.link_to(vpc, Node::AwsEc2Subnet("subnet-1".into()), Edge::Contains);
        let inst = builder.get_or_add_node(instance(id));
        let eni = builder.link_to(inst, Node::AwsEc2Eni(id.into()), Edge::HasIp);
        builder.add_edge(eni, subnet, Edge::AttachedTo);
        builder.graph
    }

    /// The instance and nothing else — what a bare EC2 state-change
    /// notification can say.
    fn instance_only_context(id: &str) -> Graph<Node, Edge> {
        let mut builder = GraphBuilder::new();
        builder.get_or_add_node(instance(id));
        builder.graph
    }

    fn created(id: &str, at: i64) -> ChangeEvent {
        event(ChangeOp::Created, instance(id), at).with_context(instance_context(id))
    }

    #[test]
    fn a_creation_adds_the_whole_context_it_carried() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        let patch = applier.apply(&mut live, &created("i-1", T0));

        assert_eq!(patch.added_nodes.len(), 4, "vpc, subnet, instance, eni");
        assert_eq!(patch.added_edges.len(), 3);
        assert!(live.contains(&instance("i-1")));
    }

    /// Delivery is at-least-once, so the same event arriving twice must be a
    /// no-op rather than a duplicated node or a second broadcast.
    #[test]
    fn a_redelivered_event_changes_nothing() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        applier.apply(&mut live, &created("i-1", T0));
        let nodes = live.graph.node_count();
        let edges = live.graph.edge_count();

        let patch = applier.apply(&mut live, &created("i-1", T0));

        assert!(patch.is_empty(), "nothing new to say");
        assert_eq!(live.graph.node_count(), nodes);
        assert_eq!(live.graph.edge_count(), edges);
    }

    #[test]
    fn a_deletion_removes_the_node_and_names_its_edges() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();
        applier.apply(&mut live, &created("i-1", T0));

        let patch = applier.apply(
            &mut live,
            &event(ChangeOp::Deleted, instance("i-1"), T0 + 1),
        );

        assert_eq!(patch.removed_nodes.len(), 1);
        assert_eq!(
            patch.removed_edges.len(),
            1,
            "the instance -> eni edge dies with it and must be named"
        );
        assert!(!live.contains(&instance("i-1")));
        assert!(
            live.contains(&Node::AwsEc2Eni("i-1".into())),
            "an event speaks only for its own resource; the ENI is Tier 3's to remove"
        );
    }

    #[test]
    fn deleting_something_already_gone_is_a_no_op() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        let patch = applier.apply(&mut live, &event(ChangeOp::Deleted, instance("i-1"), T0));

        assert!(patch.is_empty());
    }

    /// The failure mode ordering exists to prevent: a create redelivered after
    /// the delete that superseded it would otherwise resurrect the resource.
    #[test]
    fn a_stale_create_cannot_resurrect_a_deleted_resource() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        applier.apply(&mut live, &created("i-1", T0));
        applier.apply(
            &mut live,
            &event(ChangeOp::Deleted, instance("i-1"), T0 + 1_000),
        );

        let late = created("i-1", T0);
        assert!(applier.is_stale(&late));
        let patch = applier.apply(&mut live, &late);

        assert!(patch.is_empty(), "the older statement must not win");
        assert!(!live.contains(&instance("i-1")));
    }

    /// Ordering is per resource: a stale event about one instance must not
    /// suppress a fresh event about another.
    #[test]
    fn ordering_is_tracked_per_resource() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        applier.apply(&mut live, &created("i-1", T0 + 1_000));
        let patch = applier.apply(&mut live, &created("i-2", T0));

        assert!(!patch.is_empty());
        assert!(live.contains(&instance("i-2")));
    }

    /// A later event still wins after an earlier one was ignored — being stale
    /// must not poison the resource's future.
    #[test]
    fn a_newer_event_applies_after_a_stale_one_was_dropped() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        applier.apply(
            &mut live,
            &event(ChangeOp::Deleted, instance("i-1"), T0 + 1_000),
        );
        applier.apply(&mut live, &created("i-1", T0));
        assert!(!live.contains(&instance("i-1")));

        applier.apply(&mut live, &created("i-1", T0 + 2_000));
        assert!(live.contains(&instance("i-1")), "the resource came back");
    }

    /// Applying a creation whose context is already fully present must report
    /// nothing, or every duplicate delivery would wake up every connected
    /// client.
    #[test]
    fn a_partially_known_context_reports_only_the_new_parts() {
        let mut live = GraphBuilder::new();
        live.get_or_add_node(Node::AwsEc2Vpc("vpc-1".into()));
        let mut applier = EventApplier::new();

        let patch = applier.apply(&mut live, &created("i-1", T0));

        assert_eq!(
            patch.added_nodes.len(),
            3,
            "the VPC was already known and must not be re-announced"
        );
    }

    /// Indices in a patch must resolve in the graph the patch describes, not
    /// in the event's own little context graph.
    #[test]
    fn added_items_carry_indices_into_the_live_graph() {
        let mut live = GraphBuilder::new();
        for id in ["i-9", "i-8", "i-7"] {
            live.get_or_add_node(instance(id));
        }
        let mut applier = EventApplier::new();

        let patch = applier.apply(&mut live, &created("i-1", T0));

        for node in &patch.added_nodes {
            let idx = petgraph::graph::NodeIndex::new(node.id as usize);
            assert_eq!(
                node_key(&live.graph[idx]),
                node.key,
                "index {} must address the node it claims",
                node.id
            );
        }
    }

    /// The feeds run at different latencies, and the slow one is the rich one:
    /// a CloudTrail `RunInstances` is stamped *before* the EC2 state change it
    /// caused but arrives minutes later. Both say the instance exists, so the
    /// late-but-older event must still land — otherwise the tier throws away
    /// precisely the payload that carries the VPC, subnet and ENI.
    #[test]
    fn an_older_event_that_agrees_still_contributes_its_context() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        // The bare, fast notification lands first with the later timestamp.
        let bare = event(ChangeOp::Created, instance("i-1"), T0 + 1_000)
            .with_context(instance_only_context("i-1"));
        applier.apply(&mut live, &bare);
        assert_eq!(live.graph.node_count(), 1);

        // The rich, slow record follows, stamped earlier.
        let patch = applier.apply(&mut live, &created("i-1", T0));

        assert!(
            !patch.is_empty(),
            "the richer statement must not be discarded"
        );
        assert!(live.contains(&Node::AwsEc2Eni("i-1".into())));
        assert!(live.contains(&Node::AwsEc2Subnet("subnet-1".into())));
    }

    /// Applying an older-but-agreeing event must not roll the guard back to its
    /// timestamp, or the stale deletion it was protecting against gets in.
    #[test]
    fn an_applied_older_event_does_not_weaken_the_guard() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        applier.apply(&mut live, &created("i-1", T0 + 1_000));
        applier.apply(&mut live, &created("i-1", T0));

        // Older than the newest statement, and contradicting it.
        let stale_delete = event(ChangeOp::Deleted, instance("i-1"), T0 + 500);
        assert!(applier.is_stale(&stale_delete));
        applier.apply(&mut live, &stale_delete);

        assert!(live.contains(&instance("i-1")));
    }

    /// The guard runs both ways: a delete redelivered after a newer create must
    /// not remove the resource that create put back.
    #[test]
    fn a_stale_delete_cannot_remove_a_recreated_resource() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        applier.apply(&mut live, &event(ChangeOp::Deleted, instance("i-1"), T0));
        applier.apply(&mut live, &created("i-1", T0 + 1_000));

        let patch = applier.apply(&mut live, &event(ChangeOp::Deleted, instance("i-1"), T0));

        assert!(patch.is_empty());
        assert!(live.contains(&instance("i-1")));
    }
}
