use super::client::GoogleApiClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedZone {
    pub name: Option<String>,
    pub dns_name: Option<String>,
    pub id: Option<String>, // Note: ID in DNS API is a string holding a uint64
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedZonesListResponse {
    pub managed_zones: Option<Vec<ManagedZone>>,
    pub next_page_token: Option<String>,
}

pub async fn list_managed_zones(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<ManagedZone>, Box<dyn std::error::Error>> {
    let mut all_zones = Vec::new();
    let mut page_token = None;

    let base_url = format!(
        "https://dns.googleapis.com/dns/v1/projects/{}/managedZones",
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
            return Err(format!("GCP API error (dns): {}", text).into());
        }

        let parsed: ManagedZonesListResponse = serde_json::from_str(&text)?;

        if let Some(items) = parsed.managed_zones {
            all_zones.extend(items);
        }

        if let Some(token) = parsed.next_page_token {
            page_token = Some(token);
        } else {
            break;
        }
    }

    Ok(all_zones)
}
