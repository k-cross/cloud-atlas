//! Tier-2 ingestion coverage: canned VPC Flow Log objects in, normalized
//! `FlowObservation`s out, and — for the transport — a replayed SQS + S3
//! conversation. No credentials, no network.
//!
//! The load-bearing assertions are the two the record format makes easy to get
//! wrong: that a *custom* field order is read from the object's own header
//! rather than assumed, and that the identities the adapter attributes are the
//! ones the projector keys the graph by.

use super::stream::{FlowLogQueue, decompress, objects};
use super::*;
use crate::Settings;
use crate::atlas::collection::CollectionReport;
use crate::atlas::definition::Edge;
use crate::atlas::flow::FlowIndex;
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::projector;
use aws_credential_types::Credentials;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use std::io::Write;

const SCOPE: &str = "us-east-1";

/// Parse with no record cap — every test here feeds a handful of lines, and
/// the cap has its own test.
fn parsed(text: &str) -> Parsed {
    parse(text, SCOPE, usize::MAX)
}
const BUCKET: &str = "globex-flow-logs";
const KEY: &str = "AWSLogs/111111111111/vpcflowlogs/us-east-1/2026/09/04/flows.log.gz";

/// The version-2 default: no header, and the field order AWS uses when nobody
/// chose one.
fn default_record(src: &str, dst: &str, action: &str) -> String {
    format!(
        "2 111111111111 eni-0abc {src} {dst} 51234 443 6 24 4800 1788436740 1788436800 {action} OK"
    )
}

#[test]
fn a_default_format_record_becomes_a_flow_between_its_endpoints() {
    let Parsed {
        observations,
        unusable,
        ..
    } = parsed(&default_record("10.10.1.10", "198.51.100.10", "ACCEPT"));

    assert_eq!(unusable, 0);
    let [flow] = &observations[..] else {
        panic!("expected one observation, got {}", observations.len());
    };
    assert_eq!(flow.src, Node::GenericIpAddress("10.10.1.10".into()));
    assert_eq!(flow.dst, Node::GenericIpAddress("198.51.100.10".into()));
    assert_eq!(flow.packets, 24);
    assert_eq!(flow.bytes, 4800);
    assert_eq!(flow.action, Some(FlowAction::Accepted));
    assert_eq!(
        flow.observed_at, 1_788_436_800_000,
        "the record's end second, in millis"
    );
}

#[test]
fn a_rejected_record_keeps_its_verdict() {
    let Parsed { observations, .. } =
        parsed(&default_record("10.10.1.11", "203.0.113.7", "REJECT"));
    assert_eq!(observations[0].action, Some(FlowAction::Rejected));
}

/// The whole reason the header is read rather than assumed. This layout puts
/// `bytes` where the default order puts `srcport`; parsing it against the
/// default would silently file a port number as a byte count and an address as
/// a protocol.
#[test]
fn a_custom_field_order_is_read_from_the_objects_own_header() {
    let object = "\
version srcaddr dstaddr bytes packets end action log-status
2 10.10.1.10 198.51.100.10 9000 40 1788436800 ACCEPT OK";

    let Parsed {
        observations,
        unusable,
        ..
    } = parsed(object);

    assert_eq!(unusable, 0);
    let flow = &observations[0];
    assert_eq!(flow.src, Node::GenericIpAddress("10.10.1.10".into()));
    assert_eq!(flow.dst, Node::GenericIpAddress("198.51.100.10".into()));
    assert_eq!(flow.bytes, 9000);
    assert_eq!(flow.packets, 40);
    assert_eq!(flow.observed_at, 1_788_436_800_000);
}

#[test]
fn a_header_is_not_mistaken_for_a_record() {
    let object = "\
version srcaddr dstaddr end
2 10.10.1.10 198.51.100.10 1788436800";

    let Parsed { observations, .. } = parsed(object);

    assert_eq!(observations.len(), 1, "the header line is not a flow");
}

/// A format chosen to minimise log volume may leave the verdict out. The
/// traffic still happened — that is the liveness signal — and calling it
/// accepted would claim something the record never said.
#[test]
fn a_format_without_a_verdict_still_yields_liveness() {
    let object = "\
version srcaddr dstaddr packets bytes end
2 10.10.1.10 198.51.100.10 40 9000 1788436800";

    let Parsed { observations, .. } = parsed(object);

    assert_eq!(observations[0].action, None);
    assert_eq!(observations[0].observed_at, 1_788_436_800_000);
}

