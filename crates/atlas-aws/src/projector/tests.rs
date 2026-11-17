use super::*;
use crate::fixtures;
use atlas_graph::flow::FlowIndex;
use atlas_graph::testing::{
    assert_edge, assert_has_node, assert_resolves, link, public, serves, serves_any,
};

#[test]
fn every_aws_kind_config_records_has_one_source() {
    use aws_sdk_config::types::ResourceType;

    let config_records_nothing_like = [
        "AwsRegion",
        "AwsTag",
        "AwsEc2AvailabilityZone",
        "AwsRoute53RecordSet",
        "AwsConfigResource",
        "AwsDirectConnectConnection",
        "AwsDirectConnectGateway",
        "AwsDirectConnectVirtualInterface",
        "AwsEc2Route",
        "AwsEc2TransitGatewayRoute",
        "AwsEc2TransitGatewayPeeringAttachment",
    ];
    let typed: Vec<&str> = TYPED_CONFIG_TYPES
        .iter()
        .map(|typed| (typed.node)("probe".into()).kind())
        .collect();
    for kind in Node::ALL_KINDS
        .iter()
        .filter(|kind| kind.starts_with("Aws"))
    {
        assert!(
            typed.contains(kind) != config_records_nothing_like.contains(kind),
            "{kind}: add its Config type to TYPED_CONFIG_TYPES, or list it as a kind Config \
             does not record"
        );
    }
    for typed in TYPED_CONFIG_TYPES {
        assert!(
            ResourceType::values().contains(&typed.resource_type),
            "{} is not a Config resource type",
            typed.resource_type
        );
        assert!(
            !use_aws_resource(typed.resource_type, false),
            "{} has a typed node, so the catch-all must never draw it",
            typed.resource_type
        );
    }
}

#[test]
fn a_config_listing_never_redraws_a_resource_its_own_collector_draws() {
    use aws_sdk_config::types::ResourceIdentifier;
    use std::collections::HashMap;

    let listed = |id: &str| vec![ResourceIdentifier::builder().resource_id(id).build()];
    let mut resources: HashMap<String, Vec<ResourceIdentifier>> = TYPED_CONFIG_TYPES
        .iter()
        .map(|typed| (typed.resource_type.to_owned(), listed("twin")))
        .collect();
    for drawn_as_an_edge in [
        "AWS::EC2::TransitGatewayAttachment",
        "AWS::ElasticLoadBalancingV2::Listener",
    ] {
        resources.insert(drawn_as_an_edge.to_owned(), listed("twin"));
    }
    resources.insert("AWS::S3::Bucket".to_owned(), listed("assets"));

    let mut builder = GraphBuilder::new();
    aws_projector(
        &mut builder,
        &[(
            "us-east-1".to_owned(),
            AmazonCollection::AmazonResources(resources),
        )],
        &Settings {
            regions: vec!["us-east-1".to_owned()],
            ..Default::default()
        },
    );

    let generic: Vec<&Node> = builder
        .graph
        .node_weights()
        .filter(|node| matches!(node, Node::AwsConfigResource { .. }))
        .collect();
    assert_eq!(
        generic,
        vec![&Node::AwsConfigResource {
            resource_type: "AWS::S3::Bucket".into(),
            id: "assets".into(),
        }],
        "Config is the source only for what no collector draws"
    );
}

fn record(zone: &str, name: &str, record_type: &str) -> Node {
    Node::AwsRoute53RecordSet {
        zone: zone.into(),
        name: name.into(),
        record_type: record_type.into(),
    }
}

fn globex_record(name: &str, record_type: &str) -> Node {
    record(fixtures::GLOBEX_ZONE, name, record_type)
}

