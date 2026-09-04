//! Incremental graph diffs (Phase 1 of `docs/change_monitoring_design.md`).
//!
//! The live backend keeps a persistent graph and, on each reconciliation scan,
//! projects into a scratch graph and diffs it against the live one. The result
//! is a [`GraphPatch`] describing exactly which nodes/edges were added or
//! removed, keyed by the stable [`node_key`]/[`edge_key`] identity so a
//! consumer can apply it without a full rebuild.
//!
//! Node *property updates* are represented as a remove + add of the same key:
//! the typed `Node` encodes its config, so a changed resource is a different
//! value. The one property that does *not* work that way is Tier-2 liveness,
//! which changes constantly and must not churn the topology — it rides along in
//! `observations`/`expired`, keyed by the same stable identity (`atlas::flow`).

use crate::atlas::collection::{CollectionReport, CollectionSource, FailureKind};
use crate::atlas::definition::{Edge, Node};
use crate::atlas::export::{
    RenderEdge, RenderNode, RenderObservation, SNAPSHOT_VERSION, edge_key, node_key,
};
use crate::atlas::graph_builder::GraphBuilder;
use petgraph::graph::Graph;
use petgraph::visit::EdgeRef;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A minimal set of changes between two graph states. Added items carry their
/// full render info so the frontend can materialize them; removed items are
/// referenced by stable key.
#[derive(Clone, Serialize)]
pub struct GraphPatch {
    pub version: u32,
    pub added_nodes: Vec<RenderNode>,
    pub removed_nodes: Vec<String>,
    pub added_edges: Vec<RenderEdge>,
    pub removed_edges: Vec<String>,
    /// Tier-2 liveness for nodes and edges whose freshness changed. Carried
    /// separately from the topology lists because it is a different kind of
    /// change: an observation neither creates nor destroys anything, and on a
    /// busy estate it is the only thing changing most ticks.
    pub observations: Vec<RenderObservation>,
    /// Keys whose observation has lapsed. Without this a client keeps showing
    /// the last freshness it heard, forever — an unobserved resource would
    /// stay lit rather than going dark.
    pub expired: Vec<String>,
}

impl GraphPatch {
    /// A patch that changes nothing. The honest answer for an event that told
    /// us something we already knew — the graph is add-only per event and
    /// every apply is idempotent, so "no change" is a routine outcome, not a
    /// failure.
    pub fn empty() -> Self {
        Self {
            version: SNAPSHOT_VERSION,
            added_nodes: Vec::new(),
            removed_nodes: Vec::new(),
            added_edges: Vec::new(),
            removed_edges: Vec::new(),
            observations: Vec::new(),
            expired: Vec::new(),
        }
    }

    /// Fold another patch into this one. The event ingest path produces one
    /// patch per event and broadcasts the batch as a single patch, so they have
    /// to combine.
    ///
    /// Concatenating the four lists is *not* enough. One batch can carry a
    /// resource's whole life — a `RunInstances` and the `TerminateInstances`
    /// that followed it can arrive in the same SQS receive — and the combined
    /// patch would then name the same key as both added and removed. A consumer
    /// has to pick an order for that, and the frontend removes before it adds
    /// (so an edge never outlives its endpoints), which means the removal
    /// no-ops against a node that is not there yet and the addition then puts
    /// the resource back. The client would keep a resource the server does not
    /// have, permanently, until it reconnected.
    ///
    /// So a removal cancels a pending addition of the same key instead of being
    /// appended beside it, leaving the net change. The reverse order needs no
    /// special case: remove-then-add already applies correctly.
    ///
    /// The scan is linear per removal, which is fine at batch scale (tens of
    /// events); this is not the Tier-3 path, whose diffs never overlap.
    pub fn extend(&mut self, other: GraphPatch) {
        for key in other.removed_nodes {
            if !cancel(&mut self.added_nodes, &key, |node| &node.key) {
                self.removed_nodes.push(key);
            }
        }
        for key in other.removed_edges {
            if !cancel(&mut self.added_edges, &key, |edge| &edge.key) {
                self.removed_edges.push(key);
            }
        }
        self.added_nodes.extend(other.added_nodes);
        self.added_edges.extend(other.added_edges);

        // Liveness is last-writer-wins within a batch, both ways round: a fresh
        // observation supersedes a pending expiry for its key, and an expiry
        // supersedes a pending observation. Appending both would leave the
        // consumer to guess, and it applies the two lists in a fixed order.
        for key in other.expired {
            cancel(&mut self.observations, &key, |o| &o.key);
            if !self.expired.contains(&key) {
                self.expired.push(key);
            }
        }
        for observation in other.observations {
            self.expired.retain(|key| key != &observation.key);
            cancel(&mut self.observations, &observation.key, |o| &o.key);
            self.observations.push(observation);
        }
    }

