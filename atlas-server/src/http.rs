//! HTTP surface: the one-shot snapshot (also handy for `curl`/static viewers)
//! and the WebSocket upgrade. CORS is permissive so the bun dev server on a
//! different port can talk to us during development.

use crate::state::AppState;
use crate::ws;
use atlas_lib::atlas::collection::CollectionReport;
use atlas_lib::atlas::export::render_snapshot_with;
use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use tower_http::cors::CorsLayer;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/snapshot.json", get(snapshot))
        .route("/collection.json", get(collection))
        .route("/ws", get(ws::handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn snapshot(State(state): State<AppState>) -> impl IntoResponse {
    let live = state.live.read().await;
    let flows = state.flows.read().await;
    Json(render_snapshot_with(&live.graph, flows.observations()))
}

/// How much of the last scan is actually trustworthy. The snapshot alone cannot
/// say whether a provider is absent because it holds nothing or because it
/// could not be reached, and that distinction matters most right after start-up,
/// when an outage makes the very first collection the baseline.
async fn collection(State(state): State<AppState>) -> impl IntoResponse {
    let report = state.report.read().await;
    let stream = state.stream_report.read().await;
    let flows = state.flow_report.read().await;
    let observed = state.flows.read().await;
    Json(collection_value(
        &report,
        &stream,
        &flows,
        observed.flow_count(),
    ))
}

fn collection_value(
    report: &CollectionReport,
    stream: &CollectionReport,
    flows: &CollectionReport,
    observed_flows: usize,
) -> serde_json::Value {
    // `complete` and `unreadable` are not the same question. A scan that read
    // every provider but could not map one drifted row is incomplete — the
    // client lost something — yet still authoritative about what exists, so
    // nothing is held. Only `unreadable` suspends removals.
    let mut unreadable: Vec<String> = report
        .unreadable_sources()
        .iter()
        .map(|source| source.to_string())
        .collect();
    unreadable.sort();

    serde_json::json!({
        "complete": report.is_complete(),
        "unreadable": unreadable,
        "failures": report.failures,
        // The Tier-1 feed's health, reported separately because it answers a
        // different question. A degraded event stream means changes reach the
        // graph at the poll interval instead of in seconds; it does *not* make
        // the scan any less authoritative, and must never be read as a reason
        // to suspend removals.
        "stream": {
            "healthy": stream.is_complete(),
            "failures": stream.failures,
        },
        // The Tier-2 feed, separate again for the same reason. A flow feed we
        // cannot read leaves the topology entirely correct and only the
        // liveness stale — so `observed` going to zero while `healthy` is false
        // means "we stopped looking", not "the network went quiet".
        "flows": {
            "healthy": flows.is_complete(),
            "failures": flows.failures,
            "observed": observed_flows,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use atlas_lib::atlas::collection::{CollectionSource, FailureKind};

    fn clean() -> CollectionReport {
        CollectionReport::default()
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

        let value = collection_value(&report, &CollectionReport::default(), &clean(), 0);

        assert_eq!(value["complete"].as_bool(), Some(false));
        let failures = value["failures"].as_array().expect("failures array");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0]["source"].as_str(), Some("aws"));
        assert_eq!(failures[0]["scope"].as_str(), Some("us-east-1/ec2"));
    }

    #[test]
    fn a_clean_scan_is_reported_as_complete() {
        let value = collection_value(&clean(), &clean(), &clean(), 0);

        assert_eq!(value["complete"].as_bool(), Some(true));
        assert!(value["failures"].as_array().expect("array").is_empty());
        assert_eq!(value["stream"]["healthy"].as_bool(), Some(true));
    }

    /// A dead event feed and an unreadable provider are different problems with
    /// different consequences, and a client has to be able to tell them apart:
    /// one costs latency, the other suspends deletions.
    #[test]
    fn a_degraded_event_feed_does_not_make_the_scan_incomplete() {
        let mut stream = CollectionReport::default();
        stream.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/events",
            "queue unreachable",
        );

        let value = collection_value(&clean(), &stream, &clean(), 0);

        assert_eq!(value["complete"].as_bool(), Some(true));
        assert!(value["unreadable"].as_array().expect("array").is_empty());
        assert_eq!(value["stream"]["healthy"].as_bool(), Some(false));
        assert_eq!(
            value["stream"]["failures"].as_array().expect("array").len(),
            1
        );
    }

    /// A flow-log bucket we cannot reach leaves the graph entirely correct and
    /// only the liveness stale, so it must not read as a scan problem — and a
    /// client has to be able to tell it from a genuinely quiet network.
    #[test]
    fn a_degraded_flow_feed_is_reported_apart_from_both_others() {
        let mut flows = CollectionReport::default();
        flows.record(
            CollectionSource::Aws,
            FailureKind::Unavailable,
            "us-east-1/flow-logs",
            "bucket unreachable",
        );

        let value = collection_value(&clean(), &clean(), &flows, 0);

        assert_eq!(value["complete"].as_bool(), Some(true));
        assert!(value["unreadable"].as_array().expect("array").is_empty());
        assert_eq!(value["stream"]["healthy"].as_bool(), Some(true));
        assert_eq!(value["flows"]["healthy"].as_bool(), Some(false));
        assert_eq!(value["flows"]["observed"].as_u64(), Some(0));
    }
}
