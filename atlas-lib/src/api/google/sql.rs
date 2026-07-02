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
}

pub async fn list_instances(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<SqlInstance>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://sqladmin.googleapis.com/sql/v1beta4/projects/{}/instances",
        project
    );
    client
        .paginated_list(&url, "sql", |r: SqlListResponse| r.items)
        .await
}