    /// A patch that touches nothing — the common case between polls, and the
    /// signal the poll loop uses to skip a broadcast.
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
            && self.observations.is_empty()
            && self.expired.is_empty()
    }
}

/// Drop a pending addition of `key`, reporting whether there was one. The two
/// halves then annihilate: nothing was added, so nothing needs removing.
fn cancel<T>(added: &mut Vec<T>, key: &str, key_of: impl Fn(&T) -> &str) -> bool {
    match added.iter().position(|item| key_of(item) == key) {
        Some(at) => {
            added.remove(at);
            true
        }
        None => false,
    }
}

/// Identity of an edge: its endpoint values plus its weight. Endpoints are
/// compared by `Node` value (not petgraph index), so this survives rebuilds.
type EdgeRefId<'a> = (&'a Node, &'a Node, &'a Edge);

fn edge_ref_ids(graph: &Graph<Node, Edge>) -> HashSet<EdgeRefId<'_>> {
    graph
        .edge_references()
        .map(|e| (&graph[e.source()], &graph[e.target()], e.weight()))
        .collect()
}

/// Diff `new` against `old`, producing the change set that turns `old` into
/// `new`. Identity is the typed `Node`/`Edge` value (both `Hash + Eq`), so this
/// is independent of petgraph index churn and — crucially for the poll loop
/// that runs this every tick — allocates strings only for items that actually
/// changed, not for the whole graph.
pub fn diff(old: &Graph<Node, Edge>, new: &Graph<Node, Edge>) -> GraphPatch {
    let old_nodes: HashSet<&Node> = old.node_weights().collect();
    let new_nodes: HashSet<&Node> = new.node_weights().collect();
    let old_edges = edge_ref_ids(old);
    let new_edges = edge_ref_ids(new);

    let added_nodes = new
        .node_indices()
        .filter(|&i| !old_nodes.contains(&new[i]))
        .map(|i| RenderNode::new(&new[i], i.index() as u32))
        .collect();
    let removed_nodes = old
        .node_weights()
        .filter(|n| !new_nodes.contains(n))
        .map(node_key)
        .collect();

    let added_edges = new
        .edge_references()
        .filter(|e| !old_edges.contains(&(&new[e.source()], &new[e.target()], e.weight())))
        .map(|e| {
            RenderEdge::new(
                &new[e.source()],
                &new[e.target()],
                e.weight(),
                e.source().index() as u32,
                e.target().index() as u32,
            )
        })
        .collect();
    let removed_edges = old
        .edge_references()
        .filter(|e| !new_edges.contains(&(&old[e.source()], &old[e.target()], e.weight())))
        .map(|e| {
            let source_key = node_key(&old[e.source()]);
            let target_key = node_key(&old[e.target()]);
            edge_key(&source_key, &target_key, e.weight())
        })
        .collect();

    GraphPatch {
        added_nodes,
        removed_nodes,
        added_edges,
        removed_edges,
        ..GraphPatch::empty()
    }
}

