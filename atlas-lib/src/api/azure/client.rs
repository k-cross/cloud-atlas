use azure_core::credentials::TokenCredential;
use azure_identity::AzureCliCredential;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;

#[derive(Clone)]
pub struct AzureApiClient {
    client: reqwest::Client,
    token: String,
    /// Overrides the ARG host when set — the seam that lets tests point the
    /// client at a mock server (see the tests below). `None` uses the real
    /// `management.azure.com`.
    base_url: Option<String>,
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
            base_url: None,
        })
    }

    /// Dependency-injection constructor: skips credential acquisition (so tests
    /// need no `az login`) and pins requests to `base_url` (e.g. a mock server).
    pub fn with_base_url(token: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url: Some(base_url),
        }
    }

    /// Run an Azure Resource Graph (ARG) query
    pub async fn query_graph(
        &self,
        query: &str,
        subscriptions: &[String],
    ) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let base = self
            .base_url
            .as_deref()
            .unwrap_or("https://management.azure.com");
        let url =
            format!("{base}/providers/Microsoft.ResourceGraph/resources?api-version=2021-03-01");

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
                .post(&url)
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

#[cfg(test)]
mod tests {
    use super::AzureApiClient;
    use crate::api::azure::models::AzureResource;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ARG_PATH: &str = "/providers/Microsoft.ResourceGraph/resources";

    fn vm_resource(name: &str) -> serde_json::Value {
        json!({
            "id": format!("/subscriptions/s/resourceGroups/rg/providers/Microsoft.Compute/virtualMachines/{name}"),
            "name": name,
            "type": "microsoft.compute/virtualmachines",
            "location": "eastus",
            "properties": { "hardwareProfile": { "vmSize": "Standard_D2s_v3" } }
        })
    }

    // Layer 1 — contract: an ARG row deserializes into the typed resource the
    // projector switches on (`type`) and reads (`name`/`location`/`properties`).
    #[test]
    fn arg_row_deserializes_into_azure_resource() {
        let res: AzureResource = serde_json::from_value(vm_resource("vm1")).expect("deserializes");
        assert_eq!(res.name.as_deref(), Some("vm1"));
        assert_eq!(
            res.r#type.as_deref(),
            Some("microsoft.compute/virtualmachines")
        );
        assert_eq!(res.location.as_deref(), Some("eastus"));
        assert!(
            res.properties.is_some(),
            "properties drive the typed mapping"
        );
    }

    // Layer 2 — HTTP replay: query_graph POSTs to ARG and returns the `data`
    // rows. No `az login`, no network.
    #[tokio::test]
    async fn query_graph_returns_the_data_rows() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ARG_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "data": [vm_resource("vm1")], "$skipToken": null })),
            )
            .mount(&server)
            .await;

        let client = AzureApiClient::with_base_url("test-token".into(), server.uri());
        let rows = client
            .query_graph("Resources", &["sub-1".into()])
            .await
            .expect("query succeeds");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "vm1");
    }

    // ARG paginates via `$skipToken`; the loop must fetch page 2 and concatenate.
    // `up_to_n_times(1)` makes the first mock answer once, then the fallback
    // serves the final page.
    #[tokio::test]
    async fn query_graph_follows_skiptoken_pagination() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ARG_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "data": [vm_resource("vm1")], "$skipToken": "TOKEN2" })),
            )
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(ARG_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "data": [vm_resource("vm2")], "$skipToken": null })),
            )
            .with_priority(2)
            .mount(&server)
            .await;

        let client = AzureApiClient::with_base_url("test-token".into(), server.uri());
        let rows = client
            .query_graph("Resources", &["sub-1".into()])
            .await
            .expect("query succeeds");

        let names: Vec<&str> = rows.iter().filter_map(|r| r["name"].as_str()).collect();
        assert_eq!(names, ["vm1", "vm2"], "both pages concatenated");
    }
}
