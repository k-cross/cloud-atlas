use super::{link_bgp_session, project_leaf, project_parallel};
use crate::Settings;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::derive::{self, HeldAddresses};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::{canonical_address, is_large_cidr};
use crate::cloud::definition::{
    AWSConnectivity, AWSDirectConnect, AWSDirectConnectGateways, AWSLoadBalancing, AWSNetworking,
    AWSRoute53, AWSTransitGatewayRouting, AmazonCollection, HostedZoneId,
};
use aws_sdk_directconnect::types::{
    BgpPeerState, ConnectionState, DirectConnectGateway, DirectConnectGatewayAssociationState,
    DirectConnectGatewayState, GatewayType, VirtualInterface, VirtualInterfaceState,
};
use aws_sdk_ec2::types::{
    AttachmentStatus, NetworkInterface, RouteState, TransitGateway, TransitGatewayAssociationState,
    TransitGatewayAttachmentResourceType, TransitGatewayAttachmentState,
    TransitGatewayPropagationState, TransitGatewayRouteState, TransitGatewayState, Vpc,
    VpcCidrBlockState, VpcCidrBlockStateCode, VpcPeeringConnectionStateReasonCode, VpnState,
};
use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;
use aws_sdk_route53::types::RrType;
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub fn aws_projector(
    builder: &mut GraphBuilder,
    aws_data: &[(String, AmazonCollection)],
    opts: &Settings,
) {
    let skipped_transit_gateways = skipped_transit_gateways(aws_data);
    project_parallel(builder, aws_data, |local, (region, collection)| {
        project_amazon_collection(local, region, collection, opts, &skipped_transit_gateways)
    });
    link_direct_connect_gateways(builder, aws_data);
    let scanned = scanned_instances(aws_data);
    link_interface_owners(builder, aws_data, &scanned);
    link_targets(builder, aws_data, &scanned);
}

// DescribeInstances is filtered to running/pending, but a *stopped* instance is
// still named elsewhere — its interfaces stay attached and its target-group
// registrations stay put. Any edge built from one of those names would create an
// instance node the scan deliberately left out: no AZ, tags or security groups,
// and indistinguishable in kind from a live one. Every such edge is therefore
// gated on this set, which is why they are built after the parallel pass.
fn scanned_instances(aws_data: &[(String, AmazonCollection)]) -> HashSet<&str> {
    aws_data
        .iter()
        .filter_map(|(_, collection)| match collection {
            AmazonCollection::AmazonInstances(instances) => Some(instances),
            _ => None,
        })
        .flatten()
        .filter_map(|instance| instance.instance_id())
        .collect()
}

// A registered target is only evidence the instance is wired up, not that it
// exists in the scan: a stopped instance stays registered, by id or by the
// address of its interface.
fn link_targets(
    builder: &mut GraphBuilder,
    aws_data: &[(String, AmazonCollection)],
    scanned: &HashSet<&str>,
) {
    let held_by_unscanned = addresses_of_unscanned_instances(aws_data, scanned);
    let held = derive::held_addresses(&builder.graph);
    let mut linked: Vec<(&str, Node, Edge)> = Vec::new();
    for (_, collection) in aws_data {
        let AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing {
            target_groups,
            target_health,
            ..
        }) = collection
        else {
            continue;
        };
        for tg in target_groups {
            let Some(arn) = tg.target_group_arn() else {
                continue;
            };
            let Some(health_descriptions) = target_health.get(arn) else {
                continue;
            };
            let targets = health_descriptions
                .iter()
                .filter_map(|h| h.target())
                .filter_map(|t| t.id());
            match tg.target_type() {
                Some(TargetTypeEnum::Instance) => linked.extend(
                    targets
                        .filter(|id| scanned.contains(id))
                        .map(|id| (arn, Node::AwsEc2Instance(id.into()), Edge::ConnectsTo)),
                ),
                // The group routes to this address; it does not hold it.
                // `ConnectsTo` to a pivot means holding, and would make the
                // group a Serves endpoint.
                Some(TargetTypeEnum::Ip) => {
                    let network = tg.vpc_id().unwrap_or(arn);
                    linked.extend(
                        targets
                            .map(|address| Node::ip(address, network))
                            .filter(|target| {
                                !resolves_to_unscanned(builder, &held, &held_by_unscanned, target)
                            })
                            .map(|target| (arn, target, Edge::RoutesTo)),
                    );
                }
                // A Lambda target is a function ARN and an ALB target a
                // balancer ARN. Neither is an instance, and an unknown type is
                // not evidence of one.
                _ => {}
            }
        }
    }
    for (arn, target, edge) in linked {
        let tg_idx = builder.get_or_add_node(Node::AwsElbTargetGroup(arn.into()));
        builder.link_to(tg_idx, target, edge);
    }
}

fn resolves_to_unscanned(
    builder: &GraphBuilder,
    held: &HeldAddresses,
    held_by_unscanned: &HashSet<Node>,
    target: &Node,
) -> bool {
    if held_by_unscanned.contains(target) {
        return true;
    }
    let Node::GenericPrivateIpAddress { network, address } = target else {
        return false;
    };
    held.holder(network, address)
        .is_some_and(|pivot| held_by_unscanned.contains(&builder.graph[pivot]))
}

fn addresses_of_unscanned_instances(
    aws_data: &[(String, AmazonCollection)],
    scanned: &HashSet<&str>,
) -> HashSet<Node> {
    aws_data
        .iter()
        .filter_map(|(_, collection)| match collection {
            AmazonCollection::AmazonNetworkInterfaces(interfaces) => Some(interfaces),
            _ => None,
        })
        .flatten()
        .filter(|eni| {
            eni.attachment()
                .and_then(|a| a.instance_id())
                .is_some_and(|instance_id| !scanned.contains(instance_id))
        })
        .filter_map(|eni| {
            let id = eni.network_interface_id()?;
            let network = eni.vpc_id().unwrap_or(id);
            Some(
                interface_addresses(eni)
                    .into_iter()
                    .map(move |address| Node::ip(address, network)),
            )
        })
        .flatten()
        .collect()
}

