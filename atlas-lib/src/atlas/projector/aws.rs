use crate::Settings;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::{AWSLoadBalancing, AWSNetworking, AWSRoute53, AmazonCollection};
use petgraph::graph::NodeIndex;
use rayon::prelude::*;

pub fn aws_projector(
    builder: &mut GraphBuilder,
    aws_data: &[(String, AmazonCollection)],
    opts: &Settings,
) {
    // Each (region, collection) tuple is independent, so project them into
    // thread-local sub-graphs in parallel, then merge serially (cheap) in the
    // input order to keep the output deterministic.
    let sub_graphs: Vec<GraphBuilder> = aws_data
        .par_iter()
        .map(|(region, collection)| {
            let mut local = GraphBuilder::new();
            project_amazon_collection(&mut local, region, collection, opts);
            local
        })
        .collect();

    for sub in &sub_graphs {
        builder.merge(&sub.graph);
    }
}

fn project_amazon_collection(
    builder: &mut GraphBuilder,
    region: &str,
    x: &AmazonCollection,
    opts: &Settings,
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
                        // `DescribeInstances` already carries every interface's
                        // real id, so the ENI pivot costs no extra API call.
                        network_interfaces: inst
                            .network_interfaces()
                            .iter()
                            .filter_map(|eni| {
                                Some(EniFacts {
                                    id: eni.network_interface_id()?,
                                    subnet_id: eni.subnet_id(),
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
            for cluster in clusters {
                if let Some(arn) = cluster.cluster_arn() {
                    builder.link_to(region_idx, Node::AwsEcsCluster(arn.into()), Edge::Contains);
                }
            }
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
            target_health,
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

                    if let Some(health_descriptions) = target_health.get(arn) {
                        for target_id in health_descriptions
                            .iter()
                            .filter_map(|h| h.target())
                            .filter_map(|t| t.id())
                        {
                            builder.link_to(
                                tg_idx,
                                Node::AwsEc2Instance(target_id.into()),
                                Edge::ConnectsTo,
                            );
                        }
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
                builder.link_to(
                    g_idx,
                    Node::AwsRoute53HostedZone(hz.id().into()),
                    Edge::Contains,
                );
            }

            for rs in record_sets {
                let rs_idx = builder.link_to(
                    g_idx,
                    Node::AwsRoute53RecordSet(rs.name().into()),
                    Edge::Contains,
                );

                let is_ip = rs.r#type() == &aws_sdk_route53::types::RrType::A
                    || rs.r#type() == &aws_sdk_route53::types::RrType::Aaaa;

                for r in rs.resource_records() {
                    let val = r.value();
                    let pivot_node = if is_ip {
                        Node::GenericIpAddress(val.into())
                    } else {
                        Node::GenericHostname(val.into())
                    };
                    builder.link_to(rs_idx, pivot_node, Edge::ConnectsTo);
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
            for api in apis {
                if let Some(id) = api.id() {
                    builder.link_to(
                        region_idx,
                        Node::AwsApiGatewayRestApi(id.into()),
                        Edge::Contains,
                    );
                }
            }
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
            for t in topics {
                if let Some(arn) = t.topic_arn() {
                    builder.link_to(region_idx, Node::AwsSnsTopic(arn.into()), Edge::Contains);
                }
            }
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
        AmazonCollection::AmazonNetworking(AWSNetworking {
            route_tables,
            internet_gateways,
            nat_gateways,
            addresses,
        }) => {
            // Elastic IPs: a managed public IP, stitched to the generic IP
            // space so egress can be followed across clouds.
            for addr in addresses {
                // Prefer the stable allocation id; fall back to the public IP.
                if let Some(alloc) = addr.allocation_id().or_else(|| addr.public_ip()) {
                    let eip_idx = builder.get_or_add_node(Node::AwsEc2Eip(alloc.into()));
                    if let Some(public_ip) = addr.public_ip() {
                        builder.link_to(
                            eip_idx,
                            Node::GenericIpAddress(public_ip.into()),
                            Edge::ConnectsTo,
                        );
                    }
                }
            }

            // Internet gateways: the public egress door, attached to a VPC.
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

            // NAT gateways: private-subnet egress, living in a subnet and
            // holding an Elastic IP.
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

            // Route tables tie it together: a subnet is associated with a
            // route table, whose routes point at an IGW or NAT gateway.
            for rt in route_tables {
                if let Some(rt_id) = rt.route_table_id() {
                    let rt_idx = builder.get_or_add_node(Node::AwsEc2RouteTable(rt_id.into()));

                    if let Some(vpc_id) = rt.vpc_id() {
                        builder.link_from(rt_idx, Node::AwsEc2Vpc(vpc_id.into()), Edge::Contains);
                    }

                    for assoc in rt.associations() {
                        if let Some(subnet_id) = assoc.subnet_id() {
                            builder.link_from(
                                rt_idx,
                                Node::AwsEc2Subnet(subnet_id.into()),
                                Edge::AttachedTo,
                            );
                        }
                    }

                    for route in rt.routes() {
                        if let Some(nat_id) = route.nat_gateway_id() {
                            builder.link_to(
                                rt_idx,
                                Node::AwsEc2NatGateway(nat_id.into()),
                                Edge::RoutesTo,
                            );
                        } else if let Some(gw_id) = route.gateway_id()
                            && gw_id.starts_with("igw-")
                        {
                            builder.link_to(
                                rt_idx,
                                Node::AwsEc2InternetGateway(gw_id.into()),
                                Edge::RoutesTo,
                            );
                        }
                    }
                }
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
                                // The referenced group allows traffic TO this group
                                builder.link_from(
                                    idx,
                                    Node::AwsEc2SecurityGroup(referenced_group_id.into()),
                                    Edge::ConnectsTo,
                                );
                            }
                        }
                    }

                    for perm in sg.ip_permissions_egress() {
                        for ip_range in perm.ip_ranges() {
                            if let Some(cidr) = ip_range.cidr_ip()
                                && !is_large_cidr(cidr)
                            {
                                builder.link_to(
                                    idx,
                                    Node::GenericIpAddress(cidr.into()),
                                    Edge::RoutesTo,
                                );
                            }
                        }
                        for ipv6_range in perm.ipv6_ranges() {
                            if let Some(cidr) = ipv6_range.cidr_ipv6()
                                && !is_large_cidr(cidr)
                            {
                                builder.link_to(
                                    idx,
                                    Node::GenericIpAddress(cidr.into()),
                                    Edge::RoutesTo,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Everything the graph takes from one EC2 instance, independent of where it
/// was read from.
///
/// The full-scan collector reads an `aws_sdk_ec2::types::Instance`; the Tier-1
/// event adapter reads an AWS Config configuration item or a CloudTrail
/// `RunInstances` record. Those are three different wire shapes describing the
/// same thing, and if each grew its own idea of how an instance attaches to the
/// graph, the event path would emit edges the full scan does not — which the
/// next reconciliation would delete and the next event re-add, forever. Reduce
/// to these facts, and [`project_instance`] stays the single definition of the
/// shape.
pub(crate) struct InstanceFacts<'a> {
    pub id: Option<&'a str>,
    pub vpc_id: Option<&'a str>,
    pub subnet_id: Option<&'a str>,
    pub availability_zone: Option<&'a str>,
    pub private_ip: Option<&'a str>,
    pub security_group_ids: Vec<&'a str>,
    pub tags: Vec<(&'a str, &'a str)>,
    /// Every interface the instance holds, by its real `eni-` id. Plural
    /// because an instance can be multi-homed, and each interface can sit in a
    /// different subnet from the instance's primary one.
    pub network_interfaces: Vec<EniFacts<'a>>,
}

/// One interface, as any of the three wire shapes describes it.
pub(crate) struct EniFacts<'a> {
    pub id: &'a str,
    /// The interface's own subnet, which is not always the instance's — a
    /// second ENI is frequently placed in another subnet on purpose.
    pub subnet_id: Option<&'a str>,
}

/// Attach one instance to the graph: the `Instance -> HasIp -> ENI ->
/// AttachedTo -> Subnet` pivot from CLAUDE.md, plus its VPC/AZ containment,
/// private IP, tags and security groups.
///
/// The VPC and subnet are created even when the instance itself has no id, so a
/// half-described instance still contributes the containment it did report.
///
/// An instance that reports no interfaces gets no ENI, and therefore no path to
/// its subnet. That is deliberate: the ENI is keyed by its own `eni-` id, so
/// there is nothing to invent one from, and substituting a direct
/// `Instance -> Subnet` edge would be a shape no other producer emits — every
/// reconciliation would delete it and every event would put it back. The
/// containment the instance did report (VPC, subnet, AZ) still lands.
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
            // Instance -> HasIp -> ENI -> AttachedTo -> Subnet
            let eni_idx = builder.link_to(idx, Node::AwsEc2Eni(eni.id.into()), Edge::HasIp);
            // The interface's own subnet wins over the instance's primary one,
            // and lands under the same VPC either way.
            let attached = match eni.subnet_id {
                Some(subnet_id) if Some(subnet_id) != facts.subnet_id => Some(builder.link_to(
                    vpc_idx,
                    Node::AwsEc2Subnet(subnet_id.into()),
                    Edge::Contains,
                )),
                _ => subnet_idx,
            };
            if let Some(subnet_idx) = attached {
                builder.add_edge(eni_idx, subnet_idx, Edge::AttachedTo);
            }
        }
    }

    if let Some(az_name) = facts.availability_zone {
        builder.link_from(
            inst_idx,
            Node::AwsEc2AvailabilityZone(az_name.into()),
            Edge::Contains,
        );
    }

    if let Some(private_ip) = facts.private_ip {
        builder.link_to(
            inst_idx,
            Node::GenericIpAddress(private_ip.into()),
            Edge::ConnectsTo,
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

/// Whether the AWS Config catch-all representation of a resource type earns a
/// node. Shared with the Tier-1 event adapter, so a Config change notification
/// files the same resource types the full scan does — a type the scan skips
/// must not arrive by event only to be deleted at the next reconciliation.
pub(crate) fn use_aws_resource(name: &str, exclude_by_default: bool) -> bool {
    match name {
        // false assoc. unclear if needed
        "AWS::RDS::DBClusterSnapshot" => false,
        "AWS::StepFunctions::StateMachine" => false,
        "AWS::ApiGateway::Stage" => false,
        "AWS::ApiGatewayV2::Api" => false,
        "AWS::EC2::NetworkAcl" => false,
        "AWS::EC2::EIP" => false,
        "AWS::EC2::NetworkInterface" => false,
        // Routing plane is now modeled structurally via AmazonNetworking, so
        // skip the edge-less AWS Config catch-all representation.
        "AWS::EC2::NatGateway" => false,
        "AWS::SNS::Topic" => false,
        // true assoc.
        "AWS::RDS::DBCluster" => true,
        "AWS::S3::Bucket" => true,
        "AWS::SQS::Queue" => true,
        // Modeled structurally via AmazonNetworking (route tables + gateways).
        "AWS::EC2::RouteTable" => false,
        "AWS::EC2::VPC" => true,
        "AWS::EC2::Instance" => true,
        "AWS::ElasticLoadBalancing::LoadBalancer" => true,
        "AWS::ElasticLoadBalancingV2::LoadBalancer" => true,
        "AWS::Redshift::ClusterSubnetGroup" => true,
        "AWS::RDS::DBSubnetGroup" => true,
        "AWS::EC2::Subnet" => true,
        // Modeled structurally via AmazonNetworking.
        "AWS::EC2::InternetGateway" => false,
        "AWS::ECS::Cluster" => true,
        "AWS::Lambda::Function" => true,
        "AWS::RDS::DBInstance" => true,
        "AWS::EKS::Cluster" => true,
        "AWS::ElasticLoadBalancingV2::Listener" => true,
        // TODO: below are unclear if actually wanted/needed
        "AWS::Route53Resolver::ResolverRuleAssociation" => true,
        "AWS::EC2::VPCEndpoint" => true,
        "AWS::Route53Resolver::ResolverRule" => true,
        "AWS::DynamoDB::Table" => true,
        // exclude by default
        _ => !exclude_by_default,
    }
}

pub(crate) fn use_global(name: &str) -> bool {
    matches!(name, "AWS::S3::Bucket")
}
