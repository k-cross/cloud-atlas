use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DurableObjectNamespace {
    pub id: String,
    pub name: String,
    pub class: Option<String>,
    pub script: Option<String>,
}

pub async fn get_do_namespaces(
    client: &super::CloudflareApiClient,
    account_id: &str,
) -> Result<Vec<DurableObjectNamespace>, Box<dyn std::error::Error>> {
    client
        .get(
            &format!(
                "/client/v4/accounts/{}/workers/durable_objects/namespaces",
                account_id
            ),
            "DO namespaces",
        )
        .await
}