#[test]
fn amazon_projection() {
    let mut builder = GraphBuilder::new();
    aws_projector(
        &mut builder,
        &fixtures::collections(),
        &atlas_provider::fixtures::settings(),
    );

    let instance = Node::AwsEc2Instance("i-globex-web-01".into());
    let eni = Node::AwsEc2Eni("eni-globex-web-01a".into());
    let second_eni = Node::AwsEc2Eni("eni-globex-web-01b".into());
    let subnet = Node::AwsEc2Subnet("subnet-public-1a".into());
    let private_subnet = Node::AwsEc2Subnet("subnet-private-1a".into());
    let vpc = Node::AwsEc2Vpc("vpc-globex".into());
    let az = Node::AwsEc2AvailabilityZone("us-east-1a".into());
    let region = Node::AwsRegion(atlas_provider::fixtures::REGION.into());
    let global = Node::AwsRegion("global".into());
    let tag = Node::AwsTag {
        key: "env".into(),
        value: "prod".into(),
    };
    let sg = Node::AwsEc2SecurityGroup("sg-web".into());
    let sg_lb = Node::AwsEc2SecurityGroup("sg-lb".into());
    let lb = Node::AwsElbLoadBalancer(
        "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/globex/1".into(),
    );
    let tg = Node::AwsElbTargetGroup(
        "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-web/1".into(),
    );
    let role = Node::AwsIamRole("arn:aws:iam::123:role/globex-lambda-role".into());
    let lambda = Node::AwsLambdaFunction("globex-events-handler".into());

    assert_edge(&builder, &instance, &eni, &Edge::HasIp);
    assert_edge(&builder, &eni, &subnet, &Edge::AttachedTo);

    assert_edge(&builder, &instance, &second_eni, &Edge::HasIp);
    assert_edge(&builder, &second_eni, &private_subnet, &Edge::AttachedTo);
    assert_edge(&builder, &region, &vpc, &Edge::Contains);
    assert_edge(&builder, &vpc, &subnet, &Edge::Contains);
    assert_edge(&builder, &az, &instance, &Edge::Contains);
    assert_edge(&builder, &instance, &tag, &Edge::DependsOn);
    assert_edge(&builder, &instance, &sg, &Edge::ConnectsTo);

    assert_edge(&builder, &lb, &tg, &Edge::ConnectsTo);
    assert_edge(&builder, &tg, &instance, &Edge::ConnectsTo);

    let nlb = Node::AwsElbLoadBalancer(
        "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/globex-db/1".into(),
    );
    let tg_db = Node::AwsElbTargetGroup(
        "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-db/1".into(),
    );
    assert_edge(&builder, &nlb, &tg_db, &Edge::ConnectsTo);
    assert_edge(
        &builder,
        &tg_db,
        &Node::ip("10.10.2.30", fixtures::AWS_NETWORK),
        &Edge::RoutesTo,
    );

    assert_edge(&builder, &lambda, &role, &Edge::DependsOn);
    assert_edge(&builder, &lambda, &sg, &Edge::ConnectsTo);

    assert_edge(&builder, &sg_lb, &sg, &Edge::ConnectsTo);
    assert_edge(
        &builder,
        &sg,
        &Node::GenericIpAddress("192.0.2.44/32".into()),
        &Edge::RoutesTo,
    );
    assert_edge(
        &builder,
        &sg,
        &Node::GenericIpAddress("2001:db8::44/128".into()),
        &Edge::RoutesTo,
    );

    assert_resolves(
        &builder,
        "origin.globex.io",
        &globex_record("origin.globex.io.", "A"),
        &public("203.0.113.10"),
    );
    assert_resolves(
        &builder,
        "app.globex.io",
        &globex_record("app.globex.io.", "CNAME"),
        &Node::hostname("app-globex.azurewebsites.net"),
    );

    let s3 = Node::AwsConfigResource {
        resource_type: "AWS::S3::Bucket".into(),
        id: "globex-assets".into(),
    };
    assert_edge(&builder, &global, &s3, &Edge::Contains);

    let public_rt = Node::AwsEc2RouteTable("rtb-public".into());
    let private_rt = Node::AwsEc2RouteTable("rtb-private".into());
    let igw = Node::AwsEc2InternetGateway("igw-globex".into());
    let nat = Node::AwsEc2NatGateway("nat-globex".into());
    let eip = Node::AwsEc2Eip("eipalloc-globex".into());
    let private_subnet = Node::AwsEc2Subnet("subnet-private-1a".into());
    assert_edge(&builder, &vpc, &public_rt, &Edge::Contains);
    assert_edge(&builder, &subnet, &public_rt, &Edge::AttachedTo);
    let default_route = vpc_route("rtb-public", "0.0.0.0/0");
    assert_edge(&builder, &public_rt, &default_route, &Edge::Contains);
    assert_edge(&builder, &default_route, &igw, &Edge::RoutesTo);
    assert_edge(
        &builder,
        &vpc_route("rtb-public", "10.10.0.0/16"),
        &vpc,
        &Edge::RoutesTo,
    );
    assert_edge(&builder, &igw, &vpc, &Edge::AttachedTo);
    assert_edge(&builder, &private_subnet, &private_rt, &Edge::AttachedTo);
    assert_edge(
        &builder,
        &vpc_route("rtb-private", "0.0.0.0/0"),
        &nat,
        &Edge::RoutesTo,
    );
    assert_edge(&builder, &nat, &subnet, &Edge::AttachedTo);
    assert_edge(&builder, &nat, &eip, &Edge::HasIp);
    assert_edge(
        &builder,
        &eip,
        &Node::GenericIpAddress("203.0.113.50".into()),
        &Edge::ConnectsTo,
    );

    assert_has_node(&builder, &Node::AwsEksCluster("globex-k8s".into()));
    assert_has_node(&builder, &Node::AwsApiGatewayRestApi("api-globex".into()));
    assert_has_node(&builder, &Node::AwsRdsDbInstance("globex-orders-db".into()));
    assert_has_node(&builder, &Node::AwsDynamoDbTable("globex-events".into()));
    assert_has_node(
        &builder,
        &Node::AwsSqsQueue("https://sqs.us-east-1.amazonaws.com/123/globex-jobs".into()),
    );
    assert_has_node(
        &builder,
        &Node::AwsSnsTopic("arn:aws:sns:us-east-1:123:globex-alerts".into()),
    );
    assert_has_node(
        &builder,
        &Node::AwsCloudFrontDistribution("EGLOBEX1".into()),
    );
    assert_has_node(
        &builder,
        &Node::AwsEcsCluster("arn:aws:ecs:us-east-1:123:cluster/globex-services".into()),
    );
}

#[test]
fn aws_connectivity_projection() {
    let mut builder = GraphBuilder::new();
    aws_projector(
        &mut builder,
        &fixtures::collections(),
        &atlas_provider::fixtures::settings(),
    );

    let region = Node::AwsRegion(atlas_provider::fixtures::REGION.into());
    let vpc = |id: &str| Node::AwsEc2Vpc(id.into());
    for id in [
        fixtures::AWS_NETWORK,
        "vpc-globex-data",
        "vpc-globex-shared",
        "vpc-globex-staging",
    ] {
        assert_edge(&builder, &region, &vpc(id), &Edge::Contains);
    }

    let pcx = Node::AwsEc2VpcPeeringConnection("pcx-globex-data".into());
    assert_edge(
        &builder,
        &pcx,
        &vpc(fixtures::AWS_NETWORK),
        &Edge::AttachedTo,
    );
    assert_edge(&builder, &pcx, &vpc("vpc-globex-data"), &Edge::AttachedTo);
    assert!(
        !builder.contains(&Node::AwsEc2VpcPeeringConnection(
            "pcx-globex-staging".into()
        )),
        "a rejected peering request connects nothing"
    );

    let tgw = Node::AwsEc2TransitGateway("tgw-globex".into());
    assert_edge(&builder, &region, &tgw, &Edge::Contains);
    assert_edge(
        &builder,
        &tgw,
        &vpc(fixtures::AWS_NETWORK),
        &Edge::AttachedTo,
    );
    assert_edge(&builder, &tgw, &vpc("vpc-globex-shared"), &Edge::AttachedTo);
    assert!(!builder.has_edge(&tgw, &vpc("vpc-globex-staging"), &Edge::AttachedTo));

    assert_edge(
        &builder,
        &vpc_route("rtb-private", "10.40.0.0/16"),
        &pcx,
        &Edge::RoutesTo,
    );
    assert_edge(
        &builder,
        &vpc_route("rtb-private", "10.50.0.0/16"),
        &tgw,
        &Edge::RoutesTo,
    );
    assert_edge(
        &builder,
        &vpc_route("rtb-private", "10.20.1.0/24"),
        &Node::AwsEc2VpnGateway("vgw-globex".into()),
        &Edge::RoutesTo,
    );
    let retired = vpc_route("rtb-data", "10.60.0.0/16");
    assert_edge(
        &builder,
        &Node::AwsEc2RouteTable("rtb-data".into()),
        &retired,
        &Edge::Contains,
    );
    assert!(
        builder
            .graph
            .edges_directed(
                builder.index_of(&retired).unwrap(),
                petgraph::Direction::Outgoing
            )
            .next()
            .is_none(),
        "a blackhole route goes nowhere"
    );
}

