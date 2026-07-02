use atlas_lib::Settings;
use atlas_lib::api::azure::models::*;
use atlas_lib::api::google::compute_network::{Network, Subnetwork};
use atlas_lib::api::google::gke::Cluster;
use atlas_lib::api::google::sql::{SqlInstance, SqlIpAddress};
use atlas_lib::api::google::storage::Bucket;
use atlas_lib::atlas::graph_builder::GraphBuilder;
use atlas_lib::atlas::projector;
use atlas_lib::cloud::cloudflare::worker::{WorkerBinding, WorkerScript};
use atlas_lib::cloud::definition::{
    AmazonCollection, CloudflareCollection, GoogleCollection, MicrosoftCollection, Provider,
};
use aws_sdk_ec2::types::GroupIdentifier;
use aws_sdk_ec2::types::builders::InstanceBuilder;
use aws_sdk_elasticloadbalancingv2::types::{
    Action, Listener, LoadBalancer, TargetDescription, TargetGroup, TargetHealthDescription,
};
use petgraph::dot::Dot;
use serde_json::json;
use std::collections::HashMap;
use std::fs;

fn make_aws_provider() -> Provider {
    // VPC and Security Groups
    let sg1 = GroupIdentifier::builder()
        .group_id("sg-web-tier")
        .group_name("web-tier-sg")
        .build();
    let _sg2 = GroupIdentifier::builder()
        .group_id("sg-lb-tier")
        .group_name("lb-tier-sg")
        .build();

    let mut instances = vec![];
    for i in 1..=3 {
        let instance = InstanceBuilder::default()
            .set_image_id(Some("ami-09d3b8424b6c5d4aa".to_owned()))
            .set_instance_id(Some(format!("i-web-node-0{}", i)))
            .set_vpc_id(Some("vpc-globalapp".to_owned()))
            .set_subnet_id(Some("subnet-public-1a".to_owned()))
            .set_private_ip_address(Some(format!("10.10.10.{}", i))) // Note: Our DB is 10.10.10.5
            .set_security_groups(Some(vec![sg1.clone()]))
            .build();
        instances.push(instance);
    }

    let lb = LoadBalancer::builder()
        .load_balancer_arn(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/alb-globalapp/50dc",
        )
        .vpc_id("vpc-globalapp")
        .build();

    let tg = TargetGroup::builder()
        .target_group_arn("arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/tg-web-tier/73e2")
        .vpc_id("vpc-globalapp")
        .build();

    let action = Action::builder()
        .target_group_arn("arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/tg-web-tier/73e2")
        .build();

    let listener = Listener::builder()
        .load_balancer_arn(
            "arn:aws:elasticloadbalancing:us-east-1:123:loadbalancer/app/alb-globalapp/50dc",
        )
        .default_actions(action)
        .build();

    let mut health_map = HashMap::new();
    let mut targets = vec![];
    for i in 1..=3 {
        let target = TargetDescription::builder()
            .id(format!("i-web-node-0{}", i))
            .build();
        let health = TargetHealthDescription::builder().target(target).build();
        targets.push(health);
    }
    health_map.insert(
        "arn:aws:elasticloadbalancing:us-east-1:123:targetgroup/tg-web-tier/73e2".to_owned(),
        targets,
    );

    // Let's add a Route53 record just for completion, mapping a legacy domain to ALB.
    let hosted_zone = aws_sdk_route53::types::HostedZone::builder()
        .id("/hostedzone/Z999999999")
        .name("legacy.globalapp.com.")
        .caller_reference("demo")
        .build()
        .unwrap();

    let record_1 = aws_sdk_route53::types::ResourceRecord::builder()
        .value("alb.aws.globalapp.com")
        .build()
        .unwrap();

    let record_set = aws_sdk_route53::types::ResourceRecordSet::builder()
        .name("legacy.globalapp.com.")
        .r#type(aws_sdk_route53::types::RrType::Cname)
        .resource_records(record_1)
        .build()
        .unwrap();

    Provider::AWS(vec![
        (
            "us-east-1".to_owned(),
            AmazonCollection::AmazonInstances(instances),
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

fn make_gcp_provider() -> Provider {
    // Our DB lives at 10.10.10.5
    let ip = SqlIpAddress {
        ip_type: Some("PRIMARY".to_string()),
        ip_address: Some("10.10.10.5".to_string()),
    };
    let sql = SqlInstance {
        name: Some("db-globalapp-master".to_string()),
        ip_addresses: Some(vec![ip]),
        ..Default::default()
    };

    let gke = Cluster {
        name: Some("gke-data-processing".to_string()),
        network: Some("projects/gcp-globalapp/global/networks/vpc-gcp".to_string()),
        ..Default::default()
    };

    let net = Network {
        self_link: Some(
            "https://www.googleapis.com/compute/v1/projects/gcp-globalapp/global/networks/vpc-gcp"
                .to_string(),
        ),
        ..Default::default()
    };

    let subnet = Subnetwork {
        self_link: Some("https://www.googleapis.com/compute/v1/projects/gcp-globalapp/regions/us-central1/subnetworks/subnet-data".to_string()),
        network: Some("https://www.googleapis.com/compute/v1/projects/gcp-globalapp/global/networks/vpc-gcp".to_string()),
        ..Default::default()
    };

    let bucket = Bucket {
        id: Some("gs://backup-globalapp-bucket".to_string()),
        name: Some("backup-globalapp-bucket".to_string()),
        ..Default::default()
    };

    Provider::GCP(vec![
        GoogleCollection::GoogleSql(vec![sql]),
        GoogleCollection::GoogleGke(vec![gke]),
        GoogleCollection::GoogleNetworks(vec![net]),
        GoogleCollection::GoogleSubnetworks(vec![subnet]),
        GoogleCollection::GoogleStorageBuckets(vec![bucket]),
    ])
}

fn make_azure_provider() -> Provider {
    let vm = VirtualMachine {
        id: Some("/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Compute/virtualMachines/vm-legacy-worker".to_string()),
        name: Some("vm-legacy-worker".to_string()),
        location: Some("eastus".to_string()),
        network_interfaces: vec!["/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Network/networkInterfaces/nic-worker".to_string()],
    };

    let vnet = VirtualNetwork {
        id: Some("/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Network/virtualNetworks/vnet-az".to_string()),
        name: Some("vnet-az".to_string()),
        location: Some("eastus".to_string()),
        subnets: vec!["/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Network/virtualNetworks/vnet-az/subnets/default".to_string()],
    };

    let subnet = Subnet {
        id: Some("/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Network/virtualNetworks/vnet-az/subnets/default".to_string()),
        name: Some("default".to_string()),
        vnet_id: Some("/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Network/virtualNetworks/vnet-az".to_string()),
        network_security_group_id: Some("/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Network/networkSecurityGroups/nsg-worker".to_string()),
    };

    let nsg = NetworkSecurityGroup {
        id: Some("/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Network/networkSecurityGroups/nsg-worker".to_string()),
        name: Some("nsg-worker".to_string()),
        location: Some("eastus".to_string()),
        properties: None,
    };

    let app_service = AppService {
        id: Some("/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.Web/sites/worker-globalapp".to_string()),
        name: Some("worker-globalapp".to_string()),
        location: Some("eastus".to_string()),
        properties: Some(AppServiceProperties {
            default_host_name: Some("worker.globalapp.com".to_string()),
        }),
    };

    let sbus = ServiceBus {
        id: Some("/subscriptions/sub-az/resourceGroups/rg-globalapp/providers/Microsoft.ServiceBus/namespaces/sb-globalapp".to_string()),
        name: Some("sb-globalapp".to_string()),
        location: Some("eastus".to_string()),
    };

    Provider::Azure(vec![
        MicrosoftCollection::AzureVirtualMachines(vec![vm]),
        MicrosoftCollection::AzureVirtualNetworks(vec![vnet]),
        MicrosoftCollection::AzureSubnets(vec![subnet]),
        MicrosoftCollection::AzureNetworkSecurityGroups(vec![nsg]),
        MicrosoftCollection::AzureAppServices(vec![app_service]),
        MicrosoftCollection::AzureServiceBuses(vec![sbus]),
    ])
}

fn make_cloudflare_provider() -> Provider {
    let zone_json = json!({
        "id": "zone-globalapp",
        "name": "globalapp.com",
        "account": { "id": "acc-1", "name": "Global App Org" },
        "activated_on": "2023-01-01T00:00:00Z",
        "created_on": "2023-01-01T00:00:00Z",
        "development_mode": 0,
        "original_name_servers": ["ns1"],
        "original_registrar": "reg",
        "original_dnshost": "host",
        "modified_on": "2023-01-01T00:00:00Z",
        "name_servers": ["ns1"],
        "owner": { "id": "owner1", "type": "user", "email": "admin@globalapp.com" },
        "permissions": ["#zone:read"],
        "plan": { "id": "plan1", "name": "enterprise", "price": 0, "currency": "USD", "frequency": "monthly", "is_subscribed": true, "can_subscribe": false, "legacy_id": "ent", "legacy_discount": false, "externally_managed": false },
        "plan_pending": { "id": "plan1", "name": "enterprise", "price": 0, "currency": "USD", "frequency": "monthly", "is_subscribed": true, "can_subscribe": false, "legacy_id": "ent", "legacy_discount": false, "externally_managed": false },
        "status": "active",
        "paused": false,
        "type": "full",
        "meta": {
            "custom_certificate_quota": 1,
            "page_rule_quota": 3,
            "phishing_detected": false
        }
    });

    let record_api = json!({
        "id": "rec-api",
        "name": "api.globalapp.com",
        "type": "CNAME",
        "content": "alb.aws.globalapp.com",
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

    let record_worker = json!({
        "id": "rec-worker",
        "name": "worker.globalapp.com",
        "type": "CNAME",
        "content": "worker-globalapp.azurewebsites.net",
        "proxied": false,
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

    let zone: cloudflare::endpoints::zones::zone::Zone = serde_json::from_value(zone_json).unwrap();
    let r_api: cloudflare::endpoints::dns::dns::DnsRecord =
        serde_json::from_value(record_api).unwrap();
    let r_worker: cloudflare::endpoints::dns::dns::DnsRecord =
        serde_json::from_value(record_worker).unwrap();

    let worker = WorkerScript {
        id: "cf-edge-router".to_string(),
        created_on: None,
        modified_on: None,
    };

    let worker_bindings = vec![(
        "cf-edge-router".to_string(),
        vec![WorkerBinding {
            name: "DB_CONNECTION".to_string(),
            binding_type: "secret".to_string(),
            namespace_id: None,
            bucket_name: None,
            id: None,
            extra: {
                let mut map = std::collections::HashMap::new();
                map.insert(
                    "text".to_string(),
                    serde_json::json!("postgres://10.10.10.5"),
                );
                map
            },
        }],
    )];

    Provider::Cloudflare(CloudflareCollection {
        zones: vec![zone],
        dns_records: vec![("zone-globalapp".to_string(), vec![r_api, r_worker])],
        workers: vec![worker],
        kv_namespaces: vec![],
        r2_buckets: vec![],
        durable_objects: vec![],
        d1_databases: vec![],
        worker_bindings,
    })
}

fn main() {
    println!("Generating Cloud Atlas Multi-Cloud Topology Demo Graph...");

    let s = Settings {
        regions: vec!["us-east-1".to_owned()],
        gcp_projects: Some(vec!["gcp-globalapp".to_owned()]),
        azure_subscriptions: Some(vec!["sub-az".to_owned()]),
        all: false,
        verbose: false,
        exclude_by_default: false,
        cloudflare: true,
    };

    let mut builder = GraphBuilder::new();

    // 1. Build AWS graph
    println!("Projecting AWS resources...");
    let aws_provider = make_aws_provider();
    projector::build(&mut builder, &aws_provider, &s);

    // 2. Build GCP graph
    println!("Projecting GCP resources...");
    let gcp_provider = make_gcp_provider();
    projector::build(&mut builder, &gcp_provider, &s);

    // 3. Build Azure graph
    println!("Projecting Azure resources...");
    let azure_provider = make_azure_provider();
    projector::build(&mut builder, &azure_provider, &s);

    // 4. Build Cloudflare graph
    println!("Projecting Cloudflare resources...");
    let cf_provider = make_cloudflare_provider();
    projector::build(&mut builder, &cf_provider, &s);

    // Let's inject a fake cross-cloud outbound route for the AWS EC2 instances pointing to the DB IP.
    // In reality, this edge would be discovered by analyzing VPC route tables or VPC flow logs.
    println!("Injecting cross-cloud semantic edges...");
    use atlas_lib::atlas::definition::{Edge, Node};
    let gcp_sql_ip = Node::GenericIpAddress("10.10.10.5".into());
    let ip_idx = builder.get_or_add_node(gcp_sql_ip);
    for i in 1..=3 {
        let aws_instance = Node::AwsEc2Instance(format!("i-web-node-0{}", i).into());
        let aws_instance_idx = builder.get_or_add_node(aws_instance);
        builder.add_edge(aws_instance_idx, ip_idx, Edge::RoutesTo);
    }

    // Export graph to DOT
    println!("Rendering graph...");
    let dot = format!("{:?}", Dot::with_config(&builder.graph, &[]));
    let filename = "multi_cloud_demo.dot";
    fs::write(filename, dot).expect("Failed to write dot file");

    println!(
        "Successfully generated graph with {} nodes and {} edges.",
        builder.graph.node_count(),
        builder.graph.edge_count()
    );
    println!("Saved to {}.", filename);
    println!("You can visualize this graph using Graphviz:");
    println!("  dot -Tsvg {} -o demo.svg", filename);
}
