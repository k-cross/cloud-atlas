use atlas_lib::atlas::definition::{Edge, Node};
use atlas_lib::atlas::flow::FlowObservation;
use atlas_lib::atlas::graph_builder::GraphBuilder;
use atlas_lib::fixtures;
use std::time::Duration;

const SENTINEL_HOSTNAME: &str = "live-demo.internal";
const SENTINEL_ADDRESS: &str = "198.51.100.42";
const BURST_PERIOD: u64 = 4;
const FLOW_TTL_TICKS: u32 = 3;

pub fn graph(tick: u64) -> GraphBuilder {
    let mut builder = fixtures::topology();
    if tick % 2 == 1 {
        let host = builder.get_or_add_node(Node::GenericHostname(SENTINEL_HOSTNAME.into()));
        let ip = builder.get_or_add_node(Node::GenericIpAddress(SENTINEL_ADDRESS.into()));
        builder.add_edge(host, ip, Edge::ResolvesTo);
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
    use atlas_lib::atlas::patch::diff;

    #[test]
    fn the_sentinel_toggles_by_parity() {
        let even = graph(2).graph;
        let odd = graph(3).graph;
        assert_eq!(odd.node_count(), even.node_count() + 2);

        let added = diff(&even, &odd);
        assert_eq!(added.added_nodes.len(), 2);
        assert_eq!(added.added_edges.len(), 1);
        assert!(added.removed_nodes.is_empty() && added.removed_edges.is_empty());

        let removed = diff(&odd, &even);
        assert_eq!(removed.removed_nodes.len(), 2);
        assert_eq!(removed.removed_edges.len(), 1);
        assert!(removed.added_nodes.is_empty() && removed.added_edges.is_empty());
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
