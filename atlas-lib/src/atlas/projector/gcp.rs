use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::GoogleCollection;
use rayon::prelude::*;

macro_rules! project_leaf {
    ($builder:expr, $items:expr, $field:ident, $variant:path) => {
        for item in $items {
            if let Some(id) = &item.$field {
                $builder.get_or_add_node($variant(id.as_str().into()));
            }
        }
    };
}

pub fn gcp_projector(builder: &mut GraphBuilder, gcp_data: &[(String, GoogleCollection)]) {
    let sub_graphs: Vec<GraphBuilder> = gcp_data
        .par_iter()
        .map(|(project, collection)| {
            let mut local = GraphBuilder::new();
            project_google_collection(&mut local, project, collection);
            local
        })
        .collect();

    for sub in &sub_graphs {
        builder.merge(&sub.graph);
    }
}

fn project_google_collection(builder: &mut GraphBuilder, project: &str, x: &GoogleCollection) {
    let project_idx = builder.get_or_add_node(Node::GcpProject(project.into()));

    match x {
        GoogleCollection::GoogleInstances(instances) => {
            for inst in instances {
                let mut zone_idx = None;
                if let Some(self_link) = &inst.self_link
                    && let Some(zone) = self_link
                        .split("/zones/")
                        .nth(1)
                        .and_then(|rest| rest.split('/').next())
                {
                    let zone_node = Node::GcpComputeZone(zone.into());
                    zone_idx = Some(builder.get_or_add_node(zone_node));
                }

                if let Some(id) = &inst.id {
                    let idx = builder.link_to(
                        project_idx,
                        Node::GcpComputeInstance(id.as_str().into()),
                        Edge::DependsOn,
                    );
                    if let Some(z_idx) = zone_idx {
                        builder.add_edge(z_idx, idx, Edge::Contains);
                    }

                    if let Some(network_interfaces) = &inst.network_interfaces {
                        for net in network_interfaces {
                            if let Some(network) = &net.network {
                                builder.link_from(
                                    idx,
                                    Node::GcpComputeNetwork(network.as_str().into()),
                                    Edge::Contains,
                                );
                            }
                        }
                    }
                }
            }
        }
        GoogleCollection::GoogleFirewalls(firewalls) => {
            for fw in firewalls {
                if let Some(id) = &fw.id {
                    let idx = builder.get_or_add_node(Node::GcpComputeFirewall(id.as_str().into()));

                    if let Some(network) = &fw.network {
                        builder.link_from(
                            idx,
                            Node::GcpComputeNetwork(network.as_str().into()),
                            Edge::Contains,
                        );
                    }

                    if let Some(direction) = &fw.direction
                        && direction == "EGRESS"
                        && let Some(ranges) = &fw.destination_ranges
                    {
                        for range in ranges {
                            if !is_large_cidr(range) {
                                builder.link_to(
                                    idx,
                                    Node::GenericIpAddress(range.as_str().into()),
                                    Edge::RoutesTo,
                                );
                            }
                        }
                    }
                }
            }
        }
        GoogleCollection::GoogleSql(instances) => {
            for sql in instances {
                if let Some(name) = &sql.name {
                    let idx = builder.get_or_add_node(Node::GcpSqlInstance(name.as_str().into()));

                    if let Some(ips) = &sql.ip_addresses {
                        for ip in ips {
                            if let Some(ip_addr) = &ip.ip_address {
                                builder.link_to(
                                    idx,
                                    Node::GenericIpAddress(ip_addr.as_str().into()),
                                    Edge::ConnectsTo,
                                );
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
                    let idx = builder.get_or_add_node(Node::GcpGkeCluster(name.as_str().into()));

                    if let Some(network) = &cluster.network {
                        builder.link_from(
                            idx,
                            Node::GcpComputeNetwork(network.as_str().into()),
                            Edge::Contains,
                        );
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
                    let idx =
                        builder.get_or_add_node(Node::GcpPubSubSubscription(name.as_str().into()));

                    if let Some(topic) = &sub.topic {
                        builder.link_to(
                            idx,
                            Node::GcpPubSubTopic(topic.as_str().into()),
                            Edge::ConnectsTo,
                        );
                    }
                }
            }
        }
        GoogleCollection::GoogleRunServices(services) => {
            for service in services {
                if let Some(name) = &service.name {
                    let s_idx =
                        builder.get_or_add_node(Node::GcpCloudRunService(name.as_str().into()));

                    if let Some(uri) = &service.uri {
                        let hostname = uri
                            .trim_start_matches("https://")
                            .trim_start_matches("http://");
                        builder.link_from(
                            s_idx,
                            Node::GenericHostname(hostname.into()),
                            Edge::RoutesTo,
                        );
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
                    let idx = builder
                        .get_or_add_node(Node::GcpComputeSubnetwork(self_link.as_str().into()));

                    if let Some(network) = &subnetwork.network {
                        builder.link_from(
                            idx,
                            Node::GcpComputeNetwork(network.as_str().into()),
                            Edge::Contains,
                        );
                    }
                }
            }
        }
        GoogleCollection::GoogleForwardingRules(rules) => {
            for rule in rules {
                if let Some(id) = &rule.id {
                    let idx =
                        builder.get_or_add_node(Node::GcpComputeForwardingRule(id.as_str().into()));

                    if let Some(ip) = &rule.ip_address {
                        builder.link_to(
                            idx,
                            Node::GenericIpAddress(ip.as_str().into()),
                            Edge::ConnectsTo,
                        );
                    }
                }
            }
        }
    }
}
