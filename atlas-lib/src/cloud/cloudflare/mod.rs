pub mod d1;
pub mod dns;
pub mod durable_objects;
pub mod kv;
pub mod provider;
pub mod r2;
pub mod worker;
pub mod zone;

use serde::Deserialize;

#[derive(Deserialize)]
struct ApiResponse<T> {
    success: bool,
    result: T,
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

        Ok(parsed.result)
    }
}
