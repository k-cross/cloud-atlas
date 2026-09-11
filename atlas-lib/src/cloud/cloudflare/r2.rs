use cloudflare::endpoints::r2::r2::{Bucket, ListBucketsResult};

const R2_BUCKETS_PER_PAGE: u32 = 100;

pub async fn get_r2_buckets(
    client: &super::CloudflareApiClient,
    account_id: &str,
) -> Result<Vec<Bucket>, Box<dyn std::error::Error>> {
    let mut buckets = Vec::new();
    let mut cursor: Option<String> = None;

    for _ in 0..super::MAX_PAGES {
        let mut path =
            format!("/client/v4/accounts/{account_id}/r2/buckets?per_page={R2_BUCKETS_PER_PAGE}");
        if let Some(cursor) = &cursor {
            let encoded: String = url::form_urlencoded::byte_serialize(cursor.as_bytes()).collect();
            path.push_str(&format!("&cursor={encoded}"));
        }

        let (page, result_info) = client
            .get_paged::<ListBucketsResult>(&path, "r2 buckets")
            .await?;

        let received = page.buckets.len();
        buckets.extend(page.buckets);

        cursor = result_info
            .as_ref()
            .and_then(|info| info.get("cursor"))
            .and_then(|cursor| cursor.as_str())
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_owned);

        if cursor.is_none() || received == 0 {
            return Ok(buckets);
        }
    }

    Err("r2 bucket pagination did not terminate".into())
}
