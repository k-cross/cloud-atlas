use atlas_cloudflare::CloudflareApiClient;
use atlas_cloudflare::account::get_accounts;
use atlas_cloudflare::d1::get_d1_databases;
use atlas_cloudflare::definition::{CloudflareCollection, ScriptId, ZoneId};
use atlas_cloudflare::dns::get_dns_records;
use atlas_cloudflare::durable_objects::get_do_namespaces;
use atlas_cloudflare::kv::get_kv_namespaces;
use atlas_cloudflare::provider::collect;
use atlas_cloudflare::r2::get_r2_buckets;
use atlas_cloudflare::zone::get_zones;
use atlas_graph::collection::{CollectionReport, CollectionSource, FailureKind};
use cloudflare::endpoints::dns::dns::DnsContent;
use cloudflare::framework::Environment;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::client::ClientConfig;
use cloudflare::framework::client::async_api::Client;
use serde_json::{Value, json};
use std::assert_matches;
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

#[tokio::test]
async fn zones_do_not_stop_early_when_the_api_clamps_per_page() {
    let server = MockServer::start().await;

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
    assert_matches!(&records[0].content, DnsContent::A { content } if content.to_string() == "203.0.113.10");
    assert_matches!(&records[1].content, DnsContent::AAAA { content } if content.to_string() == "2001:db8::1");
    assert_matches!(&records[2].content, DnsContent::CNAME { content } if content == "app.globex.com");
    assert_matches!(&records[3].content, DnsContent::MX { priority: 10, .. });
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

#[tokio::test]
async fn accounts_follow_the_total_count_past_a_clamped_page() {
    let server = MockServer::start().await;
    for (page, id) in [(1, "acct-1"), (2, "acct-2")] {
        Mock::given(method("GET"))
            .and(path("/client/v4/accounts"))
            .and(query_param("page", page.to_string()))
            .and(query_param("per_page", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true, "errors": [], "messages": [],
                "result": [{ "id": id, "name": id }],
                "result_info": { "page": page, "per_page": 1, "count": 1, "total_count": 2 }
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    let client = CloudflareApiClient::with_base_url("test-token".into(), server.uri());

    let accounts = get_accounts(&client).await.expect("ok");

    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["acct-1", "acct-2"]);
}

const ACCOUNT: &str = "/client/v4/accounts/acct-1";

fn envelope(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "success": true, "errors": [], "messages": [], "result": result
    }))
}

fn refusal() -> ResponseTemplate {
    ResponseTemplate::new(500).set_body_json(json!({
        "success": false,
        "errors": [{ "code": 10000, "message": "internal error" }],
        "messages": [],
        "result": null
    }))
}

fn globex_routes() -> Vec<(String, Value)> {
    let a_record = |id: &str, name: &str, address: &str| {
        dns_record_json(id, name, json!({ "type": "A", "content": address }))
    };
    vec![
        (
            "/client/v4/accounts".to_owned(),
            json!([{ "id": "acct-1", "name": "Globex Corp" }]),
        ),
        (
            "/client/v4/zones".to_owned(),
            json!([
                zone_json("zone-1", "globex.com", "acct-1"),
                zone_json("zone-2", "globex.io", "acct-1")
            ]),
        ),
        (
            "/client/v4/zones/zone-1/dns_records".to_owned(),
            json!([a_record("rec-com", "app.globex.com", "203.0.113.10")]),
        ),
        (
            "/client/v4/zones/zone-2/dns_records".to_owned(),
            json!([a_record("rec-io", "app.globex.io", "203.0.113.11")]),
        ),
        (
            format!("{ACCOUNT}/workers/scripts"),
            json!([{ "id": "edge-router" }, { "id": "cron-jobs" }]),
        ),
        (
            format!("{ACCOUNT}/workers/scripts/edge-router/bindings"),
            json!([{ "name": "SESSIONS", "type": "kv_namespace", "namespace_id": "kv-1" }]),
        ),
        (
            format!("{ACCOUNT}/workers/scripts/cron-jobs/bindings"),
            json!([]),
        ),
        (
            format!("{ACCOUNT}/storage/kv/namespaces"),
            json!([{ "id": "kv-1", "title": "sessions" }]),
        ),
        (
            format!("{ACCOUNT}/r2/buckets"),
            json!({ "buckets": [{ "name": "globex-assets", "creation_date": "2024-01-01T00:00:00Z" }] }),
        ),
        (
            format!("{ACCOUNT}/workers/durable_objects/namespaces"),
            json!([{ "id": "do-1", "name": "Counter", "class": "Counter", "script": "edge-router" }]),
        ),
        (
            format!("{ACCOUNT}/d1/database"),
            json!([{ "uuid": "d1-uuid", "name": "app-db", "version": "production" }]),
        ),
    ]
}

async fn globex_account(failing: &[&str]) -> MockServer {
    serve_routes(globex_routes(), failing).await
}

async fn serve_routes(routes: Vec<(String, Value)>, failing: &[&str]) -> MockServer {
    let server = MockServer::start().await;
    for (route, result) in routes {
        let response = if failing.contains(&route.as_str()) {
            refusal()
        } else {
            envelope(result)
        };
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
    }
    server
}

