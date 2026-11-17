use crate::state::AppState;
use atlas_graph::collection::CollectionReport;
use atlas_graph::patch::UnreadableSource;
use serde_json::{Value, json};

pub fn value(
    report: &CollectionReport,
    unreadable_sources: &[UnreadableSource],
    stream: &CollectionReport,
    flows: &CollectionReport,
    observed_flows: usize,
) -> Value {
    let mut unreadable: Vec<String> = report
        .unreadable_sources()
        .iter()
        .map(|source| source.to_string())
        .collect();
    unreadable.sort();

    let retention: Vec<Value> = unreadable_sources
        .iter()
        .map(|source| {
            json!({
                "source": source.source,
                "kind": source.kind,
                "scans": source.scans,
                "budget": source.budget,
                "held": source.held(),
            })
        })
        .collect();

    json!({
        "complete": report.is_complete(),
        "unreadable": unreadable,
        "retention": retention,
        "failures": report.failures,

        "stream": {
            "healthy": stream.is_complete(),
            "failures": stream.failures,
        },

        "flows": {
            "healthy": flows.is_complete(),
            "failures": flows.failures,
            "observed": observed_flows,
        },
    })
}

async fn current(state: &AppState) -> Value {
    let report = state.report.read().await;
    let unreadable = state.unreadable.read().await;
    let stream = state.stream_report.read().await;
    let flows = state.flow_report.read().await;
    let observed = state.flows.read().await;
    value(&report, &unreadable, &stream, &flows, observed.flow_count())
}

pub async fn publish(state: &AppState) {
    let latest = current(state).await;
    state.health.send_if_modified(|sent| {
        if *sent == latest {
            return false;
        }
        *sent = latest;
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_graph::collection::{CollectionSource, FailureKind};
    use atlas_graph::flow::FlowIndex;
    use atlas_graph::graph_builder::GraphBuilder;
    use atlas_graph::patch::Retention;

    fn clean() -> CollectionReport {
        CollectionReport::default()
    }

    fn azure_down() -> CollectionReport {
        let mut report = CollectionReport::default();
        report.record(
            CollectionSource::Azure,
            FailureKind::Unavailable,
            "resource_graph",
            "503",
        );
        report
    }

    #[test]
    fn a_partial_scan_reports_its_attributed_failures() {
        let mut report = CollectionReport::default();
        report.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/ec2",
            "throttled",
        );

        let value = value(&report, &[], &clean(), &clean(), 0);

        assert_eq!(value["complete"].as_bool(), Some(false));
        let failures = value["failures"].as_array().expect("failures array");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0]["source"].as_str(), Some("aws"));
        assert_eq!(failures[0]["scope"].as_str(), Some("us-east-1/ec2"));
    }

    #[test]
    fn a_clean_scan_is_reported_as_complete() {
        let value = value(&clean(), &[], &clean(), &clean(), 0);

        assert_eq!(value["complete"].as_bool(), Some(true));
        assert!(value["failures"].as_array().expect("array").is_empty());
        assert!(value["retention"].as_array().expect("array").is_empty());
        assert_eq!(value["stream"]["healthy"].as_bool(), Some(true));
    }

    #[test]
    fn a_degraded_event_feed_does_not_make_the_scan_incomplete() {
        let mut stream = CollectionReport::default();
        stream.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/events",
            "queue unreachable",
        );

        let value = value(&clean(), &[], &stream, &clean(), 0);

        assert_eq!(value["complete"].as_bool(), Some(true));
        assert!(value["unreadable"].as_array().expect("array").is_empty());
        assert_eq!(value["stream"]["healthy"].as_bool(), Some(false));
        assert_eq!(
            value["stream"]["failures"].as_array().expect("array").len(),
            1
        );
    }

    #[test]
    fn a_degraded_flow_feed_is_reported_apart_from_both_others() {
        let mut flows = CollectionReport::default();
        flows.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/flow-logs",
            "bucket unreachable",
        );

        let value = value(&clean(), &[], &clean(), &flows, 0);

        assert_eq!(value["complete"].as_bool(), Some(true));
        assert!(value["unreadable"].as_array().expect("array").is_empty());
        assert_eq!(value["stream"]["healthy"].as_bool(), Some(true));
        assert_eq!(value["flows"]["healthy"].as_bool(), Some(false));
        assert_eq!(value["flows"]["observed"].as_u64(), Some(0));
    }

    #[test]
    fn an_unreadable_source_reports_how_long_it_has_been_held() {
        let report = azure_down();
        let mut retention = Retention::new(2);
        let mut held = Vec::new();
        for _ in 0..3 {
            retention.hold(&report);
            let value = value(
                &report,
                &retention.unreadable(&report),
                &clean(),
                &clean(),
                0,
            );
            held.push(value["retention"][0].clone());
        }

        assert_eq!(held[0]["source"].as_str(), Some("azure"));
        assert_eq!(held[0]["kind"].as_str(), Some("unavailable"));
        let progress: Vec<(u64, u64, bool)> = held
            .iter()
            .map(|entry| {
                (
                    entry["scans"].as_u64().expect("scans"),
                    entry["budget"].as_u64().expect("budget"),
                    entry["held"].as_bool().expect("held"),
                )
            })
            .collect();
        assert_eq!(
            progress,
            vec![(1, 2, true), (2, 2, true), (3, 2, false)],
            "past its budget a source is reported as released, not still held"
        );
    }

    #[tokio::test]
    async fn health_is_published_only_when_it_changes() {
        let state = AppState::new(GraphBuilder::new(), clean(), FlowIndex::default());
        let mut health = state.health.subscribe();
        health.borrow_and_update();

        publish(&state).await;
        assert!(
            !health.has_changed().expect("open"),
            "nothing moved, so nothing is news"
        );

        *state.report.write().await = azure_down();
        publish(&state).await;
        assert!(health.has_changed().expect("open"));
        assert_eq!(
            health.borrow_and_update()["unreadable"][0].as_str(),
            Some("Azure")
        );
    }
}
