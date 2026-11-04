use super::{link_bgp_session, project_leaf, project_parallel};
use crate::api::azure::models::{
    ExpressRouteCircuit, ExpressRoutePeering, IpConfiguration, in_service,
};
use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::atlas::util::{
    format_address, is_large_cidr, mask_to_prefix, offset_address, parse_address,
};
use crate::cloud::definition::MicrosoftCollection;
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub fn azure_projector(builder: &mut GraphBuilder, azure_data: &[MicrosoftCollection]) {
    let skipped = skipped_resources(azure_data);
    let basic_wans = basic_wans(azure_data);
    project_parallel(builder, azure_data, |local, collection| {
        project_microsoft_collection(local, collection, &skipped, &basic_wans)
    });
    link_rule_destinations(builder, azure_data);
}

fn basic_wans(azure_data: &[MicrosoftCollection]) -> HashSet<Arc<str>> {
    azure_data
        .iter()
        .filter_map(|collection| match collection {
            MicrosoftCollection::AzureVirtualWans(wans) => Some(wans),
            _ => None,
        })
        .flatten()
        .filter(|wan| {
            wan.wan_type
                .as_deref()
                .is_some_and(|wan_type| wan_type.eq_ignore_ascii_case("Basic"))
        })
        .filter_map(|wan| wan.id.as_deref().map(canonical_azure_id))
        .collect()
}

