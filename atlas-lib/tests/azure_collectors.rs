use atlas_lib::cloud::azure::provider::map_resources;
use atlas_lib::cloud::definition::MicrosoftCollection;
use serde_json::{Value, json};

macro_rules! variant {
    ($cols:expr, $v:ident) => {
        $cols
            .iter()
            .find_map(|c| match c {
                MicrosoftCollection::$v(items) => Some(items),
                _ => None,
            })
            .expect(concat!("collection ", stringify!($v), " present"))
    };
}

fn map(rows: Vec<Value>) -> Vec<MicrosoftCollection> {
    let (collections, report) = map_resources(rows);
    assert!(
        report.is_complete(),
        "unexpected mapping failure: {}",
        report.summary()
    );
    collections
}

#[test]
fn a_malformed_row_is_skipped_without_discarding_the_batch() {
    let good = |id: &str, name: &str| {
        json!({
            "id": id,
            "name": name,
            "type": "microsoft.storage/storageaccounts",
            "location": "eastus"
        })
    };

    let malformed = json!({
        "id": "/subscriptions/s/.../storageAccounts/broken",
        "name": 12345,
        "type": "microsoft.storage/storageaccounts",
        "location": "eastus"
    });

    let (cols, report) = map_resources(vec![
        good("/subscriptions/s/.../storageAccounts/a", "sa-a"),
        malformed,
        good("/subscriptions/s/.../storageAccounts/b", "sa-b"),
    ]);

    let accounts = variant!(cols, AzureStorageAccounts);
    let names: Vec<&str> = accounts
        .iter()
        .filter_map(|a| a.name.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["sa-a", "sa-b"],
        "rows either side of the malformed one must survive"
    );

    assert!(
        !report.is_complete(),
        "a skipped row must be reported, or its absence looks like a deletion"
    );
    assert!(
        report.summary().contains("storageAccounts/broken"),
        "the report must name the offending row: {}",
        report.summary()
    );
}

#[test]
fn an_unmodelled_resource_type_is_filtered_without_being_reported() {
    let (_cols, report) = map_resources(vec![json!({
        "id": "/subscriptions/s/.../redis/cache1",
        "name": "cache1",
        "type": "microsoft.cache/redis",
        "location": "eastus"
    })]);

    assert!(report.is_complete(), "got: {}", report.summary());
}

#[test]
fn vm_extracts_nic_ids_from_network_profile() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../virtualMachines/vm1",
        "name": "vm1",
        "type": "Microsoft.Compute/virtualMachines",
        "location": "eastus",
        "properties": { "networkProfile": { "networkInterfaces": [ { "id": "/subscriptions/s/.../nic1" } ] } }
    })]);

    let vms = variant!(cols, AzureVirtualMachines);
    assert_eq!(vms.len(), 1);
    assert_eq!(vms[0].name.as_deref(), Some("vm1"));
    assert_eq!(vms[0].location.as_deref(), Some("eastus"));
    assert_eq!(
        vms[0].network_interfaces,
        vec!["/subscriptions/s/.../nic1".to_string()]
    );
}

#[test]
fn vnet_extracts_inline_subnets_with_nsg() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../virtualNetworks/vnet1",
        "name": "vnet1",
        "type": "microsoft.network/virtualnetworks",
        "location": "eastus",
        "properties": {
            "subnets": [ {
                "id": "/subscriptions/s/.../subnets/sn1",
                "name": "sn1",
                "properties": { "networkSecurityGroup": { "id": "/subscriptions/s/.../nsg1" } }
            } ],
            "virtualNetworkPeerings": [ {
                "id": "/subscriptions/s/.../virtualNetworks/vnet1/virtualNetworkPeerings/to-hub",
                "name": "to-hub",
                "properties": {
                    "peeringState": "Connected",
                    "remoteVirtualNetwork": { "id": "/subscriptions/s/.../virtualNetworks/hub" },
                    "allowForwardedTraffic": true,
                    "useRemoteGateways": true
                }
            } ]
        }
    })]);

    let vnets = variant!(cols, AzureVirtualNetworks);
    assert_eq!(
        vnets[0].subnets,
        vec!["/subscriptions/s/.../subnets/sn1".to_string()]
    );
    assert_eq!(
        vnets[0].peerings[0].remote_id.as_deref(),
        Some("/subscriptions/s/.../virtualNetworks/hub")
    );
    assert_eq!(vnets[0].peerings[0].state.as_deref(), Some("Connected"));
    assert!(vnets[0].peerings[0].use_remote_gateways);

    let subnets = variant!(cols, AzureSubnets);
    assert_eq!(subnets[0].name.as_deref(), Some("sn1"));
    assert_eq!(
        subnets[0].vnet_id.as_deref(),
        Some("/subscriptions/s/.../virtualNetworks/vnet1")
    );
    assert_eq!(
        subnets[0].network_security_group_id.as_deref(),
        Some("/subscriptions/s/.../nsg1")
    );
}

