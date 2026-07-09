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
}

pub async fn list_managed_zones(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<ManagedZone>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://dns.googleapis.com",
        &format!("/dns/v1/projects/{}/managedZones", project),
    );
    client
        .paginated_list(&url, "dns", |r: ManagedZonesListResponse| r.managed_zones)
        .await
}
