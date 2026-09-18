use super::{project_leaf, project_parallel};
use crate::api::azure::models::IpConfiguration;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::MicrosoftCollection;
use petgraph::graph::NodeIndex;

pub fn azure_projector(builder: &mut GraphBuilder, azure_data: &[MicrosoftCollection]) {
    project_parallel(builder, azure_data, project_microsoft_collection);
}

fn project_microsoft_collection(builder: &mut GraphBuilder, x: &MicrosoftCollection) {
    match x {
        MicrosoftCollection::AzureVirtualMachines(vms) => {
            for vm in vms {
                if let Some(id) = &vm.id {
                    let idx =
                        builder.get_or_add_node(Node::AzureVirtualMachine(id.as_str().into()));

                    // `HasIp`, not `ConnectsTo`: the NIC is what holds the
                    // machine's addresses, and `derive::service` resolves an
                    // address up through its holders to name the VM rather
                    // than the interface.
                    for nic_id in &vm.network_interfaces {
                        builder.link_to(
                            idx,
                            Node::AzureNetworkInterface(nic_id.as_str().into()),
                            Edge::HasIp,
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
        MicrosoftCollection::AzureNetworkInterfaces(nics) => {
            for nic in nics {
                if let Some(id) = &nic.id {
                    let idx =
                        builder.get_or_add_node(Node::AzureNetworkInterface(id.as_str().into()));
                    let configs = nic
                        .properties
                        .as_ref()
                        .and_then(|p| p.ip_configurations.as_deref());
                    project_ip_configurations(builder, idx, configs);
                }
            }
        }
        MicrosoftCollection::AzureLoadBalancers(balancers) => {
            for balancer in balancers {
                if let Some(id) = &balancer.id {
                    let idx = builder.get_or_add_node(Node::AzureLoadBalancer(id.as_str().into()));
                    let configs = balancer
                        .properties
                        .as_ref()
                        .and_then(|p| p.frontend_ip_configurations.as_deref());
                    project_ip_configurations(builder, idx, configs);
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
                                        builder.link_to(idx, Node::ip(dest), Edge::RoutesTo);
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
                        builder.link_to(idx, Node::ip(ip), Edge::ConnectsTo);
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
                        builder.link_from(app_idx, Node::hostname(hostname), Edge::RoutesTo);
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

// A NIC's ip configurations and a load balancer's frontend ones say the same
// three things: the address the resource holds, the public address object
// associated with it, and the subnet it sits in. The public address is a
// resource of its own, so the association is `HasIp` — the pivot stays under
// the address object and resolves up to whatever holds the interface.
fn project_ip_configurations(
    builder: &mut GraphBuilder,
    idx: NodeIndex,
    configs: Option<&[IpConfiguration]>,
) {
    for config in configs.unwrap_or_default() {
        let Some(properties) = &config.properties else {
            continue;
        };

        if let Some(private_ip) = &properties.private_ip_address {
            builder.link_to(idx, Node::ip(private_ip), Edge::ConnectsTo);
        }

        if let Some(public_ip_id) = properties
            .public_ip_address
            .as_ref()
            .and_then(|r| r.id.as_deref())
        {
            builder.link_to(
                idx,
                Node::AzurePublicIpAddress(public_ip_id.into()),
                Edge::HasIp,
            );
        }

        if let Some(subnet_id) = properties.subnet.as_ref().and_then(|r| r.id.as_deref()) {
            builder.link_to(idx, Node::AzureSubnet(subnet_id.into()), Edge::AttachedTo);
        }
    }
}

fn is_service_tag(tag: &str) -> bool {
    !tag.contains('.') && !tag.contains(':') && tag != "*"
}
