use crate::definition::{
    AWSConnectivity, AWSDirectConnect, AWSDirectConnectGateways, AWSLoadBalancing, AWSNetworking,
    AWSRoute53, AWSTransitGatewayRouteTable, AWSTransitGatewayRouting, AmazonCollection,
    HostedZoneId, QueueUrl, TableName,
};
use atlas_graph::collection::CollectionSource;
use atlas_graph::definition::Node;
use atlas_graph::flow::{FlowAction, FlowIndex, FlowObservation};
use atlas_provider::fixtures::{
    AWS_VPN_ADDRESSES, AZURE_SPOKE_RANGE, AZURE_VPN_ADDRESS, CLOUD_SQL_RANGE, DX_AMAZON_ADDRESS,
    DX_GOOGLE_ADDRESS, GCP_VPN_ADDRESS, REGION,
};
use std::collections::HashMap;

pub const AWS_NETWORK: &str = "vpc-globex";
pub const GLOBEX_ZONE: &str = "/hostedzone/ZGLOBEX";
const AVERAGE_PACKET_BYTES: u64 = 128;
const DATA_NETWORK: &str = "vpc-globex-data";
const WEST_REGION: &str = "us-west-2";
const WEST_NETWORK: &str = "vpc-globex-west";
const WEST_RANGE: &str = "10.70.0.0/16";
const WEST_API: &str = "10.70.1.15";
const WEST_PEERING: &str = "tgw-attach-globex-peer-west";

