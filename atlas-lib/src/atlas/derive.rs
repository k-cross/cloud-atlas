mod containment;
mod service;

use crate::atlas::definition::{Edge, Node};
use crate::atlas::export::RenderObservation;
use crate::atlas::flow::FlowIndex;
use crate::atlas::graph_builder::GraphBuilder;
use petgraph::graph::Graph;
use std::collections::HashMap;

// Every derived edge, in the one order they may run in.
//
// A derived edge is a pure function of what it is handed — the graph, and the
// flow index for which end of an observed flow is the service — so it holds no
// state, needs no retention or TTL, and every lifecycle falls out of the
// ordinary differ. A caller with no flow feed (the CLI) passes an empty index,
// which is also what its graph holds: no traffic, so only control-plane edges. The
// price is that it has to be recomputed at *every* point a graph is finalized,
// and a finalisation point that forgets one is invisible until a diff starts
// flickering. One entry point instead of a call per pass is what keeps the next
// pass from being added to three of the four places that need it.
//
// Containment runs first: it adds edges between pivots that are already there
// and adds no nodes, and nothing downstream reads `Covers`.
pub fn all(builder: &mut GraphBuilder, flows: &FlowIndex) {
    containment::link(builder);
    service::link(builder, flows);
}

// The observation channel: what the flow feed recorded, plus the status of
// every edge derivation produced. A derived edge carries no packets or bytes of
// its own — it summarises however many flows confirmed it, and counting theirs
// against it would report the same traffic twice — so it rides the channel with
// `last_seen` alone, which composes as a maximum. Snapshot readers call this
// rather than `FlowIndex::observations` so a derived edge cannot reach a client
// with no status attached.
pub fn observations(graph: &Graph<Node, Edge>, flows: &FlowIndex) -> Vec<RenderObservation> {
    let mut all = flows.observations();
    all.extend(derived_observations(graph, flows));
    all.sort_by(|a, b| a.key.cmp(&b.key));
    all
}

// The derived half alone, recomputed whole. A patch wants only what changed,
// which is `DerivedObservations::changed`.
pub fn derived_observations(
    graph: &Graph<Node, Edge>,
    flows: &FlowIndex,
) -> Vec<RenderObservation> {
    service::observations(graph, flows)
}

// What a patch stream last said about each derived edge, so a tick sends only
// what moved. Recomputing whole is cheap; sending whole is not — an inferred
// edge's status never changes on its own, and without this every idle tick in
// an estate with a single load balancer would be a patch to every client.
//
// Flows ingested between scans move `last_seen` without leaving a trace the
// reconciliation tick could see, so this compares values rather than gating on
// whether the flow half happened to drain anything.
#[derive(Default)]
pub struct DerivedObservations {
    sent: HashMap<String, (i64, &'static str)>,
}

impl DerivedObservations {
    // Rebuilt from `current` every call, so an edge that disappeared stops being
    // remembered: bounded by the live derived edges, which a daemon needs.
    pub fn changed(&mut self, current: Vec<RenderObservation>) -> Vec<RenderObservation> {
        let mut sent = HashMap::with_capacity(current.len());
        let mut changed = Vec::new();
        for observation in current {
            let value = (observation.last_seen, observation.status);
            let previous = self.sent.get(&observation.key);
            sent.insert(observation.key.clone(), value);
            if previous != Some(&value) {
                changed.push(observation);
            }
        }
        self.sent = sent;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::definition::{Edge, Node};

    #[test]
    fn deriving_twice_changes_nothing() {
        let mut builder = GraphBuilder::new();
        let range = builder.get_or_add_node(Node::ip("10.0.0.0/8"));
        builder.link_to(range, Node::ip("10.1.2.3"), Edge::RoutesTo);

        let flows = FlowIndex::default();
        all(&mut builder, &flows);
        let once = (builder.graph.node_count(), builder.graph.edge_count());
        all(&mut builder, &flows);

        assert_eq!(
            once,
            (builder.graph.node_count(), builder.graph.edge_count()),
            "derivation runs on every tick, so it has to be idempotent"
        );
        assert!(builder.has_edge(
            &Node::ip("10.0.0.0/8"),
            &Node::ip("10.1.2.3"),
            &Edge::Covers
        ));
    }

    fn observed(key: &str, last_seen: i64, status: &'static str) -> RenderObservation {
        RenderObservation {
            key: key.to_owned(),
            last_seen,
            packets: None,
            bytes: None,
            status,
        }
    }

    #[test]
    fn an_idle_tick_sends_nothing() {
        let mut stream = DerivedObservations::default();
        let tick = || vec![observed("wired", 0, "inferred")];

        assert_eq!(
            stream.changed(tick()).len(),
            1,
            "the first tick says everything"
        );
        assert!(
            stream.changed(tick()).is_empty(),
            "an inferred edge never changes on its own"
        );
    }

    #[test]
    fn freshness_and_status_both_count_as_change() {
        let mut stream = DerivedObservations::default();
        stream.changed(vec![observed("edge", 5, "confirmed")]);

        assert_eq!(
            stream.changed(vec![observed("edge", 9, "confirmed")]).len(),
            1
        );
        assert_eq!(
            stream.changed(vec![observed("edge", 9, "inferred")]).len(),
            1
        );
    }

    #[test]
    fn an_edge_that_returns_is_sent_again() {
        let mut stream = DerivedObservations::default();
        stream.changed(vec![observed("edge", 0, "inferred")]);
        stream.changed(Vec::new());

        assert_eq!(
            stream.changed(vec![observed("edge", 0, "inferred")]).len(),
            1,
            "a client that saw the edge removed needs its status again"
        );
    }
}