// The two owner edges an interface cannot decide on its own, because each needs
// another collection and projection runs every collection into its own builder.
//
// The balancer: an interface names it as "ELB app/<name>/<id>" and the ARN ends
// with that same triple, but neither collection carries the other's key.
//
// The instance: an attachment names one even when the scan left it out (see
// scanned_instances). The edge is emitted only for an instance the scan
// returned, which costs nothing: a running one reports its own interfaces and
// project_instance has already drawn it.
fn link_interface_owners(
    builder: &mut GraphBuilder,
    aws_data: &[(String, AmazonCollection)],
    scanned_instances: &HashSet<&str>,
) {
    let mut balancers: HashMap<&str, &str> = HashMap::new();
    for (_, collection) in aws_data {
        if let AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing { load_balancers, .. }) =
            collection
        {
            for arn in load_balancers
                .iter()
                .filter_map(|lb| lb.load_balancer_arn())
            {
                if let Some((_, suffix)) = arn.split_once("loadbalancer/") {
                    balancers.insert(suffix, arn);
                }
            }
        }
    }

    for (_, collection) in aws_data {
        if let AmazonCollection::AmazonNetworkInterfaces(interfaces) = collection {
            for eni in interfaces {
                let Some(eni_id) = eni.network_interface_id() else {
                    continue;
                };

                if let Some(suffix) = eni.description().and_then(|d| d.strip_prefix("ELB "))
                    && let Some(arn) = balancers.get(suffix)
                {
                    let lb_idx = builder.get_or_add_node(Node::AwsElbLoadBalancer((*arn).into()));
                    builder.link_to(lb_idx, Node::AwsEc2Eni(eni_id.into()), Edge::HasIp);
                }

                if let Some(instance_id) = eni.attachment().and_then(|a| a.instance_id())
                    && scanned_instances.contains(instance_id)
                {
                    let inst_idx =
                        builder.get_or_add_node(Node::AwsEc2Instance(instance_id.into()));
                    builder.link_to(inst_idx, Node::AwsEc2Eni(eni_id.into()), Edge::HasIp);
                }
            }
        }
    }
}