#[test]
fn only_live_routes_and_attachments_join_networks() {
    use crate::definition::{AWSConnectivity, AWSNetworking, AmazonCollection};
    use aws_sdk_ec2::types::{
        Route, RouteState, RouteTable, TransitGateway, TransitGatewayAttachment,
        TransitGatewayAttachmentResourceType, TransitGatewayAttachmentState, TransitGatewayState,
    };

    let builder = project_aws(vec![
        AmazonCollection::AmazonNetworking(AWSNetworking {
            route_tables: vec![
                RouteTable::builder()
                    .route_table_id("rtb-1")
                    .vpc_id("vpc-a")
                    .routes(
                        Route::builder()
                            .destination_cidr_block("10.9.0.0/16")
                            .vpc_peering_connection_id("pcx-gone")
                            .state(RouteState::Blackhole)
                            .build(),
                    )
                    .routes(
                        Route::builder()
                            .destination_cidr_block("0.0.0.0/0")
                            .nat_gateway_id("nat-gone")
                            .state(RouteState::Blackhole)
                            .build(),
                    )
                    .build(),
            ],
            internet_gateways: Vec::new(),
            nat_gateways: Vec::new(),
            addresses: Vec::new(),
        }),
        AmazonCollection::AmazonConnectivity(AWSConnectivity {
            transit_gateways: vec![
                TransitGateway::builder()
                    .transit_gateway_id("tgw-new")
                    .state(TransitGatewayState::Pending)
                    .build(),
            ],
            transit_gateway_attachments: vec![
                TransitGatewayAttachment::builder()
                    .transit_gateway_id("tgw-1")
                    .resource_type(TransitGatewayAttachmentResourceType::Vpc)
                    .resource_id("vpc-a")
                    .state(TransitGatewayAttachmentState::PendingAcceptance)
                    .build(),
                TransitGatewayAttachment::builder()
                    .transit_gateway_id("tgw-1")
                    .resource_type(TransitGatewayAttachmentResourceType::Vpn)
                    .resource_id("vpn-1")
                    .state(TransitGatewayAttachmentState::Available)
                    .build(),
            ],
            ..Default::default()
        }),
    ]);

    assert!(
        !builder.contains(&Node::AwsEc2VpcPeeringConnection("pcx-gone".into())),
        "a blackhole route names a peering that no longer exists"
    );
    assert!(
        !builder.contains(&Node::AwsEc2NatGateway("nat-gone".into())),
        "a blackhole route draws nothing, whatever its target"
    );
    assert_has_node(&builder, &vpc_route("rtb-1", "10.9.0.0/16"));
    assert!(!builder.contains(&Node::AwsEc2TransitGateway("tgw-new".into())));
    assert!(
        !builder
            .graph
            .node_weights()
            .any(|n| matches!(n, Node::AwsEc2TransitGateway(_))),
        "neither a pending attachment nor a VPN attachment draws the gateway"
    );
}

#[test]
fn only_a_working_vpn_is_projected() {
    use crate::definition::{AWSConnectivity, AmazonCollection};
    use aws_sdk_ec2::types::{
        AttachmentStatus, CustomerGateway, VpcAttachment, VpnConnection, VpnGateway, VpnState,
    };

    let builder = project_aws(vec![AmazonCollection::AmazonConnectivity(
        AWSConnectivity {
            vpn_gateways: vec![
                VpnGateway::builder()
                    .vpn_gateway_id("vgw-detached")
                    .state(VpnState::Available)
                    .vpc_attachments(
                        VpcAttachment::builder()
                            .vpc_id("vpc-old")
                            .state(AttachmentStatus::Detached)
                            .build(),
                    )
                    .build(),
                VpnGateway::builder()
                    .vpn_gateway_id("vgw-gone")
                    .state(VpnState::Deleted)
                    .build(),
            ],
            customer_gateways: vec![
                CustomerGateway::builder()
                    .customer_gateway_id("cgw-gone")
                    .ip_address("198.51.100.9")
                    .state("deleted")
                    .build(),
            ],
            vpn_connections: vec![
                VpnConnection::builder()
                    .vpn_connection_id("vpn-pending")
                    .state(VpnState::Pending)
                    .customer_gateway_id("cgw-gone")
                    .build(),
            ],
            ..Default::default()
        },
    )]);

    assert_has_node(&builder, &Node::AwsEc2VpnGateway("vgw-detached".into()));
    assert!(
        !builder.contains(&Node::AwsEc2Vpc("vpc-old".into())),
        "a detached gateway is not attached to the VPC"
    );
    for gone in [
        Node::AwsEc2VpnGateway("vgw-gone".into()),
        Node::AwsEc2CustomerGateway("cgw-gone".into()),
        Node::AwsEc2VpnConnection("vpn-pending".into()),
    ] {
        assert!(!builder.contains(&gone), "{gone:?} is not available");
    }
}

