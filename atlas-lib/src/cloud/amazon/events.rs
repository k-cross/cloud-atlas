use crate::atlas::collection::CollectionSource;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::event::{ChangeEvent, ChangeOp};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::projector::aws::{
    EniFacts, InstanceFacts, InterfaceFacts, project_instance, project_interface, use_aws_resource,
    use_global,
};
use crate::cloud::amazon::sqs::feed::unwrap_sns;
use aws_smithy_types::date_time::{DateTime, Format};
use petgraph::graph::Graph;
use serde::Deserialize;
use serde_json::Value;
use std::fmt;

const SOURCE: CollectionSource = CollectionSource::Aws;

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

#[derive(Deserialize)]
struct Envelope {
    id: Option<String>,
    #[serde(rename = "detail-type")]
    detail_type: Option<String>,
    time: Option<String>,
    region: Option<String>,
    detail: Option<Value>,
}

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
        _ => Ok(Vec::new()),
    }
}

fn epoch_millis(timestamp: &str) -> Option<i64> {
    DateTime::from_str(timestamp, Format::DateTime)
        .ok()?
        .to_millis()
        .ok()
}

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

    fn configuration<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        match self.configuration.as_ref()? {
            Value::String(raw) => serde_json::from_str(raw).ok(),
            other => serde_json::from_value(other.clone()).ok(),
        }
    }
}

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

#[derive(Deserialize)]
struct NetworkInterfaceRef {
    #[serde(rename = "networkInterfaceId")]
    network_interface_id: Option<String>,
    #[serde(rename = "subnetId")]
    subnet_id: Option<String>,
    #[serde(rename = "privateIpAddress")]
    private_ip_address: Option<String>,
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

#[derive(Deserialize, Default)]
struct InterfaceConfiguration {
    #[serde(rename = "vpcId")]
    vpc_id: Option<String>,
    #[serde(rename = "subnetId")]
    subnet_id: Option<String>,
    description: Option<String>,
    #[serde(rename = "privateIpAddress")]
    private_ip_address: Option<String>,
    #[serde(rename = "privateIpAddresses")]
    private_ip_addresses: Option<Vec<PrivateIpRef>>,
    #[serde(rename = "ipv6Addresses")]
    ipv6_addresses: Option<Vec<Ipv6Ref>>,
    groups: Option<Vec<GroupRef>>,
    association: Option<AssociationRef>,
}

#[derive(Deserialize)]
struct PrivateIpRef {
    #[serde(rename = "privateIpAddress")]
    private_ip_address: Option<String>,
    association: Option<AssociationRef>,
}

#[derive(Deserialize)]
struct AssociationRef {
    #[serde(rename = "publicIp")]
    public_ip: Option<String>,
}

#[derive(Deserialize)]
struct Ipv6Ref {
    #[serde(rename = "ipv6Address")]
    ipv6_address: Option<String>,
}

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

    if let Some(node) = typed_node(&item, resource_type) {
        let context = typed_context(&item, region, resource_type, &node);
        events.push(
            ChangeEvent::new(SOURCE, region, id, observed_at, op, node).with_context(context),
        );
    }

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
        "AWS::EC2::NetworkInterface" => Node::AwsEc2Eni(id?.into()),
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
        "AWS::EC2::NetworkInterface" => interface_context(item, region, node),

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

        "AWS::EKS::Cluster"
        | "AWS::RDS::DBInstance"
        | "AWS::ElasticLoadBalancingV2::LoadBalancer" => linked(
            vpc().unwrap_or_else(region_node),
            node.clone(),
            Edge::Contains,
        ),

        "AWS::EC2::Subnet" | "AWS::EC2::RouteTable" => match vpc() {
            Some(vpc) => linked(vpc, node.clone(), Edge::Contains),
            None => solo(node.clone()),
        },

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
                Some(ip) => linked(node.clone(), Node::ip(&ip), Edge::ConnectsTo),
                None => solo(node.clone()),
            }
        }

        _ => solo(node.clone()),
    }
}

