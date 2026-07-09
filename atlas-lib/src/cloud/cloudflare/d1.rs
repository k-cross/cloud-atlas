use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct D1Database {
    pub uuid: String,
    pub name: String,
    pub version: Option<String>,
}

pub async fn get_d1_databases(
    client: &super::CloudflareApiClient,
    account_id: &str,
) -> Result<Vec<D1Database>, Box<dyn std::error::Error>> {
    client
        .get(
            &format!("/client/v4/accounts/{}/d1/database", account_id),
            "D1 databases",
        )
        .await
}
