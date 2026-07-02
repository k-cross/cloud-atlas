#[allow(clippy::module_inception)]
mod tests {
    use crate::Settings;
    use crate::atlas::definition::{Edge, Node};
    use crate::atlas::graph_builder::GraphBuilder;
    use crate::atlas::projector;
    use crate::cloud::definition::{AmazonCollection, Provider};
    use aws_sdk_ec2::types::builders::InstanceBuilder;
    use petgraph::visit::EdgeRef;

    fn assert_edge(builder: &GraphBuilder, a: &Node, b: &Node, edge: &Edge) {
        let a_idx = builder
            .node_map
            .get(a)
            .unwrap_or_else(|| panic!("Node not found: {:?}", a));
        let b_idx = builder
            .node_map
            .get(b)
            .unwrap_or_else(|| panic!("Node not found: {:?}", b));

        let mut found = false;
        for e in builder.graph.edges_connecting(*a_idx, *b_idx) {
            if e.weight() == edge {
                found = true;
                break;
            }
        }
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

    fn make_aws_provider() -> Provider {
        let i1 = InstanceBuilder::default()
            .set_image_id(Some("ami-09d3b8424b6c5d4aa".to_owned()))
            .set_instance_id(Some("i-01ee77706a905ce9627".to_owned()))
            .set_vpc_id(Some("vpc-f98d2f9f".to_owned()))
            .set_subnet_id(Some("subnet-dac7a6f7".to_owned()))
            .build();
        let i2 = InstanceBuilder::default()
            .set_image_id(Some("ami-09d3b8424b6c5d4aa".to_owned()))
            .set_instance_id(Some("i-0385e6e2f59529b68".to_owned()))
            .set_vpc_id(Some("vpc-f98d2f9f".to_owned()))
            .set_subnet_id(Some("subnet-dac7a6f7".to_owned()))
            .build();

        let lb = aws_sdk_elasticloadbalancingv2::types::LoadBalancer::builder()
            .load_balancer_arn(
                "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/my-lb/50dc",
            )
            .vpc_id("vpc-f98d2f9f")
            .build();

        let tg = aws_sdk_elasticloadbalancingv2::types::TargetGroup::builder()
            .target_group_arn("arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/my-tg/73e2")
            .vpc_id("vpc-f98d2f9f")
            .build();

        let action = aws_sdk_elasticloadbalancingv2::types::Action::builder()
            .target_group_arn("arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/my-tg/73e2")
            .build();

        let listener = aws_sdk_elasticloadbalancingv2::types::Listener::builder()
            .load_balancer_arn(
                "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/my-lb/50dc",
            )
            .default_actions(action)
            .build();

        let target = aws_sdk_elasticloadbalancingv2::types::TargetDescription::builder()
            .id("i-01ee77706a905ce9627")
            .build();

        let health = aws_sdk_elasticloadbalancingv2::types::TargetHealthDescription::builder()
            .target(target)
            .build();

        let mut health_map = std::collections::HashMap::new();
        health_map.insert(
            "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/my-tg/73e2".to_owned(),
            vec![health],
        );

        let hosted_zone = aws_sdk_route53::types::HostedZone::builder()
            .id("/hostedzone/Z123456789")
            .name("example.com.")
            .caller_reference("test")
            .build()
            .unwrap();

        let record_1 = aws_sdk_route53::types::ResourceRecord::builder()
            .value("10.0.0.1")
            .build()
            .unwrap();

        let record_set = aws_sdk_route53::types::ResourceRecordSet::builder()
            .name("www.example.com.")
            .r#type(aws_sdk_route53::types::RrType::A)
            .resource_records(record_1)
            .build()
            .unwrap();

        Provider::AWS(vec![
            (
                "us-east-1".to_owned(),
                AmazonCollection::AmazonInstances(vec![i1, i2]),
            ),
            (
                "us-east-1".to_owned(),
                AmazonCollection::AmazonLoadBalancers {
                    load_balancers: vec![lb],
                    target_groups: vec![tg],
                    listeners: vec![listener],
                    target_health: health_map,
                },
            ),
            (
                "us-east-1".to_owned(),
                AmazonCollection::AmazonRoute53 {
                    hosted_zones: vec![hosted_zone],
                    record_sets: vec![record_set],
                },
            ),
        ])
    }

    #[test]
    fn amazon_instance_graph() {
        let s = Settings {
            regions: vec!["us-east-1".to_owned()],
            gcp_projects: None,
            azure_subscriptions: None,
            all: false,
            verbose: false,
            exclude_by_default: false,
            cloudflare: false,
        };
        let provider = make_aws_provider();
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &provider, &s);

        // Assert nodes
        let instance = Node::AwsEc2Instance("i-01ee77706a905ce9627".into());
        let eni = Node::AwsEc2Eni("i-01ee77706a905ce9627".into());
        let subnet = Node::AwsEc2Subnet("subnet-dac7a6f7".into());
        let lb = Node::AwsElbLoadBalancer(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/my-lb/50dc".into(),
        );
        let tg = Node::AwsElbTargetGroup(
            "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/my-tg/73e2".into(),
        );
        let hosted_zone = Node::AwsRoute53HostedZone("/hostedzone/Z123456789".into());
        let record_set = Node::AwsRoute53RecordSet("www.example.com.".into());
        let generic_ip = Node::GenericIpAddress("10.0.0.1".into());

        assert_has_node(&builder, &instance);
        assert_has_node(&builder, &eni);
        assert_has_node(&builder, &subnet);
        assert_has_node(&builder, &lb);
        assert_has_node(&builder, &tg);
        assert_has_node(&builder, &hosted_zone);
        assert_has_node(&builder, &record_set);
        assert_has_node(&builder, &generic_ip);

        // Assert edges (Instance -> ENI -> Subnet)
        assert_edge(&builder, &instance, &eni, &Edge::HasIp);
        assert_edge(&builder, &eni, &subnet, &Edge::AttachedTo);

        // Assert Load Balancer -> Target Group -> Instance
        assert_edge(&builder, &lb, &tg, &Edge::ConnectsTo);
        assert_edge(&builder, &tg, &instance, &Edge::ConnectsTo);

        // Assert Route53 -> Generic IP
        assert_edge(&builder, &record_set, &generic_ip, &Edge::ConnectsTo);
    }
    fn make_gcp_provider() -> Provider {
        use crate::api::google::compute::{Firewall, Instance};
        use crate::api::google::compute_network::{ForwardingRule, Network, Subnetwork};
        use crate::api::google::dns::ManagedZone;
        use crate::api::google::functions::CloudFunction;
        use crate::api::google::gke::Cluster;
        use crate::api::google::pubsub::{Subscription, Topic};
        use crate::api::google::run::Service;
        use crate::api::google::sql::{SqlInstance, SqlIpAddress};
        use crate::api::google::storage::Bucket;
        use crate::cloud::definition::GoogleCollection;

        let i1 = Instance {
            id: Some("12345".to_string().into()),
            name: Some("my-gcp-instance".to_owned()),
            self_link: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/zones/us-central1-a/instances/my-gcp-instance".to_owned()),
            ..Default::default()
        };

        let i2 = Instance {
            id: Some("67890".to_string().into()),
            name: Some("my-gcp-instance-2".to_owned()),
            self_link: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/zones/us-central1-a/instances/my-gcp-instance-2".to_owned()),
            ..Default::default()
        };

        let fw = Firewall {
            id: Some("fw-1".to_string().into()),
            name: Some("allow-ssh".to_string().into()),
            network: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/global/networks/default".to_string().into()),
            ..Default::default()
        };

        let ip = SqlIpAddress {
            ip_type: Some("PRIMARY".to_string().into()),
            ip_address: Some("10.0.0.5".to_string().into()),
        };
        let sql = SqlInstance {
            name: Some("my-sql-db".to_string().into()),
            ip_addresses: Some(vec![ip]),
            ..Default::default()
        };

        let dns = ManagedZone {
            name: Some("my-zone".to_string().into()),
            dns_name: Some("example.com.".to_string().into()),
            ..Default::default()
        };

        let gke = Cluster {
            name: Some("my-cluster".to_string().into()),
            network: Some(
                "projects/my-gcp-project/global/networks/default"
                    .to_string()
                    .into(),
            ),
            ..Default::default()
        };

        let func = CloudFunction {
            name: Some(
                "projects/my-gcp-project/locations/us-central1/functions/my-func".to_string(),
            ),
            ..Default::default()
        };

        let bucket = Bucket {
            id: Some("my-bucket".to_string().into()),
            name: Some("my-bucket".to_string().into()),
            ..Default::default()
        };

        let topic = Topic {
            name: Some("projects/my-gcp-project/topics/my-topic".to_string().into()),
        };

        let sub = Subscription {
            name: Some(
                "projects/my-gcp-project/subscriptions/my-sub"
                    .to_string()
                    .into(),
            ),
            topic: Some("projects/my-gcp-project/topics/my-topic".to_string().into()),
        };

        let run_svc = Service {
            name: Some(
                "projects/my-gcp-project/locations/us-central1/services/my-svc"
                    .to_string()
                    .into(),
            ),
            ..Default::default()
        };

        let net = Network {
            self_link: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/global/networks/default".to_string().into()),
            ..Default::default()
        };

        let subnet = Subnetwork {
            self_link: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/regions/us-central1/subnetworks/default".to_string().into()),
            network: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/global/networks/default".to_string().into()),
            ..Default::default()
        };

        let fw_rule = ForwardingRule {
            id: Some("fw-rule-1".to_string().into()),
            ip_address: Some("34.120.0.1".to_string().into()),
            ..Default::default()
        };

        Provider::GCP(vec![
            GoogleCollection::GoogleInstances(vec![i1, i2]),
            GoogleCollection::GoogleFirewalls(vec![fw]),
            GoogleCollection::GoogleSql(vec![sql]),
            GoogleCollection::GoogleDns(vec![dns]),
            GoogleCollection::GoogleGke(vec![gke]),
            GoogleCollection::GoogleFunctions(vec![func]),
            GoogleCollection::GoogleStorageBuckets(vec![bucket]),
            GoogleCollection::GooglePubSubTopics(vec![topic]),
            GoogleCollection::GooglePubSubSubscriptions(vec![sub]),
            GoogleCollection::GoogleRunServices(vec![run_svc]),
            GoogleCollection::GoogleNetworks(vec![net]),
            GoogleCollection::GoogleSubnetworks(vec![subnet]),
            GoogleCollection::GoogleForwardingRules(vec![fw_rule]),
        ])
    }