#[test]
fn only_a_direct_connect_circuit_that_reaches_a_network_is_projected() {
    use crate::definition::{AWSDirectConnect, AWSDirectConnectGateways, AmazonCollection};
    use aws_sdk_directconnect::types::{
        AssociatedGateway, BgpPeer, BgpPeerState, Connection, ConnectionState,
        DirectConnectGateway, DirectConnectGatewayAssociation,
        DirectConnectGatewayAssociationState, DirectConnectGatewayState, GatewayType,
        VirtualInterface, VirtualInterfaceState,
    };

    let interface = |id: &str, kind: &str, state: VirtualInterfaceState| {
        VirtualInterface::builder()
            .virtual_interface_id(id)
            .virtual_interface_type(kind)
            .virtual_interface_state(state)
    };
    let peer = |amazon: &str, customer: &str, state: BgpPeerState| {
        BgpPeer::builder()
            .amazon_address(amazon)
            .customer_address(customer)
            .bgp_peer_state(state)
            .build()
    };
    let association = |associated: AssociatedGateway,
                       state: DirectConnectGatewayAssociationState| {
        DirectConnectGatewayAssociation::builder()
            .direct_connect_gateway_id("dxgw-a")
            .associated_gateway(associated)
            .association_state(state)
            .build()
    };
    let builder = project_aws(vec![
        AmazonCollection::AmazonDirectConnect(AWSDirectConnect {
            connections: vec![
                Connection::builder()
                    .connection_id("dxcon-a")
                    .connection_state(ConnectionState::Down)
                    .build(),
                Connection::builder()
                    .connection_id("dxcon-ordering")
                    .connection_state(ConnectionState::Ordering)
                    .build(),
            ],
            virtual_interfaces: vec![
                interface("vif-private", "private", VirtualInterfaceState::Available)
                    .connection_id("dxcon-a")
                    .virtual_gateway_id("vgw-a")
                    .direct_connect_gateway_id("")
                    .amazon_address("169.254.1.1/30")
                    .customer_address("169.254.1.2/30")
                    .bgp_peers(peer(
                        "169.254.1.1/30",
                        "169.254.1.2/30",
                        BgpPeerState::Available,
                    ))
                    .bgp_peers(peer(
                        "169.254.9.1/30",
                        "169.254.9.2/30",
                        BgpPeerState::Deleting,
                    ))
                    .build(),
                interface("vif-transit", "transit", VirtualInterfaceState::Down)
                    .connection_id("dxlag-a")
                    .virtual_gateway_id("")
                    .direct_connect_gateway_id("dxgw-a")
                    .amazon_address("169.254.2.1/30")
                    .customer_address("169.254.2.2/30")
                    .build(),
                interface("vif-orphaned", "private", VirtualInterfaceState::Down)
                    .connection_id("dxcon-a")
                    .virtual_gateway_id("")
                    .direct_connect_gateway_id("dxgw-deleting")
                    .build(),
                interface("vif-public", "public", VirtualInterfaceState::Available)
                    .connection_id("dxcon-a")
                    .build(),
                interface("vif-pending", "private", VirtualInterfaceState::Pending)
                    .connection_id("dxcon-a")
                    .build(),
            ],
        }),
        AmazonCollection::AmazonDirectConnectGateways(AWSDirectConnectGateways {
            gateways: vec![
                DirectConnectGateway::builder()
                    .direct_connect_gateway_id("dxgw-a")
                    .direct_connect_gateway_state(DirectConnectGatewayState::Available)
                    .build(),
                DirectConnectGateway::builder()
                    .direct_connect_gateway_id("dxgw-deleting")
                    .direct_connect_gateway_state(DirectConnectGatewayState::Deleting)
                    .build(),
            ],
            associations: vec![
                association(
                    AssociatedGateway::builder()
                        .id("tgw-a")
                        .r#type(GatewayType::TransitGateway)
                        .build(),
                    DirectConnectGatewayAssociationState::Updating,
                ),
                association(
                    AssociatedGateway::builder()
                        .id("vgw-old")
                        .r#type(GatewayType::VirtualPrivateGateway)
                        .build(),
                    DirectConnectGatewayAssociationState::Disassociated,
                ),
                DirectConnectGatewayAssociation::builder()
                    .direct_connect_gateway_id("dxgw-deleting")
                    .associated_gateway(
                        AssociatedGateway::builder()
                            .id("vgw-b")
                            .r#type(GatewayType::VirtualPrivateGateway)
                            .build(),
                    )
                    .association_state(DirectConnectGatewayAssociationState::Associated)
                    .build(),
            ],
        }),
    ]);

    let gateway = Node::AwsDirectConnectGateway("dxgw-a".into());
    assert_edge(
        &builder,
        &Node::AwsEc2TransitGateway("tgw-a".into()),
        &gateway,
        &Edge::AttachedTo,
    );
    let private = Node::AwsDirectConnectVirtualInterface("vif-private".into());
    assert_edge(
        &builder,
        &private,
        &Node::AwsDirectConnectConnection("dxcon-a".into()),
        &Edge::AttachedTo,
    );
    assert_edge(
        &builder,
        &private,
        &Node::AwsEc2VpnGateway("vgw-a".into()),
        &Edge::AttachedTo,
    );
    assert_edge(
        &builder,
        &private,
        &link("169.254.1.1/30"),
        &Edge::ConnectsTo,
    );
    assert_edge(&builder, &private, &link("169.254.1.2/30"), &Edge::RoutesTo);
    let transit = Node::AwsDirectConnectVirtualInterface("vif-transit".into());
    assert_edge(&builder, &transit, &gateway, &Edge::AttachedTo);
    assert_edge(
        &builder,
        &transit,
        &link("169.254.2.1/30"),
        &Edge::ConnectsTo,
    );
    assert_has_node(
        &builder,
        &Node::AwsDirectConnectVirtualInterface("vif-orphaned".into()),
    );
    for absent in [
        Node::AwsDirectConnectConnection("dxcon-ordering".into()),
        Node::AwsDirectConnectConnection("dxlag-a".into()),
        Node::AwsDirectConnectGateway("dxgw-deleting".into()),
        Node::AwsEc2VpnGateway("vgw-old".into()),
        Node::AwsEc2VpnGateway("vgw-b".into()),
        Node::AwsEc2VpnGateway("".into()),
        Node::AwsDirectConnectGateway("".into()),
        Node::AwsDirectConnectVirtualInterface("vif-public".into()),
        Node::AwsDirectConnectVirtualInterface("vif-pending".into()),
        link("169.254.9.1/30"),
    ] {
        assert!(!builder.contains(&absent), "{absent:?} is not projected");
    }
}

#[test]
fn a_gateway_being_modified_still_joins_its_networks() {
    use crate::definition::{AWSConnectivity, AmazonCollection};
    use aws_sdk_ec2::types::{
        TransitGateway, TransitGatewayAttachment, TransitGatewayAttachmentResourceType,
        TransitGatewayAttachmentState, TransitGatewayState,
    };

    let attachment = |vpc: &str| {
        TransitGatewayAttachment::builder()
            .transit_gateway_id("tgw-1")
            .resource_type(TransitGatewayAttachmentResourceType::Vpc)
            .resource_id(vpc)
            .state(TransitGatewayAttachmentState::Modifying)
            .build()
    };
    let builder = project_aws(vec![AmazonCollection::AmazonConnectivity(
        AWSConnectivity {
            transit_gateways: vec![
                TransitGateway::builder()
                    .transit_gateway_id("tgw-1")
                    .state(TransitGatewayState::Modifying)
                    .build(),
            ],
            transit_gateway_attachments: vec![attachment("vpc-a"), attachment("vpc-b")],
            ..Default::default()
        },
    )]);

    let tgw = Node::AwsEc2TransitGateway("tgw-1".into());
    assert_edge(
        &builder,
        &Node::AwsRegion("us-east-1".into()),
        &tgw,
        &Edge::Contains,
    );
    for vpc in ["vpc-a", "vpc-b"] {
        assert_edge(
            &builder,
            &tgw,
            &Node::AwsEc2Vpc(vpc.into()),
            &Edge::AttachedTo,
        );
    }
}

#[test]
fn an_instance_reporting_no_interfaces_keeps_its_own_address() {
    use aws_sdk_ec2::types::Instance;

    let instance = Instance::builder()
        .instance_id("i-lonely")
        .private_ip_address("10.0.9.9")
        .build();
    let mut builder = GraphBuilder::new();
    aws_projector(
        &mut builder,
        &[(
            "us-east-1".to_owned(),
            crate::definition::AmazonCollection::AmazonInstances(vec![instance]),
        )],
        &Settings::default(),
    );

    assert_edge(
        &builder,
        &Node::AwsEc2Instance("i-lonely".into()),
        &Node::ip("10.0.9.9", "i-lonely"),
        &Edge::ConnectsTo,
    );
}

const PUBLIC_ZONE: &str = "/hostedzone/ZPUBLIC";
const PRIVATE_ZONE: &str = "/hostedzone/ZPRIVATE";

