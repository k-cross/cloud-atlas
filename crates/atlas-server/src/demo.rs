use atlas_engine::fixtures;
use atlas_graph::collection::{CollectionReport, CollectionSource, FailureKind};
use atlas_graph::flow::FlowObservation;
use atlas_graph::graph_builder::GraphBuilder;
use std::time::Duration;

pub const POLL: Duration = Duration::from_secs(5);
const FLOW_TTL_TICKS: u32 = 3;
const BURST_TICK: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Steady,
    Launched,
    Serving,
    Quiet,
    Outage,
}

// Steady outlasts the flow TTL so the traffic web-04 saw before it terminated
// lapses before it relaunches: confirmation is keyed by address, and a flow
// still current when its address gets a new holder confirms the new holder.
// Quiet outlasts it by two ticks so web-01's balancer edge is seen back at
// `inferred` before the outage starts, and the outage stays inside the default
// retention budget so Azure is held rather than released.
const SCRIPT: [(Stage, u64); 5] = [
    (Stage::Steady, FLOW_TTL_TICKS as u64 + 1),
    (Stage::Launched, 2),
    (Stage::Serving, 2),
    (Stage::Quiet, FLOW_TTL_TICKS as u64 + 2),
    (Stage::Outage, 3),
];

pub const CYCLE: u64 = {
    let mut total = 0;
    let mut i = 0;
    while i < SCRIPT.len() {
        total += SCRIPT[i].1;
        i += 1;
    }
    total
};

impl Stage {
    fn web_04_running(self) -> bool {
        self != Stage::Steady
    }

    fn web_04_serving(self) -> bool {
        matches!(self, Stage::Serving | Stage::Quiet | Stage::Outage)
    }

    fn web_01_serving(self) -> bool {
        matches!(self, Stage::Steady | Stage::Launched | Stage::Serving)
    }

    fn azure_readable(self) -> bool {
        self != Stage::Outage
    }
}

pub fn stage(tick: u64) -> Stage {
    let mut at = tick % CYCLE;
    for (stage, ticks) in SCRIPT {
        if at < ticks {
            return stage;
        }
        at -= ticks;
    }
    unreachable!("CYCLE is the sum of the script's stages")
}

pub fn scan(tick: u64) -> (GraphBuilder, CollectionReport) {
    let stage = stage(tick);
    let aws = if stage.web_04_running() {
        fixtures::with_web_04(fixtures::aws())
    } else {
        fixtures::aws()
    };

    let mut report = CollectionReport::default();
    let mut providers = vec![aws, fixtures::gcp()];
    if stage.azure_readable() {
        providers.push(fixtures::azure());
    } else {
        report.note(
            CollectionSource::Azure,
            FailureKind::Unavailable,
            "resource_graph",
            "503 Service Unavailable (scripted demo outage)",
        );
    }
    providers.push(fixtures::cloudflare());

    (fixtures::topology_of(providers), report)
}

#[cfg(test)]
pub fn graph(tick: u64) -> GraphBuilder {
    scan(tick).0
}

#[cfg(test)]
pub fn first_tick(wanted: Stage) -> u64 {
    (0..CYCLE)
        .find(|&tick| stage(tick) == wanted)
        .expect("every stage is in the script")
}