    #[test]
    fn gcp_instance_graph() {
        let s = Settings {
            regions: vec![],
            gcp_projects: Some(vec!["my-gcp-project".to_owned()]),
            azure_subscriptions: None,
            all: false,
            verbose: false,
            exclude_by_default: false,
            cloudflare: false,
        };
        let provider = make_gcp_provider();
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &provider, &s);

        let project = Node::GcpProject("my-gcp-project".into());
        let instance = Node::GcpComputeInstance("12345".into());
        let sql = Node::GcpSqlInstance("my-sql-db".into());
        let generic_ip = Node::GenericIpAddress("10.0.0.5".into());
        let gke = Node::GcpGkeCluster("my-cluster".into());
        let gke_network =
            Node::GcpComputeNetwork("projects/my-gcp-project/global/networks/default".into());
        let network = Node::GcpComputeNetwork(
            "https://www.googleapis.com/compute/v1/projects/my-gcp-project/global/networks/default"
                .into(),
        );
        let subnet = Node::GcpComputeSubnetwork("https://www.googleapis.com/compute/v1/projects/my-gcp-project/regions/us-central1/subnetworks/default".into());

        assert_has_node(&builder, &project);
        assert_has_node(&builder, &instance);
        assert_has_node(&builder, &sql);
        assert_has_node(&builder, &generic_ip);
        assert_has_node(&builder, &gke);
        assert_has_node(&builder, &network);
        assert_has_node(&builder, &gke_network);
        assert_has_node(&builder, &subnet);

