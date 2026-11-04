use atlas_lib::api::google::client::GoogleApiClient;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
        json!({ "items": [ {
            "id": "n1", "name": "vpc", "selfLink": "https://x/networks/vpc",
            "peerings": [ {
                "name": "servicenetworking-googleapis-com",
                "network": "https://www.googleapis.com/compute/v1/projects/tenant/global/networks/servicenetworking",
                "state": "ACTIVE",
                "exportCustomRoutes": false
            } ]
        } ] }),
    )
    .await;
    let nets = list_networks(&c, "p").await.expect("ok");
    assert_eq!(nets[0].self_link.as_deref(), Some("https://x/networks/vpc"));
    let peering = &nets[0].peerings.as_ref().expect("peerings")[0];
    assert_eq!(
        peering.network.as_deref(),
        Some(
            "https://www.googleapis.com/compute/v1/projects/tenant/global/networks/servicenetworking"
        )
    );
    assert_eq!(peering.state.as_deref(), Some("ACTIVE"));
}

#[tokio::test]
async fn compute_subnetworks_aggregated() {
    use atlas_lib::api::google::compute_network::list_subnetworks;
    let (_s, c) = serve(
        "/compute/v1/projects/p/aggregated/subnetworks",
        json!({ "items": {
            "regions/us-central1": { "subnetworks": [ {
                "name": "sn", "network": "vpc", "selfLink": "https://x/sn",
                "ipCidrRange": "10.30.0.0/24",
                "secondaryIpRanges": [ { "rangeName": "pods", "ipCidrRange": "10.31.0.0/16" } ],
                "internalIpv6Prefix": "fd20:1:2:3::/64",
                "externalIpv6Prefix": "2600:1900:4000:1::/64",
                "stackType": "IPV4_IPV6"
            } ] },
            "regions/empty": { "warning": { "code": "NO_RESULTS_ON_PAGE" } }
        } }),
    )
    .await;
    let subs = list_subnetworks(&c, "p").await.expect("ok");
    assert_eq!(subs.len(), 1, "empty scope skipped");
    assert_eq!(subs[0].network.as_deref(), Some("vpc"));
    assert_eq!(
        subs[0].address_space().collect::<Vec<_>>(),
        vec![
            "10.30.0.0/24",
            "fd20:1:2:3::/64",
            "2600:1900:4000:1::/64",
            "10.31.0.0/16"
        ]
    );
}