// ARG spells these two `IP` where serde's camelCase would spell them `Ip`, and
// an all-`None` row would read as a NIC that holds no address at all.
#[test]
fn nic_extracts_the_addresses_its_ip_configurations_carry() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../networkInterfaces/nic1",
        "name": "nic1",
        "type": "microsoft.network/networkInterfaces",
        "location": "eastus",
        "properties": { "ipConfigurations": [ {
            "name": "ipconfig1",
            "properties": {
                "privateIPAddress": "10.0.1.4",
                "publicIPAddress": { "id": "/subscriptions/s/.../publicIPAddresses/pip1" },
                "subnet": { "id": "/subscriptions/s/.../subnets/sn1" }
            }
        } ] }
    })]);

    let nics = variant!(cols, AzureNetworkInterfaces);
    let configs = nics[0]
        .properties
        .as_ref()
        .and_then(|p| p.ip_configurations.as_ref())
        .expect("ip configurations present");
    let properties = configs[0].properties.as_ref().expect("properties present");

    assert_eq!(properties.private_ip_address.as_deref(), Some("10.0.1.4"));
    assert_eq!(
        properties
            .public_ip_address
            .as_ref()
            .and_then(|r| r.id.as_deref()),
        Some("/subscriptions/s/.../publicIPAddresses/pip1")
    );
    assert_eq!(
        properties.subnet.as_ref().and_then(|r| r.id.as_deref()),
        Some("/subscriptions/s/.../subnets/sn1")
    );
}

#[test]
fn load_balancer_extracts_its_frontend_addresses() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../loadBalancers/lb1",
        "name": "lb1",
        "type": "microsoft.network/loadBalancers",
        "location": "eastus",
        "properties": {
            "frontendIPConfigurations": [ {
                "name": "frontend",
                "properties": { "publicIPAddress": { "id": "/subscriptions/s/.../publicIPAddresses/pip1" } }
            } ],
            "backendAddressPools": [ { "id": "/subscriptions/s/.../backendAddressPools/pool1" } ]
        }
    })]);

    let balancers = variant!(cols, AzureLoadBalancers);
    let configs = balancers[0]
        .properties
        .as_ref()
        .and_then(|p| p.frontend_ip_configurations.as_ref())
        .expect("frontend configurations present");

    assert_eq!(
        configs[0]
            .properties
            .as_ref()
            .and_then(|p| p.public_ip_address.as_ref())
            .and_then(|r| r.id.as_deref()),
        Some("/subscriptions/s/.../publicIPAddresses/pip1")
    );
}

// An internal balancer fronts a private address in a subnet instead, which is
// the other two branches of the projector's shared configuration reader.
#[test]
fn an_internal_load_balancer_frontend_carries_a_private_address() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../loadBalancers/lb-internal",
        "name": "lb-internal",
        "type": "microsoft.network/loadBalancers",
        "location": "eastus",
        "properties": { "frontendIPConfigurations": [ {
            "name": "internal",
            "properties": {
                "privateIPAddress": "10.0.2.10",
                "subnet": { "id": "/subscriptions/s/.../subnets/sn1" }
            }
        } ] }
    })]);

    let balancers = variant!(cols, AzureLoadBalancers);
    let properties = balancers[0]
        .properties
        .as_ref()
        .and_then(|p| p.frontend_ip_configurations.as_ref())
        .expect("frontend configurations present")[0]
        .properties
        .as_ref()
        .expect("configuration properties present");

    assert_eq!(properties.private_ip_address.as_deref(), Some("10.0.2.10"));
    assert_eq!(
        properties.subnet.as_ref().and_then(|r| r.id.as_deref()),
        Some("/subscriptions/s/.../subnets/sn1")
    );
    assert!(properties.public_ip_address.is_none());
}

#[test]
fn public_ip_extracts_ip_address() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../publicIPAddresses/pip1",
        "name": "pip1",
        "type": "microsoft.network/publicipaddresses",
        "location": "eastus",
        "properties": { "ipAddress": "20.1.2.3" }
    })]);
    let pips = variant!(cols, AzurePublicIpAddresses);
    assert_eq!(pips[0].ip_address.as_deref(), Some("20.1.2.3"));
}

