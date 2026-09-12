use crate::atlas::collection::CollectionSource;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::export::{edge_key, node_key};
use crate::atlas::flow::{eviction_cut, survives};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::patch::{GraphPatch, merge_additions};
use petgraph::graph::Graph;
use std::collections::HashMap;
use std::fmt;

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

pub struct ChangeEvent {
    pub source: CollectionSource,
    pub scope: String,
    pub id: String,
    pub observed_at: i64,
    pub op: ChangeOp,
    pub node: Node,
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

    pub fn with_context(mut self, context: Graph<Node, Edge>) -> Self {
        self.context = context;
        self
    }

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
    const MAX_TRACKED: usize = 100_000;

    const ORDERING_WINDOW_MS: i64 = 60 * 60 * 1000;

    const LOW_WATER: usize = Self::MAX_TRACKED * 3 / 4;

    pub fn new() -> Self {
        Self {
            applied: HashMap::new(),
            newest: i64::MIN,
        }
    }

    pub fn apply(&mut self, live: &mut GraphBuilder, event: &ChangeEvent) -> GraphPatch {
        let key = event.key();
        if self.is_stale(event) {
            return GraphPatch::empty();
        }

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

    pub fn is_stale(&self, event: &ChangeEvent) -> bool {
        self.applied
            .get(&event.key())
            .is_some_and(|&(at, claim)| event.observed_at < at && claim != Claim::from(event.op))
    }

    fn prune(&mut self) {
        if self.applied.len() <= Self::MAX_TRACKED {
            return;
        }

        let horizon = self.newest.saturating_sub(Self::ORDERING_WINDOW_MS);
        self.applied.retain(|_, &mut (at, _)| at >= horizon);
        if self.applied.len() <= Self::MAX_TRACKED {
            return;
        }

        let (cutoff, mut ties) =
            eviction_cut(self.applied.values().map(|&(at, _)| at), Self::LOW_WATER);
        self.applied
            .retain(|_, &mut (at, _)| survives(at, cutoff, &mut ties));
    }
}

fn merge_context(live: &mut GraphBuilder, event: &ChangeEvent) -> GraphPatch {
    merge_additions(live, &event.context)
}

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

    fn instance_context(id: &str) -> Graph<Node, Edge> {
        let mut builder = GraphBuilder::new();
        let vpc = builder.get_or_add_node(Node::AwsEc2Vpc("vpc-1".into()));
        let subnet = builder.link_to(vpc, Node::AwsEc2Subnet("subnet-1".into()), Edge::Contains);
        let inst = builder.get_or_add_node(instance(id));
        let eni = builder.link_to(inst, Node::AwsEc2Eni(id.into()), Edge::HasIp);
        builder.add_edge(eni, subnet, Edge::AttachedTo);
        builder.graph
    }

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

    #[test]
    fn ordering_is_tracked_per_resource() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        applier.apply(&mut live, &created("i-1", T0 + 1_000));
        let patch = applier.apply(&mut live, &created("i-2", T0));

        assert!(!patch.is_empty());
        assert!(live.contains(&instance("i-2")));
    }

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

    #[test]
    fn pruning_keeps_a_batch_that_shares_one_timestamp() {
        let mut applier = EventApplier::new();
        for i in 0..=EventApplier::MAX_TRACKED {
            applier
                .applied
                .insert(format!("aws/i-{i}"), (T0, Claim::Present));
        }
        applier.newest = T0;

        applier.prune();

        assert_eq!(
            applier.applied.len(),
            EventApplier::LOW_WATER,
            "a tied batch must be trimmed to the low-water mark, not emptied"
        );
    }

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

    #[test]
    fn an_older_event_that_agrees_still_contributes_its_context() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        let bare = event(ChangeOp::Created, instance("i-1"), T0 + 1_000)
            .with_context(instance_only_context("i-1"));
        applier.apply(&mut live, &bare);
        assert_eq!(live.graph.node_count(), 1);

        let patch = applier.apply(&mut live, &created("i-1", T0));

        assert!(
            !patch.is_empty(),
            "the richer statement must not be discarded"
        );
        assert!(live.contains(&Node::AwsEc2Eni("i-1".into())));
        assert!(live.contains(&Node::AwsEc2Subnet("subnet-1".into())));
    }

    #[test]
    fn an_applied_older_event_does_not_weaken_the_guard() {
        let mut live = GraphBuilder::new();
        let mut applier = EventApplier::new();

        applier.apply(&mut live, &created("i-1", T0 + 1_000));
        applier.apply(&mut live, &created("i-1", T0));

        let stale_delete = event(ChangeOp::Deleted, instance("i-1"), T0 + 500);
        assert!(applier.is_stale(&stale_delete));
        applier.apply(&mut live, &stale_delete);

        assert!(live.contains(&instance("i-1")));
    }

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