fn project_amazon_collection(
    builder: &mut GraphBuilder,
    region: &str,
    x: &AmazonCollection,
    opts: &Settings,
    skipped_transit_gateways: &HashSet<&str>,
) {
    let region_node = Node::AwsRegion(region.into());
    let region_idx = builder.get_or_add_node(region_node);

    match x {
        AmazonCollection::AmazonInstances(instance_data) => {
            for inst in instance_data {
                project_instance(
                    builder,
                    region_idx,
                    &InstanceFacts {
                        id: inst.instance_id.as_deref(),
                        vpc_id: inst.vpc_id.as_deref(),
                        subnet_id: inst.subnet_id.as_deref(),
                        availability_zone: inst
                            .placement
                            .as_ref()
                            .and_then(|p| p.availability_zone.as_deref()),
                        private_ip: inst.private_ip_address.as_deref(),
                        security_group_ids: inst
                            .security_groups()
                            .iter()
                            .filter_map(|sg| sg.group_id())
                            .collect(),
                        tags: inst
                            .tags
                            .as_deref()
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|tag| tag.key.as_deref().zip(tag.value.as_deref()))
                            .collect(),

                        network_interfaces: inst
                            .network_interfaces()
                            .iter()
                            .filter_map(|eni| {
                                Some(EniFacts {
                                    id: eni.network_interface_id()?,
                                    vpc_id: eni.vpc_id(),
                                    subnet_id: eni.subnet_id(),
                                    private_ip: eni.private_ip_address(),
                                })
                            })
                            .collect(),
                    },
                );
            }
        }
        AmazonCollection::AmazonResources(resource_map) => {
            for (res_name, rs) in resource_map {
                if use_aws_resource(res_name.as_str(), opts.exclude_by_default) {
                    for r in rs {
                        if let Some(id) = r.resource_id() {
                            let parent_idx = if use_global(res_name.as_str()) {
                                builder.get_or_add_node(Node::AwsRegion("global".into()))
                            } else {
                                region_idx
                            };
                            builder.link_to(
                                parent_idx,
                                Node::AwsConfigResource {
                                    resource_type: res_name.as_str().into(),
                                    id: id.into(),
                                },
                                Edge::Contains,
                            );
                        }
                    }
                }
            }
        }
        AmazonCollection::AmazonClusters(clusters) => {
            project_leaf!(
                builder,
                clusters,
                cluster_arn(),
                Node::AwsEcsCluster,
                region_idx,
                Edge::Contains
            )
        }
        AmazonCollection::AmazonLambdas(lambdas) => {
            for lambda in lambdas {
                if let Some(name) = lambda.function_name() {
                    let idx = builder.link_to(
                        region_idx,
                        Node::AwsLambdaFunction(name.into()),
                        Edge::Contains,
                    );

                    if let Some(role) = lambda.role() {
                        builder.link_to(idx, Node::AwsIamRole(role.into()), Edge::DependsOn);
                    }

                    if let Some(vpc_config) = lambda.vpc_config() {
                        for sg_id in vpc_config.security_group_ids() {
                            builder.link_to(
                                idx,
                                Node::AwsEc2SecurityGroup(sg_id.as_str().into()),
                                Edge::ConnectsTo,
                            );
                        }
                    }
                }
            }

            if opts.verbose {
                dbg!(&lambdas);
            }
        }
        AmazonCollection::AmazonEventbridge(buses) => {
            if opts.verbose {
                dbg!(&buses);
            }
        }
        AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing {
            load_balancers,
            target_groups,
            listeners,
            ..
        }) => {
            for lb in load_balancers {
                if let Some(arn) = lb.load_balancer_arn() {
                    let parent_idx = match lb.vpc_id() {
                        Some(vpc_id) => builder.get_or_add_node(Node::AwsEc2Vpc(vpc_id.into())),
                        None => region_idx,
                    };
                    builder.link_to(
                        parent_idx,
                        Node::AwsElbLoadBalancer(arn.into()),
                        Edge::Contains,
                    );
                }
            }

            for tg in target_groups {
                if let Some(arn) = tg.target_group_arn() {
                    let tg_idx = builder.get_or_add_node(Node::AwsElbTargetGroup(arn.into()));

                    if let Some(vpc_id) = tg.vpc_id() {
                        builder.link_from(tg_idx, Node::AwsEc2Vpc(vpc_id.into()), Edge::Contains);
                    }
                }
            }

            for listener in listeners {
                if let Some(lb_arn) = listener.load_balancer_arn() {
                    for tg_arn in listener
                        .default_actions()
                        .iter()
                        .filter_map(|a| a.target_group_arn())
                    {
                        let lb_idx =
                            builder.get_or_add_node(Node::AwsElbLoadBalancer(lb_arn.into()));
                        builder.link_to(
                            lb_idx,
                            Node::AwsElbTargetGroup(tg_arn.into()),
                            Edge::ConnectsTo,
                        );
                    }
                }
            }
        }
        AmazonCollection::AmazonRoute53(AWSRoute53 {
            hosted_zones,
            record_sets,
        }) => {
            let g_idx = builder.get_or_add_node(Node::AwsRegion("global".into()));

            for hz in hosted_zones {
                let zone = hz.id();
                let zone_idx = builder.link_to(
                    g_idx,
                    Node::AwsRoute53HostedZone(zone.into()),
                    Edge::Contains,
                );

                let Some(records) = record_sets.get(&HostedZoneId(zone.to_owned())) else {
                    continue;
                };
                for rs in records {
                    let rs_idx = builder.link_to(
                        zone_idx,
                        Node::AwsRoute53RecordSet {
                            zone: zone.into(),
                            name: rs.name().into(),
                            record_type: rs.r#type().as_str().into(),
                        },
                        Edge::Contains,
                    );
                    builder.link_from(rs_idx, Node::hostname(rs.name()), Edge::ResolvesTo);

                    for r in rs.resource_records() {
                        let target = match rs.r#type() {
                            RrType::A | RrType::Aaaa => Node::ip(r.value(), zone),
                            RrType::Cname => Node::hostname(r.value()),
                            _ => continue,
                        };
                        builder.link_to(rs_idx, target, Edge::ResolvesTo);
                    }
                }
            }
        }
        AmazonCollection::AmazonEks(clusters) => {
            for cluster in clusters {
                if let Some(name) = cluster.name() {
                    let vpc_config = cluster.resources_vpc_config();
                    let parent_idx = match vpc_config.and_then(|c| c.vpc_id()) {
                        Some(vpc_id) => builder.get_or_add_node(Node::AwsEc2Vpc(vpc_id.into())),
                        None => region_idx,
                    };
                    let idx = builder.link_to(
                        parent_idx,
                        Node::AwsEksCluster(name.into()),
                        Edge::Contains,
                    );

                    if let Some(vpc_config) = vpc_config {
                        for sg_id in vpc_config.security_group_ids() {
                            builder.link_to(
                                idx,
                                Node::AwsEc2SecurityGroup(sg_id.as_str().into()),
                                Edge::ConnectsTo,
                            );
                        }
                    }
                }
            }
        }
        AmazonCollection::AmazonApiGateway(apis) => {
            project_leaf!(
                builder,
                apis,
                id(),
                Node::AwsApiGatewayRestApi,
                region_idx,
                Edge::Contains
            )
        }
        AmazonCollection::AmazonRds(dbs) => {
            for db in dbs {
                if let Some(id) = db.db_instance_identifier() {
                    let parent_idx = match db.db_subnet_group().and_then(|g| g.vpc_id()) {
                        Some(vpc_id) => builder.get_or_add_node(Node::AwsEc2Vpc(vpc_id.into())),
                        None => region_idx,
                    };
                    let idx = builder.link_to(
                        parent_idx,
                        Node::AwsRdsDbInstance(id.into()),
                        Edge::Contains,
                    );

                    if let Some(address) = db.endpoint().and_then(|e| e.address()) {
                        builder.link_to(idx, Node::hostname(address), Edge::ConnectsTo);
                    }

                    for sg in db.vpc_security_groups() {
                        if let Some(sg_id) = sg.vpc_security_group_id() {
                            builder.link_to(
                                idx,
                                Node::AwsEc2SecurityGroup(sg_id.into()),
                                Edge::ConnectsTo,
                            );
                        }
                    }
                }
            }
        }
        AmazonCollection::AmazonDynamoDb(tables) => {
            for t in tables {
                builder.link_to(
                    region_idx,
                    Node::AwsDynamoDbTable(t.0.as_str().into()),
                    Edge::Contains,
                );
            }
        }
        AmazonCollection::AmazonSqs(queues) => {
            for q in queues {
                builder.link_to(
                    region_idx,
                    Node::AwsSqsQueue(q.0.as_str().into()),
                    Edge::Contains,
                );
            }
        }
        AmazonCollection::AmazonSns(topics) => {
            project_leaf!(
                builder,
                topics,
                topic_arn(),
                Node::AwsSnsTopic,
                region_idx,
                Edge::Contains
            )
        }
        AmazonCollection::AmazonCloudFront(dists) => {
            let g_idx = builder.get_or_add_node(Node::AwsRegion("global".into()));

            for d in dists {
                builder.link_to(
                    g_idx,
                    Node::AwsCloudFrontDistribution(d.id().into()),
                    Edge::Contains,
                );
            }
        }
        AmazonCollection::AmazonNetworkInterfaces(interfaces) => {
            for eni in interfaces {
                project_network_interface(builder, region_idx, eni);
            }
        }
        AmazonCollection::AmazonNetworking(AWSNetworking {
            route_tables,
            internet_gateways,
            nat_gateways,
            addresses,
        }) => {
            for addr in addresses {
                if let Some(alloc) = addr.allocation_id().or_else(|| addr.public_ip()) {
                    let eip_idx = builder.get_or_add_node(Node::AwsEc2Eip(alloc.into()));
                    if let Some(public_ip) = addr.public_ip() {
                        builder.link_to(eip_idx, Node::ip(public_ip, alloc), Edge::ConnectsTo);
                    }
                    // The interface is what holds an associated address, and
                    // through it whatever holds the interface. The instance id
                    // is deliberately not used: a stopped instance keeps its
                    // association, and the scan does not keep the instance.
                    if let Some(eni_id) = addr.network_interface_id() {
                        builder.link_from(eip_idx, Node::AwsEc2Eni(eni_id.into()), Edge::HasIp);
                    }
                }
            }

            for igw in internet_gateways {
                if let Some(igw_id) = igw.internet_gateway_id() {
                    let igw_idx =
                        builder.get_or_add_node(Node::AwsEc2InternetGateway(igw_id.into()));
                    for att in igw.attachments() {
                        if let Some(vpc_id) = att.vpc_id() {
                            builder.link_to(
                                igw_idx,
                                Node::AwsEc2Vpc(vpc_id.into()),
                                Edge::AttachedTo,
                            );
                        }
                    }
                }
            }

            for nat in nat_gateways {
                if let Some(nat_id) = nat.nat_gateway_id() {
                    let nat_idx = builder.get_or_add_node(Node::AwsEc2NatGateway(nat_id.into()));
                    if let Some(subnet_id) = nat.subnet_id() {
                        builder.link_to(
                            nat_idx,
                            Node::AwsEc2Subnet(subnet_id.into()),
                            Edge::AttachedTo,
                        );
                    }
                    for nat_addr in nat.nat_gateway_addresses() {
                        if let Some(alloc) =
                            nat_addr.allocation_id().or_else(|| nat_addr.public_ip())
                        {
                            builder.link_to(nat_idx, Node::AwsEc2Eip(alloc.into()), Edge::HasIp);
                        }
                    }
                }
            }

            for table in route_tables {
                if let Some(id) = table.route_table_id() {
                    project_route_table(
                        builder,
                        &RouteTableFacts {
                            id,
                            vpc_id: table.vpc_id(),
                            subnet_ids: table
                                .associations()
                                .iter()
                                .filter_map(|association| association.subnet_id())
                                .collect(),
                            routes: table
                                .routes()
                                .iter()
                                .filter_map(|route| {
                                    Some(RouteFacts {
                                        destination: route
                                            .destination_cidr_block()
                                            .or(route.destination_ipv6_cidr_block())
                                            .or(route.destination_prefix_list_id())?,
                                        active: route.state() != Some(&RouteState::Blackhole),
                                        gateway_id: route.gateway_id(),
                                        nat_gateway_id: route.nat_gateway_id(),
                                        transit_gateway_id: route.transit_gateway_id(),
                                        vpc_peering_connection_id: route
                                            .vpc_peering_connection_id(),
                                        network_interface_id: route.network_interface_id(),
                                    })
                                })
                                .collect(),
                        },
                    );
                }
            }
        }
        AmazonCollection::AmazonConnectivity(AWSConnectivity {
            vpcs,
            peering_connections,
            transit_gateways,
            transit_gateway_attachments,
            vpn_gateways,
            customer_gateways,
            vpn_connections,
        }) => {
            for vpc in vpcs {
                if let Some(id) = vpc.vpc_id() {
                    project_vpc(
                        builder,
                        region_idx,
                        &VpcFacts {
                            id,
                            address_space: vpc_address_space(vpc).collect(),
                        },
                    );
                }
            }

            for pcx in peering_connections.iter().filter(|pcx| {
                pcx.status().and_then(|status| status.code())
                    == Some(&VpcPeeringConnectionStateReasonCode::Active)
            }) {
                if let Some(id) = pcx.vpc_peering_connection_id() {
                    project_peering(
                        builder,
                        &PeeringFacts {
                            id,
                            vpc_ids: [pcx.requester_vpc_info(), pcx.accepter_vpc_info()]
                                .into_iter()
                                .flatten()
                                .filter_map(|side| side.vpc_id())
                                .collect(),
                        },
                    );
                }
            }

            for tgw_id in transit_gateways
                .iter()
                .filter(|tgw| transit_gateway_serves(tgw))
                .filter_map(|tgw| tgw.transit_gateway_id())
            {
                builder.link_to(
                    region_idx,
                    Node::AwsEc2TransitGateway(tgw_id.into()),
                    Edge::Contains,
                );
            }

            for attachment in transit_gateway_attachments.iter().filter(|attachment| {
                matches!(
                    attachment.state(),
                    Some(
                        TransitGatewayAttachmentState::Available
                            | TransitGatewayAttachmentState::Modifying
                    )
                ) && attachment.resource_type() == Some(&TransitGatewayAttachmentResourceType::Vpc)
            }) {
                if let (Some(tgw_id), Some(vpc_id)) =
                    (attachment.transit_gateway_id(), attachment.resource_id())
                {
                    let tgw_idx =
                        builder.get_or_add_node(Node::AwsEc2TransitGateway(tgw_id.into()));
                    builder.link_to(tgw_idx, Node::AwsEc2Vpc(vpc_id.into()), Edge::AttachedTo);
                }
            }

            for vgw in vpn_gateways
                .iter()
                .filter(|vgw| vgw.state() == Some(&VpnState::Available))
            {
                if let Some(id) = vgw.vpn_gateway_id() {
                    project_vpn_gateway(
                        builder,
                        region_idx,
                        &VpnGatewayFacts {
                            id,
                            attached_vpc_ids: vgw
                                .vpc_attachments()
                                .iter()
                                .filter(|attachment| {
                                    attachment.state() == Some(&AttachmentStatus::Attached)
                                })
                                .filter_map(|attachment| attachment.vpc_id())
                                .collect(),
                        },
                    );
                }
            }

            for cgw in customer_gateways
                .iter()
                .filter(|cgw| cgw.state() == Some("available"))
            {
                if let Some(id) = cgw.customer_gateway_id() {
                    project_customer_gateway(
                        builder,
                        region_idx,
                        &CustomerGatewayFacts {
                            id,
                            ip_address: cgw.ip_address(),
                        },
                    );
                }
            }

            for connection in vpn_connections
                .iter()
                .filter(|connection| connection.state() == Some(&VpnState::Available))
            {
                let Some(id) = connection.vpn_connection_id() else {
                    continue;
                };
                let configured = connection
                    .options()
                    .into_iter()
                    .flat_map(|options| options.tunnel_options())
                    .filter_map(|tunnel| tunnel.outside_ip_address());
                let reported = connection
                    .vgw_telemetry()
                    .iter()
                    .filter_map(|telemetry| telemetry.outside_ip_address());
                project_vpn_connection(
                    builder,
                    region_idx,
                    &VpnConnectionFacts {
                        id,
                        vpn_gateway_id: connection.vpn_gateway_id(),
                        transit_gateway_id: connection.transit_gateway_id(),
                        customer_gateway_id: connection.customer_gateway_id(),
                        outside_addresses: configured.chain(reported).collect(),
                    },
                );
            }
        }
        AmazonCollection::AmazonTransitGatewayRouting(AWSTransitGatewayRouting {
            route_tables,
            peerings,
        }) => {
            for routing in route_tables {
                let (Some(table_id), Some(tgw_id)) = (
                    routing.table.transit_gateway_route_table_id(),
                    routing
                        .table
                        .transit_gateway_id()
                        .filter(|id| !skipped_transit_gateways.contains(id)),
                ) else {
                    continue;
                };
                let tgw_idx = builder.get_or_add_node(Node::AwsEc2TransitGateway(tgw_id.into()));
                let table_idx = builder.link_to(
                    tgw_idx,
                    Node::AwsEc2TransitGatewayRouteTable(table_id.into()),
                    Edge::Contains,
                );
                for association in routing.associations.iter().filter(|association| {
                    matches!(
                        association.state(),
                        Some(
                            TransitGatewayAssociationState::Associated
                                | TransitGatewayAssociationState::Disassociating
                        )
                    )
                }) {
                    if let Some(resource) = attached_resource(
                        association.resource_type(),
                        association.resource_id(),
                        association.transit_gateway_attachment_id(),
                    ) {
                        builder.link_from(table_idx, resource, Edge::AttachedTo);
                    }
                }
                for propagation in routing.propagations.iter().filter(|propagation| {
                    matches!(
                        propagation.state(),
                        Some(
                            TransitGatewayPropagationState::Enabled
                                | TransitGatewayPropagationState::Disabling
                        )
                    )
                }) {
                    if let Some(resource) = attached_resource(
                        propagation.resource_type(),
                        propagation.resource_id(),
                        propagation.transit_gateway_attachment_id(),
                    ) {
                        builder.link_to(table_idx, resource, Edge::RoutesTo);
                    }
                }
                for route in &routing.static_routes {
                    let Some(destination) =
                        route.destination_cidr_block().or(route.prefix_list_id())
                    else {
                        continue;
                    };
                    let canonical = canonical_address(destination);
                    let route_idx = builder.link_to(
                        table_idx,
                        Node::AwsEc2TransitGatewayRoute {
                            table: table_id.into(),
                            destination: canonical.as_deref().unwrap_or(destination).into(),
                        },
                        Edge::Contains,
                    );
                    if route.state() != Some(&TransitGatewayRouteState::Active) {
                        continue;
                    }
                    for attachment in route.transit_gateway_attachments() {
                        if let Some(resource) = attached_resource(
                            attachment.resource_type(),
                            attachment.resource_id(),
                            attachment.transit_gateway_attachment_id(),
                        ) {
                            builder.link_to(route_idx, resource, Edge::RoutesTo);
                        }
                    }
                }
            }

            for peering in peerings.iter().filter(|peering| {
                matches!(
                    peering.state(),
                    Some(
                        TransitGatewayAttachmentState::Available
                            | TransitGatewayAttachmentState::Modifying
                    )
                )
            }) {
                let Some(attachment_id) = peering.transit_gateway_attachment_id() else {
                    continue;
                };
                let peering_idx = builder.link_to(
                    region_idx,
                    Node::AwsEc2TransitGatewayPeeringAttachment(attachment_id.into()),
                    Edge::Contains,
                );
                for tgw_id in [peering.requester_tgw_info(), peering.accepter_tgw_info()]
                    .into_iter()
                    .flatten()
                    .filter_map(|side| side.transit_gateway_id())
                    .filter(|id| !skipped_transit_gateways.contains(id))
                {
                    builder.link_to(
                        peering_idx,
                        Node::AwsEc2TransitGateway(tgw_id.into()),
                        Edge::AttachedTo,
                    );
                }
            }
        }
        AmazonCollection::AmazonDirectConnect(AWSDirectConnect {
            connections,
            virtual_interfaces,
        }) => {
            let mut drawn_connections = HashSet::new();
            for connection_id in connections
                .iter()
                .filter(|connection| {
                    matches!(
                        connection.connection_state(),
                        Some(ConnectionState::Available | ConnectionState::Down)
                    )
                })
                .filter_map(|connection| connection.connection_id())
            {
                builder.link_to(
                    region_idx,
                    Node::AwsDirectConnectConnection(connection_id.into()),
                    Edge::Contains,
                );
                drawn_connections.insert(connection_id);
            }

            for interface in virtual_interfaces
                .iter()
                .filter(|interface| drawn_interface(interface))
            {
                let Some(interface_id) = interface.virtual_interface_id() else {
                    continue;
                };
                let interface_idx = builder.link_to(
                    region_idx,
                    Node::AwsDirectConnectVirtualInterface(interface_id.into()),
                    Edge::Contains,
                );
                if let Some(connection_id) = interface
                    .connection_id()
                    .filter(|id| drawn_connections.contains(id))
                {
                    builder.link_to(
                        interface_idx,
                        Node::AwsDirectConnectConnection(connection_id.into()),
                        Edge::AttachedTo,
                    );
                }
                if let Some(vgw_id) = interface.virtual_gateway_id().filter(|id| !id.is_empty()) {
                    builder.link_to(
                        interface_idx,
                        Node::AwsEc2VpnGateway(vgw_id.into()),
                        Edge::AttachedTo,
                    );
                }
                for (amazon, customer) in bgp_sessions(interface) {
                    if let (Some(amazon), Some(customer)) =
                        (Node::link_ip(amazon), Node::link_ip(customer))
                    {
                        link_bgp_session(builder, interface_idx, amazon, customer);
                    }
                }
            }
        }
        AmazonCollection::AmazonDirectConnectGateways(AWSDirectConnectGateways {
            gateways,
            associations,
        }) => {
            let global_idx = builder.get_or_add_node(Node::AwsRegion("global".into()));
            let skipped: HashSet<_> = skipped_direct_connect_gateways(gateways).collect();
            for gateway_id in gateways
                .iter()
                .filter_map(|gateway| gateway.direct_connect_gateway_id())
                .filter(|id| !skipped.contains(id))
            {
                builder.link_to(
                    global_idx,
                    Node::AwsDirectConnectGateway(gateway_id.into()),
                    Edge::Contains,
                );
            }

            for association in associations.iter().filter(|association| {
                matches!(
                    association.association_state(),
                    Some(
                        DirectConnectGatewayAssociationState::Associated
                            | DirectConnectGatewayAssociationState::Updating
                    )
                )
            }) {
                let Some(gateway_id) = association
                    .direct_connect_gateway_id()
                    .filter(|id| !skipped.contains(id))
                else {
                    continue;
                };
                let Some(associated) = association.associated_gateway() else {
                    continue;
                };
                let associated = match (associated.r#type(), associated.id()) {
                    (Some(GatewayType::VirtualPrivateGateway), Some(id)) => {
                        Node::AwsEc2VpnGateway(id.into())
                    }
                    (Some(GatewayType::TransitGateway), Some(id)) => {
                        Node::AwsEc2TransitGateway(id.into())
                    }
                    _ => continue,
                };
                let associated_idx = builder.get_or_add_node(associated);
                builder.link_to(
                    associated_idx,
                    Node::AwsDirectConnectGateway(gateway_id.into()),
                    Edge::AttachedTo,
                );
            }
        }
        AmazonCollection::AmazonSecurityGroups(groups) => {
            for sg in groups {
                if let Some(id) = sg.group_id() {
                    let idx = builder.link_to(
                        region_idx,
                        Node::AwsEc2SecurityGroup(id.into()),
                        Edge::Contains,
                    );

                    for perm in sg.ip_permissions() {
                        for pair in perm.user_id_group_pairs() {
                            if let Some(referenced_group_id) = pair.group_id() {
                                builder.link_from(
                                    idx,
                                    Node::AwsEc2SecurityGroup(referenced_group_id.into()),
                                    Edge::ConnectsTo,
                                );
                            }
                        }
                    }

                    let network = sg.vpc_id().unwrap_or(id);
                    for perm in sg.ip_permissions_egress() {
                        for ip_range in perm.ip_ranges() {
                            if let Some(cidr) = ip_range.cidr_ip()
                                && !is_large_cidr(cidr)
                            {
                                builder.link_to(idx, Node::ip(cidr, network), Edge::RoutesTo);
                            }
                        }
                        for ipv6_range in perm.ipv6_ranges() {
                            if let Some(cidr) = ipv6_range.cidr_ipv6()
                                && !is_large_cidr(cidr)
                            {
                                builder.link_to(idx, Node::ip(cidr, network), Edge::RoutesTo);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn link_direct_connect_gateways(
    builder: &mut GraphBuilder,
    aws_data: &[(String, AmazonCollection)],
) {
    let skipped: HashSet<&str> = aws_data
        .iter()
        .filter_map(|(_, collection)| match collection {
            AmazonCollection::AmazonDirectConnectGateways(AWSDirectConnectGateways {
                gateways,
                ..
            }) => Some(gateways),
            _ => None,
        })
        .flat_map(|gateways| skipped_direct_connect_gateways(gateways))
        .collect();

    for (_, collection) in aws_data {
        let AmazonCollection::AmazonDirectConnect(AWSDirectConnect {
            virtual_interfaces, ..
        }) = collection
        else {
            continue;
        };
        for interface in virtual_interfaces
            .iter()
            .filter(|interface| drawn_interface(interface))
        {
            let (Some(interface_id), Some(gateway_id)) = (
                interface.virtual_interface_id(),
                interface
                    .direct_connect_gateway_id()
                    .filter(|id| !id.is_empty() && !skipped.contains(id)),
            ) else {
                continue;
            };
            let interface_idx = builder
                .get_or_add_node(Node::AwsDirectConnectVirtualInterface(interface_id.into()));
            builder.link_to(
                interface_idx,
                Node::AwsDirectConnectGateway(gateway_id.into()),
                Edge::AttachedTo,
            );
        }
    }
}

fn drawn_interface(interface: &VirtualInterface) -> bool {
    matches!(
        interface.virtual_interface_type(),
        Some("private" | "transit")
    ) && matches!(
        interface.virtual_interface_state(),
        Some(
            VirtualInterfaceState::Available
                | VirtualInterfaceState::Down
                | VirtualInterfaceState::Testing
        )
    )
}

fn transit_gateway_serves(tgw: &TransitGateway) -> bool {
    matches!(
        tgw.state(),
        Some(TransitGatewayState::Available | TransitGatewayState::Modifying)
    )
}

fn skipped_transit_gateways(aws_data: &[(String, AmazonCollection)]) -> HashSet<&str> {
    aws_data
        .iter()
        .filter_map(|(_, collection)| match collection {
            AmazonCollection::AmazonConnectivity(AWSConnectivity {
                transit_gateways, ..
            }) => Some(transit_gateways),
            _ => None,
        })
        .flatten()
        .filter(|tgw| !transit_gateway_serves(tgw))
        .filter_map(|tgw| tgw.transit_gateway_id())
        .collect()
}

fn skipped_direct_connect_gateways(
    gateways: &[DirectConnectGateway],
) -> impl Iterator<Item = &str> {
    gateways
        .iter()
        .filter(|gateway| {
            gateway.direct_connect_gateway_state() != Some(&DirectConnectGatewayState::Available)
        })
        .filter_map(|gateway| gateway.direct_connect_gateway_id())
}

fn attached_resource(
    kind: Option<&TransitGatewayAttachmentResourceType>,
    resource_id: Option<&str>,
    attachment_id: Option<&str>,
) -> Option<Node> {
    match kind? {
        TransitGatewayAttachmentResourceType::Vpc => Some(Node::AwsEc2Vpc(resource_id?.into())),
        TransitGatewayAttachmentResourceType::Vpn => {
            Some(Node::AwsEc2VpnConnection(resource_id?.into()))
        }
        TransitGatewayAttachmentResourceType::DirectConnectGateway => {
            Some(Node::AwsDirectConnectGateway(resource_id?.into()))
        }
        TransitGatewayAttachmentResourceType::Peering => Some(
            Node::AwsEc2TransitGatewayPeeringAttachment(attachment_id?.into()),
        ),
        _ => None,
    }
}

fn bgp_sessions(interface: &VirtualInterface) -> Vec<(&str, &str)> {
    let peers = interface.bgp_peers();
    if peers.is_empty() {
        return interface
            .amazon_address()
            .zip(interface.customer_address())
            .into_iter()
            .collect();
    }
    peers
        .iter()
        .filter(|peer| peer.bgp_peer_state() == Some(&BgpPeerState::Available))
        .filter_map(|peer| peer.amazon_address().zip(peer.customer_address()))
        .collect()
}

pub(crate) struct PeeringFacts<'a> {
    pub id: &'a str,
    pub vpc_ids: Vec<&'a str>,
}

pub(crate) fn project_peering(builder: &mut GraphBuilder, facts: &PeeringFacts) {
    let pcx_idx = builder.get_or_add_node(Node::AwsEc2VpcPeeringConnection(facts.id.into()));
    for vpc_id in &facts.vpc_ids {
        builder.link_to(pcx_idx, Node::AwsEc2Vpc((*vpc_id).into()), Edge::AttachedTo);
    }
}

pub(crate) struct VpnGatewayFacts<'a> {
    pub id: &'a str,
    pub attached_vpc_ids: Vec<&'a str>,
}

pub(crate) fn project_vpn_gateway(
    builder: &mut GraphBuilder,
    region_idx: NodeIndex,
    facts: &VpnGatewayFacts,
) {
    let vgw_idx = builder.link_to(
        region_idx,
        Node::AwsEc2VpnGateway(facts.id.into()),
        Edge::Contains,
    );
    for vpc_id in &facts.attached_vpc_ids {
        builder.link_to(vgw_idx, Node::AwsEc2Vpc((*vpc_id).into()), Edge::AttachedTo);
    }
}

pub(crate) struct CustomerGatewayFacts<'a> {
    pub id: &'a str,
    pub ip_address: Option<&'a str>,
}

pub(crate) fn project_customer_gateway(
    builder: &mut GraphBuilder,
    region_idx: NodeIndex,
    facts: &CustomerGatewayFacts,
) {
    let cgw_idx = builder.link_to(
        region_idx,
        Node::AwsEc2CustomerGateway(facts.id.into()),
        Edge::Contains,
    );
    if let Some(address) = facts.ip_address {
        builder.link_to(cgw_idx, Node::ip(address, facts.id), Edge::RoutesTo);
    }
}

pub(crate) struct VpnConnectionFacts<'a> {
    pub id: &'a str,
    pub vpn_gateway_id: Option<&'a str>,
    pub transit_gateway_id: Option<&'a str>,
    pub customer_gateway_id: Option<&'a str>,
    pub outside_addresses: Vec<&'a str>,
}

pub(crate) fn project_vpn_connection(
    builder: &mut GraphBuilder,
    region_idx: NodeIndex,
    facts: &VpnConnectionFacts,
) {
    let connection_idx = builder.link_to(
        region_idx,
        Node::AwsEc2VpnConnection(facts.id.into()),
        Edge::Contains,
    );
    if let Some(id) = facts.vpn_gateway_id {
        builder.link_to(
            connection_idx,
            Node::AwsEc2VpnGateway(id.into()),
            Edge::AttachedTo,
        );
    }
    if let Some(id) = facts.transit_gateway_id {
        builder.link_to(
            connection_idx,
            Node::AwsEc2TransitGateway(id.into()),
            Edge::AttachedTo,
        );
    }
    if let Some(id) = facts.customer_gateway_id {
        builder.link_to(
            connection_idx,
            Node::AwsEc2CustomerGateway(id.into()),
            Edge::AttachedTo,
        );
    }
    for address in &facts.outside_addresses {
        builder.link_to(
            connection_idx,
            Node::ip(address, facts.id),
            Edge::ConnectsTo,
        );
    }
}

pub(crate) struct RouteTableFacts<'a> {
    pub id: &'a str,
    pub vpc_id: Option<&'a str>,
    pub subnet_ids: Vec<&'a str>,
    pub routes: Vec<RouteFacts<'a>>,
}

pub(crate) struct RouteFacts<'a> {
    pub destination: &'a str,
    pub active: bool,
    pub gateway_id: Option<&'a str>,
    pub nat_gateway_id: Option<&'a str>,
    pub transit_gateway_id: Option<&'a str>,
    pub vpc_peering_connection_id: Option<&'a str>,
    pub network_interface_id: Option<&'a str>,
}

