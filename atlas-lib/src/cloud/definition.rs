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

#[derive(Debug)]
pub enum Provider {
    AWS(Vec<(String, AmazonCollection)>),
    GCP(Vec<GoogleCollection>),
    Azure(Vec<MicrosoftCollection>),
    Cloudflare(Box<CloudflareCollection>),
}

/// Identity of a Cloudflare zone, as the key of [`CloudflareCollection::dns_records`].
/// A `Zone` carries both an `id` and a `name` and only the former keys the map,
/// so the newtype is what stops a projector looking up by the wrong one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ZoneId(pub String);

/// Identity of a Worker script. Script names are unique per *account*, not
/// globally, so the account travels with the name — two accounts owning an
/// "api" worker must not collide in [`CloudflareCollection::worker_bindings`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptId {
    pub account: String,
    pub script: String,
}

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
    AmazonLoadBalancers {
        load_balancers: Vec<AWSLoadBalancer>,
        target_groups: Vec<AWSTargetGroup>,
        listeners: Vec<AWSListener>,
        target_health: HashMap<String, Vec<AWSTargetHealthDescription>>,
    },
    AmazonRoute53 {
        hosted_zones: Vec<aws_sdk_route53::types::HostedZone>,
        record_sets: Vec<aws_sdk_route53::types::ResourceRecordSet>,
    },
    AmazonEks(Vec<aws_sdk_eks::types::Cluster>),
    AmazonApiGateway(Vec<aws_sdk_apigateway::types::RestApi>),
    AmazonRds(Vec<aws_sdk_rds::types::DbInstance>),
    AmazonDynamoDb(Vec<TableName>),
    AmazonSqs(Vec<QueueUrl>),
    AmazonSns(Vec<aws_sdk_sns::types::Topic>),
    AmazonCloudFront(Vec<aws_sdk_cloudfront::types::DistributionSummary>),
    AmazonSecurityGroups(Vec<aws_sdk_ec2::types::SecurityGroup>),
    // L3 routing / egress plane: how a subnet actually reaches the internet.
    AmazonNetworking {
        route_tables: Vec<aws_sdk_ec2::types::RouteTable>,
        internet_gateways: Vec<aws_sdk_ec2::types::InternetGateway>,
        nat_gateways: Vec<aws_sdk_ec2::types::NatGateway>,
        addresses: Vec<aws_sdk_ec2::types::Address>, // Elastic IPs
    },
}

#[derive(Debug)]
pub enum GoogleCollection {
    GoogleInstances(Vec<crate::api::google::compute::Instance>),
    GoogleFirewalls(Vec<crate::api::google::compute::Firewall>),
    GoogleSql(Vec<crate::api::google::sql::SqlInstance>),
    GoogleDns(Vec<crate::api::google::dns::ManagedZone>),
    GoogleGke(Vec<crate::api::google::gke::Cluster>),
    GoogleFunctions(Vec<crate::api::google::functions::CloudFunction>),
    GoogleStorageBuckets(Vec<crate::api::google::storage::Bucket>),
    GooglePubSubTopics(Vec<crate::api::google::pubsub::Topic>),
    GooglePubSubSubscriptions(Vec<crate::api::google::pubsub::Subscription>),
    GoogleRunServices(Vec<crate::api::google::run::Service>),
    GoogleNetworks(Vec<crate::api::google::compute_network::Network>),
    GoogleSubnetworks(Vec<crate::api::google::compute_network::Subnetwork>),
    GoogleForwardingRules(Vec<crate::api::google::compute_network::ForwardingRule>),
}

#[derive(Debug)]
pub enum MicrosoftCollection {
    AzureVirtualMachines(Vec<crate::api::azure::models::VirtualMachine>),
    AzureVirtualNetworks(Vec<crate::api::azure::models::VirtualNetwork>),
    AzureSubnets(Vec<crate::api::azure::models::Subnet>),
    AzureNetworkSecurityGroups(Vec<crate::api::azure::models::NetworkSecurityGroup>),
    AzurePublicIpAddresses(Vec<crate::api::azure::models::PublicIpAddress>),
    AzureStorageAccounts(Vec<crate::api::azure::models::StorageAccount>),
    AzureManagedClusters(Vec<crate::api::azure::models::ManagedCluster>),
    AzureSqlServers(Vec<crate::api::azure::models::SqlServer>),
    AzureAppServices(Vec<crate::api::azure::models::AppService>),
    AzureFunctionApps(Vec<crate::api::azure::models::FunctionApp>),
    AzureApiManagement(Vec<crate::api::azure::models::ApiManagement>),
    AzureCosmosDbs(Vec<crate::api::azure::models::CosmosDb>),
    AzureServiceBuses(Vec<crate::api::azure::models::ServiceBus>),
    AzureEventGridTopics(Vec<crate::api::azure::models::EventGridTopic>),
    AzureDnsZones(Vec<crate::api::azure::models::DnsZone>),
    AzureCdnProfiles(Vec<crate::api::azure::models::CdnProfile>),
}

#[derive(Debug, Default)]
pub struct CloudflareCollection {
    pub zones: Vec<cloudflare::endpoints::zones::zone::Zone>,
    pub dns_records: HashMap<ZoneId, Vec<cloudflare::endpoints::dns::dns::DnsRecord>>,
    pub workers: Vec<ScriptId>,
    pub kv_namespaces: Vec<cloudflare::endpoints::workerskv::WorkersKvNamespace>,
    pub r2_buckets: Vec<cloudflare::endpoints::r2::r2::Bucket>,
    pub durable_objects: Vec<crate::cloud::cloudflare::durable_objects::DurableObjectNamespace>,
    pub d1_databases: Vec<crate::cloud::cloudflare::d1::D1Database>,
    pub worker_bindings: HashMap<ScriptId, Vec<crate::cloud::cloudflare::worker::WorkerBinding>>,
}
