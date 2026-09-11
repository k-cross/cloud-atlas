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

const MAX_PAGES: u32 = 1_000;

#[derive(Deserialize)]
struct ApiResponse<T> {
    success: bool,
    result: T,
    #[serde(default)]
    result_info: Option<serde_json::Value>,
}

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

    pub fn with_base_url(token: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url: Some(base_url),
        }
    }

    fn url(&self, path: &str) -> String {
        let base = self
            .base_url
            .as_deref()
            .unwrap_or("https://api.cloudflare.com");
        format!("{base}{path}")
    }

    pub async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        context: &str,
    ) -> Result<T, Box<dyn std::error::Error>> {
        Ok(self.get_paged(path, context).await?.0)
    }

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