/// `NODATA` means the interface had no traffic in the window and `SKIPDATA`
/// means AWS dropped records. Neither is evidence of a flow, and neither is a
/// parse problem — counting them either way would be wrong.
#[test]
fn records_with_no_data_are_neither_traffic_nor_failures() {
    let object = format!(
        "{}\n{}\n{}",
        "2 111111111111 eni-0abc - - - - - - - 1788436740 1788436800 - NODATA",
        "2 111111111111 eni-0abc - - - - - - - 1788436740 1788436800 - SKIPDATA",
        default_record("10.10.1.10", "198.51.100.10", "ACCEPT"),
    );

    let Parsed {
        observations,
        unusable,
        ..
    } = parsed(&object);

    assert_eq!(observations.len(), 1, "only the real flow");
    assert_eq!(unusable, 0, "an empty window is not a failure");
}

/// Both identities come straight off the record, and both have to be keyed the
/// way the projector keys them — freshness filed under a key no node carries is
/// freshness nobody can see.
#[test]
fn a_record_attributes_both_identities_the_way_the_projector_keys_them() {
    let object = "\
version srcaddr dstaddr end instance-id interface-id
2 10.10.1.10 198.51.100.10 1788436800 i-globex-web-01 eni-globex-web-01a";

    let Parsed { observations, .. } = parsed(object);

    let mut scanned = GraphBuilder::new();
    projector::build(&mut scanned, &crate::fixtures::aws(), &fixture_settings());

    for named in &observations[0].resources {
        assert!(
            scanned.contains(named),
            "{named} is not a node the full scan produces"
        );
    }
    assert!(
        observations[0]
            .resources
            .contains(&Node::AwsEc2Eni("eni-globex-web-01a".into()))
    );
    assert!(
        observations[0]
            .resources
            .contains(&Node::AwsEc2Instance("i-globex-web-01".into()))
    );
}

/// `interface-id` is the only identity in the *version-2 default* field set, so
/// it is the one every flow log carries without the operator opting into
/// anything. It is also the only one present for an interface that belongs to a
/// NAT gateway or a load balancer rather than an instance.
#[test]
fn the_default_field_set_still_attributes_through_the_interface_alone() {
    let Parsed { observations, .. } =
        parsed(&default_record("10.10.1.10", "198.51.100.10", "ACCEPT"));

    assert_eq!(
        observations[0].resources,
        vec![Node::AwsEc2Eni("eni-0abc".into())],
        "no instance-id column, but the interface is right there"
    );
}

/// Attribution is not creation: the overlay admits a typed resource only when a
/// scan already found it, so an interface the graph has never heard of buys
/// freshness on nothing rather than a node built from an id.
#[test]
fn an_unknown_interface_earns_no_node() {
    let Parsed { observations, .. } =
        parsed(&default_record("10.10.1.10", "198.51.100.10", "ACCEPT"));
    let mut index = FlowIndex::default();
    index.observe(&observations[0]);

    let mut graph = GraphBuilder::new();
    projector::build(&mut graph, &crate::fixtures::aws(), &fixture_settings());
    index.overlay(&mut graph);

    assert!(!graph.contains(&Node::AwsEc2Eni("eni-0abc".into())));
}

/// Without addresses there are no endpoints and without an end time there is no
/// freshness, so such an object yields nothing — and must say so rather than
/// looking like a quiet network.
#[test]
fn a_layout_that_cannot_place_a_flow_loses_the_whole_object_and_says_so() {
    let object = "\
version srcport dstport protocol
2 51234 443 6";

    let parsed = parsed(object);

    assert!(parsed.observations.is_empty());
    assert!(
        parsed.unusable_layout,
        "one cause and total loss, not a scattering of drifted lines"
    );
    assert_eq!(parsed.unusable, 0, "no individual record was at fault");
}

/// A busy VPC writes millions of records per window. The cap has to stop the
/// parser *allocating* them, not trim a list that has already been built — and
/// it still has to say how much went unread.
#[test]
fn records_past_the_cap_are_counted_but_never_materialized() {
    let object: String = (0..10)
        .map(|i| default_record(&format!("10.0.0.{i}"), "198.51.100.10", "ACCEPT"))
        .collect::<Vec<_>>()
        .join("\n");

    let parsed = parse(&object, SCOPE, 4);

    assert_eq!(parsed.observations.len(), 4);
    assert_eq!(parsed.dropped, 6);
    assert_eq!(parsed.unusable, 0);
}

/// The discriminator has to survive a header-less object whose first field is
/// not `version`. A twelve-digit account id is not a `u32`, so testing only
/// that would read the first record as a header, match no fields, and discard
/// the entire object as one unreadable line.
#[test]
fn a_headerless_record_starting_with_an_account_id_is_not_read_as_a_header() {
    let object = "111111111111 eni-0abc 10.10.1.10 198.51.100.10 1788436800";

    assert!(!Layout::looks_like_header(object));
}

#[test]
fn a_real_header_is_still_recognised() {
    assert!(Layout::looks_like_header(
        "version account-id interface-id srcaddr dstaddr end action log-status"
    ));
}

