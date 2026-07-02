use super::client::GoogleApiClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SqlInstance {
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub database_version: Option<String>,
    pub connection_name: Option<String>,
    pub ip_addresses: Option<Vec<SqlIpAddress>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SqlIpAddress {
    #[serde(rename = "type")]
    pub ip_type: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlListResponse {
    pub items: Option<Vec<SqlInstance>>,
    pub next_page_token: Option<String>,
}

pub async fn list_instances(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<SqlInstance>, Box<dyn std::error::Error>> {
    let mut all_instances = Vec::new();
    let mut page_token = None;

    let base_url = format!(
        "https://sqladmin.googleapis.com/sql/v1beta4/projects/{}/instances",
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
            return Err(format!("GCP API error (sql): {}", text).into());
        }

        let parsed: SqlListResponse = serde_json::from_str(&text)?;

        if let Some(items) = parsed.items {
            all_instances.extend(items);
        }

        if let Some(token) = parsed.next_page_token {
            page_token = Some(token);
        } else {
            break;
        }
    }

    Ok(all_instances)
}