impl RouteFacts<'_> {
    fn target(&self, vpc_id: Option<&str>) -> Option<Node> {
        let gateway = self.gateway_id.filter(|id| !id.is_empty());
        if gateway == Some("local") {
            return vpc_id.map(|id| Node::AwsEc2Vpc(id.into()));
        }
        if let Some(id) = self.nat_gateway_id {
            return Some(Node::AwsEc2NatGateway(id.into()));
        }
        if let Some(id) = self.vpc_peering_connection_id {
            return Some(Node::AwsEc2VpcPeeringConnection(id.into()));
        }
        if let Some(id) = self.transit_gateway_id {
            return Some(Node::AwsEc2TransitGateway(id.into()));
        }
        if let Some(id) = self.network_interface_id {
            return Some(Node::AwsEc2Eni(id.into()));
        }
        match gateway {
            Some(id) if id.starts_with("igw-") => Some(Node::AwsEc2InternetGateway(id.into())),
            Some(id) if id.starts_with("vgw-") => Some(Node::AwsEc2VpnGateway(id.into())),
            _ => None,
        }
    }
}

pub(crate) fn project_route_table(builder: &mut GraphBuilder, facts: &RouteTableFacts) {
    let table_idx = builder.get_or_add_node(Node::AwsEc2RouteTable(facts.id.into()));
    if let Some(vpc_id) = facts.vpc_id {
        builder.link_from(table_idx, Node::AwsEc2Vpc(vpc_id.into()), Edge::Contains);
    }
    for subnet_id in &facts.subnet_ids {
        builder.link_from(
            table_idx,
            Node::AwsEc2Subnet((*subnet_id).into()),
            Edge::AttachedTo,
        );
    }
    for route in &facts.routes {
        let destination = canonical_address(route.destination);
        let route_idx = builder.link_to(
            table_idx,
            Node::AwsEc2Route {
                table: facts.id.into(),
                destination: destination.as_deref().unwrap_or(route.destination).into(),
            },
            Edge::Contains,
        );
        if route.active
            && let Some(target) = route.target(facts.vpc_id)
        {
            builder.link_to(route_idx, target, Edge::RoutesTo);
        }
    }
}

