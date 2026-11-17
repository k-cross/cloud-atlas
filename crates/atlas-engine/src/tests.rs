#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use crate::Settings;
    use crate::fixtures;
    use crate::fixtures::{azure_id, azure_network, azure_networking_id, gcp_network};
    use crate::projector;
    use atlas_graph::definition::{Edge, Node};
    use atlas_graph::graph_builder::GraphBuilder;
    use atlas_graph::testing::{all_sources, assert_edge, assert_resolves, link, public};
    use petgraph::visit::EdgeRef;

    fn record(zone: &str, name: &str, record_type: &str) -> Node {
        Node::AwsRoute53RecordSet {
            zone: zone.into(),
            name: name.into(),
            record_type: record_type.into(),
        }
    }

    fn globex_record(name: &str, record_type: &str) -> Node {
        record(fixtures::GLOBEX_ZONE, name, record_type)
    }

    #[test]
    fn every_node_kind_appears_in_fixture_graph() {
        let builder = fixtures::build_graph();
        let present: std::collections::HashSet<&str> =
            builder.graph.node_weights().map(|n| n.kind()).collect();

        let missing: Vec<&&str> = Node::ALL_KINDS
            .iter()
            .filter(|k| !present.contains(**k))
            .collect();
        assert!(
            missing.is_empty(),
            "Node kinds never projected from fixtures: {:?}. \
             Add fixture data (src/fixtures.rs) and a projector mapping for them.",
            missing
        );
    }

    // carry_forward scopes an edge by its owned endpoints, so an edge between
    // two ownerless pivots belongs to every source at once and survives any
    // provider's outage, however stale it is.
    #[test]
    fn every_projected_edge_has_an_owned_endpoint() {
        let graph = fixtures::topology().graph;
        let unowned: Vec<String> = graph
            .edge_references()
            .filter(|e| e.weight().is_projected())
            .filter(|e| graph[e.source()].owner().is_none() && graph[e.target()].owner().is_none())
            .map(|e| {
                format!(
                    "{} -{}-> {}",
                    graph[e.source()],
                    e.weight(),
                    graph[e.target()]
                )
            })
            .collect();
        assert!(
            unowned.is_empty(),
            "projected edges no provider owns: {unowned:?}"
        );
    }

    #[test]
    fn every_edge_kind_appears_in_fixture_graph() {
        let builder = fixtures::build_graph();
        let present: std::collections::HashSet<&str> =
            builder.graph.edge_weights().map(|e| e.kind()).collect();

        let missing: Vec<&&str> = Edge::ALL_KINDS
            .iter()
            .filter(|k| !present.contains(**k))
            .collect();
        assert!(
            missing.is_empty(),
            "Edge kinds never projected from fixtures: {:?}",
            missing
        );
    }

    #[test]
    fn multi_cloud_seams_merge() {
        let builder = fixtures::build_graph();

        let azure_hostname = Node::GenericHostname("app-globex.azurewebsites.net".into());
        let app = Node::AzureAppService(azure_id("Microsoft.Web/sites/app-globex").as_str().into());
        assert_edge(&builder, &azure_hostname, &app, &Edge::RoutesTo);

        for record in [
            Node::CloudflareDnsRecord("rec-app".into()),
            globex_record("app.globex.io.", "CNAME"),
        ] {
            assert_resolves(&builder, "app.globex.io", &record, &azure_hostname);
        }

        let public_ip = Node::GenericIpAddress("198.51.100.10".into());
        assert_resolves(
            &builder,
            "db.globex.io",
            &Node::CloudflareDnsRecord("rec-db".into()),
            &public_ip,
        );
        assert_edge(
            &builder,
            &Node::AzurePublicIpAddress(azure_networking_id(
                "Microsoft.Network/publicIPAddresses/pip-globex",
            )),
            &public_ip,
            &Edge::ConnectsTo,
        );

        assert_resolves(
            &builder,
            "data.globex.io",
            &Node::CloudflareDnsRecord("rec-data".into()),
            &Node::hostname("run.globex.app"),
        );
    }

    #[test]
    fn the_globex_vpn_story_is_projected_on_all_three_clouds() {
        let builder = fixtures::build_graph();
        let azure_address = public("198.51.100.60");
        let gcp_address = public("198.51.100.70");
        let connection = Node::AwsEc2VpnConnection("vpn-globex-azure".into());
        let vgw = Node::AwsEc2VpnGateway("vgw-globex".into());
        let cgw = Node::AwsEc2CustomerGateway("cgw-globex-azure".into());
        assert_edge(
            &builder,
            &vgw,
            &Node::AwsEc2Vpc(fixtures::AWS_NETWORK.into()),
            &Edge::AttachedTo,
        );
        assert_edge(&builder, &connection, &vgw, &Edge::AttachedTo);
        assert_edge(&builder, &connection, &cgw, &Edge::AttachedTo);
        assert_edge(&builder, &cgw, &azure_address, &Edge::RoutesTo);
        assert_edge(
            &builder,
            &connection,
            &public("192.0.2.101"),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &connection,
            &public("192.0.2.102"),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &Node::AwsEc2VpnConnection("vpn-globex-gcp".into()),
            &Node::AwsEc2TransitGateway("tgw-globex".into()),
            &Edge::AttachedTo,
        );

        let vng = Node::AzureVirtualNetworkGateway(azure_networking_id(
            "Microsoft.Network/virtualNetworkGateways/vng-globex-hub",
        ));
        let hub = Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-globex-hub",
        ));
        let spoke = Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-globex",
        ));
        let lng = Node::AzureLocalNetworkGateway(azure_networking_id(
            "Microsoft.Network/localNetworkGateways/lng-globex-aws",
        ));
        assert_edge(&builder, &vng, &hub, &Edge::AttachedTo);
        assert_edge(
            &builder,
            &vng,
            &Node::AzurePublicIpAddress(azure_networking_id(
                "Microsoft.Network/publicIPAddresses/pip-globex-vpn",
            )),
            &Edge::HasIp,
        );
        assert_edge(&builder, &lng, &public("192.0.2.101"), &Edge::RoutesTo);
        assert_edge(&builder, &spoke, &hub, &Edge::RoutesTo);
        assert!(
            !builder.has_edge(&hub, &spoke, &Edge::RoutesTo),
            "only the side that uses the remote gateway routes through it"
        );

        let gcp_gateway = Node::GcpComputeVpnGateway(
            "projects/globex-prod/regions/us-central1/vpnGateways/ha-gw-globex".into(),
        );
        assert_edge(
            &builder,
            &gcp_gateway,
            &gcp_network_node(),
            &Edge::AttachedTo,
        );
        assert_edge(&builder, &gcp_gateway, &gcp_address, &Edge::ConnectsTo);
        let external = Node::GcpComputeExternalVpnGateway(
            "projects/globex-prod/global/externalVpnGateways/aws-peer".into(),
        );
        assert_edge(&builder, &external, &public("192.0.2.103"), &Edge::RoutesTo);
    }

    fn gcp_network_node() -> Node {
        Node::GcpComputeNetwork(gcp_network().into())
    }

    #[test]
    fn the_fixture_circuits_meet_on_their_link_addresses() {
        let builder = fixtures::build_graph();

        let interface = Node::AwsDirectConnectVirtualInterface("dxvif-globex-gcp".into());
        let dx_gateway = Node::AwsDirectConnectGateway("dxgw-globex".into());
        assert_edge(
            &builder,
            &Node::AwsRegion("global".into()),
            &dx_gateway,
            &Edge::Contains,
        );
        assert_edge(
            &builder,
            &Node::AwsEc2VpnGateway("vgw-globex-data".into()),
            &dx_gateway,
            &Edge::AttachedTo,
        );
        assert_edge(&builder, &interface, &dx_gateway, &Edge::AttachedTo);
        assert_edge(
            &builder,
            &interface,
            &Node::AwsDirectConnectConnection("dxcon-globex-gcp".into()),
            &Edge::AttachedTo,
        );
        assert_edge(
            &builder,
            &interface,
            &link("169.254.60.1/30"),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &interface,
            &link("169.254.60.2/30"),
            &Edge::RoutesTo,
        );

        let router = Node::GcpComputeRouter(
            "projects/globex-prod/regions/us-east4/routers/rtr-globex-interconnect".into(),
        );
        let to_aws = Node::GcpComputeInterconnectAttachment(
            "projects/globex-prod/regions/us-east4/interconnectAttachments/att-globex-aws".into(),
        );
        assert_edge(&builder, &router, &gcp_network_node(), &Edge::AttachedTo);
        assert_edge(&builder, &to_aws, &router, &Edge::AttachedTo);
        assert_edge(
            &builder,
            &to_aws,
            &Node::GcpComputeInterconnect(
                "projects/globex-prod/global/interconnects/cci-globex-aws".into(),
            ),
            &Edge::AttachedTo,
        );
        assert_edge(
            &builder,
            &to_aws,
            &link("169.254.60.2/30"),
            &Edge::ConnectsTo,
        );
        assert_edge(&builder, &to_aws, &link("169.254.60.1/30"), &Edge::RoutesTo);

        let circuit = Node::AzureExpressRouteCircuit(azure_networking_id(
            "Microsoft.Network/expressRouteCircuits/er-globex-gcp",
        ));
        let expressroute_gateway = Node::AzureVirtualNetworkGateway(azure_networking_id(
            "Microsoft.Network/virtualNetworkGateways/ergw-globex-hub",
        ));
        let connection = Node::AzureGatewayConnection(azure_networking_id(
            "Microsoft.Network/connections/conn-globex-gcp",
        ));
        assert_edge(
            &builder,
            &expressroute_gateway,
            &Node::AzureVirtualNetwork(azure_networking_id(
                "Microsoft.Network/virtualNetworks/vnet-globex-hub",
            )),
            &Edge::AttachedTo,
        );
        assert_edge(
            &builder,
            &connection,
            &expressroute_gateway,
            &Edge::AttachedTo,
        );
        assert_edge(&builder, &connection, &circuit, &Edge::AttachedTo);
        assert_edge(
            &builder,
            &circuit,
            &link("169.254.70.2/30"),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &circuit,
            &link("169.254.70.1/30"),
            &Edge::RoutesTo,
        );
    }

    #[test]
    fn a_private_address_is_one_node_per_network_it_is_read_in() {
        let builder = fixtures::build_graph();
        let nsg = Node::AzureNetworkSecurityGroup(azure_networking_id(
            "Microsoft.Network/networkSecurityGroups/nsg-worker",
        ));
        let sql = Node::GcpSqlInstance("globex-analytics-db".into());

        let in_azure = Node::ip("10.20.0.5", &azure_network());
        let in_gcp = Node::ip("10.20.0.5", &gcp_network());
        let in_aws = Node::ip("10.20.0.5", fixtures::AWS_NETWORK);
        assert_edge(&builder, &nsg, &in_azure, &Edge::RoutesTo);
        assert_edge(&builder, &sql, &in_gcp, &Edge::ConnectsTo);
        assert_edge(
            &builder,
            &Node::ip("10.10.1.11", fixtures::AWS_NETWORK),
            &in_aws,
            &Edge::TrafficFlow,
        );

        let spellings: std::collections::HashSet<&Node> =
            [&in_azure, &in_gcp, &in_aws].into_iter().collect();
        assert_eq!(
            spellings.len(),
            3,
            "one value, three networks, three addresses"
        );
        assert!(
            !builder.has_edge(&sql, &in_aws, &Edge::ConnectsTo),
            "the database holds its address in its own network, not in the VPC that sent to it"
        );
    }

    #[test]
    fn every_interface_carries_its_address_not_just_the_instance_attached_ones() {
        let builder = fixtures::build_graph();

        for (eni, address) in [
            ("eni-globex-web-01a", "10.10.1.10"),
            ("eni-globex-web-01b", "10.10.2.20"),
            ("eni-globex-web-02a", "10.10.1.11"),
            ("eni-globex-alb-1a", "10.10.1.50"),
            ("eni-globex-nat-1a", "10.10.1.60"),
            ("eni-globex-rds-1a", "10.10.2.30"),
        ] {
            assert_edge(
                &builder,
                &Node::AwsEc2Eni(eni.into()),
                &Node::ip(address, fixtures::AWS_NETWORK),
                &Edge::ConnectsTo,
            );
        }
    }

    #[test]
    fn an_instance_with_interfaces_puts_the_address_on_the_interface() {
        let builder = fixtures::build_graph();
        let instance = Node::AwsEc2Instance("i-globex-web-01".into());

        let address = Node::ip("10.10.1.10", fixtures::AWS_NETWORK);
        assert_edge(
            &builder,
            &Node::AwsEc2Eni("eni-globex-web-01a".into()),
            &address,
            &Edge::ConnectsTo,
        );
        assert!(
            !builder.has_edge(&instance, &address, &Edge::ConnectsTo),
            "the instance-level address is a fallback, not a duplicate assertion"
        );
    }

    #[test]
    fn a_database_is_reached_through_its_endpoint_hostname() {
        let builder = fixtures::build_graph();
        let endpoint =
            Node::hostname("globex-orders-db.cluster-abc123.us-east-1.rds.amazonaws.com");

        assert_edge(
            &builder,
            &Node::AwsRdsDbInstance("globex-orders-db".into()),
            &endpoint,
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &globex_record("db.globex.io.", "CNAME"),
            &endpoint,
            &Edge::ResolvesTo,
        );
    }

    #[test]
    fn every_network_projects_its_own_address_space() {
        use atlas_aws::definition::{AWSConnectivity, AmazonCollection};
        use aws_sdk_ec2::types::{
            Vpc, VpcCidrBlockAssociation, VpcCidrBlockState, VpcCidrBlockStateCode,
            VpcIpv6CidrBlockAssociation,
        };

        let fixture = fixtures::build_graph();
        assert_edge(
            &fixture,
            &Node::AwsEc2Vpc(fixtures::AWS_NETWORK.into()),
            &Node::ip("10.10.0.0/16", fixtures::AWS_NETWORK),
            &Edge::ConnectsTo,
        );
        let spoke = azure_networking_id("Microsoft.Network/virtualNetworks/vnet-globex");
        assert_edge(
            &fixture,
            &Node::AzureVirtualNetwork(spoke.clone()),
            &Node::ip("10.20.1.0/24", &spoke),
            &Edge::ConnectsTo,
        );
        let network = format!(
            "projects/{}/global/networks/vpc-globex",
            fixtures::GCP_PROJECT
        );
        assert_edge(
            &fixture,
            &Node::GcpComputeNetwork(network.as_str().into()),
            &Node::ip("10.30.0.0/24", &network),
            &Edge::ConnectsTo,
        );

        let association = |cidr: &str, state: VpcCidrBlockStateCode| {
            VpcCidrBlockAssociation::builder()
                .cidr_block(cidr)
                .cidr_block_state(VpcCidrBlockState::builder().state(state).build())
                .build()
        };
        let builder = project_aws(vec![AmazonCollection::AmazonConnectivity(
            AWSConnectivity {
                vpcs: vec![
                    Vpc::builder()
                        .vpc_id("vpc-a")
                        .cidr_block("10.1.0.0/16")
                        .cidr_block_association_set(association(
                            "10.1.0.0/16",
                            VpcCidrBlockStateCode::Associated,
                        ))
                        .cidr_block_association_set(association(
                            "10.2.0.0/16",
                            VpcCidrBlockStateCode::Associated,
                        ))
                        .cidr_block_association_set(association(
                            "10.3.0.0/16",
                            VpcCidrBlockStateCode::Disassociating,
                        ))
                        .ipv6_cidr_block_association_set(
                            VpcIpv6CidrBlockAssociation::builder()
                                .ipv6_cidr_block("2600:1f18:aaaa::/56")
                                .ipv6_cidr_block_state(
                                    VpcCidrBlockState::builder()
                                        .state(VpcCidrBlockStateCode::Associated)
                                        .build(),
                                )
                                .build(),
                        )
                        .build(),
                ],
                ..Default::default()
            },
        )]);
        let vpc = Node::AwsEc2Vpc("vpc-a".into());
        for range in ["10.1.0.0/16", "10.2.0.0/16", "2600:1f18:aaaa::/56"] {
            assert_edge(&builder, &vpc, &Node::ip(range, "vpc-a"), &Edge::ConnectsTo);
        }
        assert!(
            !builder.contains(&Node::ip("10.3.0.0/16", "vpc-a")),
            "a range being released is no longer the VPC's"
        );
    }

    #[test]
    fn the_fixture_transit_gateways_route_and_peer() {
        let builder = fixtures::build_graph();
        let east = Node::AwsEc2TransitGateway("tgw-globex".into());
        let west = Node::AwsEc2TransitGateway("tgw-globex-west".into());
        let table = Node::AwsEc2TransitGatewayRouteTable("tgw-rtb-globex".into());
        let peering =
            Node::AwsEc2TransitGatewayPeeringAttachment("tgw-attach-globex-peer-west".into());
        assert_edge(&builder, &east, &table, &Edge::Contains);
        assert_edge(
            &builder,
            &Node::AwsEc2Vpc(fixtures::AWS_NETWORK.into()),
            &table,
            &Edge::AttachedTo,
        );
        assert_edge(
            &builder,
            &table,
            &Node::AwsEc2VpnConnection("vpn-globex-gcp".into()),
            &Edge::RoutesTo,
        );
        let to_west = Node::AwsEc2TransitGatewayRoute {
            table: "tgw-rtb-globex".into(),
            destination: "10.70.0.0/16".into(),
        };
        assert_edge(&builder, &table, &to_west, &Edge::Contains);
        assert_edge(&builder, &to_west, &peering, &Edge::RoutesTo);
        assert_edge(&builder, &peering, &east, &Edge::AttachedTo);
        assert_edge(&builder, &peering, &west, &Edge::AttachedTo);
        assert_edge(
            &builder,
            &peering,
            &Node::AwsEc2TransitGatewayRouteTable("tgw-rtb-globex-west".into()),
            &Edge::AttachedTo,
        );
    }

    fn project_aws(collections: Vec<atlas_aws::definition::AmazonCollection>) -> GraphBuilder {
        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &crate::projector::Provider::AWS(
                collections
                    .into_iter()
                    .map(|c| ("us-east-1".to_owned(), c))
                    .collect(),
            ),
            &Settings::default(),
        );
        builder
    }

    #[test]
    fn an_interface_names_the_owner_its_description_actually_identifies() {
        let builder = fixtures::build_graph();

        assert_edge(
            &builder,
            &Node::AwsElbLoadBalancer(
                "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/globex/1".into(),
            ),
            &Node::AwsEc2Eni("eni-globex-alb-1a".into()),
            &Edge::HasIp,
        );
        assert_edge(
            &builder,
            &Node::AwsEc2NatGateway("nat-globex".into()),
            &Node::AwsEc2Eni("eni-globex-nat-1a".into()),
            &Edge::HasIp,
        );
        assert_edge(
            &builder,
            &Node::AwsEc2Instance("i-globex-web-01".into()),
            &Node::AwsEc2Eni("eni-globex-web-01b".into()),
            &Edge::HasIp,
        );

        let rds = builder
            .index_of(&Node::AwsEc2Eni("eni-globex-rds-1a".into()))
            .expect("the RDS interface is still a node with its address");
        assert!(
            !builder
                .graph
                .neighbors_directed(rds, petgraph::Direction::Incoming)
                .any(|n| matches!(builder.graph[n], Node::AwsRdsDbInstance(_))),
            "an RDSNetworkInterface does not name its database; guessing one would be invention"
        );
    }

    #[test]
    fn observed_traffic_reaches_a_load_balancers_targets() {
        let builder = fixtures::build_graph();
        let alb = Node::AwsElbLoadBalancer(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/globex/1".into(),
        );

        for (eni, instance, address) in [
            ("eni-globex-web-01a", "i-globex-web-01", "10.10.1.10"),
            ("eni-globex-web-02a", "i-globex-web-02", "10.10.1.11"),
        ] {
            let target_ip = Node::ip(address, fixtures::AWS_NETWORK);
            assert_edge(
                &builder,
                &Node::AwsEc2Eni(eni.into()),
                &target_ip,
                &Edge::ConnectsTo,
            );

            assert_edge(
                &builder,
                &alb,
                &Node::AwsEc2Eni("eni-globex-alb-1a".into()),
                &Edge::HasIp,
            );
            assert_edge(
                &builder,
                &Node::ip("10.10.1.50", fixtures::AWS_NETWORK),
                &target_ip,
                &Edge::TrafficFlow,
            );
            assert_edge(
                &builder,
                &Node::AwsEc2Instance(instance.into()),
                &Node::AwsEc2Eni(eni.into()),
                &Edge::HasIp,
            );
        }
    }

    #[test]
    fn render_snapshot_covers_whole_fixture_graph() {
        let builder = fixtures::build_graph();
        let snapshot = atlas_graph::export::render_snapshot(&builder.graph);

        assert_eq!(snapshot.version, atlas_graph::export::SNAPSHOT_VERSION);
        assert_eq!(snapshot.nodes.len(), builder.graph.node_count());
        assert_eq!(snapshot.edges.len(), builder.graph.edge_count());

        let ids: std::collections::HashSet<u32> = snapshot.nodes.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), snapshot.nodes.len(), "node ids must be unique");
        for edge in &snapshot.edges {
            assert!(ids.contains(&edge.source) && ids.contains(&edge.target));
        }

        let json = atlas_graph::export::snapshot_json(&builder.graph).unwrap();
        assert!(json.contains("\"kind\":\"AwsEc2Eni\""));
        assert!(json.contains("\"version\":3"));

        let keys: std::collections::HashSet<&str> =
            snapshot.nodes.iter().map(|n| n.key.as_str()).collect();
        assert_eq!(keys.len(), snapshot.nodes.len(), "node keys must be unique");
    }

    #[test]
    fn node_key_is_unique_across_fixture_graph() {
        use atlas_graph::export::node_key;
        let builder = fixtures::build_graph();
        let keys: std::collections::HashSet<String> =
            builder.graph.node_weights().map(node_key).collect();
        assert_eq!(
            keys.len(),
            builder.graph.node_count(),
            "node_key must be a unique identity for every node"
        );
    }

    #[test]
    fn diff_of_identical_graphs_is_empty() {
        let builder = fixtures::build_graph();
        let patch = atlas_graph::patch::diff(&builder.graph, &builder.graph);
        assert!(
            patch.is_empty(),
            "diffing a graph against itself is a no-op"
        );
    }

    #[test]
    fn diff_detects_added_and_removed_nodes_and_edges() {
        use atlas_graph::definition::{Edge, Node};
        use atlas_graph::export::{edge_key, node_key};

        let old = fixtures::build_graph();

        let removed = old
            .graph
            .node_weights()
            .find(|n| matches!(n, Node::AwsEc2Eni(_)))
            .expect("fixture has an ENI")
            .clone();
        let removed_key = node_key(&removed);

        let mut new = GraphBuilder::new();
        for node in old.graph.node_weights() {
            if node != &removed {
                new.get_or_add_node(node.clone());
            }
        }
        for e in old.graph.edge_indices() {
            let (a, b) = old.graph.edge_endpoints(e).unwrap();
            let (na, nb) = (&old.graph[a], &old.graph[b]);
            if na != &removed && nb != &removed {
                let ia = new.get_or_add_node(na.clone());
                let ib = new.get_or_add_node(nb.clone());
                new.add_edge(ia, ib, old.graph[e].clone());
            }
        }
        let extra_src = Node::GenericHostname("diff-test.example.com".into());
        let extra_dst = Node::GenericIpAddress("203.0.113.7".into());
        let src_key = node_key(&extra_src);
        let dst_key = node_key(&extra_dst);
        let ia = new.get_or_add_node(extra_src);
        let ib = new.get_or_add_node(extra_dst);
        new.add_edge(ia, ib, Edge::ResolvesTo);

        let patch = atlas_graph::patch::diff(&old.graph, &new.graph);

        assert!(patch.removed_nodes.contains(&removed_key));
        assert!(
            patch.added_nodes.iter().any(|n| n.key == src_key)
                && patch.added_nodes.iter().any(|n| n.key == dst_key),
            "both new nodes appear as additions"
        );
        let new_edge_key = edge_key(&src_key, &dst_key, &Edge::ResolvesTo);
        assert!(patch.added_edges.iter().any(|e| e.key == new_edge_key));

        assert!(!patch.removed_edges.is_empty());
    }

    #[test]
    fn carry_forward_restores_only_what_the_scan_is_missing() {
        use atlas_graph::export::{edge_key, node_key};
        use atlas_graph::patch::{carry_forward, diff};

        let live = fixtures::build_graph().graph;

        let mut trimmed = live.clone();
        trimmed.retain_nodes(|g, i| g[i].kind() != "AwsEc2Instance");
        let missing_nodes = live.node_count() - trimmed.node_count();
        assert!(missing_nodes > 0, "fixture must contain the dropped kind");

        let mut partial = GraphBuilder::new();
        partial.merge(&trimmed);

        let fresh_src = Node::GenericHostname("carry-forward.example.com".into());
        let fresh_dst = Node::GenericIpAddress("203.0.113.99".into());
        let a = partial.get_or_add_node(fresh_src.clone());
        let b = partial.get_or_add_node(fresh_dst.clone());
        partial.add_edge(a, b, Edge::ResolvesTo);

        carry_forward(&mut partial, &live, &all_sources());
        // Derived edges are not a scan's evidence, so carry_forward refuses to
        // hold them; the pipeline recomputes them on the carried graph instead.
        atlas_graph::derive::all(&mut partial, &fixtures::observed());

        assert_eq!(partial.graph.node_count(), live.node_count() + 2);

        let patch = diff(&live, &partial.graph);
        assert!(
            patch.removed_nodes.is_empty() && patch.removed_edges.is_empty(),
            "carrying forward must make the diff purely additive"
        );
        assert_eq!(patch.added_nodes.len(), 2);
        assert!(patch.added_edges.iter().any(|e| e.key
            == edge_key(
                &node_key(&fresh_src),
                &node_key(&fresh_dst),
                &Edge::ResolvesTo
            )));
        // The fresh address falls inside a range the fixture already had, so
        // derivation covers it. Nothing beyond that may appear.
        assert!(patch.added_edges.iter().any(|e| e.key
            == edge_key(
                &node_key(&public("203.0.113.0/24")),
                &node_key(&fresh_dst),
                &Edge::Covers
            )));
        assert_eq!(patch.added_edges.len(), 2);
    }

    #[test]
    fn carry_forward_is_a_no_op_when_the_scan_is_a_superset() {
        use atlas_graph::patch::{carry_forward, diff};

        let mut next = fixtures::build_graph();
        let live = next.graph.clone();
        let before_nodes = next.graph.node_count();
        let before_edges = next.graph.edge_count();

        carry_forward(&mut next, &live, &all_sources());

        assert_eq!(next.graph.node_count(), before_nodes);
        assert_eq!(next.graph.edge_count(), before_edges);
        assert!(diff(&live, &next.graph).is_empty());
    }
}
