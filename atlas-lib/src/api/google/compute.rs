use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: Option<String>, // the REST API returns ID as a string number e.g. "12345"
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub network_interfaces: Option<Vec<NetworkInterface>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Firewall {
    pub id: Option<String>,
    pub name: Option<String>,
    pub network: Option<String>,
    pub self_link: Option<String>,
    pub source_ranges: Option<Vec<String>>,
    pub destination_ranges: Option<Vec<String>>,
    pub allowed: Option<Vec<FirewallAllowed>>,
    pub direction: Option<String>,
    pub target_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FirewallAllowed {
    #[serde(rename = "IPProtocol")]
    pub ip_protocol: Option<String>,
    pub ports: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallListResponse {
    pub items: Option<Vec<Firewall>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub network: Option<String>,
    pub subnetwork: Option<String>,
    pub network_i_p: Option<String>, // 'networkIP' in camelCase deserializes to network_i_p by default unless explicitly specified, let's use explicit
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceAggregatedListResponse {
    pub items: Option<std::collections::HashMap<String, InstancesScopedList>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancesScopedList {
    pub instances: Option<Vec<Instance>>,
}

use super::client::GoogleApiClient;

pub async fn list_instances(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Instance>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/aggregated/instances",
        project
    );
    client
        .paginated_list(&url, "instances", |r: InstanceAggregatedListResponse| {
            r.items.map(|items| {
                items
                    .into_values()
                    .filter_map(|scoped| scoped.instances)
                    .flatten()
                    .collect()
            })
        })
        .await
}

pub async fn list_firewalls(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Firewall>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/global/firewalls",
        project
    );
    client
        .paginated_list(&url, "firewalls", |r: FirewallListResponse| r.items)
        .await
}
