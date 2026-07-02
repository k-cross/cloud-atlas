use crate::Settings;
use crate::atlas::definition::{Edge, Node, Provider as AtlasProvider};
use crate::cloud::definition::{
    AmazonCollection, GoogleCollection, MicrosoftCollection, Provider as CloudProvider,
};
use petgraph::graph::{Graph, NodeIndex};
use std::collections::HashMap;

pub fn build<'b>(data: &'b CloudProvider, opts: &'b Settings) -> Graph<Node, Edge> {
    match data {
        CloudProvider::AWS(aws_data) => aws_projector(aws_data, opts),
        CloudProvider::GCP(gcp_data) => gcp_projector(gcp_data),
        CloudProvider::Azure(azure_data) => azure_projector(azure_data),
    }
}

pub fn aws_projector<'a>(
    aws_data: &'a Vec<(String, AmazonCollection)>,
    opts: &'a Settings,
) -> Graph<Node, Edge> {
    let mut graph: Graph<Node, Edge> = Graph::new();
    let mut node_map: HashMap<Node, NodeIndex> = HashMap::new();

    let mut get_or_add_node = |graph: &mut Graph<Node, Edge>, node: Node| -> NodeIndex {
        if let Some(&idx) = node_map.get(&node) {
            idx
        } else {
            let idx = graph.add_node(node.clone());
            node_map.insert(node, idx);
            idx
        }
    };

    for (region, x) in aws_data {
        let region_node = Node {
            id: region.to_string(),
            name: "AWS::Region".to_string(),
            category: "AWS".to_string(),
            provider: AtlasProvider::Aws,
        };
        let region_idx = get_or_add_node(&mut graph, region_node);

        match x {
            AmazonCollection::AmazonInstances(instance_data) => {
                for inst in instance_data {
                    let mut vpc_idx = None;
                    if let Some(vpc_id) = inst.vpc_id.as_ref() {
                        let node = Node {
                            id: vpc_id.to_string(),
                            name: "AWS::EC2::VPC".to_string(),
                            category: "AWS::EC2".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::Contains);
                        vpc_idx = Some(idx);
                    }

                    let mut subnet_idx = None;
                    if let Some(subnet_id) = inst.subnet_id.as_ref() {
                        let node = Node {
                            id: subnet_id.to_string(),
                            name: "AWS::EC2::Subnet".to_string(),
                            category: "AWS::EC2".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);
                        if let Some(v_idx) = vpc_idx {
                            graph.add_edge(v_idx, idx, Edge::Contains);
                        }
                        subnet_idx = Some(idx);
                    }

                    let mut inst_idx = None;
                    if let Some(instance_id) = inst.instance_id.as_ref() {
                        let node = Node {
                            id: instance_id.to_string(),
                            name: "AWS::EC2::Instance".to_string(),
                            category: "AWS::EC2".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);
                        if let Some(s_idx) = subnet_idx {
                            graph.add_edge(s_idx, idx, Edge::Contains);
                        }
                        inst_idx = Some(idx);
                    }

                    if let Some(place) = inst.placement.as_ref()
                        && let Some(az_name) = place.availability_zone.as_ref()
                    {
                        let node = Node {
                            id: az_name.to_string(),
                            name: "AWS::EC2::AvailabilityZone".to_string(),
                            category: "AWS::EC2".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let az_idx = get_or_add_node(&mut graph, node);

                        if let Some(i_idx) = inst_idx {
                            graph.add_edge(az_idx, i_idx, Edge::Contains);
                        }
                    }

                    if let Some(private_ip) = inst.private_ip_address.as_ref() {
                        let node = Node {
                            id: private_ip.to_string(),
                            name: "Generic::IpAddress".to_string(),
                            category: "Generic".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let ip_idx = get_or_add_node(&mut graph, node);
                        if let Some(i_idx) = inst_idx {
                            graph.add_edge(i_idx, ip_idx, Edge::ConnectsTo);
                        }
                    }

                    if let Some(tags) = inst.tags.as_ref() {
                        for tag in tags {
                            if let (Some(k), Some(v)) = (tag.key.as_ref(), tag.value.as_ref()) {
                                let node = Node {
                                    id: format!("{}={}", k, v),
                                    name: "AWS::Tag".to_string(),
                                    category: "AWS".to_string(),
                                    provider: AtlasProvider::Aws,
                                };
                                let tag_idx = get_or_add_node(&mut graph, node);
                                if let Some(i_idx) = inst_idx {
                                    graph.add_edge(i_idx, tag_idx, Edge::DependsOn);
                                }
                            }
                        }
                    }

                    for sg in inst.security_groups() {
                        if let Some(sg_id) = sg.group_id() {
                            let sg_node = Node {
                                id: sg_id.to_string(),
                                name: "AWS::EC2::SecurityGroup".to_string(),
                                category: "AWS::EC2".to_string(),
                                provider: AtlasProvider::Aws,
                            };
                            let sg_idx = get_or_add_node(&mut graph, sg_node);
                            if let Some(i_idx) = inst_idx {
                                graph.add_edge(i_idx, sg_idx, Edge::ConnectsTo);
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
                                let category = get_category(res_name.as_str());
                                let node = Node {
                                    id: id.to_string(),
                                    name: res_name.to_string(),
                                    category,
                                    provider: AtlasProvider::Aws,
                                };
                                let idx = get_or_add_node(&mut graph, node);

                                if use_global(res_name.as_str()) {
                                    let global_node = Node {
                                        id: "global".to_string(),
                                        name: "AWS::Region".to_string(),
                                        category: "AWS".to_string(),
                                        provider: AtlasProvider::Aws,
                                    };
                                    let g_idx = get_or_add_node(&mut graph, global_node);
                                    graph.add_edge(g_idx, idx, Edge::DependsOn);
                                } else {
                                    graph.add_edge(region_idx, idx, Edge::DependsOn);
                                }
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonClusters(clusters) => {
                for cluster in clusters {
                    if let Some(arn) = cluster.cluster_arn() {
                        let node = Node {
                            id: arn.to_string(),
                            name: "AWS::ECS::Cluster".to_string(),
                            category: "AWS::ECS".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::DependsOn);
                    }
                }
            }
            AmazonCollection::AmazonLambdas(lambdas) => {
                for lambda in lambdas {
                    if let Some(name) = lambda.function_name() {
                        let node = Node {
                            id: name.to_string(),
                            name: "AWS::Lambda::Function".to_string(),
                            category: "AWS::Lambda".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::DependsOn);

                        if let Some(role) = lambda.role() {
                            let role_node = Node {
                                id: role.to_string(),
                                name: "AWS::IAM::Role".to_string(),
                                category: "AWS::IAM".to_string(),
                                provider: AtlasProvider::Aws,
                            };
                            let r_idx = get_or_add_node(&mut graph, role_node);
                            graph.add_edge(idx, r_idx, Edge::DependsOn);
                        }

                        if let Some(vpc_config) = lambda.vpc_config() {
                            for sg_id in vpc_config.security_group_ids() {
                                let sg_node = Node {
                                    id: sg_id.to_string(),
                                    name: "AWS::EC2::SecurityGroup".to_string(),
                                    category: "AWS::EC2".to_string(),
                                    provider: AtlasProvider::Aws,
                                };
                                let sg_idx = get_or_add_node(&mut graph, sg_node);
                                graph.add_edge(idx, sg_idx, Edge::ConnectsTo);
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
                        let lb_node = Node {
                            id: arn.to_string(),
                            name: "AWS::ElasticLoadBalancingV2::LoadBalancer".to_string(),
                            category: "AWS::ElasticLoadBalancingV2".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let lb_idx = get_or_add_node(&mut graph, lb_node);

                        if let Some(vpc_id) = lb.vpc_id() {
                            let vpc_node = Node {
                                id: vpc_id.to_string(),
                                name: "AWS::EC2::VPC".to_string(),
                                category: "AWS::EC2".to_string(),
                                provider: AtlasProvider::Aws,
                            };
                            let v_idx = get_or_add_node(&mut graph, vpc_node);
                            graph.add_edge(v_idx, lb_idx, Edge::Contains);
                        } else {
                            graph.add_edge(region_idx, lb_idx, Edge::DependsOn);
                        }
                    }
                }

                for tg in target_groups {
                    if let Some(arn) = tg.target_group_arn() {
                        let tg_node = Node {
                            id: arn.to_string(),
                            name: "AWS::ElasticLoadBalancingV2::TargetGroup".to_string(),
                            category: "AWS::ElasticLoadBalancingV2".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let tg_idx = get_or_add_node(&mut graph, tg_node);

                        if let Some(vpc_id) = tg.vpc_id() {
                            let vpc_node = Node {
                                id: vpc_id.to_string(),
                                name: "AWS::EC2::VPC".to_string(),
                                category: "AWS::EC2".to_string(),
                                provider: AtlasProvider::Aws,
                            };
                            let v_idx = get_or_add_node(&mut graph, vpc_node);
                            graph.add_edge(v_idx, tg_idx, Edge::Contains);
                        }

                        if let Some(health_descriptions) = target_health.get(arn) {
                            for target_id in health_descriptions
                                .iter()
                                .filter_map(|h| h.target())
                                .filter_map(|t| t.id())
                            {
                                let inst_node = Node {
                                    id: target_id.to_string(),
                                    name: "AWS::EC2::Instance".to_string(),
                                    category: "AWS::EC2".to_string(),
                                    provider: AtlasProvider::Aws,
                                };
                                let i_idx = get_or_add_node(&mut graph, inst_node);
                                graph.add_edge(tg_idx, i_idx, Edge::ConnectsTo);
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
                            let lb_node = Node {
                                id: lb_arn.to_string(),
                                name: "AWS::ElasticLoadBalancingV2::LoadBalancer".to_string(),
                                category: "AWS::ElasticLoadBalancingV2".to_string(),
                                provider: AtlasProvider::Aws,
                            };
                            let tg_node = Node {
                                id: tg_arn.to_string(),
                                name: "AWS::ElasticLoadBalancingV2::TargetGroup".to_string(),
                                category: "AWS::ElasticLoadBalancingV2".to_string(),
                                provider: AtlasProvider::Aws,
                            };
                            let lb_idx = get_or_add_node(&mut graph, lb_node);
                            let tg_idx = get_or_add_node(&mut graph, tg_node);
                            graph.add_edge(lb_idx, tg_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            AmazonCollection::AmazonRoute53 {
                hosted_zones,
                record_sets,
            } => {
                let global_node = Node {
                    id: "global".to_string(),
                    name: "AWS::Region".to_string(),
                    category: "AWS".to_string(),
                    provider: AtlasProvider::Aws,
                };
                let g_idx = get_or_add_node(&mut graph, global_node);

                for hz in hosted_zones {
                    let id = hz.id();
                    let hz_node = Node {
                        id: id.to_string(),
                        name: "AWS::Route53::HostedZone".to_string(),
                        category: "AWS::Route53".to_string(),
                        provider: AtlasProvider::Aws,
                    };
                    let hz_idx = get_or_add_node(&mut graph, hz_node);
                    graph.add_edge(g_idx, hz_idx, Edge::Contains);
                }

                for rs in record_sets {
                    let name = rs.name();
                    let rs_node = Node {
                        id: name.to_string(),
                        name: "AWS::Route53::RecordSet".to_string(),
                        category: "AWS::Route53".to_string(),
                        provider: AtlasProvider::Aws,
                    };
                    let rs_idx = get_or_add_node(&mut graph, rs_node);

                    graph.add_edge(g_idx, rs_idx, Edge::Contains);

                    let records = rs.resource_records();
                    for r in records {
                        let val = r.value();
                        let ip_node = Node {
                            id: val.to_string(),
                            name: "Generic::IpAddress".to_string(),
                            category: "Generic".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let ip_idx = get_or_add_node(&mut graph, ip_node);
                        graph.add_edge(rs_idx, ip_idx, Edge::ConnectsTo);
                    }
                }
            }
            AmazonCollection::AmazonEks(clusters) => {
                for cluster in clusters {
                    if let Some(name) = cluster.name() {
                        let node = Node {
                            id: name.to_string(),
                            name: "AWS::EKS::Cluster".to_string(),
                            category: "AWS::EKS".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);

                        if let Some(vpc_config) = cluster.resources_vpc_config() {
                            if let Some(vpc_id) = vpc_config.vpc_id() {
                                let vpc_node = Node {
                                    id: vpc_id.to_string(),
                                    name: "AWS::EC2::VPC".to_string(),
                                    category: "AWS::EC2".to_string(),
                                    provider: AtlasProvider::Aws,
                                };
                                let vpc_idx = get_or_add_node(&mut graph, vpc_node);
                                graph.add_edge(vpc_idx, idx, Edge::Contains);
                            } else {
                                graph.add_edge(region_idx, idx, Edge::DependsOn);
                            }

                            for sg_id in vpc_config.security_group_ids() {
                                let sg_node = Node {
                                    id: sg_id.to_string(),
                                    name: "AWS::EC2::SecurityGroup".to_string(),
                                    category: "AWS::EC2".to_string(),
                                    provider: AtlasProvider::Aws,
                                };
                                let sg_idx = get_or_add_node(&mut graph, sg_node);
                                graph.add_edge(idx, sg_idx, Edge::ConnectsTo);
                            }
                        } else {
                            graph.add_edge(region_idx, idx, Edge::DependsOn);
                        }
                    }
                }
            }
            AmazonCollection::AmazonApiGateway(apis) => {
                for api in apis {
                    if let Some(id) = api.id() {
                        let node = Node {
                            id: id.to_string(),
                            name: "AWS::ApiGateway::RestApi".to_string(),
                            category: "AWS::ApiGateway".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::DependsOn);
                    }
                }
            }
            AmazonCollection::AmazonRds(dbs) => {
                for db in dbs {
                    if let Some(id) = db.db_instance_identifier() {
                        let node = Node {
                            id: id.to_string(),
                            name: "AWS::RDS::DBInstance".to_string(),
                            category: "AWS::RDS".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);

                        if let Some(subnet_group) = db.db_subnet_group() {
                            if let Some(vpc_id) = subnet_group.vpc_id() {
                                let vpc_node = Node {
                                    id: vpc_id.to_string(),
                                    name: "AWS::EC2::VPC".to_string(),
                                    category: "AWS::EC2".to_string(),
                                    provider: AtlasProvider::Aws,
                                };
                                let vpc_idx = get_or_add_node(&mut graph, vpc_node);
                                graph.add_edge(vpc_idx, idx, Edge::Contains);
                            } else {
                                graph.add_edge(region_idx, idx, Edge::DependsOn);
                            }
                        } else {
                            graph.add_edge(region_idx, idx, Edge::DependsOn);
                        }

                        for sg in db.vpc_security_groups() {
                            if let Some(sg_id) = sg.vpc_security_group_id() {
                                let sg_node = Node {
                                    id: sg_id.to_string(),
                                    name: "AWS::EC2::SecurityGroup".to_string(),
                                    category: "AWS::EC2".to_string(),
                                    provider: AtlasProvider::Aws,
                                };
                                let sg_idx = get_or_add_node(&mut graph, sg_node);
                                graph.add_edge(idx, sg_idx, Edge::ConnectsTo);
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonDynamoDb(tables) => {
                for t in tables {
                    let node = Node {
                        id: t.to_string(),
                        name: "AWS::DynamoDB::Table".to_string(),
                        category: "AWS::DynamoDB".to_string(),
                        provider: AtlasProvider::Aws,
                    };
                    let idx = get_or_add_node(&mut graph, node);
                    graph.add_edge(region_idx, idx, Edge::DependsOn);
                }
            }
            AmazonCollection::AmazonSqs(queues) => {
                for q in queues {
                    let node = Node {
                        id: q.to_string(),
                        name: "AWS::SQS::Queue".to_string(),
                        category: "AWS::SQS".to_string(),
                        provider: AtlasProvider::Aws,
                    };
                    let idx = get_or_add_node(&mut graph, node);
                    graph.add_edge(region_idx, idx, Edge::DependsOn);
                }
            }
            AmazonCollection::AmazonSns(topics) => {
                for t in topics {
                    if let Some(arn) = t.topic_arn() {
                        let node = Node {
                            id: arn.to_string(),
                            name: "AWS::SNS::Topic".to_string(),
                            category: "AWS::SNS".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::DependsOn);
                    }
                }
            }
            AmazonCollection::AmazonCloudFront(dists) => {
                let global_node = Node {
                    id: "global".to_string(),
                    name: "AWS::Region".to_string(),
                    category: "AWS".to_string(),
                    provider: AtlasProvider::Aws,
                };
                let g_idx = get_or_add_node(&mut graph, global_node);

                for d in dists {
                    let id = d.id();
                    let node = Node {
                        id: id.to_string(),
                        name: "AWS::CloudFront::Distribution".to_string(),
                        category: "AWS::CloudFront".to_string(),
                        provider: AtlasProvider::Aws,
                    };
                    let idx = get_or_add_node(&mut graph, node);
                    graph.add_edge(g_idx, idx, Edge::Contains);
                }
            }
            AmazonCollection::AmazonSecurityGroups(groups) => {
                for sg in groups {
                    if let Some(id) = sg.group_id() {
                        let node = Node {
                            id: id.to_string(),
                            name: "AWS::EC2::SecurityGroup".to_string(),
                            category: "AWS::EC2".to_string(),
                            provider: AtlasProvider::Aws,
                        };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::DependsOn);

                        for perm in sg.ip_permissions() {
                            for pair in perm.user_id_group_pairs() {
                                if let Some(referenced_group_id) = pair.group_id() {
                                    let ref_node = Node {
                                        id: referenced_group_id.to_string(),
                                        name: "AWS::EC2::SecurityGroup".to_string(),
                                        category: "AWS::EC2".to_string(),
                                        provider: AtlasProvider::Aws,
                                    };
                                    let ref_idx = get_or_add_node(&mut graph, ref_node);
                                    // The referenced group allows traffic TO this group
                                    graph.add_edge(ref_idx, idx, Edge::ConnectsTo);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    graph
}

pub fn gcp_projector(_gcp_data: &[GoogleCollection]) -> Graph<Node, Edge> {
    todo!()
}

pub fn azure_projector(_azure_data: &[MicrosoftCollection]) -> Graph<Node, Edge> {
    todo!()
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
        "AWS::SNS::Topic" => false,
        // true assoc.
        "AWS::RDS::DBCluster" => true,
        "AWS::S3::Bucket" => true,
        "AWS::SQS::Queue" => true,
        "AWS::EC2::RouteTable" => true,
        "AWS::EC2::VPC" => true,
        "AWS::EC2::Instance" => true,
        "AWS::ElasticLoadBalancing::LoadBalancer" => true,
        "AWS::ElasticLoadBalancingV2::LoadBalancer" => true,
        "AWS::Redshift::ClusterSubnetGroup" => true,
        "AWS::RDS::DBSubnetGroup" => true,
        "AWS::EC2::Subnet" => true,
        "AWS::EC2::InternetGateway" => true,
        "AWS::ECS::Cluster" => true,
        "AWS::Lambda::Function" => true,
        "AWS::RDS::DBInstance" => true,
        "AWS::EKS::Cluster" => true,
        // listeners are probably too granular
        "AWS::ElasticLoadBalancingV2::Listener" => true,
        // TODO: below are unclear if actually wanted/needed
        "AWS::Route53Resolver::ResolverRuleAssociation" => true,
        "AWS::EC2::VPCEndpoint" => true,
        "AWS::Route53Resolver::ResolverRule" => true,
        "AWS::DynamoDB::Table" => true,
        // exclude by default
        _ => !exclude_by_default.to_owned(),
    }
}

fn use_global(name: &str) -> bool {
    matches!(name, "AWS::S3::Bucket")
}

fn get_category(name: &str) -> String {
    let parts: Vec<&str> = name.split("::").collect();
    if parts.len() >= 2 {
        parts[..parts.len() - 1].join("::")
    } else {
        "Generic".to_string()
    }
}
