//! Coverage for the raw-REST Cloudflare collectors (those not covered by the
//! `cloudflare` crate). Each runs its real path + `{ success, result }` envelope
//! unwrap against a `wiremock` server via `CloudflareApiClient::with_base_url`.
//! `worker.rs` holds the fuller reference (envelope + success:false error).

use atlas_lib::cloud::cloudflare::CloudflareApiClient;
use atlas_lib::cloud::cloudflare::d1::get_d1_databases;
use atlas_lib::cloud::cloudflare::durable_objects::get_do_namespaces;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn serve(p: &str, result: Value) -> (MockServer, CloudflareApiClient) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": result
        })))
        .mount(&server)
        .await;
    let client = CloudflareApiClient::with_base_url("test-token".into(), server.uri());
    (server, client)
}

#[tokio::test]
async fn d1_databases() {
    let (_s, c) = serve(
        "/client/v4/accounts/acct-1/d1/database",
        json!([ { "uuid": "d1-uuid", "name": "app-db", "version": "production" } ]),
    )
    .await;
    let dbs = get_d1_databases(&c, "acct-1").await.expect("ok");
    assert_eq!(dbs[0].uuid, "d1-uuid");
    assert_eq!(dbs[0].name, "app-db");
}

#[tokio::test]
async fn durable_object_namespaces() {
    let (_s, c) = serve(
        "/client/v4/accounts/acct-1/workers/durable_objects/namespaces",
        json!([ { "id": "do-1", "name": "Counter", "class": "Counter", "script": "my-worker" } ]),
    )
    .await;
    let dos = get_do_namespaces(&c, "acct-1").await.expect("ok");
    assert_eq!(dos[0].id, "do-1");
    assert_eq!(dos[0].script.as_deref(), Some("my-worker"));
}
