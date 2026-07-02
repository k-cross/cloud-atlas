use cloudflare::endpoints::workerskv::WorkersKvNamespace;
use cloudflare::endpoints::workerskv::list_namespaces::{ListNamespaces, ListNamespacesParams};

pub async fn get_kv_namespaces(
    client: &cloudflare::framework::client::async_api::Client,
    account_id: &str,
) -> Result<Vec<WorkersKvNamespace>, Box<dyn std::error::Error>> {
    let endpoint = ListNamespaces {
        account_identifier: account_id,
        params: ListNamespacesParams::default(),
    };
    let response = client.request(&endpoint).await?;
    Ok(response.result)
}