fn split_horizon_zones() -> GraphBuilder {
    use crate::definition::{AWSRoute53, AmazonCollection, HostedZoneId};
    use aws_sdk_route53::types::{HostedZone, ResourceRecord, ResourceRecordSet, RrType};

    let zone = |id: &str| {
        HostedZone::builder()
            .id(id)
            .name("globex.io.")
            .caller_reference(id)
            .build()
            .unwrap()
    };
    let record_set = |name: &str, record_type: RrType, values: &[&str]| {
        values
            .iter()
            .fold(
                ResourceRecordSet::builder().name(name).r#type(record_type),
                |set, value| {
                    set.resource_records(ResourceRecord::builder().value(*value).build().unwrap())
                },
            )
            .build()
            .unwrap()
    };

    project_aws(vec![AmazonCollection::AmazonRoute53(AWSRoute53 {
        hosted_zones: vec![zone(PUBLIC_ZONE), zone(PRIVATE_ZONE)],
        record_sets: std::collections::HashMap::from([
            (
                HostedZoneId(PUBLIC_ZONE.into()),
                vec![
                    record_set("api.globex.io.", RrType::A, &["203.0.113.20"]),
                    record_set("api.globex.io.", RrType::Txt, &["\"v=spf1 -all\""]),
                    record_set("globex.io.", RrType::Mx, &["10 mail.globex.io."]),
                    record_set("globex.io.", RrType::Ns, &["ns-1.awsdns-01.org."]),
                ],
            ),
            (
                HostedZoneId(PRIVATE_ZONE.into()),
                vec![
                    record_set("api.globex.io.", RrType::A, &["10.10.1.50"]),
                    record_set("db.globex.io.", RrType::A, &["10.10.1.50"]),
                ],
            ),
        ]),
    })])
}

#[test]
fn records_sharing_a_name_stay_distinct_by_zone_and_type() {
    let builder = split_horizon_zones();
    let public_a = record(PUBLIC_ZONE, "api.globex.io.", "A");
    let public_txt = record(PUBLIC_ZONE, "api.globex.io.", "TXT");
    let private_a = record(PRIVATE_ZONE, "api.globex.io.", "A");

    assert_resolves(
        &builder,
        "api.globex.io",
        &public_a,
        &public("203.0.113.20"),
    );
    assert_resolves(
        &builder,
        "api.globex.io",
        &private_a,
        &Node::ip("10.10.1.50", PRIVATE_ZONE),
    );
    assert_edge(
        &builder,
        &Node::hostname("api.globex.io"),
        &public_txt,
        &Edge::ResolvesTo,
    );

    for (zone, records) in [
        (PUBLIC_ZONE, vec![&public_a, &public_txt]),
        (PRIVATE_ZONE, vec![&private_a]),
    ] {
        for record in records {
            assert_edge(
                &builder,
                &Node::AwsRoute53HostedZone(zone.into()),
                record,
                &Edge::Contains,
            );
        }
    }
}

#[test]
fn a_private_answer_is_scoped_to_its_hosted_zone() {
    let builder = split_horizon_zones();
    let answer = Node::ip("10.10.1.50", PRIVATE_ZONE);

    for name in ["api.globex.io.", "db.globex.io."] {
        assert_edge(
            &builder,
            &record(PRIVATE_ZONE, name, "A"),
            &answer,
            &Edge::ResolvesTo,
        );
        assert!(
            !builder.contains(&Node::ip("10.10.1.50", name)),
            "the zone is the namespace its private answers share, not each record's name"
        );
    }
}

#[test]
fn a_record_naming_no_address_or_host_resolves_to_nothing() {
    let builder = split_horizon_zones();

    for record in [
        record(PUBLIC_ZONE, "api.globex.io.", "TXT"),
        record(PUBLIC_ZONE, "globex.io.", "MX"),
        record(PUBLIC_ZONE, "globex.io.", "NS"),
    ] {
        let index = builder.node_map[&record];
        assert_eq!(
            builder
                .graph
                .edges_directed(index, petgraph::Direction::Outgoing)
                .count(),
            0,
            "{record} must not turn its value into a hostname pivot"
        );
    }
}