#[test]
fn web_site_kind_splits_function_apps_from_app_services() {
    let cols = map(vec![
        json!({
            "id": "/f", "name": "fn-app", "type": "microsoft.web/sites", "location": "eastus",
            "kind": "functionapp,linux", "properties": {}
        }),
        json!({
            "id": "/w", "name": "web-app", "type": "microsoft.web/sites", "location": "eastus",
            "kind": "app,linux", "properties": {}
        }),
    ]);
    assert_eq!(variant!(cols, AzureFunctionApps).len(), 1);
    assert_eq!(
        variant!(cols, AzureFunctionApps)[0].name.as_deref(),
        Some("fn-app")
    );
    assert_eq!(
        variant!(cols, AzureAppServices)[0].name.as_deref(),
        Some("web-app")
    );
}

#[test]
fn unknown_resource_types_are_ignored() {
    let cols = map(vec![json!({
        "id": "/x", "name": "mystery", "type": "microsoft.unknown/thing", "location": "eastus",
        "properties": {}
    })]);
    assert!(variant!(cols, AzureVirtualMachines).is_empty());
    assert!(variant!(cols, AzureStorageAccounts).is_empty());
}

#[test]
fn vpn_gateways_local_gateways_and_connections_are_extracted() {
    let cols = map(vec![
        json!({
            "id": "/subscriptions/s/.../virtualNetworkGateways/vng1",
            "name": "vng1",
            "type": "microsoft.network/virtualnetworkgateways",
            "location": "eastus",
            "properties": {
                "gatewayType": "Vpn",
                "provisioningState": "Succeeded",
                "ipConfigurations": [ { "properties": {
                    "subnet": { "id": "/subscriptions/s/.../virtualNetworks/hub/subnets/GatewaySubnet" },
                    "publicIPAddress": { "id": "/subscriptions/s/.../publicIPAddresses/pip-vpn" }
                } } ]
            }
        }),
        json!({
            "id": "/subscriptions/s/.../localNetworkGateways/lng1",
            "name": "lng1",
            "type": "microsoft.network/localnetworkgateways",
            "location": "eastus",
            "properties": { "gatewayIpAddress": "192.0.2.101", "provisioningState": "Updating" }
        }),
        json!({
            "id": "/subscriptions/s/.../connections/conn1",
            "name": "conn1",
            "type": "microsoft.network/connections",
            "location": "eastus",
            "properties": {
                "connectionType": "IPsec",
                "provisioningState": "Deleting",
                "virtualNetworkGateway1": { "id": "/subscriptions/s/.../virtualNetworkGateways/vng1" },
                "localNetworkGateway2": { "id": "/subscriptions/s/.../localNetworkGateways/lng1" }
            }
        }),
    ]);

    let gateway = &variant!(cols, AzureVirtualNetworkGateways)[0];
    assert_eq!(gateway.provisioning_state.as_deref(), Some("Succeeded"));
    assert_eq!(
        gateway.subnet_ids,
        vec!["/subscriptions/s/.../virtualNetworks/hub/subnets/GatewaySubnet".to_string()]
    );
    assert_eq!(
        gateway.public_ip_ids,
        vec!["/subscriptions/s/.../publicIPAddresses/pip-vpn".to_string()]
    );
    let local_gateway = &variant!(cols, AzureLocalNetworkGateways)[0];
    assert_eq!(
        local_gateway.gateway_ip_address.as_deref(),
        Some("192.0.2.101")
    );
    assert_eq!(
        local_gateway.provisioning_state.as_deref(),
        Some("Updating")
    );
    let connection = &variant!(cols, AzureGatewayConnections)[0];
    assert_eq!(connection.provisioning_state.as_deref(), Some("Deleting"));
    assert_eq!(
        connection.gateway_id.as_deref(),
        Some("/subscriptions/s/.../virtualNetworkGateways/vng1")
    );
    assert_eq!(
        connection.local_gateway_id.as_deref(),
        Some("/subscriptions/s/.../localNetworkGateways/lng1")
    );
}