pub fn collections() -> Vec<(String, AmazonCollection)> {
    use aws_sdk_directconnect::types::{
        AssociatedGateway, BgpPeer, BgpPeerState, Connection, ConnectionState,
        DirectConnectGateway, DirectConnectGatewayAssociation,
        DirectConnectGatewayAssociationState, DirectConnectGatewayState, GatewayType,
        VirtualInterface, VirtualInterfaceState,
    };
    use aws_sdk_ec2::types::{
        Address, AttachmentStatus, CustomerGateway, GroupIdentifier, InstanceNetworkInterface,
        InternetGateway, InternetGatewayAttachment, IpPermission, IpRange, Ipv6Range, NatGateway,
        NatGatewayAddress, NetworkInterface, NetworkInterfaceAssociation,
        NetworkInterfaceAttachment, NetworkInterfacePrivateIpAddress, NetworkInterfaceType,
        PeeringTgwInfo, Placement, Route, RouteState, RouteTable, RouteTableAssociation,
        SecurityGroup, Tag, TransitGateway, TransitGatewayAssociationState,
        TransitGatewayAttachment, TransitGatewayAttachmentResourceType,
        TransitGatewayAttachmentState, TransitGatewayPeeringAttachment,
        TransitGatewayPropagationState, TransitGatewayRoute, TransitGatewayRouteAttachment,
        TransitGatewayRouteState, TransitGatewayRouteTable, TransitGatewayRouteTableAssociation,
        TransitGatewayRouteTablePropagation, TransitGatewayRouteTableState, TransitGatewayState,
        TunnelOption, UserIdGroupPair, Vpc, VpcAttachment, VpcPeeringConnection,
        VpcPeeringConnectionStateReason, VpcPeeringConnectionStateReasonCode,
        VpcPeeringConnectionVpcInfo, VpnConnection, VpnConnectionOptions, VpnGateway, VpnState,
        builders::InstanceBuilder,
    };

    let sg_web = GroupIdentifier::builder()
        .group_id("sg-web")
        .group_name("globex-web")
        .build();

    let eni_in = |vpc: &str, id: &str, subnet: &str| {
        InstanceNetworkInterface::builder()
            .network_interface_id(id)
            .subnet_id(subnet)
            .vpc_id(vpc)
            .build()
    };
    let eni = |id: &str, subnet: &str| eni_in(AWS_NETWORK, id, subnet);

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
        .set_network_interfaces(Some(vec![
            eni("eni-globex-web-01a", "subnet-public-1a"),
            eni("eni-globex-web-01b", "subnet-private-1a"),
        ]))
        .build();
    let i2 = InstanceBuilder::default()
        .set_instance_id(Some("i-globex-web-02".to_owned()))
        .set_vpc_id(Some("vpc-globex".to_owned()))
        .set_subnet_id(Some("subnet-public-1a".to_owned()))
        .set_private_ip_address(Some("10.10.1.11".to_owned()))
        .set_placement(Some(
            Placement::builder().availability_zone("us-east-1a").build(),
        ))
        .set_security_groups(Some(vec![sg_web.clone()]))
        .set_network_interfaces(Some(vec![eni("eni-globex-web-02a", "subnet-public-1a")]))
        .build();
    // Registered behind the balancer and receiving nothing: the state that only
    // an `inferred` Serves edge can express.
    let i3 = InstanceBuilder::default()
        .set_instance_id(Some("i-globex-web-03".to_owned()))
        .set_vpc_id(Some("vpc-globex".to_owned()))
        .set_subnet_id(Some("subnet-public-1a".to_owned()))
        .set_private_ip_address(Some("10.10.1.12".to_owned()))
        .set_placement(Some(
            Placement::builder().availability_zone("us-east-1a").build(),
        ))
        .set_security_groups(Some(vec![sg_web]))
        .set_network_interfaces(Some(vec![eni("eni-globex-web-03a", "subnet-public-1a")]))
        .build();
    let warehouse = InstanceBuilder::default()
        .set_instance_id(Some("i-globex-warehouse".to_owned()))
        .set_vpc_id(Some(DATA_NETWORK.to_owned()))
        .set_subnet_id(Some("subnet-data-1a".to_owned()))
        .set_private_ip_address(Some("10.40.1.20".to_owned()))
        .set_network_interfaces(Some(vec![eni_in(
            DATA_NETWORK,
            "eni-globex-warehouse",
            "subnet-data-1a",
        )]))
        .build();
    let staging_web = InstanceBuilder::default()
        .set_instance_id(Some("i-globex-staging-web".to_owned()))
        .set_vpc_id(Some("vpc-globex-staging".to_owned()))
        .set_subnet_id(Some("subnet-staging-1a".to_owned()))
        .set_private_ip_address(Some("10.10.1.10".to_owned()))
        .set_network_interfaces(Some(vec![eni_in(
            "vpc-globex-staging",
            "eni-globex-staging-web",
            "subnet-staging-1a",
        )]))
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
    let nlb = aws_sdk_elasticloadbalancingv2::types::LoadBalancer::builder()
        .load_balancer_arn(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/globex-db/1",
        )
        .vpc_id("vpc-globex")
        .build();
    let tg = aws_sdk_elasticloadbalancingv2::types::TargetGroup::builder()
        .target_group_arn("arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-web/1")
        .target_type(aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum::Instance)
        .vpc_id("vpc-globex")
        .build();
    let tg_db = aws_sdk_elasticloadbalancingv2::types::TargetGroup::builder()
        .target_group_arn("arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-db/1")
        .target_type(aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum::Ip)
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
    let listener_db = aws_sdk_elasticloadbalancingv2::types::Listener::builder()
        .load_balancer_arn(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/globex-db/1",
        )
        .default_actions(
            aws_sdk_elasticloadbalancingv2::types::Action::builder()
                .target_group_arn(
                    "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-db/1",
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
            aws_sdk_elasticloadbalancingv2::types::TargetHealthDescription::builder()
                .target(
                    aws_sdk_elasticloadbalancingv2::types::TargetDescription::builder()
                        .id("i-globex-web-03")
                        .build(),
                )
                .build(),
        ],
    );
    target_health.insert(
        "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-db/1".to_owned(),
        vec![
            aws_sdk_elasticloadbalancingv2::types::TargetHealthDescription::builder()
                .target(
                    aws_sdk_elasticloadbalancingv2::types::TargetDescription::builder()
                        .id("10.10.2.30")
                        .build(),
                )
                .build(),
        ],
    );

    let hosted_zone = aws_sdk_route53::types::HostedZone::builder()
        .id(GLOBEX_ZONE)
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

    let record_cname = aws_sdk_route53::types::ResourceRecordSet::builder()
        .name("app.globex.io.")
        .r#type(aws_sdk_route53::types::RrType::Cname)
        .resource_records(
            aws_sdk_route53::types::ResourceRecord::builder()
                .value("app-globex.azurewebsites.net.")
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    let record_db_cname = aws_sdk_route53::types::ResourceRecordSet::builder()
        .name("db.globex.io.")
        .r#type(aws_sdk_route53::types::RrType::Cname)
        .resource_records(
            aws_sdk_route53::types::ResourceRecord::builder()
                .value("globex-orders-db.cluster-abc123.us-east-1.rds.amazonaws.com.")
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
        .endpoint(
            aws_sdk_rds::types::Endpoint::builder()
                .address("globex-orders-db.cluster-abc123.us-east-1.rds.amazonaws.com")
                .port(5432)
                .build(),
        )
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
                .ipv6_ranges(
                    Ipv6Range::builder()
                        .cidr_ipv6("2001:0DB8:0000:0000:0000:0000:0000:0044/128")
                        .build(),
                )
                .build(),
        )
        .ip_permissions_egress(
            IpPermission::builder()
                .ip_ranges(IpRange::builder().cidr_ip("203.0.113.0/24").build())
                .ipv6_ranges(Ipv6Range::builder().cidr_ipv6("2001:db8::/32").build())
                .build(),
        )
        .build();

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
        .network_interface_id("eni-globex-nat-1a")
        .build();
    let web_eip = Address::builder()
        .allocation_id("eipalloc-globex-web")
        .public_ip("198.51.100.40")
        .network_interface_id("eni-globex-web-02a")
        .instance_id("i-globex-web-02")
        .build();
    let route = |destination: &str| {
        Route::builder()
            .destination_cidr_block(destination)
            .state(RouteState::Active)
    };
    let local = |cidr: &str| route(cidr).gateway_id("local").build();
    let public_rt = RouteTable::builder()
        .route_table_id("rtb-public")
        .vpc_id("vpc-globex")
        .associations(
            RouteTableAssociation::builder()
                .subnet_id("subnet-public-1a")
                .build(),
        )
        .routes(local("10.10.0.0/16"))
        .routes(route("0.0.0.0/0").gateway_id("igw-globex").build())
        .build();
    let private_rt = RouteTable::builder()
        .route_table_id("rtb-private")
        .vpc_id("vpc-globex")
        .associations(
            RouteTableAssociation::builder()
                .subnet_id("subnet-private-1a")
                .build(),
        )
        .routes(local("10.10.0.0/16"))
        .routes(route("0.0.0.0/0").nat_gateway_id("nat-globex").build())
        .routes(
            route("10.40.0.0/16")
                .vpc_peering_connection_id("pcx-globex-data")
                .build(),
        )
        .routes(
            route("10.50.0.0/16")
                .transit_gateway_id("tgw-globex")
                .build(),
        )
        .routes(route(AZURE_SPOKE_RANGE).gateway_id("vgw-globex").build())
        .routes(
            route(CLOUD_SQL_RANGE)
                .transit_gateway_id("tgw-globex")
                .build(),
        )
        .routes(route(WEST_RANGE).transit_gateway_id("tgw-globex").build())
        .build();
    let data_rt = RouteTable::builder()
        .route_table_id("rtb-data")
        .vpc_id(DATA_NETWORK)
        .routes(local("10.40.0.0/16"))
        .routes(
            route("10.10.0.0/16")
                .vpc_peering_connection_id("pcx-globex-data")
                .build(),
        )
        .routes(route(CLOUD_SQL_RANGE).gateway_id("vgw-globex-data").build())
        .routes(
            route("10.60.0.0/16")
                .vpc_peering_connection_id("pcx-globex-retired")
                .state(RouteState::Blackhole)
                .build(),
        )
        .build();

    let vpc = |id: &str, cidr: &str| Vpc::builder().vpc_id(id).cidr_block(cidr).build();
    let vpcs = vec![
        vpc(AWS_NETWORK, "10.10.0.0/16"),
        vpc(DATA_NETWORK, "10.40.0.0/16"),
        vpc("vpc-globex-shared", "10.50.0.0/16"),
        vpc("vpc-globex-staging", "10.10.0.0/16"),
    ];
    let peering = |id: &str, code: VpcPeeringConnectionStateReasonCode, accepter: &str| {
        let side = |vpc_id: &str| {
            VpcPeeringConnectionVpcInfo::builder()
                .vpc_id(vpc_id)
                .build()
        };
        VpcPeeringConnection::builder()
            .vpc_peering_connection_id(id)
            .status(
                VpcPeeringConnectionStateReason::builder()
                    .code(code)
                    .build(),
            )
            .requester_vpc_info(side(AWS_NETWORK))
            .accepter_vpc_info(side(accepter))
            .build()
    };
    let peering_connections = vec![
        peering(
            "pcx-globex-data",
            VpcPeeringConnectionStateReasonCode::Active,
            DATA_NETWORK,
        ),
        peering(
            "pcx-globex-staging",
            VpcPeeringConnectionStateReasonCode::Rejected,
            "vpc-globex-staging",
        ),
    ];
    let transit_gateways = vec![
        TransitGateway::builder()
            .transit_gateway_id("tgw-globex")
            .state(TransitGatewayState::Available)
            .build(),
    ];
    let attachment = |id: &str, vpc_id: &str| {
        TransitGatewayAttachment::builder()
            .transit_gateway_attachment_id(id)
            .transit_gateway_id("tgw-globex")
            .resource_type(TransitGatewayAttachmentResourceType::Vpc)
            .resource_id(vpc_id)
            .state(TransitGatewayAttachmentState::Available)
            .build()
    };
    let transit_gateway_attachments = vec![
        attachment("tgw-attach-globex-prod", AWS_NETWORK),
        attachment("tgw-attach-globex-shared", "vpc-globex-shared"),
    ];
    let attached_to = |kind: TransitGatewayAttachmentResourceType, resource: &str, id: &str| {
        (kind, resource.to_owned(), id.to_owned())
    };
    let prod = attached_to(
        TransitGatewayAttachmentResourceType::Vpc,
        AWS_NETWORK,
        "tgw-attach-globex-prod",
    );
    let shared = attached_to(
        TransitGatewayAttachmentResourceType::Vpc,
        "vpc-globex-shared",
        "tgw-attach-globex-shared",
    );
    let gcp_vpn = attached_to(
        TransitGatewayAttachmentResourceType::Vpn,
        "vpn-globex-gcp",
        "tgw-attach-globex-gcp",
    );
    let to_west = attached_to(
        TransitGatewayAttachmentResourceType::Peering,
        "tgw-globex-west",
        WEST_PEERING,
    );
    let to_east = attached_to(
        TransitGatewayAttachmentResourceType::Peering,
        "tgw-globex",
        WEST_PEERING,
    );
    let west_vpc = attached_to(
        TransitGatewayAttachmentResourceType::Vpc,
        WEST_NETWORK,
        "tgw-attach-globex-west-vpc",
    );
    type Attached = (TransitGatewayAttachmentResourceType, String, String);
    let route_table = |id: &str,
                       tgw: &str,
                       associated: &[&Attached],
                       propagated: &[&Attached],
                       routes: Vec<TransitGatewayRoute>| {
        AWSTransitGatewayRouteTable {
            table: TransitGatewayRouteTable::builder()
                .transit_gateway_route_table_id(id)
                .transit_gateway_id(tgw)
                .state(TransitGatewayRouteTableState::Available)
                .build(),
            associations: associated
                .iter()
                .map(|(kind, resource, attachment)| {
                    TransitGatewayRouteTableAssociation::builder()
                        .resource_type(kind.clone())
                        .resource_id(resource)
                        .transit_gateway_attachment_id(attachment)
                        .state(TransitGatewayAssociationState::Associated)
                        .build()
                })
                .collect(),
            propagations: propagated
                .iter()
                .map(|(kind, resource, attachment)| {
                    TransitGatewayRouteTablePropagation::builder()
                        .resource_type(kind.clone())
                        .resource_id(resource)
                        .transit_gateway_attachment_id(attachment)
                        .state(TransitGatewayPropagationState::Enabled)
                        .build()
                })
                .collect(),
            static_routes: routes,
        }
    };
    let static_route =
        |destination: &str, state: TransitGatewayRouteState, to: Option<&Attached>| {
            let route = TransitGatewayRoute::builder()
                .destination_cidr_block(destination)
                .state(state);
            match to {
                Some((kind, resource, attachment)) => route.transit_gateway_attachments(
                    TransitGatewayRouteAttachment::builder()
                        .resource_type(kind.clone())
                        .resource_id(resource)
                        .transit_gateway_attachment_id(attachment)
                        .build(),
                ),
                None => route,
            }
            .build()
        };
    let transit_gateway_route_tables = vec![route_table(
        "tgw-rtb-globex",
        "tgw-globex",
        &[&prod, &shared, &gcp_vpn, &to_west],
        &[&prod, &shared, &gcp_vpn],
        vec![
            static_route(WEST_RANGE, TransitGatewayRouteState::Active, Some(&to_west)),
            static_route("10.66.0.0/16", TransitGatewayRouteState::Blackhole, None),
        ],
    )];
    let west_route_tables = vec![route_table(
        "tgw-rtb-globex-west",
        "tgw-globex-west",
        &[&west_vpc, &to_east],
        &[&west_vpc],
        vec![static_route(
            "10.10.0.0/16",
            TransitGatewayRouteState::Active,
            Some(&to_east),
        )],
    )];
    let peering_to_west = || {
        let side = |tgw: &str, region: &str| {
            PeeringTgwInfo::builder()
                .transit_gateway_id(tgw)
                .region(region)
                .build()
        };
        TransitGatewayPeeringAttachment::builder()
            .transit_gateway_attachment_id(WEST_PEERING)
            .requester_tgw_info(side("tgw-globex", REGION))
            .accepter_tgw_info(side("tgw-globex-west", WEST_REGION))
            .state(TransitGatewayAttachmentState::Available)
            .build()
    };
    let west = AWSConnectivity {
        vpcs: vec![vpc(WEST_NETWORK, WEST_RANGE)],
        transit_gateways: vec![
            TransitGateway::builder()
                .transit_gateway_id("tgw-globex-west")
                .state(TransitGatewayState::Available)
                .build(),
        ],
        transit_gateway_attachments: vec![
            TransitGatewayAttachment::builder()
                .transit_gateway_attachment_id("tgw-attach-globex-west-vpc")
                .transit_gateway_id("tgw-globex-west")
                .resource_type(TransitGatewayAttachmentResourceType::Vpc)
                .resource_id(WEST_NETWORK)
                .state(TransitGatewayAttachmentState::Available)
                .build(),
        ],
        ..Default::default()
    };

    let vpn_gateway = |id: &str, vpc_id: &str| {
        VpnGateway::builder()
            .vpn_gateway_id(id)
            .state(VpnState::Available)
            .vpc_attachments(
                VpcAttachment::builder()
                    .vpc_id(vpc_id)
                    .state(AttachmentStatus::Attached)
                    .build(),
            )
            .build()
    };
    let vpn_gateways = vec![
        vpn_gateway("vgw-globex", AWS_NETWORK),
        vpn_gateway("vgw-globex-data", DATA_NETWORK),
    ];
    let customer_gateway = |id: &str, ip: &str| {
        CustomerGateway::builder()
            .customer_gateway_id(id)
            .ip_address(ip)
            .state("available")
            .build()
    };
    let customer_gateways = vec![
        customer_gateway("cgw-globex-azure", AZURE_VPN_ADDRESS),
        customer_gateway("cgw-globex-gcp", GCP_VPN_ADDRESS),
    ];
    let vpn_connection = |id: &str, cgw: &str, tunnels: &[&str]| {
        let options = tunnels
            .iter()
            .fold(VpnConnectionOptions::builder(), |options, ip| {
                options.tunnel_options(TunnelOption::builder().outside_ip_address(*ip).build())
            });
        VpnConnection::builder()
            .vpn_connection_id(id)
            .state(VpnState::Available)
            .customer_gateway_id(cgw)
            .options(options.build())
    };
    let vpn_connections = vec![
        vpn_connection(
            "vpn-globex-azure",
            "cgw-globex-azure",
            &[AWS_VPN_ADDRESSES[0], AWS_VPN_ADDRESSES[1]],
        )
        .vpn_gateway_id("vgw-globex")
        .build(),
        vpn_connection("vpn-globex-gcp", "cgw-globex-gcp", &[AWS_VPN_ADDRESSES[2]])
            .transit_gateway_id("tgw-globex")
            .build(),
    ];

    let direct_connect = AWSDirectConnect {
        connections: vec![
            Connection::builder()
                .connection_id("dxcon-globex-gcp")
                .connection_name("globex-cross-cloud-gcp")
                .connection_state(ConnectionState::Available)
                .location("EqDC2")
                .build(),
        ],
        virtual_interfaces: vec![
            VirtualInterface::builder()
                .virtual_interface_id("dxvif-globex-gcp")
                .virtual_interface_type("private")
                .virtual_interface_state(VirtualInterfaceState::Available)
                .connection_id("dxcon-globex-gcp")
                .direct_connect_gateway_id("dxgw-globex")
                .amazon_address(DX_AMAZON_ADDRESS)
                .customer_address(DX_GOOGLE_ADDRESS)
                .bgp_peers(
                    BgpPeer::builder()
                        .amazon_address(DX_AMAZON_ADDRESS)
                        .customer_address(DX_GOOGLE_ADDRESS)
                        .bgp_peer_state(BgpPeerState::Available)
                        .build(),
                )
                .build(),
        ],
    };
    let direct_connect_gateways = AWSDirectConnectGateways {
        gateways: vec![
            DirectConnectGateway::builder()
                .direct_connect_gateway_id("dxgw-globex")
                .direct_connect_gateway_state(DirectConnectGatewayState::Available)
                .build(),
        ],
        associations: vec![
            DirectConnectGatewayAssociation::builder()
                .direct_connect_gateway_id("dxgw-globex")
                .association_state(DirectConnectGatewayAssociationState::Associated)
                .associated_gateway(
                    AssociatedGateway::builder()
                        .id("vgw-globex-data")
                        .r#type(GatewayType::VirtualPrivateGateway)
                        .build(),
                )
                .build(),
        ],
    };

    let interface_in = |vpc: &str, id: &str, subnet: &str, ip: &str| {
        NetworkInterface::builder()
            .network_interface_id(id)
            .subnet_id(subnet)
            .vpc_id(vpc)
            .groups(GroupIdentifier::builder().group_id("sg-web").build())
            .private_ip_addresses(
                NetworkInterfacePrivateIpAddress::builder()
                    .private_ip_address(ip)
                    .build(),
            )
    };
    let interface = |id: &str, subnet: &str, ip: &str| interface_in(AWS_NETWORK, id, subnet, ip);
    let attached_in = |vpc: &str, id: &str, subnet: &str, ip: &str, instance: &str| {
        interface_in(vpc, id, subnet, ip)
            .interface_type(NetworkInterfaceType::Interface)
            .attachment(
                NetworkInterfaceAttachment::builder()
                    .instance_id(instance)
                    .build(),
            )
            .build()
    };
    let attached = |id: &str, subnet: &str, ip: &str, instance: &str| {
        attached_in(AWS_NETWORK, id, subnet, ip, instance)
    };
    let west_api = interface_in(
        WEST_NETWORK,
        "eni-globex-west-api",
        "subnet-west-1a",
        WEST_API,
    )
    .description("VPC Endpoint Interface vpce-globex-west-api")
    .interface_type(NetworkInterfaceType::VpcEndpoint)
    .build();
    let interfaces = vec![
        attached(
            "eni-globex-web-01a",
            "subnet-public-1a",
            "10.10.1.10",
            "i-globex-web-01",
        ),
        attached(
            "eni-globex-web-01b",
            "subnet-private-1a",
            "10.10.2.20",
            "i-globex-web-01",
        ),
        interface("eni-globex-web-02a", "subnet-public-1a", "10.10.1.11")
            .interface_type(NetworkInterfaceType::Interface)
            .attachment(
                NetworkInterfaceAttachment::builder()
                    .instance_id("i-globex-web-02")
                    .build(),
            )
            .association(
                NetworkInterfaceAssociation::builder()
                    .public_ip("198.51.100.40")
                    .build(),
            )
            .build(),
        attached(
            "eni-globex-web-03a",
            "subnet-public-1a",
            "10.10.1.12",
            "i-globex-web-03",
        ),
        interface("eni-globex-alb-1a", "subnet-public-1a", "10.10.1.50")
            .interface_type(NetworkInterfaceType::NetworkLoadBalancer)
            .description("ELB app/globex/1")
            .build(),
        interface("eni-globex-nat-1a", "subnet-public-1a", "10.10.1.60")
            .interface_type(NetworkInterfaceType::NatGateway)
            .description("Interface for NAT Gateway nat-globex")
            .association(
                NetworkInterfaceAssociation::builder()
                    .public_ip("203.0.113.50")
                    .build(),
            )
            .build(),
        interface("eni-globex-rds-1a", "subnet-private-1a", "10.10.2.30")
            .description("RDSNetworkInterface")
            .build(),
        attached_in(
            DATA_NETWORK,
            "eni-globex-warehouse",
            "subnet-data-1a",
            "10.40.1.20",
            "i-globex-warehouse",
        ),
        interface_in(
            "vpc-globex-shared",
            "eni-globex-ldap",
            "subnet-shared-1a",
            "10.50.0.10",
        )
        .interface_type(NetworkInterfaceType::VpcEndpoint)
        .description("VPC Endpoint Interface vpce-globex-ldap")
        .build(),
        attached_in(
            "vpc-globex-staging",
            "eni-globex-staging-web",
            "subnet-staging-1a",
            "10.10.1.10",
            "i-globex-staging-web",
        ),
    ];

    let r = REGION.to_owned();
    vec![
        (
            r.clone(),
            AmazonCollection::AmazonNetworkInterfaces(interfaces),
        ),
        (
            r.clone(),
            AmazonCollection::AmazonInstances(vec![i1, i2, i3, warehouse, staging_web]),
        ),
        (r.clone(), AmazonCollection::AmazonClusters(vec![ecs])),
        (r.clone(), AmazonCollection::AmazonLambdas(vec![lambda])),
        (r.clone(), AmazonCollection::AmazonEventbridge(vec![bus])),
        (r.clone(), AmazonCollection::AmazonResources(resources)),
        (
            r.clone(),
            AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing {
                load_balancers: vec![lb, nlb],
                target_groups: vec![tg, tg_db],
                listeners: vec![listener, listener_db],
                target_health,
            }),
        ),
        (
            "global".to_owned(),
            AmazonCollection::AmazonRoute53(AWSRoute53 {
                hosted_zones: vec![hosted_zone],
                record_sets: HashMap::from([(
                    HostedZoneId(GLOBEX_ZONE.into()),
                    vec![record_a, record_cname, record_db_cname],
                )]),
            }),
        ),
        (r.clone(), AmazonCollection::AmazonEks(vec![eks])),
        (r.clone(), AmazonCollection::AmazonApiGateway(vec![api])),
        (r.clone(), AmazonCollection::AmazonRds(vec![rds])),
        (
            r.clone(),
            AmazonCollection::AmazonDynamoDb(vec![TableName("globex-events".to_owned())]),
        ),
        (
            r.clone(),
            AmazonCollection::AmazonSqs(vec![QueueUrl(
                "https://sqs.us-east-1.amazonaws.com/123/globex-jobs".to_owned(),
            )]),
        ),
        (r.clone(), AmazonCollection::AmazonSns(vec![sns])),
        (
            "global".to_owned(),
            AmazonCollection::AmazonCloudFront(vec![cloudfront]),
        ),
        (r.clone(), AmazonCollection::AmazonSecurityGroups(vec![sg])),
        (
            r.clone(),
            AmazonCollection::AmazonConnectivity(AWSConnectivity {
                vpcs,
                peering_connections,
                transit_gateways,
                transit_gateway_attachments,
                vpn_gateways,
                customer_gateways,
                vpn_connections,
            }),
        ),
        (
            r.clone(),
            AmazonCollection::AmazonTransitGatewayRouting(AWSTransitGatewayRouting {
                route_tables: transit_gateway_route_tables,
                peerings: vec![peering_to_west()],
            }),
        ),
        (
            WEST_REGION.to_owned(),
            AmazonCollection::AmazonConnectivity(west),
        ),
        (
            WEST_REGION.to_owned(),
            AmazonCollection::AmazonTransitGatewayRouting(AWSTransitGatewayRouting {
                route_tables: west_route_tables,
                peerings: vec![peering_to_west()],
            }),
        ),
        (
            WEST_REGION.to_owned(),
            AmazonCollection::AmazonNetworkInterfaces(vec![west_api]),
        ),
        (
            r.clone(),
            AmazonCollection::AmazonDirectConnect(direct_connect),
        ),
        (
            "global".to_owned(),
            AmazonCollection::AmazonDirectConnectGateways(direct_connect_gateways),
        ),
        (
            r,
            AmazonCollection::AmazonNetworking(AWSNetworking {
                route_tables: vec![public_rt, private_rt, data_rt],
                internet_gateways: vec![igw],
                nat_gateways: vec![nat],
                addresses: vec![eip, web_eip],
            }),
        ),
    ]
}

