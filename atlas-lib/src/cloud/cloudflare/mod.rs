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

/// GET a raw Cloudflare REST endpoint (one not covered by the `cloudflare`
/// crate) and unwrap the standard `{ success, result }` envelope.
async fn api_get<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    context: &str,
) -> Result<T, Box<dyn std::error::Error>> {
    let response = client.get(url).bearer_auth(token).send().await?;

    if !response.status().is_success() {
        return Err(format!("Failed to fetch {}: {}", context, response.status()).into());
    }

    let parsed: ApiResponse<T> = response.json().await?;
    if !parsed.success {
        return Err(format!("Cloudflare API returned success = false for {}", context).into());
    }

    Ok(parsed.result)
}