/// Fold `context` into `live` and report only what was genuinely new.
///
/// Both live tiers need exactly this: Tier 1 merges the neighbourhood an event
/// described, Tier 2 merges the flow edges a batch of records described, and
/// neither may re-announce what the graph already held or every redelivery
/// would wake up every connected client. Novelty has to be measured *before*
/// the merge — `GraphBuilder::merge` deduplicates silently, and afterwards
/// there is no way to tell what it added.
pub fn merge_additions(live: &mut GraphBuilder, context: &Graph<Node, Edge>) -> GraphPatch {
    let new_nodes: Vec<Node> = context
        .node_weights()
        .filter(|node| !live.contains(node))
        .cloned()
        .collect();
    let new_edges: Vec<(Node, Node, Edge)> = context
        .edge_references()
        .filter(|e| !live.has_edge(&context[e.source()], &context[e.target()], e.weight()))
        .map(|e| {
            (
                context[e.source()].clone(),
                context[e.target()].clone(),
                e.weight().clone(),
            )
        })
        .collect();

    live.merge(context);

    // Indices only exist after the merge, and the render payload needs them.
    let index_of =
        |live: &GraphBuilder, node: &Node| live.index_of(node).map_or(0, |idx| idx.index() as u32);
    let added_nodes = new_nodes
        .iter()
        .map(|node| RenderNode::new(node, index_of(live, node)))
        .collect();
    let added_edges = new_edges
        .iter()
        .map(|(source, target, edge)| {
            RenderEdge::new(
                source,
                target,
                edge,
                index_of(live, source),
                index_of(live, target),
            )
        })
        .collect();

    GraphPatch {
        added_nodes,
        added_edges,
        ..GraphPatch::empty()
    }
}

/// Fold the parts of `previous` that `next` cannot speak for back into `next`,
/// so a diff against `previous` only adds *within the failed sources'
/// territory*.
///
/// This is the reconciliation policy for an incomplete scan. When a source
/// could not be read, its resources are absent from `next` for a reason that
/// has nothing to do with them being gone, and diffing as-is would broadcast
/// spurious removals. Carrying that source's state forward keeps its resources
/// until a scan can speak to them again — stale rather than flickering, the
/// safer failure for a graph meant to be authoritative.
///
/// Crucially the retention is *scoped*: only nodes owned by an unreadable
/// source (plus the cross-cloud stitching nodes no source owns, per
/// [`Node::owner`]) are held. Every healthy provider stays fully authoritative,
/// including its deletions, so a collector that fails on every tick can no
/// longer stop the rest of the graph from converging.
///
/// [`Edge::TrafficFlow`] is the one thing deliberately *not* carried forward.
/// It comes from the Tier-2 flow overlay, not from any provider scan, so a
/// provider going dark says nothing about whether traffic is still flowing —
/// and holding those edges would suspend the overlay's own expiry for as long
/// as any collector is unhealthy. The overlay re-folds every flow it still
/// believes in on the same tick, so nothing live is lost.
///
/// The fold itself is [`GraphBuilder::merge_selected`] — the scan's own builder
/// already carries the node index this needs, and node/edge duplicate identity
/// stays defined in exactly one place.
/// An owner-less node is retained only while something other than observed
/// traffic still points at it. They are held at all because any provider may
/// reference them, but the flow overlay creates one per unrecognised remote
/// address, and those are bounded by [`FlowIndex`]'s capacity rather than by
/// the graph's: carrying an edgeless pivot forward on every tick would let a
/// single long outage accumulate an unbounded population of orphan nodes in
/// the live graph and in every client's snapshot. A pivot a projector produced
/// keeps the edge that produced it and so survives; one the overlay left
/// behind has nothing and goes.
///
/// [`FlowIndex`]: crate::atlas::flow::FlowIndex
pub fn carry_forward(
    next: &mut GraphBuilder,
    previous: &Graph<Node, Edge>,
    unreadable: &HashSet<CollectionSource>,
) {
    let anchored: HashSet<&Node> = previous
        .edge_references()
        .filter(|e| e.weight() != &Edge::TrafficFlow)
        .flat_map(|e| [&previous[e.source()], &previous[e.target()]])
        .collect();

    next.merge_selected(
        previous,
        |node| match node.owner() {
            Some(source) => unreadable.contains(&source),
            None => anchored.contains(node),
        },
        |edge| *edge != Edge::TrafficFlow,
    );
}