pub fn observed() -> FlowIndex {
    let mut index = FlowIndex::default();
    for flow in flows() {
        index.observe(&flow);
    }
    index
}

pub fn flows() -> Vec<FlowObservation> {
    let now = now_millis();
    let flow_in = |network: &str,
                   (src, src_port): (&str, u16),
                   (dst, dst_port): (&str, u16),
                   resources: Vec<Node>,
                   packets: u64,
                   action| FlowObservation {
        source: CollectionSource::Aws,
        scope: REGION.to_owned(),
        src: Node::ip(src, network),
        dst: Node::ip(dst, network),
        src_port: Some(src_port),
        dst_port: Some(dst_port),
        resources,
        packets,
        bytes: packets.saturating_mul(AVERAGE_PACKET_BYTES),
        action,
        observed_at: now,
    };
    let flow = |src, dst, resources, packets, action| {
        flow_in(AWS_NETWORK, src, dst, resources, packets, action)
    };
    let web01 = || {
        vec![
            Node::AwsEc2Instance("i-globex-web-01".into()),
            Node::AwsEc2Eni("eni-globex-web-01a".into()),
        ]
    };
    let web02 = || {
        vec![
            Node::AwsEc2Instance("i-globex-web-02".into()),
            Node::AwsEc2Eni("eni-globex-web-02a".into()),
        ]
    };
    let alb = || vec![Node::AwsEc2Eni("eni-globex-alb-1a".into())];
    let nat = || vec![Node::AwsEc2Eni("eni-globex-nat-1a".into())];

    vec![
        flow(
            ("10.10.1.50", 43122),
            ("10.10.1.10", 8080),
            alb(),
            210_000,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.50", 43188),
            ("10.10.1.11", 8080),
            alb(),
            198_400,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.2.20", 51544),
            ("10.10.2.30", 5432),
            vec![Node::AwsEc2Eni("eni-globex-web-01b".into())],
            58_600,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.10", 38001),
            ("198.51.100.10", 443),
            web01(),
            184_000,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.11", 39002),
            ("10.20.0.5", 5432),
            web02(),
            96_400,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("192.0.2.55", 55123),
            ("10.10.1.10", 443),
            web01(),
            61_200,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.11", 40100),
            ("203.0.113.77", 443),
            web02(),
            42_800,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("203.0.113.10", 44000),
            ("34.120.0.9", 443),
            Vec::new(),
            12_500,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("2001:0db8:0000:0000:0000:0000:0000:0010", 45000),
            ("198.51.100.10", 443),
            Vec::new(),
            7_400,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.11", 40200),
            ("203.0.113.77", 8443),
            Vec::new(),
            940,
            Some(FlowAction::Rejected),
        ),
        flow(
            ("203.0.113.50", 61000),
            ("10.10.1.11", 22),
            nat(),
            310,
            Some(FlowAction::Rejected),
        ),
        // Replies. A flow log writes each as its own record with the addresses
        // swapped, and taken at packet direction every one of them would draw
        // its relationship backwards beside the forwards one.
        flow(
            ("10.10.1.10", 8080),
            ("10.10.1.50", 43122),
            web01(),
            206_500,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.2.30", 5432),
            ("10.10.2.20", 51544),
            vec![Node::AwsEc2Eni("eni-globex-rds-1a".into())],
            57_900,
            Some(FlowAction::Accepted),
        ),
        // A request to an Elastic IP associated with web-02: the address object
        // must resolve to the instance behind it, not stand in for it.
        flow(
            ("10.10.1.10", 40500),
            ("198.51.100.40", 443),
            web01(),
            3_100,
            Some(FlowAction::Accepted),
        ),
        // The other two clouds' compute: an Azure VM by the private address
        // its NIC holds, reached over the interconnect, and a GCE instance by
        // the one-to-one NAT address its access config carries. The NAT
        // address is public, so it names the instance; the private one is read
        // in this VPC, where nothing holds it.
        flow(
            ("10.10.1.10", 41000),
            ("10.20.1.4", 1433),
            web01(),
            21_700,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.11", 42000),
            ("34.120.0.21", 443),
            web02(),
            18_300,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.10", 41500),
            ("10.40.1.20", 5439),
            web01(),
            15_200,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.11", 42500),
            ("10.50.0.10", 636),
            web02(),
            8_400,
            Some(FlowAction::Accepted),
        ),
        flow(
            ("10.10.1.11", 43100),
            (WEST_API, 443),
            web02(),
            6_200,
            Some(FlowAction::Accepted),
        ),
        flow_in(
            DATA_NETWORK,
            ("10.40.1.20", 47100),
            ("10.20.0.5", 5432),
            vec![
                Node::AwsEc2Instance("i-globex-warehouse".into()),
                Node::AwsEc2Eni("eni-globex-warehouse".into()),
            ],
            33_600,
            Some(FlowAction::Accepted),
        ),
    ]
}

