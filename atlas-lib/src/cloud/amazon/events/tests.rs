//! Tier-1 ingestion coverage: canned EventBridge bodies in, normalized
//! `ChangeEvent`s out, and — for the transport — a replayed SQS conversation.
//! No credentials, no network.
//!
//! The load-bearing assertion is not "did it parse". It is that what the event
//! path produces is a *subgraph of what the full scan produces* for the same
//! resource: an edge Tier 1 invents that Tier 3 does not know about is deleted
//! at every reconciliation and re-added by every event.

use super::stream::EventQueue;
use super::*;
use crate::Settings;
use crate::atlas::collection::{CollectionReport, FailureKind};
use crate::atlas::event::EventApplier;
use crate::atlas::export::{edge_key, node_key};
use crate::atlas::projector;
use crate::cloud::definition::{AmazonCollection, Provider};
use aws_credential_types::Credentials;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use petgraph::visit::EdgeRef;
use std::collections::HashSet;

const INCLUDE_UNKNOWN: bool = false;

/// A Config configuration item for a running instance, with the shape Config
/// actually delivers: a configuration snapshot *and* a relationships list that
/// restates part of it.
fn instance_config_item(status: &str) -> String {
    format!(
        r#"{{
          "version": "0",
          "id": "cfg-event-1",
          "detail-type": "Config Configuration Item Change Notification",
          "source": "aws.config",
          "account": "111111111111",
          "time": "2026-09-03T12:00:00Z",
          "region": "us-east-1",
          "detail": {{
            "messageType": "ConfigurationItemChangeNotification",
            "configurationItem": {{
              "configurationItemStatus": "{status}",
              "configurationItemCaptureTime": "2026-09-03T12:00:00.000Z",
              "resourceType": "AWS::EC2::Instance",
              "resourceId": "i-0abc123",
              "ARN": "arn:aws:ec2:us-east-1:111111111111:instance/i-0abc123",
              "awsRegion": "us-east-1",
              "availabilityZone": "us-east-1a",
              "relationships": [
                {{ "resourceType": "AWS::EC2::VPC", "resourceId": "vpc-globex" }},
                {{ "resourceType": "AWS::EC2::Subnet", "resourceId": "subnet-globex" }},
                {{ "resourceType": "AWS::EC2::SecurityGroup", "resourceId": "sg-globex" }}
              ],
              "configuration": {{
                "instanceId": "i-0abc123",
                "vpcId": "vpc-globex",
                "subnetId": "subnet-globex",
                "privateIpAddress": "10.0.1.20",
                "placement": {{ "availabilityZone": "us-east-1a" }},
                "securityGroups": [{{ "groupId": "sg-globex", "groupName": "web" }}],
                "tags": [{{ "key": "env", "value": "prod" }}]
              }}
            }}
          }}
        }}"#
    )
}

fn parse_ok(body: &str) -> Vec<ChangeEvent> {
    parse(body, INCLUDE_UNKNOWN).expect("body is a readable event")
}

/// The same instance, as the full-scan collector would hand it to the
/// projector.
fn scanned_instance_graph() -> Graph<Node, Edge> {
    use aws_sdk_ec2::types::{GroupIdentifier, Instance, Placement, Tag};

    let instance = Instance::builder()
        .instance_id("i-0abc123")
        .vpc_id("vpc-globex")
        .subnet_id("subnet-globex")
        .private_ip_address("10.0.1.20")
        .placement(Placement::builder().availability_zone("us-east-1a").build())
        .security_groups(GroupIdentifier::builder().group_id("sg-globex").build())
        .tags(Tag::builder().key("env").value("prod").build())
        .build();

    let mut builder = GraphBuilder::new();
    projector::build(
        &mut builder,
        &Provider::AWS(vec![(
            "us-east-1".to_owned(),
            AmazonCollection::AmazonInstances(vec![instance]),
        )]),
        &Settings::default(),
    );
    builder.graph
}

fn node_keys(graph: &Graph<Node, Edge>) -> HashSet<String> {
    graph.node_weights().map(node_key).collect()
}

