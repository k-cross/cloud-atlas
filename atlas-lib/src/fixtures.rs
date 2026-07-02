//! A complete fake multi-cloud environment for the fictional company
//! "Globex" — no cloud credentials required.
//!
//! Every collection variant of every provider is populated with at least one
//! resource, so projecting `all()` exercises every projector arm and emits
//! every `Node` kind (enforced by the exhaustiveness tests in
//! `atlas::tests`). The demo example renders the same environment for manual
//! verification.
//!
//! Cross-cloud seams (identical strings on purpose, so the graph merges the
//! generic pivot nodes):
//! - `app-globex.azurewebsites.net` — Azure App Service hostname, Cloudflare
//!   CNAME target, and AWS Route53 CNAME value
//! - `run.globex.app` — GCP Cloud Run URI and Cloudflare CNAME target
//! - `198.51.100.10` — Azure Public IP and Cloudflare A record
//! - `10.20.0.5` — GCP Cloud SQL private IP and Azure NSG outbound rule

use crate::Settings;
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::projector;
use crate::cloud::definition::{
    AmazonCollection, CloudflareCollection, GoogleCollection, MicrosoftCollection, Provider,
};
use std::collections::HashMap;

pub const REGION: &str = "us-east-1";
pub const GCP_PROJECT: &str = "globex-prod";
pub const AZURE_SUBSCRIPTION: &str = "sub-globex";

pub fn azure_id(rest: &str) -> String {
    format!(
        "/subscriptions/{}/resourceGroups/rg-globex/providers/{}",
        AZURE_SUBSCRIPTION, rest
    )
}

pub fn settings() -> Settings {
    Settings {
        regions: vec![REGION.to_owned()],
        gcp_projects: Some(vec![GCP_PROJECT.to_owned()]),
        azure_subscriptions: Some(vec![AZURE_SUBSCRIPTION.to_owned()]),
        cloudflare: true,
        ..Default::default()
    }
}

/// All four providers, fully populated.
pub fn all() -> Vec<Provider> {
    vec![aws(), gcp(), azure(), cloudflare()]
}

/// Project the entire fake environment onto a fresh graph.
pub fn build_graph() -> GraphBuilder {
    let s = settings();
    let mut builder = GraphBuilder::new();
    for provider in all() {
        projector::build(&mut builder, &provider, &s);
    }
    builder
}

