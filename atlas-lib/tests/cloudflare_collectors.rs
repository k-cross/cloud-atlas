//! Coverage for every Cloudflare collector, replayed against a `wiremock`
//! server so each runs its real path + envelope unwrap without credentials.
//! Two client seams are in play: the raw-REST collectors (d1, durable_objects,
//! and r2 — whose crate endpoint offers no pagination inputs) use
//! `CloudflareApiClient::with_base_url`, while the `cloudflare`-crate
//! collectors (zone, dns, kv) use `Environment::Custom`. The crate's result
//! structs are strict (mostly non-`Option`), so a drifted response body fails
//! to deserialize rather than silently yielding empties -- assertions still
//! pin the specific fields `provider.rs` and the projector read.
//! `worker.rs` holds `get_workers` and `get_worker_bindings`.

use atlas_lib::cloud::cloudflare::CloudflareApiClient;
use atlas_lib::cloud::cloudflare::d1::get_d1_databases;
use atlas_lib::cloud::cloudflare::dns::get_dns_records;
use atlas_lib::cloud::cloudflare::durable_objects::get_do_namespaces;
use atlas_lib::cloud::cloudflare::kv::get_kv_namespaces;
use atlas_lib::cloud::cloudflare::r2::get_r2_buckets;
use atlas_lib::cloud::cloudflare::zone::get_zones;
use cloudflare::endpoints::dns::dns::DnsContent;
use cloudflare::framework::Environment;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::client::ClientConfig;
use cloudflare::framework::client::async_api::Client;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
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

async fn serve_crate(p: &str, result: Value) -> (MockServer, Client) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": result
        })))
        .mount(&server)
        .await;
    let client = crate_client(&server);
    (server, client)
}

fn crate_client(server: &MockServer) -> Client {
    Client::new(
        Credentials::UserAuthToken {
            token: "test-token".into(),
        },
        ClientConfig::default(),
        Environment::Custom(format!("{}/client/v4/", server.uri())),
    )
    .expect("client")
}

fn zone_json(id: &str, name: &str, account_id: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "account": { "id": account_id, "name": "Globex Corp" },
        "activated_on": "2024-01-02T03:04:05Z",
        "betas": ["dns_ttl_reduced"],
        "created_on": "2024-01-01T00:00:00Z",
        "development_mode": 0,
        "meta": {
            "custom_certificate_quota": 0,
            "page_rule_quota": 3,
            "phishing_detected": false
        },
        "modified_on": "2024-02-01T00:00:00Z",
        "name_servers": ["ana.ns.cloudflare.com", "bob.ns.cloudflare.com"],
        "original_dnshost": null,
        "original_name_servers": ["ns1.registrar.com"],
        "original_registrar": "registrar, llc",
        "owner": { "type": "organization", "id": "org-1", "name": "Globex Corp" },
        "paused": false,
        "permissions": ["#zone:read", "#zone:edit"],
        "plan": {
            "id": "free",
            "name": "Free Website",
            "price": 0.0,
            "currency": "USD",
            "frequency": "",
            "legacy_id": "free",
            "is_subscribed": true,
            "can_subscribe": false
        },
        "status": "active",
        "type": "full"
    })
}

fn dns_record_json(id: &str, name: &str, content: Value) -> Value {
    let mut record = json!({
        "id": id,
        "name": name,
        "meta": {},
        "ttl": 300,
        "created_on": "2024-01-01T00:00:00Z",
        "modified_on": "2024-02-01T00:00:00Z",
        "proxiable": true,
        "proxied": false
    });
    let object = record.as_object_mut().expect("object");
    for (k, v) in content.as_object().expect("object") {
        object.insert(k.clone(), v.clone());
    }
    record
}

#[tokio::test]
async fn zones() {
    let (_s, c) = serve_crate(
        "/client/v4/zones",
        json!([zone_json("zone-1", "globex.com", "acct-1")]),
    )
    .await;

    let zones = get_zones(&c).await.expect("ok");

    assert_eq!(zones.len(), 1);
    assert_eq!(zones[0].id, "zone-1");
    assert_eq!(zones[0].name, "globex.com");
    assert_eq!(zones[0].account.id, "acct-1");
}

#[tokio::test]
async fn zones_follows_pagination_past_the_first_page() {
    let server = MockServer::start().await;

    let full_page: Vec<Value> = (0..50)
        .map(|i| zone_json(&format!("zone-{i}"), &format!("z{i}.globex.com"), "acct-1"))
        .collect();
    Mock::given(method("GET"))
        .and(path("/client/v4/zones"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": full_page
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/client/v4/zones"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [],
            "result": [zone_json("zone-50", "z50.globex.com", "acct-1")]
        })))
        .mount(&server)
        .await;

    let zones = get_zones(&crate_client(&server)).await.expect("ok");

    assert_eq!(
        zones.len(),
        51,
        "a full first page must not end collection -- zones beyond page 1 were dropped"
    );
    assert_eq!(zones[50].id, "zone-50");
}

