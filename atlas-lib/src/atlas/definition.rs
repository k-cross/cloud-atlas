use std::fmt;

/// A node in the property graph, representing a semantic cloud resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Node {
    // Generic
    GenericIpAddress(std::sync::Arc<str>),
    GenericHostname(std::sync::Arc<str>),

    // AWS
    AwsRegion(std::sync::Arc<str>),
    AwsTag {
        key: std::sync::Arc<str>,
        value: std::sync::Arc<str>,
    },
    AwsEc2Instance(std::sync::Arc<str>),
    AwsEc2Vpc(std::sync::Arc<str>),
    AwsEc2Subnet(std::sync::Arc<str>),
    AwsEc2AvailabilityZone(std::sync::Arc<str>),
    AwsEc2SecurityGroup(std::sync::Arc<str>),
    AwsEc2Eni(std::sync::Arc<str>), // New for pivot
    // L3 routing / egress plane
    AwsEc2RouteTable(std::sync::Arc<str>),
    AwsEc2InternetGateway(std::sync::Arc<str>),
    AwsEc2NatGateway(std::sync::Arc<str>),
    AwsEc2Eip(std::sync::Arc<str>),
    AwsEcsCluster(std::sync::Arc<str>),
    AwsLambdaFunction(std::sync::Arc<str>),
    AwsIamRole(std::sync::Arc<str>),
    AwsElbLoadBalancer(std::sync::Arc<str>),
    AwsElbTargetGroup(std::sync::Arc<str>),
    AwsRoute53HostedZone(std::sync::Arc<str>),
    AwsRoute53RecordSet(std::sync::Arc<str>),
    AwsEksCluster(std::sync::Arc<str>),
    AwsApiGatewayRestApi(std::sync::Arc<str>),
    AwsRdsDbInstance(std::sync::Arc<str>),
    AwsDynamoDbTable(std::sync::Arc<str>),
    AwsSqsQueue(std::sync::Arc<str>),
    AwsSnsTopic(std::sync::Arc<str>),
    AwsCloudFrontDistribution(std::sync::Arc<str>),
    AwsConfigResource {
        resource_type: std::sync::Arc<str>,
        id: std::sync::Arc<str>,
    }, // Catch-all for AWS config

    // GCP
    GcpProject(std::sync::Arc<str>),
    GcpComputeInstance(std::sync::Arc<str>),
    GcpComputeNetwork(std::sync::Arc<str>),
    GcpComputeSubnetwork(std::sync::Arc<str>),
    GcpComputeFirewall(std::sync::Arc<str>),
    GcpComputeForwardingRule(std::sync::Arc<str>),
    GcpComputeZone(std::sync::Arc<str>),
    GcpSqlInstance(std::sync::Arc<str>),
    GcpDnsManagedZone(std::sync::Arc<str>),
    GcpGkeCluster(std::sync::Arc<str>),
    GcpCloudFunction(std::sync::Arc<str>),
    GcpStorageBucket(std::sync::Arc<str>),
    GcpPubSubTopic(std::sync::Arc<str>),
    GcpPubSubSubscription(std::sync::Arc<str>),
    GcpCloudRunService(std::sync::Arc<str>),

    // Azure
    AzureVirtualMachine(std::sync::Arc<str>),
    AzureVirtualNetwork(std::sync::Arc<str>),
    AzureSubnet(std::sync::Arc<str>),
    AzureNetworkInterface(std::sync::Arc<str>),
    AzureNetworkSecurityGroup(std::sync::Arc<str>),
    AzurePublicIpAddress(std::sync::Arc<str>),
    AzureStorageAccount(std::sync::Arc<str>),
    AzureManagedCluster(std::sync::Arc<str>), // AKS
    AzureSqlServer(std::sync::Arc<str>),
    AzureAppService(std::sync::Arc<str>),
    AzureFunctionApp(std::sync::Arc<str>),
    AzureApiManagement(std::sync::Arc<str>),
    AzureCosmosDb(std::sync::Arc<str>),
    AzureServiceBus(std::sync::Arc<str>),
    AzureEventGridTopic(std::sync::Arc<str>),
    AzureDnsZone(std::sync::Arc<str>),
    AzureCdnProfile(std::sync::Arc<str>),
    AzureServiceTag(std::sync::Arc<str>),

    // Cloudflare
    CloudflareZone(std::sync::Arc<str>),
    CloudflareDnsRecord(std::sync::Arc<str>),
    CloudflareWorker(std::sync::Arc<str>),
    CloudflareDurableObject(std::sync::Arc<str>),
    CloudflareKvNamespace(std::sync::Arc<str>),
    CloudflareR2Bucket(std::sync::Arc<str>),
    CloudflareD1Database(std::sync::Arc<str>),
    ExternalService(std::sync::Arc<str>),
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Output in the format: Type(ID)
        match self {
            // Generic
            Node::GenericIpAddress(id) => write!(f, "Generic::IpAddress({})", id),
            Node::GenericHostname(id) => write!(f, "Generic::Hostname({})", id),

            // AWS
            Node::AwsRegion(id) => write!(f, "AWS::Region({})", id),
            Node::AwsTag { key, value } => write!(f, "AWS::Tag({}={})", key, value),
            Node::AwsEc2Instance(id) => write!(f, "AWS::Ec2Instance({})", id),
            Node::AwsEc2Vpc(id) => write!(f, "AWS::EC2::VPC({})", id),
            Node::AwsEc2Subnet(id) => write!(f, "AWS::EC2::Subnet({})", id),
            Node::AwsEc2AvailabilityZone(id) => write!(f, "AWS::EC2::AvailabilityZone({})", id),
            Node::AwsEc2SecurityGroup(id) => write!(f, "AWS::Ec2SecurityGroup({})", id),
            Node::AwsEc2Eni(id) => write!(f, "AWS::Ec2Eni({}-eni)", id), // preserve the -eni suffix for visual display without allocation
            Node::AwsEc2RouteTable(id) => write!(f, "AWS::EC2::RouteTable({})", id),
            Node::AwsEc2InternetGateway(id) => write!(f, "AWS::EC2::InternetGateway({})", id),
            Node::AwsEc2NatGateway(id) => write!(f, "AWS::EC2::NatGateway({})", id),
            Node::AwsEc2Eip(id) => write!(f, "AWS::EC2::Eip({})", id),
            Node::AwsEcsCluster(id) => write!(f, "AWS::EcsCluster({})", id),
            Node::AwsLambdaFunction(id) => write!(f, "AWS::Lambda::Function({})", id),
            Node::AwsIamRole(id) => write!(f, "AWS::IAM::Role({})", id),
            Node::AwsElbLoadBalancer(id) => write!(f, "AWS::ELB::LoadBalancer({})", id),
            Node::AwsElbTargetGroup(id) => write!(f, "AWS::ELB::TargetGroup({})", id),
            Node::AwsRoute53HostedZone(id) => write!(f, "AWS::Route53::HostedZone({})", id),
            Node::AwsRoute53RecordSet(id) => write!(f, "AWS::Route53::RecordSet({})", id),
            Node::AwsEksCluster(id) => write!(f, "AWS::EKS::Cluster({})", id),
            Node::AwsApiGatewayRestApi(id) => write!(f, "AWS::ApiGateway::RestApi({})", id),
            Node::AwsRdsDbInstance(id) => write!(f, "AWS::RDS::DbInstance({})", id),
            Node::AwsDynamoDbTable(id) => write!(f, "AWS::DynamoDb::Table({})", id),
            Node::AwsSqsQueue(id) => write!(f, "AWS::SQS::Queue({})", id),
            Node::AwsSnsTopic(id) => write!(f, "AWS::SNS::Topic({})", id),
            Node::AwsCloudFrontDistribution(id) => {
                write!(f, "AWS::CloudFront::Distribution({})", id)
            }
            Node::AwsConfigResource { resource_type, id } => write!(f, "{}({})", resource_type, id),

            // GCP
            Node::GcpProject(id) => write!(f, "GCP::Project({})", id),
            Node::GcpComputeInstance(id) => write!(f, "GCP::Compute::Instance({})", id),
            Node::GcpComputeNetwork(id) => write!(f, "GCP::Compute::Network({})", id),
            Node::GcpComputeSubnetwork(id) => write!(f, "GCP::Compute::Subnetwork({})", id),
            Node::GcpComputeFirewall(id) => write!(f, "GCP::Compute::Firewall({})", id),
            Node::GcpComputeForwardingRule(id) => write!(f, "GCP::Compute::ForwardingRule({})", id),
            Node::GcpComputeZone(id) => write!(f, "GCP::Compute::Zone({})", id),
            Node::GcpSqlInstance(id) => write!(f, "GCP::SQL::Instance({})", id),
            Node::GcpDnsManagedZone(id) => write!(f, "GCP::DNS::ManagedZone({})", id),
            Node::GcpGkeCluster(id) => write!(f, "GCP::GKE::Cluster({})", id),
            Node::GcpCloudFunction(id) => write!(f, "GCP::CloudFunctions::Function({})", id),
            Node::GcpStorageBucket(id) => write!(f, "GCP::Storage::Bucket({})", id),
            Node::GcpPubSubTopic(id) => write!(f, "GCP::PubSub::Topic({})", id),
            Node::GcpPubSubSubscription(id) => write!(f, "GCP::PubSub::Subscription({})", id),
            Node::GcpCloudRunService(id) => write!(f, "GCP::CloudRun::Service({})", id),

            // Azure
            Node::AzureVirtualMachine(id) => write!(f, "Azure::Compute::VirtualMachine({})", id),
            Node::AzureVirtualNetwork(id) => write!(f, "Azure::Network::VirtualNetwork({})", id),
            Node::AzureSubnet(id) => write!(f, "Azure::Network::Subnet({})", id),
            Node::AzureNetworkInterface(id) => {
                write!(f, "Azure::Network::NetworkInterface({})", id)
            }
            Node::AzureNetworkSecurityGroup(id) => {
                write!(f, "Azure::Network::NetworkSecurityGroup({})", id)
            }
            Node::AzurePublicIpAddress(id) => write!(f, "Azure::Network::PublicIpAddress({})", id),
            Node::AzureStorageAccount(id) => write!(f, "Azure::Storage::StorageAccount({})", id),
            Node::AzureManagedCluster(id) => write!(f, "Azure::Containers::ManagedCluster({})", id),
            Node::AzureSqlServer(id) => write!(f, "Azure::Databases::SqlServer({})", id),
            Node::AzureAppService(id) => write!(f, "Azure::Web::AppService({})", id),
            Node::AzureFunctionApp(id) => write!(f, "Azure::Web::FunctionApp({})", id),
            Node::AzureApiManagement(id) => write!(f, "Azure::Web::ApiManagement({})", id),
            Node::AzureCosmosDb(id) => write!(f, "Azure::Databases::CosmosDb({})", id),
            Node::AzureServiceBus(id) => write!(f, "Azure::Integration::ServiceBus({})", id),
            Node::AzureEventGridTopic(id) => {
                write!(f, "Azure::Integration::EventGridTopic({})", id)
            }
            Node::AzureDnsZone(id) => write!(f, "Azure::Network::DnsZone({})", id),
            Node::AzureCdnProfile(id) => write!(f, "Azure::Network::CdnProfile({})", id),
            Node::AzureServiceTag(id) => write!(f, "Azure::Network::ServiceTag({})", id),

            // Cloudflare
            Node::CloudflareZone(id) => write!(f, "Cloudflare::Zone({})", id),
            Node::CloudflareDnsRecord(id) => write!(f, "Cloudflare::DnsRecord({})", id),
            Node::CloudflareWorker(id) => write!(f, "Cloudflare::Worker({})", id),
            Node::CloudflareDurableObject(id) => write!(f, "Cloudflare::DurableObject({})", id),
            Node::CloudflareKvNamespace(id) => write!(f, "Cloudflare::KvNamespace({})", id),
            Node::CloudflareR2Bucket(id) => write!(f, "Cloudflare::R2Bucket({})", id),
            Node::CloudflareD1Database(id) => write!(f, "Cloudflare::D1Database({})", id),
            Node::ExternalService(id) => write!(f, "External::Service({})", id),
        }
    }
}

