pub mod d1;
pub mod dns;
pub mod durable_objects;
pub mod kv;
pub mod provider;
pub mod r2;
pub mod worker;
pub mod zone;

use serde::Deserialize;
use std::future::Future;

/// Hard bound on any pagination loop here. An endpoint that ignores the page
/// parameter and keeps serving full pages would otherwise spin forever and hang
/// the scan tick; hitting this is reported as a failure rather than quietly
/// truncating the collection.
const MAX_PAGES: u32 = 1_000;

#[derive(Deserialize)]
struct ApiResponse<T> {
    success: bool,
    result: T,
    #[serde(default)]
    result_info: Option<serde_json::Value>,
}

/// Walk a page-numbered `cloudflare`-crate list endpoint to exhaustion.
///
/// Termination is driven by the response's own `result_info`, not by "this page
/// came back short". A short page is not proof of the end — Cloudflare may
/// clamp `per_page` below what we asked for — and ending there silently
/// truncates the collection, which the differ then reads as a mass deletion.
/// The short-page rule survives only as the fallback for endpoints that report
/// no `result_info` at all.
pub async fn paginate<T, Fut>(
    per_page: u32,
    fetch: impl Fn(u32) -> Fut,
) -> Result<Vec<T>, Box<dyn std::error::Error>>
where
    Fut: Future<
        Output = Result<
            cloudflare::framework::response::ApiSuccess<Vec<T>>,
            cloudflare::framework::response::ApiFailure,
        >,
    >,
{
    let mut items: Vec<T> = Vec::new();
    let mut page = 1;

    loop {
        let response = fetch(page).await?;
        let short_page = (response.result.len() as u32) < per_page;
        items.extend(response.result);

        if !more_pages(response.result_info.as_ref(), page, items.len(), short_page) {
            return Ok(items);
        }
        if page == MAX_PAGES {
            return Err(format!("pagination did not terminate after {MAX_PAGES} pages").into());
        }
        page += 1;
    }
}

/// Whether a page after `page` is expected, preferring what the API reports
/// about the totals over the shape of the page we just received.
fn more_pages(
    result_info: Option<&serde_json::Value>,
    page: u32,
    collected: usize,
    short_page: bool,
) -> bool {
    let count = |name: &str| {
        result_info
            .and_then(|info| info.get(name))
            .and_then(|value| value.as_u64())
    };

    if let Some(total_pages) = count("total_pages") {
        return u64::from(page) < total_pages;
    }
    if let Some(total_count) = count("total_count") {
        return (collected as u64) < total_count;
    }
    !short_page
}

/// Client for the raw Cloudflare REST endpoints not covered by the `cloudflare`
/// crate. Holds an overridable `base_url` seam so collector tests can point
/// every request at a mock server (see `worker.rs` tests).
#[derive(Clone)]
pub struct CloudflareApiClient {
    client: reqwest::Client,
    token: String,
    base_url: Option<String>,
}

impl CloudflareApiClient {
    pub fn new(token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url: None,
        }
    }

    /// Dependency-injection constructor: pins requests to `base_url` (e.g. a
    /// mock server). Used by the collector tests.
    pub fn with_base_url(token: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url: Some(base_url),
        }
    }

    /// `path` starts with `/` (e.g. `/client/v4/accounts/{id}/workers/scripts`).
    fn url(&self, path: &str) -> String {
        let base = self
            .base_url
            .as_deref()
            .unwrap_or("https://api.cloudflare.com");
        format!("{base}{path}")
    }

    /// GET a raw endpoint and unwrap the standard `{ success, result }`
    /// envelope.
    pub async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        context: &str,
    ) -> Result<T, Box<dyn std::error::Error>> {
        Ok(self.get_paged(path, context).await?.0)
    }

    /// `get`, keeping the `result_info` block alongside the payload — the
    /// cursor-paginated endpoints need it to find their next page.
    pub async fn get_paged<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        context: &str,
    ) -> Result<(T, Option<serde_json::Value>), Box<dyn std::error::Error>> {
        let response = self
            .client
            .get(self.url(path))
            .bearer_auth(&self.token)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Failed to fetch {}: {}", context, response.status()).into());
        }

        let parsed: ApiResponse<T> = response.json().await?;
        if !parsed.success {
            return Err(format!("Cloudflare API returned success = false for {}", context).into());
        }

        Ok((parsed.result, parsed.result_info))
    }
}