// The failure this guards: terminating on "the page came back short" ends
// collection early whenever the API serves fewer records than we asked for,
// and the differ reads the missing tail as deletions. `result_info` is the
// authority on whether more pages exist.
#[tokio::test]
async fn zones_do_not_stop_early_when_the_api_clamps_per_page() {
    let server = MockServer::start().await;

    // Asked for 50, served 20 -- but there are two pages.
    let clamped: Vec<Value> = (0..20)
        .map(|i| zone_json(&format!("zone-{i}"), &format!("z{i}.globex.com"), "acct-1"))
        .collect();
    let info = json!({
        "page": 1, "per_page": 20, "count": 20, "total_count": 21, "total_pages": 2
    });

    Mock::given(method("GET"))
        .and(path("/client/v4/zones"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [],
            "result": clamped, "result_info": info
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/client/v4/zones"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [],
            "result": [zone_json("zone-20", "z20.globex.com", "acct-1")],
            "result_info": { "page": 2, "per_page": 20, "count": 1, "total_count": 21, "total_pages": 2 }
        })))
        .mount(&server)
        .await;

    let zones = get_zones(&crate_client(&server)).await.expect("ok");

    assert_eq!(
        zones.len(),
        21,
        "a clamped short page must not be mistaken for the last page"
    );
}

#[tokio::test]
async fn zones_surfaces_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/client/v4/zones"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "success": false,
            "errors": [{ "code": 9109, "message": "Invalid access token" }],
            "messages": [],
            "result": null
        })))
        .mount(&server)
        .await;

    let err = get_zones(&crate_client(&server)).await.expect_err("error");
    assert!(err.to_string().contains("9109"), "got: {err}");
}

#[tokio::test]
async fn dns_records() {
    let (_s, c) = serve_crate(
        "/client/v4/zones/zone-1/dns_records",
        json!([
            dns_record_json(
                "rec-a",
                "app.globex.com",
                json!({ "type": "A", "content": "203.0.113.10" })
            ),
            dns_record_json(
                "rec-aaaa",
                "v6.globex.com",
                json!({ "type": "AAAA", "content": "2001:db8::1" })
            ),
            dns_record_json(
                "rec-cname",
                "www.globex.com",
                json!({ "type": "CNAME", "content": "app.globex.com" })
            ),
            dns_record_json(
                "rec-mx",
                "globex.com",
                json!({ "type": "MX", "content": "mail.globex.com", "priority": 10 })
            ),
        ]),
    )
    .await;

    let records = get_dns_records(&c, "zone-1").await.expect("ok");

    assert_eq!(records.len(), 4);
    assert_eq!(records[0].id, "rec-a");
    assert_eq!(records[0].name, "app.globex.com");
    assert!(
        matches!(&records[0].content, DnsContent::A { content } if content.to_string() == "203.0.113.10")
    );
    assert!(
        matches!(&records[1].content, DnsContent::AAAA { content } if content.to_string() == "2001:db8::1")
    );
    assert!(
        matches!(&records[2].content, DnsContent::CNAME { content } if content == "app.globex.com")
    );
    assert!(matches!(
        &records[3].content,
        DnsContent::MX { priority: 10, .. }
    ));
}

#[tokio::test]
async fn kv_namespaces() {
    let (_s, c) = serve_crate(
        "/client/v4/accounts/acct-1/storage/kv/namespaces",
        json!([
            { "id": "kv-1", "title": "sessions", "supports_url_encoding": true },
            { "id": "kv-2", "title": "feature-flags" }
        ]),
    )
    .await;

    let namespaces = get_kv_namespaces(&c, "acct-1").await.expect("ok");

    assert_eq!(namespaces.len(), 2);
    assert_eq!(namespaces[0].id, "kv-1");
    assert_eq!(namespaces[0].title, "sessions");
    assert_eq!(namespaces[1].id, "kv-2");
    assert_eq!(namespaces[1].supports_url_encoding, None);
}

// R2 goes through the raw seam, not the crate: `ListBuckets` has no pagination
// inputs, and R2 pages by cursor rather than page number.
#[tokio::test]
async fn r2_buckets() {
    let (_s, c) = serve(
        "/client/v4/accounts/acct-1/r2/buckets",
        json!({
            "buckets": [
                { "name": "globex-assets", "creation_date": "2024-01-01T00:00:00Z" },
                { "name": "globex-backups", "creation_date": "2024-03-04T05:06:07Z" }
            ]
        }),
    )
    .await;

    let buckets = get_r2_buckets(&c, "acct-1").await.expect("ok");

    assert_eq!(buckets.len(), 2);
    assert_eq!(buckets[0].name, "globex-assets");
    assert_eq!(buckets[1].name, "globex-backups");
}

#[tokio::test]
async fn r2_buckets_follow_the_cursor_past_the_first_page() {
    let server = MockServer::start().await;
    let bucket = |name: &str| json!({ "name": name, "creation_date": "2024-01-01T00:00:00Z" });

    Mock::given(method("GET"))
        .and(path("/client/v4/accounts/acct-1/r2/buckets"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [],
            "result": { "buckets": [bucket("globex-assets")] },
            "result_info": { "cursor": "page/2==" }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/client/v4/accounts/acct-1/r2/buckets"))
        .and(query_param("cursor", "page/2=="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [],
            "result": { "buckets": [bucket("globex-backups")] },
            "result_info": {}
        })))
        .mount(&server)
        .await;

    let client = CloudflareApiClient::with_base_url("test-token".into(), server.uri());
    let buckets = get_r2_buckets(&client, "acct-1").await.expect("ok");

    let names: Vec<&str> = buckets.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        ["globex-assets", "globex-backups"],
        "buckets past the first cursor page were dropped"
    );
}
