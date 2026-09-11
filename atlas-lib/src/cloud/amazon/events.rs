//! AWS Tier-1 ingestion: EventBridge → normalized [`ChangeEvent`].
//!
//! This is the first real event feed (Phase 3 of
//! `docs/change_monitoring_design.md`). EventBridge is the single delivery
//! path; three different producers put events on it, and this module reads all
//! three because they trade completeness against latency in opposite
//! directions:
//!
//! - **AWS Config configuration items** (`Config Configuration Item Change
//!   Notification`) — the richest. Each item carries the resource's type, id,
//!   region, its relationships to other resources and a snapshot of its
//!   configuration, so a create can be projected with its VPC/subnet/security
//!   groups attached rather than as a bare floating node. Costs money and lags
//!   by a minute or two.
//! - **EC2 instance state-change notifications** — free, on by default, and
//!   near-instant, but carries an instance id and a state and nothing else.
//! - **CloudTrail management events** (`AWS API Call via CloudTrail`) — the
//!   broadest coverage and the messiest shape: every mutating API call, with
//!   the resource id buried somewhere different in `requestParameters` or
//!   `responseElements` for each one. Handled for a curated set of calls.
//!
//! **Transport.** The consumer is [`stream::EventQueue`], an SQS queue that an
//! EventBridge rule targets. SQS rather than a direct push endpoint because the
//! server is a long-running process that may be restarted or briefly
//! unreachable: a queue buffers across that, and its at-least-once redelivery
//! is exactly what [`EventApplier`](crate::atlas::event::EventApplier) is built
//! to tolerate.
//!
//! **The rule that shapes everything here:** an adapter may only produce nodes
//! and edges the full-scan projector would also produce. Tier 3 diffs the whole
//! graph and is authoritative, so an edge invented here that no projector emits
//! gets deleted at the next reconciliation and re-added by the next event,
//! flapping forever. That is why EC2 instances are projected through the
//! projector's own [`project_instance`], and why several resource types are
//! deliberately *not* mapped below even though the events exist — see
//! [`typed_node`].

use crate::atlas::collection::CollectionSource;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::event::{ChangeEvent, ChangeOp};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::projector::aws::{
    EniFacts, InstanceFacts, project_instance, use_aws_resource, use_global,
};
use aws_smithy_types::date_time::{DateTime, Format};
use petgraph::graph::Graph;
use serde::Deserialize;
use serde_json::Value;
use std::fmt;

const SOURCE: CollectionSource = CollectionSource::Aws;

/// A message that could not be understood as an EventBridge event at all.
///
/// This is [`FailureKind::Malformed`] territory, not a read failure: we
/// received the message, so the feed is working and nothing should be held.
/// What we lost is one event, and losing it silently is the thing that must not
/// happen — a dropped delete leaves a resource in the graph until the next
/// reconciliation quietly cleans it up, with no record of why.
#[derive(Debug)]
pub struct MalformedEvent {
    pub reason: String,
}

impl fmt::Display for MalformedEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for MalformedEvent {}

fn malformed(reason: impl Into<String>) -> MalformedEvent {
    MalformedEvent {
        reason: reason.into(),
    }
}

/// The EventBridge envelope every event shares. Only the fields that survive
/// normalization are named; `detail` stays a `Value` because its shape is
/// decided by `detail-type`.
#[derive(Deserialize)]
struct Envelope {
    id: Option<String>,
    #[serde(rename = "detail-type")]
    detail_type: Option<String>,
    time: Option<String>,
    region: Option<String>,
    detail: Option<Value>,
}

/// An SNS envelope wrapping an EventBridge event. A rule may target SNS with
/// SQS subscribed behind it (the fan-out shape, and how AWS Config's own
/// delivery channel is usually wired), in which case the event we want is a
/// JSON string inside `Message`.
#[derive(Deserialize)]
struct SnsEnvelope {
    #[serde(rename = "Type")]
    kind: Option<String>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

/// Translate one queue message into the changes it describes.
///
/// One message can be several changes: a `RunInstances` call launches a batch,
/// and a Config item yields both the typed node and the Config catch-all node
/// that the full scan also produces for it.
///
/// An empty `Ok` is the ordinary outcome for an event we understand but do not
/// model — an unmapped resource type, a non-mutating API call. Only a body we
/// cannot read as an event at all is an error.
pub fn parse(body: &str, exclude_by_default: bool) -> Result<Vec<ChangeEvent>, MalformedEvent> {
    let body = unwrap_sns(body);
    let envelope: Envelope = serde_json::from_str(&body)
        .map_err(|e| malformed(format!("not an EventBridge event: {e}")))?;

    let Some(detail_type) = envelope.detail_type.as_deref() else {
        return Err(malformed("event has no detail-type"));
    };
    let Some(detail) = envelope.detail.as_ref() else {
        return Err(malformed(format!("{detail_type} event has no detail")));
    };

    let region = envelope.region.as_deref().unwrap_or_default();
    let id = envelope.id.as_deref().unwrap_or_default();
    let time = envelope.time.as_deref().and_then(epoch_millis);

    match detail_type {
        "Config Configuration Item Change Notification" => {
            from_config_item(detail, region, id, time, exclude_by_default)
        }
        "EC2 Instance State-change Notification" => from_instance_state(detail, region, id, time),
        "AWS API Call via CloudTrail" => from_cloud_trail(detail, region, id, time),
        // A rule matched something broader than we model. Not a failure: the
        // operator is free to point a coarse rule at the queue, and the events
        // we do not understand simply wait for Tier 3.
        _ => Ok(Vec::new()),
    }
}

/// Peel an SNS notification, if that is what this is. A raw EventBridge event
/// has no `Type`/`Message` pair, so this is a no-op for the direct path.
fn unwrap_sns(body: &str) -> std::borrow::Cow<'_, str> {
    match serde_json::from_str::<SnsEnvelope>(body) {
        Ok(SnsEnvelope {
            kind: Some(kind),
            message: Some(message),
        }) if kind == "Notification" => std::borrow::Cow::Owned(message),
        _ => std::borrow::Cow::Borrowed(body),
    }
}

