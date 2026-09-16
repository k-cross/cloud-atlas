use crate::atlas::collection::CollectionSource;
use crate::atlas::util::{canonical_address, canonical_hostname};
use std::fmt;
use std::sync::Arc;

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

macro_rules! node_id {
    ($field:ident) => {
        std::sync::Arc<str>
    };
}

macro_rules! nodes {
    ($($owner:expr => [
        $($variant:ident
            $(($($tuple_field:ident),+))?
            $({$($struct_field:ident),+})?
            => $label:literal),* $(,)?
    ]),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum Node {
            $($(
                $variant
                    $(($(node_id!($tuple_field)),+))?
                    $({$($struct_field: node_id!($struct_field)),+})?,
            )*)+
        }

        impl fmt::Display for Node {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $($(
                        Node::$variant
                            $(($($tuple_field),+))?
                            $({$($struct_field),+})?
                            => write!(f, $label),
                    )*)+
                }
            }
        }

        impl Node {
            pub fn owner(&self) -> Option<CollectionSource> {
                match self {
                    $($(Node::$variant { .. } => $owner),*),+
                }
            }
        }

        kinds!(Node, $($($variant),*),+);
    };
}

nodes!(
    None => [
        GenericIpAddress(id) => "Generic::IpAddress({id})",
        GenericHostname(id) => "Generic::Hostname({id})",
        ExternalService(id) => "External::Service({id})",
    ],
    Some(CollectionSource::Aws) => [
        AwsRegion(id) => "AWS::Region({id})",
        AwsTag { key, value } => "AWS::Tag({key}={value})",
        AwsEc2Instance(id) => "AWS::Ec2Instance({id})",
        AwsEc2Vpc(id) => "AWS::EC2::VPC({id})",
        AwsEc2Subnet(id) => "AWS::EC2::Subnet({id})",
        AwsEc2AvailabilityZone(id) => "AWS::EC2::AvailabilityZone({id})",
        AwsEc2SecurityGroup(id) => "AWS::Ec2SecurityGroup({id})",
        AwsEc2Eni(id) => "AWS::Ec2Eni({id})",
        AwsEc2RouteTable(id) => "AWS::EC2::RouteTable({id})",
        AwsEc2InternetGateway(id) => "AWS::EC2::InternetGateway({id})",
        AwsEc2NatGateway(id) => "AWS::EC2::NatGateway({id})",
        AwsEc2Eip(id) => "AWS::EC2::Eip({id})",
        AwsEcsCluster(id) => "AWS::EcsCluster({id})",
        AwsLambdaFunction(id) => "AWS::Lambda::Function({id})",
        AwsIamRole(id) => "AWS::IAM::Role({id})",
        AwsElbLoadBalancer(id) => "AWS::ELB::LoadBalancer({id})",
        AwsElbTargetGroup(id) => "AWS::ELB::TargetGroup({id})",
        AwsRoute53HostedZone(id) => "AWS::Route53::HostedZone({id})",
        AwsRoute53RecordSet(id) => "AWS::Route53::RecordSet({id})",
        AwsEksCluster(id) => "AWS::EKS::Cluster({id})",
        AwsApiGatewayRestApi(id) => "AWS::ApiGateway::RestApi({id})",
        AwsRdsDbInstance(id) => "AWS::RDS::DbInstance({id})",
        AwsDynamoDbTable(id) => "AWS::DynamoDb::Table({id})",
        AwsSqsQueue(id) => "AWS::SQS::Queue({id})",
        AwsSnsTopic(id) => "AWS::SNS::Topic({id})",
        AwsCloudFrontDistribution(id) => "AWS::CloudFront::Distribution({id})",
        AwsConfigResource { resource_type, id } => "{resource_type}({id})",
    ],
    Some(CollectionSource::Gcp) => [
        GcpProject(id) => "GCP::Project({id})",
        GcpComputeInstance(id) => "GCP::Compute::Instance({id})",
        GcpComputeNetwork(id) => "GCP::Compute::Network({id})",
        GcpComputeSubnetwork(id) => "GCP::Compute::Subnetwork({id})",
        GcpComputeFirewall(id) => "GCP::Compute::Firewall({id})",
        GcpComputeForwardingRule(id) => "GCP::Compute::ForwardingRule({id})",
        GcpComputeZone(id) => "GCP::Compute::Zone({id})",
        GcpSqlInstance(id) => "GCP::SQL::Instance({id})",
        GcpDnsManagedZone(id) => "GCP::DNS::ManagedZone({id})",
        GcpGkeCluster(id) => "GCP::GKE::Cluster({id})",
        GcpCloudFunction(id) => "GCP::CloudFunctions::Function({id})",
        GcpStorageBucket(id) => "GCP::Storage::Bucket({id})",
        GcpPubSubTopic(id) => "GCP::PubSub::Topic({id})",
        GcpPubSubSubscription(id) => "GCP::PubSub::Subscription({id})",
        GcpCloudRunService(id) => "GCP::CloudRun::Service({id})",
    ],
    Some(CollectionSource::Azure) => [
        AzureVirtualMachine(id) => "Azure::Compute::VirtualMachine({id})",
        AzureVirtualNetwork(id) => "Azure::Network::VirtualNetwork({id})",
        AzureSubnet(id) => "Azure::Network::Subnet({id})",
        AzureNetworkInterface(id) => "Azure::Network::NetworkInterface({id})",
        AzureNetworkSecurityGroup(id) => "Azure::Network::NetworkSecurityGroup({id})",
        AzurePublicIpAddress(id) => "Azure::Network::PublicIpAddress({id})",
        AzureStorageAccount(id) => "Azure::Storage::StorageAccount({id})",
        AzureManagedCluster(id) => "Azure::Containers::ManagedCluster({id})",
        AzureSqlServer(id) => "Azure::Databases::SqlServer({id})",
        AzureAppService(id) => "Azure::Web::AppService({id})",
        AzureFunctionApp(id) => "Azure::Web::FunctionApp({id})",
        AzureApiManagement(id) => "Azure::Web::ApiManagement({id})",
        AzureCosmosDb(id) => "Azure::Databases::CosmosDb({id})",
        AzureServiceBus(id) => "Azure::Integration::ServiceBus({id})",
        AzureEventGridTopic(id) => "Azure::Integration::EventGridTopic({id})",
        AzureDnsZone(id) => "Azure::Network::DnsZone({id})",
        AzureCdnProfile(id) => "Azure::Network::CdnProfile({id})",
        AzureServiceTag(id) => "Azure::Network::ServiceTag({id})",
    ],
    Some(CollectionSource::Cloudflare) => [
        CloudflareZone(id) => "Cloudflare::Zone({id})",
        CloudflareDnsRecord(id) => "Cloudflare::DnsRecord({id})",
        CloudflareWorker(id) => "Cloudflare::Worker({id})",
        CloudflareDurableObject(id) => "Cloudflare::DurableObject({id})",
        CloudflareKvNamespace(id) => "Cloudflare::KvNamespace({id})",
        CloudflareR2Bucket(id) => "Cloudflare::R2Bucket({id})",
        CloudflareD1Database(id) => "Cloudflare::D1Database({id})",
    ],
);