/// How long a source may go unread before the graph stops waiting for it.
///
/// Carrying resources forward is the right answer to a *transient* failure and
/// the wrong answer to a permanent one. A collector that fails on every single
/// tick would otherwise pin its resources in the graph indefinitely, and
/// "unconfirmed since the process started" is not a live twin — the retention
/// that protects against flicker turns into a guarantee that deletions never
/// converge.
///
/// So retention is a budget. A source is held for `budget` consecutive
/// incomplete scans; on the next one it is released, the differ deletes
/// whatever those scans could not confirm, and the graph goes back to telling
/// the truth about what it can actually see. A source that recovers starts over
/// with a full budget.
///
/// This is deliberately provider-grained and deliberately blunt: a long outage
/// releases that provider's whole unconfirmed estate at once. The alternative —
/// attributing every node to the collector that produced it — needs per-node
/// provenance, because a node kind does not identify its collector (an
/// `AwsEc2Vpc` comes from five of them).
///
/// The budget is not uniform, because not every failure is equally likely to
/// recover. Waiting out a throttle is sensible; waiting out a rejected
/// credential is just claiming resources nobody can verify, since no amount of
/// polling will fix it without a human. So a source diagnosed as
/// [`FailureKind::Unauthorized`] spends the shorter [`Retention::AUTH_BUDGET`]
/// instead. [`FailureKind::Malformed`] never reaches here at all — such a scan
/// was read, so the source stays authoritative and nothing is held.
pub struct Retention {
    budget: u32,
    auth_budget: u32,
    consecutive_failures: HashMap<CollectionSource, u32>,
}

impl Retention {
    /// Ticks a source is held for by default. At the default 60s poll interval
    /// that is ten minutes of an outage before the graph gives up on a provider.
    pub const DEFAULT_BUDGET: u32 = 10;

    /// Ticks a source is held for when the diagnosis is unambiguously a
    /// permissions problem. Deliberately short but not zero: a token refresh
    /// can race, and wiping a provider's whole estate over one such blip only
    /// to restore it on the next tick is its own kind of wrong.
    pub const AUTH_BUDGET: u32 = 2;

    /// A budget of 0 disables retention entirely (every incomplete scan deletes
    /// what it could not confirm); there is no "forever" — pass a large budget
    /// if that is what you want.
    pub fn new(budget: u32) -> Self {
        Self {
            budget,
            // Never longer than the configured budget: `--retain-scans 0`
            // means retain nothing, and an auth failure is not an exception.
            auth_budget: Self::AUTH_BUDGET.min(budget),
            consecutive_failures: HashMap::new(),
        }
    }

    fn budget_for(&self, kind: FailureKind) -> u32 {
        match kind {
            FailureKind::Unauthorized => self.auth_budget,
            _ => self.budget,
        }
    }

    /// Fold one scan's report in and answer which sources are still being held.
    /// Sources missing from the report have recovered, so their streak resets.
    pub fn hold(&mut self, report: &CollectionReport) -> HashSet<CollectionSource> {
        let unreadable = report.unreadable_sources();
        self.consecutive_failures
            .retain(|source, _| unreadable.contains(source));

        unreadable
            .into_iter()
            .filter(|&source| {
                let budget = report
                    .unreadable_kind(source)
                    .map_or(self.budget, |kind| self.budget_for(kind));
                let streak = self.consecutive_failures.entry(source).or_default();
                *streak += 1;
                *streak <= budget
            })
            .collect()
    }

    /// Consecutive scans that have failed to read `source`, for reporting.
    pub fn streak(&self, source: CollectionSource) -> u32 {
        self.consecutive_failures
            .get(&source)
            .copied()
            .unwrap_or_default()
    }
}

impl Default for Retention {
    fn default() -> Self {
        Self::new(Self::DEFAULT_BUDGET)
    }
}
