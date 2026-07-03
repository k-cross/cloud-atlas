use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::GoogleCollection;
use rayon::prelude::*;

/// Project resources that only contribute a standalone node keyed by one
/// optional identifier field.
macro_rules! project_leaf {
    ($builder:expr, $items:expr, $field:ident, $variant:path) => {
        for item in $items {
            if let Some(id) = &item.$field {
                $builder.get_or_add_node($variant(id.as_str().into()));
            }
        }
    };
}

pub fn gcp_projector(builder: &mut GraphBuilder, gcp_data: &[GoogleCollection]) {
    // Collections are independent, so project each into a thread-local
    // sub-graph in parallel and merge serially in input order.
    let sub_graphs: Vec<GraphBuilder> = gcp_data
        .par_iter()
        .map(|collection| {
            let mut local = GraphBuilder::new();
            project_google_collection(&mut local, collection);
            local
        })
        .collect();

    for sub in &sub_graphs {
        builder.merge(sub);
    }
}

fn project_google_collection(builder: &mut GraphBuilder, x: &GoogleCollection) {
    match x {
        GoogleCollection::GoogleInstances(instances) => {
            for inst in instances {
                // Extract project and zone from the self_link e.g., https://www.googleapis.com/compute/v1/projects/my-project/zones/us-central1-a/instances/my-instance
                let mut project_idx = None;
                let mut zone_idx = None;
                if let Some(self_link) = &inst.self_link {
                    if let Some(project_id) = self_link
                        .split("/projects/")
                        .nth(1)
                        .and_then(|rest| rest.split('/').next())
                    {
                        let project_node = Node::GcpProject(project_id.into());
                        project_idx = Some(builder.get_or_add_node(project_node));
                    }
                    if let Some(zone) = self_link
                        .split("/zones/")
                        .nth(1)
                        .and_then(|rest| rest.split('/').next())
                    {
                        let zone_node = Node::GcpComputeZone(zone.into());
                        zone_idx = Some(builder.get_or_add_node(zone_node));
                    }
                }

                if let Some(id) = &inst.id {
                    let node = Node::GcpComputeInstance(id.as_str().into());
                    let idx = builder.get_or_add_node(node);
                    if let Some(p_idx) = project_idx {
                        builder.add_edge(p_idx, idx, Edge::DependsOn);
                    }
                    if let Some(z_idx) = zone_idx {
                        builder.add_edge(z_idx, idx, Edge::Contains);
                    }

                    if let Some(network_interfaces) = &inst.network_interfaces {
                        for net in network_interfaces {
                            if let Some(network) = &net.network {
                                let net_node = Node::GcpComputeNetwork(network.as_str().into());
                                let n_idx = builder.get_or_add_node(net_node);
                                builder.add_edge(n_idx, idx, Edge::Contains);
                            }
                        }
                    }
                }
            }
        }
        GoogleCollection::GoogleFirewalls(firewalls) => {
            for fw in firewalls {
                if let Some(id) = &fw.id {
                    let node = Node::GcpComputeFirewall(id.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(network) = &fw.network {
                        let net_node = Node::GcpComputeNetwork(network.as_str().into());
                        let n_idx = builder.get_or_add_node(net_node);
                        builder.add_edge(n_idx, idx, Edge::Contains);
                    }

                    if let Some(direction) = &fw.direction
                        && direction == "EGRESS"
                        && let Some(ranges) = &fw.destination_ranges
                    {
                        for range in ranges {
                            if !is_large_cidr(range) {
                                let ip_node = Node::GenericIpAddress(range.as_str().into());
                                let ip_idx = builder.get_or_add_node(ip_node);
                                builder.add_edge(idx, ip_idx, Edge::RoutesTo);
                            }
                        }
                    }
                }
            }
        }
        GoogleCollection::GoogleSql(instances) => {
            for sql in instances {
                if let Some(name) = &sql.name {
                    let node = Node::GcpSqlInstance(name.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(ips) = &sql.ip_addresses {
                        for ip in ips {
                            if let Some(ip_addr) = &ip.ip_address {
                                let ip_node = Node::GenericIpAddress(ip_addr.as_str().into());
                                let ip_idx = builder.get_or_add_node(ip_node);
                                builder.add_edge(idx, ip_idx, Edge::ConnectsTo);
                            }
                        }
                    }
                }
            }
        }
        GoogleCollection::GoogleDns(zones) => {
            project_leaf!(builder, zones, name, Node::GcpDnsManagedZone)
        }
        GoogleCollection::GoogleGke(clusters) => {
            for cluster in clusters {
                if let Some(name) = &cluster.name {
                    let node = Node::GcpGkeCluster(name.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(network) = &cluster.network {
                        let net_node = Node::GcpComputeNetwork(network.as_str().into());
                        let n_idx = builder.get_or_add_node(net_node);
                        builder.add_edge(n_idx, idx, Edge::Contains);
                    }
                }
            }
        }
        GoogleCollection::GoogleFunctions(functions) => {
            project_leaf!(builder, functions, name, Node::GcpCloudFunction)
        }
        GoogleCollection::GoogleStorageBuckets(buckets) => {
            project_leaf!(builder, buckets, id, Node::GcpStorageBucket)
        }
        GoogleCollection::GooglePubSubTopics(topics) => {
            project_leaf!(builder, topics, name, Node::GcpPubSubTopic)
        }
        GoogleCollection::GooglePubSubSubscriptions(subscriptions) => {
            for sub in subscriptions {
                if let Some(name) = &sub.name {
                    let node = Node::GcpPubSubSubscription(name.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(topic) = &sub.topic {
                        let topic_node = Node::GcpPubSubTopic(topic.as_str().into());
                        let t_idx = builder.get_or_add_node(topic_node);
                        builder.add_edge(idx, t_idx, Edge::ConnectsTo);
                    }
                }
            }
        }
        GoogleCollection::GoogleRunServices(services) => {
            for service in services {
                if let Some(name) = &service.name {
                    let node = Node::GcpCloudRunService(name.as_str().into());
                    let s_idx = builder.get_or_add_node(node);

                    if let Some(uri) = &service.uri {
                        let hostname = uri
                            .trim_start_matches("https://")
                            .trim_start_matches("http://");
                        let pivot_node = Node::GenericHostname(hostname.into());
                        let pivot_idx = builder.get_or_add_node(pivot_node);
                        builder.add_edge(pivot_idx, s_idx, Edge::RoutesTo);
                    }
                }
            }
        }
        GoogleCollection::GoogleNetworks(networks) => {
            project_leaf!(builder, networks, self_link, Node::GcpComputeNetwork)
        }
        GoogleCollection::GoogleSubnetworks(subnetworks) => {
            for subnetwork in subnetworks {
                if let Some(self_link) = &subnetwork.self_link {
                    let node = Node::GcpComputeSubnetwork(self_link.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(network) = &subnetwork.network {
                        let net_node = Node::GcpComputeNetwork(network.as_str().into());
                        let n_idx = builder.get_or_add_node(net_node);
                        builder.add_edge(n_idx, idx, Edge::Contains);
                    }
                }
            }
        }
        GoogleCollection::GoogleForwardingRules(rules) => {
            for rule in rules {
                if let Some(id) = &rule.id {
                    let node = Node::GcpComputeForwardingRule(id.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(ip) = &rule.ip_address {
                        let ip_node = Node::GenericIpAddress(ip.as_str().into());
                        let ip_idx = builder.get_or_add_node(ip_node);
                        builder.add_edge(idx, ip_idx, Edge::ConnectsTo);
                    }
                }
            }
        }
    }
}
