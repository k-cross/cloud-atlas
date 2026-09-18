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
    pub subnets: Vec<String>,
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
