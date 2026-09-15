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

#[derive(Clone, Serialize)]
pub struct GraphPatch {
    pub version: u32,
    pub added_nodes: Vec<RenderNode>,
    pub removed_nodes: Vec<String>,
    pub added_edges: Vec<RenderEdge>,
    pub removed_edges: Vec<String>,
    pub observations: Vec<RenderObservation>,
    pub expired: Vec<String>,
}

impl GraphPatch {
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

    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
            && self.observations.is_empty()
            && self.expired.is_empty()
    }
}

fn cancel<T>(added: &mut Vec<T>, key: &str, key_of: impl Fn(&T) -> &str) -> bool {
    match added.iter().position(|item| key_of(item) == key) {
        Some(at) => {
            added.remove(at);
            true
        }
        None => false,
    }
}

type EdgeRefId<'a> = (&'a Node, &'a Node, &'a Edge);

fn edge_ref_ids(graph: &Graph<Node, Edge>) -> HashSet<EdgeRefId<'_>> {
    graph
        .edge_references()
        .map(|e| (&graph[e.source()], &graph[e.target()], e.weight()))
        .collect()
}

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

pub fn carry_forward(
    next: &mut GraphBuilder,
    previous: &Graph<Node, Edge>,
    unreadable: &HashSet<CollectionSource>,
) {
    let anchored: HashSet<&Node> = previous
        .edge_references()
        .filter(|e| e.weight().is_projected())
        .flat_map(|e| [&previous[e.source()], &previous[e.target()]])
        .collect();

    let held = |node: &Node| match node.owner() {
        Some(source) => unreadable.contains(&source),
        None => anchored.contains(node),
    };

    next.merge_selected(previous, held, |source, target, edge| {
        edge.is_projected()
            && [source, target].iter().all(|node| {
                node.owner()
                    .is_none_or(|source| unreadable.contains(&source))
            })
    });
}

pub struct Retention {
    budget: u32,
    auth_budget: u32,
    consecutive_failures: HashMap<CollectionSource, u32>,
}

impl Retention {
    pub const DEFAULT_BUDGET: u32 = 10;

    pub const AUTH_BUDGET: u32 = 2;

    pub fn new(budget: u32) -> Self {
        Self {
            budget,
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