pub fn burst_flow() -> FlowObservation {
    FlowObservation {
        source: CollectionSource::Aws,
        scope: REGION.to_owned(),
        src: Node::ip("192.0.2.99", AWS_NETWORK),
        dst: Node::ip("10.10.1.10", AWS_NETWORK),
        src_port: Some(58000),
        dst_port: Some(22),
        resources: vec![
            Node::AwsEc2Instance("i-globex-web-01".into()),
            Node::AwsEc2Eni("eni-globex-web-01a".into()),
        ],
        packets: 4_800,
        bytes: 4_800 * AVERAGE_PACKET_BYTES,
        action: Some(FlowAction::Rejected),
        observed_at: now_millis(),
    }
}

pub const WEB_04: &str = "i-globex-web-04";
const WEB_04_INTERFACE: &str = "eni-globex-web-04a";
const WEB_04_ADDRESS: &str = "10.10.1.13";
const WEB_SUBNET: &str = "subnet-public-1a";
const WEB_TARGET_GROUP: &str =
    "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-web/1";

pub fn with_web_04(
    mut collections: Vec<(String, AmazonCollection)>,
) -> Vec<(String, AmazonCollection)> {
    use aws_sdk_ec2::types::{
        GroupIdentifier, InstanceNetworkInterface, NetworkInterface, NetworkInterfaceAttachment,
        NetworkInterfacePrivateIpAddress, NetworkInterfaceType, Placement,
        builders::InstanceBuilder,
    };
    use aws_sdk_elasticloadbalancingv2::types::{TargetDescription, TargetHealthDescription};

    for (region, collection) in &mut collections {
        if region != REGION {
            continue;
        }
        match collection {
            AmazonCollection::AmazonInstances(instances) => instances.push(
                InstanceBuilder::default()
                    .set_instance_id(Some(WEB_04.to_owned()))
                    .set_vpc_id(Some(AWS_NETWORK.to_owned()))
                    .set_subnet_id(Some(WEB_SUBNET.to_owned()))
                    .set_private_ip_address(Some(WEB_04_ADDRESS.to_owned()))
                    .set_placement(Some(
                        Placement::builder().availability_zone("us-east-1a").build(),
                    ))
                    .set_security_groups(Some(vec![
                        GroupIdentifier::builder()
                            .group_id("sg-web")
                            .group_name("globex-web")
                            .build(),
                    ]))
                    .set_network_interfaces(Some(vec![
                        InstanceNetworkInterface::builder()
                            .network_interface_id(WEB_04_INTERFACE)
                            .subnet_id(WEB_SUBNET)
                            .vpc_id(AWS_NETWORK)
                            .build(),
                    ]))
                    .build(),
            ),
            AmazonCollection::AmazonNetworkInterfaces(interfaces) => interfaces.push(
                NetworkInterface::builder()
                    .network_interface_id(WEB_04_INTERFACE)
                    .subnet_id(WEB_SUBNET)
                    .vpc_id(AWS_NETWORK)
                    .groups(GroupIdentifier::builder().group_id("sg-web").build())
                    .private_ip_addresses(
                        NetworkInterfacePrivateIpAddress::builder()
                            .private_ip_address(WEB_04_ADDRESS)
                            .build(),
                    )
                    .interface_type(NetworkInterfaceType::Interface)
                    .attachment(
                        NetworkInterfaceAttachment::builder()
                            .instance_id(WEB_04)
                            .build(),
                    )
                    .build(),
            ),
            AmazonCollection::AmazonLoadBalancers(balancing) => balancing
                .target_health
                .entry(WEB_TARGET_GROUP.to_owned())
                .or_default()
                .push(
                    TargetHealthDescription::builder()
                        .target(TargetDescription::builder().id(WEB_04).build())
                        .build(),
                ),
            _ => {}
        }
    }
    collections
}