fn edge_keys(graph: &Graph<Node, Edge>) -> HashSet<String> {
    graph
        .edge_references()
        .map(|e| {
            edge_key(
                &node_key(&graph[e.source()]),
                &node_key(&graph[e.target()]),
                e.weight(),
            )
        })
        .collect()
}

// ---- AWS Config -------------------------------------------------------------

/// The contract between the tiers. Anything Tier 1 asserts about an instance
/// must be something the full scan also asserts, or the two tiers spend forever
/// undoing each other.
#[test]
fn an_instance_event_produces_only_what_a_full_scan_would() {
    let events = parse_ok(&instance_config_item("OK"));
    let typed = events
        .iter()
        .find(|e| matches!(e.node, Node::AwsEc2Instance(_)))
        .expect("the typed instance event");

    let scanned = scanned_instance_graph();

    let extra_nodes: Vec<_> = node_keys(&typed.context)
        .difference(&node_keys(&scanned))
        .cloned()
        .collect();
    assert!(
        extra_nodes.is_empty(),
        "event path invented nodes the full scan does not produce: {extra_nodes:?}"
    );

    let extra_edges: Vec<_> = edge_keys(&typed.context)
        .difference(&edge_keys(&scanned))
        .cloned()
        .collect();
    assert!(
        extra_edges.is_empty(),
        "event path invented edges the full scan does not produce: {extra_edges:?}"
    );
}

/// And the other direction: the event has to be *worth* something. A create
/// that lands as a bare node with no VPC, subnet or ENI is not what Config
/// paid for.
#[test]
fn an_instance_event_carries_the_eni_pivot() {
    let events = parse_ok(&instance_config_item("ResourceDiscovered"));
    let typed = events
        .iter()
        .find(|e| matches!(e.node, Node::AwsEc2Instance(_)))
        .expect("the typed instance event");

    assert_eq!(typed.op, ChangeOp::Created);
    let keys = node_keys(&typed.context);
    for expected in [
        Node::AwsEc2Instance("i-0abc123".into()),
        Node::AwsEc2Eni("i-0abc123".into()),
        Node::AwsEc2Subnet("subnet-globex".into()),
        Node::AwsEc2Vpc("vpc-globex".into()),
        Node::AwsEc2SecurityGroup("sg-globex".into()),
        Node::AwsEc2AvailabilityZone("us-east-1a".into()),
        Node::GenericIpAddress("10.0.1.20".into()),
        Node::AwsTag {
            key: "env".into(),
            value: "prod".into(),
        },
    ] {
        assert!(keys.contains(&node_key(&expected)), "missing {expected}");
    }
    assert!(
        typed.context.edge_references().any(|e| {
            typed.context[e.source()] == Node::AwsEc2Eni("i-0abc123".into())
                && typed.context[e.target()] == Node::AwsEc2Subnet("subnet-globex".into())
                && e.weight() == &Edge::AttachedTo
        }),
        "the ENI -> Subnet pivot is the point of the instance projection"
    );
}

/// Config restates a resource's relations in two places and only sometimes
/// includes the configuration snapshot. Falling back to `relationships` is what
/// keeps a create attached rather than floating.
#[test]
fn relationships_stand_in_for_a_missing_configuration_snapshot() {
    let body = instance_config_item("OK");
    let stripped = body
        .split("\"configuration\": {")
        .next()
        .expect("split")
        .trim_end()
        .trim_end_matches(',')
        .to_owned()
        + "}}}";

    let events = parse(&stripped, INCLUDE_UNKNOWN).expect("still a readable event");
    let typed = events
        .iter()
        .find(|e| matches!(e.node, Node::AwsEc2Instance(_)))
        .expect("the typed instance event");

    let keys = node_keys(&typed.context);
    assert!(keys.contains(&node_key(&Node::AwsEc2Vpc("vpc-globex".into()))));
    assert!(keys.contains(&node_key(&Node::AwsEc2Subnet("subnet-globex".into()))));
    assert!(keys.contains(&node_key(&Node::AwsEc2SecurityGroup("sg-globex".into()))));
}

