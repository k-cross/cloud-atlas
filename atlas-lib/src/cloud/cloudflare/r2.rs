use cloudflare::endpoints::r2::r2::{Bucket, ListBuckets};

pub async fn get_r2_buckets(
    client: &cloudflare::framework::client::async_api::Client,
    account_id: &str,
) -> Result<Vec<Bucket>, Box<dyn std::error::Error>> {
    let endpoint = ListBuckets {
        account_identifier: account_id,
    };
    let response = client.request(&endpoint).await?;
    Ok(response.result.buckets)
}
