use cloudflare::endpoints::workerskv::WorkersKvNamespace;
use cloudflare::endpoints::workerskv::list_namespaces::{ListNamespaces, ListNamespacesParams};

const KV_NAMESPACES_PER_PAGE: u32 = 100;

pub async fn get_kv_namespaces(
    client: &cloudflare::framework::client::async_api::Client,
    account_id: &str,
) -> Result<Vec<WorkersKvNamespace>, Box<dyn std::error::Error>> {
    let mut namespaces = Vec::new();
    let mut page = 1;

    loop {
        let endpoint = ListNamespaces {
            account_identifier: account_id,
            params: ListNamespacesParams {
                page: Some(page),
                per_page: Some(KV_NAMESPACES_PER_PAGE),
                ..Default::default()
            },
        };
        let response = client.request(&endpoint).await?;
        let received = response.result.len() as u32;
        namespaces.extend(response.result);

        if received < KV_NAMESPACES_PER_PAGE {
            break;
        }
        page += 1;
    }

    Ok(namespaces)
}
