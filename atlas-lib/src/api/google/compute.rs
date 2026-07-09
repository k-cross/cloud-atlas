use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: Option<String>, // the REST API returns ID as a string number e.g. "12345"
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub network_interfaces: Option<Vec<NetworkInterface>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Firewall {
    pub id: Option<String>,
    pub name: Option<String>,
    pub network: Option<String>,
    pub self_link: Option<String>,
    pub source_ranges: Option<Vec<String>>,
    pub destination_ranges: Option<Vec<String>>,
    pub allowed: Option<Vec<FirewallAllowed>>,
    pub direction: Option<String>,
    pub target_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FirewallAllowed {
    #[serde(rename = "IPProtocol")]
    pub ip_protocol: Option<String>,
    pub ports: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallListResponse {
    pub items: Option<Vec<Firewall>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub network: Option<String>,
    pub subnetwork: Option<String>,
    pub network_i_p: Option<String>, // 'networkIP' in camelCase deserializes to network_i_p by default unless explicitly specified, let's use explicit
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceAggregatedListResponse {
    pub items: Option<std::collections::HashMap<String, InstancesScopedList>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancesScopedList {
    pub instances: Option<Vec<Instance>>,
}

use super::client::GoogleApiClient;

pub async fn list_instances(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Instance>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!("/compute/v1/projects/{}/aggregated/instances", project),
    );
    client
        .paginated_list(&url, "instances", |r: InstanceAggregatedListResponse| {
            r.items.map(|items| {
                items
                    .into_values()
                    .filter_map(|scoped| scoped.instances)
                    .flatten()
                    .collect()
            })
        })
        .await
}

pub async fn list_firewalls(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Firewall>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!("/compute/v1/projects/{}/global/firewalls", project),
    );
    client
        .paginated_list(&url, "firewalls", |r: FirewallListResponse| r.items)
        .await
}

#[cfg(test)]
mod tests {
    use super::{InstanceAggregatedListResponse, list_instances};
    use crate::api::google::client::GoogleApiClient;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // A trimmed but realistic GCP `compute.instances.aggregatedList` body. Note
    // the id-as-string-number, the self_link the projector splits for
    // project/zone, and a zone with no instances (a `warning` scope) — all
    // shapes the collector must tolerate. Ideally captured from a real response
    // and committed as a golden; inlined here to keep the example self-contained.
    const AGGREGATED_INSTANCES: &str = r#"{
      "kind": "compute#instanceAggregatedList",
      "items": {
        "zones/us-central1-a": {
          "instances": [
            {
              "id": "1234567890123456789",
              "name": "web-1",
              "selfLink": "https://www.googleapis.com/compute/v1/projects/my-project/zones/us-central1-a/instances/web-1",
              "networkInterfaces": [
                { "network": "https://www.googleapis.com/compute/v1/projects/my-project/global/networks/default", "networkIP": "10.128.0.2" }
              ]
            }
          ]
        },
        "zones/us-central1-b": {
          "warning": { "code": "NO_RESULTS_ON_PAGE", "message": "There are no results for scope 'zones/us-central1-b'." }
        }
      }
    }"#;

    // Layer 1 — contract test: does the response shape still populate the exact
    // fields the projector depends on? Everything is `Option`, so this asserts
    // *values*, not merely that parsing didn't error (a mismatched struct would
    // parse into all-`None` and silently pass a weaker check).
    #[test]
    fn aggregated_instances_populates_the_fields_the_projector_reads() {
        let resp: InstanceAggregatedListResponse =
            serde_json::from_str(AGGREGATED_INSTANCES).expect("body deserializes");
        let items = resp.items.expect("items present");

        let scoped = items
            .get("zones/us-central1-a")
            .expect("zone scope present");
        let inst = &scoped.instances.as_ref().expect("instances present")[0];

        assert_eq!(inst.id.as_deref(), Some("1234567890123456789"));
        let self_link = inst.self_link.as_deref().expect("self_link present");
        assert!(self_link.contains("/projects/my-project/"));
        assert!(self_link.contains("/zones/us-central1-a/"));
        let network = inst.network_interfaces.as_ref().expect("nics present")[0]
            .network
            .as_deref()
            .expect("network present");
        assert!(network.ends_with("/networks/default"));

        // The empty scope must not blow up: no instances there.
        assert!(
            items
                .get("zones/us-central1-b")
                .unwrap()
                .instances
                .is_none()
        );
    }

    // Layer 2 — HTTP replay: run the real collector path (URL building, auth,
    // pagination loop, deserialization) against a mock server. Two pages prove
    // the `nextPageToken` follow-through, and results are gathered across zones.
    #[tokio::test]
    async fn list_instances_follows_pagination_across_pages() {
        let server = MockServer::start().await;
        let endpoint = "/compute/v1/projects/my-project/aggregated/instances";

        let page1 = serde_json::json!({
            "items": { "zones/z-a": { "instances": [
                { "id": "1", "name": "a", "selfLink": "https://x/zones/z-a/instances/a", "networkInterfaces": [] }
            ] } },
            "nextPageToken": "TOKEN2"
        });
        let page2 = serde_json::json!({
            "items": { "zones/z-b": { "instances": [
                { "id": "2", "name": "b", "selfLink": "https://x/zones/z-b/instances/b", "networkInterfaces": [] }
            ] } }
        });

        Mock::given(method("GET"))
            .and(path(endpoint))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page1))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(endpoint))
            .and(query_param("pageToken", "TOKEN2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page2))
            .mount(&server)
            .await;

        let client = GoogleApiClient::with_base_url("test-token".into(), server.uri());
        let instances = list_instances(&client, "my-project")
            .await
            .expect("collector succeeds");

        let ids: Vec<&str> = instances.iter().filter_map(|i| i.id.as_deref()).collect();
        assert_eq!(ids, ["1", "2"], "both pages, across zones");
    }

    // Error paths matter too: a non-2xx must surface as an error, not empty data.
    #[tokio::test]
    async fn list_instances_propagates_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("permission denied"))
            .mount(&server)
            .await;

        let client = GoogleApiClient::with_base_url("test-token".into(), server.uri());
        let result = list_instances(&client, "my-project").await;
        assert!(
            result.is_err(),
            "HTTP 403 must be an error, not empty success"
        );
    }
}
