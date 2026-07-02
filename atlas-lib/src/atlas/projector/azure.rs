use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::cloud::definition::MicrosoftCollection;

pub fn azure_projector(builder: &mut GraphBuilder, azure_data: &[MicrosoftCollection]) {
    for x in azure_data {
        match x {
            MicrosoftCollection::AzureVirtualMachines(vms) => {
                for vm in vms {
                    if let Some(id) = &vm.id {
                        let node = Node::AzureVirtualMachine(id.to_string().into());
                        let idx = builder.get_or_add_node(node);

                        for nic_id in &vm.network_interfaces {
                            let nic_node =
                                Node::AzureNetworkSecurityGroup(nic_id.to_string().into());
                            let nic_idx = builder.get_or_add_node(nic_node);
                            builder.add_edge(idx, nic_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureVirtualNetworks(vnets) => {
                for vnet in vnets {
                    if let Some(id) = &vnet.id {
                        let node = Node::AzureVirtualNetwork(id.to_string().into());
                        let idx = builder.get_or_add_node(node);

                        for subnet_id in &vnet.subnets {
                            let subnet_node = Node::AzureSubnet(subnet_id.to_string().into());
                            let subnet_idx = builder.get_or_add_node(subnet_node);
                            builder.add_edge(idx, subnet_idx, Edge::Contains);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureSubnets(subnets) => {
                for subnet in subnets {
                    if let Some(id) = &subnet.id {
                        let node = Node::AzureSubnet(id.to_string().into());
                        let idx = builder.get_or_add_node(node);

                        if let Some(vnet_id) = &subnet.vnet_id {
                            let vnet_node = Node::AzureVirtualNetwork(vnet_id.to_string().into());
                            let v_idx = builder.get_or_add_node(vnet_node);
                            builder.add_edge(v_idx, idx, Edge::Contains);
                        }

                        if let Some(nsg_id) = &subnet.network_security_group_id {
                            let nsg_node =
                                Node::AzureNetworkSecurityGroup(nsg_id.to_string().into());
                            let nsg_idx = builder.get_or_add_node(nsg_node);
                            builder.add_edge(idx, nsg_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureNetworkSecurityGroups(nsgs) => {
                for nsg in nsgs {
                    if let Some(id) = &nsg.id {
                        let node = Node::AzureNetworkSecurityGroup(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzurePublicIpAddresses(pips) => {
                for pip in pips {
                    if let Some(id) = &pip.id {
                        let node = Node::AzurePublicIpAddress(id.to_string().into());
                        let idx = builder.get_or_add_node(node);

                        if let Some(ip) = &pip.ip_address {
                            let ip_node = Node::GenericIpAddress(ip.to_string().into());
                            let ip_idx = builder.get_or_add_node(ip_node);
                            builder.add_edge(idx, ip_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureStorageAccounts(accounts) => {
                for account in accounts {
                    if let Some(id) = &account.id {
                        let node = Node::AzureStorageAccount(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureManagedClusters(clusters) => {
                for cluster in clusters {
                    if let Some(id) = &cluster.id {
                        let node = Node::AzureManagedCluster(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureSqlServers(servers) => {
                for server in servers {
                    if let Some(id) = &server.id {
                        let node = Node::AzureSqlServer(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureAppServices(apps) => {
                for app in apps {
                    if let Some(id) = &app.id {
                        let node = Node::AzureAppService(id.to_string().into());
                        let app_idx = builder.get_or_add_node(node);

                        if let Some(props) = &app.properties
                            && let Some(hostname) = &props.default_host_name
                        {
                            let pivot_node = Node::GenericHostname(hostname.to_string().into());
                            let pivot_idx = builder.get_or_add_node(pivot_node);
                            builder.add_edge(pivot_idx, app_idx, Edge::RoutesTo);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureFunctionApps(funcs) => {
                for func in funcs {
                    if let Some(id) = &func.id {
                        let node = Node::AzureFunctionApp(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureApiManagement(apims) => {
                for apim in apims {
                    if let Some(id) = &apim.id {
                        let node = Node::AzureApiManagement(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureCosmosDbs(cosmos) => {
                for db in cosmos {
                    if let Some(id) = &db.id {
                        let node = Node::AzureCosmosDb(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureServiceBuses(sbuses) => {
                for bus in sbuses {
                    if let Some(id) = &bus.id {
                        let node = Node::AzureServiceBus(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureEventGridTopics(egrids) => {
                for topic in egrids {
                    if let Some(id) = &topic.id {
                        let node = Node::AzureEventGridTopic(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureDnsZones(dns) => {
                for zone in dns {
                    if let Some(id) = &zone.id {
                        let node = Node::AzureDnsZone(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureCdnProfiles(cdns) => {
                for cdn in cdns {
                    if let Some(id) = &cdn.id {
                        let node = Node::AzureCdnProfile(id.to_string().into());
                        builder.get_or_add_node(node);
                    }
                }
            }
        }
    }
}
