use super::client::GoogleApiClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub endpoint: Option<String>,
    pub network: Option<String>,
    pub subnetwork: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterListResponse {
    pub clusters: Option<Vec<Cluster>>,
}

pub async fn list_clusters(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Cluster>, Box<dyn std::error::Error>> {
    let mut all_clusters = Vec::new();

    // GKE uses location='-' to mean all locations
    let url = format!(
        "https://container.googleapis.com/v1/projects/{}/locations/-/clusters",
        project
    );

    let req = client.get(&url);
    let res = req.send().await?;

    let status = res.status();
    let text = res.text().await?;
    if !status.is_success() {
        return Err(format!("GCP API error (gke): {}", text).into());
    }

    let parsed: ClusterListResponse = serde_json::from_str(&text)?;

    if let Some(items) = parsed.clusters {
        all_clusters.extend(items);
    }

    Ok(all_clusters)
}