#[test]
fn a_truncated_line_is_dropped_and_counted() {
    let object = format!(
        "{}\n2 111111111111 eni-0abc 10.10.1.10",
        default_record("10.10.1.10", "198.51.100.10", "ACCEPT")
    );

    let Parsed {
        observations,
        unusable,
        ..
    } = parsed(&object);

    assert_eq!(observations.len(), 1);
    assert_eq!(unusable, 1);
}

/// End to end at the record level: the parsed flows become the `TrafficFlow`
/// edges a client renders, over the topology a full scan produced.
#[test]
fn parsed_flows_become_traffic_edges_over_the_scanned_topology() {
    let Parsed { observations, .. } =
        parsed(&default_record("10.10.1.10", "198.51.100.10", "ACCEPT"));

    let mut index = FlowIndex::default();
    for observation in &observations {
        index.observe(observation);
    }

    let mut graph = GraphBuilder::new();
    projector::build(&mut graph, &crate::fixtures::aws(), &fixture_settings());
    index.overlay(&mut graph);

    assert!(graph.has_edge(
        &Node::GenericIpAddress("10.10.1.10".into()),
        &Node::GenericIpAddress("198.51.100.10".into()),
        &Edge::TrafficFlow
    ));
}

// ---- transport --------------------------------------------------------------

fn notification(bucket: &str, key: &str) -> String {
    serde_json::json!({
        "Records": [{
            "eventSource": "aws:s3",
            "eventName": "ObjectCreated:Put",
            "s3": {
                "bucket": { "name": bucket },
                "object": { "key": key }
            }
        }]
    })
    .to_string()
}

#[test]
fn a_notification_names_the_object_it_points_at() {
    let mut report = CollectionReport::default();

    let found = objects(&notification(BUCKET, KEY), &mut report, SCOPE);

    assert_eq!(found, vec![(BUCKET.to_owned(), KEY.to_owned())]);
    assert!(report.is_complete());
}

/// S3 form-encodes keys in its notifications. Fetching the raw key would 404 on
/// every object whose prefix contains an encoded character — and flow-log keys
/// are built from account, region and timestamp, so it happens routinely.
#[test]
fn an_encoded_object_key_is_decoded_before_it_is_fetched() {
    let mut report = CollectionReport::default();

    let found = objects(
        &notification(BUCKET, "flow+logs/2026%3D09/flows.log.gz"),
        &mut report,
        SCOPE,
    );

    assert_eq!(found[0].1, "flow logs/2026=09/flows.log.gz");
}

#[test]
fn an_sns_wrapped_notification_is_unwrapped() {
    let wrapped = serde_json::json!({
        "Type": "Notification",
        "Message": notification(BUCKET, KEY),
    })
    .to_string();
    let mut report = CollectionReport::default();

    let found = objects(&wrapped, &mut report, SCOPE);

    assert_eq!(found.len(), 1);
    assert!(report.is_complete());
}

/// S3 posts this the moment a notification is configured. Reporting it would
/// put a permanent, meaningless failure in the feed's health.
#[test]
fn the_buckets_configuration_test_message_is_not_a_failure() {
    let body = r#"{"Service":"Amazon S3","Event":"s3:TestEvent","Bucket":"globex-flow-logs"}"#;
    let mut report = CollectionReport::default();

    let found = objects(body, &mut report, SCOPE);

    assert!(found.is_empty());
    assert!(report.is_complete(), "{}", report.summary());
}

/// A message that is not an S3 notification at all is a lost object, and losing
/// it silently is the thing that must not happen.
#[test]
fn an_unreadable_message_is_reported_as_malformed() {
    let mut report = CollectionReport::default();

    objects("{{ not json", &mut report, SCOPE);

    assert!(!report.is_complete());
    assert!(
        report.unreadable_sources().is_empty(),
        "the queue was read fine; a bad message must not suspend AWS removals"
    );
}

/// No limit worth hitting — the cap has its own test.
const ROOMY: u64 = 1 << 20;

#[test]
fn a_gzipped_object_is_decompressed() {
    let text = default_record("10.10.1.10", "198.51.100.10", "ACCEPT");

    let out = decompress(&gzip(&text), ROOMY).expect("decompresses");

    assert_eq!(out.text, text);
    assert!(!out.truncated);
}

#[test]
fn a_plain_text_object_is_read_as_is() {
    let text = default_record("10.10.1.10", "198.51.100.10", "ACCEPT");

    assert_eq!(
        decompress(text.as_bytes(), ROOMY).expect("reads").text,
        text
    );
}

