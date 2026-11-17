use aws_sdk_config::types::ResourceIdentifier as AWSResource;
use aws_sdk_ec2::types::Instance as AWSInstance;
use aws_sdk_ecs::types::Cluster as AWSCluster;
use aws_sdk_elasticloadbalancingv2::types::{
    Listener as AWSListener, LoadBalancer as AWSLoadBalancer, TargetGroup as AWSTargetGroup,
    TargetHealthDescription as AWSTargetHealthDescription,
};
use aws_sdk_eventbridge::types::EventBus as AWSEventbridge;
use aws_sdk_lambda::types::FunctionConfiguration as AWSLambda;
use std::collections::HashMap;

#[derive(Debug)]
pub enum CloudError {
    AwsEC2Error(aws_sdk_ec2::Error),
    AwsConfigError(aws_sdk_config::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostedZoneId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TableName(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueueUrl(pub String);

impl std::fmt::Display for TableName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for QueueUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug)]
pub enum AmazonCollection {
    AmazonInstances(Vec<AWSInstance>),
    AmazonClusters(Vec<AWSCluster>),
    AmazonLambdas(Vec<AWSLambda>),
    AmazonEventbridge(Vec<AWSEventbridge>),
    AmazonResources(HashMap<String, Vec<AWSResource>>),
    AmazonLoadBalancers(AWSLoadBalancing),
    AmazonRoute53(AWSRoute53),
    AmazonEks(Vec<aws_sdk_eks::types::Cluster>),
    AmazonApiGateway(Vec<aws_sdk_apigateway::types::RestApi>),
    AmazonRds(Vec<aws_sdk_rds::types::DbInstance>),
    AmazonDynamoDb(Vec<TableName>),
    AmazonSqs(Vec<QueueUrl>),
    AmazonSns(Vec<aws_sdk_sns::types::Topic>),
    AmazonCloudFront(Vec<aws_sdk_cloudfront::types::DistributionSummary>),
    AmazonSecurityGroups(Vec<aws_sdk_ec2::types::SecurityGroup>),
    AmazonNetworking(AWSNetworking),
    AmazonNetworkInterfaces(Vec<aws_sdk_ec2::types::NetworkInterface>),
    AmazonConnectivity(AWSConnectivity),
    AmazonTransitGatewayRouting(AWSTransitGatewayRouting),
    AmazonDirectConnect(AWSDirectConnect),
    AmazonDirectConnectGateways(AWSDirectConnectGateways),
}

#[derive(Debug)]
pub struct AWSLoadBalancing {
    pub load_balancers: Vec<AWSLoadBalancer>,
    pub target_groups: Vec<AWSTargetGroup>,
    pub listeners: Vec<AWSListener>,
    pub target_health: HashMap<String, Vec<AWSTargetHealthDescription>>,
}

#[derive(Debug)]
pub struct AWSRoute53 {
    pub hosted_zones: Vec<aws_sdk_route53::types::HostedZone>,
    pub record_sets: HashMap<HostedZoneId, Vec<aws_sdk_route53::types::ResourceRecordSet>>,
}

#[derive(Debug)]
pub struct AWSNetworking {
    pub route_tables: Vec<aws_sdk_ec2::types::RouteTable>,
    pub internet_gateways: Vec<aws_sdk_ec2::types::InternetGateway>,
    pub nat_gateways: Vec<aws_sdk_ec2::types::NatGateway>,
    pub addresses: Vec<aws_sdk_ec2::types::Address>,
}

#[derive(Debug, Default)]
pub struct AWSConnectivity {
    pub vpcs: Vec<aws_sdk_ec2::types::Vpc>,
    pub peering_connections: Vec<aws_sdk_ec2::types::VpcPeeringConnection>,
    pub transit_gateways: Vec<aws_sdk_ec2::types::TransitGateway>,
    pub transit_gateway_attachments: Vec<aws_sdk_ec2::types::TransitGatewayAttachment>,
    pub vpn_gateways: Vec<aws_sdk_ec2::types::VpnGateway>,
    pub customer_gateways: Vec<aws_sdk_ec2::types::CustomerGateway>,
    pub vpn_connections: Vec<aws_sdk_ec2::types::VpnConnection>,
}

#[derive(Debug, Default)]
pub struct AWSTransitGatewayRouting {
    pub route_tables: Vec<AWSTransitGatewayRouteTable>,
    pub peerings: Vec<aws_sdk_ec2::types::TransitGatewayPeeringAttachment>,
}

#[derive(Debug)]
pub struct AWSTransitGatewayRouteTable {
    pub table: aws_sdk_ec2::types::TransitGatewayRouteTable,
    pub associations: Vec<aws_sdk_ec2::types::TransitGatewayRouteTableAssociation>,
    pub propagations: Vec<aws_sdk_ec2::types::TransitGatewayRouteTablePropagation>,
    pub static_routes: Vec<aws_sdk_ec2::types::TransitGatewayRoute>,
}

#[derive(Debug, Default)]
pub struct AWSDirectConnect {
    pub connections: Vec<aws_sdk_directconnect::types::Connection>,
    pub virtual_interfaces: Vec<aws_sdk_directconnect::types::VirtualInterface>,
}

#[derive(Debug, Default)]
pub struct AWSDirectConnectGateways {
    pub gateways: Vec<aws_sdk_directconnect::types::DirectConnectGateway>,
    pub associations: Vec<aws_sdk_directconnect::types::DirectConnectGatewayAssociation>,
}
