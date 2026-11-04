#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use crate::atlas::collection::CollectionSource;
    use crate::atlas::definition::{Edge, Node};
    use crate::atlas::graph_builder::GraphBuilder;
    use crate::atlas::projector;
    use crate::atlas::util::is_large_cidr;
    use crate::fixtures;
    use crate::{Settings, fixtures::azure_id};
    use petgraph::visit::EdgeRef;

    // The projector folds a networking resource's id to one casing (Resource
    // Graph is not consistent about which casing it echoes an id back with,
    // depending on which resource's JSON names it), so a test's expectation
    // has to be built the same way. Leaf resources are never cross-referenced
    // by id, so they keep whatever casing the fixture wrote and need no helper.
    fn azure_networking_id(rest: &str) -> std::sync::Arc<str> {
        azure_id(rest).to_ascii_lowercase().into()
    }

    fn public(address: &str) -> Node {
        Node::public_ip(address).expect("a public address")
    }

    fn gcp_network() -> String {
        format!(
            "projects/{}/global/networks/vpc-globex",
            fixtures::GCP_PROJECT
        )
    }

    fn azure_network() -> std::sync::Arc<str> {
        azure_networking_id("Microsoft.Network/virtualNetworks/vnet-globex")
    }

    fn assert_edge(builder: &GraphBuilder, a: &Node, b: &Node, edge: &Edge) {
        let a_idx = builder
            .node_map
            .get(a)
            .unwrap_or_else(|| panic!("Node not found: {:?}", a));
        let b_idx = builder
            .node_map
            .get(b)
            .unwrap_or_else(|| panic!("Node not found: {:?}", b));

        let found = builder
            .graph
            .edges_connecting(*a_idx, *b_idx)
            .any(|e| e.weight() == edge);
        assert!(
            found,
            "Edge {:?} -> {:?} with weight {:?} not found",
            a, b, edge
        );
    }

    fn assert_has_node(builder: &GraphBuilder, node: &Node) {
        assert!(
            builder.node_map.contains_key(node),
            "Node not found: {:?}",
            node
        );
    }

    fn assert_resolves(builder: &GraphBuilder, name: &str, record: &Node, target: &Node) {
        assert_edge(builder, &Node::hostname(name), record, &Edge::ResolvesTo);
        assert_edge(builder, record, target, &Edge::ResolvesTo);
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
    fn amazon_projection() {
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &fixtures::aws(), &fixtures::settings());

        let instance = Node::AwsEc2Instance("i-globex-web-01".into());
        let eni = Node::AwsEc2Eni("eni-globex-web-01a".into());
        let second_eni = Node::AwsEc2Eni("eni-globex-web-01b".into());
        let subnet = Node::AwsEc2Subnet("subnet-public-1a".into());
        let private_subnet = Node::AwsEc2Subnet("subnet-private-1a".into());
        let vpc = Node::AwsEc2Vpc("vpc-globex".into());
        let az = Node::AwsEc2AvailabilityZone("us-east-1a".into());
        let region = Node::AwsRegion(fixtures::REGION.into());
        let global = Node::AwsRegion("global".into());
        let tag = Node::AwsTag {
            key: "env".into(),
            value: "prod".into(),
        };
        let sg = Node::AwsEc2SecurityGroup("sg-web".into());
        let sg_lb = Node::AwsEc2SecurityGroup("sg-lb".into());
        let lb = Node::AwsElbLoadBalancer(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/globex/1".into(),
        );
        let tg = Node::AwsElbTargetGroup(
            "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-web/1".into(),
        );
        let role = Node::AwsIamRole("arn:aws:iam::123:role/globex-lambda-role".into());
        let lambda = Node::AwsLambdaFunction("globex-events-handler".into());

        assert_edge(&builder, &instance, &eni, &Edge::HasIp);
        assert_edge(&builder, &eni, &subnet, &Edge::AttachedTo);

        assert_edge(&builder, &instance, &second_eni, &Edge::HasIp);
        assert_edge(&builder, &second_eni, &private_subnet, &Edge::AttachedTo);
        assert_edge(&builder, &region, &vpc, &Edge::Contains);
        assert_edge(&builder, &vpc, &subnet, &Edge::Contains);
        assert_edge(&builder, &az, &instance, &Edge::Contains);
        assert_edge(&builder, &instance, &tag, &Edge::DependsOn);
        assert_edge(&builder, &instance, &sg, &Edge::ConnectsTo);

        assert_edge(&builder, &lb, &tg, &Edge::ConnectsTo);
        assert_edge(&builder, &tg, &instance, &Edge::ConnectsTo);

        let nlb = Node::AwsElbLoadBalancer(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/net/globex-db/1".into(),
        );
        let tg_db = Node::AwsElbTargetGroup(
            "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/globex-db/1".into(),
        );
        assert_edge(&builder, &nlb, &tg_db, &Edge::ConnectsTo);
        assert_edge(
            &builder,
            &tg_db,
            &Node::ip("10.10.2.30", fixtures::AWS_NETWORK),
            &Edge::RoutesTo,
        );

        assert_edge(&builder, &lambda, &role, &Edge::DependsOn);
        assert_edge(&builder, &lambda, &sg, &Edge::ConnectsTo);

        assert_edge(&builder, &sg_lb, &sg, &Edge::ConnectsTo);
        assert_edge(
            &builder,
            &sg,
            &Node::GenericIpAddress("192.0.2.44/32".into()),
            &Edge::RoutesTo,
        );
        assert_edge(
            &builder,
            &sg,
            &Node::GenericIpAddress("2001:db8::44/128".into()),
            &Edge::RoutesTo,
        );

        assert_resolves(
            &builder,
            "origin.globex.io",
            &Node::AwsRoute53RecordSet("origin.globex.io.".into()),
            &public("203.0.113.10"),
        );
        assert_resolves(
            &builder,
            "app.globex.io",
            &Node::AwsRoute53RecordSet("app.globex.io.".into()),
            &Node::hostname("app-globex.azurewebsites.net"),
        );

        let s3 = Node::AwsConfigResource {
            resource_type: "AWS::S3::Bucket".into(),
            id: "globex-assets".into(),
        };
        assert_edge(&builder, &global, &s3, &Edge::Contains);

        let public_rt = Node::AwsEc2RouteTable("rtb-public".into());
        let private_rt = Node::AwsEc2RouteTable("rtb-private".into());
        let igw = Node::AwsEc2InternetGateway("igw-globex".into());
        let nat = Node::AwsEc2NatGateway("nat-globex".into());
        let eip = Node::AwsEc2Eip("eipalloc-globex".into());
        let private_subnet = Node::AwsEc2Subnet("subnet-private-1a".into());
        assert_edge(&builder, &vpc, &public_rt, &Edge::Contains);
        assert_edge(&builder, &subnet, &public_rt, &Edge::AttachedTo);
        let default_route = vpc_route("rtb-public", "0.0.0.0/0");
        assert_edge(&builder, &public_rt, &default_route, &Edge::Contains);
        assert_edge(&builder, &default_route, &igw, &Edge::RoutesTo);
        assert_edge(
            &builder,
            &vpc_route("rtb-public", "10.10.0.0/16"),
            &vpc,
            &Edge::RoutesTo,
        );
        assert_edge(&builder, &igw, &vpc, &Edge::AttachedTo);
        assert_edge(&builder, &private_subnet, &private_rt, &Edge::AttachedTo);
        assert_edge(
            &builder,
            &vpc_route("rtb-private", "0.0.0.0/0"),
            &nat,
            &Edge::RoutesTo,
        );
        assert_edge(&builder, &nat, &subnet, &Edge::AttachedTo);
        assert_edge(&builder, &nat, &eip, &Edge::HasIp);
        assert_edge(
            &builder,
            &eip,
            &Node::GenericIpAddress("203.0.113.50".into()),
            &Edge::ConnectsTo,
        );

        assert_has_node(&builder, &Node::AwsEksCluster("globex-k8s".into()));
        assert_has_node(&builder, &Node::AwsApiGatewayRestApi("api-globex".into()));
        assert_has_node(&builder, &Node::AwsRdsDbInstance("globex-orders-db".into()));
        assert_has_node(&builder, &Node::AwsDynamoDbTable("globex-events".into()));
        assert_has_node(
            &builder,
            &Node::AwsSqsQueue("https://sqs.us-east-1.amazonaws.com/123/globex-jobs".into()),
        );
        assert_has_node(
            &builder,
            &Node::AwsSnsTopic("arn:aws:sns:us-east-1:123:globex-alerts".into()),
        );
        assert_has_node(
            &builder,
            &Node::AwsCloudFrontDistribution("EGLOBEX1".into()),
        );
        assert_has_node(
            &builder,
            &Node::AwsEcsCluster("arn:aws:ecs:us-east-1:123:cluster/globex-services".into()),
        );
    }

    #[test]
    fn gcp_projection() {
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &fixtures::gcp(), &fixtures::settings());

        let project = Node::GcpProject(fixtures::GCP_PROJECT.into());
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
                    fixtures::GCP_PROJECT
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
                    fixtures::GCP_PROJECT
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
                    fixtures::GCP_PROJECT
                )
                .as_str()
                .into(),
            ),
            &Node::GcpPubSubTopic(
                format!("projects/{}/topics/globex-events", fixtures::GCP_PROJECT)
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
                    fixtures::GCP_PROJECT
                )
                .as_str()
                .into(),
            ),
        );
    }

    #[test]
    fn azure_projection() {
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &fixtures::azure(), &fixtures::settings());

        let vm = Node::AzureVirtualMachine(azure_networking_id(
            "Microsoft.Compute/virtualMachines/vm-worker",
        ));
        let nic = Node::AzureNetworkInterface(azure_networking_id(
            "Microsoft.Network/networkInterfaces/nic-worker",
        ));
        let vnet = Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-globex",
        ));
        let subnet = Node::AzureSubnet(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-globex/subnets/default",
        ));
        let nsg = Node::AzureNetworkSecurityGroup(azure_networking_id(
            "Microsoft.Network/networkSecurityGroups/nsg-worker",
        ));

        assert_edge(&builder, &vm, &nic, &Edge::HasIp);
        assert_edge(&builder, &vnet, &subnet, &Edge::Contains);

        let hub = Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/vnet-globex-hub",
        ));
        assert_edge(&builder, &vnet, &hub, &Edge::ConnectsTo);
        assert_edge(&builder, &hub, &vnet, &Edge::ConnectsTo);
        assert!(
            !builder.contains(&Node::AzureVirtualNetwork(azure_networking_id(
                "Microsoft.Network/virtualNetworks/vnet-globex-dr",
            ))),
            "a peering only one side has created joins nothing"
        );
        assert_edge(&builder, &subnet, &nsg, &Edge::ConnectsTo);

        assert_edge(
            &builder,
            &nic,
            &Node::ip("10.20.1.4", &azure_network()),
            &Edge::ConnectsTo,
        );
        assert_edge(&builder, &nic, &subnet, &Edge::AttachedTo);

        let balancer = Node::AzureLoadBalancer(azure_networking_id(
            "Microsoft.Network/loadBalancers/lb-globex",
        ));
        let pip = Node::AzurePublicIpAddress(azure_networking_id(
            "Microsoft.Network/publicIPAddresses/pip-globex",
        ));
        assert_edge(&builder, &balancer, &pip, &Edge::HasIp);

        assert_edge(
            &builder,
            &nsg,
            &Node::ip("10.20.0.5", &azure_network()),
            &Edge::RoutesTo,
        );
        assert_edge(
            &builder,
            &nsg,
            &Node::AzureServiceTag("AzureCloud".into()),
            &Edge::RoutesTo,
        );

        assert_edge(
            &builder,
            &pip,
            &Node::GenericIpAddress("198.51.100.10".into()),
            &Edge::ConnectsTo,
        );

        assert_edge(
            &builder,
            &Node::GenericHostname("app-globex.azurewebsites.net".into()),
            &Node::AzureAppService(azure_id("Microsoft.Web/sites/app-globex").as_str().into()),
            &Edge::RoutesTo,
        );

        for node in [
            Node::AzureStorageAccount(
                azure_id("Microsoft.Storage/storageAccounts/globexstore")
                    .as_str()
                    .into(),
            ),
            Node::AzureManagedCluster(
                azure_id("Microsoft.ContainerService/managedClusters/aks-globex")
                    .as_str()
                    .into(),
            ),
            Node::AzureSqlServer(azure_id("Microsoft.Sql/servers/sql-globex").as_str().into()),
            Node::AzureFunctionApp(azure_id("Microsoft.Web/sites/func-globex").as_str().into()),
            Node::AzureApiManagement(
                azure_id("Microsoft.ApiManagement/service/apim-globex")
                    .as_str()
                    .into(),
            ),
            Node::AzureCosmosDb(
                azure_id("Microsoft.DocumentDB/databaseAccounts/cosmos-globex")
                    .as_str()
                    .into(),
            ),
            Node::AzureServiceBus(
                azure_id("Microsoft.ServiceBus/namespaces/sb-globex")
                    .as_str()
                    .into(),
            ),
            Node::AzureEventGridTopic(
                azure_id("Microsoft.EventGrid/topics/eg-globex")
                    .as_str()
                    .into(),
            ),
            Node::AzureDnsZone(
                azure_id("Microsoft.Network/dnsZones/globex.azure")
                    .as_str()
                    .into(),
            ),
            Node::AzureCdnProfile(
                azure_id("Microsoft.Cdn/profiles/cdn-globex")
                    .as_str()
                    .into(),
            ),
        ] {
            assert_has_node(&builder, &node);
        }
    }

    #[test]
    fn cloudflare_projection() {
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &fixtures::cloudflare(), &fixtures::settings());

        let zone = Node::CloudflareZone("zone-globex".into());
        let worker = Node::CloudflareWorker("edge-router".into());

        assert_edge(
            &builder,
            &zone,
            &Node::CloudflareDnsRecord("rec-app".into()),
            &Edge::Contains,
        );
        let record = |id: &str| Node::CloudflareDnsRecord(id.into());
        assert_resolves(
            &builder,
            "app.globex.io",
            &record("rec-app"),
            &Node::hostname("app-globex.azurewebsites.net"),
        );
        assert_resolves(
            &builder,
            "data.globex.io",
            &record("rec-data"),
            &Node::hostname("run.globex.app"),
        );
        assert_resolves(
            &builder,
            "db.globex.io",
            &record("rec-db"),
            &public("198.51.100.10"),
        );
        assert_resolves(
            &builder,
            "v6.globex.io",
            &record("rec-v6"),
            &public("2001:db8::10"),
        );

        assert_edge(
            &builder,
            &worker,
            &Node::CloudflareKvNamespace("kv-sessions".into()),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &worker,
            &Node::CloudflareR2Bucket("r2-media".into()),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &worker,
            &Node::CloudflareDurableObject("do-coordinator".into()),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &worker,
            &Node::CloudflareD1Database("d1-edge-cache".into()),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &worker,
            &Node::ExternalService("postgres://db.internal.globex.io".into()),
            &Edge::ConnectsTo,
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
            Node::AwsRoute53RecordSet("app.globex.io.".into()),
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
    fn aws_connectivity_projection() {
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &fixtures::aws(), &fixtures::settings());

        let region = Node::AwsRegion(fixtures::REGION.into());
        let vpc = |id: &str| Node::AwsEc2Vpc(id.into());
        for id in [
            fixtures::AWS_NETWORK,
            "vpc-globex-data",
            "vpc-globex-shared",
            "vpc-globex-staging",
        ] {
            assert_edge(&builder, &region, &vpc(id), &Edge::Contains);
        }

        let pcx = Node::AwsEc2VpcPeeringConnection("pcx-globex-data".into());
        assert_edge(
            &builder,
            &pcx,
            &vpc(fixtures::AWS_NETWORK),
            &Edge::AttachedTo,
        );
        assert_edge(&builder, &pcx, &vpc("vpc-globex-data"), &Edge::AttachedTo);
        assert!(
            !builder.contains(&Node::AwsEc2VpcPeeringConnection(
                "pcx-globex-staging".into()
            )),
            "a rejected peering request connects nothing"
        );

        let tgw = Node::AwsEc2TransitGateway("tgw-globex".into());
        assert_edge(&builder, &region, &tgw, &Edge::Contains);
        assert_edge(
            &builder,
            &tgw,
            &vpc(fixtures::AWS_NETWORK),
            &Edge::AttachedTo,
        );
        assert_edge(&builder, &tgw, &vpc("vpc-globex-shared"), &Edge::AttachedTo);
        assert!(!builder.has_edge(&tgw, &vpc("vpc-globex-staging"), &Edge::AttachedTo));

        assert_edge(
            &builder,
            &vpc_route("rtb-private", "10.40.0.0/16"),
            &pcx,
            &Edge::RoutesTo,
        );
        assert_edge(
            &builder,
            &vpc_route("rtb-private", "10.50.0.0/16"),
            &tgw,
            &Edge::RoutesTo,
        );
        assert_edge(
            &builder,
            &vpc_route("rtb-private", "10.20.1.0/24"),
            &Node::AwsEc2VpnGateway("vgw-globex".into()),
            &Edge::RoutesTo,
        );
        let retired = vpc_route("rtb-data", "10.60.0.0/16");
        assert_edge(
            &builder,
            &Node::AwsEc2RouteTable("rtb-data".into()),
            &retired,
            &Edge::Contains,
        );
        assert!(
            builder
                .graph
                .edges_directed(
                    builder.index_of(&retired).unwrap(),
                    petgraph::Direction::Outgoing
                )
                .next()
                .is_none(),
            "a blackhole route goes nowhere"
        );
    }

    #[test]
    fn only_live_routes_and_attachments_join_networks() {
        use crate::cloud::definition::{AWSConnectivity, AWSNetworking, AmazonCollection};
        use aws_sdk_ec2::types::{
            Route, RouteState, RouteTable, TransitGateway, TransitGatewayAttachment,
            TransitGatewayAttachmentResourceType, TransitGatewayAttachmentState,
            TransitGatewayState,
        };

        let builder = project_aws(vec![
            AmazonCollection::AmazonNetworking(AWSNetworking {
                route_tables: vec![
                    RouteTable::builder()
                        .route_table_id("rtb-1")
                        .vpc_id("vpc-a")
                        .routes(
                            Route::builder()
                                .destination_cidr_block("10.9.0.0/16")
                                .vpc_peering_connection_id("pcx-gone")
                                .state(RouteState::Blackhole)
                                .build(),
                        )
                        .routes(
                            Route::builder()
                                .destination_cidr_block("0.0.0.0/0")
                                .nat_gateway_id("nat-gone")
                                .state(RouteState::Blackhole)
                                .build(),
                        )
                        .build(),
                ],
                internet_gateways: Vec::new(),
                nat_gateways: Vec::new(),
                addresses: Vec::new(),
            }),
            AmazonCollection::AmazonConnectivity(AWSConnectivity {
                transit_gateways: vec![
                    TransitGateway::builder()
                        .transit_gateway_id("tgw-new")
                        .state(TransitGatewayState::Pending)
                        .build(),
                ],
                transit_gateway_attachments: vec![
                    TransitGatewayAttachment::builder()
                        .transit_gateway_id("tgw-1")
                        .resource_type(TransitGatewayAttachmentResourceType::Vpc)
                        .resource_id("vpc-a")
                        .state(TransitGatewayAttachmentState::PendingAcceptance)
                        .build(),
                    TransitGatewayAttachment::builder()
                        .transit_gateway_id("tgw-1")
                        .resource_type(TransitGatewayAttachmentResourceType::Vpn)
                        .resource_id("vpn-1")
                        .state(TransitGatewayAttachmentState::Available)
                        .build(),
                ],
                ..Default::default()
            }),
        ]);

        assert!(
            !builder.contains(&Node::AwsEc2VpcPeeringConnection("pcx-gone".into())),
            "a blackhole route names a peering that no longer exists"
        );
        assert!(
            !builder.contains(&Node::AwsEc2NatGateway("nat-gone".into())),
            "a blackhole route draws nothing, whatever its target"
        );
        assert_has_node(&builder, &vpc_route("rtb-1", "10.9.0.0/16"));
        assert!(!builder.contains(&Node::AwsEc2TransitGateway("tgw-new".into())));
        assert!(
            !builder
                .graph
                .node_weights()
                .any(|n| matches!(n, Node::AwsEc2TransitGateway(_))),
            "neither a pending attachment nor a VPN attachment draws the gateway"
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
    fn only_a_working_vpn_is_projected() {
        use crate::cloud::definition::{AWSConnectivity, AmazonCollection};
        use aws_sdk_ec2::types::{
            AttachmentStatus, CustomerGateway, VpcAttachment, VpnConnection, VpnGateway, VpnState,
        };

        let builder = project_aws(vec![AmazonCollection::AmazonConnectivity(
            AWSConnectivity {
                vpn_gateways: vec![
                    VpnGateway::builder()
                        .vpn_gateway_id("vgw-detached")
                        .state(VpnState::Available)
                        .vpc_attachments(
                            VpcAttachment::builder()
                                .vpc_id("vpc-old")
                                .state(AttachmentStatus::Detached)
                                .build(),
                        )
                        .build(),
                    VpnGateway::builder()
                        .vpn_gateway_id("vgw-gone")
                        .state(VpnState::Deleted)
                        .build(),
                ],
                customer_gateways: vec![
                    CustomerGateway::builder()
                        .customer_gateway_id("cgw-gone")
                        .ip_address("198.51.100.9")
                        .state("deleted")
                        .build(),
                ],
                vpn_connections: vec![
                    VpnConnection::builder()
                        .vpn_connection_id("vpn-pending")
                        .state(VpnState::Pending)
                        .customer_gateway_id("cgw-gone")
                        .build(),
                ],
                ..Default::default()
            },
        )]);

        assert_has_node(&builder, &Node::AwsEc2VpnGateway("vgw-detached".into()));
        assert!(
            !builder.contains(&Node::AwsEc2Vpc("vpc-old".into())),
            "a detached gateway is not attached to the VPC"
        );
        for gone in [
            Node::AwsEc2VpnGateway("vgw-gone".into()),
            Node::AwsEc2CustomerGateway("cgw-gone".into()),
            Node::AwsEc2VpnConnection("vpn-pending".into()),
        ] {
            assert!(!builder.contains(&gone), "{gone:?} is not available");
        }
    }

    #[test]
    fn an_unconnected_peering_does_not_ride_the_hub_gateway() {
        use crate::api::azure::models::{VirtualNetwork, VirtualNetworkPeering};
        use crate::cloud::definition::{MicrosoftCollection, Provider};

        let remote = azure_networking_id("Microsoft.Network/virtualNetworks/hub");
        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &Provider::Azure(vec![MicrosoftCollection::AzureVirtualNetworks(vec![
                VirtualNetwork {
                    id: Some(
                        azure_networking_id("Microsoft.Network/virtualNetworks/spoke").to_string(),
                    ),
                    peerings: vec![VirtualNetworkPeering {
                        remote_id: Some(remote.to_string()),
                        state: Some("Initiated".to_owned()),
                        use_remote_gateways: true,
                    }],
                    ..Default::default()
                },
            ])]),
            &Settings::default(),
        );

        let spoke = Node::AzureVirtualNetwork(azure_networking_id(
            "Microsoft.Network/virtualNetworks/spoke",
        ));
        assert!(!builder.has_edge(&spoke, &Node::AzureVirtualNetwork(remote), &Edge::RoutesTo));
    }

    #[test]
    fn an_azure_vpn_being_deleted_or_failed_is_not_projected() {
        use crate::api::azure::models::{
            GatewayConnection, LocalNetworkGateway, VirtualNetworkGateway,
        };
        use crate::cloud::definition::{MicrosoftCollection, Provider};

        let id = |rest: &str| Some(azure_networking_id(rest).to_string());
        let gateway = |name: &str, state: &str| VirtualNetworkGateway {
            id: id(&format!("Microsoft.Network/virtualNetworkGateways/{name}")),
            provisioning_state: Some(state.to_owned()),
            subnet_ids: vec![
                azure_networking_id(&format!(
                    "Microsoft.Network/virtualNetworks/{name}/subnets/GatewaySubnet"
                ))
                .to_string(),
            ],
            ..Default::default()
        };
        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &Provider::Azure(vec![
                MicrosoftCollection::AzureVirtualNetworkGateways(vec![
                    gateway("updating", "Updating"),
                    gateway("deleting", "Deleting"),
                    gateway("failed", "failed"),
                ]),
                MicrosoftCollection::AzureLocalNetworkGateways(vec![LocalNetworkGateway {
                    id: id("Microsoft.Network/localNetworkGateways/deleting"),
                    gateway_ip_address: Some("192.0.2.50".to_owned()),
                    provisioning_state: Some("Deleting".to_owned()),
                    ..Default::default()
                }]),
                MicrosoftCollection::AzureGatewayConnections(vec![
                    GatewayConnection {
                        id: id("Microsoft.Network/connections/failed"),
                        provisioning_state: Some("Failed".to_owned()),
                        gateway_id: id("Microsoft.Network/virtualNetworkGateways/updating"),
                        ..Default::default()
                    },
                    GatewayConnection {
                        id: id("Microsoft.Network/connections/to-deleting"),
                        provisioning_state: Some("Succeeded".to_owned()),
                        gateway_id: id("Microsoft.Network/virtualNetworkGateways/deleting"),
                        local_gateway_id: id("Microsoft.Network/localNetworkGateways/deleting"),
                        ..Default::default()
                    },
                ]),
            ]),
            &Settings::default(),
        );

        assert_edge(
            &builder,
            &Node::AzureVirtualNetworkGateway(azure_networking_id(
                "Microsoft.Network/virtualNetworkGateways/updating",
            )),
            &Node::AzureVirtualNetwork(azure_networking_id(
                "Microsoft.Network/virtualNetworks/updating",
            )),
            &Edge::AttachedTo,
        );
        assert_has_node(
            &builder,
            &Node::AzureGatewayConnection(azure_networking_id(
                "Microsoft.Network/connections/to-deleting",
            )),
        );
        for gone in [
            Node::AzureVirtualNetworkGateway(azure_networking_id(
                "Microsoft.Network/virtualNetworkGateways/deleting",
            )),
            Node::AzureVirtualNetworkGateway(azure_networking_id(
                "Microsoft.Network/virtualNetworkGateways/failed",
            )),
            Node::AzureLocalNetworkGateway(azure_networking_id(
                "Microsoft.Network/localNetworkGateways/deleting",
            )),
            Node::AzureGatewayConnection(azure_networking_id(
                "Microsoft.Network/connections/failed",
            )),
        ] {
            assert!(!builder.contains(&gone), "{gone:?} is going away");
        }
        assert!(!builder.contains(&public("192.0.2.50")));
    }

    #[test]
    fn a_gcp_tunnel_is_projected_unless_it_is_being_built_or_torn_down() {
        use crate::api::google::vpn::VpnTunnel;
        use crate::cloud::definition::{GoogleCollection, Provider};

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
        projector::build(
            &mut builder,
            &Provider::GCP(vec![(
                "p".to_owned(),
                GoogleCollection::GoogleVpnTunnels(
                    statuses
                        .iter()
                        .map(|(name, status)| tunnel(name, status))
                        .collect(),
                ),
            )]),
            &Settings::default(),
        );

        let gateway = Node::GcpComputeVpnGateway("projects/p/regions/r/vpnGateways/gw".into());
        for (name, _) in &statuses[..3] {
            assert_edge(
                &builder,
                &Node::GcpComputeVpnTunnel(
                    format!("projects/p/regions/r/vpnTunnels/{name}").into(),
                ),
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

    fn link(address: &str) -> Node {
        Node::link_ip(address).expect("an address with its link prefix")
    }

    #[test]
    fn a_link_address_is_scoped_to_the_subnet_both_ends_share() {
        let link_scope = |address: &str| match link(address) {
            Node::GenericPrivateIpAddress { network, address } => {
                (network.to_string(), address.to_string())
            }
            other => panic!("{other:?} is not a private link address"),
        };

        assert_eq!(
            link_scope("169.254.60.1/30"),
            ("169.254.60.0/30".to_owned(), "169.254.60.1".to_owned())
        );
        assert_eq!(link_scope("169.254.60.2/30").0, "169.254.60.0/30");
        assert_eq!(
            link("203.0.113.1/31"),
            public("203.0.113.1"),
            "a public link address is the internet's own pivot"
        );
        assert_eq!(
            Node::link_ip("169.254.60.1"),
            None,
            "without its prefix the link it sits on is unknown"
        );
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
    fn only_a_direct_connect_circuit_that_reaches_a_network_is_projected() {
        use crate::cloud::definition::{
            AWSDirectConnect, AWSDirectConnectGateways, AmazonCollection,
        };
        use aws_sdk_directconnect::types::{
            AssociatedGateway, BgpPeer, BgpPeerState, Connection, ConnectionState,
            DirectConnectGateway, DirectConnectGatewayAssociation,
            DirectConnectGatewayAssociationState, DirectConnectGatewayState, GatewayType,
            VirtualInterface, VirtualInterfaceState,
        };

        let interface = |id: &str, kind: &str, state: VirtualInterfaceState| {
            VirtualInterface::builder()
                .virtual_interface_id(id)
                .virtual_interface_type(kind)
                .virtual_interface_state(state)
        };
        let peer = |amazon: &str, customer: &str, state: BgpPeerState| {
            BgpPeer::builder()
                .amazon_address(amazon)
                .customer_address(customer)
                .bgp_peer_state(state)
                .build()
        };
        let association = |associated: AssociatedGateway,
                           state: DirectConnectGatewayAssociationState| {
            DirectConnectGatewayAssociation::builder()
                .direct_connect_gateway_id("dxgw-a")
                .associated_gateway(associated)
                .association_state(state)
                .build()
        };
        let builder = project_aws(vec![
            AmazonCollection::AmazonDirectConnect(AWSDirectConnect {
                connections: vec![
                    Connection::builder()
                        .connection_id("dxcon-a")
                        .connection_state(ConnectionState::Down)
                        .build(),
                    Connection::builder()
                        .connection_id("dxcon-ordering")
                        .connection_state(ConnectionState::Ordering)
                        .build(),
                ],
                virtual_interfaces: vec![
                    interface("vif-private", "private", VirtualInterfaceState::Available)
                        .connection_id("dxcon-a")
                        .virtual_gateway_id("vgw-a")
                        .direct_connect_gateway_id("")
                        .amazon_address("169.254.1.1/30")
                        .customer_address("169.254.1.2/30")
                        .bgp_peers(peer(
                            "169.254.1.1/30",
                            "169.254.1.2/30",
                            BgpPeerState::Available,
                        ))
                        .bgp_peers(peer(
                            "169.254.9.1/30",
                            "169.254.9.2/30",
                            BgpPeerState::Deleting,
                        ))
                        .build(),
                    interface("vif-transit", "transit", VirtualInterfaceState::Down)
                        .connection_id("dxlag-a")
                        .virtual_gateway_id("")
                        .direct_connect_gateway_id("dxgw-a")
                        .amazon_address("169.254.2.1/30")
                        .customer_address("169.254.2.2/30")
                        .build(),
                    interface("vif-orphaned", "private", VirtualInterfaceState::Down)
                        .connection_id("dxcon-a")
                        .virtual_gateway_id("")
                        .direct_connect_gateway_id("dxgw-deleting")
                        .build(),
                    interface("vif-public", "public", VirtualInterfaceState::Available)
                        .connection_id("dxcon-a")
                        .build(),
                    interface("vif-pending", "private", VirtualInterfaceState::Pending)
                        .connection_id("dxcon-a")
                        .build(),
                ],
            }),
            AmazonCollection::AmazonDirectConnectGateways(AWSDirectConnectGateways {
                gateways: vec![
                    DirectConnectGateway::builder()
                        .direct_connect_gateway_id("dxgw-a")
                        .direct_connect_gateway_state(DirectConnectGatewayState::Available)
                        .build(),
                    DirectConnectGateway::builder()
                        .direct_connect_gateway_id("dxgw-deleting")
                        .direct_connect_gateway_state(DirectConnectGatewayState::Deleting)
                        .build(),
                ],
                associations: vec![
                    association(
                        AssociatedGateway::builder()
                            .id("tgw-a")
                            .r#type(GatewayType::TransitGateway)
                            .build(),
                        DirectConnectGatewayAssociationState::Updating,
                    ),
                    association(
                        AssociatedGateway::builder()
                            .id("vgw-old")
                            .r#type(GatewayType::VirtualPrivateGateway)
                            .build(),
                        DirectConnectGatewayAssociationState::Disassociated,
                    ),
                    DirectConnectGatewayAssociation::builder()
                        .direct_connect_gateway_id("dxgw-deleting")
                        .associated_gateway(
                            AssociatedGateway::builder()
                                .id("vgw-b")
                                .r#type(GatewayType::VirtualPrivateGateway)
                                .build(),
                        )
                        .association_state(DirectConnectGatewayAssociationState::Associated)
                        .build(),
                ],
            }),
        ]);

        let gateway = Node::AwsDirectConnectGateway("dxgw-a".into());
        assert_edge(
            &builder,
            &Node::AwsEc2TransitGateway("tgw-a".into()),
            &gateway,
            &Edge::AttachedTo,
        );
        let private = Node::AwsDirectConnectVirtualInterface("vif-private".into());
        assert_edge(
            &builder,
            &private,
            &Node::AwsDirectConnectConnection("dxcon-a".into()),
            &Edge::AttachedTo,
        );
        assert_edge(
            &builder,
            &private,
            &Node::AwsEc2VpnGateway("vgw-a".into()),
            &Edge::AttachedTo,
        );
        assert_edge(
            &builder,
            &private,
            &link("169.254.1.1/30"),
            &Edge::ConnectsTo,
        );
        assert_edge(&builder, &private, &link("169.254.1.2/30"), &Edge::RoutesTo);
        let transit = Node::AwsDirectConnectVirtualInterface("vif-transit".into());
        assert_edge(&builder, &transit, &gateway, &Edge::AttachedTo);
        assert_edge(
            &builder,
            &transit,
            &link("169.254.2.1/30"),
            &Edge::ConnectsTo,
        );
        assert_has_node(
            &builder,
            &Node::AwsDirectConnectVirtualInterface("vif-orphaned".into()),
        );
        for absent in [
            Node::AwsDirectConnectConnection("dxcon-ordering".into()),
            Node::AwsDirectConnectConnection("dxlag-a".into()),
            Node::AwsDirectConnectGateway("dxgw-deleting".into()),
            Node::AwsEc2VpnGateway("vgw-old".into()),
            Node::AwsEc2VpnGateway("vgw-b".into()),
            Node::AwsEc2VpnGateway("".into()),
            Node::AwsDirectConnectGateway("".into()),
            Node::AwsDirectConnectVirtualInterface("vif-public".into()),
            Node::AwsDirectConnectVirtualInterface("vif-pending".into()),
            link("169.254.9.1/30"),
        ] {
            assert!(!builder.contains(&absent), "{absent:?} is not projected");
        }
    }

    #[test]
    fn an_expressroute_circuit_holds_the_second_address_of_each_peering_and_names_the_first() {
        use crate::api::azure::models::{
            ExpressRouteCircuit, ExpressRoutePeering, GatewayConnection, VirtualNetworkGateway,
        };
        use crate::cloud::definition::{MicrosoftCollection, Provider};

        let id = |rest: &str| Some(azure_networking_id(rest).to_string());
        let peering = |kind: &str, state: &str, prefixes: &[&str]| ExpressRoutePeering {
            peering_type: Some(kind.to_owned()),
            state: Some(state.to_owned()),
            peer_address_prefixes: prefixes.iter().map(|prefix| prefix.to_string()).collect(),
        };
        let circuit = |name: &str, provisioning: &str, provider: &str| ExpressRouteCircuit {
            id: id(&format!("Microsoft.Network/expressRouteCircuits/{name}")),
            provisioning_state: Some(provisioning.to_owned()),
            service_provider_provisioning_state: Some(provider.to_owned()),
            peerings: vec![
                peering(
                    "AzurePrivatePeering",
                    "Enabled",
                    &["192.168.15.16/30", "192.168.15.20/30"],
                ),
                peering("MicrosoftPeering", "Enabled", &["203.0.113.0/30"]),
                peering("AzurePrivatePeering", "Disabled", &["fd00:15::/126"]),
            ],
            ..Default::default()
        };
        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &Provider::Azure(vec![
                MicrosoftCollection::AzureVirtualNetworkGateways(vec![VirtualNetworkGateway {
                    id: id("Microsoft.Network/virtualNetworkGateways/ergw"),
                    provisioning_state: Some("Succeeded".to_owned()),
                    subnet_ids: vec![
                        azure_networking_id(
                            "Microsoft.Network/virtualNetworks/hub/subnets/GatewaySubnet",
                        )
                        .to_string(),
                    ],
                    ..Default::default()
                }]),
                MicrosoftCollection::AzureGatewayConnections(
                    ["live", "unprovisioned", "deleting"]
                        .into_iter()
                        .map(|circuit| GatewayConnection {
                            id: id(&format!("Microsoft.Network/connections/er-{circuit}")),
                            provisioning_state: Some("Succeeded".to_owned()),
                            gateway_id: id("Microsoft.Network/virtualNetworkGateways/ergw"),
                            circuit_id: id(&format!(
                                "Microsoft.Network/expressRouteCircuits/{circuit}"
                            )),
                            ..Default::default()
                        })
                        .collect(),
                ),
                MicrosoftCollection::AzureExpressRouteCircuits(vec![
                    circuit("live", "Succeeded", "Provisioned"),
                    circuit("unprovisioned", "Succeeded", "NotProvisioned"),
                    circuit("deleting", "Deleting", "Provisioned"),
                ]),
            ]),
            &Settings::default(),
        );

        let gateway = Node::AzureVirtualNetworkGateway(azure_networking_id(
            "Microsoft.Network/virtualNetworkGateways/ergw",
        ));
        let live = Node::AzureExpressRouteCircuit(azure_networking_id(
            "Microsoft.Network/expressRouteCircuits/live",
        ));
        assert_edge(
            &builder,
            &gateway,
            &Node::AzureVirtualNetwork(azure_networking_id(
                "Microsoft.Network/virtualNetworks/hub",
            )),
            &Edge::AttachedTo,
        );
        let connection = Node::AzureGatewayConnection(azure_networking_id(
            "Microsoft.Network/connections/er-live",
        ));
        assert_edge(&builder, &connection, &gateway, &Edge::AttachedTo);
        assert_edge(&builder, &connection, &live, &Edge::AttachedTo);
        for (microsoft, customer) in [
            ("192.168.15.18/30", "192.168.15.17/30"),
            ("192.168.15.22/30", "192.168.15.21/30"),
        ] {
            assert_edge(&builder, &live, &link(microsoft), &Edge::ConnectsTo);
            assert_edge(&builder, &live, &link(customer), &Edge::RoutesTo);
        }
        for absent in [
            public("203.0.113.2"),
            link("fd00:15::2/126"),
            Node::AzureExpressRouteCircuit(azure_networking_id(
                "Microsoft.Network/expressRouteCircuits/unprovisioned",
            )),
            Node::AzureExpressRouteCircuit(azure_networking_id(
                "Microsoft.Network/expressRouteCircuits/deleting",
            )),
        ] {
            assert!(!builder.contains(&absent), "{absent:?} is not projected");
        }
    }

    #[test]
    fn an_interconnect_attachment_is_projected_unless_it_is_being_built_or_torn_down() {
        use crate::api::google::interconnect::{Interconnect, InterconnectAttachment, Router};
        use crate::cloud::definition::{GoogleCollection, Provider};

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
        projector::build(
            &mut builder,
            &Provider::GCP(vec![
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
            ]),
            &Settings::default(),
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
    fn a_gateway_being_modified_still_joins_its_networks() {
        use crate::cloud::definition::{AWSConnectivity, AmazonCollection};
        use aws_sdk_ec2::types::{
            TransitGateway, TransitGatewayAttachment, TransitGatewayAttachmentResourceType,
            TransitGatewayAttachmentState, TransitGatewayState,
        };

        let attachment = |vpc: &str| {
            TransitGatewayAttachment::builder()
                .transit_gateway_id("tgw-1")
                .resource_type(TransitGatewayAttachmentResourceType::Vpc)
                .resource_id(vpc)
                .state(TransitGatewayAttachmentState::Modifying)
                .build()
        };
        let builder = project_aws(vec![AmazonCollection::AmazonConnectivity(
            AWSConnectivity {
                transit_gateways: vec![
                    TransitGateway::builder()
                        .transit_gateway_id("tgw-1")
                        .state(TransitGatewayState::Modifying)
                        .build(),
                ],
                transit_gateway_attachments: vec![attachment("vpc-a"), attachment("vpc-b")],
                ..Default::default()
            },
        )]);

        let tgw = Node::AwsEc2TransitGateway("tgw-1".into());
        assert_edge(
            &builder,
            &Node::AwsRegion("us-east-1".into()),
            &tgw,
            &Edge::Contains,
        );
        for vpc in ["vpc-a", "vpc-b"] {
            assert_edge(
                &builder,
                &tgw,
                &Node::AwsEc2Vpc(vpc.into()),
                &Edge::AttachedTo,
            );
        }
    }

    #[test]
    fn a_gke_network_named_bare_is_the_same_network_instances_name_by_url() {
        use crate::api::google::gke::Cluster;
        use crate::cloud::definition::{GoogleCollection, Provider};

        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &Provider::GCP(vec![(
                "p".to_owned(),
                GoogleCollection::GoogleGke(vec![Cluster {
                    name: Some("c".to_owned()),
                    network: Some("default".to_owned()),
                    ..Default::default()
                }]),
            )]),
            &Settings::default(),
        );

        assert_edge(
            &builder,
            &Node::GcpComputeNetwork("projects/p/global/networks/default".into()),
            &Node::GcpGkeCluster("c".into()),
            &Edge::Contains,
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
    fn an_instance_reporting_no_interfaces_keeps_its_own_address() {
        use aws_sdk_ec2::types::Instance;

        let instance = Instance::builder()
            .instance_id("i-lonely")
            .private_ip_address("10.0.9.9")
            .build();
        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &crate::cloud::definition::Provider::AWS(vec![(
                "us-east-1".to_owned(),
                crate::cloud::definition::AmazonCollection::AmazonInstances(vec![instance]),
            )]),
            &Settings::default(),
        );

        assert_edge(
            &builder,
            &Node::AwsEc2Instance("i-lonely".into()),
            &Node::ip("10.0.9.9", "i-lonely"),
            &Edge::ConnectsTo,
        );
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
            &Node::AwsRoute53RecordSet("db.globex.io.".into()),
            &endpoint,
            &Edge::ResolvesTo,
        );
    }

    #[test]
    fn every_network_projects_its_own_address_space() {
        use crate::cloud::definition::{AWSConnectivity, AmazonCollection};
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

    #[test]
    fn only_live_transit_gateway_routing_is_projected() {
        use crate::cloud::definition::{
            AWSConnectivity, AWSTransitGatewayRouteTable, AWSTransitGatewayRouting,
            AmazonCollection,
        };
        use aws_sdk_ec2::types::{
            PeeringTgwInfo, TransitGateway, TransitGatewayAssociationState,
            TransitGatewayAttachmentResourceType, TransitGatewayAttachmentState,
            TransitGatewayPeeringAttachment, TransitGatewayPropagationState, TransitGatewayRoute,
            TransitGatewayRouteAttachment, TransitGatewayRouteState, TransitGatewayRouteTable,
            TransitGatewayRouteTableAssociation, TransitGatewayRouteTablePropagation,
            TransitGatewayState,
        };

        let gateway = |id: &str, state: TransitGatewayState| {
            TransitGateway::builder()
                .transit_gateway_id(id)
                .state(state)
                .build()
        };
        let table = |id: &str, tgw: &str| {
            TransitGatewayRouteTable::builder()
                .transit_gateway_route_table_id(id)
                .transit_gateway_id(tgw)
                .build()
        };
        let association = |vpc: &str, state: TransitGatewayAssociationState| {
            TransitGatewayRouteTableAssociation::builder()
                .resource_type(TransitGatewayAttachmentResourceType::Vpc)
                .resource_id(vpc)
                .state(state)
                .build()
        };
        let propagation = |vpc: &str, state: TransitGatewayPropagationState| {
            TransitGatewayRouteTablePropagation::builder()
                .resource_type(TransitGatewayAttachmentResourceType::Vpc)
                .resource_id(vpc)
                .state(state)
                .build()
        };
        let route = |destination: &str, state: TransitGatewayRouteState| {
            let builder = TransitGatewayRoute::builder();
            if destination.starts_with("pl-") {
                builder.prefix_list_id(destination)
            } else {
                builder.destination_cidr_block(destination)
            }
            .state(state)
            .transit_gateway_attachments(
                TransitGatewayRouteAttachment::builder()
                    .resource_type(TransitGatewayAttachmentResourceType::Vpc)
                    .resource_id("vpc-routed")
                    .build(),
            )
            .build()
        };
        let peering = |id: &str, far: &str, state: TransitGatewayAttachmentState| {
            TransitGatewayPeeringAttachment::builder()
                .transit_gateway_attachment_id(id)
                .requester_tgw_info(
                    PeeringTgwInfo::builder()
                        .transit_gateway_id("tgw-a")
                        .build(),
                )
                .accepter_tgw_info(PeeringTgwInfo::builder().transit_gateway_id(far).build())
                .state(state)
                .build()
        };
        let builder = project_aws(vec![
            AmazonCollection::AmazonConnectivity(AWSConnectivity {
                transit_gateways: vec![
                    gateway("tgw-a", TransitGatewayState::Available),
                    gateway("tgw-going", TransitGatewayState::Deleting),
                ],
                ..Default::default()
            }),
            AmazonCollection::AmazonTransitGatewayRouting(AWSTransitGatewayRouting {
                route_tables: vec![
                    AWSTransitGatewayRouteTable {
                        table: table("tgw-rtb-a", "tgw-a"),
                        associations: vec![
                            association("vpc-on", TransitGatewayAssociationState::Associated),
                            association("vpc-joining", TransitGatewayAssociationState::Associating),
                        ],
                        propagations: vec![
                            propagation("vpc-on", TransitGatewayPropagationState::Enabled),
                            propagation("vpc-off", TransitGatewayPropagationState::Disabled),
                        ],
                        static_routes: vec![
                            route("10.8.0.0/16", TransitGatewayRouteState::Active),
                            route("10.9.0.0/16", TransitGatewayRouteState::Blackhole),
                            route("pl-0shared", TransitGatewayRouteState::Active),
                        ],
                    },
                    AWSTransitGatewayRouteTable {
                        table: table("tgw-rtb-going", "tgw-going"),
                        associations: Vec::new(),
                        propagations: Vec::new(),
                        static_routes: Vec::new(),
                    },
                ],
                peerings: vec![
                    peering(
                        "tgw-attach-live",
                        "tgw-far",
                        TransitGatewayAttachmentState::Available,
                    ),
                    peering(
                        "tgw-attach-asked",
                        "tgw-far",
                        TransitGatewayAttachmentState::PendingAcceptance,
                    ),
                    peering(
                        "tgw-attach-to-going",
                        "tgw-going",
                        TransitGatewayAttachmentState::Available,
                    ),
                ],
            }),
        ]);

        let live = Node::AwsEc2TransitGatewayRouteTable("tgw-rtb-a".into());
        assert_edge(
            &builder,
            &Node::AwsEc2Vpc("vpc-on".into()),
            &live,
            &Edge::AttachedTo,
        );
        assert_edge(
            &builder,
            &live,
            &Node::AwsEc2Vpc("vpc-on".into()),
            &Edge::RoutesTo,
        );
        let active = Node::AwsEc2TransitGatewayRoute {
            table: "tgw-rtb-a".into(),
            destination: "10.8.0.0/16".into(),
        };
        assert_edge(
            &builder,
            &active,
            &Node::AwsEc2Vpc("vpc-routed".into()),
            &Edge::RoutesTo,
        );
        assert_edge(
            &builder,
            &Node::AwsEc2TransitGatewayRoute {
                table: "tgw-rtb-a".into(),
                destination: "pl-0shared".into(),
            },
            &Node::AwsEc2Vpc("vpc-routed".into()),
            &Edge::RoutesTo,
        );
        let blackhole = Node::AwsEc2TransitGatewayRoute {
            table: "tgw-rtb-a".into(),
            destination: "10.9.0.0/16".into(),
        };
        assert_has_node(&builder, &blackhole);
        assert!(!builder.has_edge(
            &blackhole,
            &Node::AwsEc2Vpc("vpc-routed".into()),
            &Edge::RoutesTo
        ));
        let live_peering = Node::AwsEc2TransitGatewayPeeringAttachment("tgw-attach-live".into());
        assert_edge(
            &builder,
            &live_peering,
            &Node::AwsEc2TransitGateway("tgw-far".into()),
            &Edge::AttachedTo,
        );
        for absent in [
            Node::AwsEc2Vpc("vpc-joining".into()),
            Node::AwsEc2Vpc("vpc-off".into()),
            Node::AwsEc2TransitGateway("tgw-going".into()),
            Node::AwsEc2TransitGatewayRouteTable("tgw-rtb-going".into()),
            Node::AwsEc2TransitGatewayPeeringAttachment("tgw-attach-asked".into()),
        ] {
            assert!(!builder.contains(&absent), "{absent:?} is not projected");
        }
    }

    #[test]
    fn an_ncc_spoke_joins_its_hub_only_while_both_serve() {
        use crate::api::google::network_connectivity::{Hub, LinkedVpcNetwork, Spoke};
        use crate::cloud::definition::{GoogleCollection, Provider};

        let hub = |name: &str, state: &str| Hub {
            name: Some(format!("projects/p/locations/global/hubs/{name}")),
            state: Some(state.to_owned()),
        };
        let spoke = |name: &str, hub: &str, group: Option<&str>, state: &str| Spoke {
            name: Some(format!("projects/p/locations/global/spokes/{name}")),
            hub: Some(format!("projects/p/locations/global/hubs/{hub}")),
            group: group
                .map(|group| format!("projects/p/locations/global/hubs/{hub}/groups/{group}")),
            state: Some(state.to_owned()),
            linked_vpc_network: Some(LinkedVpcNetwork {
                uri: Some(format!(
                    "https://x/compute/v1/projects/p/global/networks/{name}"
                )),
            }),
        };
        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &Provider::GCP(vec![
                (
                    "p".to_owned(),
                    GoogleCollection::GoogleHubs(vec![
                        hub("live", "ACTIVE"),
                        hub("going", "DELETING"),
                    ]),
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
            ]),
            &Settings::default(),
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

    #[test]
    fn a_virtual_wan_draws_nothing_that_is_going_away() {
        use crate::api::azure::models::{HubVirtualNetworkConnection, VirtualHub, VirtualWan};
        use crate::cloud::definition::{MicrosoftCollection, Provider};

        let id = |rest: &str| fixtures::azure_id(rest);
        let hub = |name: &str, state: &str| VirtualHub {
            id: Some(id(&format!("Microsoft.Network/virtualHubs/{name}"))),
            virtual_wan_id: Some(id("Microsoft.Network/virtualWans/wan")),
            provisioning_state: Some(state.to_owned()),
            ..Default::default()
        };
        let connection = |hub: &str, vnet: &str, state: &str| HubVirtualNetworkConnection {
            hub_id: Some(id(&format!("Microsoft.Network/virtualHubs/{hub}"))),
            remote_virtual_network_id: Some(id(&format!(
                "Microsoft.Network/virtualNetworks/{vnet}"
            ))),
            provisioning_state: Some(state.to_owned()),
            ..Default::default()
        };
        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &Provider::Azure(vec![
                MicrosoftCollection::AzureVirtualWans(vec![VirtualWan {
                    id: Some(id("Microsoft.Network/virtualWans/wan")),
                    provisioning_state: Some("Succeeded".to_owned()),
                    ..Default::default()
                }]),
                MicrosoftCollection::AzureVirtualHubs(vec![
                    hub("hub-live", "Succeeded"),
                    hub("hub-going", "Deleting"),
                ]),
                MicrosoftCollection::AzureHubVirtualNetworkConnections(vec![
                    connection("hub-live", "vnet-a", "Succeeded"),
                    connection("hub-live", "vnet-b", "Deleting"),
                    connection("hub-going", "vnet-c", "Succeeded"),
                ]),
            ]),
            &Settings::default(),
        );

        let live = Node::AzureVirtualHub(azure_networking_id(
            "Microsoft.Network/virtualHubs/hub-live",
        ));
        assert_edge(
            &builder,
            &live,
            &Node::AzureVirtualWan(azure_networking_id("Microsoft.Network/virtualWans/wan")),
            &Edge::AttachedTo,
        );
        assert_edge(
            &builder,
            &live,
            &Node::AzureVirtualNetwork(azure_networking_id(
                "Microsoft.Network/virtualNetworks/vnet-a",
            )),
            &Edge::AttachedTo,
        );
        for absent in [
            Node::AzureVirtualNetwork(azure_networking_id(
                "Microsoft.Network/virtualNetworks/vnet-b",
            )),
            Node::AzureVirtualNetwork(azure_networking_id(
                "Microsoft.Network/virtualNetworks/vnet-c",
            )),
            Node::AzureVirtualHub(azure_networking_id(
                "Microsoft.Network/virtualHubs/hub-going",
            )),
        ] {
            assert!(!builder.contains(&absent), "{absent:?} is not projected");
        }
    }

    #[test]
    fn only_a_standard_virtual_wan_carries_traffic_between_its_networks() {
        use crate::api::azure::models::{HubVirtualNetworkConnection, VirtualHub, VirtualWan};
        use crate::atlas::derive::held_addresses;
        use crate::cloud::definition::{MicrosoftCollection, Provider};

        let vnet =
            |name: &str| azure_networking_id(&format!("Microsoft.Network/virtualNetworks/{name}"));
        let project = |wan_type: &str| {
            let mut builder = GraphBuilder::new();
            projector::build(
                &mut builder,
                &Provider::Azure(vec![
                    MicrosoftCollection::AzureVirtualWans(vec![VirtualWan {
                        id: Some(azure_id("Microsoft.Network/virtualWans/wan")),
                        wan_type: Some(wan_type.to_owned()),
                        ..Default::default()
                    }]),
                    MicrosoftCollection::AzureVirtualHubs(vec![VirtualHub {
                        id: Some(azure_id("Microsoft.Network/virtualHubs/hub")),
                        virtual_wan_id: Some(azure_id("Microsoft.Network/virtualWans/wan")),
                        ..Default::default()
                    }]),
                    MicrosoftCollection::AzureHubVirtualNetworkConnections(
                        ["vnet-a", "vnet-b"]
                            .iter()
                            .map(|name| HubVirtualNetworkConnection {
                                hub_id: Some(azure_id("Microsoft.Network/virtualHubs/hub")),
                                remote_virtual_network_id: Some(azure_id(&format!(
                                    "Microsoft.Network/virtualNetworks/{name}"
                                ))),
                                ..Default::default()
                            })
                            .collect(),
                    ),
                ]),
                &Settings::default(),
            );
            let nic = builder.get_or_add_node(Node::AzureNetworkInterface("nic-b".into()));
            builder.link_to(nic, Node::ip("10.2.0.4", &vnet("vnet-b")), Edge::ConnectsTo);
            builder
        };
        let reached_from_a = |builder: &GraphBuilder| {
            held_addresses(&builder.graph)
                .holder(&vnet("vnet-a"), "10.2.0.4")
                .map(|index| builder.graph[index].clone())
        };
        let wan = Node::AzureVirtualWan(azure_networking_id("Microsoft.Network/virtualWans/wan"));
        let hub = Node::AzureVirtualHub(azure_networking_id("Microsoft.Network/virtualHubs/hub"));

        let standard = project("Standard");
        assert_edge(&standard, &hub, &wan, &Edge::AttachedTo);
        assert_eq!(
            reached_from_a(&standard),
            Some(Node::ip("10.2.0.4", &vnet("vnet-b")))
        );

        let basic = project("Basic");
        assert_edge(&basic, &wan, &hub, &Edge::Contains);
        assert_eq!(
            reached_from_a(&basic),
            None,
            "a Basic hub carries no traffic between the networks on it"
        );
    }

    fn vpc_route(table: &str, destination: &str) -> Node {
        Node::AwsEc2Route {
            table: table.into(),
            destination: destination.into(),
        }
    }

    fn project_aws(collections: Vec<crate::cloud::definition::AmazonCollection>) -> GraphBuilder {
        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &crate::cloud::definition::Provider::AWS(
                collections
                    .into_iter()
                    .map(|c| ("us-east-1".to_owned(), c))
                    .collect(),
            ),
            &Settings::default(),
        );
        builder
    }

    fn attached_interface(id: &str, instance_id: &str) -> aws_sdk_ec2::types::NetworkInterface {
        use aws_sdk_ec2::types::{NetworkInterface, NetworkInterfaceAttachment};

        NetworkInterface::builder()
            .network_interface_id(id)
            .subnet_id("subnet-globex")
            .vpc_id("vpc-globex")
            .private_ip_address("10.0.5.5")
            .attachment(
                NetworkInterfaceAttachment::builder()
                    .instance_id(instance_id)
                    .build(),
            )
            .build()
    }

    // DescribeInstances is filtered to running/pending, but a stopped instance
    // keeps its interfaces attached, so the attachment still names it.
    #[test]
    fn an_interface_attached_to_an_instance_the_scan_dropped_invents_no_instance() {
        use crate::cloud::definition::AmazonCollection;

        let builder = project_aws(vec![
            AmazonCollection::AmazonNetworkInterfaces(vec![attached_interface(
                "eni-stopped",
                "i-stopped",
            )]),
            AmazonCollection::AmazonInstances(Vec::new()),
        ]);

        assert!(
            builder
                .index_of(&Node::AwsEc2Instance("i-stopped".into()))
                .is_none(),
            "an attachment is not evidence the scan kept the instance"
        );
        assert_edge(
            &builder,
            &Node::AwsEc2Eni("eni-stopped".into()),
            &Node::ip("10.0.5.5", "vpc-globex"),
            &Edge::ConnectsTo,
        );
    }

    #[test]
    fn an_interface_owns_its_attachment_once_the_scan_confirms_the_instance() {
        use crate::cloud::definition::AmazonCollection;

        let instance = aws_sdk_ec2::types::Instance::builder()
            .instance_id("i-running")
            .build();
        let builder = project_aws(vec![
            AmazonCollection::AmazonNetworkInterfaces(vec![attached_interface(
                "eni-running",
                "i-running",
            )]),
            AmazonCollection::AmazonInstances(vec![instance]),
        ]);

        assert_edge(
            &builder,
            &Node::AwsEc2Instance("i-running".into()),
            &Node::AwsEc2Eni("eni-running".into()),
            &Edge::HasIp,
        );
    }

    fn target_group(
        target_type: aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum,
        target: &str,
    ) -> GraphBuilder {
        target_group_scanning(target_type, target, Vec::new())
    }

    fn target_group_scanning(
        target_type: aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum,
        target: &str,
        running: Vec<&str>,
    ) -> GraphBuilder {
        target_group_with_interfaces(target_type, target, running, Vec::new())
    }

    fn target_group_with_interfaces(
        target_type: aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum,
        target: &str,
        running: Vec<&str>,
        interfaces: Vec<aws_sdk_ec2::types::NetworkInterface>,
    ) -> GraphBuilder {
        use crate::cloud::definition::{AWSLoadBalancing, AmazonCollection};
        use aws_sdk_elasticloadbalancingv2::types::{
            TargetDescription, TargetGroup, TargetHealthDescription,
        };

        let arn = "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/typed/1";
        let mut target_health = std::collections::HashMap::new();
        target_health.insert(
            arn.to_owned(),
            vec![
                TargetHealthDescription::builder()
                    .target(TargetDescription::builder().id(target).build())
                    .build(),
            ],
        );
        project_aws(vec![
            AmazonCollection::AmazonLoadBalancers(AWSLoadBalancing {
                load_balancers: Vec::new(),
                target_groups: vec![
                    TargetGroup::builder()
                        .target_group_arn(arn)
                        .target_type(target_type)
                        .vpc_id("vpc-globex")
                        .build(),
                ],
                listeners: Vec::new(),
                target_health,
            }),
            AmazonCollection::AmazonInstances(
                running
                    .into_iter()
                    .map(|id| {
                        aws_sdk_ec2::types::Instance::builder()
                            .instance_id(id)
                            .build()
                    })
                    .collect(),
            ),
            AmazonCollection::AmazonNetworkInterfaces(interfaces),
        ])
    }

    fn typed_target_group() -> Node {
        Node::AwsElbTargetGroup(
            "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/typed/1".into(),
        )
    }

    fn any_instance(builder: &GraphBuilder) -> bool {
        builder
            .graph
            .node_weights()
            .any(|n| matches!(n, Node::AwsEc2Instance(_)))
    }

    // An IP target group's targets are addresses. Building an instance from one
    // invented a node, and the service chain then drew an edge to it.
    #[test]
    fn an_ip_target_is_an_address_the_group_routes_to() {
        use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

        let builder = target_group(TargetTypeEnum::Ip, "10.0.1.5");

        assert!(!any_instance(&builder));
        assert_edge(
            &builder,
            &typed_target_group(),
            &Node::ip("10.0.1.5", "vpc-globex"),
            &Edge::RoutesTo,
        );
        assert!(
            !builder.graph.edge_weights().any(|e| e == &Edge::ConnectsTo),
            "ConnectsTo to a pivot means holding it, which a target group does not"
        );
    }

    #[test]
    fn an_ip_target_on_a_stopped_instances_interface_is_not_linked() {
        use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

        let builder = target_group_with_interfaces(
            TargetTypeEnum::Ip,
            "10.0.5.5",
            Vec::new(),
            vec![attached_interface("eni-stopped", "i-stopped")],
        );

        assert!(
            !builder.has_edge(
                &typed_target_group(),
                &Node::ip("10.0.5.5", "vpc-globex"),
                &Edge::RoutesTo
            ),
            "the registration names the stopped instance by its address, and the scan dropped it"
        );
        assert!(!any_instance(&builder));
    }

    #[test]
    fn an_ip_target_on_a_running_instances_interface_is_linked() {
        use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

        let builder = target_group_with_interfaces(
            TargetTypeEnum::Ip,
            "10.0.5.5",
            vec!["i-running"],
            vec![attached_interface("eni-running", "i-running")],
        );

        assert_edge(
            &builder,
            &typed_target_group(),
            &Node::ip("10.0.5.5", "vpc-globex"),
            &Edge::RoutesTo,
        );
    }

    #[test]
    fn an_ip_target_is_read_in_its_groups_vpc() {
        use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

        let builder = target_group_with_interfaces(
            TargetTypeEnum::Ip,
            "10.0.5.5",
            vec!["i-elsewhere"],
            vec![{
                let mut interface = attached_interface("eni-elsewhere", "i-elsewhere");
                interface.vpc_id = Some("vpc-elsewhere".to_owned());
                interface
            }],
        );

        assert_edge(
            &builder,
            &typed_target_group(),
            &Node::ip("10.0.5.5", "vpc-globex"),
            &Edge::RoutesTo,
        );
        assert!(!builder.has_edge(
            &typed_target_group(),
            &Node::ip("10.0.5.5", "vpc-elsewhere"),
            &Edge::RoutesTo
        ));
    }

    #[test]
    fn a_lambda_target_invents_no_instance() {
        use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

        let builder = target_group(
            TargetTypeEnum::Lambda,
            "arn:aws:lambda:us-east-1:123:function:globex-handler",
        );

        assert!(!any_instance(&builder));
    }

    #[test]
    fn a_running_instance_target_is_linked() {
        use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

        let builder = target_group_scanning(TargetTypeEnum::Instance, "i-target", vec!["i-target"]);

        assert_edge(
            &builder,
            &Node::AwsElbTargetGroup(
                "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/typed/1".into(),
            ),
            &Node::AwsEc2Instance("i-target".into()),
            &Edge::ConnectsTo,
        );
    }

    // A stopped instance stays registered, but DescribeInstances only returns
    // running and pending ones. The registration must not bring it back.
    #[test]
    fn a_stopped_instance_target_invents_no_instance() {
        use aws_sdk_elasticloadbalancingv2::types::TargetTypeEnum;

        let builder = target_group_scanning(TargetTypeEnum::Instance, "i-stopped", Vec::new());

        assert!(!any_instance(&builder));
        assert!(
            builder.index_of(&typed_target_group()).is_some(),
            "the group itself is still real"
        );
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
    fn identical_nodes_merge() {
        let mut builder = GraphBuilder::new();
        let a = builder.get_or_add_node(Node::ip("10.0.0.1", fixtures::AWS_NETWORK));
        let b = builder.get_or_add_node(Node::ip("10.0.0.1", fixtures::AWS_NETWORK));
        assert_eq!(a, b);
        assert_eq!(builder.graph.node_count(), 1);
    }

    #[test]
    fn identical_edges_dedupe() {
        let mut builder = GraphBuilder::new();
        let a = builder.get_or_add_node(Node::ip("10.0.0.1", fixtures::AWS_NETWORK));
        let b = builder.get_or_add_node(Node::GenericHostname("db.example.com".into()));
        builder.add_edge(a, b, Edge::ResolvesTo);
        builder.add_edge(a, b, Edge::ResolvesTo);
        assert_eq!(builder.graph.edge_count(), 1);

        builder.add_edge(a, b, Edge::RoutesTo);
        assert_eq!(builder.graph.edge_count(), 2);
    }

    #[test]
    fn large_cidrs_are_filtered() {
        assert!(is_large_cidr("0.0.0.0/0"));
        assert!(is_large_cidr("::/0"));
        assert!(is_large_cidr("*"));
        assert!(is_large_cidr("10.0.0.0/8"));
        assert!(!is_large_cidr("10.20.0.5"));
        assert!(!is_large_cidr("192.0.2.44/32"));
        assert!(!is_large_cidr("10.0.0.0/16"));
    }

    #[test]
    fn config_resources_respect_exclude_by_default() {
        use crate::cloud::definition::{AmazonCollection, Provider};
        use std::collections::HashMap;

        let make_provider = || {
            let mut resources = HashMap::new();
            resources.insert(
                "AWS::Unknown::Widget".to_owned(),
                vec![
                    aws_sdk_config::types::ResourceIdentifier::builder()
                        .resource_id("widget-1")
                        .build(),
                ],
            );
            Provider::AWS(vec![(
                fixtures::REGION.to_owned(),
                AmazonCollection::AmazonResources(resources),
            )])
        };
        let widget = Node::AwsConfigResource {
            resource_type: "AWS::Unknown::Widget".into(),
            id: "widget-1".into(),
        };

        let include = Settings {
            regions: vec![fixtures::REGION.to_owned()],
            ..Default::default()
        };
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &make_provider(), &include);
        assert_has_node(&builder, &widget);

        let exclude = Settings {
            regions: vec![fixtures::REGION.to_owned()],
            exclude_by_default: true,
            ..Default::default()
        };
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &make_provider(), &exclude);
        assert!(
            !builder.node_map.contains_key(&widget),
            "unknown config resource should be excluded when exclude_by_default is set"
        );
    }

    #[test]
    fn every_aws_kind_config_records_has_one_source() {
        use crate::atlas::projector::aws::{TYPED_CONFIG_TYPES, use_aws_resource};
        use aws_sdk_config::types::ResourceType;

        let config_records_nothing_like = [
            "AwsRegion",
            "AwsTag",
            "AwsEc2AvailabilityZone",
            "AwsRoute53RecordSet",
            "AwsConfigResource",
            "AwsDirectConnectConnection",
            "AwsDirectConnectGateway",
            "AwsDirectConnectVirtualInterface",
            "AwsEc2Route",
            "AwsEc2TransitGatewayRoute",
            "AwsEc2TransitGatewayPeeringAttachment",
        ];
        let typed: Vec<&str> = TYPED_CONFIG_TYPES
            .iter()
            .map(|typed| (typed.node)("probe".into()).kind())
            .collect();
        for kind in Node::ALL_KINDS
            .iter()
            .filter(|kind| kind.starts_with("Aws"))
        {
            assert!(
                typed.contains(kind) != config_records_nothing_like.contains(kind),
                "{kind}: add its Config type to TYPED_CONFIG_TYPES, or list it as a kind Config \
                 does not record"
            );
        }
        for typed in TYPED_CONFIG_TYPES {
            assert!(
                ResourceType::values().contains(&typed.resource_type),
                "{} is not a Config resource type",
                typed.resource_type
            );
            assert!(
                !use_aws_resource(typed.resource_type, false),
                "{} has a typed node, so the catch-all must never draw it",
                typed.resource_type
            );
        }
    }

    #[test]
    fn a_config_listing_never_redraws_a_resource_its_own_collector_draws() {
        use crate::atlas::projector::aws::TYPED_CONFIG_TYPES;
        use crate::cloud::definition::{AmazonCollection, Provider};
        use aws_sdk_config::types::ResourceIdentifier;
        use std::collections::HashMap;

        let listed = |id: &str| vec![ResourceIdentifier::builder().resource_id(id).build()];
        let mut resources: HashMap<String, Vec<ResourceIdentifier>> = TYPED_CONFIG_TYPES
            .iter()
            .map(|typed| (typed.resource_type.to_owned(), listed("twin")))
            .collect();
        for drawn_as_an_edge in [
            "AWS::EC2::TransitGatewayAttachment",
            "AWS::ElasticLoadBalancingV2::Listener",
        ] {
            resources.insert(drawn_as_an_edge.to_owned(), listed("twin"));
        }
        resources.insert("AWS::S3::Bucket".to_owned(), listed("assets"));

        let mut builder = GraphBuilder::new();
        projector::build(
            &mut builder,
            &Provider::AWS(vec![(
                fixtures::REGION.to_owned(),
                AmazonCollection::AmazonResources(resources),
            )]),
            &Settings {
                regions: vec![fixtures::REGION.to_owned()],
                ..Default::default()
            },
        );

        let generic: Vec<&Node> = builder
            .graph
            .node_weights()
            .filter(|node| matches!(node, Node::AwsConfigResource { .. }))
            .collect();
        assert_eq!(
            generic,
            vec![&Node::AwsConfigResource {
                resource_type: "AWS::S3::Bucket".into(),
                id: "assets".into(),
            }],
            "Config is the source only for what no collector draws"
        );
    }

    #[test]
    fn render_snapshot_covers_whole_fixture_graph() {
        let builder = fixtures::build_graph();
        let snapshot = crate::atlas::export::render_snapshot(&builder.graph);

        assert_eq!(snapshot.version, crate::atlas::export::SNAPSHOT_VERSION);
        assert_eq!(snapshot.nodes.len(), builder.graph.node_count());
        assert_eq!(snapshot.edges.len(), builder.graph.edge_count());

        let ids: std::collections::HashSet<u32> = snapshot.nodes.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), snapshot.nodes.len(), "node ids must be unique");
        for edge in &snapshot.edges {
            assert!(ids.contains(&edge.source) && ids.contains(&edge.target));
        }

        let json = crate::atlas::export::snapshot_json(&builder.graph).unwrap();
        assert!(json.contains("\"kind\":\"AwsEc2Eni\""));
        assert!(json.contains("\"version\":3"));

        let keys: std::collections::HashSet<&str> =
            snapshot.nodes.iter().map(|n| n.key.as_str()).collect();
        assert_eq!(keys.len(), snapshot.nodes.len(), "node keys must be unique");
    }

    #[test]
    fn node_key_is_unique_across_fixture_graph() {
        use crate::atlas::export::node_key;
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
        let patch = crate::atlas::patch::diff(&builder.graph, &builder.graph);
        assert!(
            patch.is_empty(),
            "diffing a graph against itself is a no-op"
        );
    }

    #[test]
    fn diff_detects_added_and_removed_nodes_and_edges() {
        use crate::atlas::definition::{Edge, Node};
        use crate::atlas::export::{edge_key, node_key};

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

        let patch = crate::atlas::patch::diff(&old.graph, &new.graph);

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

    fn all_sources() -> std::collections::HashSet<CollectionSource> {
        std::collections::HashSet::from([
            CollectionSource::Aws,
            CollectionSource::Gcp,
            CollectionSource::Azure,
            CollectionSource::Cloudflare,
        ])
    }

    #[test]
    fn carry_forward_lets_a_healthy_source_delete_its_edge_to_a_shared_pivot() {
        use crate::atlas::patch::carry_forward;

        let host = Node::GenericHostname("old.example.com".into());
        let zone = Node::GcpDnsManagedZone("globex-zone".into());
        let instance = Node::AwsEc2Instance("i-1".into());

        let mut live = GraphBuilder::new();
        let zone_idx = live.get_or_add_node(zone.clone());
        live.link_to(zone_idx, host.clone(), Edge::ResolvesTo);
        let instance_idx = live.get_or_add_node(instance.clone());
        live.link_to(instance_idx, host.clone(), Edge::ConnectsTo);

        let mut next = GraphBuilder::new();
        next.get_or_add_node(zone.clone());

        carry_forward(
            &mut next,
            &live.graph,
            &std::collections::HashSet::from([CollectionSource::Aws]),
        );

        assert!(
            next.has_edge(&instance, &host, &Edge::ConnectsTo),
            "the unreadable source's edge must be retained"
        );
        assert!(
            !next.has_edge(&zone, &host, &Edge::ResolvesTo),
            "a healthy source's deleted edge must not be resurrected by the pivot it shares"
        );
    }

    #[test]
    fn carry_forward_lets_a_healthy_dns_provider_repoint_and_delete_records() {
        use crate::atlas::patch::carry_forward;

        let repointed = Node::CloudflareDnsRecord("rec-app".into());
        let deleted = Node::CloudflareDnsRecord("rec-gone".into());
        let (old_ip, new_ip) = (public("192.0.2.1"), public("192.0.2.2"));

        let mut live = GraphBuilder::new();
        for (record, name) in [
            (&repointed, "app.example.com"),
            (&deleted, "gone.example.com"),
        ] {
            let idx = live.get_or_add_node(record.clone());
            live.link_from(idx, Node::hostname(name), Edge::ResolvesTo);
            live.link_to(idx, old_ip.clone(), Edge::ResolvesTo);
        }

        let mut next = GraphBuilder::new();
        let idx = next.get_or_add_node(repointed.clone());
        next.link_from(idx, Node::hostname("app.example.com"), Edge::ResolvesTo);
        next.link_to(idx, new_ip.clone(), Edge::ResolvesTo);

        carry_forward(
            &mut next,
            &live.graph,
            &std::collections::HashSet::from([CollectionSource::Aws]),
        );

        assert!(!next.has_edge(&repointed, &old_ip, &Edge::ResolvesTo));
        assert!(!next.has_edge(
            &Node::hostname("gone.example.com"),
            &deleted,
            &Edge::ResolvesTo
        ));
        assert!(
            !next
                .graph
                .edge_references()
                .any(|e| next.graph[e.target()] == old_ip),
            "an unreadable AWS scan must not keep a healthy Cloudflare zone's old answer"
        );
    }

    #[test]
    fn carry_forward_restores_only_what_the_scan_is_missing() {
        use crate::atlas::export::{edge_key, node_key};
        use crate::atlas::patch::{carry_forward, diff};

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
        crate::atlas::derive::all(&mut partial, &fixtures::observed());

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
    fn carry_forward_drops_pivots_nothing_points_at_any_more() {
        use crate::atlas::patch::carry_forward;

        let anchored = Node::ip("10.0.0.1", fixtures::AWS_NETWORK);
        let orphan = Node::GenericIpAddress("203.0.113.7".into());
        let traffic_only = Node::GenericIpAddress("203.0.113.8".into());

        let mut live = GraphBuilder::new();

        let instance = live.get_or_add_node(Node::AwsEc2Instance("i-1".into()));
        live.link_to(instance, anchored.clone(), Edge::ConnectsTo);

        live.get_or_add_node(orphan.clone());

        live.link_to(instance, traffic_only.clone(), Edge::TrafficFlow);

        let mut next = GraphBuilder::new();
        carry_forward(&mut next, &live.graph, &all_sources());

        assert!(
            next.contains(&anchored),
            "a referenced pivot is still needed"
        );
        assert!(!next.contains(&orphan));
        assert!(!next.contains(&traffic_only));
    }

    #[test]
    fn carry_forward_is_a_no_op_when_the_scan_is_a_superset() {
        use crate::atlas::patch::{carry_forward, diff};

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