pub(crate) struct VpcFacts<'a> {
    pub id: &'a str,
    pub address_space: Vec<&'a str>,
}

pub(crate) fn project_vpc(builder: &mut GraphBuilder, region_idx: NodeIndex, facts: &VpcFacts) {
    let vpc_idx = builder.link_to(region_idx, Node::AwsEc2Vpc(facts.id.into()), Edge::Contains);
    for range in &facts.address_space {
        builder.link_to(vpc_idx, Node::ip(range, facts.id), Edge::ConnectsTo);
    }
}

fn vpc_address_space(vpc: &Vpc) -> impl Iterator<Item = &str> {
    let associated = |state: Option<&VpcCidrBlockState>| {
        state.and_then(|state| state.state()) == Some(&VpcCidrBlockStateCode::Associated)
    };
    let ipv4 = vpc
        .cidr_block_association_set()
        .iter()
        .filter(move |association| associated(association.cidr_block_state()))
        .filter_map(|association| association.cidr_block());
    let ipv6 = vpc
        .ipv6_cidr_block_association_set()
        .iter()
        .filter(move |association| associated(association.ipv6_cidr_block_state()))
        .filter_map(|association| association.ipv6_cidr_block());
    vpc.cidr_block().into_iter().chain(ipv4).chain(ipv6)
}