#[test]
fn only_live_transit_gateway_routing_is_projected() {
    use crate::definition::{
        AWSConnectivity, AWSTransitGatewayRouteTable, AWSTransitGatewayRouting, AmazonCollection,
    };
    use aws_sdk_ec2::types::{
        PeeringTgwInfo, TransitGateway, TransitGatewayAssociationState,
        TransitGatewayAttachmentResourceType, TransitGatewayAttachmentState,
        TransitGatewayPeeringAttachment, TransitGatewayPropagationState, TransitGatewayRoute,
        TransitGatewayRouteAttachment, TransitGatewayRouteState, TransitGatewayRouteTable,
        TransitGatewayRouteTableAssociation, TransitGatewayRouteTablePropagation,
        TransitGatewayState,
    };

    let gateway = |id: &str, state: TransitGatewayState| {
        TransitGateway::builder()
            .transit_gateway_id(id)
            .state(state)
            .build()
    };
    let table = |id: &str, tgw: &str| {
        TransitGatewayRouteTable::builder()
            .transit_gateway_route_table_id(id)
            .transit_gateway_id(tgw)
            .build()
    };
    let association = |vpc: &str, state: TransitGatewayAssociationState| {
        TransitGatewayRouteTableAssociation::builder()
            .resource_type(TransitGatewayAttachmentResourceType::Vpc)
            .resource_id(vpc)
            .state(state)
            .build()
    };
    let propagation = |vpc: &str, state: TransitGatewayPropagationState| {
        TransitGatewayRouteTablePropagation::builder()
            .resource_type(TransitGatewayAttachmentResourceType::Vpc)
            .resource_id(vpc)
            .state(state)
            .build()
    };
    let route = |destination: &str, state: TransitGatewayRouteState| {
        let builder = TransitGatewayRoute::builder();
        if destination.starts_with("pl-") {
            builder.prefix_list_id(destination)
        } else {
            builder.destination_cidr_block(destination)
        }
        .state(state)
        .transit_gateway_attachments(
            TransitGatewayRouteAttachment::builder()
                .resource_type(TransitGatewayAttachmentResourceType::Vpc)
                .resource_id("vpc-routed")
                .build(),
        )
        .build()
    };
    let peering = |id: &str, far: &str, state: TransitGatewayAttachmentState| {
        TransitGatewayPeeringAttachment::builder()
            .transit_gateway_attachment_id(id)
            .requester_tgw_info(
                PeeringTgwInfo::builder()
                    .transit_gateway_id("tgw-a")
                    .build(),
            )
            .accepter_tgw_info(PeeringTgwInfo::builder().transit_gateway_id(far).build())
            .state(state)
            .build()
    };
    let builder = project_aws(vec![
        AmazonCollection::AmazonConnectivity(AWSConnectivity {
            transit_gateways: vec![
                gateway("tgw-a", TransitGatewayState::Available),
                gateway("tgw-going", TransitGatewayState::Deleting),
            ],
            ..Default::default()
        }),
        AmazonCollection::AmazonTransitGatewayRouting(AWSTransitGatewayRouting {
            route_tables: vec![
                AWSTransitGatewayRouteTable {
                    table: table("tgw-rtb-a", "tgw-a"),
                    associations: vec![
                        association("vpc-on", TransitGatewayAssociationState::Associated),
                        association("vpc-joining", TransitGatewayAssociationState::Associating),
                    ],
                    propagations: vec![
                        propagation("vpc-on", TransitGatewayPropagationState::Enabled),
                        propagation("vpc-off", TransitGatewayPropagationState::Disabled),
                    ],
                    static_routes: vec![
                        route("10.8.0.0/16", TransitGatewayRouteState::Active),
                        route("10.9.0.0/16", TransitGatewayRouteState::Blackhole),
                        route("pl-0shared", TransitGatewayRouteState::Active),
                    ],
                },
                AWSTransitGatewayRouteTable {
                    table: table("tgw-rtb-going", "tgw-going"),
                    associations: Vec::new(),
                    propagations: Vec::new(),
                    static_routes: Vec::new(),
                },
            ],
            peerings: vec![
                peering(
                    "tgw-attach-live",
                    "tgw-far",
                    TransitGatewayAttachmentState::Available,
                ),
                peering(
                    "tgw-attach-asked",
                    "tgw-far",
                    TransitGatewayAttachmentState::PendingAcceptance,
                ),
                peering(
                    "tgw-attach-to-going",
                    "tgw-going",
                    TransitGatewayAttachmentState::Available,
                ),
            ],
        }),
    ]);

    let live = Node::AwsEc2TransitGatewayRouteTable("tgw-rtb-a".into());
    assert_edge(
        &builder,
        &Node::AwsEc2Vpc("vpc-on".into()),
        &live,
        &Edge::AttachedTo,
    );
    assert_edge(
        &builder,
        &live,
        &Node::AwsEc2Vpc("vpc-on".into()),
        &Edge::RoutesTo,
    );
    let active = Node::AwsEc2TransitGatewayRoute {
        table: "tgw-rtb-a".into(),
        destination: "10.8.0.0/16".into(),
    };
    assert_edge(
        &builder,
        &active,
        &Node::AwsEc2Vpc("vpc-routed".into()),
        &Edge::RoutesTo,
    );
    assert_edge(
        &builder,
        &Node::AwsEc2TransitGatewayRoute {
            table: "tgw-rtb-a".into(),
            destination: "pl-0shared".into(),
        },
        &Node::AwsEc2Vpc("vpc-routed".into()),
        &Edge::RoutesTo,
    );
    let blackhole = Node::AwsEc2TransitGatewayRoute {
        table: "tgw-rtb-a".into(),
        destination: "10.9.0.0/16".into(),
    };
    assert_has_node(&builder, &blackhole);
    assert!(!builder.has_edge(
        &blackhole,
        &Node::AwsEc2Vpc("vpc-routed".into()),
        &Edge::RoutesTo
    ));
    let live_peering = Node::AwsEc2TransitGatewayPeeringAttachment("tgw-attach-live".into());
    assert_edge(
        &builder,
        &live_peering,
        &Node::AwsEc2TransitGateway("tgw-far".into()),
        &Edge::AttachedTo,
    );
    for absent in [
        Node::AwsEc2Vpc("vpc-joining".into()),
        Node::AwsEc2Vpc("vpc-off".into()),
        Node::AwsEc2TransitGateway("tgw-going".into()),
        Node::AwsEc2TransitGatewayRouteTable("tgw-rtb-going".into()),
        Node::AwsEc2TransitGatewayPeeringAttachment("tgw-attach-asked".into()),
    ] {
        assert!(!builder.contains(&absent), "{absent:?} is not projected");
    }
}

fn vpc_route(table: &str, destination: &str) -> Node {
    Node::AwsEc2Route {
        table: table.into(),
        destination: destination.into(),
    }
}

fn project_aws(collections: Vec<crate::definition::AmazonCollection>) -> GraphBuilder {
    let mut builder = GraphBuilder::new();
    aws_projector(
        &mut builder,
        &collections
            .into_iter()
            .map(|c| ("us-east-1".to_owned(), c))
            .collect::<Vec<_>>(),
        &Settings::default(),
    );
    builder
}

fn attached_interface(id: &str, instance_id: &str) -> aws_sdk_ec2::types::NetworkInterface {
    use aws_sdk_ec2::types::{NetworkInterface, NetworkInterfaceAttachment};

    NetworkInterface::builder()
        .network_interface_id(id)
        .subnet_id("subnet-globex")
        .vpc_id("vpc-globex")
        .private_ip_address("10.0.5.5")
        .attachment(
            NetworkInterfaceAttachment::builder()
                .instance_id(instance_id)
                .build(),
        )
        .build()
}

// DescribeInstances is filtered to running/pending, but a stopped instance
// keeps its interfaces attached, so the attachment still names it.
#[test]
fn an_interface_attached_to_an_instance_the_scan_dropped_invents_no_instance() {
    use crate::definition::AmazonCollection;

    let builder = project_aws(vec![
        AmazonCollection::AmazonNetworkInterfaces(vec![attached_interface(
            "eni-stopped",
            "i-stopped",
        )]),
        AmazonCollection::AmazonInstances(Vec::new()),
    ]);

    assert!(
        builder
            .index_of(&Node::AwsEc2Instance("i-stopped".into()))
            .is_none(),
        "an attachment is not evidence the scan kept the instance"
    );
    assert_edge(
        &builder,
        &Node::AwsEc2Eni("eni-stopped".into()),
        &Node::ip("10.0.5.5", "vpc-globex"),
        &Edge::ConnectsTo,
    );
}

#[test]
fn an_interface_owns_its_attachment_once_the_scan_confirms_the_instance() {
    use crate::definition::AmazonCollection;

    let instance = aws_sdk_ec2::types::Instance::builder()
        .instance_id("i-running")
        .build();
    let builder = project_aws(vec![
        AmazonCollection::AmazonNetworkInterfaces(vec![attached_interface(
            "eni-running",
            "i-running",
        )]),
        AmazonCollection::AmazonInstances(vec![instance]),
    ]);

    assert_edge(
        &builder,
        &Node::AwsEc2Instance("i-running".into()),
        &Node::AwsEc2Eni("eni-running".into()),
        &Edge::HasIp,
    );
}

fn target_group(
    target_type: aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum,
    target: &str,
) -> GraphBuilder {
    target_group_scanning(target_type, target, Vec::new())
}