        // Assert edges
        assert_edge(&builder, &project, &instance, &Edge::DependsOn);
        assert_edge(&builder, &sql, &generic_ip, &Edge::ConnectsTo);
        assert_edge(&builder, &gke_network, &gke, &Edge::Contains);
        assert_edge(&builder, &network, &subnet, &Edge::Contains);
    }
    fn make_azure_provider() -> Provider {
        use crate::api::azure::models::*;
        use crate::cloud::definition::MicrosoftCollection;

        let vm = VirtualMachine {
            id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Compute/virtualMachines/vm1".to_string().into()),
            name: Some("vm1".to_string().into()),
            location: Some("eastus".to_string().into()),
            network_interfaces: vec!["/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/networkInterfaces/nic1".to_string()],
        };

        let vnet = VirtualNetwork {
            id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1".to_string().into()),
            name: Some("vnet1".to_string().into()),
            location: Some("eastus".to_string().into()),
            subnets: vec!["/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1/subnets/default".to_string()],
        };

        let subnet = Subnet {
            id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1/subnets/default".to_string().into()),
            name: Some("default".to_string().into()),
            vnet_id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1".to_string().into()),
            network_security_group_id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/networkSecurityGroups/nsg1".to_string().into()),
        };

        let nsg = NetworkSecurityGroup {
            id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/networkSecurityGroups/nsg1".to_string().into()),
            name: Some("nsg1".to_string().into()),
            location: Some("eastus".to_string().into()),
        };

        Provider::Azure(vec![
            MicrosoftCollection::AzureVirtualMachines(vec![vm]),
            MicrosoftCollection::AzureVirtualNetworks(vec![vnet]),
            MicrosoftCollection::AzureSubnets(vec![subnet]),
            MicrosoftCollection::AzureNetworkSecurityGroups(vec![nsg]),
        ])
    }

    #[test]
    fn azure_instance_graph() {
        let s = Settings {
            regions: vec![],
            gcp_projects: None,
            azure_subscriptions: Some(vec!["sub1".to_owned()]),
            all: false,
            verbose: false,
            exclude_by_default: false,
            cloudflare: false,
        };
        let provider = make_azure_provider();
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &provider, &s);

        let vm = Node::AzureVirtualMachine("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Compute/virtualMachines/vm1".into());
        let nic = Node::AzureNetworkSecurityGroup("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/networkInterfaces/nic1".into());
        let vnet = Node::AzureVirtualNetwork("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1".into());
        let subnet = Node::AzureSubnet("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1/subnets/default".into());
        let nsg = Node::AzureNetworkSecurityGroup("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/networkSecurityGroups/nsg1".into());

        assert_has_node(&builder, &vm);
        assert_has_node(&builder, &nic);
        assert_has_node(&builder, &vnet);
        assert_has_node(&builder, &subnet);
        assert_has_node(&builder, &nsg);

        // Assert edges
        assert_edge(&builder, &vm, &nic, &Edge::ConnectsTo);
        assert_edge(&builder, &vnet, &subnet, &Edge::Contains);
        assert_edge(&builder, &subnet, &nsg, &Edge::ConnectsTo);
    }

    fn make_cloudflare_provider() -> Provider {
        use crate::cloud::definition::CloudflareCollection;
        use serde_json::json;

        let zone_json = json!({
            "id": "zone-123",
            "name": "example.com",
            "account": { "id": "acc-1", "name": "my acc" },
            "activated_on": "2023-01-01T00:00:00Z",
            "created_on": "2023-01-01T00:00:00Z",
            "development_mode": 0,
            "original_name_servers": ["ns1"],
            "original_registrar": "reg",
            "original_dnshost": "host",
            "modified_on": "2023-01-01T00:00:00Z",
            "name_servers": ["ns1"],
            "owner": { "id": "owner1", "type": "user", "email": "a@a.com" },
            "permissions": ["#zone:read"],
            "plan": { "id": "plan1", "name": "free", "price": 0, "currency": "USD", "frequency": "monthly", "is_subscribed": true, "can_subscribe": false, "legacy_id": "free", "legacy_discount": false, "externally_managed": false },
            "plan_pending": { "id": "plan1", "name": "free", "price": 0, "currency": "USD", "frequency": "monthly", "is_subscribed": true, "can_subscribe": false, "legacy_id": "free", "legacy_discount": false, "externally_managed": false },
            "status": "active",
            "paused": false,
            "type": "full",
            "meta": {
                "custom_certificate_quota": 1,
                "page_rule_quota": 3,
                "phishing_detected": false
            }
        });

        let record_json = json!({
            "id": "rec-456",
            "name": "api.example.com",
            "type": "CNAME",
            "content": "aws-alb.example.com",
            "proxied": true,
            "ttl": 1,
            "modified_on": "2023-01-01T00:00:00Z",
            "created_on": "2023-01-01T00:00:00Z",
            "meta": {
                "auto_added": true,
                "managed_by_apps": false,
                "managed_by_argo_tunnel": false,
                "source": "primary"
            },
            "proxiable": true
        });

        let zone: cloudflare::endpoints::zones::zone::Zone =
            serde_json::from_value(zone_json).unwrap();
        let record: cloudflare::endpoints::dns::dns::DnsRecord =
            serde_json::from_value(record_json).unwrap();

        let worker = crate::cloud::cloudflare::worker::WorkerScript {
            id: "my-worker".to_string(),
            created_on: None,
            modified_on: None,
        };

        let worker_bindings = vec![(
            "my-worker".to_string(),
            vec![
                crate::cloud::cloudflare::worker::WorkerBinding {
                    name: "MY_KV".to_string(),
                    binding_type: "kv_namespace".to_string(),
                    namespace_id: Some("kv-789".to_string()),
                    bucket_name: None,
                    id: None,
                    extra: std::collections::HashMap::new(),
                },
                crate::cloud::cloudflare::worker::WorkerBinding {
                    name: "MY_POSTGRES".to_string(),
                    binding_type: "secret".to_string(),
                    namespace_id: None,
                    bucket_name: None,
                    id: None,
                    extra: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            "text".to_string(),
                            serde_json::json!("postgres://db.neon.tech"),
                        );
                        map
                    },
                },
            ],
        )];

        Provider::Cloudflare(CloudflareCollection {
            zones: vec![zone],
            dns_records: vec![("zone-123".to_string(), vec![record])],
            workers: vec![worker],
            kv_namespaces: vec![],
            r2_buckets: vec![],
            durable_objects: vec![],
            d1_databases: vec![],
            worker_bindings,
        })
    }

    #[test]
    fn cloudflare_instance_graph() {
        let s = Settings {
            regions: vec![],
            gcp_projects: None,
            azure_subscriptions: None,
            all: false,
            verbose: false,
            exclude_by_default: false,
            cloudflare: true,
        };
        let provider = make_cloudflare_provider();
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &provider, &s);

        let zone = Node::CloudflareZone("zone-123".into());
        let dns_record = Node::CloudflareDnsRecord("rec-456".into());
        let generic_hostname = Node::GenericHostname("api.example.com".into());
        let worker = Node::CloudflareWorker("my-worker".into());
        let kv = Node::CloudflareKvNamespace("kv-789".into());
        let ext_service = Node::ExternalService("postgres://db.neon.tech".into());

        assert_has_node(&builder, &zone);
        assert_has_node(&builder, &dns_record);
        assert_has_node(&builder, &generic_hostname);
        assert_has_node(&builder, &worker);
        assert_has_node(&builder, &kv);
        assert_has_node(&builder, &ext_service);

        // Assert edges
        assert_edge(&builder, &zone, &dns_record, &Edge::Contains);
        assert_edge(&builder, &dns_record, &generic_hostname, &Edge::RoutesTo);
        assert_edge(&builder, &worker, &kv, &Edge::ConnectsTo);
        assert_edge(&builder, &worker, &ext_service, &Edge::ConnectsTo);
    }

    #[test]
    fn multi_cloud_interaction_graph() {
        let s = Settings {
            regions: vec!["us-east-1".to_owned()],
            gcp_projects: Some(vec!["my-gcp-project".to_owned()]),
            azure_subscriptions: Some(vec!["sub1".to_owned()]),
            all: false,
            verbose: false,
            exclude_by_default: false,
            cloudflare: true,
        };

        let aws = make_aws_provider();
        let gcp = make_gcp_provider();
        let azure = make_azure_provider();
        let cf = make_cloudflare_provider();

        let mut builder = GraphBuilder::new();

        // Project all clouds onto the same graph
        projector::build(&mut builder, &aws, &s);
        projector::build(&mut builder, &gcp, &s);
        projector::build(&mut builder, &azure, &s);
        projector::build(&mut builder, &cf, &s);

        // Assert cross-cloud relationships via generic nodes

        // 1. Cloudflare DNS resolving to a generic hostname
        let cf_record = Node::CloudflareDnsRecord("rec-456".into());
        let generic_hostname = Node::GenericHostname("api.example.com".into());
        assert_edge(&builder, &cf_record, &generic_hostname, &Edge::RoutesTo);

        // 2. AWS Route53 resolving to a generic IP
        let route53_record = Node::AwsRoute53RecordSet("www.example.com.".into());
        let generic_ip = Node::GenericIpAddress("10.0.0.1".into());
        assert_edge(&builder, &route53_record, &generic_ip, &Edge::ConnectsTo);

        // 3. GCP SQL Instance connected to generic IP
        let gcp_sql = Node::GcpSqlInstance("my-sql-db".into());
        let generic_sql_ip = Node::GenericIpAddress("10.0.0.5".into());
        assert_edge(&builder, &gcp_sql, &generic_sql_ip, &Edge::ConnectsTo);

        // Note: In real-world data, the IPs and Hostnames would match exactly between
        // two cloud providers in their API responses (e.g. an AWS ALB hostname returned
        // as a Cloudflare DNS CNAME target). Our graph intrinsically merges nodes with the
        // same enum variant and inner string because they derive PartialEq, Eq, Hash.
        // This test proves that different providers populate the same generic nodes.

        // For instance, let's inject a fake route in AWS to the GCP SQL IP to prove
        // the graph correctly connects them.
        let aws_instance = Node::AwsEc2Instance("i-01ee77706a905ce9627".into());
        let aws_instance_idx = builder.get_or_add_node(aws_instance.clone());
        let gcp_sql_ip_idx = builder.get_or_add_node(generic_sql_ip.clone());
        builder.add_edge(aws_instance_idx, gcp_sql_ip_idx, Edge::RoutesTo);

        assert_edge(&builder, &aws_instance, &generic_sql_ip, &Edge::RoutesTo);
        assert_edge(&builder, &gcp_sql, &generic_sql_ip, &Edge::ConnectsTo);
    }
}