/// Cloud-recorded time in epoch milliseconds. The ordering key for the whole
/// tier, so a timestamp we cannot parse is better dropped than guessed —
/// substituting arrival time would silently assert that delivery order is
/// change order, which is the one thing this feed does not promise.
fn epoch_millis(timestamp: &str) -> Option<i64> {
    DateTime::from_str(timestamp, Format::DateTime)
        .ok()?
        .to_millis()
        .ok()
}

// ---- AWS Config configuration items -----------------------------------------

#[derive(Deserialize)]
struct ConfigDetail {
    #[serde(rename = "configurationItem")]
    configuration_item: Option<ConfigurationItem>,
}

#[derive(Deserialize)]
struct ConfigurationItem {
    #[serde(rename = "configurationItemStatus")]
    status: Option<String>,
    #[serde(rename = "configurationItemCaptureTime")]
    capture_time: Option<String>,
    #[serde(rename = "resourceType")]
    resource_type: Option<String>,
    #[serde(rename = "resourceId")]
    resource_id: Option<String>,
    #[serde(rename = "resourceName")]
    resource_name: Option<String>,
    #[serde(rename = "ARN")]
    arn: Option<String>,
    #[serde(rename = "awsRegion")]
    aws_region: Option<String>,
    #[serde(rename = "availabilityZone")]
    availability_zone: Option<String>,
    relationships: Option<Vec<Relationship>>,
    configuration: Option<Value>,
}

#[derive(Deserialize)]
struct Relationship {
    #[serde(rename = "resourceType")]
    resource_type: Option<String>,
    #[serde(rename = "resourceId")]
    resource_id: Option<String>,
}

impl ConfigurationItem {
    /// The id of the first related resource of a given type. Config lists a
    /// resource's relationships explicitly, which is what lets a create event
    /// arrive already attached to its VPC instead of floating.
    fn related(&self, resource_type: &str) -> Option<&str> {
        self.relationships
            .as_deref()?
            .iter()
            .find(|r| r.resource_type.as_deref() == Some(resource_type))
            .and_then(|r| r.resource_id.as_deref())
    }

    fn related_all(&self, resource_type: &str) -> Vec<&str> {
        self.relationships
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|r| r.resource_type.as_deref() == Some(resource_type))
            .filter_map(|r| r.resource_id.as_deref())
            .collect()
    }

    /// Config delivers `configuration` as an object over EventBridge and as an
    /// embedded JSON string over some other paths; accept both.
    fn configuration<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        match self.configuration.as_ref()? {
            Value::String(raw) => serde_json::from_str(raw).ok(),
            other => serde_json::from_value(other.clone()).ok(),
        }
    }
}

/// The EC2 instance fields Config puts in `configuration`, in the camelCase the
/// EC2 API uses on the wire (the SDK's `Instance` is the same data under
/// different casing).
#[derive(Deserialize, Default)]
struct InstanceConfiguration {
    #[serde(rename = "vpcId")]
    vpc_id: Option<String>,
    #[serde(rename = "subnetId")]
    subnet_id: Option<String>,
    #[serde(rename = "privateIpAddress")]
    private_ip_address: Option<String>,
    placement: Option<Placement>,
    #[serde(rename = "securityGroups")]
    security_groups: Option<Vec<GroupRef>>,
    #[serde(rename = "networkInterfaces")]
    network_interfaces: Option<Vec<NetworkInterfaceRef>>,
    tags: Option<Vec<TagPair>>,
}