#[tokio::test]
async fn ncc_hubs_and_spokes() {
    use atlas_lib::api::google::network_connectivity::{list_hubs, list_spokes};
    let (_s, c) = serve(
        "/v1/projects/p/locations/global/hubs",
        json!({ "hubs": [ {
            "name": "projects/p/locations/global/hubs/hub-1",
            "state": "ACTIVE",
            "presetTopology": "MESH",
            "routingVpcs": [ { "uri": "https://x/projects/p/global/networks/vpc" } ]
        } ] }),
    )
    .await;
    let hubs = list_hubs(&c, "p").await.expect("ok");
    assert_eq!(
        hubs[0].name.as_deref(),
        Some("projects/p/locations/global/hubs/hub-1")
    );
    assert_eq!(hubs[0].state.as_deref(), Some("ACTIVE"));

    let (_s, c) = serve(
        "/v1/projects/p/locations/global/spokes",
        json!({ "spokes": [ {
            "name": "projects/p/locations/global/spokes/spoke-1",
            "hub": "projects/p/locations/global/hubs/hub-1",
            "group": "projects/p/locations/global/hubs/hub-1/groups/default",
            "state": "ACTIVE",
            "spokeType": "VPC_NETWORK",
            "linkedVpcNetwork": {
                "uri": "https://www.googleapis.com/compute/v1/projects/p/global/networks/vpc",
                "excludeExportRanges": []
            }
        } ], "unreachable": [] }),
    )
    .await;
    let spokes = list_spokes(&c, "p").await.expect("ok");
    let spoke = &spokes[0];
    assert_eq!(
        spoke.name.as_deref(),
        Some("projects/p/locations/global/spokes/spoke-1")
    );
    assert_eq!(
        spoke.hub.as_deref(),
        Some("projects/p/locations/global/hubs/hub-1")
    );
    assert_eq!(
        spoke.group.as_deref(),
        Some("projects/p/locations/global/hubs/hub-1/groups/default")
    );
    assert_eq!(spoke.state.as_deref(), Some("ACTIVE"));
    assert_eq!(
        spoke
            .linked_vpc_network
            .as_ref()
            .and_then(|linked| linked.uri.as_deref()),
        Some("https://www.googleapis.com/compute/v1/projects/p/global/networks/vpc")
    );
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
async fn vpn_gateways_aggregated() {
    use atlas_lib::api::google::vpn::list_vpn_gateways;
    let (_s, c) = serve(
        "/compute/v1/projects/p/aggregated/vpnGateways",
        json!({ "items": {
            "regions/us-central1": { "vpnGateways": [ {
                "name": "ha-gw",
                "network": "https://x/projects/p/global/networks/vpc",
                "selfLink": "https://x/projects/p/regions/us-central1/vpnGateways/ha-gw",
                "vpnInterfaces": [ { "id": 0, "ipAddress": "198.51.100.70" } ]
            } ] },
            "regions/empty": { "warning": { "code": "NO_RESULTS_ON_PAGE" } }
        } }),
    )
    .await;
    let gateways = list_vpn_gateways(&c, "p").await.expect("ok");
    assert_eq!(gateways.len(), 1, "empty scope skipped");
    assert_eq!(
        gateways[0].self_link.as_deref(),
        Some("https://x/projects/p/regions/us-central1/vpnGateways/ha-gw")
    );
    assert_eq!(
        gateways[0].network.as_deref(),
        Some("https://x/projects/p/global/networks/vpc")
    );
    assert_eq!(
        gateways[0].vpn_interfaces.as_ref().unwrap()[0]
            .ip_address
            .as_deref(),
        Some("198.51.100.70")
    );
}

#[tokio::test]
async fn vpn_tunnels_and_external_gateways() {
    use atlas_lib::api::google::vpn::{list_external_vpn_gateways, list_vpn_tunnels};
    let (_s, c) = serve(
        "/compute/v1/projects/p/aggregated/vpnTunnels",
        json!({ "items": {
            "regions/us-central1": { "vpnTunnels": [ {
                "name": "tun",
                "selfLink": "https://x/projects/p/regions/us-central1/vpnTunnels/tun",
                "status": "NO_INCOMING_PACKETS",
                "vpnGateway": "https://x/projects/p/regions/us-central1/vpnGateways/ha-gw",
                "peerExternalGateway": "https://x/projects/p/global/externalVpnGateways/aws-peer"
            } ] }
        } }),
    )
    .await;
    let tunnels = list_vpn_tunnels(&c, "p").await.expect("ok");
    assert_eq!(
        tunnels[0].self_link.as_deref(),
        Some("https://x/projects/p/regions/us-central1/vpnTunnels/tun")
    );
    assert_eq!(tunnels[0].status.as_deref(), Some("NO_INCOMING_PACKETS"));
    assert_eq!(
        tunnels[0].vpn_gateway.as_deref(),
        Some("https://x/projects/p/regions/us-central1/vpnGateways/ha-gw")
    );
    assert_eq!(
        tunnels[0].peer_external_gateway.as_deref(),
        Some("https://x/projects/p/global/externalVpnGateways/aws-peer")
    );

    let (_s, c) = serve(
        "/compute/v1/projects/p/global/externalVpnGateways",
        json!({ "items": [ {
            "name": "aws-peer",
            "selfLink": "https://x/projects/p/global/externalVpnGateways/aws-peer",
            "interfaces": [ { "id": 0, "ipAddress": "192.0.2.103" } ]
        } ] }),
    )
    .await;
    let external = list_external_vpn_gateways(&c, "p").await.expect("ok");
    assert_eq!(
        external[0].self_link.as_deref(),
        Some("https://x/projects/p/global/externalVpnGateways/aws-peer")
    );
    assert_eq!(
        external[0].interfaces.as_ref().unwrap()[0]
            .ip_address
            .as_deref(),
        Some("192.0.2.103")
    );
}

#[tokio::test]
async fn routers_and_interconnects() {
    use atlas_lib::api::google::interconnect::{
        list_interconnect_attachments, list_interconnects, list_routers,
    };
    let (_s, c) = serve(
        "/compute/v1/projects/p/aggregated/routers",
        json!({ "items": {
            "regions/us-east4": { "routers": [ {
                "name": "rtr",
                "selfLink": "https://x/projects/p/regions/us-east4/routers/rtr",
                "network": "https://x/projects/p/global/networks/vpc",
                "bgp": { "asn": 16550 }
            } ] },
            "regions/empty": { "warning": { "code": "NO_RESULTS_ON_PAGE" } }
        } }),
    )
    .await;
    let routers = list_routers(&c, "p").await.expect("ok");
    assert_eq!(routers.len(), 1, "empty scope skipped");
    assert_eq!(
        routers[0].self_link.as_deref(),
        Some("https://x/projects/p/regions/us-east4/routers/rtr")
    );
    assert_eq!(
        routers[0].network.as_deref(),
        Some("https://x/projects/p/global/networks/vpc")
    );

    let (_s, c) = serve(
        "/compute/v1/projects/p/global/interconnects",
        json!({ "items": [ {
            "name": "cci-aws",
            "selfLink": "https://x/projects/p/global/interconnects/cci-aws",
            "interconnectType": "DEDICATED",
            "state": "ACTIVE",
            "operationalStatus": "OS_ACTIVE"
        } ] }),
    )
    .await;
    let interconnects = list_interconnects(&c, "p").await.expect("ok");
    assert_eq!(
        interconnects[0].self_link.as_deref(),
        Some("https://x/projects/p/global/interconnects/cci-aws")
    );
    assert_eq!(interconnects[0].state.as_deref(), Some("ACTIVE"));

    let (_s, c) = serve(
        "/compute/v1/projects/p/aggregated/interconnectAttachments",
        json!({ "items": {
            "regions/us-east4": { "interconnectAttachments": [ {
                "name": "att-aws",
                "selfLink": "https://x/projects/p/regions/us-east4/interconnectAttachments/att-aws",
                "type": "DEDICATED",
                "state": "ACTIVE",
                "router": "https://x/projects/p/regions/us-east4/routers/rtr",
                "interconnect": "https://x/projects/p/global/interconnects/cci-aws",
                "cloudRouterIpAddress": "169.254.60.2/30",
                "customerRouterIpAddress": "169.254.60.1/30",
                "cloudRouterIpv6Address": "2600:2d00:0:2::1/125",
                "customerRouterIpv6Address": "2600:2d00:0:2::2/125"
            } ] }
        } }),
    )
    .await;
    let attachments = list_interconnect_attachments(&c, "p").await.expect("ok");
    let attachment = &attachments[0];
    assert_eq!(
        attachment.self_link.as_deref(),
        Some("https://x/projects/p/regions/us-east4/interconnectAttachments/att-aws")
    );
    assert_eq!(attachment.state.as_deref(), Some("ACTIVE"));
    assert_eq!(
        attachment.router.as_deref(),
        Some("https://x/projects/p/regions/us-east4/routers/rtr")
    );
    assert_eq!(
        attachment.interconnect.as_deref(),
        Some("https://x/projects/p/global/interconnects/cci-aws")
    );
    assert_eq!(
        attachment.cloud_router_ip_address.as_deref(),
        Some("169.254.60.2/30")
    );
    assert_eq!(
        attachment.customer_router_ip_address.as_deref(),
        Some("169.254.60.1/30")
    );
    assert_eq!(
        attachment.cloud_router_ipv6_address.as_deref(),
        Some("2600:2d00:0:2::1/125")
    );
    assert_eq!(
        attachment.customer_router_ipv6_address.as_deref(),
        Some("2600:2d00:0:2::2/125")
    );
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
        json!({ "clusters": [ {
            "name": "c1", "network": "vpc", "selfLink": "https://x/c1",
            "networkConfig": { "network": "projects/host/global/networks/vpc" }
        } ] }),
    )
    .await;
    let clusters = list_clusters(&c, "p").await.expect("ok");
    assert_eq!(clusters[0].network.as_deref(), Some("vpc"));
    assert_eq!(
        clusters[0]
            .network_config
            .as_ref()
            .and_then(|config| config.network.as_deref()),
        Some("projects/host/global/networks/vpc")
    );
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

mod disabled_apis {
    use atlas_lib::api::google::client::{GoogleApiClient, ProjectApis};
    use atlas_lib::api::google::network_connectivity::{list_hubs, list_spokes};
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const HUBS: &str = "/v1/projects/globex-prod/locations/global/hubs";
    const SPOKES: &str = "/v1/projects/globex-prod/locations/global/spokes";
    const PROJECT: &str = "/v3/projects/globex-prod";

    fn service_disabled(consumer: &str) -> Value {
        json!({ "error": {
            "code": 403,
            "message": format!("Network Connectivity API has not been used in {consumer} before or it is disabled."),
            "status": "PERMISSION_DENIED",
            "details": [
                {
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": "SERVICE_DISABLED",
                    "domain": "googleapis.com",
                    "metadata": {
                        "consumer": consumer,
                        "service": "networkconnectivity.googleapis.com"
                    }
                },
                { "@type": "type.googleapis.com/google.rpc.Help", "links": [] }
            ]
        } })
    }

    async fn answer(server: &MockServer, at: &str, status: u16, body: Value, calls: u64) {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .expect(calls)
            .mount(server)
            .await;
    }

    async fn named(server: &MockServer, number: &str) {
        answer(
            server,
            PROJECT,
            200,
            json!({ "name": format!("projects/{number}"), "projectId": "globex-prod" }),
            1,
        )
        .await;
    }

    #[tokio::test]
    async fn an_api_off_in_the_scanned_project_reads_as_empty_and_names_it_once() {
        let server = MockServer::start().await;
        answer(&server, HUBS, 403, service_disabled("projects/4815"), 1).await;
        answer(&server, SPOKES, 403, service_disabled("projects/4815"), 1).await;
        named(&server, "4815").await;
        let client = GoogleApiClient::with_base_url("test-token".into(), server.uri());
        let apis = ProjectApis::new(&client, "globex-prod");

        let (hubs, spokes) = futures::join!(
            apis.unless_disabled(list_hubs(&client, "globex-prod")),
            apis.unless_disabled(list_spokes(&client, "globex-prod")),
        );

        assert!(hubs.expect("an API that is off holds no hubs").is_empty());
        assert!(spokes.expect("nor spokes").is_empty());
    }

    #[tokio::test]
    async fn an_api_off_in_another_project_is_still_a_failure() {
        let server = MockServer::start().await;
        answer(
            &server,
            HUBS,
            403,
            service_disabled("projects/764086051850"),
            1,
        )
        .await;
        named(&server, "4815").await;
        let client = GoogleApiClient::with_base_url("test-token".into(), server.uri());

        let hubs = ProjectApis::new(&client, "globex-prod")
            .unless_disabled(list_hubs(&client, "globex-prod"))
            .await;

        let error = hubs.expect_err("a quota project's switch says nothing about globex-prod");
        assert!(error.to_string().contains("764086051850"), "{error}");
    }

    #[tokio::test]
    async fn an_api_off_where_the_project_cannot_be_named_is_still_a_failure() {
        let server = MockServer::start().await;
        answer(&server, HUBS, 403, service_disabled("projects/4815"), 1).await;
        answer(
            &server,
            PROJECT,
            403,
            json!({ "error": { "code": 403, "status": "PERMISSION_DENIED" } }),
            1,
        )
        .await;
        let client = GoogleApiClient::with_base_url("test-token".into(), server.uri());

        let hubs = ProjectApis::new(&client, "globex-prod")
            .unless_disabled(list_hubs(&client, "globex-prod"))
            .await;

        let error = hubs.expect_err("an unconfirmed switch is a failure");
        assert!(error.to_string().contains("could not tell"), "{error}");
    }

    #[tokio::test]
    async fn a_throttled_project_is_still_a_failure() {
        let server = MockServer::start().await;
        answer(
            &server,
            HUBS,
            429,
            json!({ "error": { "code": 429, "status": "RESOURCE_EXHAUSTED", "details": [{
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": "RATE_LIMIT_EXCEEDED",
                "domain": "googleapis.com",
                "metadata": {
                    "consumer": "projects/4815",
                    "service": "networkconnectivity.googleapis.com"
                }
            }] } }),
            1,
        )
        .await;
        answer(&server, PROJECT, 200, json!({}), 0).await;
        let client = GoogleApiClient::with_base_url("test-token".into(), server.uri());

        let hubs = ProjectApis::new(&client, "globex-prod")
            .unless_disabled(list_hubs(&client, "globex-prod"))
            .await;

        assert!(hubs.is_err());
    }
}
