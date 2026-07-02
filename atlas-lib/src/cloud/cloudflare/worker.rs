use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerScript {
    pub id: String,
    pub created_on: Option<String>,
    pub modified_on: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ListScriptsResponse {
    pub success: bool,
    pub errors: Vec<serde_json::Value>,
    pub messages: Vec<serde_json::Value>,
    pub result: Vec<WorkerScript>,
}

pub async fn get_workers(
    account_id: &str,
    token: &str,
) -> Result<Vec<WorkerScript>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts",
        account_id
    );
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch workers: {}", response.status()).into());
    }

    let parsed: ListScriptsResponse = response.json().await?;
    if !parsed.success {
        return Err("Cloudflare API returned success = false".into());
    }

    Ok(parsed.result)
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

#[derive(Serialize, Deserialize, Debug)]
pub struct ListBindingsResponse {
    pub success: bool,
    pub result: Vec<WorkerBinding>,
}

pub async fn get_worker_bindings(
    account_id: &str,
    script_name: &str,
    token: &str,
) -> Result<Vec<WorkerBinding>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}/bindings",
        account_id, script_name
    );
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to fetch bindings for script {}: {}",
            script_name,
            response.status()
        )
        .into());
    }

    let parsed: ListBindingsResponse = response.json().await?;
    if !parsed.success {
        return Err("Cloudflare API returned success = false".into());
    }

    Ok(parsed.result)
}