/// One interface as EC2 reports it on the wire, in the shape Config's
/// `networkInterfaces` and CloudTrail's `networkInterfaceSet` share.
#[derive(Deserialize)]
struct NetworkInterfaceRef {
    #[serde(rename = "networkInterfaceId")]
    network_interface_id: Option<String>,
    #[serde(rename = "subnetId")]
    subnet_id: Option<String>,
}

#[derive(Deserialize)]
struct Placement {
    #[serde(rename = "availabilityZone")]
    availability_zone: Option<String>,
}

#[derive(Deserialize)]
struct GroupRef {
    #[serde(rename = "groupId")]
    group_id: Option<String>,
}

#[derive(Deserialize)]
struct TagPair {
    key: Option<String>,
    value: Option<String>,
}

/// The `publicIp` an Elastic IP configuration carries, so the EIP arrives
/// stitched to the generic IP space the way the networking projector stitches
/// it.
#[derive(Deserialize, Default)]
struct EipConfiguration {
    #[serde(rename = "publicIp")]
    public_ip: Option<String>,
}

fn from_config_item(
    detail: &Value,
    envelope_region: &str,
    id: &str,
    envelope_time: Option<i64>,
    exclude_by_default: bool,
) -> Result<Vec<ChangeEvent>, MalformedEvent> {
    let detail: ConfigDetail = serde_json::from_value(detail.clone())
        .map_err(|e| malformed(format!("unreadable Config detail: {e}")))?;
    let Some(item) = detail.configuration_item else {
        return Err(malformed("Config notification has no configurationItem"));
    };

    let Some(resource_type) = item.resource_type.as_deref() else {
        return Err(malformed("configuration item has no resourceType"));
    };
    let Some(op) = config_op(item.status.as_deref()) else {
        // `ResourceNotRecorded` and friends: Config is telling us it is *not*
        // tracking this resource. That says nothing about whether it exists.
        return Ok(Vec::new());
    };

    let region = match item.aws_region.as_deref() {
        Some(region) if !region.is_empty() => region,
        _ => envelope_region,
    };
    let observed_at = item
        .capture_time
        .as_deref()
        .and_then(epoch_millis)
        .or(envelope_time);
    let Some(observed_at) = observed_at else {
        return Err(malformed("configuration item has no usable timestamp"));
    };

    let mut events = Vec::new();

    // The typed node, when this is a resource type the graph models directly.
    if let Some(node) = typed_node(&item, resource_type) {
        let context = typed_context(&item, region, resource_type, &node);
        events.push(
            ChangeEvent::new(SOURCE, region, id, observed_at, op, node).with_context(context),
        );
    }

    // The AWS Config catch-all node, which the full scan's `config` collector
    // also produces for this resource type. Emitting both keeps a deletion
    // complete: dropping only the typed node would leave the catch-all behind
    // until the next reconciliation swept it.
    if use_aws_resource(resource_type, exclude_by_default)
        && let Some(resource_id) = item.resource_id.as_deref()
    {
        let node = Node::AwsConfigResource {
            resource_type: resource_type.into(),
            id: resource_id.into(),
        };
        let parent = if use_global(resource_type) {
            Node::AwsRegion("global".into())
        } else {
            Node::AwsRegion(region.into())
        };
        let context = linked(parent, node.clone(), Edge::Contains);
        events.push(
            ChangeEvent::new(SOURCE, region, id, observed_at, op, node).with_context(context),
        );
    }

    Ok(events)
}

fn config_op(status: Option<&str>) -> Option<ChangeOp> {
    match status? {
        "ResourceDiscovered" => Some(ChangeOp::Created),
        "OK" => Some(ChangeOp::Modified),
        "ResourceDeleted" | "ResourceDeletedNotRecorded" => Some(ChangeOp::Deleted),
        _ => None,
    }
}

