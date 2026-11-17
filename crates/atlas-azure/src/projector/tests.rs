use super::*;
use crate::fixtures::{self, azure_id, azure_network, azure_networking_id};
use atlas_graph::testing::{assert_edge, assert_has_node, link, public};

#[test]
fn azure_projection() {
    let mut builder = GraphBuilder::new();
    azure_projector(&mut builder, &fixtures::collections());

    let vm = Node::AzureVirtualMachine(azure_networking_id(
        "Microsoft.Compute/virtualMachines/vm-worker",
    ));
    let nic = Node::AzureNetworkInterface(azure_networking_id(
        "Microsoft.Network/networkInterfaces/nic-worker",
    ));
    let vnet = Node::AzureVirtualNetwork(azure_networking_id(
        "Microsoft.Network/virtualNetworks/vnet-globex",
    ));
    let subnet = Node::AzureSubnet(azure_networking_id(
        "Microsoft.Network/virtualNetworks/vnet-globex/subnets/default",
    ));
    let nsg = Node::AzureNetworkSecurityGroup(azure_networking_id(
        "Microsoft.Network/networkSecurityGroups/nsg-worker",
    ));

    assert_edge(&builder, &vm, &nic, &Edge::HasIp);
    assert_edge(&builder, &vnet, &subnet, &Edge::Contains);

    let hub = Node::AzureVirtualNetwork(azure_networking_id(
        "Microsoft.Network/virtualNetworks/vnet-globex-hub",
    ));
    assert_edge(&builder, &vnet, &hub, &Edge::ConnectsTo);
    assert_edge(&builder, &hub, &vnet, &Edge::ConnectsTo);
    assert!(
        !builder.contains(&Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-globex-dr",
        ))),
        "a peering only one side has created joins nothing"
    );
    assert_edge(&builder, &subnet, &nsg, &Edge::ConnectsTo);

    assert_edge(
        &builder,
        &nic,
        &Node::ip("10.20.1.4", &azure_network()),
        &Edge::ConnectsTo,
    );
    assert_edge(&builder, &nic, &subnet, &Edge::AttachedTo);

    let balancer = Node::AzureLoadBalancer(azure_networking_id(
        "Microsoft.Network/loadBalancers/lb-globex",
    ));
    let pip = Node::AzurePublicIpAddress(azure_networking_id(
        "Microsoft.Network/publicIPAddresses/pip-globex",
    ));
    assert_edge(&builder, &balancer, &pip, &Edge::HasIp);

    assert_edge(
        &builder,
        &nsg,
        &Node::ip("10.20.0.5", &azure_network()),
        &Edge::RoutesTo,
    );
    assert_edge(
        &builder,
        &nsg,
        &Node::AzureServiceTag("AzureCloud".into()),
        &Edge::RoutesTo,
    );

    assert_edge(
        &builder,
        &pip,
        &Node::GenericIpAddress("198.51.100.10".into()),
        &Edge::ConnectsTo,
    );

    assert_edge(
        &builder,
        &Node::GenericHostname("app-globex.azurewebsites.net".into()),
        &Node::AzureAppService(azure_id("Microsoft.Web/sites/app-globex").as_str().into()),
        &Edge::RoutesTo,
    );

    for node in [
        Node::AzureStorageAccount(
            azure_id("Microsoft.Storage/storageAccounts/globexstore")
                .as_str()
                .into(),
        ),
        Node::AzureManagedCluster(
            azure_id("Microsoft.ContainerService/managedClusters/aks-globex")
                .as_str()
                .into(),
        ),
        Node::AzureSqlServer(azure_id("Microsoft.Sql/servers/sql-globex").as_str().into()),
        Node::AzureFunctionApp(azure_id("Microsoft.Web/sites/func-globex").as_str().into()),
        Node::AzureApiManagement(
            azure_id("Microsoft.ApiManagement/service/apim-globex")
                .as_str()
                .into(),
        ),
        Node::AzureCosmosDb(
            azure_id("Microsoft.DocumentDB/databaseAccounts/cosmos-globex")
                .as_str()
                .into(),
        ),
        Node::AzureServiceBus(
            azure_id("Microsoft.ServiceBus/namespaces/sb-globex")
                .as_str()
                .into(),
        ),
        Node::AzureEventGridTopic(
            azure_id("Microsoft.EventGrid/topics/eg-globex")
                .as_str()
                .into(),
        ),
        Node::AzureDnsZone(
            azure_id("Microsoft.Network/dnsZones/globex.azure")
                .as_str()
                .into(),
        ),
        Node::AzureCdnProfile(
            azure_id("Microsoft.Cdn/profiles/cdn-globex")
                .as_str()
                .into(),
        ),
    ] {
        assert_has_node(&builder, &node);
    }
}

