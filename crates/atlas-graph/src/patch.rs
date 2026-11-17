use crate::collection::{CollectionReport, CollectionSource, FailureKind};
use crate::definition::{Edge, Node};
use crate::export::{
    RenderEdge, RenderNode, RenderObservation, SNAPSHOT_VERSION, edge_key, node_key,
};
use crate::graph_builder::GraphBuilder;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnreadableSource {
    pub source: CollectionSource,
    pub kind: FailureKind,
    pub scans: u32,
    pub budget: u32,
}

impl UnreadableSource {
    pub fn held(&self) -> bool {
        self.scans <= self.budget
    }
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

    fn budget_of(&self, report: &CollectionReport, source: CollectionSource) -> u32 {
        report
            .unreadable_kind(source)
            .map_or(self.budget, |kind| self.budget_for(kind))
    }

    pub fn hold(&mut self, report: &CollectionReport) -> HashSet<CollectionSource> {
        let unreadable = report.unreadable_sources();
        self.consecutive_failures
            .retain(|source, _| unreadable.contains(source));

        unreadable
            .into_iter()
            .filter(|&source| {
                let budget = self.budget_of(report, source);
                let streak = self.consecutive_failures.entry(source).or_default();
                *streak += 1;
                *streak <= budget
            })
            .collect()
    }

    pub fn unreadable(&self, report: &CollectionReport) -> Vec<UnreadableSource> {
        let mut sources: Vec<UnreadableSource> = report
            .unreadable_sources()
            .into_iter()
            .map(|source| UnreadableSource {
                source,
                kind: report
                    .unreadable_kind(source)
                    .unwrap_or(FailureKind::Unavailable),
                scans: self.streak(source),
                budget: self.budget_of(report, source),
            })
            .collect();
        sources.sort_by_cached_key(|unreadable| unreadable.source.to_string());
        sources
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{all_sources, public};

    #[test]
    fn carry_forward_lets_a_healthy_source_delete_its_edge_to_a_shared_pivot() {
        let host = Node::GenericHostname("old.example.com".into());
        let zone = Node::GcpDnsManagedZone("globex-zone".into());
        let instance = Node::AwsEc2Instance("i-1".into());

        let mut live = GraphBuilder::new();
        let zone_idx = live.get_or_add_node(zone.clone());
        live.link_to(zone_idx, host.clone(), Edge::ResolvesTo);
        let instance_idx = live.get_or_add_node(instance.clone());
        live.link_to(instance_idx, host.clone(), Edge::ConnectsTo);

        let mut next = GraphBuilder::new();
        next.get_or_add_node(zone.clone());

        carry_forward(
            &mut next,
            &live.graph,
            &std::collections::HashSet::from([CollectionSource::Aws]),
        );

        assert!(
            next.has_edge(&instance, &host, &Edge::ConnectsTo),
            "the unreadable source's edge must be retained"
        );
        assert!(
            !next.has_edge(&zone, &host, &Edge::ResolvesTo),
            "a healthy source's deleted edge must not be resurrected by the pivot it shares"
        );
    }

    #[test]
    fn carry_forward_lets_a_healthy_dns_provider_repoint_and_delete_records() {
        let repointed = Node::CloudflareDnsRecord("rec-app".into());
        let deleted = Node::CloudflareDnsRecord("rec-gone".into());
        let (old_ip, new_ip) = (public("192.0.2.1"), public("192.0.2.2"));

        let mut live = GraphBuilder::new();
        for (record, name) in [
            (&repointed, "app.example.com"),
            (&deleted, "gone.example.com"),
        ] {
            let idx = live.get_or_add_node(record.clone());
            live.link_from(idx, Node::hostname(name), Edge::ResolvesTo);
            live.link_to(idx, old_ip.clone(), Edge::ResolvesTo);
        }

        let mut next = GraphBuilder::new();
        let idx = next.get_or_add_node(repointed.clone());
        next.link_from(idx, Node::hostname("app.example.com"), Edge::ResolvesTo);
        next.link_to(idx, new_ip.clone(), Edge::ResolvesTo);

        carry_forward(
            &mut next,
            &live.graph,
            &std::collections::HashSet::from([CollectionSource::Aws]),
        );

        assert!(!next.has_edge(&repointed, &old_ip, &Edge::ResolvesTo));
        assert!(!next.has_edge(
            &Node::hostname("gone.example.com"),
            &deleted,
            &Edge::ResolvesTo
        ));
        assert!(
            !next
                .graph
                .edge_references()
                .any(|e| next.graph[e.target()] == old_ip),
            "an unreadable AWS scan must not keep a healthy Cloudflare zone's old answer"
        );
    }

    #[test]
    fn carry_forward_drops_pivots_nothing_points_at_any_more() {
        let anchored = Node::ip("10.0.0.1", "vpc-globex");
        let orphan = Node::GenericIpAddress("203.0.113.7".into());
        let traffic_only = Node::GenericIpAddress("203.0.113.8".into());

        let mut live = GraphBuilder::new();

        let instance = live.get_or_add_node(Node::AwsEc2Instance("i-1".into()));
        live.link_to(instance, anchored.clone(), Edge::ConnectsTo);

        live.get_or_add_node(orphan.clone());

        live.link_to(instance, traffic_only.clone(), Edge::TrafficFlow);

        let mut next = GraphBuilder::new();
        carry_forward(&mut next, &live.graph, &all_sources());

        assert!(
            next.contains(&anchored),
            "a referenced pivot is still needed"
        );
        assert!(!next.contains(&orphan));
        assert!(!next.contains(&traffic_only));
    }
}
