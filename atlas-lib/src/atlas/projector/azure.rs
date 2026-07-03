use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::MicrosoftCollection;
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

pub fn azure_projector(builder: &mut GraphBuilder, azure_data: &[MicrosoftCollection]) {
    // Collections are independent, so project each into a thread-local
    // sub-graph in parallel and merge serially in input order.
    let sub_graphs: Vec<GraphBuilder> = azure_data
        .par_iter()
        .map(|collection| {
            let mut local = GraphBuilder::new();
            project_microsoft_collection(&mut local, collection);
            local
        })
        .collect();

    for sub in &sub_graphs {
        builder.merge(sub);
    }
}

fn project_microsoft_collection(builder: &mut GraphBuilder, x: &MicrosoftCollection) {
    match x {
        MicrosoftCollection::AzureVirtualMachines(vms) => {
            for vm in vms {
                if let Some(id) = &vm.id {
                    let node = Node::AzureVirtualMachine(id.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    for nic_id in &vm.network_interfaces {
                        let nic_node = Node::AzureNetworkInterface(nic_id.as_str().into());
                        let nic_idx = builder.get_or_add_node(nic_node);
                        builder.add_edge(idx, nic_idx, Edge::ConnectsTo);
                    }
                }
            }
        }
        MicrosoftCollection::AzureVirtualNetworks(vnets) => {
            for vnet in vnets {
                if let Some(id) = &vnet.id {
                    let node = Node::AzureVirtualNetwork(id.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    for subnet_id in &vnet.subnets {
                        let subnet_node = Node::AzureSubnet(subnet_id.as_str().into());
                        let subnet_idx = builder.get_or_add_node(subnet_node);
                        builder.add_edge(idx, subnet_idx, Edge::Contains);
                    }
                }
            }
        }
        MicrosoftCollection::AzureSubnets(subnets) => {
            for subnet in subnets {
                if let Some(id) = &subnet.id {
                    let node = Node::AzureSubnet(id.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(vnet_id) = &subnet.vnet_id {
                        let vnet_node = Node::AzureVirtualNetwork(vnet_id.as_str().into());
                        let v_idx = builder.get_or_add_node(vnet_node);
                        builder.add_edge(v_idx, idx, Edge::Contains);
                    }

                    if let Some(nsg_id) = &subnet.network_security_group_id {
                        let nsg_node = Node::AzureNetworkSecurityGroup(nsg_id.as_str().into());
                        let nsg_idx = builder.get_or_add_node(nsg_node);
                        builder.add_edge(idx, nsg_idx, Edge::ConnectsTo);
                    }
                }
            }
        }
        MicrosoftCollection::AzureNetworkSecurityGroups(nsgs) => {
            for nsg in nsgs {
                if let Some(id) = &nsg.id {
                    let node = Node::AzureNetworkSecurityGroup(id.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(props) = &nsg.properties
                        && let Some(rules) = &props.security_rules
                    {
                        for rule in rules {
                            if let Some(rprops) = &rule.properties
                                && let Some(direction) = &rprops.direction
                                && direction.eq_ignore_ascii_case("Outbound")
                            {
                                let mut destinations = Vec::new();
                                if let Some(dest) = &rprops.destination_address_prefix {
                                    destinations.push(dest.clone());
                                }
                                if let Some(dests) = &rprops.destination_address_prefixes {
                                    destinations.extend(dests.clone());
                                }

                                for dest in destinations {
                                    if is_service_tag(&dest) {
                                        let tag_node = Node::AzureServiceTag(dest.into());
                                        let tag_idx = builder.get_or_add_node(tag_node);
                                        builder.add_edge(idx, tag_idx, Edge::RoutesTo);
                                    } else if !is_large_cidr(&dest) {
                                        let ip_node = Node::GenericIpAddress(dest.into());
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
        MicrosoftCollection::AzurePublicIpAddresses(pips) => {
            for pip in pips {
                if let Some(id) = &pip.id {
                    let node = Node::AzurePublicIpAddress(id.as_str().into());
                    let idx = builder.get_or_add_node(node);

                    if let Some(ip) = &pip.ip_address {
                        let ip_node = Node::GenericIpAddress(ip.as_str().into());
                        let ip_idx = builder.get_or_add_node(ip_node);
                        builder.add_edge(idx, ip_idx, Edge::ConnectsTo);
                    }
                }
            }
        }
        MicrosoftCollection::AzureStorageAccounts(accounts) => {
            project_leaf!(builder, accounts, id, Node::AzureStorageAccount)
        }
        MicrosoftCollection::AzureManagedClusters(clusters) => {
            project_leaf!(builder, clusters, id, Node::AzureManagedCluster)
        }
        MicrosoftCollection::AzureSqlServers(servers) => {
            project_leaf!(builder, servers, id, Node::AzureSqlServer)
        }
        MicrosoftCollection::AzureAppServices(apps) => {
            for app in apps {
                if let Some(id) = &app.id {
                    let node = Node::AzureAppService(id.as_str().into());
                    let app_idx = builder.get_or_add_node(node);

                    if let Some(props) = &app.properties
                        && let Some(hostname) = &props.default_host_name
                    {
                        let pivot_node = Node::GenericHostname(hostname.as_str().into());
                        let pivot_idx = builder.get_or_add_node(pivot_node);
                        builder.add_edge(pivot_idx, app_idx, Edge::RoutesTo);
                    }
                }
            }
        }
        MicrosoftCollection::AzureFunctionApps(funcs) => {
            project_leaf!(builder, funcs, id, Node::AzureFunctionApp)
        }
        MicrosoftCollection::AzureApiManagement(apims) => {
            project_leaf!(builder, apims, id, Node::AzureApiManagement)
        }
        MicrosoftCollection::AzureCosmosDbs(cosmos) => {
            project_leaf!(builder, cosmos, id, Node::AzureCosmosDb)
        }
        MicrosoftCollection::AzureServiceBuses(sbuses) => {
            project_leaf!(builder, sbuses, id, Node::AzureServiceBus)
        }
        MicrosoftCollection::AzureEventGridTopics(egrids) => {
            project_leaf!(builder, egrids, id, Node::AzureEventGridTopic)
        }
        MicrosoftCollection::AzureDnsZones(dns) => {
            project_leaf!(builder, dns, id, Node::AzureDnsZone)
        }
        MicrosoftCollection::AzureCdnProfiles(cdns) => {
            project_leaf!(builder, cdns, id, Node::AzureCdnProfile)
        }
    }
}

fn is_service_tag(tag: &str) -> bool {
    // Service tags are typically alphabetic (e.g. "Internet", "AzureCloud.WestUS")
    !tag.contains('.') && !tag.contains(':') && tag != "*"
}