#[test]
fn an_unconnected_peering_does_not_ride_the_hub_gateway() {
    use crate::api::models::{VirtualNetwork, VirtualNetworkPeering};
    use crate::definition::MicrosoftCollection;

    let remote = azure_networking_id("Microsoft.Network/virtualNetworks/hub");
    let mut builder = GraphBuilder::new();
    azure_projector(
        &mut builder,
        &[MicrosoftCollection::AzureVirtualNetworks(vec![
            VirtualNetwork {
                id: Some(
                    azure_networking_id("Microsoft.Network/virtualNetworks/spoke").to_string(),
                ),
                peerings: vec![VirtualNetworkPeering {
                    remote_id: Some(remote.to_string()),
                    state: Some("Initiated".to_owned()),
                    use_remote_gateways: true,
                }],
                ..Default::default()
            },
        ])],
    );

    let spoke = Node::AzureVirtualNetwork(azure_networking_id(
        "Microsoft.Network/virtualNetworks/spoke",
    ));
    assert!(!builder.has_edge(&spoke, &Node::AzureVirtualNetwork(remote), &Edge::RoutesTo));
}

#[test]
fn an_azure_vpn_being_deleted_or_failed_is_not_projected() {
    use crate::api::models::{GatewayConnection, LocalNetworkGateway, VirtualNetworkGateway};
    use crate::definition::MicrosoftCollection;

    let id = |rest: &str| Some(azure_networking_id(rest).to_string());
    let gateway = |name: &str, state: &str| VirtualNetworkGateway {
        id: id(&format!("Microsoft.Network/virtualNetworkGateways/{name}")),
        provisioning_state: Some(state.to_owned()),
        subnet_ids: vec![
            azure_networking_id(&format!(
                "Microsoft.Network/virtualNetworks/{name}/subnets/GatewaySubnet"
            ))
            .to_string(),
        ],
        ..Default::default()
    };
    let mut builder = GraphBuilder::new();
    azure_projector(
        &mut builder,
        &[
            MicrosoftCollection::AzureVirtualNetworkGateways(vec![
                gateway("updating", "Updating"),
                gateway("deleting", "Deleting"),
                gateway("failed", "failed"),
            ]),
            MicrosoftCollection::AzureLocalNetworkGateways(vec![LocalNetworkGateway {
                id: id("Microsoft.Network/localNetworkGateways/deleting"),
                gateway_ip_address: Some("192.0.2.50".to_owned()),
                provisioning_state: Some("Deleting".to_owned()),
                ..Default::default()
            }]),
            MicrosoftCollection::AzureGatewayConnections(vec![
                GatewayConnection {
                    id: id("Microsoft.Network/connections/failed"),
                    provisioning_state: Some("Failed".to_owned()),
                    gateway_id: id("Microsoft.Network/virtualNetworkGateways/updating"),
                    ..Default::default()
                },
                GatewayConnection {
                    id: id("Microsoft.Network/connections/to-deleting"),
                    provisioning_state: Some("Succeeded".to_owned()),
                    gateway_id: id("Microsoft.Network/virtualNetworkGateways/deleting"),
                    local_gateway_id: id("Microsoft.Network/localNetworkGateways/deleting"),
                    ..Default::default()
                },
            ]),
        ],
    );

    assert_edge(
        &builder,
        &Node::AzureVirtualNetworkGateway(azure_networking_id(
            "Microsoft.Network/virtualNetworkGateways/updating",
        )),
        &Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/updating",
        )),
        &Edge::AttachedTo,
    );
    assert_has_node(
        &builder,
        &Node::AzureGatewayConnection(azure_networking_id(
            "Microsoft.Network/connections/to-deleting",
        )),
    );
    for gone in [
        Node::AzureVirtualNetworkGateway(azure_networking_id(
            "Microsoft.Network/virtualNetworkGateways/deleting",
        )),
        Node::AzureVirtualNetworkGateway(azure_networking_id(
            "Microsoft.Network/virtualNetworkGateways/failed",
        )),
        Node::AzureLocalNetworkGateway(azure_networking_id(
            "Microsoft.Network/localNetworkGateways/deleting",
        )),
        Node::AzureGatewayConnection(azure_networking_id("Microsoft.Network/connections/failed")),
    ] {
        assert!(!builder.contains(&gone), "{gone:?} is going away");
    }
    assert!(!builder.contains(&public("192.0.2.50")));
}

