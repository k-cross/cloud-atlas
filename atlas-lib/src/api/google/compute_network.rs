use crate::api::google::client::GoogleApiClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub id: Option<String>,
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub peerings: Option<Vec<NetworkPeering>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPeering {
    pub name: Option<String>,
    pub network: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkListResponse {
    pub items: Option<Vec<Network>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Subnetwork {
    pub id: Option<String>,
    pub name: Option<String>,
    pub network: Option<String>,
    pub self_link: Option<String>,
    pub ip_cidr_range: Option<String>,
    #[serde(default)]
    pub secondary_ip_ranges: Vec<SecondaryRange>,
    pub internal_ipv6_prefix: Option<String>,
    pub external_ipv6_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryRange {
    pub range_name: Option<String>,
    pub ip_cidr_range: Option<String>,
}

impl Subnetwork {
    pub fn address_space(&self) -> impl Iterator<Item = &str> {
        [
            &self.ip_cidr_range,
            &self.internal_ipv6_prefix,
            &self.external_ipv6_prefix,
        ]
        .into_iter()
        .flatten()
        .chain(
            self.secondary_ip_ranges
                .iter()
                .filter_map(|range| range.ip_cidr_range.as_ref()),
        )
        .map(String::as_str)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubnetworkAggregatedListResponse {
    pub items: Option<std::collections::HashMap<String, SubnetworksScopedList>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubnetworksScopedList {
    pub subnetworks: Option<Vec<Subnetwork>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForwardingRule {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "IPAddress")]
    pub ip_address: Option<String>,
    pub network: Option<String>,
    pub target: Option<String>,
    pub self_link: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardingRuleAggregatedListResponse {
    pub items: Option<std::collections::HashMap<String, ForwardingRulesScopedList>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardingRulesScopedList {
    pub forwarding_rules: Option<Vec<ForwardingRule>>,
}

pub async fn list_networks(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Network>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!("/compute/v1/projects/{}/global/networks", project),
    );
    client
        .paginated_list(&url, "networks", |r: NetworkListResponse| r.items)
        .await
}

pub async fn list_subnetworks(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Subnetwork>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!("/compute/v1/projects/{}/aggregated/subnetworks", project),
    );
    client
        .paginated_list(
            &url,
            "subnetworks",
            |r: SubnetworkAggregatedListResponse| {
                r.items.map(|items| {
                    items
                        .into_values()
                        .filter_map(|scoped| scoped.subnetworks)
                        .flatten()
                        .collect()
                })
            },
        )
        .await
}

pub async fn list_forwarding_rules(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<ForwardingRule>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!(
            "/compute/v1/projects/{}/aggregated/forwardingRules",
            project
        ),
    );
    client
        .paginated_list(
            &url,
            "forwardingRules",
            |r: ForwardingRuleAggregatedListResponse| {
                r.items.map(|items| {
                    items
                        .into_values()
                        .filter_map(|scoped| scoped.forwarding_rules)
                        .flatten()
                        .collect()
                })
            },
        )
        .await
}
