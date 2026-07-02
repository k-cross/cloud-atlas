use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DurableObjectNamespace {
    pub id: String,
    pub name: String,
    pub class: Option<String>,
    pub script: Option<String>,
}

pub async fn get_do_namespaces(
    client: &reqwest::Client,
    account_id: &str,
    token: &str,
) -> Result<Vec<DurableObjectNamespace>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/workers/durable_objects/namespaces",
        account_id
    );
    super::api_get(client, &url, token, "DO namespaces").await
}