pub(crate) struct InstanceFacts<'a> {
    pub id: Option<&'a str>,
    pub vpc_id: Option<&'a str>,
    pub subnet_id: Option<&'a str>,
    pub availability_zone: Option<&'a str>,
    pub private_ip: Option<&'a str>,
    pub security_group_ids: Vec<&'a str>,
    pub tags: Vec<(&'a str, &'a str)>,
    pub network_interfaces: Vec<EniFacts<'a>>,
}

pub(crate) struct EniFacts<'a> {
    pub id: &'a str,
    pub vpc_id: Option<&'a str>,
    pub subnet_id: Option<&'a str>,
    pub private_ip: Option<&'a str>,
}

pub(crate) struct InterfaceFacts<'a> {
    pub id: &'a str,
    pub vpc_id: Option<&'a str>,
    pub subnet_id: Option<&'a str>,
    pub description: Option<&'a str>,
    pub addresses: Vec<&'a str>,
    pub security_group_ids: Vec<&'a str>,
}

fn interface_addresses(eni: &NetworkInterface) -> Vec<&str> {
    let mut addresses = Vec::new();
    for ip in eni.private_ip_addresses() {
        addresses.extend(ip.private_ip_address());
        addresses.extend(ip.association().and_then(|a| a.public_ip()));
    }
    addresses.extend(eni.private_ip_address());
    addresses.extend(eni.association().and_then(|a| a.public_ip()));
    addresses.extend(eni.ipv6_addresses().iter().filter_map(|a| a.ipv6_address()));
    addresses
}