/// Edge types for the topology graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Edge {
    Contains,   // Hierarchical containment (e.g. VPC -> Subnet)
    ConnectsTo, // Routing/Traffic flow (e.g. Subnet -> ENI)
    DependsOn,  // Logical dependency
    AttachedTo, // Hardware/Logical attachment (e.g. ENI -> Subnet)
    HasIp,      // Semantic IP relationship (e.g. Instance -> HasIp)
    RoutesTo,   // Traffic routing
    ResolvesTo, // DNS resolution
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Generates `ALL_KINDS` and an exhaustive `kind()` from a single variant
/// list. The `kind()` match is exhaustive, so adding an enum variant without
/// listing it here is a compile error — and once listed, the fixture
/// exhaustiveness tests require it to actually appear in the graph.
macro_rules! kinds {
    ($ty:ident, $($variant:ident),* $(,)?) => {
        impl $ty {
            pub const ALL_KINDS: &'static [&'static str] = &[$(stringify!($variant)),*];

            pub fn kind(&self) -> &'static str {
                match self {
                    $($ty::$variant { .. } => stringify!($variant)),*
                }
            }
        }
    };
}

kinds!(
    Node,
    // Generic
    GenericIpAddress,
    GenericHostname,
    // AWS
    AwsRegion,
    AwsTag,
    AwsEc2Instance,
    AwsEc2Vpc,
    AwsEc2Subnet,
    AwsEc2AvailabilityZone,
    AwsEc2SecurityGroup,
    AwsEc2Eni,
    AwsEc2RouteTable,
    AwsEc2InternetGateway,
    AwsEc2NatGateway,
    AwsEc2Eip,
    AwsEcsCluster,
    AwsLambdaFunction,
    AwsIamRole,
    AwsElbLoadBalancer,
    AwsElbTargetGroup,
    AwsRoute53HostedZone,
    AwsRoute53RecordSet,
    AwsEksCluster,
    AwsApiGatewayRestApi,
    AwsRdsDbInstance,
    AwsDynamoDbTable,
    AwsSqsQueue,
    AwsSnsTopic,
    AwsCloudFrontDistribution,
    AwsConfigResource,
    // GCP
    GcpProject,
    GcpComputeInstance,
    GcpComputeNetwork,
    GcpComputeSubnetwork,
    GcpComputeFirewall,
    GcpComputeForwardingRule,
    GcpComputeZone,
    GcpSqlInstance,
    GcpDnsManagedZone,
    GcpGkeCluster,
    GcpCloudFunction,
    GcpStorageBucket,
    GcpPubSubTopic,
    GcpPubSubSubscription,
    GcpCloudRunService,
    // Azure
    AzureVirtualMachine,
    AzureVirtualNetwork,
    AzureSubnet,
    AzureNetworkInterface,
    AzureNetworkSecurityGroup,
    AzurePublicIpAddress,
    AzureStorageAccount,
    AzureManagedCluster,
    AzureSqlServer,
    AzureAppService,
    AzureFunctionApp,
    AzureApiManagement,
    AzureCosmosDb,
    AzureServiceBus,
    AzureEventGridTopic,
    AzureDnsZone,
    AzureCdnProfile,
    AzureServiceTag,
    // Cloudflare
    CloudflareZone,
    CloudflareDnsRecord,
    CloudflareWorker,
    CloudflareDurableObject,
    CloudflareKvNamespace,
    CloudflareR2Bucket,
    CloudflareD1Database,
    ExternalService,
);

kinds!(
    Edge, Contains, ConnectsTo, DependsOn, AttachedTo, HasIp, RoutesTo, ResolvesTo,
);