/// The typed `Node` for a Config resource type, keyed **exactly** the way the
/// full-scan projector keys it.
///
/// The omissions matter as much as the entries, because a mis-keyed node is
/// worse than no node: it never matches what the scan produces, so it is
/// deleted at every reconciliation and recreated by every event.
///
/// - `AWS::EC2::NetworkInterface` — the node is keyed by `eni-` id and would
///   match, but the full scan only learns about interfaces through the
///   `networkInterfaces` list on a described *instance*. Config also reports
///   the ENIs of NAT gateways, load balancers, RDS and in-VPC Lambda, and a
///   node for one of those is a node no scan produces — deleted at every
///   reconciliation, recreated by every event. This arm opens up as soon as a
///   `DescribeNetworkInterfaces` collector makes the scan authoritative for all
///   of them.
/// - `AWS::Route53::HostedZone` — the Route 53 API returns ids as
///   `/hostedzone/Z123` and the projector stores them that way; Config reports
///   the bare id.
/// - `AWS::SQS::Queue` — the projector keys queues by URL, which Config does
///   not report as the resource id.
///
/// All three still reach the graph through the Config catch-all node, which is
/// keyed on the Config id by construction and so cannot drift.
fn typed_node(item: &ConfigurationItem, resource_type: &str) -> Option<Node> {
    let id = item.resource_id.as_deref();
    let name = item.resource_name.as_deref().or(id);
    let arn = item.arn.as_deref().or(id);

    let node = match resource_type {
        "AWS::EC2::Instance" => Node::AwsEc2Instance(id?.into()),
        "AWS::EC2::VPC" => Node::AwsEc2Vpc(id?.into()),
        "AWS::EC2::Subnet" => Node::AwsEc2Subnet(id?.into()),
        "AWS::EC2::SecurityGroup" => Node::AwsEc2SecurityGroup(id?.into()),
        "AWS::EC2::RouteTable" => Node::AwsEc2RouteTable(id?.into()),
        "AWS::EC2::InternetGateway" => Node::AwsEc2InternetGateway(id?.into()),
        "AWS::EC2::NatGateway" => Node::AwsEc2NatGateway(id?.into()),
        "AWS::EC2::EIP" => Node::AwsEc2Eip(id?.into()),
        "AWS::Lambda::Function" => Node::AwsLambdaFunction(name?.into()),
        "AWS::ECS::Cluster" => Node::AwsEcsCluster(arn?.into()),
        "AWS::EKS::Cluster" => Node::AwsEksCluster(name?.into()),
        "AWS::RDS::DBInstance" => Node::AwsRdsDbInstance(name?.into()),
        "AWS::DynamoDB::Table" => Node::AwsDynamoDbTable(name?.into()),
        "AWS::SNS::Topic" => Node::AwsSnsTopic(arn?.into()),
        "AWS::ElasticLoadBalancingV2::LoadBalancer" => Node::AwsElbLoadBalancer(arn?.into()),
        "AWS::ApiGateway::RestApi" => Node::AwsApiGatewayRestApi(id?.into()),
        "AWS::CloudFront::Distribution" => Node::AwsCloudFrontDistribution(id?.into()),
        _ => return None,
    };
    Some(node)
}

/// The neighbourhood to create the typed node in, matching the containment the
/// full-scan projector gives that resource type — region for most things, the
/// VPC where the projector prefers it, and the ENI pivot for instances.
fn typed_context(
    item: &ConfigurationItem,
    region: &str,
    resource_type: &str,
    node: &Node,
) -> Graph<Node, Edge> {
    let vpc = || {
        item.related("AWS::EC2::VPC")
            .map(|id| Node::AwsEc2Vpc(id.into()))
    };
    let region_node = || Node::AwsRegion(region.into());

    match resource_type {
        "AWS::EC2::Instance" => instance_context(item, region),

        // Region-contained, exactly as their collectors project them.
        "AWS::EC2::VPC"
        | "AWS::EC2::SecurityGroup"
        | "AWS::Lambda::Function"
        | "AWS::ECS::Cluster"
        | "AWS::DynamoDB::Table"
        | "AWS::SNS::Topic"
        | "AWS::ApiGateway::RestApi" => linked(region_node(), node.clone(), Edge::Contains),

        "AWS::CloudFront::Distribution" => linked(
            Node::AwsRegion("global".into()),
            node.clone(),
            Edge::Contains,
        ),

        // Inside their VPC where one is known, region otherwise — the same
        // `match vpc_id { Some => vpc, None => region }` the ELB/EKS/RDS arms
        // of the projector use.
        "AWS::EKS::Cluster"
        | "AWS::RDS::DBInstance"
        | "AWS::ElasticLoadBalancingV2::LoadBalancer" => linked(
            vpc().unwrap_or_else(region_node),
            node.clone(),
            Edge::Contains,
        ),

        // Inside their VPC or nowhere. These two look like the arms above and
        // are not: the projector reaches a subnet through
        // `link_to(vpc_idx, ..)` and a route table through
        // `link_from(rt_idx, ..)`, and both emit *no* edge when the VPC is
        // unknown. Falling back to the region here would invent
        // `Region -Contains-> Subnet`, which every reconciliation deletes and
        // every event re-adds.
        "AWS::EC2::Subnet" | "AWS::EC2::RouteTable" => match vpc() {
            Some(vpc) => linked(vpc, node.clone(), Edge::Contains),
            None => solo(node.clone()),
        },

        // The egress plane attaches rather than contains.
        "AWS::EC2::InternetGateway" => match vpc() {
            Some(vpc) => linked(node.clone(), vpc, Edge::AttachedTo),
            None => solo(node.clone()),
        },
        "AWS::EC2::NatGateway" => match item.related("AWS::EC2::Subnet") {
            Some(subnet) => linked(
                node.clone(),
                Node::AwsEc2Subnet(subnet.into()),
                Edge::AttachedTo,
            ),
            None => solo(node.clone()),
        },
        "AWS::EC2::EIP" => {
            let config: EipConfiguration = item.configuration().unwrap_or_default();
            match config.public_ip {
                Some(ip) => linked(
                    node.clone(),
                    Node::GenericIpAddress(ip.as_str().into()),
                    Edge::ConnectsTo,
                ),
                None => solo(node.clone()),
            }
        }

        _ => solo(node.clone()),
    }
}

