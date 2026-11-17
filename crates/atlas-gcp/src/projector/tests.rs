use super::*;
use crate::fixtures::{self, gcp_network};
use atlas_graph::testing::{assert_edge, assert_has_node, link};

#[test]
fn gcp_projection() {
    let mut builder = GraphBuilder::new();
    gcp_projector(&mut builder, &fixtures::collections());

    let project = Node::GcpProject(atlas_provider::fixtures::GCP_PROJECT.into());
    let zone = Node::GcpComputeZone("us-central1-a".into());
    let instance = Node::GcpComputeInstance("7000000000000000001".into());
    let network = Node::GcpComputeNetwork(gcp_network().as_str().into());

    assert_edge(&builder, &project, &instance, &Edge::DependsOn);
    assert_edge(&builder, &zone, &instance, &Edge::Contains);

    let fw = Node::GcpComputeFirewall("fw-egress-globex".into());
    assert_edge(&builder, &network, &fw, &Edge::Contains);
    assert_edge(
        &builder,
        &fw,
        &Node::GenericIpAddress("192.0.2.55".into()),
        &Edge::RoutesTo,
    );

    assert_edge(
        &builder,
        &Node::GcpSqlInstance("globex-analytics-db".into()),
        &Node::ip("10.20.0.5", &gcp_network()),
        &Edge::ConnectsTo,
    );

    // Both halves of an instance's interface: the internal address and the
    // one-to-one NAT address a flow log records for its egress.
    let gce = Node::GcpComputeInstance("7000000000000000001".into());
    for address in ["10.30.0.11", "34.120.0.21", "2600:1900:4000:1::15"] {
        assert_edge(
            &builder,
            &gce,
            &Node::ip(address, &gcp_network()),
            &Edge::ConnectsTo,
        );
    }
    assert_edge(&builder, &network, &gce, &Edge::Contains);
    assert_edge(
        &builder,
        &gce,
        &Node::GcpComputeSubnetwork(
            format!(
                "https://www.googleapis.com/compute/v1/projects/{}/regions/us-central1/subnetworks/subnet-data",
                atlas_provider::fixtures::GCP_PROJECT
            )
            .as_str()
            .into(),
        ),
        &Edge::AttachedTo,
    );

    assert_edge(
        &builder,
        &Node::GenericHostname("run.globex.app".into()),
        &Node::GcpCloudRunService(
            format!(
                "projects/{}/locations/us-central1/services/globex-api",
                atlas_provider::fixtures::GCP_PROJECT
            )
            .as_str()
            .into(),
        ),
        &Edge::RoutesTo,
    );

    assert_edge(
        &builder,
        &network,
        &Node::GcpGkeCluster("gke-globex".into()),
        &Edge::Contains,
    );
    assert_edge(
        &builder,
        &Node::GcpComputeForwardingRule("fr-globex-lb".into()),
        &Node::GenericIpAddress("34.120.0.9".into()),
        &Edge::ConnectsTo,
    );

    assert_edge(
        &builder,
        &Node::GcpPubSubSubscription(
            format!(
                "projects/{}/subscriptions/globex-sink",
                atlas_provider::fixtures::GCP_PROJECT
            )
            .as_str()
            .into(),
        ),
        &Node::GcpPubSubTopic(
            format!(
                "projects/{}/topics/globex-events",
                atlas_provider::fixtures::GCP_PROJECT
            )
            .as_str()
            .into(),
        ),
        &Edge::ConnectsTo,
    );

    assert_edge(
        &builder,
        &network,
        &Node::GcpComputeNetwork(
            "projects/globex-sql-tenant/global/networks/servicenetworking".into(),
        ),
        &Edge::ConnectsTo,
    );
    assert!(
        !builder.contains(&Node::GcpComputeNetwork(
            "projects/globex-legacy/global/networks/legacy".into()
        )),
        "an inactive peering joins nothing"
    );

    assert_has_node(&builder, &Node::GcpDnsManagedZone("globex-internal".into()));
    assert_has_node(&builder, &Node::GcpStorageBucket("globex-datalake".into()));
    assert_has_node(
        &builder,
        &Node::GcpCloudFunction(
            format!(
                "projects/{}/locations/us-central1/functions/resize-images",
                atlas_provider::fixtures::GCP_PROJECT
            )
            .as_str()
            .into(),
        ),
    );
}