pub fn web_04_flows() -> Vec<FlowObservation> {
    let now = now_millis();
    let flow = |(src, src_port): (&str, u16),
                (dst, dst_port): (&str, u16),
                resources: Vec<Node>,
                packets: u64| FlowObservation {
        source: CollectionSource::Aws,
        scope: REGION.to_owned(),
        src: Node::ip(src, AWS_NETWORK),
        dst: Node::ip(dst, AWS_NETWORK),
        src_port: Some(src_port),
        dst_port: Some(dst_port),
        resources,
        packets,
        bytes: packets.saturating_mul(AVERAGE_PACKET_BYTES),
        action: Some(FlowAction::Accepted),
        observed_at: now,
    };

    vec![
        flow(
            ("10.10.1.50", 43260),
            (WEB_04_ADDRESS, 8080),
            vec![Node::AwsEc2Eni("eni-globex-alb-1a".into())],
            176_000,
        ),
        flow(
            (WEB_04_ADDRESS, 8080),
            ("10.10.1.50", 43260),
            vec![
                Node::AwsEc2Instance(WEB_04.into()),
                Node::AwsEc2Eni(WEB_04_INTERFACE.into()),
            ],
            171_300,
        ),
    ]
}

pub fn web_01_addresses() -> [Node; 2] {
    [
        Node::ip("10.10.1.10", AWS_NETWORK),
        Node::ip("10.10.2.20", AWS_NETWORK),
    ]
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
