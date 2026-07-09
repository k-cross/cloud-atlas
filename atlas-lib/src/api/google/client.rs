use reqwest::{Client, RequestBuilder};

#[derive(Clone)]
pub struct GoogleApiClient {
    pub client: Client,
    pub token: String,
    /// Overrides the per-service API host when set — the seam that lets tests
    /// point every collector at a local mock server (see
    /// `tests/gcp_collector.rs`). `None` in production uses the real hosts.
    pub base_url: Option<String>,
}

impl GoogleApiClient {
    pub fn new(token: String) -> Self {
        Self {
            client: Client::new(),
            token,
            base_url: None,
        }
    }

    /// Dependency-injection constructor: pins every collector's requests to
    /// `base_url` (e.g. a mock server). Used by the collector tests.
    pub fn with_base_url(token: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            token,
            base_url: Some(base_url),
        }
    }

    /// Build a request URL, substituting `base_url` for the service host when
    /// one is configured. `path` starts with `/` (e.g. `/compute/v1/...`).
    pub fn endpoint(&self, default_host: &str, path: &str) -> String {
        let base = self.base_url.as_deref().unwrap_or(default_host);
        format!("{base}{path}")
    }

    pub fn get(&self, url: &str) -> RequestBuilder {
        self.client.get(url).bearer_auth(&self.token)
    }

    /// Fetch every page of a GCP list endpoint, extracting the items from
    /// each parsed page with `extract`. The `nextPageToken` field is read
    /// generically so response types don't need to declare it.
    pub async fn paginated_list<T, I, F>(
        &self,
        base_url: &str,
        context: &str,
        extract: F,
    ) -> Result<Vec<I>, Box<dyn std::error::Error>>
    where
        T: serde::de::DeserializeOwned,
        F: Fn(T) -> Option<Vec<I>>,
    {
        let mut all_items = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let url = match &page_token {
                Some(token) => {
                    let sep = if base_url.contains('?') { '&' } else { '?' };
                    format!("{}{}pageToken={}", base_url, sep, token)
                }
                None => base_url.to_owned(),
            };

            let res = self.get(&url).send().await?;
            let status = res.status();
            let text = res.text().await?;
            if !status.is_success() {
                return Err(format!("GCP API error ({}): {}", context, text).into());
            }

            let value: serde_json::Value = serde_json::from_str(&text)?;
            page_token = value
                .get("nextPageToken")
                .and_then(|t| t.as_str())
                .map(String::from);

            if let Some(items) = extract(serde_json::from_value(value)?) {
                all_items.extend(items);
            }

            if page_token.is_none() {
                return Ok(all_items);
            }
        }
    }
}