// The cross-cloud pivots resolve by exact value through `GraphBuilder`'s
// `HashMap<Node, NodeIndex>`, so every producer must spell one address the same
// way. Build them here, never by naming the variant.
impl Node {
    pub fn ip(value: &str) -> Node {
        Node::GenericIpAddress(intern(canonical_address(value), value))
    }

    pub fn hostname(value: &str) -> Node {
        Node::GenericHostname(intern(canonical_hostname(value), value))
    }
}

fn intern(canonical: Option<String>, original: &str) -> Arc<str> {
    canonical.map_or_else(|| Arc::from(original), Arc::from)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Edge {
    Contains,
    ConnectsTo,
    DependsOn,
    AttachedTo,
    HasIp,
    RoutesTo,
    ResolvesTo,
    TrafficFlow,
    Covers,
    Serves,
}

// Only a scan's own edges are evidence of what exists. `TrafficFlow` is
// observed, and `Covers` and `Serves` are derived from whatever nodes survived
// the pass, so `patch::carry_forward` must not hold any of them as if a
// provider had reported it.
impl Edge {
    pub fn is_projected(&self) -> bool {
        match self {
            Edge::Contains
            | Edge::ConnectsTo
            | Edge::DependsOn
            | Edge::AttachedTo
            | Edge::HasIp
            | Edge::RoutesTo
            | Edge::ResolvesTo => true,
            Edge::TrafficFlow | Edge::Covers | Edge::Serves => false,
        }
    }
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

kinds!(
    Edge,
    Contains,
    ConnectsTo,
    DependsOn,
    AttachedTo,
    HasIp,
    RoutesTo,
    ResolvesTo,
    TrafficFlow,
    Covers,
    Serves,
);
