use azure_core::credentials::TokenCredential;
use azure_identity::AzureCliCredential;
use serde_json::Value;

#[derive(Clone)]
pub struct AzureApiClient {
    client: reqwest::Client,
    token: String,
    base_url: Option<String>,
}

impl AzureApiClient {
    const ARG_PAGE_SIZE: u32 = 1000;

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

    pub fn with_base_url(token: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            base_url: Some(base_url),
        }
    }

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

        let mut current_body = serde_json::json!({
            "subscriptions": subscriptions,
            "query": query,
            "options": {
                "$top": Self::ARG_PAGE_SIZE,
                "$skipToken": null
            }
        });

        let mut all_results = Vec::new();
        let mut expected_total: Option<u64> = None;

        loop {
            let res = self
                .client
                .post(&url)
                .bearer_auth(&self.token)
                .json(&current_body)
                .send()
                .await?;

            let status = res.status();
            let text = res.text().await?;

            if !status.is_success() {
                return Err(format!("Azure Resource Graph error: {}", text).into());
            }

            let mut parsed: Value = serde_json::from_str(&text)?;

            let Some(data) = parsed.get_mut("data").and_then(|d| d.as_array_mut()) else {
                return Err(format!(
                    "Azure Resource Graph returned no `data` array -- the response shape changed, \
                     or an error was delivered with a success status. Response starts: {}",
                    preview(&text)
                )
                .into());
            };
            all_results.append(data);

            if let Some(total) = parsed.get("totalRecords").and_then(|t| t.as_u64()) {
                expected_total = Some(total);
            }

            if let Some(skip_token) = parsed.get_mut("$skipToken")
                && !skip_token.is_null()
            {
                current_body["options"]["$skipToken"] = skip_token.take();
                continue;
            }

            if parsed.get("resultTruncated").and_then(|t| t.as_str()) == Some("true") {
                return Err(
                    "Azure Resource Graph truncated the result set without a continuation token; \
                     the returned inventory would be incomplete"
                        .into(),
                );
            }
            break;
        }

        if let Some(total) = expected_total
            && (all_results.len() as u64) < total
        {
            return Err(format!(
                "Azure Resource Graph reported {total} matching records but returned {}; the \
                 collected inventory would be incomplete",
                all_results.len()
            )
            .into());
        }

        Ok(all_results)
    }
}

fn preview(body: &str) -> String {
    const MAX_CHARS: usize = 200;
    match body.char_indices().nth(MAX_CHARS) {
        Some((end, _)) => format!("{}…", &body[..end]),
        None => body.to_owned(),
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

    async fn replay(body: serde_json::Value) -> Result<Vec<serde_json::Value>, String> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ARG_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        AzureApiClient::with_base_url("test-token".into(), server.uri())
            .query_graph("Resources", &["sub-1".into()])
            .await
            .map_err(|e| e.to_string())
    }

    #[tokio::test]
    async fn a_response_without_a_data_array_is_an_error_not_an_empty_tenant() {
        let shapes = [
            (
                "table format",
                json!({ "data": { "columns": [{ "name": "id" }], "rows": [["/x"]] } }),
            ),
            ("no data field", json!({ "$skipToken": null })),
            ("null data", json!({ "data": null })),
            (
                "error body with a 200",
                json!({ "error": { "code": "BadRequest", "message": "query invalid" } }),
            ),
        ];

        for (label, body) in shapes {
            let result = replay(body).await;
            assert!(
                result.is_err(),
                "{label} was accepted as an empty tenant: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_genuinely_empty_result_still_succeeds() {
        let rows = replay(json!({ "data": [], "totalRecords": 0, "$skipToken": null }))
            .await
            .expect("an empty tenant is not a failure");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn fewer_rows_than_the_response_claims_is_an_error() {
        let result = replay(json!({
            "data": [vm_resource("vm1")], "totalRecords": 4210, "$skipToken": null
        }))
        .await;

        let err = result.expect_err("a short read must not pass as complete");
        assert!(err.contains("4210"), "got: {err}");
    }

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
