use crate::api::google::client::GoogleApiClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VpnGateway {
    pub id: Option<String>,
    pub name: Option<String>,
    pub network: Option<String>,
    pub self_link: Option<String>,
    pub vpn_interfaces: Option<Vec<GatewayInterface>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewayInterface {
    pub id: Option<u32>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VpnTunnel {
    pub id: Option<String>,
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub status: Option<String>,
    pub vpn_gateway: Option<String>,
    pub peer_external_gateway: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExternalVpnGateway {
    pub id: Option<String>,
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub interfaces: Option<Vec<GatewayInterface>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VpnGatewayAggregatedList {
    items: Option<HashMap<String, VpnGatewaysScopedList>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VpnGatewaysScopedList {
    vpn_gateways: Option<Vec<VpnGateway>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VpnTunnelAggregatedList {
    items: Option<HashMap<String, VpnTunnelsScopedList>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VpnTunnelsScopedList {
    vpn_tunnels: Option<Vec<VpnTunnel>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExternalVpnGatewayList {
    items: Option<Vec<ExternalVpnGateway>>,
}

pub async fn list_vpn_gateways(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<VpnGateway>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!("/compute/v1/projects/{}/aggregated/vpnGateways", project),
    );
    client
        .paginated_list(&url, "vpnGateways", |r: VpnGatewayAggregatedList| {
            r.items.map(|items| {
                items
                    .into_values()
                    .filter_map(|scoped| scoped.vpn_gateways)
                    .flatten()
                    .collect()
            })
        })
        .await
}

pub async fn list_vpn_tunnels(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<VpnTunnel>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!("/compute/v1/projects/{}/aggregated/vpnTunnels", project),
    );
    client
        .paginated_list(&url, "vpnTunnels", |r: VpnTunnelAggregatedList| {
            r.items.map(|items| {
                items
                    .into_values()
                    .filter_map(|scoped| scoped.vpn_tunnels)
                    .flatten()
                    .collect()
            })
        })
        .await
}

pub async fn list_external_vpn_gateways(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<ExternalVpnGateway>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!(
            "/compute/v1/projects/{}/global/externalVpnGateways",
            project
        ),
    );
    client
        .paginated_list(&url, "externalVpnGateways", |r: ExternalVpnGatewayList| {
            r.items
        })
        .await
}