#[test]
fn an_expressroute_circuit_holds_the_second_address_of_each_peering_and_names_the_first() {
    use crate::api::models::{
        ExpressRouteCircuit, ExpressRoutePeering, GatewayConnection, VirtualNetworkGateway,
    };
    use crate::definition::MicrosoftCollection;

    let id = |rest: &str| Some(azure_networking_id(rest).to_string());
    let peering = |kind: &str, state: &str, prefixes: &[&str]| ExpressRoutePeering {
        peering_type: Some(kind.to_owned()),
        state: Some(state.to_owned()),
        peer_address_prefixes: prefixes.iter().map(|prefix| prefix.to_string()).collect(),
    };
    let circuit = |name: &str, provisioning: &str, provider: &str| ExpressRouteCircuit {
        id: id(&format!("Microsoft.Network/expressRouteCircuits/{name}")),
        provisioning_state: Some(provisioning.to_owned()),
        service_provider_provisioning_state: Some(provider.to_owned()),
        peerings: vec![
            peering(
                "AzurePrivatePeering",
                "Enabled",
                &["192.168.15.16/30", "192.168.15.20/30"],
            ),
            peering("MicrosoftPeering", "Enabled", &["203.0.113.0/30"]),
            peering("AzurePrivatePeering", "Disabled", &["fd00:15::/126"]),
        ],
        ..Default::default()
    };
    let mut builder = GraphBuilder::new();
    azure_projector(
        &mut builder,
        &[
            MicrosoftCollection::AzureVirtualNetworkGateways(vec![VirtualNetworkGateway {
                id: id("Microsoft.Network/virtualNetworkGateways/ergw"),
                provisioning_state: Some("Succeeded".to_owned()),
                subnet_ids: vec![
                    azure_networking_id(
                        "Microsoft.Network/virtualNetworks/hub/subnets/GatewaySubnet",
                    )
                    .to_string(),
                ],
                ..Default::default()
            }]),
            MicrosoftCollection::AzureGatewayConnections(
                ["live", "unprovisioned", "deleting"]
                    .into_iter()
                    .map(|circuit| GatewayConnection {
                        id: id(&format!("Microsoft.Network/connections/er-{circuit}")),
                        provisioning_state: Some("Succeeded".to_owned()),
                        gateway_id: id("Microsoft.Network/virtualNetworkGateways/ergw"),
                        circuit_id: id(&format!(
                            "Microsoft.Network/expressRouteCircuits/{circuit}"
                        )),
                        ..Default::default()
                    })
                    .collect(),
            ),
            MicrosoftCollection::AzureExpressRouteCircuits(vec![
                circuit("live", "Succeeded", "Provisioned"),
                circuit("unprovisioned", "Succeeded", "NotProvisioned"),
                circuit("deleting", "Deleting", "Provisioned"),
            ]),
        ],
    );

    let gateway = Node::AzureVirtualNetworkGateway(azure_networking_id(
        "Microsoft.Network/virtualNetworkGateways/ergw",
    ));
    let live = Node::AzureExpressRouteCircuit(azure_networking_id(
        "Microsoft.Network/expressRouteCircuits/live",
    ));
    assert_edge(
        &builder,
        &gateway,
        &Node::AzureVirtualNetwork(azure_networking_id("Microsoft.Network/virtualNetworks/hub")),
        &Edge::AttachedTo,
    );
    let connection =
        Node::AzureGatewayConnection(azure_networking_id("Microsoft.Network/connections/er-live"));
    assert_edge(&builder, &connection, &gateway, &Edge::AttachedTo);
    assert_edge(&builder, &connection, &live, &Edge::AttachedTo);
    for (microsoft, customer) in [
        ("192.168.15.18/30", "192.168.15.17/30"),
        ("192.168.15.22/30", "192.168.15.21/30"),
    ] {
        assert_edge(&builder, &live, &link(microsoft), &Edge::ConnectsTo);
        assert_edge(&builder, &live, &link(customer), &Edge::RoutesTo);
    }
    for absent in [
        public("203.0.113.2"),
        link("fd00:15::2/126"),
        Node::AzureExpressRouteCircuit(azure_networking_id(
            "Microsoft.Network/expressRouteCircuits/unprovisioned",
        )),
        Node::AzureExpressRouteCircuit(azure_networking_id(
            "Microsoft.Network/expressRouteCircuits/deleting",
        )),
    ] {
        assert!(!builder.contains(&absent), "{absent:?} is not projected");
    }
}

