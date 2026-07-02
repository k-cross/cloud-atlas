use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerScript {
    pub id: String,
    pub created_on: Option<String>,
    pub modified_on: Option<String>,
}

pub async fn get_workers(
    client: &reqwest::Client,
    account_id: &str,
    token: &str,
) -> Result<Vec<WorkerScript>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts",
        account_id
    );
    super::api_get(client, &url, token, "workers").await
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerBinding {
    pub name: String,
    #[serde(rename = "type")]
    pub binding_type: String,

    // KV Namespace
    pub namespace_id: Option<String>,
    // R2 Bucket
    pub bucket_name: Option<String>,
    // D1
    pub id: Option<String>,

    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

pub async fn get_worker_bindings(
    client: &reqwest::Client,
    account_id: &str,
    script_name: &str,
    token: &str,
) -> Result<Vec<WorkerBinding>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}/bindings",
        account_id, script_name
    );
    super::api_get(
        client,
        &url,
        token,
        &format!("bindings for script {}", script_name),
    )
    .await
}
