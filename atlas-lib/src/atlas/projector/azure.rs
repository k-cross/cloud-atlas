use super::{project_leaf, project_parallel};
use crate::api::azure::models::IpConfiguration;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::is_large_cidr;
use crate::cloud::definition::MicrosoftCollection;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::sync::Arc;

pub fn azure_projector(builder: &mut GraphBuilder, azure_data: &[MicrosoftCollection]) {
    project_parallel(builder, azure_data, project_microsoft_collection);
    link_rule_destinations(builder, azure_data);
}

// Resource Graph echoes one resource's id with whatever casing the JSON that
// mentions it happened to use — a VM's own top-level `id` and the same
// resource named inside another resource's `properties` are not guaranteed
// to agree — and Node identity is exact string equality (rule 6), so a
// networking resource's id has to be folded to one spelling before it
// becomes a node, everywhere it can be named from two directions. ARM ids
// are case-insensitive as identifiers (only the final segment's display
// casing varies), so lowercasing is safe. Leaf resources that nothing else
// ever names by id (`project_leaf!` below) have no second spelling to
// disagree with, so they are left as ARG returned them.
fn canonical_azure_id(id: &str) -> Arc<str> {
    id.to_ascii_lowercase().into()
}

fn project_microsoft_collection(builder: &mut GraphBuilder, x: &MicrosoftCollection) {
    match x {
        MicrosoftCollection::AzureVirtualMachines(vms) => {
            for vm in vms {
                if let Some(id) = &vm.id {
                    let idx =
                        builder.get_or_add_node(Node::AzureVirtualMachine(canonical_azure_id(id)));

                    // `HasIp`, not `ConnectsTo`: the NIC is what holds the
                    // machine's addresses, and `derive::service` resolves an
                    // address up through its holders to name the VM rather
                    // than the interface.
                    for nic_id in &vm.network_interfaces {
                        builder.link_to(
                            idx,
                            Node::AzureNetworkInterface(canonical_azure_id(nic_id)),
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
                        builder.get_or_add_node(Node::AzureVirtualNetwork(canonical_azure_id(id)));

                    for subnet_id in &vnet.subnets {
                        builder.link_to(
                            idx,
                            Node::AzureSubnet(canonical_azure_id(subnet_id)),
                            Edge::Contains,
                        );
                    }

                    for remote_id in vnet
                        .peerings
                        .iter()
                        .filter(|peering| {
                            peering
                                .state
                                .as_deref()
                                .is_some_and(|state| state.eq_ignore_ascii_case("Connected"))
                        })
                        .filter_map(|peering| peering.remote_id.as_deref())
                    {
                        builder.link_to(
                            idx,
                            Node::AzureVirtualNetwork(canonical_azure_id(remote_id)),
                            Edge::ConnectsTo,
                        );
                    }
                }
            }
        }
        MicrosoftCollection::AzureSubnets(subnets) => {
            for subnet in subnets {
                if let Some(id) = &subnet.id {
                    let idx = builder.get_or_add_node(Node::AzureSubnet(canonical_azure_id(id)));

                    if let Some(vnet_id) = &subnet.vnet_id {
                        builder.link_from(
                            idx,
                            Node::AzureVirtualNetwork(canonical_azure_id(vnet_id)),
                            Edge::Contains,
                        );
                    }

                    if let Some(nsg_id) = &subnet.network_security_group_id {
                        builder.link_to(
                            idx,
                            Node::AzureNetworkSecurityGroup(canonical_azure_id(nsg_id)),
                            Edge::ConnectsTo,
                        );
                    }
                }
            }
        }
        MicrosoftCollection::AzureNetworkInterfaces(nics) => {
            for nic in nics {
                if let Some(id) = &nic.id {
                    let id = canonical_azure_id(id);
                    let idx = builder.get_or_add_node(Node::AzureNetworkInterface(id.clone()));
                    let configs = nic
                        .properties
                        .as_ref()
                        .and_then(|p| p.ip_configurations.as_deref());
                    project_ip_configurations(builder, idx, &id, configs);
                }
            }
        }
        MicrosoftCollection::AzureLoadBalancers(balancers) => {
            for balancer in balancers {
                if let Some(id) = &balancer.id {
                    let id = canonical_azure_id(id);
                    let idx = builder.get_or_add_node(Node::AzureLoadBalancer(id.clone()));
                    let configs = balancer
                        .properties
                        .as_ref()
                        .and_then(|p| p.frontend_ip_configurations.as_deref());
                    project_ip_configurations(builder, idx, &id, configs);
                }
            }
        }
        MicrosoftCollection::AzureNetworkSecurityGroups(nsgs) => {
            for nsg in nsgs {
                if let Some(id) = &nsg.id {
                    builder
                        .get_or_add_node(Node::AzureNetworkSecurityGroup(canonical_azure_id(id)));
                }
            }
        }
        MicrosoftCollection::AzurePublicIpAddresses(pips) => {
            for pip in pips {
                if let Some(id) = &pip.id {
                    let id = canonical_azure_id(id);
                    let idx = builder.get_or_add_node(Node::AzurePublicIpAddress(id.clone()));

                    if let Some(ip) = &pip.ip_address {
                        builder.link_to(idx, Node::ip(ip, &id), Edge::ConnectsTo);
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
    owner: &Arc<str>,
    configs: Option<&[IpConfiguration]>,
) {
    for config in configs.unwrap_or_default() {
        let Some(properties) = &config.properties else {
            continue;
        };

        if let Some(private_ip) = &properties.private_ip_address {
            let network = properties
                .subnet
                .as_ref()
                .and_then(|r| r.id.as_deref())
                .and_then(virtual_network_of)
                .unwrap_or_else(|| owner.clone());
            builder.link_to(idx, Node::ip(private_ip, &network), Edge::ConnectsTo);
        }

        if let Some(public_ip_id) = properties
            .public_ip_address
            .as_ref()
            .and_then(|r| r.id.as_deref())
        {
            builder.link_to(
                idx,
                Node::AzurePublicIpAddress(canonical_azure_id(public_ip_id)),
                Edge::HasIp,
            );
        }

        if let Some(subnet_id) = properties.subnet.as_ref().and_then(|r| r.id.as_deref()) {
            builder.link_to(
                idx,
                Node::AzureSubnet(canonical_azure_id(subnet_id)),
                Edge::AttachedTo,
            );
        }
    }
}

fn virtual_network_of(subnet_id: &str) -> Option<Arc<str>> {
    let subnet_id = canonical_azure_id(subnet_id);
    subnet_id
        .find("/subnets/")
        .map(|at| Arc::from(&subnet_id[..at]))
}

fn link_rule_destinations(builder: &mut GraphBuilder, azure_data: &[MicrosoftCollection]) {
    let mut networks: HashMap<Arc<str>, Vec<Arc<str>>> = HashMap::new();
    for collection in azure_data {
        if let MicrosoftCollection::AzureSubnets(subnets) = collection {
            for subnet in subnets {
                if let (Some(nsg_id), Some(vnet_id)) =
                    (&subnet.network_security_group_id, &subnet.vnet_id)
                {
                    networks
                        .entry(canonical_azure_id(nsg_id))
                        .or_default()
                        .push(canonical_azure_id(vnet_id));
                }
            }
        }
    }
    for scopes in networks.values_mut() {
        scopes.sort();
        scopes.dedup();
    }

    for collection in azure_data {
        let MicrosoftCollection::AzureNetworkSecurityGroups(nsgs) = collection else {
            continue;
        };
        for nsg in nsgs {
            let Some(id) = &nsg.id else {
                continue;
            };
            let id = canonical_azure_id(id);
            let scopes = networks
                .get(&id)
                .cloned()
                .unwrap_or_else(|| vec![id.clone()]);
            let idx = builder.get_or_add_node(Node::AzureNetworkSecurityGroup(id));

            let outbound = nsg
                .properties
                .iter()
                .flat_map(|props| props.security_rules.iter().flatten())
                .filter_map(|rule| rule.properties.as_ref())
                .filter(|rprops| {
                    rprops
                        .direction
                        .as_deref()
                        .is_some_and(|d| d.eq_ignore_ascii_case("Outbound"))
                });
            for rprops in outbound {
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
                        for scope in &scopes {
                            builder.link_to(idx, Node::ip(dest, scope), Edge::RoutesTo);
                        }
                    }
                }
            }
        }
    }
}

fn is_service_tag(tag: &str) -> bool {
    !tag.contains('.') && !tag.contains(':') && tag != "*"
}
