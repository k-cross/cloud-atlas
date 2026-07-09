//! Coverage for the GCP collectors: each `list_*` runs its real path (URL
//! build, auth, pagination loop, deserialization, item extraction) against a
//! `wiremock` server via the `base_url` seam, and asserts the fields the
//! projector reads are populated. See `api/google/compute.rs` for the fuller
//! reference (pagination + error paths); these are the per-collector fan-out.

use atlas_lib::api::google::client::GoogleApiClient;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Serve `body` for a single GET `p`, returning a client pinned to the mock.
/// The server must stay in scope for the request to succeed.
async fn serve(p: &str, body: Value) -> (MockServer, GoogleApiClient) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let client = GoogleApiClient::with_base_url("test-token".into(), server.uri());
    (server, client)
}

#[tokio::test]
async fn compute_firewalls() {
    use atlas_lib::api::google::compute::list_firewalls;
    let (_s, c) = serve(
        "/compute/v1/projects/p/global/firewalls",
        json!({ "items": [
            { "id": "fw-1", "network": "net", "direction": "EGRESS", "destinationRanges": ["1.2.3.4/32"] }
        ] }),
    )
    .await;
    let fws = list_firewalls(&c, "p").await.expect("ok");
    assert_eq!(fws.len(), 1);
    assert_eq!(fws[0].id.as_deref(), Some("fw-1"));
    assert_eq!(fws[0].network.as_deref(), Some("net"));
    assert_eq!(fws[0].destination_ranges.as_ref().unwrap()[0], "1.2.3.4/32");
}

#[tokio::test]
async fn compute_networks() {
    use atlas_lib::api::google::compute_network::list_networks;
    let (_s, c) = serve(
        "/compute/v1/projects/p/global/networks",
        json!({ "items": [ { "id": "n1", "name": "vpc", "selfLink": "https://x/networks/vpc" } ] }),
    )
    .await;
    let nets = list_networks(&c, "p").await.expect("ok");
    assert_eq!(nets[0].self_link.as_deref(), Some("https://x/networks/vpc"));
}

#[tokio::test]
async fn compute_subnetworks_aggregated() {
    use atlas_lib::api::google::compute_network::list_subnetworks;
    let (_s, c) = serve(
        "/compute/v1/projects/p/aggregated/subnetworks",
        json!({ "items": {
            "regions/us-central1": { "subnetworks": [ { "name": "sn", "network": "vpc", "selfLink": "https://x/sn" } ] },
            "regions/empty": { "warning": { "code": "NO_RESULTS_ON_PAGE" } }
        } }),
    )
    .await;
    let subs = list_subnetworks(&c, "p").await.expect("ok");
    assert_eq!(subs.len(), 1, "empty scope skipped");
    assert_eq!(subs[0].network.as_deref(), Some("vpc"));
}

#[tokio::test]
async fn compute_forwarding_rules_aggregated() {
    use atlas_lib::api::google::compute_network::list_forwarding_rules;
    let (_s, c) = serve(
        "/compute/v1/projects/p/aggregated/forwardingRules",
        json!({ "items": {
            "regions/us-central1": { "forwardingRules": [ { "id": "fr1", "IPAddress": "34.1.2.3" } ] }
        } }),
    )
    .await;
    let rules = list_forwarding_rules(&c, "p").await.expect("ok");
    assert_eq!(rules[0].ip_address.as_deref(), Some("34.1.2.3"));
}

#[tokio::test]
async fn dns_managed_zones() {
    use atlas_lib::api::google::dns::list_managed_zones;
    let (_s, c) = serve(
        "/dns/v1/projects/p/managedZones",
        json!({ "managedZones": [ { "id": "1", "name": "zone-1", "dnsName": "example.com." } ] }),
    )
    .await;
    let zones = list_managed_zones(&c, "p").await.expect("ok");
    assert_eq!(zones[0].name.as_deref(), Some("zone-1"));
    assert_eq!(zones[0].dns_name.as_deref(), Some("example.com."));
}

#[tokio::test]
async fn cloud_functions() {
    use atlas_lib::api::google::functions::list_functions;
    let (_s, c) = serve(
        "/v2/projects/p/locations/-/functions",
        json!({ "functions": [ { "name": "projects/p/locations/us/functions/fn1", "environment": "GEN_2" } ] }),
    )
    .await;
    let fns = list_functions(&c, "p").await.expect("ok");
    assert_eq!(
        fns[0].name.as_deref().unwrap(),
        "projects/p/locations/us/functions/fn1"
    );
}

#[tokio::test]
async fn gke_clusters() {
    use atlas_lib::api::google::gke::list_clusters;
    let (_s, c) = serve(
        "/v1/projects/p/locations/-/clusters",
        json!({ "clusters": [ { "name": "c1", "network": "vpc", "selfLink": "https://x/c1" } ] }),
    )
    .await;
    let clusters = list_clusters(&c, "p").await.expect("ok");
    assert_eq!(clusters[0].network.as_deref(), Some("vpc"));
}

#[tokio::test]
async fn sql_instances() {
    use atlas_lib::api::google::sql::list_instances as list_sql;
    let (_s, c) = serve(
        "/sql/v1beta4/projects/p/instances",
        json!({ "items": [
            { "name": "db1", "ipAddresses": [ { "type": "PRIMARY", "ipAddress": "35.1.2.3" } ] }
        ] }),
    )
    .await;
    let dbs = list_sql(&c, "p").await.expect("ok");
    assert_eq!(dbs[0].name.as_deref(), Some("db1"));
    assert_eq!(
        dbs[0].ip_addresses.as_ref().unwrap()[0]
            .ip_address
            .as_deref(),
        Some("35.1.2.3")
    );
}

#[tokio::test]
async fn pubsub_topics_and_subscriptions() {
    let (_s, c) = serve(
        "/v1/projects/p/topics",
        json!({ "topics": [ { "name": "projects/p/topics/t1" } ] }),
    )
    .await;
    let topics = c.list_topics("p").await.expect("ok");
    assert_eq!(topics[0].name.as_deref(), Some("projects/p/topics/t1"));

    let (_s2, c2) = serve(
        "/v1/projects/p/subscriptions",
        json!({ "subscriptions": [ { "name": "projects/p/subscriptions/s1", "topic": "projects/p/topics/t1" } ] }),
    )
    .await;
    let subs = c2.list_subscriptions("p").await.expect("ok");
    assert_eq!(subs[0].topic.as_deref(), Some("projects/p/topics/t1"));
}

#[tokio::test]
async fn cloud_run_services() {
    let (_s, c) = serve(
        "/v2/projects/p/locations/-/services",
        json!({ "services": [ { "name": "svc", "uid": "u1", "uri": "https://svc-abc.run.app" } ] }),
    )
    .await;
    let services = c.list_run_services("p").await.expect("ok");
    assert_eq!(services[0].uri.as_deref(), Some("https://svc-abc.run.app"));
}

#[tokio::test]
async fn storage_buckets() {
    let (_s, c) = serve(
        "/storage/v1/b",
        json!({ "items": [ { "id": "b1", "name": "bucket-1", "location": "US" } ] }),
    )
    .await;
    let buckets = c.list_buckets("p").await.expect("ok");
    assert_eq!(buckets[0].name.as_deref(), Some("bucket-1"));
    assert_eq!(buckets[0].location.as_deref(), Some("US"));
}
