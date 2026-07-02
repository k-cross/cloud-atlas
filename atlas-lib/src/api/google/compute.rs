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
    pub next_page_token: Option<String>,
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
    pub next_page_token: Option<String>,
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
    let mut all_instances = Vec::new();
    let mut page_token = None;

    let base_url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/aggregated/instances",
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
            return Err(format!("GCP API error: {}", text).into());
        }

        let parsed: InstanceAggregatedListResponse = serde_json::from_str(&text)?;

        if let Some(items) = parsed.items {
            for (_, scoped_list) in items {
                if let Some(instances) = scoped_list.instances {
                    all_instances.extend(instances);
                }
            }
        }

        if let Some(token) = parsed.next_page_token {
            page_token = Some(token);
        } else {
            break;
        }
    }

    Ok(all_instances)
}

pub async fn list_firewalls(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Firewall>, Box<dyn std::error::Error>> {
    let mut all_firewalls = Vec::new();
    let mut page_token = None;

    let base_url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/global/firewalls",
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
            return Err(format!("GCP API error (firewalls): {}", text).into());
        }

        let parsed: FirewallListResponse = serde_json::from_str(&text)?;

        if let Some(items) = parsed.items {
            all_firewalls.extend(items);
        }

        if let Some(token) = parsed.next_page_token {
            page_token = Some(token);
        } else {
            break;
        }
    }

    Ok(all_firewalls)
}
