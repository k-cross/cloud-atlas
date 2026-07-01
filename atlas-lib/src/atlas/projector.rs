use crate::Settings;
use crate::cloud::definition::{AmazonCollection, GoogleCollection, MicrosoftCollection, Provider};
use petgraph::graph::{Graph, NodeIndex};
use std::collections::HashMap;
use crate::atlas::definition::{Node, Edge};

pub fn build<'b>(data: &'b Provider, opts: &'b Settings) -> Graph<Node, Edge> {
    match data {
        Provider::AWS(aws_data) => aws_projector(&aws_data, opts),
        Provider::GCP(gcp_data) => gcp_projector(&gcp_data),
        Provider::Azure(azure_data) => azure_projector(&azure_data),
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
        let region_node = Node::Region { name: region.to_string() };
        let region_idx = get_or_add_node(&mut graph, region_node);

        match x {
            AmazonCollection::AmazonInstances(instance_data) => {
                for inst in instance_data {
                    let mut vpc_idx = None;
                    if let Some(vpc_id) = inst.vpc_id.as_ref() {
                        let node = Node::Vpc { id: vpc_id.to_string() };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::Contains);
                        vpc_idx = Some(idx);
                    }

                    let mut subnet_idx = None;
                    if let Some(subnet_id) = inst.subnet_id.as_ref() {
                        let node = Node::Subnet { id: subnet_id.to_string() };
                        let idx = get_or_add_node(&mut graph, node);
                        if let Some(v_idx) = vpc_idx {
                            graph.add_edge(v_idx, idx, Edge::Contains);
                        }
                        subnet_idx = Some(idx);
                    }

                    let mut inst_idx = None;
                    if let Some(instance_id) = inst.instance_id.as_ref() {
                        let node = Node::Instance { id: instance_id.to_string() };
                        let idx = get_or_add_node(&mut graph, node);
                        if let Some(s_idx) = subnet_idx {
                            graph.add_edge(s_idx, idx, Edge::Contains);
                        }
                        inst_idx = Some(idx);
                    }

                    if let Some(place) = inst.placement.as_ref() {
                        if let Some(az_name) = place.availability_zone.as_ref() {
                            let node = Node::Az { name: az_name.to_string() };
                            let az_idx = get_or_add_node(&mut graph, node);
                            
                            if let Some(i_idx) = inst_idx {
                                graph.add_edge(az_idx, i_idx, Edge::Contains);
                            }
                        }
                    }

                    if let Some(private_ip) = inst.private_ip_address.as_ref() {
                        let node = Node::IpAddress { ip: private_ip.to_string() };
                        let ip_idx = get_or_add_node(&mut graph, node);
                        if let Some(i_idx) = inst_idx {
                            graph.add_edge(i_idx, ip_idx, Edge::HasIp);
                        }
                    }

                    if let Some(tags) = inst.tags.as_ref() {
                        for tag in tags {
                            if let (Some(k), Some(v)) = (tag.key.as_ref(), tag.value.as_ref()) {
                                let node = Node::Tag { key: k.to_string(), value: v.to_string() };
                                let tag_idx = get_or_add_node(&mut graph, node);
                                if let Some(i_idx) = inst_idx {
                                    graph.add_edge(i_idx, tag_idx, Edge::HasTag);
                                }
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
                                let node = Node::Generic { id: id.to_string() };
                                let idx = get_or_add_node(&mut graph, node);

                                if use_global(res_name.as_str()) {
                                    let global_node = Node::Region { name: "global".to_string() };
                                    let g_idx = get_or_add_node(&mut graph, global_node);
                                    graph.add_edge(g_idx, idx, Edge::Generic);
                                } else {
                                    graph.add_edge(region_idx, idx, Edge::Generic);
                                }
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonClusters(clusters) => {
                for cluster in clusters {
                    if let Some(arn) = cluster.cluster_arn() {
                        let node = Node::Generic { id: arn.to_string() };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::Generic);
                    }
                }
            }
            AmazonCollection::AmazonLambdas(lambdas) => {
                for lambda in lambdas {
                    if let Some(name) = lambda.function_name() {
                        let node = Node::Generic { id: name.to_string() };
                        let idx = get_or_add_node(&mut graph, node);
                        graph.add_edge(region_idx, idx, Edge::Generic);
                        
                        if let Some(role) = lambda.role() {
                            let role_node = Node::Generic { id: role.to_string() };
                            let r_idx = get_or_add_node(&mut graph, role_node);
                            graph.add_edge(idx, r_idx, Edge::AttachedTo);
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
        }
    }

    graph
}

pub fn gcp_projector<'a>(_gcp_data: &Vec<GoogleCollection>) -> Graph<Node, Edge> {
    todo!()
}

pub fn azure_projector<'a>(_azure_data: &Vec<MicrosoftCollection>) -> Graph<Node, Edge> {
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
    match name {
        "AWS::S3::Bucket" => true,
        _ => false,
    }
}