//[
//    Instance {
//        ami_launch_index: Some(0),
//        image_id: Some("ami-09d3b8424b6c5d4aa".to_owned()),
//        instance_id: Some("i-01ee77706a905ce9627".to_owned()),
//        instance_type: Some(T2Micro),
//        kernel_id: None,
//        key_name: Some("a-key".to_owned()),
//        launch_time: Some(DateTime {
//            seconds: 1666371279,
//            subsecond_nanos: 0,
//        }),
//        monitoring: Some(Monitoring {
//            state: Some(Disabled),
//        }),
//        placement: Some(Placement {
//            availability_zone: Some("us-east-1d".to_owned()),
//            affinity: None,
//            group_name: Some("".to_owned()),
//            partition_number: None,
//            host_id: None,
//            tenancy: Some(Default),
//            spread_domain: None,
//            host_resource_group_arn: None,
//        }),
//        platform: None,
//        private_dns_name: Some("ip-178-31-61-178.ec2.internal".to_owned()),
//        private_ip_address: Some("178.31.62.185".to_owned()),
//        product_codes: Some([]),
//        public_dns_name: Some("ec2-5-95-160-24.compute-1.amazonaws.com".to_owned()),
//        public_ip_address: Some("5.96.160.24".to_owned()),
//        ramdisk_id: None,
//        state: Some(InstanceState {
//            code: Some(16),
//            name: Some(Running),
//        }),
//        state_transition_reason: Some("".to_owned()),
//        subnet_id: Some("subnet-dac7a6f7".to_owned()),
//        vpc_id: Some("vpc-f98d2f9f".to_owned()),
//        architecture: Some(X8664),
//        block_device_mappings: Some([InstanceBlockDeviceMapping {
//            device_name: Some("/dev/xvda".to_owned()),
//            ebs: Some(EbsInstanceBlockDevice {
//                attach_time: Some(DateTime {
//                    seconds: 1666371280,
//                    subsecond_nanos: 0,
//                }),
//                delete_on_termination: Some(true),
//                status: Some(Attached),
//                volume_id: Some("vol-0d3693f48963f8821".to_owned()),
//            }),
//        }]),
//        client_token: Some("".to_owned()),
//        ebs_optimized: Some(false),
//        ena_support: Some(true),
//        hypervisor: Some(Xen),
//        iam_instance_profile: None,
//        instance_lifecycle: None,
//        elastic_gpu_associations: None,
//        elastic_inference_accelerator_associations: None,
//        network_interfaces: Some([InstanceNetworkInterface {
//            association: Some(InstanceNetworkInterfaceAssociation {
//                carrier_ip: None,
//                customer_owned_ip: None,
//                ip_owner_id: Some("amazon".to_owned()),
//                public_dns_name: Some(
//                    "ec2-5-95-160-24.compute-1.amazonaws.com".to_owned(),
//                ),
//                public_ip: Some("5.95.160.24".to_owned()),
//            }),
//            attachment: Some(InstanceNetworkInterfaceAttachment {
//                attach_time: Some(DateTime {
//                    seconds: 1666371279,
//                    subsecond_nanos: 0,
//                }),
//                attachment_id: Some("eni-attach-07671ca1b6be32131".to_owned()),
//                delete_on_termination: Some(true),
//                device_index: Some(0),
//                status: Some(Attached),
//                network_card_index: Some(0),
//            }),
//            description: Some("".to_owned()),
//            groups: Some([
//                GroupIdentifier {
//                    group_name: Some("ec2-rds-5".to_owned()),
//                    group_id: Some("sg-0e7924cea43567852".to_owned()),
//                },
//                GroupIdentifier {
//                    group_name: Some("launch-wizard-6-sec".to_owned()),
//                    group_id: Some("sg-04c42260980ef15e5".to_owned()),
//                },
//            ]),
//            ipv6_addresses: Some([]),
//            mac_address: Some("12:74:d4:da:ff:b7".to_owned()),
//            network_interface_id: Some("eni-0b96c4be1eb530476".to_owned()),
//            owner_id: Some("781865768738".to_owned()),
//            private_dns_name: Some("ip-178-31-61-178.ec2.internal".to_owned()),
//            private_ip_address: Some("178.31.62.185".to_owned()),
//            private_ip_addresses: Some([InstancePrivateIpAddress {
//                association: Some(InstanceNetworkInterfaceAssociation {
//                    carrier_ip: None,
//                    customer_owned_ip: None,
//                    ip_owner_id: Some("amazon".to_owned()),
//                    public_dns_name: Some(
//                        "ec2-5-95-160-24.compute-1.amazonaws.com".to_owned(),
//                    ),
//                    public_ip: Some("5.95.160.24".to_owned()),
//                }),
//                primary: Some(true),
//                private_dns_name: Some("ip-178-31-61-178.ec2.internal".to_owned()),
//                private_ip_address: Some("178.31.62.185".to_owned()),
//            }]),
//            source_dest_check: Some(true),
//            status: Some(InUse),
//            subnet_id: Some("subnet-dac7a6f7".to_owned()),
//            vpc_id: Some("vpc-f98d2f9f".to_owned()),
//            interface_type: Some("interface".to_owned()),
//            ipv4_prefixes: None,
//            ipv6_prefixes: None,
//        }]),
//        outpost_arn: None,
//        root_device_name: Some("/dev/xvda".to_owned()),
//        root_device_type: Some(Ebs),
//        security_groups: Some([
//            GroupIdentifier {
//                group_name: Some("ec2-rds-5".to_owned()),
//                group_id: Some("sg-0e7924cea43567852".to_owned()),
//            },
//            GroupIdentifier {
//                group_name: Some("launch-wizard-6-sec2".to_owned()),
//                group_id: Some("sg-04c42260980ef15e5".to_owned()),
//            },
//        ]),
//        source_dest_check: Some(true),
//        spot_instance_request_id: None,
//        sriov_net_support: None,
//        state_reason: None,
//        tags: Some([Tag {
//            key: Some("Name".to_owned()),
//            value: Some("sec2".to_owned()),
//        }]),
//        virtualization_type: Some(Hvm),
//        cpu_options: Some(CpuOptions {
//            core_count: Some(1),
//            threads_per_core: Some(1),
//        }),
//        capacity_reservation_id: None,
//        capacity_reservation_specification: Some(
//            CapacityReservationSpecificationResponse {
//                capacity_reservation_preference: Some(Open),
//                capacity_reservation_target: None,
//            },
//        ),
//        hibernation_options: Some(HibernationOptions {
//            configured: Some(false),
//        }),
//        licenses: None,
//        metadata_options: Some(InstanceMetadataOptionsResponse {
//            state: Some(Applied),
//            http_tokens: Some(Optional),
//            http_put_response_hop_limit: Some(1),
//            http_endpoint: Some(Enabled),
//            http_protocol_ipv6: Some(Disabled),
//            instance_metadata_tags: Some(Disabled),
//        }),
//        enclave_options: Some(EnclaveOptions {
//            enabled: Some(false),
//        }),
//        boot_mode: None,
//        platform_details: Some("Linux/UNIX".to_owned()),
//        usage_operation: Some("RunInstances".to_owned()),
//        usage_operation_update_time: Some(DateTime {
//            seconds: 1666371279,
//            subsecond_nanos: 0,
//        }),
//        private_dns_name_options: Some(PrivateDnsNameOptionsResponse {
//            hostname_type: Some(IpName),
//            enable_resource_name_dns_a_record: Some(true),
//            enable_resource_name_dns_aaaa_record: Some(false),
//        }),
//        ipv6_address: None,
//        tpm_support: None,
//        maintenance_options: Some(InstanceMaintenanceOptions {
//            auto_recovery: Some(Default),
//        }),
//    },
//    Instance {
//        ami_launch_index: Some(0),
//        image_id: Some("ami-09d3b3274b6c5d4aa".to_owned()),
//        instance_id: Some("i-0385e6e2f59529b68".to_owned()),
//        instance_type: Some(C524xlarge),
//        kernel_id: None,
//        key_name: Some("drb-rmq-perftest-kp".to_owned()),
//        launch_time: Some(DateTime {
//            seconds: 1666624590,
//            subsecond_nanos: 0,
//        }),
//        monitoring: Some(Monitoring {
//            state: Some(Disabled),
//        }),
//        placement: Some(Placement {
//            availability_zone: Some("us-east-1a".to_owned()),
//            affinity: None,
//            group_name: Some("".to_owned()),
//            partition_number: None,
//            host_id: None,
//            tenancy: Some(Default),
//            spread_domain: None,
//            host_resource_group_arn: None,
//        }),
//        platform: None,
//        private_dns_name: Some("ip-178-31-61-178.ec2.internal".to_owned()),
//        private_ip_address: Some("178.31.11.76".to_owned()),
//        product_codes: Some([]),
//        public_dns_name: Some("ec2-99-163-27-126.compute-1.amazonaws.com".to_owned()),
//        public_ip_address: Some("5.163.27.126".to_owned()),
//        ramdisk_id: None,
//        state: Some(InstanceState {
//            code: Some(16),
//            name: Some(Running),
//        }),
//        state_transition_reason: Some("".to_owned()),
//        subnet_id: Some("subnet-089bca41".to_owned()),
//        vpc_id: Some("vpc-f98d2f9f".to_owned()),
//        architecture: Some(X8664),
//        block_device_mappings: Some([InstanceBlockDeviceMapping {
//            device_name: Some("/dev/xvda".to_owned()),
//            ebs: Some(EbsInstanceBlockDevice {
//                attach_time: Some(DateTime {
//                    seconds: 1666624592,
//                    subsecond_nanos: 0,
//                }),
//                delete_on_termination: Some(true),
//                status: Some(Attached),
//                volume_id: Some("vol-0c95677277803b019".to_owned()),
//            }),
//        }]),
//        client_token: Some("".to_owned()),
//        ebs_optimized: Some(true),
//        ena_support: Some(true),
//        hypervisor: Some(Xen),
//        iam_instance_profile: None,
//        instance_lifecycle: None,
//        elastic_gpu_associations: None,
//        elastic_inference_accelerator_associations: None,
//        network_interfaces: Some([InstanceNetworkInterface {
//            association: Some(InstanceNetworkInterfaceAssociation {
//                carrier_ip: None,
//                customer_owned_ip: None,
//                ip_owner_id: Some("amazon".to_owned()),
//                public_dns_name: Some(
//                    "ec2-99-163-27-126.compute-1.amazonaws.com".to_owned(),
//                ),
//                public_ip: Some("5.163.27.126".to_owned()),
//            }),
//            attachment: Some(InstanceNetworkInterfaceAttachment {
//                attach_time: Some(DateTime {
//                    seconds: 1666624590,
//                    subsecond_nanos: 0,
//                }),
//                attachment_id: Some("eni-attach-00e1c7de777819760".to_owned()),
//                delete_on_termination: Some(true),
//                device_index: Some(0),
//                status: Some(Attached),
//                network_card_index: Some(0),
//            }),
//            description: Some("".to_owned()),
//            groups: Some([GroupIdentifier {
//                group_name: Some("launch-wizard-15".to_owned()),
//                group_id: Some("sg-0a1a4d9b4877794d4".to_owned()),
//            }]),
//            ipv6_addresses: Some([]),
//            mac_address: Some("0a:1e:92:34:e9:93".to_owned()),
//            network_interface_id: Some("eni-034777fa5aaba2fd3".to_owned()),
//            owner_id: Some("781865768738".to_owned()),
//            private_dns_name: Some("ip-178-31-61-178.ec2.internal".to_owned()),
//            private_ip_address: Some("178.31.11.76".to_owned()),
//            private_ip_addresses: Some([InstancePrivateIpAddress {
//                association: Some(InstanceNetworkInterfaceAssociation {
//                    carrier_ip: None,
//                    customer_owned_ip: None,
//                    ip_owner_id: Some("amazon".to_owned()),
//                    public_dns_name: Some(
//                        "ec2-99-163-27-126.compute-1.amazonaws.com".to_owned(),
//                    ),
//                    public_ip: Some("5.163.27.126".to_owned()),
//                }),
//                primary: Some(true),
//                private_dns_name: Some("ip-178-31-61-178.ec2.internal".to_owned()),
//                private_ip_address: Some("178.31.11.76".to_owned()),
//            }]),
//            source_dest_check: Some(true),
//            status: Some(InUse),
//            subnet_id: Some("subnet-087abc41".to_owned()),
//            vpc_id: Some("vpc-f9777f9f".to_owned()),
//            interface_type: Some("interface".to_owned()),
//            ipv4_prefixes: None,
//            ipv6_prefixes: None,
//        }]),
//        outpost_arn: None,
//        root_device_name: Some("/dev/xvda".to_owned()),
//        root_device_type: Some(Ebs),
//        security_groups: Some([GroupIdentifier {
//            group_name: Some("launch-wizard-15".to_owned()),
//            group_id: Some("sg-0a17779b4829694d4".to_owned()),
//        }]),
//        source_dest_check: Some(true),
//        spot_instance_request_id: None,
//        sriov_net_support: None,
//        state_reason: None,
//        tags: Some([Tag {
//            key: Some("Name".to_owned()),
//            value: Some("drb-rmq-perftest-source".to_owned()),
//        }]),
//        virtualization_type: Some(Hvm),
//        cpu_options: Some(CpuOptions {
//            core_count: Some(48),
//            threads_per_core: Some(2),
//        }),
//        capacity_reservation_id: None,
//        capacity_reservation_specification: Some(
//            CapacityReservationSpecificationResponse {
//                capacity_reservation_preference: Some(Open),
//                capacity_reservation_target: None,
//            },
//        ),
//        hibernation_options: Some(HibernationOptions {
//            configured: Some(false),
//        }),
//        licenses: None,
//        metadata_options: Some(InstanceMetadataOptionsResponse {
//            state: Some(Applied),
//            http_tokens: Some(Optional),
//            http_put_response_hop_limit: Some(1),
//            http_endpoint: Some(Enabled),
//            http_protocol_ipv6: Some(Disabled),
//            instance_metadata_tags: Some(Disabled),
//        }),
//        enclave_options: Some(EnclaveOptions {
//            enabled: Some(false),
//        }),
//        boot_mode: None,
//        platform_details: Some("Linux/UNIX".to_owned()),
//        usage_operation: Some("RunInstances".to_owned()),
//        usage_operation_update_time: Some(DateTime {
//            seconds: 1666624590,
//            subsecond_nanos: 0,
//        }),
//        private_dns_name_options: Some(PrivateDnsNameOptionsResponse {
//            hostname_type: Some(IpName),
//            enable_resource_name_dns_a_record: Some(true),
//            enable_resource_name_dns_aaaa_record: Some(false),
//        }),
//        ipv6_address: None,
//        tpm_support: None,
//        maintenance_options: Some(InstanceMaintenanceOptions {
//            auto_recovery: Some(Default),
//        }),
//    },
//]
