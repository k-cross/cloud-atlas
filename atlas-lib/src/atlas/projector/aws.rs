use crate::Settings;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::AmazonCollection;

pub fn aws_projector(
    builder: &mut GraphBuilder,
    aws_data: &Vec<(String, AmazonCollection)>,
    opts: &Settings,
) {
    for (region, x) in aws_data {
        let region_node = Node::AwsRegion(region.as_str().into());
        let region_idx = builder.get_or_add_node(region_node);

        match x {
            AmazonCollection::AmazonInstances(instance_data) => {
                for inst in instance_data {
                    let mut vpc_idx = None;
                    if let Some(vpc_id) = inst.vpc_id.as_ref() {
                        let node = Node::AwsEc2Vpc(vpc_id.as_str().into());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::Contains);
                        vpc_idx = Some(idx);
                    }

                    let mut subnet_idx = None;
                    if let Some(subnet_id) = inst.subnet_id.as_ref() {
                        let node = Node::AwsEc2Subnet(subnet_id.as_str().into());
                        let idx = builder.get_or_add_node(node);
                        if let Some(v_idx) = vpc_idx {
                            builder.add_edge(v_idx, idx, Edge::Contains);
                        }
                        subnet_idx = Some(idx);
                    }

                    let mut inst_idx = None;
                    if let Some(instance_id) = inst.instance_id.as_ref() {
                        let node = Node::AwsEc2Instance(instance_id.as_str().into());
                        let idx = builder.get_or_add_node(node);
                        inst_idx = Some(idx);

                        if let Some(subnet_idx) = subnet_idx {
                            let eni_node = Node::AwsEc2Eni(instance_id.as_str().into());
                            let eni_idx = builder.get_or_add_node(eni_node);

                            // Instance -> HasIp -> ENI
                            builder.add_edge(idx, eni_idx, Edge::HasIp);
                            // ENI -> AttachedTo -> Subnet
                            builder.add_edge(eni_idx, subnet_idx, Edge::AttachedTo);
                        }
                    }

                    if let Some(place) = inst.placement.as_ref()
                        && let Some(az_name) = place.availability_zone.as_ref()
                    {
                        let node = Node::AwsEc2AvailabilityZone(az_name.as_str().into());
                        let az_idx = builder.get_or_add_node(node);

                        if let Some(i_idx) = inst_idx {
                            builder.add_edge(az_idx, i_idx, Edge::Contains);
                        }
                    }

                    if let Some(private_ip) = inst.private_ip_address.as_ref() {
                        let node = Node::GenericIpAddress(private_ip.as_str().into());
                        let ip_idx = builder.get_or_add_node(node);
                        if let Some(i_idx) = inst_idx {
                            builder.add_edge(i_idx, ip_idx, Edge::ConnectsTo);
                        }
                    }

                    if let Some(tags) = inst.tags.as_ref() {
                        for tag in tags {
                            if let (Some(k), Some(v)) = (tag.key.as_ref(), tag.value.as_ref()) {
                                let node = Node::AwsTag {
                                    key: k.as_str().into(),
                                    value: v.as_str().into(),
                                };
                                let tag_idx = builder.get_or_add_node(node);
                                if let Some(i_idx) = inst_idx {
                                    builder.add_edge(i_idx, tag_idx, Edge::DependsOn);
                                }
                            }
                        }
                    }

                    for sg in inst.security_groups() {
                        if let Some(sg_id) = sg.group_id() {
                            let sg_node = Node::AwsEc2SecurityGroup(sg_id.into());
                            let sg_idx = builder.get_or_add_node(sg_node);
                            if let Some(i_idx) = inst_idx {
                                builder.add_edge(i_idx, sg_idx, Edge::ConnectsTo);
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonResources(resource_map) => {
                for (res_name, rs) in resource_map {
                    if use_aws_resource(res_name.as_str(), opts.exclude_by_default) {
                        for r in rs {
                            if let Some(id) = r.resource_id() {
                                let node = Node::AwsConfigResource {
                                    resource_type: res_name.as_str().into(),
                                    id: id.into(),
                                };
                                let idx = builder.get_or_add_node(node);

                                if use_global(res_name.as_str()) {
                                    let global_node = Node::AwsRegion("global".into());
                                    let g_idx = builder.get_or_add_node(global_node);
                                    builder.add_edge(g_idx, idx, Edge::Contains);
                                } else {
                                    builder.add_edge(region_idx, idx, Edge::Contains);
                                }
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonClusters(clusters) => {
                for cluster in clusters {
                    if let Some(arn) = cluster.cluster_arn() {
                        let node = Node::AwsEcsCluster(arn.into());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::Contains);
                    }
                }
            }
            AmazonCollection::AmazonLambdas(lambdas) => {
                for lambda in lambdas {
                    if let Some(name) = lambda.function_name() {
                        let node = Node::AwsLambdaFunction(name.into());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::Contains);

                        if let Some(role) = lambda.role() {
                            let role_node = Node::AwsIamRole(role.into());
                            let r_idx = builder.get_or_add_node(role_node);
                            builder.add_edge(idx, r_idx, Edge::DependsOn);
                        }

                        if let Some(vpc_config) = lambda.vpc_config() {
                            for sg_id in vpc_config.security_group_ids() {
                                let sg_node = Node::AwsEc2SecurityGroup(sg_id.as_str().into());
                                let sg_idx = builder.get_or_add_node(sg_node);
                                builder.add_edge(idx, sg_idx, Edge::ConnectsTo);
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
            AmazonCollection::AmazonLoadBalancers {
                load_balancers,
                target_groups,
                listeners,
                target_health,
            } => {
                for lb in load_balancers {
                    if let Some(arn) = lb.load_balancer_arn() {
                        let lb_node = Node::AwsElbLoadBalancer(arn.into());
                        let lb_idx = builder.get_or_add_node(lb_node);

                        if let Some(vpc_id) = lb.vpc_id() {
                            let vpc_node = Node::AwsEc2Vpc(vpc_id.into());
                            let v_idx = builder.get_or_add_node(vpc_node);
                            builder.add_edge(v_idx, lb_idx, Edge::Contains);
                        } else {
                            builder.add_edge(region_idx, lb_idx, Edge::Contains);
                        }
                    }
                }

                for tg in target_groups {
                    if let Some(arn) = tg.target_group_arn() {
                        let tg_node = Node::AwsElbTargetGroup(arn.into());
                        let tg_idx = builder.get_or_add_node(tg_node);

                        if let Some(vpc_id) = tg.vpc_id() {
                            let vpc_node = Node::AwsEc2Vpc(vpc_id.into());
                            let v_idx = builder.get_or_add_node(vpc_node);
                            builder.add_edge(v_idx, tg_idx, Edge::Contains);
                        }

                        if let Some(health_descriptions) = target_health.get(arn) {
                            for target_id in health_descriptions
                                .iter()
                                .filter_map(|h| h.target())
                                .filter_map(|t| t.id())
                            {
                                let inst_node = Node::AwsEc2Instance(target_id.into());
                                let i_idx = builder.get_or_add_node(inst_node);
                                builder.add_edge(tg_idx, i_idx, Edge::ConnectsTo);
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
                            let lb_node = Node::AwsElbLoadBalancer(lb_arn.into());
                            let tg_node = Node::AwsElbTargetGroup(tg_arn.into());
                            let lb_idx = builder.get_or_add_node(lb_node);
                            let tg_idx = builder.get_or_add_node(tg_node);
                            builder.add_edge(lb_idx, tg_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            AmazonCollection::AmazonRoute53 {
                hosted_zones,
                record_sets,
            } => {
                let global_node = Node::AwsRegion("global".into());
                let g_idx = builder.get_or_add_node(global_node);

                for hz in hosted_zones {
                    let id = hz.id();
                    let hz_node = Node::AwsRoute53HostedZone(id.into());
                    let hz_idx = builder.get_or_add_node(hz_node);
                    builder.add_edge(g_idx, hz_idx, Edge::Contains);
                }

                for rs in record_sets {
                    let name = rs.name();
                    let rs_node = Node::AwsRoute53RecordSet(name.into());
                    let rs_idx = builder.get_or_add_node(rs_node);

                    builder.add_edge(g_idx, rs_idx, Edge::Contains);

                    let is_ip = rs.r#type() == &aws_sdk_route53::types::RrType::A
                        || rs.r#type() == &aws_sdk_route53::types::RrType::Aaaa;

                    let records = rs.resource_records();
                    for r in records {
                        let val = r.value();
                        let pivot_node = if is_ip {
                            Node::GenericIpAddress(val.into())
                        } else {
                            Node::GenericHostname(val.into())
                        };
                        let pivot_idx = builder.get_or_add_node(pivot_node);
                        builder.add_edge(rs_idx, pivot_idx, Edge::ConnectsTo);
                    }
                }
            }
            AmazonCollection::AmazonEks(clusters) => {
                for cluster in clusters {
                    if let Some(name) = cluster.name() {
                        let node = Node::AwsEksCluster(name.into());
                        let idx = builder.get_or_add_node(node);

                        if let Some(vpc_config) = cluster.resources_vpc_config() {
                            if let Some(vpc_id) = vpc_config.vpc_id() {
                                let vpc_node = Node::AwsEc2Vpc(vpc_id.into());
                                let vpc_idx = builder.get_or_add_node(vpc_node);
                                builder.add_edge(vpc_idx, idx, Edge::Contains);
                            } else {
                                builder.add_edge(region_idx, idx, Edge::Contains);
                            }

                            for sg_id in vpc_config.security_group_ids() {
                                let sg_node = Node::AwsEc2SecurityGroup(sg_id.as_str().into());
                                let sg_idx = builder.get_or_add_node(sg_node);
                                builder.add_edge(idx, sg_idx, Edge::ConnectsTo);
                            }
                        } else {
                            builder.add_edge(region_idx, idx, Edge::Contains);
                        }
                    }
                }
            }
            AmazonCollection::AmazonApiGateway(apis) => {
                for api in apis {
                    if let Some(id) = api.id() {
                        let node = Node::AwsApiGatewayRestApi(id.into());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::Contains);
                    }
                }
            }
            AmazonCollection::AmazonRds(dbs) => {
                for db in dbs {
                    if let Some(id) = db.db_instance_identifier() {
                        let node = Node::AwsRdsDbInstance(id.into());
                        let idx = builder.get_or_add_node(node);

                        if let Some(subnet_group) = db.db_subnet_group() {
                            if let Some(vpc_id) = subnet_group.vpc_id() {
                                let vpc_node = Node::AwsEc2Vpc(vpc_id.into());
                                let vpc_idx = builder.get_or_add_node(vpc_node);
                                builder.add_edge(vpc_idx, idx, Edge::Contains);
                            } else {
                                builder.add_edge(region_idx, idx, Edge::Contains);
                            }
                        } else {
                            builder.add_edge(region_idx, idx, Edge::Contains);
                        }

                        for sg in db.vpc_security_groups() {
                            if let Some(sg_id) = sg.vpc_security_group_id() {
                                let sg_node = Node::AwsEc2SecurityGroup(sg_id.into());
                                let sg_idx = builder.get_or_add_node(sg_node);
                                builder.add_edge(idx, sg_idx, Edge::ConnectsTo);
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonDynamoDb(tables) => {
                for t in tables {
                    let node = Node::AwsDynamoDbTable(t.as_str().into());
                    let idx = builder.get_or_add_node(node);
                    builder.add_edge(region_idx, idx, Edge::Contains);
                }
            }
            AmazonCollection::AmazonSqs(queues) => {
                for q in queues {
                    let node = Node::AwsSqsQueue(q.as_str().into());
                    let idx = builder.get_or_add_node(node);
                    builder.add_edge(region_idx, idx, Edge::Contains);
                }
            }
            AmazonCollection::AmazonSns(topics) => {
                for t in topics {
                    if let Some(arn) = t.topic_arn() {
                        let node = Node::AwsSnsTopic(arn.into());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::Contains);
                    }
                }
            }
            AmazonCollection::AmazonCloudFront(dists) => {
                let global_node = Node::AwsRegion("global".into());
                let g_idx = builder.get_or_add_node(global_node);

                for d in dists {
                    let id = d.id();
                    let node = Node::AwsCloudFrontDistribution(id.into());
                    let idx = builder.get_or_add_node(node);
                    builder.add_edge(g_idx, idx, Edge::Contains);
                }
            }
            AmazonCollection::AmazonNetworking {
                route_tables,
                internet_gateways,
                nat_gateways,
                addresses,
            } => {
                // Elastic IPs: a managed public IP, stitched to the generic IP
                // space so egress can be followed across clouds.
                for addr in addresses {
                    // Prefer the stable allocation id; fall back to the public IP.
                    if let Some(alloc) = addr.allocation_id().or_else(|| addr.public_ip()) {
                        let eip_idx = builder.get_or_add_node(Node::AwsEc2Eip(alloc.into()));
                        if let Some(public_ip) = addr.public_ip() {
                            let ip_idx =
                                builder.get_or_add_node(Node::GenericIpAddress(public_ip.into()));
                            builder.add_edge(eip_idx, ip_idx, Edge::ConnectsTo);
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
                                let vpc_idx =
                                    builder.get_or_add_node(Node::AwsEc2Vpc(vpc_id.into()));
                                builder.add_edge(igw_idx, vpc_idx, Edge::AttachedTo);
                            }
                        }
                    }
                }

                // NAT gateways: private-subnet egress, living in a subnet and
                // holding an Elastic IP.
                for nat in nat_gateways {
                    if let Some(nat_id) = nat.nat_gateway_id() {
                        let nat_idx =
                            builder.get_or_add_node(Node::AwsEc2NatGateway(nat_id.into()));
                        if let Some(subnet_id) = nat.subnet_id() {
                            let subnet_idx =
                                builder.get_or_add_node(Node::AwsEc2Subnet(subnet_id.into()));
                            builder.add_edge(nat_idx, subnet_idx, Edge::AttachedTo);
                        }
                        for nat_addr in nat.nat_gateway_addresses() {
                            if let Some(alloc) =
                                nat_addr.allocation_id().or_else(|| nat_addr.public_ip())
                            {
                                let eip_idx =
                                    builder.get_or_add_node(Node::AwsEc2Eip(alloc.into()));
                                builder.add_edge(nat_idx, eip_idx, Edge::HasIp);
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
                            let vpc_idx = builder.get_or_add_node(Node::AwsEc2Vpc(vpc_id.into()));
                            builder.add_edge(vpc_idx, rt_idx, Edge::Contains);
                        }

                        for assoc in rt.associations() {
                            if let Some(subnet_id) = assoc.subnet_id() {
                                let subnet_idx =
                                    builder.get_or_add_node(Node::AwsEc2Subnet(subnet_id.into()));
                                builder.add_edge(subnet_idx, rt_idx, Edge::AttachedTo);
                            }
                        }

                        for route in rt.routes() {
                            if let Some(nat_id) = route.nat_gateway_id() {
                                let nat_idx =
                                    builder.get_or_add_node(Node::AwsEc2NatGateway(nat_id.into()));
                                builder.add_edge(rt_idx, nat_idx, Edge::RoutesTo);
                            } else if let Some(gw_id) = route.gateway_id()
                                && gw_id.starts_with("igw-")
                            {
                                let igw_idx = builder
                                    .get_or_add_node(Node::AwsEc2InternetGateway(gw_id.into()));
                                builder.add_edge(rt_idx, igw_idx, Edge::RoutesTo);
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonSecurityGroups(groups) => {
                for sg in groups {
                    if let Some(id) = sg.group_id() {
                        let node = Node::AwsEc2SecurityGroup(id.into());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::Contains);

                        for perm in sg.ip_permissions() {
                            for pair in perm.user_id_group_pairs() {
                                if let Some(referenced_group_id) = pair.group_id() {
                                    let ref_node =
                                        Node::AwsEc2SecurityGroup(referenced_group_id.into());
                                    let ref_idx = builder.get_or_add_node(ref_node);
                                    // The referenced group allows traffic TO this group
                                    builder.add_edge(ref_idx, idx, Edge::ConnectsTo);
                                }
                            }
                        }

                        for perm in sg.ip_permissions_egress() {
                            for ip_range in perm.ip_ranges() {
                                if let Some(cidr) = ip_range.cidr_ip()
                                    && !is_large_cidr(cidr)
                                {
                                    let ip_node = Node::GenericIpAddress(cidr.into());
                                    let ip_idx = builder.get_or_add_node(ip_node);
                                    builder.add_edge(idx, ip_idx, Edge::RoutesTo);
                                }
                            }
                            for ipv6_range in perm.ipv6_ranges() {
                                if let Some(cidr) = ipv6_range.cidr_ipv6()
                                    && !is_large_cidr(cidr)
                                {
                                    let ip_node = Node::GenericIpAddress(cidr.into());
                                    let ip_idx = builder.get_or_add_node(ip_node);
                                    builder.add_edge(idx, ip_idx, Edge::RoutesTo);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn use_aws_resource(name: &str, exclude_by_default: bool) -> bool {
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

fn use_global(name: &str) -> bool {
    matches!(name, "AWS::S3::Bucket")
}