pub fn aws() -> Provider {
    use aws_sdk_ec2::types::{
        Address, GroupIdentifier, InternetGateway, InternetGatewayAttachment, IpPermission,
        IpRange, Ipv6Range, NatGateway, NatGatewayAddress, Placement, Route, RouteTable,
        RouteTableAssociation, SecurityGroup, Tag, UserIdGroupPair, builders::InstanceBuilder,
    };

    let sg_web = GroupIdentifier::builder()
        .group_id("sg-web")
        .group_name("globex-web")
        .build();

    let i1 = InstanceBuilder::default()
        .set_instance_id(Some("i-globex-web-01".to_owned()))
        .set_vpc_id(Some("vpc-globex".to_owned()))
        .set_subnet_id(Some("subnet-public-1a".to_owned()))
        .set_private_ip_address(Some("10.10.1.10".to_owned()))
        .set_placement(Some(
            Placement::builder().availability_zone("us-east-1a").build(),
        ))
        .set_tags(Some(vec![Tag::builder().key("env").value("prod").build()]))
        .set_security_groups(Some(vec![sg_web.clone()]))
        .build();
    let i2 = InstanceBuilder::default()
        .set_instance_id(Some("i-globex-web-02".to_owned()))
        .set_vpc_id(Some("vpc-globex".to_owned()))
        .set_subnet_id(Some("subnet-public-1a".to_owned()))
        .set_private_ip_address(Some("10.10.1.11".to_owned()))
        .set_placement(Some(
            Placement::builder().availability_zone("us-east-1a").build(),
        ))
        .set_security_groups(Some(vec![sg_web]))
        .build();

    let ecs = aws_sdk_ecs::types::Cluster::builder()
        .cluster_arn("arn:aws:ecs:us-east-1:123:cluster/globex-services")
        .build();

    let lambda = aws_sdk_lambda::types::FunctionConfiguration::builder()
        .function_name("globex-events-handler")
        .role("arn:aws:iam::123:role/globex-lambda-role")
        .vpc_config(
            aws_sdk_lambda::types::VpcConfigResponse::builder()
                .security_group_ids("sg-web")
                .build(),
        )
        .build();

    let bus = aws_sdk_eventbridge::types::EventBus::builder()
        .name("globex-bus")
        .build();

    let mut resources = HashMap::new();
    resources.insert(
        "AWS::S3::Bucket".to_owned(),
        vec![
            aws_sdk_config::types::ResourceIdentifier::builder()
                .resource_id("globex-assets")
                .build(),
        ],
    );

    let lb = aws_sdk_elasticloadbalancingv2::types::LoadBalancer::builder()
        .load_balancer_arn("arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/globex/1")
        .vpc_id("vpc-globex")
        .build();
    let tg = aws_sdk_elasticloadbalancingv2::types::TargetGroup::builder()
        .target_group_arn("arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-web/1")
        .vpc_id("vpc-globex")
        .build();
    let listener = aws_sdk_elasticloadbalancingv2::types::Listener::builder()
        .load_balancer_arn("arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/globex/1")
        .default_actions(
            aws_sdk_elasticloadbalancingv2::types::Action::builder()
                .target_group_arn(
                    "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-web/1",
                )
                .build(),
        )
        .build();
    let mut target_health = HashMap::new();
    target_health.insert(
        "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-web/1".to_owned(),
        vec![
            aws_sdk_elasticloadbalancingv2::types::TargetHealthDescription::builder()
                .target(
                    aws_sdk_elasticloadbalancingv2::types::TargetDescription::builder()
                        .id("i-globex-web-01")
                        .build(),
                )
                .build(),
        ],
    );

    let hosted_zone = aws_sdk_route53::types::HostedZone::builder()
        .id("/hostedzone/ZGLOBEX")
        .name("globex.io.")
        .caller_reference("fixture")
        .build()
        .unwrap();
    let record_a = aws_sdk_route53::types::ResourceRecordSet::builder()
        .name("origin.globex.io.")
        .r#type(aws_sdk_route53::types::RrType::A)
        .resource_records(
            aws_sdk_route53::types::ResourceRecord::builder()
                .value("203.0.113.10")
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    // CNAME to the Azure App Service hostname — AWS -> Azure seam
    let record_cname = aws_sdk_route53::types::ResourceRecordSet::builder()
        .name("app.globex.io.")
        .r#type(aws_sdk_route53::types::RrType::Cname)
        .resource_records(
            aws_sdk_route53::types::ResourceRecord::builder()
                .value("app-globex.azurewebsites.net")
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    let eks = aws_sdk_eks::types::Cluster::builder()
        .name("globex-k8s")
        .resources_vpc_config(
            aws_sdk_eks::types::VpcConfigResponse::builder()
                .vpc_id("vpc-globex")
                .security_group_ids("sg-web")
                .build(),
        )
        .build();

    let api = aws_sdk_apigateway::types::RestApi::builder()
        .id("api-globex")
        .build();

    let rds = aws_sdk_rds::types::DbInstance::builder()
        .db_instance_identifier("globex-orders-db")
        .db_subnet_group(
            aws_sdk_rds::types::DbSubnetGroup::builder()
                .vpc_id("vpc-globex")
                .build(),
        )
        .vpc_security_groups(
            aws_sdk_rds::types::VpcSecurityGroupMembership::builder()
                .vpc_security_group_id("sg-web")
                .build(),
        )
        .build();

    let sns = aws_sdk_sns::types::Topic::builder()
        .topic_arn("arn:aws:sns:us-east-1:123:globex-alerts")
        .build();

    // DistributionSummary mirrors the CloudFront API contract, which marks
    // most fields required — the projector only reads `id`.
    let cloudfront = {
        use aws_sdk_cloudfront::types::*;
        DistributionSummary::builder()
            .id("EGLOBEX1")
            .arn("arn:aws:cloudfront::123:distribution/EGLOBEX1")
            .status("Deployed")
            .last_modified_time(aws_smithy_types::DateTime::from_secs(1_700_000_000))
            .domain_name("dglobex.cloudfront.net")
            .aliases(Aliases::builder().quantity(0).build().unwrap())
            .origins(
                Origins::builder()
                    .quantity(1)
                    .items(
                        Origin::builder()
                            .id("alb-origin")
                            .domain_name("alb.aws.globex.io")
                            .build()
                            .unwrap(),
                    )
                    .build()
                    .unwrap(),
            )
            .default_cache_behavior(
                DefaultCacheBehavior::builder()
                    .target_origin_id("alb-origin")
                    .viewer_protocol_policy(ViewerProtocolPolicy::RedirectToHttps)
                    .build()
                    .unwrap(),
            )
            .cache_behaviors(CacheBehaviors::builder().quantity(0).build().unwrap())
            .custom_error_responses(CustomErrorResponses::builder().quantity(0).build().unwrap())
            .comment("globex distribution")
            .price_class(PriceClass::PriceClassAll)
            .enabled(true)
            .viewer_certificate(ViewerCertificate::builder().build())
            .restrictions(
                Restrictions::builder()
                    .geo_restriction(
                        GeoRestriction::builder()
                            .restriction_type(GeoRestrictionType::None)
                            .quantity(0)
                            .build()
                            .unwrap(),
                    )
                    .build(),
            )
            .web_acl_id("")
            .http_version(HttpVersion::Http2)
            .is_ipv6_enabled(false)
            .staging(false)
            .build()
            .unwrap()
    };

    let sg = SecurityGroup::builder()
        .group_id("sg-web")
        .ip_permissions(
            IpPermission::builder()
                .user_id_group_pairs(UserIdGroupPair::builder().group_id("sg-lb").build())
                .build(),
        )
        .ip_permissions_egress(
            IpPermission::builder()
                .ip_ranges(IpRange::builder().cidr_ip("192.0.2.44/32").build())
                .ipv6_ranges(Ipv6Range::builder().cidr_ipv6("2001:db8::44/128").build())
                .build(),
        )
        .build();

    // Routing / egress plane: public subnet -> IGW, private subnet -> NAT -> EIP.
    let igw = InternetGateway::builder()
        .internet_gateway_id("igw-globex")
        .attachments(
            InternetGatewayAttachment::builder()
                .vpc_id("vpc-globex")
                .build(),
        )
        .build();
    let nat = NatGateway::builder()
        .nat_gateway_id("nat-globex")
        .subnet_id("subnet-public-1a")
        .vpc_id("vpc-globex")
        .nat_gateway_addresses(
            NatGatewayAddress::builder()
                .allocation_id("eipalloc-globex")
                .public_ip("203.0.113.50")
                .build(),
        )
        .build();
    let eip = Address::builder()
        .allocation_id("eipalloc-globex")
        .public_ip("203.0.113.50")
        .build();
    let public_rt = RouteTable::builder()
        .route_table_id("rtb-public")
        .vpc_id("vpc-globex")
        .associations(
            RouteTableAssociation::builder()
                .subnet_id("subnet-public-1a")
                .build(),
        )
        .routes(
            Route::builder()
                .destination_cidr_block("0.0.0.0/0")
                .gateway_id("igw-globex")
                .build(),
        )
        .build();
    let private_rt = RouteTable::builder()
        .route_table_id("rtb-private")
        .vpc_id("vpc-globex")
        .associations(
            RouteTableAssociation::builder()
                .subnet_id("subnet-private-1a")
                .build(),
        )
        .routes(
            Route::builder()
                .destination_cidr_block("0.0.0.0/0")
                .nat_gateway_id("nat-globex")
                .build(),
        )
        .build();

    let r = REGION.to_owned();
    Provider::AWS(vec![
        (r.clone(), AmazonCollection::AmazonInstances(vec![i1, i2])),
        (r.clone(), AmazonCollection::AmazonClusters(vec![ecs])),
        (r.clone(), AmazonCollection::AmazonLambdas(vec![lambda])),
        (r.clone(), AmazonCollection::AmazonEventbridge(vec![bus])),
        (r.clone(), AmazonCollection::AmazonResources(resources)),
        (
            r.clone(),
            AmazonCollection::AmazonLoadBalancers {
                load_balancers: vec![lb],
                target_groups: vec![tg],
                listeners: vec![listener],
                target_health,
            },
        ),
        (
            r.clone(),
            AmazonCollection::AmazonRoute53 {
                hosted_zones: vec![hosted_zone],
                record_sets: vec![record_a, record_cname],
            },
        ),
        (r.clone(), AmazonCollection::AmazonEks(vec![eks])),
        (r.clone(), AmazonCollection::AmazonApiGateway(vec![api])),
        (r.clone(), AmazonCollection::AmazonRds(vec![rds])),
        (
            r.clone(),
            AmazonCollection::AmazonDynamoDb(vec!["globex-events".to_owned()]),
        ),
        (
            r.clone(),
            AmazonCollection::AmazonSqs(vec![
                "https://sqs.us-east-1.amazonaws.com/123/globex-jobs".to_owned(),
            ]),
        ),
        (r.clone(), AmazonCollection::AmazonSns(vec![sns])),
        (
            r.clone(),
            AmazonCollection::AmazonCloudFront(vec![cloudfront]),
        ),
        (r.clone(), AmazonCollection::AmazonSecurityGroups(vec![sg])),
        (
            r,
            AmazonCollection::AmazonNetworking {
                route_tables: vec![public_rt, private_rt],
                internet_gateways: vec![igw],
                nat_gateways: vec![nat],
                addresses: vec![eip],
            },
        ),
    ])
}

pub fn gcp() -> Provider {
    use crate::api::google::compute::{Firewall, Instance};
    use crate::api::google::compute_network::{ForwardingRule, Network, Subnetwork};
    use crate::api::google::dns::ManagedZone;
    use crate::api::google::functions::CloudFunction;
    use crate::api::google::gke::Cluster;
    use crate::api::google::pubsub::{Subscription, Topic};
    use crate::api::google::run::Service;
    use crate::api::google::sql::{SqlInstance, SqlIpAddress};
    use crate::api::google::storage::Bucket;

    let network_link = format!(
        "https://www.googleapis.com/compute/v1/projects/{}/global/networks/vpc-globex",
        GCP_PROJECT
    );
    let subnet_link = format!(
        "https://www.googleapis.com/compute/v1/projects/{}/regions/us-central1/subnetworks/subnet-data",
        GCP_PROJECT
    );

    let instance = Instance {
        id: Some("7000000000000000001".to_owned()),
        name: Some("gce-worker-01".to_owned()),
        self_link: Some(format!(
            "https://www.googleapis.com/compute/v1/projects/{}/zones/us-central1-a/instances/gce-worker-01",
            GCP_PROJECT
        )),
        ..Default::default()
    };

    let firewall = Firewall {
        id: Some("fw-egress-globex".to_owned()),
        name: Some("allow-egress-partner".to_owned()),
        network: Some(network_link.clone()),
        direction: Some("EGRESS".to_owned()),
        destination_ranges: Some(vec!["192.0.2.55".to_owned()]),
        ..Default::default()
    };

    let sql = SqlInstance {
        name: Some("globex-analytics-db".to_owned()),
        ip_addresses: Some(vec![SqlIpAddress {
            ip_type: Some("PRIVATE".to_owned()),
            // Azure's NSG outbound rule points at this same IP — Azure -> GCP seam
            ip_address: Some("10.20.0.5".to_owned()),
        }]),
        ..Default::default()
    };

    let dns = ManagedZone {
        name: Some("globex-internal".to_owned()),
        dns_name: Some("internal.globex.io.".to_owned()),
        ..Default::default()
    };

    let gke = Cluster {
        name: Some("gke-globex".to_owned()),
        network: Some(network_link.clone()),
        ..Default::default()
    };

    let func = CloudFunction {
        name: Some(format!(
            "projects/{}/locations/us-central1/functions/resize-images",
            GCP_PROJECT
        )),
        ..Default::default()
    };

    let bucket = Bucket {
        id: Some("globex-datalake".to_owned()),
        name: Some("globex-datalake".to_owned()),
        ..Default::default()
    };

    let topic = Topic {
        name: Some(format!("projects/{}/topics/globex-events", GCP_PROJECT)),
    };
    let sub = Subscription {
        name: Some(format!(
            "projects/{}/subscriptions/globex-sink",
            GCP_PROJECT
        )),
        topic: Some(format!("projects/{}/topics/globex-events", GCP_PROJECT)),
    };

    let run_svc = Service {
        name: Some(format!(
            "projects/{}/locations/us-central1/services/globex-api",
            GCP_PROJECT
        )),
        // Cloudflare CNAMEs data.globex.io to this — Cloudflare -> GCP seam
        uri: Some("https://run.globex.app".to_owned()),
        ..Default::default()
    };

    let net = Network {
        self_link: Some(network_link.clone()),
        ..Default::default()
    };
    let subnet = Subnetwork {
        self_link: Some(subnet_link),
        network: Some(network_link),
        ..Default::default()
    };

    let fw_rule = ForwardingRule {
        id: Some("fr-globex-lb".to_owned()),
        ip_address: Some("34.120.0.9".to_owned()),
        ..Default::default()
    };

    Provider::GCP(vec![
        GoogleCollection::GoogleInstances(vec![instance]),
        GoogleCollection::GoogleFirewalls(vec![firewall]),
        GoogleCollection::GoogleSql(vec![sql]),
        GoogleCollection::GoogleDns(vec![dns]),
        GoogleCollection::GoogleGke(vec![gke]),
        GoogleCollection::GoogleFunctions(vec![func]),
        GoogleCollection::GoogleStorageBuckets(vec![bucket]),
        GoogleCollection::GooglePubSubTopics(vec![topic]),
        GoogleCollection::GooglePubSubSubscriptions(vec![sub]),
        GoogleCollection::GoogleRunServices(vec![run_svc]),
        GoogleCollection::GoogleNetworks(vec![net]),
        GoogleCollection::GoogleSubnetworks(vec![subnet]),
        GoogleCollection::GoogleForwardingRules(vec![fw_rule]),
    ])
}

pub fn azure() -> Provider {
    use crate::api::azure::models::*;

    let vm = VirtualMachine {
        id: Some(azure_id("Microsoft.Compute/virtualMachines/vm-worker")),
        name: Some("vm-worker".to_owned()),
        location: Some("eastus".to_owned()),
        network_interfaces: vec![azure_id("Microsoft.Network/networkInterfaces/nic-worker")],
    };

    let vnet = VirtualNetwork {
        id: Some(azure_id("Microsoft.Network/virtualNetworks/vnet-globex")),
        name: Some("vnet-globex".to_owned()),
        location: Some("eastus".to_owned()),
        subnets: vec![azure_id(
            "Microsoft.Network/virtualNetworks/vnet-globex/subnets/default",
        )],
    };

    let subnet = Subnet {
        id: Some(azure_id(
            "Microsoft.Network/virtualNetworks/vnet-globex/subnets/default",
        )),
        name: Some("default".to_owned()),
        vnet_id: Some(azure_id("Microsoft.Network/virtualNetworks/vnet-globex")),
        network_security_group_id: Some(azure_id(
            "Microsoft.Network/networkSecurityGroups/nsg-worker",
        )),
    };

    let nsg = NetworkSecurityGroup {
        id: Some(azure_id(
            "Microsoft.Network/networkSecurityGroups/nsg-worker",
        )),
        name: Some("nsg-worker".to_owned()),
        location: Some("eastus".to_owned()),
        properties: Some(NetworkSecurityGroupProperties {
            security_rules: Some(vec![
                // -> GCP Cloud SQL private IP (cross-cloud seam)
                SecurityRule {
                    properties: Some(SecurityRuleProperties {
                        direction: Some("Outbound".to_owned()),
                        destination_address_prefix: Some("10.20.0.5".to_owned()),
                        destination_address_prefixes: None,
                    }),
                },
                // -> Azure service tag
                SecurityRule {
                    properties: Some(SecurityRuleProperties {
                        direction: Some("Outbound".to_owned()),
                        destination_address_prefix: Some("AzureCloud".to_owned()),
                        destination_address_prefixes: None,
                    }),
                },
            ]),
        }),
    };

    let pip = PublicIpAddress {
        id: Some(azure_id("Microsoft.Network/publicIPAddresses/pip-globex")),
        name: Some("pip-globex".to_owned()),
        // Cloudflare A record db.globex.io points here — Cloudflare -> Azure seam
        ip_address: Some("198.51.100.10".to_owned()),
    };

    let leaf = |rest: &str, name: &str| (azure_id(rest), name.to_owned());

    let (sa_id, sa_name) = leaf(
        "Microsoft.Storage/storageAccounts/globexstore",
        "globexstore",
    );
    let storage = StorageAccount {
        id: Some(sa_id),
        name: Some(sa_name),
        location: Some("eastus".to_owned()),
    };

    let (aks_id, aks_name) = leaf(
        "Microsoft.ContainerService/managedClusters/aks-globex",
        "aks-globex",
    );
    let aks = ManagedCluster {
        id: Some(aks_id),
        name: Some(aks_name),
        location: Some("eastus".to_owned()),
    };

    let (sql_id, sql_name) = leaf("Microsoft.Sql/servers/sql-globex", "sql-globex");
    let sql = SqlServer {
        id: Some(sql_id),
        name: Some(sql_name),
        location: Some("eastus".to_owned()),
    };

    let app = AppService {
        id: Some(azure_id("Microsoft.Web/sites/app-globex")),
        name: Some("app-globex".to_owned()),
        location: Some("eastus".to_owned()),
        properties: Some(AppServiceProperties {
            // Cloudflare and Route53 CNAME here — inbound seams
            default_host_name: Some("app-globex.azurewebsites.net".to_owned()),
        }),
    };

    let (fa_id, fa_name) = leaf("Microsoft.Web/sites/func-globex", "func-globex");
    let func = FunctionApp {
        id: Some(fa_id),
        name: Some(fa_name),
        location: Some("eastus".to_owned()),
    };

    let (apim_id, apim_name) = leaf("Microsoft.ApiManagement/service/apim-globex", "apim-globex");
    let apim = ApiManagement {
        id: Some(apim_id),
        name: Some(apim_name),
        location: Some("eastus".to_owned()),
    };

    let (cos_id, cos_name) = leaf(
        "Microsoft.DocumentDB/databaseAccounts/cosmos-globex",
        "cosmos-globex",
    );
    let cosmos = CosmosDb {
        id: Some(cos_id),
        name: Some(cos_name),
        location: Some("eastus".to_owned()),
    };

    let (sb_id, sb_name) = leaf("Microsoft.ServiceBus/namespaces/sb-globex", "sb-globex");
    let sbus = ServiceBus {
        id: Some(sb_id),
        name: Some(sb_name),
        location: Some("eastus".to_owned()),
    };

    let (eg_id, eg_name) = leaf("Microsoft.EventGrid/topics/eg-globex", "eg-globex");
    let egrid = EventGridTopic {
        id: Some(eg_id),
        name: Some(eg_name),
        location: Some("eastus".to_owned()),
    };

    let (dns_id, dns_name) = leaf("Microsoft.Network/dnsZones/globex.azure", "globex.azure");
    let dns = DnsZone {
        id: Some(dns_id),
        name: Some(dns_name),
        location: Some("global".to_owned()),
    };

    let (cdn_id, cdn_name) = leaf("Microsoft.Cdn/profiles/cdn-globex", "cdn-globex");
    let cdn = CdnProfile {
        id: Some(cdn_id),
        name: Some(cdn_name),
        location: Some("global".to_owned()),
    };

    Provider::Azure(vec![
        MicrosoftCollection::AzureVirtualMachines(vec![vm]),
        MicrosoftCollection::AzureVirtualNetworks(vec![vnet]),
        MicrosoftCollection::AzureSubnets(vec![subnet]),
        MicrosoftCollection::AzureNetworkSecurityGroups(vec![nsg]),
        MicrosoftCollection::AzurePublicIpAddresses(vec![pip]),
        MicrosoftCollection::AzureStorageAccounts(vec![storage]),
        MicrosoftCollection::AzureManagedClusters(vec![aks]),
        MicrosoftCollection::AzureSqlServers(vec![sql]),
        MicrosoftCollection::AzureAppServices(vec![app]),
        MicrosoftCollection::AzureFunctionApps(vec![func]),
        MicrosoftCollection::AzureApiManagement(vec![apim]),
        MicrosoftCollection::AzureCosmosDbs(vec![cosmos]),
        MicrosoftCollection::AzureServiceBuses(vec![sbus]),
        MicrosoftCollection::AzureEventGridTopics(vec![egrid]),
        MicrosoftCollection::AzureDnsZones(vec![dns]),
        MicrosoftCollection::AzureCdnProfiles(vec![cdn]),
    ])
}

pub fn cloudflare() -> Provider {
    use crate::cloud::cloudflare::d1::D1Database;
    use crate::cloud::cloudflare::durable_objects::DurableObjectNamespace;
    use crate::cloud::cloudflare::worker::{WorkerBinding, WorkerScript};
    use serde_json::json;

    let zone_json = json!({
        "id": "zone-globex",
        "name": "globex.io",
        "account": { "id": "acc-globex", "name": "Globex Org" },
        "activated_on": "2023-01-01T00:00:00Z",
        "created_on": "2023-01-01T00:00:00Z",
        "development_mode": 0,
        "original_name_servers": ["ns1"],
        "original_registrar": "reg",
        "original_dnshost": "host",
        "modified_on": "2023-01-01T00:00:00Z",
        "name_servers": ["ns1"],
        "owner": { "id": "owner1", "type": "user", "email": "admin@globex.io" },
        "permissions": ["#zone:read"],
        "plan": { "id": "plan1", "name": "enterprise", "price": 0, "currency": "USD", "frequency": "monthly", "is_subscribed": true, "can_subscribe": false, "legacy_id": "ent", "legacy_discount": false, "externally_managed": false },
        "plan_pending": { "id": "plan1", "name": "enterprise", "price": 0, "currency": "USD", "frequency": "monthly", "is_subscribed": true, "can_subscribe": false, "legacy_id": "ent", "legacy_discount": false, "externally_managed": false },
        "status": "active",
        "paused": false,
        "type": "full",
        "meta": {
            "custom_certificate_quota": 1,
            "page_rule_quota": 3,
            "phishing_detected": false
        }
    });

    let record_meta = json!({
        "auto_added": false,
        "managed_by_apps": false,
        "managed_by_argo_tunnel": false,
        "source": "primary"
    });
    let dns_record = |id: &str, name: &str, rtype: &str, content: &str| {
        serde_json::from_value::<cloudflare::endpoints::dns::dns::DnsRecord>(json!({
            "id": id,
            "name": name,
            "type": rtype,
            "content": content,
            "proxied": false,
            "ttl": 300,
            "modified_on": "2023-01-01T00:00:00Z",
            "created_on": "2023-01-01T00:00:00Z",
            "meta": record_meta,
            "proxiable": true
        }))
        .expect("fixture DNS record must deserialize")
    };

    let records = vec![
        // CNAME to Azure App Service — Cloudflare -> Azure seam
        dns_record(
            "rec-app",
            "app.globex.io",
            "CNAME",
            "app-globex.azurewebsites.net",
        ),
        // CNAME to GCP Cloud Run — Cloudflare -> GCP seam
        dns_record("rec-data", "data.globex.io", "CNAME", "run.globex.app"),
        // A record to the Azure public IP — Cloudflare -> Azure seam
        dns_record("rec-db", "db.globex.io", "A", "198.51.100.10"),
        // AAAA record
        dns_record("rec-v6", "v6.globex.io", "AAAA", "2001:db8::10"),
    ];

    let worker = WorkerScript {
        id: "edge-router".to_owned(),
        created_on: None,
        modified_on: None,
    };

    let kv: cloudflare::endpoints::workerskv::WorkersKvNamespace =
        serde_json::from_value(json!({ "id": "kv-sessions", "title": "sessions" }))
            .expect("fixture KV namespace must deserialize");

    let r2: cloudflare::endpoints::r2::r2::Bucket = serde_json::from_value(
        json!({ "name": "r2-media", "creation_date": "2023-01-01T00:00:00Z" }),
    )
    .expect("fixture R2 bucket must deserialize");

    let durable = DurableObjectNamespace {
        id: "do-coordinator".to_owned(),
        name: "coordinator".to_owned(),
        class: Some("Coordinator".to_owned()),
        script: Some("edge-router".to_owned()),
    };

    let d1 = D1Database {
        uuid: "d1-edge-cache".to_owned(),
        name: "edge-cache".to_owned(),
        version: None,
    };

    let worker_bindings = vec![(
        "edge-router".to_owned(),
        vec![
            WorkerBinding {
                name: "SESSIONS".to_owned(),
                binding_type: "kv_namespace".to_owned(),
                namespace_id: Some("kv-sessions".to_owned()),
                bucket_name: None,
                id: None,
                extra: HashMap::new(),
            },
            WorkerBinding {
                name: "MEDIA".to_owned(),
                binding_type: "r2_bucket".to_owned(),
                namespace_id: None,
                bucket_name: Some("r2-media".to_owned()),
                id: None,
                extra: HashMap::new(),
            },
            WorkerBinding {
                name: "COORDINATOR".to_owned(),
                binding_type: "durable_object_namespace".to_owned(),
                namespace_id: Some("do-coordinator".to_owned()),
                bucket_name: None,
                id: None,
                extra: HashMap::new(),
            },
            WorkerBinding {
                name: "EDGE_CACHE".to_owned(),
                binding_type: "d1".to_owned(),
                namespace_id: None,
                bucket_name: None,
                id: Some("d1-edge-cache".to_owned()),
                extra: HashMap::new(),
            },
            WorkerBinding {
                name: "ANALYTICS_DB".to_owned(),
                binding_type: "secret".to_owned(),
                namespace_id: None,
                bucket_name: None,
                id: None,
                extra: HashMap::from([(
                    "text".to_owned(),
                    json!("postgres://db.internal.globex.io"),
                )]),
            },
        ],
    )];

    Provider::Cloudflare(CloudflareCollection {
        zones: vec![serde_json::from_value(zone_json).expect("fixture zone must deserialize")],
        dns_records: vec![("zone-globex".to_owned(), records)],
        workers: vec![worker],
        kv_namespaces: vec![kv],
        r2_buckets: vec![r2],
        durable_objects: vec![durable],
        d1_databases: vec![d1],
        worker_bindings,
    })
}