/// An instance's neighbourhood, built by the *projector's* own
/// [`project_instance`] so the ENI pivot and every other edge is identical to
/// what a full scan of the same instance produces.
fn instance_context(item: &ConfigurationItem, region: &str) -> Graph<Node, Edge> {
    let config: InstanceConfiguration = item.configuration().unwrap_or_default();

    // Config states the same facts twice — in `configuration` and, for the
    // relational ones, in `relationships`. Prefer the configuration and fall
    // back, so an item delivered without a configuration snapshot still lands
    // attached.
    let vpc_id = config
        .vpc_id
        .as_deref()
        .or_else(|| item.related("AWS::EC2::VPC"));
    let subnet_id = config
        .subnet_id
        .as_deref()
        .or_else(|| item.related("AWS::EC2::Subnet"));
    let mut security_group_ids: Vec<&str> = config
        .security_groups
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|group| group.group_id.as_deref())
        .collect();
    if security_group_ids.is_empty() {
        security_group_ids = item.related_all("AWS::EC2::SecurityGroup");
    }

    // Same two-sources-for-one-fact shape as the fields above. The
    // relationships list gives ids *without* subnets, so these ENIs land as
    // nodes on the instance with no `AttachedTo` edge — the full scan reads
    // each interface's own subnet, and guessing here would contradict it.
    let mut network_interfaces: Vec<EniFacts<'_>> = config
        .network_interfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|eni| {
            Some(EniFacts {
                id: eni.network_interface_id.as_deref()?,
                subnet_id: eni.subnet_id.as_deref(),
            })
        })
        .collect();
    if network_interfaces.is_empty() {
        network_interfaces = item
            .related_all("AWS::EC2::NetworkInterface")
            .into_iter()
            .map(|id| EniFacts {
                id,
                subnet_id: None,
            })
            .collect();
    }

    let mut builder = GraphBuilder::new();
    let region_idx = builder.get_or_add_node(Node::AwsRegion(region.into()));
    project_instance(
        &mut builder,
        region_idx,
        &InstanceFacts {
            id: item.resource_id.as_deref(),
            vpc_id,
            subnet_id,
            availability_zone: config
                .placement
                .as_ref()
                .and_then(|p| p.availability_zone.as_deref())
                .or(item.availability_zone.as_deref()),
            private_ip: config.private_ip_address.as_deref(),
            security_group_ids,
            network_interfaces,
            tags: config
                .tags
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter_map(|tag| tag.key.as_deref().zip(tag.value.as_deref()))
                .collect(),
        },
    );
    builder.graph
}

// ---- EC2 instance state-change notifications --------------------------------

#[derive(Deserialize)]
struct InstanceStateDetail {
    #[serde(rename = "instance-id")]
    instance_id: Option<String>,
    state: Option<String>,
}

/// The cheapest AWS change feed there is: on by default, no Config, seconds of
/// latency. It carries an id and a lifecycle state and nothing else, so the
/// node arrives bare and Tier 3 (or a Config item) attaches it.
fn from_instance_state(
    detail: &Value,
    region: &str,
    id: &str,
    time: Option<i64>,
) -> Result<Vec<ChangeEvent>, MalformedEvent> {
    let detail: InstanceStateDetail = serde_json::from_value(detail.clone())
        .map_err(|e| malformed(format!("unreadable instance state detail: {e}")))?;
    let Some(instance_id) = detail.instance_id else {
        return Err(malformed("state change has no instance-id"));
    };
    let Some(observed_at) = time else {
        return Err(malformed("state change has no usable timestamp"));
    };

    // `stopped` is not `terminated`: a stopped instance still exists, still has
    // its ENI and its subnet, and deleting it here would make the graph
    // disagree with the next scan.
    let op = match detail.state.as_deref() {
        Some("terminated" | "shutting-down") => ChangeOp::Deleted,
        Some(_) => ChangeOp::Created,
        None => return Err(malformed("state change has no state")),
    };

    let node = Node::AwsEc2Instance(instance_id.as_str().into());
    let context = if op == ChangeOp::Deleted {
        Graph::new()
    } else {
        solo(node.clone())
    };
    Ok(vec![
        ChangeEvent::new(SOURCE, region, id, observed_at, op, node).with_context(context),
    ])
}

// ---- CloudTrail management events -------------------------------------------

#[derive(Deserialize)]
struct CloudTrailDetail {
    #[serde(rename = "eventName")]
    event_name: Option<String>,
    #[serde(rename = "eventTime")]
    event_time: Option<String>,
    #[serde(rename = "awsRegion")]
    aws_region: Option<String>,
    #[serde(rename = "errorCode")]
    error_code: Option<String>,
    #[serde(rename = "requestParameters")]
    request_parameters: Option<Value>,
    #[serde(rename = "responseElements")]
    response_elements: Option<Value>,
}