#[test]
fn expressroute_circuits_and_their_connections_are_extracted() {
    let cols = map(vec![
        json!({
            "id": "/subscriptions/s/.../expressRouteCircuits/er1",
            "name": "er1",
            "type": "microsoft.network/expressroutecircuits",
            "location": "eastus",
            "properties": {
                "provisioningState": "Succeeded",
                "circuitProvisioningState": "Enabled",
                "serviceProviderProvisioningState": "Provisioned",
                "serviceKey": "00000000-0000-0000-0000-000000000000",
                "peerings": [
                    {
                        "name": "AzurePrivatePeering",
                        "properties": {
                            "peeringType": "AzurePrivatePeering",
                            "state": "Enabled",
                            "azureASN": 12076,
                            "peerASN": 16550,
                            "primaryPeerAddressPrefix": "169.254.70.0/30",
                            "secondaryPeerAddressPrefix": "169.254.70.4/30",
                            "ipv6PeeringConfig": {
                                "primaryPeerAddressPrefix": "fd00:70::/126",
                                "secondaryPeerAddressPrefix": "fd00:70::4/126",
                                "state": "Disabled"
                            }
                        }
                    },
                    {
                        "name": "MicrosoftPeering",
                        "properties": {
                            "peeringType": "MicrosoftPeering",
                            "state": "Enabled",
                            "primaryPeerAddressPrefix": "203.0.113.0/30"
                        }
                    }
                ]
            }
        }),
        json!({
            "id": "/subscriptions/s/.../connections/er-conn",
            "name": "er-conn",
            "type": "microsoft.network/connections",
            "location": "eastus",
            "properties": {
                "connectionType": "ExpressRoute",
                "provisioningState": "Succeeded",
                "virtualNetworkGateway1": { "id": "/subscriptions/s/.../virtualNetworkGateways/ergw" },
                "peer": { "id": "/subscriptions/s/.../expressRouteCircuits/er1" }
            }
        }),
    ]);

    let circuit = &variant!(cols, AzureExpressRouteCircuits)[0];
    assert_eq!(
        circuit.id.as_deref(),
        Some("/subscriptions/s/.../expressRouteCircuits/er1")
    );
    assert_eq!(circuit.provisioning_state.as_deref(), Some("Succeeded"));
    assert_eq!(
        circuit.service_provider_provisioning_state.as_deref(),
        Some("Provisioned")
    );
    let peerings: Vec<_> = circuit
        .peerings
        .iter()
        .map(|peering| {
            (
                peering.peering_type.as_deref(),
                peering.state.as_deref(),
                peering.peer_address_prefixes.clone(),
            )
        })
        .collect();
    assert_eq!(
        peerings,
        vec![
            (
                Some("AzurePrivatePeering"),
                Some("Enabled"),
                vec!["169.254.70.0/30".to_owned(), "169.254.70.4/30".to_owned()]
            ),
            (
                Some("AzurePrivatePeering"),
                Some("Disabled"),
                vec!["fd00:70::/126".to_owned(), "fd00:70::4/126".to_owned()]
            ),
            (
                Some("MicrosoftPeering"),
                Some("Enabled"),
                vec!["203.0.113.0/30".to_owned()]
            ),
        ]
    );
    let connection = &variant!(cols, AzureGatewayConnections)[0];
    assert_eq!(
        connection.gateway_id.as_deref(),
        Some("/subscriptions/s/.../virtualNetworkGateways/ergw")
    );
    assert_eq!(
        connection.circuit_id.as_deref(),
        Some("/subscriptions/s/.../expressRouteCircuits/er1")
    );
}

#[test]
fn an_ipv6_peering_with_no_state_of_its_own_takes_its_peering_state() {
    let cols = map(vec![json!({
        "id": "/subscriptions/s/.../expressRouteCircuits/er2",
        "name": "er2",
        "type": "microsoft.network/expressroutecircuits",
        "location": "eastus",
        "properties": {
            "provisioningState": "Succeeded",
            "serviceProviderProvisioningState": "Provisioned",
            "peerings": [ {
                "name": "AzurePrivatePeering",
                "properties": {
                    "peeringType": "AzurePrivatePeering",
                    "state": "Disabled",
                    "primaryPeerAddressPrefix": "169.254.71.0/30",
                    "ipv6PeeringConfig": { "primaryPeerAddressPrefix": "fd00:71::/126" }
                }
            } ]
        }
    })]);

    let circuit = &variant!(cols, AzureExpressRouteCircuits)[0];
    let states: Vec<_> = circuit
        .peerings
        .iter()
        .map(|peering| peering.state.as_deref())
        .collect();
    assert_eq!(states, vec![Some("Disabled"), Some("Disabled")]);
}
