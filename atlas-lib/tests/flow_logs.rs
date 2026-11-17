use atlas_aws::flow_logs::stream::FlowLogQueue;
use atlas_aws::flow_logs::{InterfaceNetworks, Parsed, parse};
use atlas_graph::definition::{Edge, Node};
use atlas_graph::flow::FlowIndex;
use atlas_graph::graph_builder::GraphBuilder;
use atlas_lib::atlas::projector;
use atlas_lib::{Settings, fixtures};
use aws_credential_types::Credentials;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use std::io::Write;

const SCOPE: &str = "us-east-1";
const NETWORK: &str = "vpc-flows";

fn networks() -> InterfaceNetworks {
    let mut builder = GraphBuilder::new();
    let eni = builder.get_or_add_node(Node::AwsEc2Eni("eni-0abc".into()));
    builder.link_to(eni, Node::ip("10.0.0.99", NETWORK), Edge::ConnectsTo);
    InterfaceNetworks::read(&builder.graph)
}

fn parsed(text: &str) -> Parsed {
    parse(text, SCOPE, usize::MAX, &networks())
}
const BUCKET: &str = "globex-flow-logs";
const KEY: &str = "AWSLogs/111111111111/vpcflowlogs/us-east-1/2026/09/04/flows.log.gz";

fn default_record(src: &str, dst: &str, action: &str) -> String {
    record_at("eni-0abc", src, dst, action)
}

fn record_at(interface: &str, src: &str, dst: &str, action: &str) -> String {
    format!(
        "2 111111111111 {interface} {src} {dst} 51234 443 6 24 4800 1788436740 1788436800 {action} OK"
    )
}

#[test]
fn an_unknown_interface_earns_no_node() {
    let Parsed { observations, .. } =
        parsed(&default_record("10.10.1.10", "198.51.100.10", "ACCEPT"));
    let mut index = FlowIndex::default();
    index.observe(&observations[0]);

    let mut graph = GraphBuilder::new();
    projector::build(&mut graph, &fixtures::aws(), &fixture_settings());
    index.overlay(&mut graph);

    assert!(!graph.contains(&Node::AwsEc2Eni("eni-0abc".into())));
}

#[test]
fn parsed_flows_become_traffic_edges_over_the_scanned_topology() {
    let mut graph = fixture_topology();
    let Parsed { observations, .. } = parse(
        &record_at(
            "eni-globex-web-01a",
            "10.10.1.10",
            "198.51.100.10",
            "ACCEPT",
        ),
        SCOPE,
        usize::MAX,
        &InterfaceNetworks::read(&graph.graph),
    );

    let mut index = FlowIndex::default();
    for observation in &observations {
        index.observe(observation);
    }
    index.overlay(&mut graph);

    let held = Node::ip("10.10.1.10", fixtures::AWS_NETWORK);
    assert!(graph.has_edge(
        &Node::AwsEc2Eni("eni-globex-web-01a".into()),
        &held,
        &Edge::ConnectsTo
    ));
    assert!(
        graph.has_edge(
            &held,
            &Node::public_ip("198.51.100.10").unwrap(),
            &Edge::TrafficFlow
        ),
        "the record lands on the address its interface holds"
    );
}

#[tokio::test]
async fn a_queued_object_reaches_the_graph_as_traffic_edges() {
    let object = format!(
        "{}\n{}",
        record_at(
            "eni-globex-web-01a",
            "10.10.1.10",
            "198.51.100.10",
            "ACCEPT"
        ),
        record_at("eni-globex-web-02a", "10.10.1.11", "203.0.113.7", "REJECT"),
    );
    let http = replay(vec![
        (OK, received(&[notification(BUCKET, KEY)]).into_bytes()),
        (OK, gzip(&object)),
        (OK, br#"{"Successful":[{"Id":"0"}]}"#.to_vec()),
    ]);
    let config = replay_config(http).await;
    let mut graph = fixture_topology();

    let batch = flow_queue(&config)
        .receive(&InterfaceNetworks::read(&graph.graph))
        .await;

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
    index.overlay(&mut graph);

    assert!(graph.has_edge(
        &Node::ip("10.10.1.10", fixtures::AWS_NETWORK),
        &Node::public_ip("198.51.100.10").unwrap(),
        &Edge::TrafficFlow
    ));
    assert!(
        graph.contains(&Node::public_ip("203.0.113.7").unwrap()),
        "an address no scan reported still earns the generic pivot node"
    );
}

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

const OK: u16 = 200;

fn fixture_settings() -> Settings {
    fixtures::settings()
}

fn fixture_topology() -> GraphBuilder {
    let mut graph = GraphBuilder::new();
    projector::build(&mut graph, &fixtures::aws(), &fixture_settings());
    graph
}

fn gzip(text: &str) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(text.as_bytes()).expect("gzip");
    encoder.finish().expect("gzip")
}

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
