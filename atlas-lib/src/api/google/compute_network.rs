use crate::api::google::client::GoogleApiClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub id: Option<String>,
    pub name: Option<String>,
    pub self_link: Option<String>,
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
