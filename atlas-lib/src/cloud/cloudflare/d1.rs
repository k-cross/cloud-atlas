use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct D1Database {
    pub uuid: String,
    pub name: String,
    pub version: Option<String>,
}

pub async fn get_d1_databases(
    client: &reqwest::Client,
    account_id: &str,
    token: &str,
) -> Result<Vec<D1Database>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/d1/database",
        account_id
    );
    super::api_get(client, &url, token, "D1 databases").await
}
