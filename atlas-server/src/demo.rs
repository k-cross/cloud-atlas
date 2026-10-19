use atlas_lib::atlas::flow::FlowObservation;
use atlas_lib::atlas::graph_builder::GraphBuilder;
use atlas_lib::atlas::projector;
use atlas_lib::fixtures;
use std::time::Duration;

pub const POLL: Duration = Duration::from_secs(5);
const BURST_PERIOD: u64 = 4;
const FLOW_TTL_TICKS: u32 = 3;

pub fn graph(tick: u64) -> GraphBuilder {
    let mut builder = fixtures::topology();
    if tick % 2 == 1 {
        projector::build(&mut builder, &fixtures::canary(), &fixtures::settings());
    }
    builder
}

pub fn observations(tick: u64) -> Vec<FlowObservation> {
    let mut flows = fixtures::flows();
    if tick % BURST_PERIOD == 1 {
        flows.push(fixtures::burst_flow());
    }
    flows
}

pub fn flow_ttl(poll: Duration) -> Duration {
    poll * FLOW_TTL_TICKS
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_lib::atlas::definition::Edge;
    use atlas_lib::atlas::patch::diff;
    use petgraph::visit::EdgeRef;

    #[test]
    fn the_canary_record_toggles_by_parity() {
        let even = graph(2).graph;
        let odd = graph(3).graph;
        assert_eq!(odd.node_count(), even.node_count() + 3);

        let added = diff(&even, &odd);
        assert_eq!(
            added.added_nodes.len(),
            3,
            "the record, its name and its answer"
        );
        assert_eq!(added.added_edges.len(), 3);
        assert!(added.removed_nodes.is_empty() && added.removed_edges.is_empty());

        let removed = diff(&odd, &even);
        assert_eq!(removed.removed_nodes.len(), 3);
        assert_eq!(removed.removed_edges.len(), 3);
        assert!(removed.added_nodes.is_empty() && removed.added_edges.is_empty());
    }

    #[test]
    fn the_live_change_is_a_shape_a_scan_produces() {
        let graph = graph(1).graph;

        assert!(
            graph
                .edge_references()
                .filter(|e| e.weight().is_projected())
                .all(|e| graph[e.source()].owner().is_some()
                    || graph[e.target()].owner().is_some()),
            "a projected edge between two pivots would outlive any provider's outage"
        );
    }

    #[test]
    fn the_scan_graph_carries_no_traffic_of_its_own() {
        assert!(
            !graph(1)
                .graph
                .edge_weights()
                .any(|e| matches!(e, Edge::TrafficFlow)),
            "traffic must reach the demo graph through the flow index, or it can never lapse"
        );
    }

    #[test]
    fn traffic_bursts_on_one_tick_in_four() {
        let steady = fixtures::flows().len();
        assert_eq!(observations(1).len(), steady + 1);
        for tick in 2..=4 {
            assert_eq!(observations(tick).len(), steady);
        }
        assert_eq!(observations(5).len(), steady + 1);
    }

    #[test]
    fn a_burst_flow_goes_quiet_for_longer_than_it_stays_current() {
        let poll = Duration::from_secs(60);
        assert!(flow_ttl(poll) < poll * (BURST_PERIOD as u32));
    }
}