/// The full scan produces both a typed node and an AWS Config catch-all node
/// for an instance. A delete that only took the typed one would leave the
/// catch-all standing until reconciliation swept it.
#[test]
fn a_config_item_yields_both_the_typed_and_the_catch_all_node() {
    let events = parse_ok(&instance_config_item("ResourceDeleted"));

    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.op == ChangeOp::Deleted));
    assert!(events.iter().any(|e| matches!(
        &e.node,
        Node::AwsConfigResource { resource_type, id }
            if &**resource_type == "AWS::EC2::Instance" && &**id == "i-0abc123"
    )));
}

/// A type the full scan deliberately skips must not arrive by event only to be
/// deleted at the next reconciliation — the filter has to be the same one.
#[test]
fn a_resource_type_the_scan_skips_produces_no_catch_all_node() {
    let body = instance_config_item("OK").replace(
        r#""resourceType": "AWS::EC2::Instance""#,
        r#""resourceType": "AWS::EC2::NetworkAcl""#,
    );

    let events = parse(&body, INCLUDE_UNKNOWN).expect("readable");

    assert!(
        events.is_empty(),
        "AWS::EC2::NetworkAcl is filtered out by use_aws_resource and has no typed node"
    );
}

/// An ENI is keyed by its instance in the graph, not by its `eni-` id, so a
/// Config ENI item has no typed node it could produce without inventing one.
#[test]
fn an_eni_item_produces_no_typed_node() {
    let body = instance_config_item("OK")
        .replace(
            r#""resourceType": "AWS::EC2::Instance""#,
            r#""resourceType": "AWS::EC2::NetworkInterface""#,
        )
        .replace(
            r#""resourceId": "i-0abc123""#,
            r#""resourceId": "eni-0abc""#,
        );

    let events = parse(&body, INCLUDE_UNKNOWN).expect("readable");

    assert!(
        !events.iter().any(|e| matches!(e.node, Node::AwsEc2Eni(_))),
        "an ENI keyed by its own id would never match what the projector builds"
    );
}

#[test]
fn a_vpc_item_lands_inside_its_region() {
    let body = instance_config_item("ResourceDiscovered")
        .replace(
            r#""resourceType": "AWS::EC2::Instance""#,
            r#""resourceType": "AWS::EC2::VPC""#,
        )
        .replace(r#""resourceId": "i-0abc123""#, r#""resourceId": "vpc-new""#);

    let events = parse_ok(&body);
    let typed = events
        .iter()
        .find(|e| matches!(e.node, Node::AwsEc2Vpc(_)))
        .expect("typed vpc event");

    assert!(typed.context.edge_references().any(|e| {
        typed.context[e.source()] == Node::AwsRegion("us-east-1".into())
            && typed.context[e.target()] == Node::AwsEc2Vpc("vpc-new".into())
            && e.weight() == &Edge::Contains
    }));
}

/// Config saying "I am not recording this" is not a statement about whether the
/// resource exists, and must not be read as one.
#[test]
fn an_unrecorded_status_changes_nothing() {
    let events = parse_ok(&instance_config_item("ResourceNotRecorded"));
    assert!(events.is_empty());
}

#[test]
fn the_capture_time_orders_the_event_not_our_clock() {
    let events = parse_ok(&instance_config_item("OK"));
    // 2026-09-03T12:00:00Z
    assert_eq!(events[0].observed_at, 1_788_436_800_000);
}

// ---- EC2 instance state changes ---------------------------------------------

fn state_change(state: &str) -> String {
    format!(
        r#"{{
          "version": "0",
          "id": "ec2-state-1",
          "detail-type": "EC2 Instance State-change Notification",
          "source": "aws.ec2",
          "time": "2026-09-03T12:05:00Z",
          "region": "us-east-1",
          "detail": {{ "instance-id": "i-0abc123", "state": "{state}" }}
        }}"#
    )
}

#[test]
fn a_termination_deletes_the_instance() {
    let events = parse_ok(&state_change("terminated"));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].op, ChangeOp::Deleted);
    assert_eq!(events[0].node, Node::AwsEc2Instance("i-0abc123".into()));
}

