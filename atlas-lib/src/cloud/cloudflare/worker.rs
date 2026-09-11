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
    pub namespace_id: Option<String>,
    pub bucket_name: Option<String>,
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
    use super::{WorkerBinding, WorkerScript, get_worker_bindings, get_workers};
    use crate::cloud::cloudflare::CloudflareApiClient;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    #[test]
    fn worker_binding_type_strings_match_the_projector_arms() {
        let body = r#"[
            {"name":"SESSIONS","type":"kv_namespace","namespace_id":"kv-1"},
            {"name":"MEDIA","type":"r2_bucket","bucket_name":"media"},
            {"name":"COORD","type":"durable_object_namespace","namespace_id":"do-1"},
            {"name":"EDGE","type":"d1","id":"d1-1"},
            {"name":"ANALYTICS_DB","type":"secret_text","text":"postgres://db.internal/app"}
        ]"#;
        let bindings: Vec<WorkerBinding> = serde_json::from_str(body).expect("deserializes");

        let types: Vec<&str> = bindings.iter().map(|b| b.binding_type.as_str()).collect();
        assert_eq!(
            types,
            [
                "kv_namespace",
                "r2_bucket",
                "durable_object_namespace",
                "d1",
                "secret_text"
            ]
        );

        assert_eq!(bindings[0].namespace_id.as_deref(), Some("kv-1"));
        assert_eq!(bindings[1].bucket_name.as_deref(), Some("media"));
        assert_eq!(bindings[3].id.as_deref(), Some("d1-1"));
        assert_eq!(
            bindings[4].extra.get("text").and_then(|t| t.as_str()),
            Some("postgres://db.internal/app"),
            "secret payload must land in `extra` for the ExternalService probe"
        );
    }

    #[tokio::test]
    async fn get_worker_bindings_unwraps_the_success_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/client/v4/accounts/acct-1/workers/scripts/edge-router/bindings",
            ))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "errors": [],
                "messages": [],
                "result": [ { "name": "SESSIONS", "type": "kv_namespace", "namespace_id": "kv-1" } ]
            })))
            .mount(&server)
            .await;

        let client = CloudflareApiClient::with_base_url("test-token".into(), server.uri());
        let bindings = get_worker_bindings(&client, "acct-1", "edge-router")
            .await
            .expect("collector succeeds");
        assert_eq!(bindings[0].binding_type, "kv_namespace");
        assert_eq!(bindings[0].namespace_id.as_deref(), Some("kv-1"));
    }
}