#[test]
fn a_gcp_tunnel_is_projected_unless_it_is_being_built_or_torn_down() {
    use crate::api::vpn::VpnTunnel;
    use crate::definition::GoogleCollection;

    let link = |name: &str| format!("https://x/projects/p/regions/r/vpnTunnels/{name}");
    let tunnel = |name: &str, status: &str| VpnTunnel {
        self_link: Some(link(name)),
        status: Some(status.to_owned()),
        vpn_gateway: Some("https://x/projects/p/regions/r/vpnGateways/gw".to_owned()),
        ..Default::default()
    };
    let statuses = [
        ("established", "ESTABLISHED"),
        ("no-packets", "NO_INCOMING_PACKETS"),
        ("negotiation", "NEGOTIATION_FAILURE"),
        ("provisioning", "PROVISIONING"),
        ("allocating", "ALLOCATING_RESOURCES"),
        ("deprovisioning", "DEPROVISIONING"),
        ("failed", "FAILED"),
    ];
    let mut builder = GraphBuilder::new();
    gcp_projector(
        &mut builder,
        &[(
            "p".to_owned(),
            GoogleCollection::GoogleVpnTunnels(
                statuses
                    .iter()
                    .map(|(name, status)| tunnel(name, status))
                    .collect(),
            ),
        )],
    );

    let gateway = Node::GcpComputeVpnGateway("projects/p/regions/r/vpnGateways/gw".into());
    for (name, _) in &statuses[..3] {
        assert_edge(
            &builder,
            &Node::GcpComputeVpnTunnel(format!("projects/p/regions/r/vpnTunnels/{name}").into()),
            &gateway,
            &Edge::AttachedTo,
        );
    }
    for (name, status) in &statuses[3..] {
        let tunnel =
            Node::GcpComputeVpnTunnel(format!("projects/p/regions/r/vpnTunnels/{name}").into());
        assert!(!builder.contains(&tunnel), "a {status} tunnel is not built");
    }
}

#[test]
fn an_interconnect_attachment_is_projected_unless_it_is_being_built_or_torn_down() {
    use crate::api::interconnect::{Interconnect, InterconnectAttachment, Router};
    use crate::definition::GoogleCollection;

    let compute = "https://x/compute/v1/projects/p";
    let attachment = |name: &str, state: Option<&str>| InterconnectAttachment {
        self_link: Some(format!(
            "{compute}/regions/r/interconnectAttachments/{name}"
        )),
        state: state.map(str::to_owned),
        router: Some(format!("{compute}/regions/r/routers/rtr")),
        interconnect: Some(format!("{compute}/global/interconnects/ic")),
        cloud_router_ip_address: Some("169.254.10.1/29".to_owned()),
        customer_router_ip_address: Some("169.254.10.2/29".to_owned()),
        cloud_router_ipv6_address: Some("2600:2d00:0:2::1/125".to_owned()),
        customer_router_ipv6_address: Some("2600:2d00:0:2::2/125".to_owned()),
        ..Default::default()
    };
    let mut builder = GraphBuilder::new();
    gcp_projector(
        &mut builder,
        &[
            (
                "p".to_owned(),
                GoogleCollection::GoogleRouters(vec![Router {
                    self_link: Some(format!("{compute}/regions/r/routers/rtr")),
                    network: Some(format!("{compute}/global/networks/net")),
                    ..Default::default()
                }]),
            ),
            (
                "p".to_owned(),
                GoogleCollection::GoogleInterconnects(vec![
                    Interconnect {
                        self_link: Some(format!("{compute}/global/interconnects/ic")),
                        state: Some("ACTIVE".to_owned()),
                        ..Default::default()
                    },
                    Interconnect {
                        self_link: Some(format!("{compute}/global/interconnects/unused")),
                        state: Some("UNPROVISIONED".to_owned()),
                        ..Default::default()
                    },
                ]),
            ),
            (
                "p".to_owned(),
                GoogleCollection::GoogleInterconnectAttachments(vec![
                    attachment("active", Some("ACTIVE")),
                    attachment("unspecified", None),
                    attachment("pending-partner", Some("PENDING_PARTNER")),
                    attachment("pending-customer", Some("PENDING_CUSTOMER")),
                    attachment("requested", Some("PARTNER_REQUEST_RECEIVED")),
                    attachment("unprovisioned", Some("UNPROVISIONED")),
                    attachment("defunct", Some("DEFUNCT")),
                    InterconnectAttachment {
                        interconnect: Some(format!("{compute}/global/interconnects/unused")),
                        ..attachment("on-unused", Some("ACTIVE"))
                    },
                ]),
            ),
        ],
    );

    let router = Node::GcpComputeRouter("projects/p/regions/r/routers/rtr".into());
    assert_edge(
        &builder,
        &router,
        &Node::GcpComputeNetwork("projects/p/global/networks/net".into()),
        &Edge::AttachedTo,
    );
    let attachment_node = |name: &str| {
        Node::GcpComputeInterconnectAttachment(
            format!("projects/p/regions/r/interconnectAttachments/{name}").into(),
        )
    };
    let active = attachment_node("active");
    assert_edge(&builder, &active, &router, &Edge::AttachedTo);
    assert_edge(
        &builder,
        &active,
        &Node::GcpComputeInterconnect("projects/p/global/interconnects/ic".into()),
        &Edge::AttachedTo,
    );
    assert_edge(
        &builder,
        &active,
        &link("169.254.10.1/29"),
        &Edge::ConnectsTo,
    );
    assert_edge(&builder, &active, &link("169.254.10.2/29"), &Edge::RoutesTo);
    assert_edge(
        &builder,
        &active,
        &link("2600:2d00:0:2::1/125"),
        &Edge::ConnectsTo,
    );
    assert_edge(
        &builder,
        &active,
        &link("2600:2d00:0:2::2/125"),
        &Edge::RoutesTo,
    );
    assert_has_node(&builder, &attachment_node("unspecified"));
    assert_has_node(&builder, &attachment_node("on-unused"));
    for name in [
        "pending-partner",
        "pending-customer",
        "requested",
        "unprovisioned",
        "defunct",
    ] {
        assert!(
            !builder.contains(&attachment_node(name)),
            "{name} is not built"
        );
    }
    assert!(!builder.contains(&Node::GcpComputeInterconnect(
        "projects/p/global/interconnects/unused".into()
    )));
}

