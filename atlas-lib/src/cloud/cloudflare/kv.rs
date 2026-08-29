use cloudflare::endpoints::workerskv::WorkersKvNamespace;
use cloudflare::endpoints::workerskv::list_namespaces::{ListNamespaces, ListNamespacesParams};

const KV_NAMESPACES_PER_PAGE: u32 = 100;

pub async fn get_kv_namespaces(
    client: &cloudflare::framework::client::async_api::Client,
    account_id: &str,
) -> Result<Vec<WorkersKvNamespace>, Box<dyn std::error::Error>> {
    super::paginate(KV_NAMESPACES_PER_PAGE, |page| async move {
        client
            .request(&ListNamespaces {
                account_identifier: account_id,
                params: ListNamespacesParams {
                    page: Some(page),
                    per_page: Some(KV_NAMESPACES_PER_PAGE),
                    ..Default::default()
                },
            })
            .await
    })
    .await
}
