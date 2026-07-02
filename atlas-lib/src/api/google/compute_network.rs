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
    pub next_page_token: Option<String>,
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
    pub next_page_token: Option<String>,
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
    pub next_page_token: Option<String>,
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
    let mut all_networks = Vec::new();
    let mut page_token = None;
    let base_url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/global/networks",
        project
    );

    loop {
        let mut url = base_url.clone();
        if let Some(token) = &page_token {
            url.push_str(&format!("?pageToken={}", token));
        }

        let req = client.get(&url);
        let res = req.send().await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("GCP API error (networks): {}", text).into());
        }

        let parsed: NetworkListResponse = serde_json::from_str(&text)?;
        if let Some(items) = parsed.items {
            all_networks.extend(items);
        }

        if let Some(token) = parsed.next_page_token {
            page_token = Some(token);
        } else {
            break;
        }
    }
    Ok(all_networks)
}

pub async fn list_subnetworks(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Subnetwork>, Box<dyn std::error::Error>> {
    let mut all_subnets = Vec::new();
    let mut page_token = None;
    let base_url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/aggregated/subnetworks",
        project
    );

    loop {
        let mut url = base_url.clone();
        if let Some(token) = &page_token {
            url.push_str(&format!("?pageToken={}", token));
        }

        let req = client.get(&url);
        let res = req.send().await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("GCP API error (subnetworks): {}", text).into());
        }

        let parsed: SubnetworkAggregatedListResponse = serde_json::from_str(&text)?;
        if let Some(items) = parsed.items {
            for (_, scoped_list) in items {
                if let Some(subnets) = scoped_list.subnetworks {
                    all_subnets.extend(subnets);
                }
            }
        }

        if let Some(token) = parsed.next_page_token {
            page_token = Some(token);
        } else {
            break;
        }
    }
    Ok(all_subnets)
}

pub async fn list_forwarding_rules(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<ForwardingRule>, Box<dyn std::error::Error>> {
    let mut all_rules = Vec::new();
    let mut page_token = None;
    let base_url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/aggregated/forwardingRules",
        project
    );

    loop {
        let mut url = base_url.clone();
        if let Some(token) = &page_token {
            url.push_str(&format!("?pageToken={}", token));
        }

        let req = client.get(&url);
        let res = req.send().await?;
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("GCP API error (forwardingRules): {}", text).into());
        }

        let parsed: ForwardingRuleAggregatedListResponse = serde_json::from_str(&text)?;
        if let Some(items) = parsed.items {
            for (_, scoped_list) in items {
                if let Some(rules) = scoped_list.forwarding_rules {
                    all_rules.extend(rules);
                }
            }
        }

        if let Some(token) = parsed.next_page_token {
            page_token = Some(token);
        } else {
            break;
        }
    }
    Ok(all_rules)
}