#[derive(Deserialize)]
struct ItemSet<T> {
    items: Option<Vec<T>>,
}

#[derive(Deserialize)]
struct TrailInstance {
    #[serde(rename = "instanceId")]
    instance_id: Option<String>,
    #[serde(rename = "vpcId")]
    vpc_id: Option<String>,
    #[serde(rename = "subnetId")]
    subnet_id: Option<String>,
    #[serde(rename = "privateIpAddress")]
    private_ip_address: Option<String>,
    placement: Option<Placement>,
    #[serde(rename = "groupSet")]
    group_set: Option<ItemSet<GroupRef>>,
    #[serde(rename = "networkInterfaceSet")]
    network_interface_set: Option<ItemSet<NetworkInterfaceRef>>,
}

/// CloudTrail is the broadest feed and the least uniform one: the id of the
/// thing that changed lives somewhere different for every API call, so coverage
/// is a curated list rather than a general rule. The calls handled here are the
/// ones that change *topology*; everything else waits for Config or Tier 3.
fn from_cloud_trail(
    detail: &Value,
    envelope_region: &str,
    id: &str,
    envelope_time: Option<i64>,
) -> Result<Vec<ChangeEvent>, MalformedEvent> {
    let detail: CloudTrailDetail = serde_json::from_value(detail.clone())
        .map_err(|e| malformed(format!("unreadable CloudTrail detail: {e}")))?;

    // A call that failed changed nothing. CloudTrail records the attempt either
    // way, so without this a rejected `TerminateInstances` would delete a
    // running instance from the graph.
    if detail.error_code.is_some() {
        return Ok(Vec::new());
    }

    let Some(event_name) = detail.event_name.as_deref() else {
        return Err(malformed("CloudTrail record has no eventName"));
    };
    let region = match detail.aws_region.as_deref() {
        Some(region) if !region.is_empty() => region,
        _ => envelope_region,
    };
    let observed_at = detail
        .event_time
        .as_deref()
        .and_then(epoch_millis)
        .or(envelope_time);
    let Some(observed_at) = observed_at else {
        return Err(malformed("CloudTrail record has no usable timestamp"));
    };

    let request = detail.request_parameters.unwrap_or(Value::Null);
    let response = detail.response_elements.unwrap_or(Value::Null);
    let event =
        |op: ChangeOp, node: Node| ChangeEvent::new(SOURCE, region, id, observed_at, op, node);
    let region_child = |op: ChangeOp, node: Node| {
        let context = linked(Node::AwsRegion(region.into()), node.clone(), Edge::Contains);
        event(op, node).with_context(context)
    };

    let events = match event_name {
        // A launch reports each instance with its placement, so these arrive
        // fully attached — the one CloudTrail call that is as rich as Config.
        "RunInstances" => launched_instances(&response, region, id, observed_at),

        "TerminateInstances" => instance_ids(&request)
            .into_iter()
            .map(|instance_id| {
                event(
                    ChangeOp::Deleted,
                    Node::AwsEc2Instance(instance_id.as_str().into()),
                )
            })
            .collect(),

        "CreateVpc" => string_at(&response, &["vpc", "vpcId"])
            .map(|vpc_id| {
                vec![region_child(
                    ChangeOp::Created,
                    Node::AwsEc2Vpc(vpc_id.as_str().into()),
                )]
            })
            .unwrap_or_default(),
        "DeleteVpc" => string_at(&request, &["vpcId"])
            .map(|vpc_id| {
                vec![event(
                    ChangeOp::Deleted,
                    Node::AwsEc2Vpc(vpc_id.as_str().into()),
                )]
            })
            .unwrap_or_default(),

        "CreateSubnet" => string_at(&response, &["subnet", "subnetId"])
            .map(|subnet_id| {
                let node = Node::AwsEc2Subnet(subnet_id.as_str().into());
                // No region fallback: the projector gives an unparented subnet
                // no edge at all, and inventing one here would flap.
                let context = match string_at(&response, &["subnet", "vpcId"]) {
                    Some(vpc_id) => linked(
                        Node::AwsEc2Vpc(vpc_id.as_str().into()),
                        node.clone(),
                        Edge::Contains,
                    ),
                    None => solo(node.clone()),
                };
                vec![event(ChangeOp::Created, node).with_context(context)]
            })
            .unwrap_or_default(),
        "DeleteSubnet" => string_at(&request, &["subnetId"])
            .map(|subnet_id| {
                vec![event(
                    ChangeOp::Deleted,
                    Node::AwsEc2Subnet(subnet_id.as_str().into()),
                )]
            })
            .unwrap_or_default(),

        "CreateSecurityGroup" => string_at(&response, &["groupId"])
            .map(|group_id| {
                vec![region_child(
                    ChangeOp::Created,
                    Node::AwsEc2SecurityGroup(group_id.as_str().into()),
                )]
            })
            .unwrap_or_default(),
        "DeleteSecurityGroup" => string_at(&request, &["groupId"])
            .map(|group_id| {
                vec![event(
                    ChangeOp::Deleted,
                    Node::AwsEc2SecurityGroup(group_id.as_str().into()),
                )]
            })
            .unwrap_or_default(),

        // Lambda's API version is part of the CloudTrail event name.
        "CreateFunction20150331" => string_at(&response, &["functionName"])
            .map(|name| {
                vec![region_child(
                    ChangeOp::Created,
                    Node::AwsLambdaFunction(name.as_str().into()),
                )]
            })
            .unwrap_or_default(),
        "DeleteFunction20150331" => string_at(&request, &["functionName"])
            .map(|name| {
                vec![event(
                    ChangeOp::Deleted,
                    Node::AwsLambdaFunction(name.as_str().into()),
                )]
            })
            .unwrap_or_default(),

        _ => Vec::new(),
    };

    Ok(events)
}

