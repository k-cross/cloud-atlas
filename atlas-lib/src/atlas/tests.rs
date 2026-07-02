#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use crate::atlas::definition::{Edge, Node};
    use crate::atlas::graph_builder::GraphBuilder;
    use crate::atlas::projector;
    use crate::atlas::util::is_large_cidr;
    use crate::fixtures;
    use crate::{Settings, fixtures::azure_id};

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

    // ------------------------------------------------------------------
    // Exhaustiveness guards
    //
    // The fixture environment must exercise every projector path. If a new
    // Node/Edge variant is added, `kind()`'s exhaustive match forces it into
    // ALL_KINDS, and these tests then fail until the fixtures (and a
    // projector) actually produce it.
    // ------------------------------------------------------------------

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

    // ------------------------------------------------------------------
    // Per-provider semantic projections
    // ------------------------------------------------------------------

    #[test]
    fn amazon_projection() {
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &fixtures::aws(), &fixtures::settings());

        let instance = Node::AwsEc2Instance("i-globex-web-01".into());
        let eni = Node::AwsEc2Eni("i-globex-web-01".into());
        let subnet = Node::AwsEc2Subnet("subnet-public-1a".into());
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

        // Core ENI pivot: Instance -> HasIp -> ENI -> AttachedTo -> Subnet
        assert_edge(&builder, &instance, &eni, &Edge::HasIp);
        assert_edge(&builder, &eni, &subnet, &Edge::AttachedTo);
        assert_edge(&builder, &region, &vpc, &Edge::Contains);
        assert_edge(&builder, &vpc, &subnet, &Edge::Contains);
        assert_edge(&builder, &az, &instance, &Edge::Contains);
        assert_edge(&builder, &instance, &tag, &Edge::DependsOn);
        assert_edge(&builder, &instance, &sg, &Edge::ConnectsTo);

        // LB -> TG -> Instance
        assert_edge(&builder, &lb, &tg, &Edge::ConnectsTo);
        assert_edge(&builder, &tg, &instance, &Edge::ConnectsTo);

        // Lambda -> IAM role and its security group
        assert_edge(&builder, &lambda, &role, &Edge::DependsOn);
        assert_edge(&builder, &lambda, &sg, &Edge::ConnectsTo);

        // Security group cross-reference and egress -> generic IPs
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

        // Route53: A record -> generic IP, CNAME record -> generic hostname
        assert_edge(
            &builder,
            &Node::AwsRoute53RecordSet("origin.globex.io.".into()),
            &Node::GenericIpAddress("203.0.113.10".into()),
            &Edge::ConnectsTo,
        );
        assert_edge(
            &builder,
            &Node::AwsRoute53RecordSet("app.globex.io.".into()),
            &Node::GenericHostname("app-globex.azurewebsites.net".into()),
            &Edge::ConnectsTo,
        );

        // Config resources: S3 is contained by the "global" region node
        let s3 = Node::AwsConfigResource {
            resource_type: "AWS::S3::Bucket".into(),
            id: "globex-assets".into(),
        };
        assert_edge(&builder, &global, &s3, &Edge::Contains);

        // Routing / egress plane: public subnet -> route table -> IGW -> VPC,
        // private subnet -> route table -> NAT -> EIP -> public IP.
        let public_rt = Node::AwsEc2RouteTable("rtb-public".into());
        let private_rt = Node::AwsEc2RouteTable("rtb-private".into());
        let igw = Node::AwsEc2InternetGateway("igw-globex".into());
        let nat = Node::AwsEc2NatGateway("nat-globex".into());
        let eip = Node::AwsEc2Eip("eipalloc-globex".into());
        let private_subnet = Node::AwsEc2Subnet("subnet-private-1a".into());
        assert_edge(&builder, &vpc, &public_rt, &Edge::Contains);
        assert_edge(&builder, &subnet, &public_rt, &Edge::AttachedTo);
        assert_edge(&builder, &public_rt, &igw, &Edge::RoutesTo);
        assert_edge(&builder, &igw, &vpc, &Edge::AttachedTo);
        assert_edge(&builder, &private_subnet, &private_rt, &Edge::AttachedTo);
        assert_edge(&builder, &private_rt, &nat, &Edge::RoutesTo);
        assert_edge(&builder, &nat, &subnet, &Edge::AttachedTo);
        assert_edge(&builder, &nat, &eip, &Edge::HasIp);
        assert_edge(
            &builder,
            &eip,
            &Node::GenericIpAddress("203.0.113.50".into()),
            &Edge::ConnectsTo,
        );

        // Remaining standalone services
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
        let network = Node::GcpComputeNetwork(
            format!(
                "https://www.googleapis.com/compute/v1/projects/{}/global/networks/vpc-globex",
                fixtures::GCP_PROJECT
            )
            .as_str()
            .into(),
        );

        assert_edge(&builder, &project, &instance, &Edge::DependsOn);
        assert_edge(&builder, &zone, &instance, &Edge::Contains);

        // Firewall: contained by network, egress rule routes to generic IP
        let fw = Node::GcpComputeFirewall("fw-egress-globex".into());
        assert_edge(&builder, &network, &fw, &Edge::Contains);
        assert_edge(
            &builder,
            &fw,
            &Node::GenericIpAddress("192.0.2.55".into()),
            &Edge::RoutesTo,
        );

        // SQL -> private IP (the Azure NSG seam target)
        assert_edge(
            &builder,
            &Node::GcpSqlInstance("globex-analytics-db".into()),
            &Node::GenericIpAddress("10.20.0.5".into()),
            &Edge::ConnectsTo,
        );

        // Cloud Run reachable via its hostname pivot
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

        // Network containment and forwarding rule IP
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

        // PubSub subscription -> topic
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

        let vm = Node::AzureVirtualMachine(
            azure_id("Microsoft.Compute/virtualMachines/vm-worker")
                .as_str()
                .into(),
        );
        let nic = Node::AzureNetworkInterface(
            azure_id("Microsoft.Network/networkInterfaces/nic-worker")
                .as_str()
                .into(),
        );
        let vnet = Node::AzureVirtualNetwork(
            azure_id("Microsoft.Network/virtualNetworks/vnet-globex")
                .as_str()
                .into(),
        );
        let subnet = Node::AzureSubnet(
            azure_id("Microsoft.Network/virtualNetworks/vnet-globex/subnets/default")
                .as_str()
                .into(),
        );
        let nsg = Node::AzureNetworkSecurityGroup(
            azure_id("Microsoft.Network/networkSecurityGroups/nsg-worker")
                .as_str()
                .into(),
        );

        assert_edge(&builder, &vm, &nic, &Edge::ConnectsTo);
        assert_edge(&builder, &vnet, &subnet, &Edge::Contains);
        assert_edge(&builder, &subnet, &nsg, &Edge::ConnectsTo);

        // NSG outbound rules: generic IP (the GCP SQL seam) + service tag
        assert_edge(
            &builder,
            &nsg,
            &Node::GenericIpAddress("10.20.0.5".into()),
            &Edge::RoutesTo,
        );
        assert_edge(
            &builder,
            &nsg,
            &Node::AzureServiceTag("AzureCloud".into()),
            &Edge::RoutesTo,
        );

        // Public IP -> generic IP (the Cloudflare A record seam)
        assert_edge(
            &builder,
            &Node::AzurePublicIpAddress(
                azure_id("Microsoft.Network/publicIPAddresses/pip-globex")
                    .as_str()
                    .into(),
            ),
            &Node::GenericIpAddress("198.51.100.10".into()),
            &Edge::ConnectsTo,
        );

        // App Service reachable via its hostname pivot
        assert_edge(
            &builder,
            &Node::GenericHostname("app-globex.azurewebsites.net".into()),
            &Node::AzureAppService(azure_id("Microsoft.Web/sites/app-globex").as_str().into()),
            &Edge::RoutesTo,
        );

        // Leaf resources
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

        // Zone contains records; records route to their hostnames
        assert_edge(
            &builder,
            &zone,
            &Node::CloudflareDnsRecord("rec-app".into()),
            &Edge::Contains,
        );
        assert_edge(
            &builder,
            &Node::CloudflareDnsRecord("rec-app".into()),
            &Node::GenericHostname("app.globex.io".into()),
            &Edge::RoutesTo,
        );

        // DNS resolution edges for all record types
        assert_edge(
            &builder,
            &Node::GenericHostname("app.globex.io".into()),
            &Node::GenericHostname("app-globex.azurewebsites.net".into()),
            &Edge::ResolvesTo,
        );
        assert_edge(
            &builder,
            &Node::GenericHostname("data.globex.io".into()),
            &Node::GenericHostname("run.globex.app".into()),
            &Edge::ResolvesTo,
        );
        assert_edge(
            &builder,
            &Node::GenericHostname("db.globex.io".into()),
            &Node::GenericIpAddress("198.51.100.10".into()),
            &Edge::ResolvesTo,
        );
        assert_edge(
            &builder,
            &Node::GenericHostname("v6.globex.io".into()),
            &Node::GenericIpAddress("2001:db8::10".into()),
            &Edge::ResolvesTo,
        );

        // Worker bindings: KV, R2, Durable Object, D1, and external service
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

    // ------------------------------------------------------------------
    // Cross-cloud stitching
    // ------------------------------------------------------------------

    #[test]
    fn multi_cloud_seams_merge() {
        let builder = fixtures::build_graph();

        // Cloudflare CNAME chain resolves through to the Azure App Service:
        // rec-app -> app.globex.io -> app-globex.azurewebsites.net -> AppService
        let cf_hostname = Node::GenericHostname("app.globex.io".into());
        let azure_hostname = Node::GenericHostname("app-globex.azurewebsites.net".into());
        let app = Node::AzureAppService(azure_id("Microsoft.Web/sites/app-globex").as_str().into());
        assert_edge(&builder, &cf_hostname, &azure_hostname, &Edge::ResolvesTo);
        assert_edge(&builder, &azure_hostname, &app, &Edge::RoutesTo);

        // AWS Route53 CNAME lands on the SAME merged hostname node
        assert_edge(
            &builder,
            &Node::AwsRoute53RecordSet("app.globex.io.".into()),
            &azure_hostname,
            &Edge::ConnectsTo,
        );

        // Azure NSG and GCP SQL meet at one merged IP node
        let shared_ip = Node::GenericIpAddress("10.20.0.5".into());
        assert_edge(
            &builder,
            &Node::AzureNetworkSecurityGroup(
                azure_id("Microsoft.Network/networkSecurityGroups/nsg-worker")
                    .as_str()
                    .into(),
            ),
            &shared_ip,
            &Edge::RoutesTo,
        );
        assert_edge(
            &builder,
            &Node::GcpSqlInstance("globex-analytics-db".into()),
            &shared_ip,
            &Edge::ConnectsTo,
        );

        // Cloudflare A record and Azure public IP meet at one merged IP node
        let public_ip = Node::GenericIpAddress("198.51.100.10".into());
        assert_edge(
            &builder,
            &Node::GenericHostname("db.globex.io".into()),
            &public_ip,
            &Edge::ResolvesTo,
        );
        assert_edge(
            &builder,
            &Node::AzurePublicIpAddress(
                azure_id("Microsoft.Network/publicIPAddresses/pip-globex")
                    .as_str()
                    .into(),
            ),
            &public_ip,
            &Edge::ConnectsTo,
        );

        // Cloudflare CNAME reaches the GCP Cloud Run hostname pivot
        assert_edge(
            &builder,
            &Node::GenericHostname("data.globex.io".into()),
            &Node::GenericHostname("run.globex.app".into()),
            &Edge::ResolvesTo,
        );
    }

    // ------------------------------------------------------------------
    // Graph behavior
    // ------------------------------------------------------------------

    #[test]
    fn identical_nodes_merge() {
        let mut builder = GraphBuilder::new();
        let a = builder.get_or_add_node(Node::GenericIpAddress("10.0.0.1".into()));
        let b = builder.get_or_add_node(Node::GenericIpAddress("10.0.0.1".into()));
        assert_eq!(a, b);
        assert_eq!(builder.graph.node_count(), 1);
    }

    #[test]
    fn identical_edges_dedupe() {
        let mut builder = GraphBuilder::new();
        let a = builder.get_or_add_node(Node::GenericIpAddress("10.0.0.1".into()));
        let b = builder.get_or_add_node(Node::GenericHostname("db.example.com".into()));
        builder.add_edge(a, b, Edge::ResolvesTo);
        builder.add_edge(a, b, Edge::ResolvesTo);
        assert_eq!(builder.graph.edge_count(), 1);

        // A different edge kind between the same nodes is NOT a duplicate
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
}