/// A stopped instance still exists, still holds its ENI and its subnet.
/// Deleting it here would put the graph at odds with the very next scan.
#[test]
fn a_stopped_instance_is_not_a_deleted_instance() {
    for state in ["stopped", "stopping", "running", "pending"] {
        let events = parse_ok(&state_change(state));
        assert_eq!(
            events[0].op,
            ChangeOp::Created,
            "{state} must not read as a deletion"
        );
    }
}

// ---- CloudTrail -------------------------------------------------------------

fn cloud_trail(detail: &str) -> String {
    format!(
        r#"{{
          "version": "0",
          "id": "ct-1",
          "detail-type": "AWS API Call via CloudTrail",
          "source": "aws.ec2",
          "time": "2026-09-03T12:10:00Z",
          "region": "us-east-1",
          "detail": {detail}
        }}"#
    )
}

#[test]
fn run_instances_creates_every_instance_it_launched() {
    let body = cloud_trail(
        r#"{
          "eventName": "RunInstances",
          "eventTime": "2026-09-03T12:10:00Z",
          "awsRegion": "us-east-1",
          "eventSource": "ec2.amazonaws.com",
          "responseElements": {
            "instancesSet": { "items": [
              { "instanceId": "i-aaa", "vpcId": "vpc-globex", "subnetId": "subnet-globex",
                "privateIpAddress": "10.0.1.5",
                "placement": { "availabilityZone": "us-east-1a" },
                "groupSet": { "items": [{ "groupId": "sg-globex" }] } },
              { "instanceId": "i-bbb", "vpcId": "vpc-globex", "subnetId": "subnet-globex" }
            ] }
          }
        }"#,
    );

    let events = parse_ok(&body);

    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.op == ChangeOp::Created));
    let first = &events[0];
    assert!(
        node_keys(&first.context).contains(&node_key(&Node::AwsEc2Eni("i-aaa".into()))),
        "a launch reports placement, so it projects with the ENI pivot"
    );
}

#[test]
fn terminate_instances_deletes_from_the_request() {
    let body = cloud_trail(
        r#"{
          "eventName": "TerminateInstances",
          "eventTime": "2026-09-03T12:11:00Z",
          "awsRegion": "us-east-1",
          "requestParameters": { "instancesSet": { "items": [{ "instanceId": "i-aaa" }] } }
        }"#,
    );

    let events = parse_ok(&body);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].op, ChangeOp::Deleted);
    assert_eq!(events[0].node, Node::AwsEc2Instance("i-aaa".into()));
}

/// CloudTrail records attempts, not just successes. A rejected terminate that
/// deleted the instance from the graph would be a live twin lying about a
/// running machine.
#[test]
fn a_rejected_api_call_changes_nothing() {
    let body = cloud_trail(
        r#"{
          "eventName": "TerminateInstances",
          "eventTime": "2026-09-03T12:11:00Z",
          "awsRegion": "us-east-1",
          "errorCode": "Client.UnauthorizedOperation",
          "requestParameters": { "instancesSet": { "items": [{ "instanceId": "i-aaa" }] } }
        }"#,
    );

    assert!(parse_ok(&body).is_empty());
}

#[test]
fn create_subnet_lands_inside_the_vpc_the_response_named() {
    let body = cloud_trail(
        r#"{
          "eventName": "CreateSubnet",
          "eventTime": "2026-09-03T12:12:00Z",
          "awsRegion": "us-east-1",
          "responseElements": { "subnet": { "subnetId": "subnet-new", "vpcId": "vpc-globex" } }
        }"#,
    );

    let events = parse_ok(&body);

    assert_eq!(events[0].node, Node::AwsEc2Subnet("subnet-new".into()));
    assert!(events[0].context.edge_references().any(|e| {
        events[0].context[e.source()] == Node::AwsEc2Vpc("vpc-globex".into())
            && e.weight() == &Edge::Contains
    }));
}

#[test]
fn an_unmodelled_api_call_is_ignored_not_failed() {
    let body = cloud_trail(
        r#"{
          "eventName": "DescribeInstances",
          "eventTime": "2026-09-03T12:12:00Z",
          "awsRegion": "us-east-1"
        }"#,
    );

    assert!(parse_ok(&body).is_empty());
}