fn launched_instances(
    response: &Value,
    region: &str,
    id: &str,
    observed_at: i64,
) -> Vec<ChangeEvent> {
    let items: ItemSet<TrailInstance> = match response.get("instancesSet") {
        Some(set) => serde_json::from_value(set.clone()).unwrap_or(ItemSet { items: None }),
        None => ItemSet { items: None },
    };

    items
        .items
        .unwrap_or_default()
        .into_iter()
        .filter_map(|instance| {
            let instance_id = instance.instance_id.as_deref()?;
            let mut builder = GraphBuilder::new();
            let region_idx = builder.get_or_add_node(Node::AwsRegion(region.into()));
            project_instance(
                &mut builder,
                region_idx,
                &InstanceFacts {
                    id: Some(instance_id),
                    vpc_id: instance.vpc_id.as_deref(),
                    subnet_id: instance.subnet_id.as_deref(),
                    availability_zone: instance
                        .placement
                        .as_ref()
                        .and_then(|p| p.availability_zone.as_deref()),
                    private_ip: instance.private_ip_address.as_deref(),
                    security_group_ids: instance
                        .group_set
                        .as_ref()
                        .and_then(|set| set.items.as_deref())
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|group| group.group_id.as_deref())
                        .collect(),
                    tags: Vec::new(),
                    network_interfaces: instance
                        .network_interface_set
                        .as_ref()
                        .and_then(|set| set.items.as_deref())
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|eni| {
                            Some(EniFacts {
                                id: eni.network_interface_id.as_deref()?,
                                subnet_id: eni.subnet_id.as_deref(),
                            })
                        })
                        .collect(),
                },
            );
            Some(
                ChangeEvent::new(
                    SOURCE,
                    region,
                    id,
                    observed_at,
                    ChangeOp::Created,
                    Node::AwsEc2Instance(instance_id.into()),
                )
                .with_context(builder.graph),
            )
        })
        .collect()
}

fn instance_ids(value: &Value) -> Vec<String> {
    let Some(set) = value.get("instancesSet") else {
        return Vec::new();
    };
    let items: ItemSet<TrailInstance> =
        serde_json::from_value(set.clone()).unwrap_or(ItemSet { items: None });
    items
        .items
        .unwrap_or_default()
        .into_iter()
        .filter_map(|instance| instance.instance_id)
        .collect()
}

fn string_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(key)?;
    }
    cursor.as_str().map(str::to_owned)
}

// ---- context helpers --------------------------------------------------------

fn solo(node: Node) -> Graph<Node, Edge> {
    let mut builder = GraphBuilder::new();
    builder.get_or_add_node(node);
    builder.graph
}

fn linked(source: Node, target: Node, edge: Edge) -> Graph<Node, Edge> {
    let mut builder = GraphBuilder::new();
    let source_idx = builder.get_or_add_node(source);
    builder.link_to(source_idx, target, edge);
    builder.graph
}

/// The SQS consumer that carries these events off EventBridge.
pub mod stream {
    use super::{MalformedEvent, SOURCE, parse};
    use crate::atlas::collection::{CollectionReport, FailureKind};
    use crate::atlas::event::ChangeEvent;
    use aws_sdk_sqs::Client;
    use aws_sdk_sqs::types::DeleteMessageBatchRequestEntry;

    /// One drain of the queue: what it told us, and what went wrong while
    /// finding out.
    ///
    /// Infallible by construction, like every `build_*` in `cloud/` — a stream
    /// that cannot be read still returns a batch, with the empty event list
    /// explained by a non-empty report, so no caller can mistake a broken feed
    /// for a quiet one.
    pub struct EventBatch {
        pub events: Vec<ChangeEvent>,
        pub report: CollectionReport,
    }

