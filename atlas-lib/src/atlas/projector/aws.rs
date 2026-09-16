use super::{project_leaf, project_parallel};
use crate::Settings;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::{AWSLoadBalancing, AWSNetworking, AWSRoute53, AmazonCollection};
use aws_sdk_ec2::types::NetworkInterface;
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, HashSet};

pub fn aws_projector(
    builder: &mut GraphBuilder,
    aws_data: &[(String, AmazonCollection)],
    opts: &Settings,
) {
    project_parallel(builder, aws_data, |local, (region, collection)| {
        project_amazon_collection(local, region, collection, opts)
    });
    link_interface_owners(builder, aws_data);
}

// The two owner edges an interface cannot decide on its own, because each needs
// another collection and projection runs every collection into its own builder.
//
// The balancer: an interface names it as "ELB app/<name>/<id>" and the ARN ends
// with that same triple, but neither collection carries the other's key.
//
// The instance: an attachment names one even when the scan deliberately left it
// out, since DescribeInstances is filtered to running/pending and a *stopped*
// instance keeps its interfaces attached. Taking the attachment at face value
// would add an instance node the scan does not produce — with no AZ, tags or
// security groups, and indistinguishable in kind from a live one. So the edge
// is emitted only for an instance the scan actually returned; a running one
// reports its own interfaces and project_instance has already drawn it.
fn link_interface_owners(builder: &mut GraphBuilder, aws_data: &[(String, AmazonCollection)]) {
    let mut balancers: HashMap<&str, &str> = HashMap::new();
    let mut scanned_instances: HashSet<&str> = HashSet::new();
    for (_, collection) in aws_data {
        match collection {
            AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing { load_balancers, .. }) => {
                for arn in load_balancers
                    .iter()
                    .filter_map(|lb| lb.load_balancer_arn())
                {
                    if let Some((_, suffix)) = arn.split_once("loadbalancer/") {
                        balancers.insert(suffix, arn);
                    }
                }
            }
            AmazonCollection::AmazonInstances(instances) => {
                scanned_instances.extend(instances.iter().filter_map(|i| i.instance_id()));
            }
            _ => {}
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
                        Node::ip(val)
                    } else {
                        Node::hostname(val)
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
                        builder.link_to(eip_idx, Node::ip(public_ip), Edge::ConnectsTo);
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
                                builder.link_to(idx, Node::ip(cidr), Edge::RoutesTo);
                            }
                        }
                        for ipv6_range in perm.ipv6_ranges() {
                            if let Some(cidr) = ipv6_range.cidr_ipv6()
                                && !is_large_cidr(cidr)
                            {
                                builder.link_to(idx, Node::ip(cidr), Edge::RoutesTo);
                            }
                        }
                    }
                }
            }
        }
    }
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
// needs the balancer collection too (see link_load_balancer_interfaces), and
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

    for address in &facts.addresses {
        builder.link_to(eni_idx, Node::ip(address), Edge::ConnectsTo);
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
                builder.link_to(eni_idx, Node::ip(private_ip), Edge::ConnectsTo);
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

    if facts.network_interfaces.is_empty()
        && let Some(private_ip) = facts.private_ip
    {
        builder.link_to(inst_idx, Node::ip(private_ip), Edge::ConnectsTo);
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

pub(crate) fn use_aws_resource(name: &str, exclude_by_default: bool) -> bool {
    match name {
        "AWS::RDS::DBClusterSnapshot" => false,
        "AWS::StepFunctions::StateMachine" => false,
        "AWS::ApiGateway::Stage" => false,
        "AWS::ApiGatewayV2::Api" => false,
        "AWS::EC2::NetworkAcl" => false,
        "AWS::EC2::EIP" => false,
        "AWS::EC2::NetworkInterface" => false,
        "AWS::EC2::NatGateway" => false,
        "AWS::SNS::Topic" => false,
        "AWS::RDS::DBCluster" => true,
        "AWS::S3::Bucket" => true,
        "AWS::SQS::Queue" => true,
        "AWS::EC2::RouteTable" => false,
        "AWS::EC2::VPC" => true,
        "AWS::EC2::Instance" => true,
        "AWS::ElasticLoadBalancing::LoadBalancer" => true,
        "AWS::ElasticLoadBalancingV2::LoadBalancer" => true,
        "AWS::Redshift::ClusterSubnetGroup" => true,
        "AWS::RDS::DBSubnetGroup" => true,
        "AWS::EC2::Subnet" => true,
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
        _ => !exclude_by_default,
    }
}

pub(crate) fn use_global(name: &str) -> bool {
    matches!(name, "AWS::S3::Bucket")
}