async fn collect_from(server: &MockServer) -> (CloudflareCollection, CollectionReport) {
    let client = crate_client(server);
    let cf = CloudflareApiClient::with_base_url("test-token".into(), server.uri());
    let mut data = CloudflareCollection::default();
    let mut report = CollectionReport::default();
    collect(&client, &cf, false, &mut data, &mut report).await;
    (data, report)
}

#[tokio::test]
async fn a_healthy_account_is_read_once_however_many_zones_it_owns() {
    let server = globex_account(&[]).await;

    let (data, report) = collect_from(&server).await;

    assert!(report.is_complete(), "{}", report.summary());
    assert_eq!(data.zones.len(), 2);
    assert_eq!(data.dns_records.len(), 2);
    assert_eq!(data.workers.len(), 2);
    assert_eq!(data.worker_bindings.len(), 2);
    assert_eq!(data.kv_namespaces.len(), 1);
    assert_eq!(data.r2_buckets.len(), 1);
    assert_eq!(data.durable_objects.len(), 1);
    assert_eq!(data.d1_databases.len(), 1);
}

#[tokio::test]
async fn a_failing_endpoint_is_attributed_and_costs_only_itself() {
    let failing = [
        "/client/v4/zones/zone-2/dns_records".to_owned(),
        format!("{ACCOUNT}/workers/scripts/cron-jobs/bindings"),
        format!("{ACCOUNT}/r2/buckets"),
    ];
    let failing: Vec<&str> = failing.iter().map(String::as_str).collect();
    let server = globex_account(&failing).await;

    let (data, report) = collect_from(&server).await;

    let mut scopes: Vec<&str> = report.failures.iter().map(|f| f.scope.as_str()).collect();
    scopes.sort_unstable();
    assert_eq!(
        scopes,
        [
            "account acct-1/r2_buckets",
            "account acct-1/worker cron-jobs/bindings",
            "zone globex.io/dns",
        ]
    );
    assert!(report.failures.iter().all(|f| {
        f.source == CollectionSource::Cloudflare && f.kind == FailureKind::Unavailable
    }));

    assert!(data.dns_records.contains_key(&ZoneId("zone-1".into())));
    assert!(!data.dns_records.contains_key(&ZoneId("zone-2".into())));
    assert_eq!(
        data.workers.len(),
        2,
        "a script whose bindings failed is still a script"
    );
    let cron = ScriptId {
        account: "acct-1".into(),
        script: "cron-jobs".into(),
    };
    assert!(!data.worker_bindings.contains_key(&cron));
    assert!(data.r2_buckets.is_empty());
    assert_eq!(data.kv_namespaces.len(), 1);
    assert_eq!(data.durable_objects.len(), 1);
    assert_eq!(data.d1_databases.len(), 1);
}

#[tokio::test]
async fn unreadable_zones_end_the_scan_before_anything_else_is_asked() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/client/v4/zones"))
        .respond_with(refusal())
        .expect(1)
        .mount(&server)
        .await;

    let (data, report) = collect_from(&server).await;

    let scopes: Vec<&str> = report.failures.iter().map(|f| f.scope.as_str()).collect();
    assert_eq!(scopes, ["zones"]);
    assert_eq!(report.failures[0].kind, FailureKind::Unavailable);
    assert!(data.zones.is_empty() && data.workers.is_empty());
    assert_eq!(
        server
            .received_requests()
            .await
            .map_or(0, |requests| requests.len()),
        1
    );
}

#[tokio::test]
async fn an_account_with_no_zones_is_still_read() {
    let account = "/client/v4/accounts/acct-edge";
    let server = serve_routes(
        vec![
            ("/client/v4/zones".to_owned(), json!([])),
            (
                "/client/v4/accounts".to_owned(),
                json!([{ "id": "acct-edge", "name": "Edge Only" }]),
            ),
            (
                format!("{account}/workers/scripts"),
                json!([{ "id": "edge-router" }]),
            ),
            (
                format!("{account}/workers/scripts/edge-router/bindings"),
                json!([]),
            ),
            (
                format!("{account}/storage/kv/namespaces"),
                json!([{ "id": "kv-edge", "title": "cache" }]),
            ),
            (format!("{account}/r2/buckets"), json!({ "buckets": [] })),
            (
                format!("{account}/workers/durable_objects/namespaces"),
                json!([]),
            ),
            (format!("{account}/d1/database"), json!([])),
        ],
        &[],
    )
    .await;

    let (data, report) = collect_from(&server).await;

    assert!(report.is_complete(), "{}", report.summary());
    assert!(data.zones.is_empty());
    assert_eq!(
        data.workers,
        [ScriptId {
            account: "acct-edge".into(),
            script: "edge-router".into(),
        }],
        "a Workers-only account owns no zone, so only the account listing can find it"
    );
    assert_eq!(data.kv_namespaces.len(), 1);
}

#[tokio::test]
async fn an_unlistable_account_set_is_reported_and_zone_accounts_are_still_read() {
    let server = globex_account(&["/client/v4/accounts"]).await;

    let (data, report) = collect_from(&server).await;

    let scopes: Vec<&str> = report.failures.iter().map(|f| f.scope.as_str()).collect();
    assert_eq!(
        scopes,
        ["accounts"],
        "an incomplete listing has to say so, or a missing account reads as deleted"
    );
    assert_eq!(report.failures[0].kind, FailureKind::Unavailable);
    assert_eq!(data.workers.len(), 2);
    assert_eq!(data.kv_namespaces.len(), 1);
}