fn target_group_scanning(
    target_type: aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum,
    target: &str,
    running: Vec<&str>,
) -> GraphBuilder {
    target_group_with_interfaces(target_type, target, running, Vec::new())
}

fn target_group_with_interfaces(
    target_type: aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum,
    target: &str,
    running: Vec<&str>,
    interfaces: Vec<aws_sdk_ec2::types::NetworkInterface>,
) -> GraphBuilder {
    use crate::definition::{AWSLoadBalancing, AmazonCollection};
    use aws_sdk_elasticloadbalancingv2::types::{
        TargetDescription, TargetGroup, TargetHealthDescription,
    };

    let arn = "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/typed/1";
    let mut target_health = std::collections::HashMap::new();
    target_health.insert(
        arn.to_owned(),
        vec![
            TargetHealthDescription::builder()
                .target(TargetDescription::builder().id(target).build())
                .build(),
        ],
    );
    project_aws(vec![
        AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing {
            load_balancers: Vec::new(),
            target_groups: vec![
                TargetGroup::builder()
                    .target_group_arn(arn)
                    .target_type(target_type)
                    .vpc_id("vpc-globex")
                    .build(),
            ],
            listeners: Vec::new(),
            target_health,
        }),
        AmazonCollection::AmazonInstances(
            running
                .into_iter()
                .map(|id| {
                    aws_sdk_ec2::types::Instance::builder()
                        .instance_id(id)
                        .build()
                })
                .collect(),
        ),
        AmazonCollection::AmazonNetworkInterfaces(interfaces),
    ])
}

fn typed_target_group() -> Node {
    Node::AwsElbTargetGroup("arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/typed/1".into())
}

fn any_instance(builder: &GraphBuilder) -> bool {
    builder
        .graph
        .node_weights()
        .any(|n| matches!(n, Node::AwsEc2Instance(_)))
}

// An IP target group's targets are addresses. Building an instance from one
// invented a node, and the service chain then drew an edge to it.
#[test]
fn an_ip_target_is_an_address_the_group_routes_to() {
    use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

    let builder = target_group(TargetTypeEnum::Ip, "10.0.1.5");

    assert!(!any_instance(&builder));
    assert_edge(
        &builder,
        &typed_target_group(),
        &Node::ip("10.0.1.5", "vpc-globex"),
        &Edge::RoutesTo,
    );
    assert!(
        !builder.graph.edge_weights().any(|e| e == &Edge::ConnectsTo),
        "ConnectsTo to a pivot means holding it, which a target group does not"
    );
}

#[test]
fn an_ip_target_on_a_stopped_instances_interface_is_not_linked() {
    use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

    let builder = target_group_with_interfaces(
        TargetTypeEnum::Ip,
        "10.0.5.5",
        Vec::new(),
        vec![attached_interface("eni-stopped", "i-stopped")],
    );

    assert!(
        !builder.has_edge(
            &typed_target_group(),
            &Node::ip("10.0.5.5", "vpc-globex"),
            &Edge::RoutesTo
        ),
        "the registration names the stopped instance by its address, and the scan dropped it"
    );
    assert!(!any_instance(&builder));
}

#[test]
fn an_ip_target_on_a_running_instances_interface_is_linked() {
    use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

    let builder = target_group_with_interfaces(
        TargetTypeEnum::Ip,
        "10.0.5.5",
        vec!["i-running"],
        vec![attached_interface("eni-running", "i-running")],
    );

    assert_edge(
        &builder,
        &typed_target_group(),
        &Node::ip("10.0.5.5", "vpc-globex"),
        &Edge::RoutesTo,
    );
}

#[test]
fn an_ip_target_is_read_in_its_groups_vpc() {
    use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

    let builder = target_group_with_interfaces(
        TargetTypeEnum::Ip,
        "10.0.5.5",
        vec!["i-elsewhere"],
        vec![{
            let mut interface = attached_interface("eni-elsewhere", "i-elsewhere");
            interface.vpc_id = Some("vpc-elsewhere".to_owned());
            interface
        }],
    );

    assert_edge(
        &builder,
        &typed_target_group(),
        &Node::ip("10.0.5.5", "vpc-globex"),
        &Edge::RoutesTo,
    );
    assert!(!builder.has_edge(
        &typed_target_group(),
        &Node::ip("10.0.5.5", "vpc-elsewhere"),
        &Edge::RoutesTo
    ));
}

#[test]
fn a_lambda_target_invents_no_instance() {
    use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

    let builder = target_group(
        TargetTypeEnum::Lambda,
        "arn:aws:lambda:us-east-1:123:function:globex-handler",
    );

    assert!(!any_instance(&builder));
}

#[test]
fn a_running_instance_target_is_linked() {
    use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

    let builder = target_group_scanning(TargetTypeEnum::Instance, "i-target", vec!["i-target"]);

    assert_edge(
        &builder,
        &Node::AwsElbTargetGroup(
            "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/typed/1".into(),
        ),
        &Node::AwsEc2Instance("i-target".into()),
        &Edge::ConnectsTo,
    );
}

// A stopped instance stays registered, but DescribeInstances only returns
// running and pending ones. The registration must not bring it back.
#[test]
fn a_stopped_instance_target_invents_no_instance() {
    use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

    let builder = target_group_scanning(TargetTypeEnum::Instance, "i-stopped", Vec::new());

    assert!(!any_instance(&builder));
    assert!(
        builder.index_of(&typed_target_group()).is_some(),
        "the group itself is still real"
    );
}

#[test]
fn config_resources_respect_exclude_by_default() {
    use crate::definition::AmazonCollection;
    use std::collections::HashMap;

    let make_provider = || {
        let mut resources = HashMap::new();
        resources.insert(
            "AWS::Unknown::Widget".to_owned(),
            vec![
                aws_sdk_config::types::ResourceIdentifier::builder()
                    .resource_id("widget-1")
                    .build(),
            ],
        );
        vec![(
            atlas_provider::fixtures::REGION.to_owned(),
            AmazonCollection::AmazonResources(resources),
        )]
    };
    let widget = Node::AwsConfigResource {
        resource_type: "AWS::Unknown::Widget".into(),
        id: "widget-1".into(),
    };

    let include = Settings {
        regions: vec![atlas_provider::fixtures::REGION.to_owned()],
        ..Default::default()
    };
    let mut builder = GraphBuilder::new();
    aws_projector(&mut builder, &make_provider(), &include);
    assert_has_node(&builder, &widget);

    let exclude = Settings {
        regions: vec![atlas_provider::fixtures::REGION.to_owned()],
        exclude_by_default: true,
        ..Default::default()
    };
    let mut builder = GraphBuilder::new();
    aws_projector(&mut builder, &make_provider(), &exclude);
    assert!(
        !builder.node_map.contains_key(&widget),
        "unknown config resource should be excluded when exclude_by_default is set"
    );
}

