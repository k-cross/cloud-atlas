use azure_core::credentials::TokenCredential;
use azure_identity::AzureCliCredential;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;

#[derive(Clone)]
pub struct AzureApiClient {
    client: reqwest::Client,
    token: String,
}

impl AzureApiClient {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let credential = AzureCliCredential::new(None)?;
        let token_response = credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await?;

        Ok(Self {
            client: reqwest::Client::new(),
            token: token_response.token.secret().to_string(),
        })
    }

    /// Run an Azure Resource Graph (ARG) query
    pub async fn query_graph(
        &self,
        query: &str,
        subscriptions: &[String],
    ) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let url = "https://management.azure.com/providers/Microsoft.ResourceGraph/resources?api-version=2021-03-01";

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.token))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let body = serde_json::json!({
            "subscriptions": subscriptions,
            "query": query,
            "options": {
                "$skipToken": null
            }
        });

        // Loop for pagination if needed
        let mut all_results = Vec::new();
        let mut current_body = body.clone();

        loop {
            let req = self
                .client
                .post(url)
                .headers(headers.clone())
                .json(&current_body);
            let res = req.send().await?;

            let status = res.status();
            let text = res.text().await?;

            if !status.is_success() {
                return Err(format!("Azure Resource Graph error: {}", text).into());
            }

            let mut parsed: Value = serde_json::from_str(&text)?;

            if let Some(data) = parsed.get_mut("data").and_then(|d| d.as_array_mut()) {
                all_results.append(data);
            }

            // Pagination handling for ARG
            if let Some(skip_token) = parsed.get("$skipToken")
                && !skip_token.is_null()
            {
                current_body["options"]["$skipToken"] = skip_token.clone();
                continue;
            }
            break;
        }

        Ok(all_results)
    }
}