#[test]
fn a_virtual_wan_draws_nothing_that_is_going_away() {
    use crate::api::models::{HubVirtualNetworkConnection, VirtualHub, VirtualWan};
    use crate::definition::MicrosoftCollection;

    let id = |rest: &str| fixtures::azure_id(rest);
    let hub = |name: &str, state: &str| VirtualHub {
        id: Some(id(&format!("Microsoft.Network/virtualHubs/{name}"))),
        virtual_wan_id: Some(id("Microsoft.Network/virtualWans/wan")),
        provisioning_state: Some(state.to_owned()),
        ..Default::default()
    };
    let connection = |hub: &str, vnet: &str, state: &str| HubVirtualNetworkConnection {
        hub_id: Some(id(&format!("Microsoft.Network/virtualHubs/{hub}"))),
        remote_virtual_network_id: Some(id(&format!("Microsoft.Network/virtualNetworks/{vnet}"))),
        provisioning_state: Some(state.to_owned()),
        ..Default::default()
    };
    let mut builder = GraphBuilder::new();
    azure_projector(
        &mut builder,
        &[
            MicrosoftCollection::AzureVirtualWans(vec![VirtualWan {
                id: Some(id("Microsoft.Network/virtualWans/wan")),
                provisioning_state: Some("Succeeded".to_owned()),
                ..Default::default()
            }]),
            MicrosoftCollection::AzureVirtualHubs(vec![
                hub("hub-live", "Succeeded"),
                hub("hub-going", "Deleting"),
            ]),
            MicrosoftCollection::AzureHubVirtualNetworkConnections(vec![
                connection("hub-live", "vnet-a", "Succeeded"),
                connection("hub-live", "vnet-b", "Deleting"),
                connection("hub-going", "vnet-c", "Succeeded"),
            ]),
        ],
    );

    let live = Node::AzureVirtualHub(azure_networking_id(
        "Microsoft.Network/virtualHubs/hub-live",
    ));
    assert_edge(
        &builder,
        &live,
        &Node::AzureVirtualWan(azure_networking_id("Microsoft.Network/virtualWans/wan")),
        &Edge::AttachedTo,
    );
    assert_edge(
        &builder,
        &live,
        &Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-a",
        )),
        &Edge::AttachedTo,
    );
    for absent in [
        Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-b",
        )),
        Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-c",
        )),
        Node::AzureVirtualHub(azure_networking_id(
            "Microsoft.Network/virtualHubs/hub-going",
        )),
    ] {
        assert!(!builder.contains(&absent), "{absent:?} is not projected");
    }
}

#[test]
fn only_a_standard_virtual_wan_carries_traffic_between_its_networks() {
    use crate::api::models::{HubVirtualNetworkConnection, VirtualHub, VirtualWan};
    use crate::definition::MicrosoftCollection;
    use atlas_graph::derive::held_addresses;

    let vnet =
        |name: &str| azure_networking_id(&format!("Microsoft.Network/virtualNetworks/{name}"));
    let project = |wan_type: &str| {
        let mut builder = GraphBuilder::new();
        azure_projector(
            &mut builder,
            &[
                MicrosoftCollection::AzureVirtualWans(vec![VirtualWan {
                    id: Some(azure_id("Microsoft.Network/virtualWans/wan")),
                    wan_type: Some(wan_type.to_owned()),
                    ..Default::default()
                }]),
                MicrosoftCollection::AzureVirtualHubs(vec![VirtualHub {
                    id: Some(azure_id("Microsoft.Network/virtualHubs/hub")),
                    virtual_wan_id: Some(azure_id("Microsoft.Network/virtualWans/wan")),
                    ..Default::default()
                }]),
                MicrosoftCollection::AzureHubVirtualNetworkConnections(
                    ["vnet-a", "vnet-b"]
                        .iter()
                        .map(|name| HubVirtualNetworkConnection {
                            hub_id: Some(azure_id("Microsoft.Network/virtualHubs/hub")),
                            remote_virtual_network_id: Some(azure_id(&format!(
                                "Microsoft.Network/virtualNetworks/{name}"
                            ))),
                            ..Default::default()
                        })
                        .collect(),
                ),
            ],
        );
        let nic = builder.get_or_add_node(Node::AzureNetworkInterface("nic-b".into()));
        builder.link_to(nic, Node::ip("10.2.0.4", &vnet("vnet-b")), Edge::ConnectsTo);
        builder
    };
    let reached_from_a = |builder: &GraphBuilder| {
        held_addresses(&builder.graph)
            .holder(&vnet("vnet-a"), "10.2.0.4")
            .map(|index| builder.graph[index].clone())
    };
    let wan = Node::AzureVirtualWan(azure_networking_id("Microsoft.Network/virtualWans/wan"));
    let hub = Node::AzureVirtualHub(azure_networking_id("Microsoft.Network/virtualHubs/hub"));

    let standard = project("Standard");
    assert_edge(&standard, &hub, &wan, &Edge::AttachedTo);
    assert_eq!(
        reached_from_a(&standard),
        Some(Node::ip("10.2.0.4", &vnet("vnet-b")))
    );

    let basic = project("Basic");
    assert_edge(&basic, &wan, &hub, &Edge::Contains);
    assert_eq!(
        reached_from_a(&basic),
        None,
        "a Basic hub carries no traffic between the networks on it"
    );
}
