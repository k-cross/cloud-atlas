use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureResource {
    pub id: Option<String>,
    pub name: Option<String>,
    pub r#type: Option<String>,
    pub location: Option<String>,
    pub kind: Option<String>,
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VirtualMachine {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub network_interfaces: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VirtualNetwork {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub address_prefixes: Vec<String>,
    pub subnets: Vec<String>,
    pub peerings: Vec<VirtualNetworkPeering>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VirtualNetworkPeering {
    pub remote_id: Option<String>,
    pub state: Option<String>,
    pub use_remote_gateways: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VirtualNetworkGateway {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub provisioning_state: Option<String>,
    pub subnet_ids: Vec<String>,
    pub public_ip_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalNetworkGateway {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub gateway_ip_address: Option<String>,
    pub provisioning_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConnection {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub provisioning_state: Option<String>,
    pub gateway_id: Option<String>,
    pub local_gateway_id: Option<String>,
    pub circuit_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VirtualWan {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub wan_type: Option<String>,
    pub provisioning_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VirtualHub {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub virtual_wan_id: Option<String>,
    pub provisioning_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HubVirtualNetworkConnection {
    pub id: Option<String>,
    pub hub_id: Option<String>,
    pub remote_virtual_network_id: Option<String>,
    pub provisioning_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExpressRouteCircuit {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub provisioning_state: Option<String>,
    pub service_provider_provisioning_state: Option<String>,
    pub peerings: Vec<ExpressRoutePeering>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExpressRoutePeering {
    pub peering_type: Option<String>,
    pub state: Option<String>,
    pub peer_address_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Subnet {
    pub id: Option<String>,
    pub name: Option<String>,
    pub vnet_id: Option<String>,
    pub network_security_group_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRuleProperties {
    pub direction: Option<String>,
    pub destination_address_prefix: Option<String>,
    pub destination_address_prefixes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRule {
    pub properties: Option<SecurityRuleProperties>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSecurityGroupProperties {
    pub security_rules: Option<Vec<SecurityRule>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSecurityGroup {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub properties: Option<NetworkSecurityGroupProperties>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRef {
    pub id: Option<String>,
}

// Both a NIC's `ipConfigurations` and a load balancer's
// `frontendIPConfigurations` are this shape, so the projector reads one
// address-bearing configuration whichever resource carried it.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IpConfigurationProperties {
    #[serde(rename = "privateIPAddress")]
    pub private_ip_address: Option<String>,
    #[serde(rename = "publicIPAddress")]
    pub public_ip_address: Option<ResourceRef>,
    pub subnet: Option<ResourceRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IpConfiguration {
    pub id: Option<String>,
    pub name: Option<String>,
    pub properties: Option<IpConfigurationProperties>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterfaceProperties {
    pub ip_configurations: Option<Vec<IpConfiguration>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub properties: Option<NetworkInterfaceProperties>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancerProperties {
    #[serde(rename = "frontendIPConfigurations")]
    pub frontend_ip_configurations: Option<Vec<IpConfiguration>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancer {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub properties: Option<LoadBalancerProperties>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PublicIpAddress {
    pub id: Option<String>,
    pub name: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StorageAccount {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedCluster {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SqlServer {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppServiceProperties {
    pub default_host_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppService {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub properties: Option<AppServiceProperties>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FunctionApp {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiManagement {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CosmosDb {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceBus {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EventGridTopic {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DnsZone {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CdnProfile {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
}

pub fn in_service(provisioning_state: Option<&str>) -> bool {
    !provisioning_state.is_some_and(|state| {
        state.eq_ignore_ascii_case("Deleting") || state.eq_ignore_ascii_case("Failed")
    })
}