// The chain is only as honest as the edges it walks. A stopped target keeps
// its registration and its interface, and neither may bring it back.
#[test]
fn a_stopped_target_is_not_inferred() {
    use crate::definition::AmazonCollection;

    let stopped = Node::AwsEc2Instance("i-globex-web-03".into());
    let collections = fixtures::collections();
    let without_web03: Vec<_> = collections
        .into_iter()
        .map(|(region, collection)| match collection {
            AmazonCollection::AmazonInstances(instances) => (
                region,
                AmazonCollection::AmazonInstances(
                    instances
                        .into_iter()
                        .filter(|i| i.instance_id() != Some("i-globex-web-03"))
                        .collect(),
                ),
            ),
            other => (region, other),
        })
        .collect();
    let mut builder = GraphBuilder::new();
    aws_projector(&mut builder, &without_web03, &Settings::default());
    derive::all(&mut builder, &FlowIndex::default());

    assert!(builder.index_of(&stopped).is_none());
    assert!(!serves_any(&builder, |_, target| target == &stopped));
}

fn overlapping_estate(target_running: bool) -> GraphBuilder {
    let mut running = vec!["i-other"];
    if target_running {
        running.push("i-target");
    }
    ip_target_estate(
        &[
            ("eni-target", "vpc-a", "i-target"),
            ("eni-other", "vpc-b", "i-other"),
        ],
        &running,
        None,
    )
}

fn peered_estate(target_running: bool) -> GraphBuilder {
    let running: &[&str] = if target_running { &["i-target"] } else { &[] };
    ip_target_estate(
        &[("eni-target", "vpc-b", "i-target")],
        running,
        Some(("vpc-a", "vpc-b")),
    )
}

fn ip_target_estate(
    holders: &[(&str, &str, &str)],
    running: &[&str],
    peering: Option<(&str, &str)>,
) -> GraphBuilder {
    use crate::definition::{AWSConnectivity, AWSLoadBalancing, AmazonCollection};
    use aws_sdk_ec2::types::{
        Instance, NetworkInterface, NetworkInterfaceAttachment, NetworkInterfacePrivateIpAddress,
        VpcPeeringConnection, VpcPeeringConnectionStateReason, VpcPeeringConnectionStateReasonCode,
        VpcPeeringConnectionVpcInfo,
    };
    use aws_sdk_elasticloadbalancingv2::types::{
        Action, Listener, LoadBalancer, TargetDescription, TargetGroup, TargetHealthDescription,
        TargetTypeEnum,
    };

    let lb_arn = "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/front/1";
    let tg_arn = "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/back/1";
    let interface = |id: &str, vpc: &str, instance: &str| {
        NetworkInterface::builder()
            .network_interface_id(id)
            .vpc_id(vpc)
            .subnet_id(format!("subnet-{vpc}"))
            .private_ip_addresses(
                NetworkInterfacePrivateIpAddress::builder()
                    .private_ip_address("10.0.1.5")
                    .build(),
            )
            .attachment(
                NetworkInterfaceAttachment::builder()
                    .instance_id(instance)
                    .build(),
            )
            .build()
    };
    let running = running
        .iter()
        .map(|id| Instance::builder().instance_id(*id).build())
        .collect();
    let side = |vpc: &str| VpcPeeringConnectionVpcInfo::builder().vpc_id(vpc).build();
    let peering_connections = peering
        .map(|(requester, accepter)| {
            VpcPeeringConnection::builder()
                .vpc_peering_connection_id("pcx-ab")
                .status(
                    VpcPeeringConnectionStateReason::builder()
                        .code(VpcPeeringConnectionStateReasonCode::Active)
                        .build(),
                )
                .requester_vpc_info(side(requester))
                .accepter_vpc_info(side(accepter))
                .build()
        })
        .into_iter()
        .collect();

    let mut builder = GraphBuilder::new();
    aws_projector(
        &mut builder,
        &[
            AmazonCollection::AmazonNetworkInterfaces(
                holders
                    .iter()
                    .map(|(id, vpc, instance)| interface(id, vpc, instance))
                    .collect(),
            ),
            AmazonCollection::AmazonInstances(running),
            AmazonCollection::AmazonConnectivity(AWSConnectivity {
                peering_connections,
                ..Default::default()
            }),
            AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing {
                load_balancers: vec![
                    LoadBalancer::builder()
                        .load_balancer_arn(lb_arn)
                        .vpc_id("vpc-a")
                        .build(),
                ],
                target_groups: vec![
                    TargetGroup::builder()
                        .target_group_arn(tg_arn)
                        .target_type(TargetTypeEnum::Ip)
                        .vpc_id("vpc-a")
                        .build(),
                ],
                listeners: vec![
                    Listener::builder()
                        .load_balancer_arn(lb_arn)
                        .default_actions(Action::builder().target_group_arn(tg_arn).build())
                        .build(),
                ],
                target_health: HashMap::from([(
                    tg_arn.to_owned(),
                    vec![
                        TargetHealthDescription::builder()
                            .target(TargetDescription::builder().id("10.0.1.5").build())
                            .build(),
                    ],
                )]),
            }),
        ]
        .into_iter()
        .map(|collection| ("us-east-1".to_owned(), collection))
        .collect::<Vec<_>>(),
        &Settings::default(),
    );
    derive::all(&mut builder, &FlowIndex::default());
    builder
}

fn front() -> Node {
    Node::AwsElbLoadBalancer(
        "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/front/1".into(),
    )
}

#[test]
fn an_ip_target_is_served_only_by_the_holder_in_its_own_network() {
    let builder = overlapping_estate(true);

    assert!(serves(
        &builder,
        &front(),
        &Node::AwsEc2Instance("i-target".into())
    ));
    assert!(!serves(
        &builder,
        &front(),
        &Node::AwsEc2Instance("i-other".into())
    ));
}

#[test]
fn a_stopped_ip_target_is_not_inferred() {
    let builder = overlapping_estate(false);

    assert!(
        !serves_any(&builder, |source, _| source == &front()),
        "the stopped instance's interface is not a service of its own"
    );
}

#[test]
fn an_ip_target_across_a_peering_is_served_by_its_running_holder() {
    let builder = peered_estate(true);

    assert!(serves(
        &builder,
        &front(),
        &Node::AwsEc2Instance("i-target".into())
    ));
}

#[test]
fn a_stopped_ip_target_across_a_peering_is_not_inferred() {
    let builder = peered_estate(false);

    assert!(
        !serves_any(&builder, |source, _| source == &front()),
        "the join must not hand the stopped instance's interface the address"
    );
}