pub fn observations(tick: u64) -> Vec<FlowObservation> {
    let stage = stage(tick);
    let mut flows = fixtures::flows();
    if !stage.web_01_serving() {
        let quiet = fixtures::web_01_addresses();
        flows.retain(|flow| !quiet.contains(&flow.src) && !quiet.contains(&flow.dst));
    }
    if stage.web_04_serving() {
        flows.extend(fixtures::web_04_flows());
    }
    if tick % CYCLE == BURST_TICK {
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
    use atlas_graph::definition::{Edge, Node};
    use atlas_graph::derive;
    use atlas_graph::export::{RenderObservation, edge_key, node_key};
    use atlas_graph::flow::FlowIndex;
    use atlas_graph::patch::{GraphPatch, Retention, carry_forward, diff};
    use petgraph::visit::EdgeRef;

    const ALB: &str = "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/globex/1";

    fn serves(target: &str) -> String {
        edge_key(
            &node_key(&Node::AwsElbLoadBalancer(ALB.into())),
            &node_key(&Node::AwsEc2Instance(target.into())),
            &Edge::Serves,
        )
    }

    struct Tick {
        stage: Stage,
        patch: GraphPatch,
        derived: Vec<RenderObservation>,
        held: Vec<CollectionSource>,
    }

    impl Tick {
        fn status(&self, key: &str) -> Option<&'static str> {
            self.derived
                .iter()
                .find(|observation| observation.key == key)
                .map(|observation| observation.status)
        }
    }

    // The server's reconcile loop on a clock that advances exactly one poll per
    // tick, so when a flow lapses is decided by the script, not by scheduling.
    fn play(ticks: u64) -> Vec<Tick> {
        let poll_ms = POLL.as_millis() as i64;
        let mut flows = FlowIndex::new(flow_ttl(POLL), FlowIndex::DEFAULT_CAPACITY);
        let mut retention = Retention::default();
        for mut observation in observations(0) {
            observation.observed_at = 0;
            flows.observe(&observation);
        }
        let mut live = graph(0);
        flows.overlay(&mut live);
        derive::all(&mut live, &flows);

        (1..=ticks)
            .map(|tick| {
                let now = tick as i64 * poll_ms;
                let (mut next, report) = scan(tick);
                let held = retention.hold(&report);
                for mut observation in observations(tick) {
                    observation.observed_at = now;
                    flows.observe(&observation);
                }
                flows.expire(now);

                carry_forward(&mut next, &live.graph, &held);
                flows.overlay(&mut next);
                derive::all(&mut next, &flows);
                let patch = diff(&live.graph, &next.graph);
                let derived = derive::derived_observations(&next.graph, &flows);
                live = next;

                Tick {
                    stage: stage(tick),
                    patch,
                    derived,
                    held: held.into_iter().collect(),
                }
            })
            .collect()
    }

    #[test]
    fn the_script_runs_its_stages_in_order_and_repeats() {
        let order: Vec<Stage> = (0..CYCLE).map(stage).fold(Vec::new(), |mut seen, stage| {
            if seen.last() != Some(&stage) {
                seen.push(stage);
            }
            seen
        });
        assert_eq!(
            order,
            vec![
                Stage::Steady,
                Stage::Launched,
                Stage::Serving,
                Stage::Quiet,
                Stage::Outage
            ]
        );
        assert_eq!(stage(CYCLE), Stage::Steady);
        assert_eq!(stage(CYCLE + first_tick(Stage::Outage)), Stage::Outage);
    }

    #[test]
    fn a_launched_instance_is_wired_before_it_is_used() {
        let web_04 = serves(fixtures::WEB_04);

        for (index, tick) in play(3 * CYCLE).iter().enumerate() {
            let expected = match tick.stage {
                Stage::Launched => "inferred",
                Stage::Serving => "confirmed",
                _ => continue,
            };
            assert_eq!(
                tick.status(&web_04),
                Some(expected),
                "tick {}: a relaunch must not inherit the traffic its previous life saw",
                index + 1
            );
        }
    }

    #[test]
    fn a_quiet_target_falls_back_to_its_wiring() {
        let ticks = play(CYCLE);
        let web_01 = serves("i-globex-web-01");

        for tick in &ticks {
            if tick.stage == Stage::Steady {
                assert_eq!(tick.status(&web_01), Some("confirmed"));
            }
        }
        let last_quiet = ticks
            .iter()
            .rfind(|tick| tick.stage == Stage::Quiet)
            .expect("the script has a quiet stage");
        assert_eq!(
            last_quiet.status(&web_01),
            Some("inferred"),
            "still registered, so the edge stays, unconfirmed"
        );
    }

    #[test]
    fn an_outage_is_carried_without_changing_the_graph() {
        let ticks = play(CYCLE);
        let outage: Vec<&Tick> = ticks
            .iter()
            .filter(|tick| tick.stage == Stage::Outage)
            .collect();

        assert!(!outage.is_empty());
        for tick in outage {
            assert_eq!(tick.held, vec![CollectionSource::Azure]);
            assert!(
                tick.patch.removed_nodes.is_empty() && tick.patch.removed_edges.is_empty(),
                "Azure is unreadable, not gone: {} nodes and {} edges removed",
                tick.patch.removed_nodes.len(),
                tick.patch.removed_edges.len()
            );
            assert!(
                tick.patch.is_empty(),
                "nothing else moves during the outage, so the graph must not either"
            );
        }
    }

    #[test]
    fn recovery_terminates_the_launched_instance_and_its_edges() {
        let ticks = play(CYCLE);
        let recovered = ticks.last().expect("the tick that starts the next cycle");
        assert_eq!(recovered.stage, Stage::Steady);

        let instance = node_key(&Node::AwsEc2Instance(fixtures::WEB_04.into()));
        assert!(recovered.patch.removed_nodes.contains(&instance));
        assert!(
            recovered
                .patch
                .removed_edges
                .contains(&serves(fixtures::WEB_04))
        );
        assert!(
            recovered.held.is_empty(),
            "Azure answered, so nothing is held"
        );
        assert_eq!(
            recovered.status(&serves("i-globex-web-01")),
            Some("confirmed"),
            "web-01's traffic is back"
        );
    }

    #[test]
    fn the_graph_changes_at_least_once_every_few_ticks() {
        let ticks = play(2 * CYCLE);
        let mut still = 0;
        for tick in &ticks {
            still = if tick.patch.is_empty() { still + 1 } else { 0 };
            assert!(
                still <= 5,
                "the graph stood still for {still} ticks, longer than a watcher waits"
            );
        }
    }

    #[test]
    fn the_live_change_is_a_shape_a_scan_produces() {
        for tick in 0..CYCLE {
            let graph = graph(tick).graph;
            assert!(
                graph
                    .edge_references()
                    .filter(|e| e.weight().is_projected())
                    .all(|e| graph[e.source()].owner().is_some()
                        || graph[e.target()].owner().is_some()),
                "a projected edge between two pivots would outlive any provider's outage"
            );
        }

        let launched = graph(first_tick(Stage::Launched)).graph;
        assert!(
            launched
                .edge_references()
                .any(|e| e.weight() == &Edge::ConnectsTo
                    && launched[e.target()] == Node::AwsEc2Instance(fixtures::WEB_04.into())),
            "the target group reaches web-04 the way it reaches any registered target"
        );
    }

    #[test]
    fn the_scan_graph_carries_no_traffic_of_its_own() {
        for tick in 0..CYCLE {
            assert!(
                !graph(tick)
                    .graph
                    .edge_weights()
                    .any(|e| matches!(e, Edge::TrafficFlow)),
                "traffic must reach the demo graph through the flow index, or it can never lapse"
            );
        }
    }

    #[test]
    fn traffic_bursts_once_a_cycle() {
        let burst = fixtures::burst_flow().src;
        let bursting: Vec<u64> = (0..2 * CYCLE)
            .filter(|&tick| observations(tick).iter().any(|o| o.src == burst))
            .collect();
        assert_eq!(bursting, vec![BURST_TICK, CYCLE + BURST_TICK]);
        assert!(
            stage(BURST_TICK).web_01_serving(),
            "the burst targets web-01, so it lands while web-01 is not scripted quiet"
        );
    }

    #[test]
    fn a_burst_flow_goes_quiet_for_longer_than_it_stays_current() {
        let poll = Duration::from_secs(60);
        assert!(flow_ttl(poll) < poll * (CYCLE as u32));
    }
}
