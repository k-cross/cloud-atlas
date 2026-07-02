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
    GenericIpAddress(String),

    // AWS
    AwsRegion(String),
    AwsGlobal(String),
    AwsTag(String),
    AwsEc2Instance(String),
    AwsEc2Vpc(String),
    AwsEc2Subnet(String),
    AwsEc2AvailabilityZone(String),
    AwsEc2SecurityGroup(String),
    AwsEc2Eni(String), // New for pivot
    AwsEcsCluster(String),
    AwsLambdaFunction(String),
    AwsIamRole(String),
    AwsEventbridgeBus(String),
    AwsElbLoadBalancer(String),
    AwsElbTargetGroup(String),
    AwsElbListener(String),
    AwsElbTargetHealth(String),
    AwsRoute53HostedZone(String),
    AwsRoute53RecordSet(String),
    AwsEksCluster(String),
    AwsApiGatewayRestApi(String),
    AwsRdsDbInstance(String),
    AwsDynamoDbTable(String),
    AwsSqsQueue(String),
    AwsSnsTopic(String),
    AwsCloudFrontDistribution(String),
    AwsConfigResource { resource_type: String, id: String }, // Catch-all for AWS config

    // GCP
    GcpProject(String),
    GcpComputeInstance(String),
    GcpComputeNetwork(String),
    GcpComputeSubnetwork(String),
    GcpComputeFirewall(String),
    GcpComputeForwardingRule(String),
    GcpComputeZone(String),
    GcpSqlInstance(String),
    GcpDnsManagedZone(String),
    GcpGkeCluster(String),
    GcpCloudFunction(String),
    GcpStorageBucket(String),
    GcpPubSubTopic(String),
    GcpPubSubSubscription(String),
    GcpCloudRunService(String),

    // Azure
    AzureVirtualMachine(String),
    AzureVirtualNetwork(String),
    AzureSubnet(String),
    AzureNetworkSecurityGroup(String),
    AzurePublicIpAddress(String),
    AzureStorageAccount(String),
    AzureManagedCluster(String), // AKS
    AzureSqlServer(String),
    AzureAppService(String),
    AzureFunctionApp(String),
    AzureApiManagement(String),
    AzureCosmosDb(String),
    AzureServiceBus(String),
    AzureEventGridTopic(String),
    AzureDnsZone(String),
    AzureCdnProfile(String),
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Output in the format: Type(ID)
        match self {
            // Generic
            Node::GenericIpAddress(id) => write!(f, "Generic::IpAddress({})", id),

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