// The only owner an interface names unambiguously in its own description. ELB
// needs the balancer collection too (see link_interface_owners), and
// Lambda, VPC endpoint and RDS interfaces are left unowned rather than guessed.
fn nat_gateway_of(description: Option<&str>) -> Option<&str> {
    let id = description?.strip_prefix("Interface for NAT Gateway ")?;
    id.starts_with("nat-").then_some(id)
}

fn project_network_interface(
    builder: &mut GraphBuilder,
    region_idx: NodeIndex,
    eni: &NetworkInterface,
) {
    let Some(id) = eni.network_interface_id() else {
        return;
    };
    project_interface(
        builder,
        region_idx,
        &InterfaceFacts {
            id,
            vpc_id: eni.vpc_id(),
            subnet_id: eni.subnet_id(),
            description: eni.description(),
            addresses: interface_addresses(eni),
            security_group_ids: eni.groups().iter().filter_map(|g| g.group_id()).collect(),
        },
    );
}

pub(crate) fn project_interface(
    builder: &mut GraphBuilder,
    region_idx: NodeIndex,
    facts: &InterfaceFacts<'_>,
) {
    let eni_idx = builder.get_or_add_node(Node::AwsEc2Eni(facts.id.into()));

    if let Some(subnet_id) = facts.subnet_id {
        let parent = facts.vpc_id.map(|vpc_id| {
            builder.link_to(region_idx, Node::AwsEc2Vpc(vpc_id.into()), Edge::Contains)
        });
        let subnet_idx =
            builder.link_to(parent, Node::AwsEc2Subnet(subnet_id.into()), Edge::Contains);
        builder.add_edge(eni_idx, subnet_idx, Edge::AttachedTo);
    }

    let network = facts.vpc_id.unwrap_or(facts.id);
    for address in &facts.addresses {
        builder.link_to(eni_idx, Node::ip(address, network), Edge::ConnectsTo);
    }

    for group_id in &facts.security_group_ids {
        builder.link_to(
            eni_idx,
            Node::AwsEc2SecurityGroup((*group_id).into()),
            Edge::ConnectsTo,
        );
    }

    if let Some(nat_id) = nat_gateway_of(facts.description) {
        builder.link_from(eni_idx, Node::AwsEc2NatGateway(nat_id.into()), Edge::HasIp);
    }
}

