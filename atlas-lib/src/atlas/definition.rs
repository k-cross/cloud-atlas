use std::fmt;

/// The cloud provider this resource belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Provider {
    Aws,
    Gcp,
    Azure,
    Hetzner,
    DigitalOcean,
    MsGraph,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A node in the property graph, representing a semantic cloud resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Node {
    // Generic
    GenericIpAddress(std::sync::Arc<str>),
    GenericHostname(std::sync::Arc<str>),

    // AWS
    AwsRegion(std::sync::Arc<str>),
    AwsGlobal(std::sync::Arc<str>),
    AwsTag(std::sync::Arc<str>),
    AwsEc2Instance(std::sync::Arc<str>),
    AwsEc2Vpc(std::sync::Arc<str>),
    AwsEc2Subnet(std::sync::Arc<str>),
    AwsEc2AvailabilityZone(std::sync::Arc<str>),
    AwsEc2SecurityGroup(std::sync::Arc<str>),
    AwsEc2Eni(std::sync::Arc<str>), // New for pivot
    AwsEcsCluster(std::sync::Arc<str>),
    AwsLambdaFunction(std::sync::Arc<str>),
    AwsIamRole(std::sync::Arc<str>),
    AwsEventbridgeBus(std::sync::Arc<str>),
    AwsElbLoadBalancer(std::sync::Arc<str>),
    AwsElbTargetGroup(std::sync::Arc<str>),
    AwsElbListener(std::sync::Arc<str>),
    AwsElbTargetHealth(std::sync::Arc<str>),
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
            Node::AwsGlobal(id) => write!(f, "AWS::Global({})", id),
            Node::AwsTag(id) => write!(f, "AWS::Tag({})", id),
            Node::AwsEc2Instance(id) => write!(f, "AWS::EC2::Instance({})", id),
            Node::AwsEc2Vpc(id) => write!(f, "AWS::EC2::VPC({})", id),
            Node::AwsEc2Subnet(id) => write!(f, "AWS::EC2::Subnet({})", id),
            Node::AwsEc2AvailabilityZone(id) => write!(f, "AWS::EC2::AvailabilityZone({})", id),
            Node::AwsEc2SecurityGroup(id) => write!(f, "AWS::EC2::SecurityGroup({})", id),
            Node::AwsEc2Eni(id) => write!(f, "AWS::EC2::ENI({})", id),
            Node::AwsEcsCluster(id) => write!(f, "AWS::ECS::Cluster({})", id),
            Node::AwsLambdaFunction(id) => write!(f, "AWS::Lambda::Function({})", id),
            Node::AwsIamRole(id) => write!(f, "AWS::IAM::Role({})", id),
            Node::AwsEventbridgeBus(id) => write!(f, "AWS::Eventbridge::Bus({})", id),
            Node::AwsElbLoadBalancer(id) => write!(f, "AWS::ELB::LoadBalancer({})", id),
            Node::AwsElbTargetGroup(id) => write!(f, "AWS::ELB::TargetGroup({})", id),
            Node::AwsElbListener(id) => write!(f, "AWS::ELB::Listener({})", id),
            Node::AwsElbTargetHealth(id) => write!(f, "AWS::ELB::TargetHealth({})", id),
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
            Node::AzureNetworkSecurityGroup(id) => {
                write!(f, "Azure::Network::NetworkSecurityGroup({})", id)
            }
            Node::AzurePublicIpAddress(id) => write!(f, "Azure::Network::PublicIpAddress({})", id),
            Node::AzureStorageAccount(id) => write!(f, "Azure::Storage::StorageAccount({})", id),
            Node::AzureManagedCluster(id) => write!(f, "Azure::Containers::ManagedCluster({})", id),
            Node::AzureSqlServer(id) => write!(f, "Azure::Sql::Server({})", id),
            Node::AzureAppService(id) => write!(f, "Azure::Web::AppService({})", id),
            Node::AzureFunctionApp(id) => write!(f, "Azure::Web::FunctionApp({})", id),
            Node::AzureApiManagement(id) => write!(f, "Azure::ApiManagement::Service({})", id),
            Node::AzureCosmosDb(id) => write!(f, "Azure::DocumentDb::DatabaseAccount({})", id),
            Node::AzureServiceBus(id) => write!(f, "Azure::ServiceBus::Namespace({})", id),
            Node::AzureEventGridTopic(id) => write!(f, "Azure::EventGrid::Topic({})", id),
            Node::AzureDnsZone(id) => write!(f, "Azure::Network::DnsZone({})", id),
            Node::AzureCdnProfile(id) => write!(f, "Azure::Cdn::Profile({})", id),
        }
    }
}

/// Edge types for the topology graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Edge {
    Contains,   // Hierarchical containment (e.g. VPC -> Subnet)
    ConnectsTo, // Routing/Traffic flow (e.g. Subnet -> ENI)
    DependsOn,  // Logical dependency
    Manages,    // Management relationship
    AttachedTo, // Hardware/Logical attachment (e.g. ENI -> Subnet)
    HasIp,      // Semantic IP relationship (e.g. Instance -> HasIp)
    RoutesTo,   // Traffic routing
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
