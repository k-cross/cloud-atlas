use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::MicrosoftCollection;
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

pub fn azure_projector(builder: &mut GraphBuilder, azure_data: &[MicrosoftCollection]) {
    let sub_graphs: Vec<GraphBuilder> = azure_data
        .par_iter()
        .map(|collection| {
            let mut local = GraphBuilder::new();
            project_microsoft_collection(&mut local, collection);
            local
        })
        .collect();

    for sub in &sub_graphs {
        builder.merge(&sub.graph);
    }
}

fn project_microsoft_collection(builder: &mut GraphBuilder, x: &MicrosoftCollection) {
    match x {
        MicrosoftCollection::AzureVirtualMachines(vms) => {
            for vm in vms {
                if let Some(id) = &vm.id {
                    let idx =
                        builder.get_or_add_node(Node::AzureVirtualMachine(id.as_str().into()));

                    for nic_id in &vm.network_interfaces {
                        builder.link_to(
                            idx,
                            Node::AzureNetworkInterface(nic_id.as_str().into()),
                            Edge::ConnectsTo,
                        );
                    }
                }
            }
        }
        MicrosoftCollection::AzureVirtualNetworks(vnets) => {
            for vnet in vnets {
                if let Some(id) = &vnet.id {
                    let idx =
                        builder.get_or_add_node(Node::AzureVirtualNetwork(id.as_str().into()));

                    for subnet_id in &vnet.subnets {
                        builder.link_to(
                            idx,
                            Node::AzureSubnet(subnet_id.as_str().into()),
                            Edge::Contains,
                        );
                    }
                }
            }
        }
        MicrosoftCollection::AzureSubnets(subnets) => {
            for subnet in subnets {
                if let Some(id) = &subnet.id {
                    let idx = builder.get_or_add_node(Node::AzureSubnet(id.as_str().into()));

                    if let Some(vnet_id) = &subnet.vnet_id {
                        builder.link_from(
                            idx,
                            Node::AzureVirtualNetwork(vnet_id.as_str().into()),
                            Edge::Contains,
                        );
                    }

                    if let Some(nsg_id) = &subnet.network_security_group_id {
                        builder.link_to(
                            idx,
                            Node::AzureNetworkSecurityGroup(nsg_id.as_str().into()),
                            Edge::ConnectsTo,
                        );
                    }
                }
            }
        }
        MicrosoftCollection::AzureNetworkSecurityGroups(nsgs) => {
            for nsg in nsgs {
                if let Some(id) = &nsg.id {
                    let idx = builder
                        .get_or_add_node(Node::AzureNetworkSecurityGroup(id.as_str().into()));

                    if let Some(props) = &nsg.properties
                        && let Some(rules) = &props.security_rules
                    {
                        for rule in rules {
                            if let Some(rprops) = &rule.properties
                                && let Some(direction) = &rprops.direction
                                && direction.eq_ignore_ascii_case("Outbound")
                            {
                                let destinations = rprops
                                    .destination_address_prefix
                                    .iter()
                                    .chain(rprops.destination_address_prefixes.iter().flatten());

                                for dest in destinations {
                                    if is_service_tag(dest) {
                                        builder.link_to(
                                            idx,
                                            Node::AzureServiceTag(dest.as_str().into()),
                                            Edge::RoutesTo,
                                        );
                                    } else if !is_large_cidr(dest) {
                                        builder.link_to(
                                            idx,
                                            Node::GenericIpAddress(dest.as_str().into()),
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
        MicrosoftCollection::AzurePublicIpAddresses(pips) => {
            for pip in pips {
                if let Some(id) = &pip.id {
                    let idx =
                        builder.get_or_add_node(Node::AzurePublicIpAddress(id.as_str().into()));

                    if let Some(ip) = &pip.ip_address {
                        builder.link_to(
                            idx,
                            Node::GenericIpAddress(ip.as_str().into()),
                            Edge::ConnectsTo,
                        );
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
                    let app_idx =
                        builder.get_or_add_node(Node::AzureAppService(id.as_str().into()));

                    if let Some(props) = &app.properties
                        && let Some(hostname) = &props.default_host_name
                    {
                        builder.link_from(
                            app_idx,
                            Node::GenericHostname(hostname.as_str().into()),
                            Edge::RoutesTo,
                        );
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
    !tag.contains('.') && !tag.contains(':') && tag != "*"
}
