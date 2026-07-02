use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DurableObjectNamespace {
    pub id: String,
    pub name: String,
    pub class: Option<String>,
    pub script: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ListDurableObjectNamespacesResponse {
    pub success: bool,
    pub result: Vec<DurableObjectNamespace>,
}

pub async fn get_do_namespaces(
    account_id: &str,
    token: &str,
) -> Result<Vec<DurableObjectNamespace>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/durable_objects/namespaces",
        account_id
    );
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch DO namespaces: {}", response.status()).into());
    }

    let parsed: ListDurableObjectNamespacesResponse = response.json().await?;
    if !parsed.success {
        return Err("Cloudflare API returned success = false".into());
    }

    Ok(parsed.result)
}