    /// An SQS queue fed by an EventBridge rule.
    ///
    /// Long-polls, translates every message it can, and deletes what it
    /// processed. Messages are deleted *after* translation but before the graph
    /// applies them: if the process dies in between, SQS redelivers, and
    /// [`EventApplier`](crate::atlas::event::EventApplier) is idempotent — the
    /// cheap failure. Holding messages until the graph confirmed them would
    /// instead stall the queue behind one slow apply.
    pub struct EventQueue {
        client: Client,
        queue_url: String,
        region: String,
        exclude_by_default: bool,
        wait_time_seconds: i32,
    }

    impl EventQueue {
        /// Seconds to hold a receive open waiting for events. Long-polling
        /// keeps latency at "as soon as the event lands" without spinning, and
        /// bounds how long a shutdown or a reconciliation tick waits on us.
        pub const WAIT_SECONDS: i32 = 20;

        /// Messages per receive. SQS's maximum; a burst simply takes several
        /// round trips.
        const BATCH: i32 = 10;

        pub fn new(
            config: &aws_config::SdkConfig,
            queue_url: impl Into<String>,
            region: impl Into<String>,
            exclude_by_default: bool,
        ) -> Self {
            Self {
                client: Client::new(config),
                queue_url: queue_url.into(),
                region: region.into(),
                exclude_by_default,
                wait_time_seconds: Self::WAIT_SECONDS,
            }
        }

        /// Shorten the long-poll. For tests, which must not block for twenty
        /// seconds on an empty replay queue.
        pub fn with_wait_seconds(mut self, seconds: i32) -> Self {
            self.wait_time_seconds = seconds;
            self
        }

        fn scope(&self) -> String {
            format!("{}/events", self.region)
        }

        pub async fn receive(&self) -> EventBatch {
            let mut report = CollectionReport::default();

            let response = self
                .client
                .receive_message()
                .queue_url(&self.queue_url)
                .max_number_of_messages(Self::BATCH)
                .wait_time_seconds(self.wait_time_seconds)
                .send()
                .await;

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    // Classified here, where the error is still typed. A 403 on
                    // the queue is a policy problem no amount of polling fixes;
                    // anything else may be a blip worth waiting out.
                    let kind = match error.raw_response().map(|r| r.status().as_u16()) {
                        Some(401 | 403) => FailureKind::Unauthorized,
                        _ => FailureKind::Unavailable,
                    };
                    report.record(SOURCE, kind, self.scope(), error);
                    return EventBatch {
                        events: Vec::new(),
                        report,
                    };
                }
            };

            let mut events = Vec::new();
            let mut processed = Vec::new();

            for message in response.messages.unwrap_or_default() {
                let body = message.body.as_deref().unwrap_or_default();
                match parse(body, self.exclude_by_default) {
                    Ok(mut parsed) => events.append(&mut parsed),
                    Err(MalformedEvent { reason }) => {
                        // Not an unreadable source: we read the queue fine, and
                        // every other message on it is good. Reported, then
                        // deleted with the rest — a message we can never parse
                        // would otherwise come back on every poll forever.
                        // Attach a redrive policy to the queue if the raw
                        // bodies are worth keeping.
                        report.note(
                            SOURCE,
                            FailureKind::Malformed,
                            self.scope(),
                            format!("dropped an unreadable event: {reason}"),
                        );
                    }
                }
                if let Some(handle) = message.receipt_handle {
                    processed.push(handle);
                }
            }

            self.delete(processed, &mut report).await;

            EventBatch { events, report }
        }

        async fn delete(&self, handles: Vec<String>, report: &mut CollectionReport) {
            if handles.is_empty() {
                return;
            }

            let entries: Vec<_> = handles
                .into_iter()
                .enumerate()
                .filter_map(|(i, handle)| {
                    DeleteMessageBatchRequestEntry::builder()
                        .id(i.to_string())
                        .receipt_handle(handle)
                        .build()
                        .ok()
                })
                .collect();

            let result = self
                .client
                .delete_message_batch()
                .queue_url(&self.queue_url)
                .set_entries(Some(entries))
                .send()
                .await;

            // A failed delete costs a redelivery, not an event. Worth
            // reporting — a queue that never drains is a real problem — but it
            // must not be an unreadable source, or a permissions gap on
            // DeleteMessage would suspend removals across all of AWS.
            match result {
                Ok(response) => {
                    let failed = response.failed();
                    if !failed.is_empty() {
                        report.note(
                            SOURCE,
                            FailureKind::Malformed,
                            self.scope(),
                            format!("{} processed event(s) could not be deleted", failed.len()),
                        );
                    }
                }
                Err(error) => report.record(
                    SOURCE,
                    FailureKind::Malformed,
                    self.scope(),
                    format!("could not delete processed events: {error:?}"),
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests;
