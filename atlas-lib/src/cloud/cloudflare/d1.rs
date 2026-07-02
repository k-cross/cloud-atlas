use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct D1Database {
    pub uuid: String,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ListD1DatabasesResponse {
    pub success: bool,
    pub result: Vec<D1Database>,
}

pub async fn get_d1_databases(
    account_id: &str,
    token: &str,
) -> Result<Vec<D1Database>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/d1/database",
        account_id
    );
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch D1 databases: {}", response.status()).into());
    }

    let parsed: ListD1DatabasesResponse = response.json().await?;
    if !parsed.success {
        return Err("Cloudflare API returned success = false".into());
    }

    Ok(parsed.result)
}