/// The compressed size bounds nothing: how far a member expands is decided by
/// whoever wrote it, so a modest object can still ask for gigabytes of `String`.
/// The cap has to sit on the output, and a cut has to be reported — a partial
/// read that passed for a small object would silently lose every record after
/// the cut.
#[test]
fn decompression_stops_at_the_limit_and_reports_the_cut() {
    let text = "x".repeat(4096);

    let out = decompress(&gzip(&text), 1024).expect("decompresses");

    assert_eq!(out.text.len(), 1024);
    assert!(out.truncated);
}

/// Naming the third delivery format beats the "invalid utf-8" a raw decode
/// would produce, since the fix is a configuration change.
#[test]
fn a_parquet_object_says_what_is_wrong_with_it() {
    let error = decompress(b"PAR1\x00\x00", ROOMY).expect_err("not supported");

    assert!(error.contains("Parquet"), "{error}");
}

/// The whole Tier-2 path with nothing stubbed but the wire: a queue message
/// names an object, the object's records become observations, and the
/// observations become the traffic edges a client renders.
#[tokio::test]
async fn a_queued_object_reaches_the_graph_as_traffic_edges() {
    let object = format!(
        "{}\n{}",
        default_record("10.10.1.10", "198.51.100.10", "ACCEPT"),
        default_record("10.10.1.11", "203.0.113.7", "REJECT"),
    );
    let http = replay(vec![
        (OK, received(&[notification(BUCKET, KEY)]).into_bytes()),
        (OK, gzip(&object)),
        (OK, br#"{"Successful":[{"Id":"0"}]}"#.to_vec()),
    ]);
    let config = replay_config(http).await;

    let batch = flow_queue(&config).receive().await;

    assert!(
        batch.report.is_complete(),
        "clean drain: {}",
        batch.report.summary()
    );
    assert_eq!(batch.observations.len(), 2);

    let mut index = FlowIndex::default();
    for observation in &batch.observations {
        index.observe(observation);
    }
    let mut graph = GraphBuilder::new();
    projector::build(&mut graph, &crate::fixtures::aws(), &fixture_settings());
    index.overlay(&mut graph);

    assert!(graph.has_edge(
        &Node::GenericIpAddress("10.10.1.10".into()),
        &Node::GenericIpAddress("198.51.100.10".into()),
        &Edge::TrafficFlow
    ));
    assert!(
        graph.contains(&Node::GenericIpAddress("203.0.113.7".into())),
        "an address no scan reported still earns the generic pivot node"
    );
}

/// A queue we cannot read makes liveness stale, which is a real problem worth
/// reporting — but it is never a reason to stop trusting the *scan* about what
/// exists, so the caller keeps this report apart from the scan's.
#[tokio::test]
async fn an_unreachable_queue_is_reported_rather_than_read_as_silence() {
    let http = replay(vec![(403, br#"{"__type":"AccessDenied"}"#.to_vec())]);
    let config = replay_config(http).await;

    let batch = flow_queue(&config).receive().await;

    assert!(batch.observations.is_empty());
    assert!(!batch.report.is_complete());
    assert_eq!(
        batch.report.unreadable_kind(SOURCE),
        Some(crate::atlas::collection::FailureKind::Unauthorized),
        "a queue policy problem no amount of polling fixes"
    );
}

// ---- harness ----------------------------------------------------------------

const OK: u16 = 200;

fn fixture_settings() -> Settings {
    crate::fixtures::settings()
}

fn gzip(text: &str) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(text.as_bytes()).expect("gzip");
    encoder.finish().expect("gzip")
}

/// Canned responses handed back in order — one per request the SDK makes, which
/// here is receive-message, get-object, delete-message-batch.
fn replay(responses: Vec<(u16, Vec<u8>)>) -> StaticReplayClient {
    let events = responses
        .into_iter()
        .map(|(status, body)| {
            ReplayEvent::new(
                http::Request::builder()
                    .uri("https://example.amazonaws.com/")
                    .body(SdkBody::empty())
                    .unwrap(),
                http::Response::builder()
                    .status(status)
                    .body(SdkBody::from(body))
                    .unwrap(),
            )
        })
        .collect();
    StaticReplayClient::new(events)
}

async fn replay_config(http: StaticReplayClient) -> aws_config::SdkConfig {
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(Credentials::for_tests())
        .http_client(http)
        .load()
        .await
}

fn flow_queue(config: &aws_config::SdkConfig) -> FlowLogQueue {
    FlowLogQueue::new(
        config,
        "https://sqs.us-east-1.amazonaws.com/111111111111/atlas-flow-logs",
        "us-east-1",
    )
    .with_wait_seconds(0)
}

fn received(bodies: &[String]) -> String {
    let messages: Vec<_> = bodies
        .iter()
        .enumerate()
        .map(|(i, body)| {
            serde_json::json!({
                "MessageId": format!("m{i}"),
                "ReceiptHandle": format!("handle-{i}"),
                "Body": body,
            })
        })
        .collect();
    serde_json::json!({ "Messages": messages }).to_string()
}
