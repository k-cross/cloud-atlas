use crate::definition::MicrosoftCollection;
use atlas_provider::fixtures::{
    AWS_VPN_ADDRESSES, AZURE_SPOKE_RANGE, AZURE_SUBSCRIPTION, AZURE_VPN_ADDRESS, ER_PRIMARY_PREFIX,
    ER_SECONDARY_PREFIX,
};

pub fn azure_id(rest: &str) -> String {
    format!(
        "/subscriptions/{}/resourceGroups/rg-globex/providers/{}",
        AZURE_SUBSCRIPTION, rest
    )
}

// The projector folds a networking resource's id to one casing (Resource
// Graph is not consistent about which casing it echoes an id back with,
// depending on which resource's JSON names it), so a test's expectation
// has to be built the same way. Leaf resources are never cross-referenced
// by id, so they keep whatever casing the fixture wrote and need no helper.
pub fn azure_networking_id(rest: &str) -> std::sync::Arc<str> {
    azure_id(rest).to_ascii_lowercase().into()
}

pub fn azure_network() -> std::sync::Arc<str> {
    azure_networking_id("Microsoft.Network/virtualNetworks/vnet-globex")
}

pub fn collections() -> Vec<MicrosoftCollection> {
    use crate::api::models::*;

    let vm = VirtualMachine {
        id: Some(azure_id("Microsoft.Compute/virtualMachines/vm-worker")),
        name: Some("vm-worker".to_owned()),
        location: Some("eastus".to_owned()),
        network_interfaces: vec![azure_id("Microsoft.Network/networkInterfaces/nic-worker")],
    };

    let peering = |remote: &str, state: &str, use_remote_gateways: bool| VirtualNetworkPeering {
        remote_id: Some(azure_id(&format!(
            "Microsoft.Network/virtualNetworks/{remote}"
        ))),
        state: Some(state.to_owned()),
        use_remote_gateways,
    };
    let vnet = VirtualNetwork {
        id: Some(azure_id("Microsoft.Network/virtualNetworks/vnet-globex")),
        name: Some("vnet-globex".to_owned()),
        location: Some("eastus".to_owned()),
        address_prefixes: vec![AZURE_SPOKE_RANGE.to_owned()],
        subnets: vec![azure_id(
            "Microsoft.Network/virtualNetworks/vnet-globex/subnets/default",
        )],
        peerings: vec![
            peering("vnet-globex-hub", "Connected", true),
            peering("vnet-globex-dr", "Initiated", false),
        ],
    };
    let hub = VirtualNetwork {
        id: Some(azure_id(
            "Microsoft.Network/virtualNetworks/vnet-globex-hub",
        )),
        name: Some("vnet-globex-hub".to_owned()),
        location: Some("eastus".to_owned()),
        address_prefixes: vec!["10.20.100.0/24".to_owned()],
        subnets: Vec::new(),
        peerings: vec![peering("vnet-globex", "Connected", false)],
    };

    let subnet = Subnet {
        id: Some(azure_id(
            "Microsoft.Network/virtualNetworks/vnet-globex/subnets/default",
        )),
        name: Some("default".to_owned()),
        vnet_id: Some(azure_id("Microsoft.Network/virtualNetworks/vnet-globex")),
        network_security_group_id: Some(azure_id(
            "Microsoft.Network/networkSecurityGroups/nsg-worker",
        )),
    };

    let nsg = NetworkSecurityGroup {
        id: Some(azure_id(
            "Microsoft.Network/networkSecurityGroups/nsg-worker",
        )),
        name: Some("nsg-worker".to_owned()),
        location: Some("eastus".to_owned()),
        properties: Some(NetworkSecurityGroupProperties {
            security_rules: Some(vec![
                SecurityRule {
                    properties: Some(SecurityRuleProperties {
                        direction: Some("Outbound".to_owned()),
                        destination_address_prefix: Some("10.20.0.5".to_owned()),
                        destination_address_prefixes: None,
                    }),
                },
                SecurityRule {
                    properties: Some(SecurityRuleProperties {
                        direction: Some("Outbound".to_owned()),
                        destination_address_prefix: Some("AzureCloud".to_owned()),
                        destination_address_prefixes: None,
                    }),
                },
            ]),
        }),
    };

    let pip = PublicIpAddress {
        id: Some(azure_id("Microsoft.Network/publicIPAddresses/pip-globex")),
        name: Some("pip-globex".to_owned()),
        ip_address: Some("198.51.100.10".to_owned()),
    };

    let vpn_pip = PublicIpAddress {
        id: Some(azure_id(
            "Microsoft.Network/publicIPAddresses/pip-globex-vpn",
        )),
        name: Some("pip-globex-vpn".to_owned()),
        ip_address: Some(AZURE_VPN_ADDRESS.to_owned()),
    };
    let vpn_gateway = VirtualNetworkGateway {
        id: Some(azure_id(
            "Microsoft.Network/virtualNetworkGateways/vng-globex-hub",
        )),
        name: Some("vng-globex-hub".to_owned()),
        location: Some("eastus".to_owned()),
        provisioning_state: Some("Succeeded".to_owned()),
        subnet_ids: vec![azure_id(
            "Microsoft.Network/virtualNetworks/vnet-globex-hub/subnets/GatewaySubnet",
        )],
        public_ip_ids: vec![azure_id(
            "Microsoft.Network/publicIPAddresses/pip-globex-vpn",
        )],
    };
    let local_gateway = LocalNetworkGateway {
        id: Some(azure_id(
            "Microsoft.Network/localNetworkGateways/lng-globex-aws",
        )),
        name: Some("lng-globex-aws".to_owned()),
        location: Some("eastus".to_owned()),
        gateway_ip_address: Some(AWS_VPN_ADDRESSES[0].to_owned()),
        provisioning_state: Some("Succeeded".to_owned()),
    };
    let vpn_connection = GatewayConnection {
        id: Some(azure_id("Microsoft.Network/connections/conn-globex-aws")),
        name: Some("conn-globex-aws".to_owned()),
        location: Some("eastus".to_owned()),
        provisioning_state: Some("Succeeded".to_owned()),
        gateway_id: vpn_gateway.id.clone(),
        local_gateway_id: local_gateway.id.clone(),
        circuit_id: None,
    };

    let expressroute_gateway = VirtualNetworkGateway {
        id: Some(azure_id(
            "Microsoft.Network/virtualNetworkGateways/ergw-globex-hub",
        )),
        name: Some("ergw-globex-hub".to_owned()),
        location: Some("eastus".to_owned()),
        provisioning_state: Some("Succeeded".to_owned()),
        subnet_ids: vec![azure_id(
            "Microsoft.Network/virtualNetworks/vnet-globex-hub/subnets/GatewaySubnet",
        )],
        public_ip_ids: Vec::new(),
    };
    let circuit = ExpressRouteCircuit {
        id: Some(azure_id(
            "Microsoft.Network/expressRouteCircuits/er-globex-gcp",
        )),
        name: Some("er-globex-gcp".to_owned()),
        location: Some("eastus".to_owned()),
        provisioning_state: Some("Succeeded".to_owned()),
        service_provider_provisioning_state: Some("Provisioned".to_owned()),
        peerings: vec![ExpressRoutePeering {
            peering_type: Some("AzurePrivatePeering".to_owned()),
            state: Some("Enabled".to_owned()),
            peer_address_prefixes: vec![
                ER_PRIMARY_PREFIX.to_owned(),
                ER_SECONDARY_PREFIX.to_owned(),
            ],
        }],
    };
    let expressroute_connection = GatewayConnection {
        id: Some(azure_id("Microsoft.Network/connections/conn-globex-gcp")),
        name: Some("conn-globex-gcp".to_owned()),
        location: Some("eastus".to_owned()),
        provisioning_state: Some("Succeeded".to_owned()),
        gateway_id: expressroute_gateway.id.clone(),
        local_gateway_id: None,
        circuit_id: circuit.id.clone(),
    };

    let ip_configuration = |name: &str,
                            private_ip: Option<&str>,
                            public_ip: Option<&str>,
                            subnet: Option<&str>| IpConfiguration {
        id: None,
        name: Some(name.to_owned()),
        properties: Some(IpConfigurationProperties {
            private_ip_address: private_ip.map(str::to_owned),
            public_ip_address: public_ip.map(|id| ResourceRef {
                id: Some(azure_id(id)),
            }),
            subnet: subnet.map(|id| ResourceRef {
                id: Some(azure_id(id)),
            }),
        }),
    };

    let nic = NetworkInterface {
        id: Some(azure_id("Microsoft.Network/networkInterfaces/nic-worker")),
        name: Some("nic-worker".to_owned()),
        location: Some("eastus".to_owned()),
        properties: Some(NetworkInterfaceProperties {
            ip_configurations: Some(vec![ip_configuration(
                "ipconfig1",
                Some("10.20.1.4"),
                None,
                Some("Microsoft.Network/virtualNetworks/vnet-globex/subnets/default"),
            )]),
        }),
    };

    // The balancer holds the public address the AWS side is seen talking to,
    // so that flow reads as a service and not as a bare address object.
    let balancer = LoadBalancer {
        id: Some(azure_id("Microsoft.Network/loadBalancers/lb-globex")),
        name: Some("lb-globex".to_owned()),
        location: Some("eastus".to_owned()),
        properties: Some(LoadBalancerProperties {
            frontend_ip_configurations: Some(vec![ip_configuration(
                "frontend",
                None,
                Some("Microsoft.Network/publicIPAddresses/pip-globex"),
                None,
            )]),
        }),
    };

    let leaf = |rest: &str, name: &str| (azure_id(rest), name.to_owned());

    let (sa_id, sa_name) = leaf(
        "Microsoft.Storage/storageAccounts/globexstore",
        "globexstore",
    );
    let storage = StorageAccount {
        id: Some(sa_id),
        name: Some(sa_name),
        location: Some("eastus".to_owned()),
    };

    let (aks_id, aks_name) = leaf(
        "Microsoft.ContainerService/managedClusters/aks-globex",
        "aks-globex",
    );
    let aks = ManagedCluster {
        id: Some(aks_id),
        name: Some(aks_name),
        location: Some("eastus".to_owned()),
    };

    let (sql_id, sql_name) = leaf("Microsoft.Sql/servers/sql-globex", "sql-globex");
    let sql = SqlServer {
        id: Some(sql_id),
        name: Some(sql_name),
        location: Some("eastus".to_owned()),
    };

    let app = AppService {
        id: Some(azure_id("Microsoft.Web/sites/app-globex")),
        name: Some("app-globex".to_owned()),
        location: Some("eastus".to_owned()),
        properties: Some(AppServiceProperties {
            default_host_name: Some("app-globex.azurewebsites.net".to_owned()),
        }),
    };

    let (fa_id, fa_name) = leaf("Microsoft.Web/sites/func-globex", "func-globex");
    let func = FunctionApp {
        id: Some(fa_id),
        name: Some(fa_name),
        location: Some("eastus".to_owned()),
    };

    let (apim_id, apim_name) = leaf("Microsoft.ApiManagement/service/apim-globex", "apim-globex");
    let apim = ApiManagement {
        id: Some(apim_id),
        name: Some(apim_name),
        location: Some("eastus".to_owned()),
    };

    let (cos_id, cos_name) = leaf(
        "Microsoft.DocumentDB/databaseAccounts/cosmos-globex",
        "cosmos-globex",
    );
    let cosmos = CosmosDb {
        id: Some(cos_id),
        name: Some(cos_name),
        location: Some("eastus".to_owned()),
    };

    let (sb_id, sb_name) = leaf("Microsoft.ServiceBus/namespaces/sb-globex", "sb-globex");
    let sbus = ServiceBus {
        id: Some(sb_id),
        name: Some(sb_name),
        location: Some("eastus".to_owned()),
    };

    let (eg_id, eg_name) = leaf("Microsoft.EventGrid/topics/eg-globex", "eg-globex");
    let egrid = EventGridTopic {
        id: Some(eg_id),
        name: Some(eg_name),
        location: Some("eastus".to_owned()),
    };

    let (dns_id, dns_name) = leaf("Microsoft.Network/dnsZones/globex.azure", "globex.azure");
    let dns = DnsZone {
        id: Some(dns_id),
        name: Some(dns_name),
        location: Some("global".to_owned()),
    };

    let wan_id = azure_id("Microsoft.Network/virtualWans/vwan-globex");
    let wan = VirtualWan {
        id: Some(wan_id.clone()),
        name: Some("vwan-globex".to_owned()),
        location: Some("eastus".to_owned()),
        wan_type: Some("Standard".to_owned()),
        provisioning_state: Some("Succeeded".to_owned()),
    };
    let virtual_hub_id = azure_id("Microsoft.Network/virtualHubs/vhub-globex-eastus");
    let virtual_hub = VirtualHub {
        id: Some(virtual_hub_id.clone()),
        name: Some("vhub-globex-eastus".to_owned()),
        location: Some("eastus".to_owned()),
        virtual_wan_id: Some(wan_id),
        provisioning_state: Some("Succeeded".to_owned()),
    };
    let wan_network = |name: &str, prefix: &str| VirtualNetwork {
        id: Some(azure_id(&format!(
            "Microsoft.Network/virtualNetworks/{name}"
        ))),
        name: Some(name.to_owned()),
        location: Some("eastus".to_owned()),
        address_prefixes: vec![prefix.to_owned()],
        ..Default::default()
    };
    let apps = wan_network("vnet-globex-apps", "10.22.0.0/16");
    let data = wan_network("vnet-globex-data", "10.23.0.0/16");
    let hub_connections = ["vnet-globex-apps", "vnet-globex-data"]
        .into_iter()
        .map(|name| HubVirtualNetworkConnection {
            id: Some(format!(
                "{virtual_hub_id}/hubVirtualNetworkConnections/conn-{name}"
            )),
            hub_id: Some(virtual_hub_id.clone()),
            remote_virtual_network_id: Some(azure_id(&format!(
                "Microsoft.Network/virtualNetworks/{name}"
            ))),
            provisioning_state: Some("Succeeded".to_owned()),
        })
        .collect();

    let (cdn_id, cdn_name) = leaf("Microsoft.Cdn/profiles/cdn-globex", "cdn-globex");
    let cdn = CdnProfile {
        id: Some(cdn_id),
        name: Some(cdn_name),
        location: Some("global".to_owned()),
    };

    vec![
        MicrosoftCollection::AzureVirtualMachines(vec![vm]),
        MicrosoftCollection::AzureVirtualNetworks(vec![vnet, hub, apps, data]),
        MicrosoftCollection::AzureSubnets(vec![subnet]),
        MicrosoftCollection::AzureNetworkInterfaces(vec![nic]),
        MicrosoftCollection::AzureLoadBalancers(vec![balancer]),
        MicrosoftCollection::AzureNetworkSecurityGroups(vec![nsg]),
        MicrosoftCollection::AzurePublicIpAddresses(vec![pip, vpn_pip]),
        MicrosoftCollection::AzureVirtualNetworkGateways(vec![vpn_gateway, expressroute_gateway]),
        MicrosoftCollection::AzureLocalNetworkGateways(vec![local_gateway]),
        MicrosoftCollection::AzureGatewayConnections(vec![vpn_connection, expressroute_connection]),
        MicrosoftCollection::AzureExpressRouteCircuits(vec![circuit]),
        MicrosoftCollection::AzureVirtualWans(vec![wan]),
        MicrosoftCollection::AzureVirtualHubs(vec![virtual_hub]),
        MicrosoftCollection::AzureHubVirtualNetworkConnections(hub_connections),
        MicrosoftCollection::AzureStorageAccounts(vec![storage]),
        MicrosoftCollection::AzureManagedClusters(vec![aks]),
        MicrosoftCollection::AzureSqlServers(vec![sql]),
        MicrosoftCollection::AzureAppServices(vec![app]),
        MicrosoftCollection::AzureFunctionApps(vec![func]),
        MicrosoftCollection::AzureApiManagement(vec![apim]),
        MicrosoftCollection::AzureCosmosDbs(vec![cosmos]),
        MicrosoftCollection::AzureServiceBuses(vec![sbus]),
        MicrosoftCollection::AzureEventGridTopics(vec![egrid]),
        MicrosoftCollection::AzureDnsZones(vec![dns]),
        MicrosoftCollection::AzureCdnProfiles(vec![cdn]),
    ]
}