pub(crate) fn project_instance(
    builder: &mut GraphBuilder,
    region_idx: NodeIndex,
    facts: &InstanceFacts<'_>,
) {
    let vpc_idx = facts
        .vpc_id
        .map(|vpc_id| builder.link_to(region_idx, Node::AwsEc2Vpc(vpc_id.into()), Edge::Contains));

    let subnet_idx = facts.subnet_id.map(|subnet_id| {
        builder.link_to(
            vpc_idx,
            Node::AwsEc2Subnet(subnet_id.into()),
            Edge::Contains,
        )
    });

    let mut inst_idx = None;
    if let Some(instance_id) = facts.id {
        let idx = builder.get_or_add_node(Node::AwsEc2Instance(instance_id.into()));
        inst_idx = Some(idx);

        for eni in &facts.network_interfaces {
            let eni_idx = builder.link_to(idx, Node::AwsEc2Eni(eni.id.into()), Edge::HasIp);

            let attached = match eni.subnet_id {
                Some(subnet_id) if Some(subnet_id) != facts.subnet_id => Some(builder.link_to(
                    vpc_idx,
                    Node::AwsEc2Subnet(subnet_id.into()),
                    Edge::Contains,
                )),
                Some(_) => subnet_idx,
                None => None,
            };
            if let Some(subnet_idx) = attached {
                builder.add_edge(eni_idx, subnet_idx, Edge::AttachedTo);
            }
            if let Some(private_ip) = eni.private_ip {
                let network = eni.vpc_id.or(facts.vpc_id).unwrap_or(eni.id);
                builder.link_to(eni_idx, Node::ip(private_ip, network), Edge::ConnectsTo);
            }
        }

        if facts.network_interfaces.is_empty()
            && let Some(private_ip) = facts.private_ip
        {
            let network = facts.vpc_id.unwrap_or(instance_id);
            builder.link_to(idx, Node::ip(private_ip, network), Edge::ConnectsTo);
        }
    }

    if let Some(az_name) = facts.availability_zone {
        builder.link_from(
            inst_idx,
            Node::AwsEc2AvailabilityZone(az_name.into()),
            Edge::Contains,
        );
    }

    for (key, value) in &facts.tags {
        builder.link_to(
            inst_idx,
            Node::AwsTag {
                key: (*key).into(),
                value: (*value).into(),
            },
            Edge::DependsOn,
        );
    }

    for sg_id in &facts.security_group_ids {
        builder.link_to(
            inst_idx,
            Node::AwsEc2SecurityGroup((*sg_id).into()),
            Edge::ConnectsTo,
        );
    }
}

pub(crate) enum ConfigKey {
    ResourceId,
    ResourceName,
    Arn,
}

pub(crate) struct TypedConfigType {
    pub(crate) resource_type: &'static str,
    pub(crate) node: fn(Arc<str>) -> Node,
    pub(crate) event_key: Option<ConfigKey>,
}

const fn typed(
    resource_type: &'static str,
    node: fn(Arc<str>) -> Node,
    event_key: Option<ConfigKey>,
) -> TypedConfigType {
    TypedConfigType {
        resource_type,
        node,
        event_key,
    }
}

pub(crate) const TYPED_CONFIG_TYPES: &[TypedConfigType] = &[
    typed(
        "AWS::EC2::Instance",
        Node::AwsEc2Instance,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::VPC",
        Node::AwsEc2Vpc,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::Subnet",
        Node::AwsEc2Subnet,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::SecurityGroup",
        Node::AwsEc2SecurityGroup,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::RouteTable",
        Node::AwsEc2RouteTable,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::InternetGateway",
        Node::AwsEc2InternetGateway,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::NatGateway",
        Node::AwsEc2NatGateway,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::NetworkInterface",
        Node::AwsEc2Eni,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::EIP",
        Node::AwsEc2Eip,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::VPCPeeringConnection",
        Node::AwsEc2VpcPeeringConnection,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::TransitGateway",
        Node::AwsEc2TransitGateway,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::TransitGatewayRouteTable",
        Node::AwsEc2TransitGatewayRouteTable,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::VPNGateway",
        Node::AwsEc2VpnGateway,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::CustomerGateway",
        Node::AwsEc2CustomerGateway,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::EC2::VPNConnection",
        Node::AwsEc2VpnConnection,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::Lambda::Function",
        Node::AwsLambdaFunction,
        Some(ConfigKey::ResourceName),
    ),
    typed("AWS::IAM::Role", Node::AwsIamRole, None),
    typed(
        "AWS::ECS::Cluster",
        Node::AwsEcsCluster,
        Some(ConfigKey::Arn),
    ),
    typed(
        "AWS::EKS::Cluster",
        Node::AwsEksCluster,
        Some(ConfigKey::ResourceName),
    ),
    typed(
        "AWS::RDS::DBInstance",
        Node::AwsRdsDbInstance,
        Some(ConfigKey::ResourceName),
    ),
    typed(
        "AWS::DynamoDB::Table",
        Node::AwsDynamoDbTable,
        Some(ConfigKey::ResourceName),
    ),
    typed("AWS::SQS::Queue", Node::AwsSqsQueue, None),
    typed("AWS::SNS::Topic", Node::AwsSnsTopic, Some(ConfigKey::Arn)),
    typed(
        "AWS::ElasticLoadBalancingV2::LoadBalancer",
        Node::AwsElbLoadBalancer,
        Some(ConfigKey::Arn),
    ),
    typed(
        "AWS::ElasticLoadBalancingV2::TargetGroup",
        Node::AwsElbTargetGroup,
        None,
    ),
    typed("AWS::Route53::HostedZone", Node::AwsRoute53HostedZone, None),
    typed(
        "AWS::ApiGateway::RestApi",
        Node::AwsApiGatewayRestApi,
        Some(ConfigKey::ResourceId),
    ),
    typed(
        "AWS::CloudFront::Distribution",
        Node::AwsCloudFrontDistribution,
        Some(ConfigKey::ResourceId),
    ),
];

pub(crate) fn typed_config_type(resource_type: &str) -> Option<&'static TypedConfigType> {
    TYPED_CONFIG_TYPES
        .iter()
        .find(|typed| typed.resource_type == resource_type)
}

pub(crate) fn use_aws_resource(name: &str, exclude_by_default: bool) -> bool {
    if typed_config_type(name).is_some() {
        return false;
    }
    match name {
        "AWS::RDS::DBClusterSnapshot" => false,
        "AWS::StepFunctions::StateMachine" => false,
        "AWS::ApiGateway::Stage" => false,
        "AWS::ApiGatewayV2::Api" => false,
        "AWS::EC2::NetworkAcl" => false,
        "AWS::EC2::TransitGatewayAttachment" => false,
        "AWS::ElasticLoadBalancingV2::Listener" => false,
        "AWS::RDS::DBCluster" => true,
        "AWS::S3::Bucket" => true,
        "AWS::ElasticLoadBalancing::LoadBalancer" => true,
        "AWS::Redshift::ClusterSubnetGroup" => true,
        "AWS::RDS::DBSubnetGroup" => true,

        // TODO: below are unclear if actually wanted/needed
        "AWS::Route53Resolver::ResolverRuleAssociation" => true,
        "AWS::EC2::VPCEndpoint" => true,
        "AWS::Route53Resolver::ResolverRule" => true,
        _ => !exclude_by_default,
    }
}

pub(crate) fn use_global(name: &str) -> bool {
    matches!(name, "AWS::S3::Bucket")
}
