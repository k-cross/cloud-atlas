use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerScript {
    pub id: String,
    pub created_on: Option<String>,
    pub modified_on: Option<String>,
}

pub async fn get_workers(
    client: &super::CloudflareApiClient,
    account_id: &str,
) -> Result<Vec<WorkerScript>, Box<dyn std::error::Error>> {
    client
        .get(
            &format!("/client/v4/accounts/{}/workers/scripts", account_id),
            "workers",
        )
        .await
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerBinding {
    pub name: String,
    #[serde(rename = "type")]
    pub binding_type: String,

    // KV Namespace
    pub namespace_id: Option<String>,
    // R2 Bucket
    pub bucket_name: Option<String>,
    // D1
    pub id: Option<String>,

    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

pub async fn get_worker_bindings(
    client: &super::CloudflareApiClient,
    account_id: &str,
    script_name: &str,
) -> Result<Vec<WorkerBinding>, Box<dyn std::error::Error>> {
    client
        .get(
            &format!(
                "/client/v4/accounts/{}/workers/scripts/{}/bindings",
                account_id, script_name
            ),
            &format!("bindings for script {}", script_name),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::{WorkerScript, get_workers};
    use crate::cloud::cloudflare::CloudflareApiClient;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Layer 1 — contract: the `result` array items map to the id we key on.
    #[test]
    fn worker_script_deserializes_result_items() {
        let body = r#"[{"id":"my-worker","created_on":"2020-01-01T00:00:00Z","modified_on":null}]"#;
        let workers: Vec<WorkerScript> = serde_json::from_str(body).expect("deserializes");
        assert_eq!(workers[0].id, "my-worker");
        assert_eq!(
            workers[0].created_on.as_deref(),
            Some("2020-01-01T00:00:00Z")
        );
    }

    // Layer 2 — HTTP replay: the collector builds the right account-scoped path,
    // sends the bearer token, and unwraps the `{ success, result }` envelope.
    #[tokio::test]
    async fn get_workers_unwraps_the_success_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/client/v4/accounts/acct-1/workers/scripts"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "messages": [],
                "result": [ { "id": "w1" }, { "id": "w2" } ]
            })))
            .mount(&server)
            .await;

        let client = CloudflareApiClient::with_base_url("test-token".into(), server.uri());
        let workers = get_workers(&client, "acct-1")
            .await
            .expect("collector succeeds");
        let ids: Vec<&str> = workers.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["w1", "w2"]);
    }

    // The envelope's own failure flag must become an error even on HTTP 200.
    #[tokio::test]
    async fn get_workers_errors_when_envelope_reports_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": false,
                "errors": [ { "code": 10000, "message": "Authentication error" } ],
                "result": []
            })))
            .mount(&server)
            .await;

        let client = CloudflareApiClient::with_base_url("test-token".into(), server.uri());
        assert!(get_workers(&client, "acct-1").await.is_err());
    }
}