#[test]
fn a_gke_network_named_bare_is_the_same_network_instances_name_by_url() {
    use crate::api::gke::Cluster;
    use crate::definition::GoogleCollection;

    let mut builder = GraphBuilder::new();
    gcp_projector(
        &mut builder,
        &[(
            "p".to_owned(),
            GoogleCollection::GoogleGke(vec![Cluster {
                name: Some("c".to_owned()),
                network: Some("default".to_owned()),
                ..Default::default()
            }]),
        )],
    );

    assert_edge(
        &builder,
        &Node::GcpComputeNetwork("projects/p/global/networks/default".into()),
        &Node::GcpGkeCluster("c".into()),
        &Edge::Contains,
    );
}

#[test]
fn an_ncc_spoke_joins_its_hub_only_while_both_serve() {
    use crate::api::network_connectivity::{Hub, LinkedVpcNetwork, Spoke};
    use crate::definition::GoogleCollection;

    let hub = |name: &str, state: &str| Hub {
        name: Some(format!("projects/p/locations/global/hubs/{name}")),
        state: Some(state.to_owned()),
    };
    let spoke = |name: &str, hub: &str, group: Option<&str>, state: &str| Spoke {
        name: Some(format!("projects/p/locations/global/spokes/{name}")),
        hub: Some(format!("projects/p/locations/global/hubs/{hub}")),
        group: group.map(|group| format!("projects/p/locations/global/hubs/{hub}/groups/{group}")),
        state: Some(state.to_owned()),
        linked_vpc_network: Some(LinkedVpcNetwork {
            uri: Some(format!(
                "https://x/compute/v1/projects/p/global/networks/{name}"
            )),
        }),
    };
    let mut builder = GraphBuilder::new();
    gcp_projector(
        &mut builder,
        &[
            (
                "p".to_owned(),
                GoogleCollection::GoogleHubs(vec![hub("live", "ACTIVE"), hub("going", "DELETING")]),
            ),
            (
                "p".to_owned(),
                GoogleCollection::GoogleSpokes(vec![
                    spoke("net-a", "live", Some("center"), "ACTIVE"),
                    spoke("net-b", "live", None, "UPDATING"),
                    spoke("net-c", "live", Some("center"), "INACTIVE"),
                    spoke("net-d", "going", Some("default"), "ACTIVE"),
                ]),
            ),
        ],
    );

    let live = Node::GcpNetworkConnectivityHub("projects/p/locations/global/hubs/live".into());
    let center = Node::GcpNetworkConnectivityGroup(
        "projects/p/locations/global/hubs/live/groups/center".into(),
    );
    let default = Node::GcpNetworkConnectivityGroup(
        "projects/p/locations/global/hubs/live/groups/default".into(),
    );
    let spoke_a =
        Node::GcpNetworkConnectivitySpoke("projects/p/locations/global/spokes/net-a".into());
    assert_edge(&builder, &live, &center, &Edge::Contains);
    assert_edge(&builder, &spoke_a, &center, &Edge::AttachedTo);
    assert_edge(
        &builder,
        &spoke_a,
        &Node::GcpComputeNetwork("projects/p/global/networks/net-a".into()),
        &Edge::AttachedTo,
    );
    assert_edge(
        &builder,
        &Node::GcpNetworkConnectivitySpoke("projects/p/locations/global/spokes/net-b".into()),
        &default,
        &Edge::AttachedTo,
    );
    for absent in [
        Node::GcpNetworkConnectivitySpoke("projects/p/locations/global/spokes/net-c".into()),
        Node::GcpNetworkConnectivitySpoke("projects/p/locations/global/spokes/net-d".into()),
        Node::GcpNetworkConnectivityHub("projects/p/locations/global/hubs/going".into()),
    ] {
        assert!(!builder.contains(&absent), "{absent:?} is not projected");
    }
}