fn skipped_resources(azure_data: &[MicrosoftCollection]) -> HashSet<Arc<str>> {
    let mut skipped = HashSet::new();
    for collection in azure_data {
        match collection {
            MicrosoftCollection::AzureVirtualNetworkGateways(gateways) => skipped.extend(
                gateways
                    .iter()
                    .filter(|gateway| !in_service(gateway.provisioning_state.as_deref()))
                    .filter_map(|gateway| gateway.id.as_deref())
                    .map(canonical_azure_id),
            ),
            MicrosoftCollection::AzureLocalNetworkGateways(gateways) => skipped.extend(
                gateways
                    .iter()
                    .filter(|gateway| !in_service(gateway.provisioning_state.as_deref()))
                    .filter_map(|gateway| gateway.id.as_deref())
                    .map(canonical_azure_id),
            ),
            MicrosoftCollection::AzureVirtualWans(wans) => skipped.extend(
                wans.iter()
                    .filter(|wan| !in_service(wan.provisioning_state.as_deref()))
                    .filter_map(|wan| wan.id.as_deref())
                    .map(canonical_azure_id),
            ),
            MicrosoftCollection::AzureVirtualHubs(hubs) => skipped.extend(
                hubs.iter()
                    .filter(|hub| !in_service(hub.provisioning_state.as_deref()))
                    .filter_map(|hub| hub.id.as_deref())
                    .map(canonical_azure_id),
            ),
            MicrosoftCollection::AzureExpressRouteCircuits(circuits) => skipped.extend(
                circuits
                    .iter()
                    .filter(|circuit| !circuit_in_service(circuit))
                    .filter_map(|circuit| circuit.id.as_deref())
                    .map(canonical_azure_id),
            ),
            _ => {}
        }
    }
    skipped
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

fn project_microsoft_collection(
    builder: &mut GraphBuilder,
    x: &MicrosoftCollection,
    skipped: &HashSet<Arc<str>>,
    basic_wans: &HashSet<Arc<str>>,
) {
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
                    let scope = canonical_azure_id(id);
                    let idx = builder.get_or_add_node(Node::AzureVirtualNetwork(scope.clone()));

                    for prefix in &vnet.address_prefixes {
                        builder.link_to(idx, Node::ip(prefix, &scope), Edge::ConnectsTo);
                    }

                    for subnet_id in &vnet.subnets {
                        builder.link_to(
                            idx,
                            Node::AzureSubnet(canonical_azure_id(subnet_id)),
                            Edge::Contains,
                        );
                    }

                    for (remote_id, use_remote_gateways) in vnet
                        .peerings
                        .iter()
                        .filter(|peering| {
                            peering
                                .state
                                .as_deref()
                                .is_some_and(|state| state.eq_ignore_ascii_case("Connected"))
                        })
                        .filter_map(|peering| {
                            Some((peering.remote_id.as_deref()?, peering.use_remote_gateways))
                        })
                    {
                        let remote = Node::AzureVirtualNetwork(canonical_azure_id(remote_id));
                        builder.link_to(idx, remote.clone(), Edge::ConnectsTo);
                        if use_remote_gateways {
                            builder.link_to(idx, remote, Edge::RoutesTo);
                        }
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
        MicrosoftCollection::AzureVirtualNetworkGateways(gateways) => {
            for gateway in gateways
                .iter()
                .filter(|gateway| in_service(gateway.provisioning_state.as_deref()))
            {
                let Some(id) = &gateway.id else { continue };
                let idx = builder
                    .get_or_add_node(Node::AzureVirtualNetworkGateway(canonical_azure_id(id)));
                for vnet in gateway
                    .subnet_ids
                    .iter()
                    .filter_map(|subnet_id| virtual_network_of(subnet_id))
                {
                    builder.link_to(idx, Node::AzureVirtualNetwork(vnet), Edge::AttachedTo);
                }
                for public_ip_id in &gateway.public_ip_ids {
                    builder.link_to(
                        idx,
                        Node::AzurePublicIpAddress(canonical_azure_id(public_ip_id)),
                        Edge::HasIp,
                    );
                }
            }
        }
        MicrosoftCollection::AzureLocalNetworkGateways(gateways) => {
            for gateway in gateways
                .iter()
                .filter(|gateway| in_service(gateway.provisioning_state.as_deref()))
            {
                let Some(id) = &gateway.id else { continue };
                let id = canonical_azure_id(id);
                let idx = builder.get_or_add_node(Node::AzureLocalNetworkGateway(id.clone()));
                if let Some(address) = &gateway.gateway_ip_address {
                    builder.link_to(idx, Node::ip(address, &id), Edge::RoutesTo);
                }
            }
        }
        MicrosoftCollection::AzureGatewayConnections(connections) => {
            for connection in connections
                .iter()
                .filter(|connection| in_service(connection.provisioning_state.as_deref()))
            {
                let Some(id) = &connection.id else { continue };
                let idx =
                    builder.get_or_add_node(Node::AzureGatewayConnection(canonical_azure_id(id)));
                let drawn = |reference: &Option<String>| {
                    reference
                        .as_deref()
                        .map(canonical_azure_id)
                        .filter(|id| !skipped.contains(id))
                };
                if let Some(gateway_id) = drawn(&connection.gateway_id) {
                    builder.link_to(
                        idx,
                        Node::AzureVirtualNetworkGateway(gateway_id),
                        Edge::AttachedTo,
                    );
                }
                if let Some(local_id) = drawn(&connection.local_gateway_id) {
                    builder.link_to(
                        idx,
                        Node::AzureLocalNetworkGateway(local_id),
                        Edge::AttachedTo,
                    );
                }
                if let Some(circuit_id) = drawn(&connection.circuit_id) {
                    builder.link_to(
                        idx,
                        Node::AzureExpressRouteCircuit(circuit_id),
                        Edge::AttachedTo,
                    );
                }
            }
        }
        MicrosoftCollection::AzureVirtualWans(wans) => {
            for id in wans
                .iter()
                .filter(|wan| in_service(wan.provisioning_state.as_deref()))
                .filter_map(|wan| wan.id.as_deref())
            {
                builder.get_or_add_node(Node::AzureVirtualWan(canonical_azure_id(id)));
            }
        }
        MicrosoftCollection::AzureVirtualHubs(hubs) => {
            for hub in hubs
                .iter()
                .filter(|hub| in_service(hub.provisioning_state.as_deref()))
            {
                let Some(id) = hub.id.as_deref() else {
                    continue;
                };
                let hub_idx =
                    builder.get_or_add_node(Node::AzureVirtualHub(canonical_azure_id(id)));
                if let Some(wan) = hub
                    .virtual_wan_id
                    .as_deref()
                    .map(canonical_azure_id)
                    .filter(|wan| !skipped.contains(wan))
                {
                    if basic_wans.contains(&wan) {
                        builder.link_from(hub_idx, Node::AzureVirtualWan(wan), Edge::Contains);
                    } else {
                        builder.link_to(hub_idx, Node::AzureVirtualWan(wan), Edge::AttachedTo);
                    }
                }
            }
        }
        MicrosoftCollection::AzureHubVirtualNetworkConnections(connections) => {
            for connection in connections
                .iter()
                .filter(|connection| in_service(connection.provisioning_state.as_deref()))
            {
                let (Some(hub), Some(vnet)) = (
                    connection
                        .hub_id
                        .as_deref()
                        .map(canonical_azure_id)
                        .filter(|hub| !skipped.contains(hub)),
                    connection.remote_virtual_network_id.as_deref(),
                ) else {
                    continue;
                };
                let hub_idx = builder.get_or_add_node(Node::AzureVirtualHub(hub));
                builder.link_to(
                    hub_idx,
                    Node::AzureVirtualNetwork(canonical_azure_id(vnet)),
                    Edge::AttachedTo,
                );
            }
        }
        MicrosoftCollection::AzureExpressRouteCircuits(circuits) => {
            for circuit in circuits
                .iter()
                .filter(|circuit| circuit_in_service(circuit))
            {
                let Some(id) = &circuit.id else { continue };
                let idx =
                    builder.get_or_add_node(Node::AzureExpressRouteCircuit(canonical_azure_id(id)));
                for prefix in circuit
                    .peerings
                    .iter()
                    .filter(|peering| private_peering_enabled(peering))
                    .flat_map(|peering| &peering.peer_address_prefixes)
                {
                    if let Some((customer, microsoft)) = peering_addresses(prefix) {
                        link_bgp_session(builder, idx, microsoft, customer);
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

fn circuit_in_service(circuit: &ExpressRouteCircuit) -> bool {
    in_service(circuit.provisioning_state.as_deref())
        && provider_provisioned(circuit.service_provider_provisioning_state.as_deref())
}

fn provider_provisioned(state: Option<&str>) -> bool {
    !state.is_some_and(|state| {
        ["NotProvisioned", "Provisioning", "Deprovisioning"]
            .iter()
            .any(|pending| state.eq_ignore_ascii_case(pending))
    })
}

fn private_peering_enabled(peering: &ExpressRoutePeering) -> bool {
    peering
        .peering_type
        .as_deref()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("AzurePrivatePeering"))
        && !peering
            .state
            .as_deref()
            .is_some_and(|state| state.eq_ignore_ascii_case("Disabled"))
}

fn peering_addresses(prefix: &str) -> Option<(Node, Node)> {
    let (address, Some(width)) = parse_address(prefix)? else {
        return None;
    };
    let link = mask_to_prefix(&address, width)?;
    let host = |offset| {
        let host = offset_address(&link, offset)?;
        (mask_to_prefix(&host, width)? == link)
            .then(|| Node::link_ip(&format_address(&host, Some(width))))
            .flatten()
    };
    Some((host(1)?, host(2)?))
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