fn instance_context(item: &ConfigurationItem, region: &str) -> Graph<Node, Edge> {
    let config: InstanceConfiguration = item.configuration().unwrap_or_default();

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

    let mut network_interfaces: Vec<EniFacts<'_>> = config
        .network_interfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|eni| {
            Some(EniFacts {
                id: eni.network_interface_id.as_deref()?,
                subnet_id: eni.subnet_id.as_deref(),
                private_ip: eni.private_ip_address.as_deref(),
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
                private_ip: None,
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

// Both owner edges an interface could name here are deliberately absent, for
// the same reason the scan defers them to link_interface_owners: each needs a
// collection the event path does not have. The balancer would mean guessing a
// partition to rebuild an ARN, and the attachment would mean trusting that the
// instance is one the scan keeps — DescribeInstances is running/pending only,
// so a stopped instance's interface would invent the node. The next
// reconciliation adds whichever edge is real.
fn interface_context(item: &ConfigurationItem, region: &str, node: &Node) -> Graph<Node, Edge> {
    let Some(id) = item.resource_id.as_deref() else {
        return solo(node.clone());
    };
    let config: InterfaceConfiguration = item.configuration().unwrap_or_default();

    let mut addresses: Vec<&str> = Vec::new();
    for ip in config.private_ip_addresses.as_deref().unwrap_or_default() {
        addresses.extend(ip.private_ip_address.as_deref());
        addresses.extend(ip.association.as_ref().and_then(|a| a.public_ip.as_deref()));
    }
    addresses.extend(config.private_ip_address.as_deref());
    addresses.extend(
        config
            .association
            .as_ref()
            .and_then(|a| a.public_ip.as_deref()),
    );
    addresses.extend(
        config
            .ipv6_addresses
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|a| a.ipv6_address.as_deref()),
    );

    let mut security_group_ids: Vec<&str> = config
        .groups
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|group| group.group_id.as_deref())
        .collect();
    if security_group_ids.is_empty() {
        security_group_ids = item.related_all("AWS::EC2::SecurityGroup");
    }

    let mut builder = GraphBuilder::new();
    let region_idx = builder.get_or_add_node(Node::AwsRegion(region.into()));
    project_interface(
        &mut builder,
        region_idx,
        &InterfaceFacts {
            id,
            vpc_id: config
                .vpc_id
                .as_deref()
                .or_else(|| item.related("AWS::EC2::VPC")),
            subnet_id: config
                .subnet_id
                .as_deref()
                .or_else(|| item.related("AWS::EC2::Subnet")),
            description: config.description.as_deref(),
            addresses,
            security_group_ids,
        },
    );
    builder.graph
}

#[derive(Deserialize)]
struct InstanceStateDetail {
    #[serde(rename = "instance-id")]
    instance_id: Option<String>,
    state: Option<String>,
}

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

fn from_cloud_trail(
    detail: &Value,
    envelope_region: &str,
    id: &str,
    envelope_time: Option<i64>,
) -> Result<Vec<ChangeEvent>, MalformedEvent> {
    let detail: CloudTrailDetail = serde_json::from_value(detail.clone())
        .map_err(|e| malformed(format!("unreadable CloudTrail detail: {e}")))?;

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
                                private_ip: eni.private_ip_address.as_deref(),
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

pub mod stream {
    use super::{MalformedEvent, SOURCE, parse};
    use crate::atlas::collection::{CollectionReport, FailureKind};
    use crate::atlas::event::ChangeEvent;
    use crate::cloud::amazon::sqs::feed;
    use aws_sdk_sqs::Client;

    pub struct EventBatch {
        pub events: Vec<ChangeEvent>,
        pub report: CollectionReport,
    }

    pub struct EventQueue {
        client: Client,
        queue_url: String,
        region: String,
        exclude_by_default: bool,
        wait_time_seconds: i32,
    }

    impl EventQueue {
        pub const WAIT_SECONDS: i32 = 20;

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
                    let kind =
                        feed::failure_kind(error.raw_response().map(|r| r.status().as_u16()));
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

            feed::delete(
                &self.client,
                &self.queue_url,
                processed,
                &mut report,
                &self.scope(),
                "event",
            )
            .await;

            EventBatch { events, report }
        }
    }
}

#[cfg(test)]
mod tests;