// ---- envelope handling ------------------------------------------------------

/// A rule may target SNS with SQS behind it, in which case the event arrives
/// as a string inside a notification envelope.
#[test]
fn an_sns_wrapped_event_is_unwrapped() {
    let inner = state_change("terminated");
    let wrapped = serde_json::json!({
        "Type": "Notification",
        "MessageId": "abc",
        "TopicArn": "arn:aws:sns:us-east-1:111111111111:atlas",
        "Message": inner,
    })
    .to_string();

    let events = parse_ok(&wrapped);

    assert_eq!(events[0].node, Node::AwsEc2Instance("i-0abc123".into()));
}

#[test]
fn an_unrecognised_detail_type_is_ignored_not_failed() {
    let body = r#"{"detail-type":"Trusted Advisor Check Item Refresh Notification",
                   "time":"2026-09-03T12:00:00Z","region":"us-east-1","detail":{}}"#;
    assert!(parse_ok(body).is_empty());
}

#[test]
fn a_body_that_is_not_an_event_is_an_error() {
    assert!(parse("not json at all", INCLUDE_UNKNOWN).is_err());
    assert!(parse(r#"{"hello":"world"}"#, INCLUDE_UNKNOWN).is_err());
}

// ---- transport --------------------------------------------------------------

fn replay(responses: &[(&'static str, String)]) -> StaticReplayClient {
    let events = responses
        .iter()
        .map(|(content_type, body)| {
            ReplayEvent::new(
                http::Request::builder()
                    .uri("https://sqs.us-east-1.amazonaws.com/")
                    .body(SdkBody::empty())
                    .unwrap(),
                http::Response::builder()
                    .status(200)
                    .header("content-type", *content_type)
                    .body(SdkBody::from(body.clone()))
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

const JSON10: &str = "application/x-amz-json-1.0";

fn queue(config: &aws_config::SdkConfig) -> EventQueue {
    EventQueue::new(
        config,
        "https://sqs.us-east-1.amazonaws.com/111111111111/atlas-events",
        "us-east-1",
        INCLUDE_UNKNOWN,
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

/// The whole Tier-1 path with nothing stubbed but the wire: a queue message
/// becomes a normalized event, the event becomes a graph mutation, and the
/// mutation becomes a patch a client could apply.
#[tokio::test]
async fn a_queued_event_reaches_the_graph_as_a_patch() {
    let http = replay(&[
        (
            JSON10,
            received(&[instance_config_item("ResourceDiscovered")]),
        ),
        (JSON10, r#"{"Successful":[{"Id":"0"}]}"#.to_owned()),
    ]);
    let config = replay_config(http).await;

    let batch = queue(&config).receive().await;

    assert!(
        batch.report.is_complete(),
        "clean drain: {}",
        batch.report.summary()
    );
    assert_eq!(
        batch.events.len(),
        2,
        "typed node plus the Config catch-all"
    );

    let mut live = GraphBuilder::new();
    let mut applier = EventApplier::new();
    let mut patch = crate::atlas::patch::GraphPatch::empty();
    for event in &batch.events {
        patch.extend(applier.apply(&mut live, event));
    }

    assert!(!patch.is_empty());
    assert!(live.contains(&Node::AwsEc2Instance("i-0abc123".into())));
    assert!(live.contains(&Node::AwsEc2Eni("i-0abc123".into())));
}

/// One poison message must not take the batch down with it, and must not go
/// unrecorded either — a silently dropped delete leaves a resource in the graph
/// with nothing to say why.
#[test]
fn an_unreadable_message_is_reported_as_malformed_not_unreadable() {
    // Exercised through the report the queue builds, without the SQS round
    // trip: what matters is the classification.
    let mut report = CollectionReport::default();
    report.note(
        SOURCE,
        FailureKind::Malformed,
        "us-east-1/events",
        "dropped an unreadable event",
    );

    assert!(!report.is_complete(), "the lost event must be visible");
    assert!(
        report.unreadable_sources().is_empty(),
        "the queue was read fine; a bad message must not suspend AWS removals"
    );
}

#[tokio::test]
async fn a_bad_message_does_not_stop_the_good_ones() {
    let http = replay(&[
        (
            JSON10,
            received(&["{{ not an event".to_owned(), state_change("terminated")]),
        ),
        (
            JSON10,
            r#"{"Successful":[{"Id":"0"},{"Id":"1"}]}"#.to_owned(),
        ),
    ]);
    let config = replay_config(http).await;

    let batch = queue(&config).receive().await;

    assert_eq!(batch.events.len(), 1, "the readable event still lands");
    assert_eq!(batch.report.failures.len(), 1);
    assert_eq!(batch.report.failures[0].kind, FailureKind::Malformed);
    assert!(batch.report.unreadable_sources().is_empty());
}

/// A queue we cannot read is a real read failure — the one case here that
/// should hold AWS's resources rather than let them be deleted.
#[tokio::test]
async fn a_refused_queue_is_reported_unauthorized() {
    let http = StaticReplayClient::new(vec![ReplayEvent::new(
        http::Request::builder()
            .uri("https://sqs.us-east-1.amazonaws.com/")
            .body(SdkBody::empty())
            .unwrap(),
        http::Response::builder()
            .status(403)
            .header("content-type", JSON10)
            .body(SdkBody::from(
                r#"{"__type":"AccessDeniedException","message":"denied"}"#,
            ))
            .unwrap(),
    )]);
    let config = replay_config(http).await;

    let batch = queue(&config).receive().await;

    assert!(batch.events.is_empty());
    assert_eq!(batch.report.failures.len(), 1);
    assert_eq!(
        batch.report.failures[0].kind,
        FailureKind::Unauthorized,
        "a 403 on the queue is not something polling fixes"
    );
}

/// A subnet or route table whose VPC the event did not name gets *no* edge.
/// These two look like the ELB/EKS/RDS arms and are not: the projector reaches
/// a subnet through `link_to(vpc_idx, ..)`, which emits nothing when the VPC is
/// unknown, so a region fallback here would invent an edge every
/// reconciliation deletes and every event re-adds.
#[test]
fn an_unparented_subnet_gets_no_region_edge() {
    for (resource_type, resource_id) in [
        ("AWS::EC2::Subnet", "subnet-orphan"),
        ("AWS::EC2::RouteTable", "rtb-orphan"),
    ] {
        let body = instance_config_item("ResourceDiscovered")
            .replace(
                r#""resourceType": "AWS::EC2::Instance""#,
                &format!(r#""resourceType": "{resource_type}""#),
            )
            .replace(
                r#""resourceId": "i-0abc123""#,
                &format!(r#""resourceId": "{resource_id}""#),
            )
            // Drop the VPC relationship, leaving the resource unparented.
            .replace(
                r#"{ "resourceType": "AWS::EC2::VPC", "resourceId": "vpc-globex" },"#,
                "",
            );

        let events = parse_ok(&body);
        let typed = events.first().expect("a typed event for {resource_type}");

        assert_eq!(
            typed.context.edge_count(),
            0,
            "{resource_type} with no VPC must contribute no edge"
        );
        assert!(
            !node_keys(&typed.context).contains(&node_key(&Node::AwsRegion("us-east-1".into()))),
            "{resource_type} must not drag in a region it does not attach to"
        );
    }
}

/// The same resource types *do* attach when the VPC is known — the fallback is
/// narrow, not a blanket refusal to build edges.
#[test]
fn a_subnet_still_lands_inside_the_vpc_its_event_named() {
    let body = instance_config_item("ResourceDiscovered")
        .replace(
            r#""resourceType": "AWS::EC2::Instance""#,
            r#""resourceType": "AWS::EC2::Subnet""#,
        )
        .replace(
            r#""resourceId": "i-0abc123""#,
            r#""resourceId": "subnet-new""#,
        );

    let events = parse_ok(&body);
    let typed = events.first().expect("a typed subnet event");

    assert!(typed.context.edge_references().any(|e| {
        typed.context[e.source()] == Node::AwsEc2Vpc("vpc-globex".into())
            && typed.context[e.target()] == Node::AwsEc2Subnet("subnet-new".into())
            && e.weight() == &Edge::Contains
    }));
}
