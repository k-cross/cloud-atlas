use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::GoogleCollection;

pub fn gcp_projector(builder: &mut GraphBuilder, gcp_data: &[GoogleCollection]) {
    for x in gcp_data {
        match x {
            GoogleCollection::GoogleInstances(instances) => {
                for inst in instances {
                    let mut project_idx = None;
                    // Attempt to extract project from the self_link e.g., https://www.googleapis.com/compute/v1/projects/my-project/zones/us-central1-a/instances/my-instance
                    if let Some(self_link) = &inst.self_link
                        && let Some(project_str) = self_link.split("/projects/").nth(1)
                        && let Some(project_id) = project_str.split('/').next()
                    {
                        let project_node = Node::GcpProject(project_id.into());
                        project_idx = Some(builder.get_or_add_node(project_node));
                    }

                    if let Some(id) = &inst.id {
                        let node = Node::GcpComputeInstance(id.as_str().into());
                        let idx = builder.get_or_add_node(node);
                        if let Some(p_idx) = project_idx {
                            builder.add_edge(p_idx, idx, Edge::DependsOn);
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
                for zone in zones {
                    if let Some(name) = &zone.name {
                        let node = Node::GcpDnsManagedZone(name.as_str().into());
                        builder.get_or_add_node(node);
                    }
                }
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
                for func in functions {
                    if let Some(name) = &func.name {
                        let node = Node::GcpCloudFunction(name.as_str().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GoogleStorageBuckets(buckets) => {
                for bucket in buckets {
                    if let Some(id) = &bucket.id {
                        let node = Node::GcpStorageBucket(id.as_str().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GooglePubSubTopics(topics) => {
                for topic in topics {
                    if let Some(name) = &topic.name {
                        let node = Node::GcpPubSubTopic(name.as_str().into());
                        builder.get_or_add_node(node);
                    }
                }
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
                for network in networks {
                    if let Some(name) = &network.self_link {
                        let node = Node::GcpComputeNetwork(name.as_str().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GoogleSubnetworks(subnetworks) => {
                for subnetwork in subnetworks {
                    if let Some(name) = &subnetwork.self_link {
                        let node = Node::GcpComputeSubnetwork(name.as_str().into());
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
}
