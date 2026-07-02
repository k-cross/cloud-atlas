#[allow(clippy::module_inception)]
mod tests {
    use crate::Settings;
    use crate::atlas::graph_builder::GraphBuilder;
    use crate::atlas::projector;
    use crate::cloud::definition::{AmazonCollection, Provider};
    use aws_sdk_ec2::types::builders::InstanceBuilder;

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
        use petgraph::dot::Dot;
        use std::fs;

        let s = Settings {
            regions: vec!["us-east-1".to_owned()],
            gcp_projects: None,
            azure_subscriptions: None,
            all: false,
            verbose: false,
            exclude_by_default: false,
        };
        let provider = make_aws_provider();
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &provider, &s);

        let s = format!("{}", Dot::with_config(&builder.graph, &[]));
        fs::write("mock_atlas.dot", s).unwrap();

        println!("Mock DOT file generated at mock_atlas.dot");
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
            id: Some("12345".to_string()),
            name: Some("my-gcp-instance".to_owned()),
            self_link: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/zones/us-central1-a/instances/my-gcp-instance".to_owned()),
            ..Default::default()
        };

        let i2 = Instance {
            id: Some("67890".to_string()),
            name: Some("my-gcp-instance-2".to_owned()),
            self_link: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/zones/us-central1-a/instances/my-gcp-instance-2".to_owned()),
            ..Default::default()
        };

        let fw = Firewall {
            id: Some("fw-1".to_string()),
            name: Some("allow-ssh".to_string()),
            network: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/global/networks/default".to_string()),
            ..Default::default()
        };

        let ip = SqlIpAddress {
            ip_type: Some("PRIMARY".to_string()),
            ip_address: Some("10.0.0.5".to_string()),
        };
        let sql = SqlInstance {
            name: Some("my-sql-db".to_string()),
            ip_addresses: Some(vec![ip]),
            ..Default::default()
        };

        let dns = ManagedZone {
            name: Some("my-zone".to_string()),
            dns_name: Some("example.com.".to_string()),
            ..Default::default()
        };

        let gke = Cluster {
            name: Some("my-cluster".to_string()),
            network: Some("projects/my-gcp-project/global/networks/default".to_string()),
            ..Default::default()
        };

        let func = CloudFunction {
            name: Some(
                "projects/my-gcp-project/locations/us-central1/functions/my-func".to_string(),
            ),
            ..Default::default()
        };

        let bucket = Bucket {
            id: Some("my-bucket".to_string()),
            name: Some("my-bucket".to_string()),
            ..Default::default()
        };

        let topic = Topic {
            name: Some("projects/my-gcp-project/topics/my-topic".to_string()),
        };

        let sub = Subscription {
            name: Some("projects/my-gcp-project/subscriptions/my-sub".to_string()),
            topic: Some("projects/my-gcp-project/topics/my-topic".to_string()),
        };

        let run_svc = Service {
            name: Some("projects/my-gcp-project/locations/us-central1/services/my-svc".to_string()),
            ..Default::default()
        };

        let net = Network {
            self_link: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/global/networks/default".to_string()),
            ..Default::default()
        };

        let subnet = Subnetwork {
            self_link: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/regions/us-central1/subnetworks/default".to_string()),
            network: Some("https://www.googleapis.com/compute/v1/projects/my-gcp-project/global/networks/default".to_string()),
            ..Default::default()
        };

        let fw_rule = ForwardingRule {
            id: Some("fw-rule-1".to_string()),
            ip_address: Some("34.120.0.1".to_string()),
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
        use petgraph::dot::Dot;
        use std::fs;

        let s = Settings {
            regions: vec![],
            gcp_projects: Some(vec!["my-gcp-project".to_owned()]),
            azure_subscriptions: None,
            all: false,
            verbose: false,
            exclude_by_default: false,
        };
        let provider = make_gcp_provider();
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &provider, &s);

        let s = format!("{}", Dot::with_config(&builder.graph, &[]));
        assert!(s.contains("GCP::Compute::Instance"));
        assert!(s.contains("GCP::Compute::Firewall"));
        assert!(s.contains("GCP::SQL::Instance"));
        assert!(s.contains("GCP::DNS::ManagedZone"));
        assert!(s.contains("GCP::GKE::Cluster"));
        assert!(s.contains("GCP::CloudFunctions::Function"));
        assert!(s.contains("GCP::Storage::Bucket"));
        assert!(s.contains("GCP::PubSub::Topic"));
        assert!(s.contains("GCP::PubSub::Subscription"));
        assert!(s.contains("GCP::CloudRun::Service"));
        assert!(s.contains("GCP::Compute::Network"));
        assert!(s.contains("GCP::Compute::Subnetwork"));
        assert!(s.contains("GCP::Compute::ForwardingRule"));
        assert!(s.contains("my-gcp-project"));
        fs::write("mock_atlas_gcp.dot", s).unwrap();
    }
    fn make_azure_provider() -> Provider {
        use crate::api::azure::models::*;
        use crate::cloud::definition::MicrosoftCollection;

        let vm = VirtualMachine {
            id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Compute/virtualMachines/vm1".to_string()),
            name: Some("vm1".to_string()),
            location: Some("eastus".to_string()),
            network_interfaces: vec!["/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/networkInterfaces/nic1".to_string()],
        };

        let vnet = VirtualNetwork {
            id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1".to_string()),
            name: Some("vnet1".to_string()),
            location: Some("eastus".to_string()),
            subnets: vec!["/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1/subnets/default".to_string()],
        };

        let subnet = Subnet {
            id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1/subnets/default".to_string()),
            name: Some("default".to_string()),
            vnet_id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/virtualNetworks/vnet1".to_string()),
            network_security_group_id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/networkSecurityGroups/nsg1".to_string()),
        };

        let nsg = NetworkSecurityGroup {
            id: Some("/subscriptions/sub1/resourceGroups/rg1/providers/Microsoft.Network/networkSecurityGroups/nsg1".to_string()),
            name: Some("nsg1".to_string()),
            location: Some("eastus".to_string()),
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
        use petgraph::dot::Dot;
        use std::fs;

        let s = Settings {
            regions: vec![],
            gcp_projects: None,
            azure_subscriptions: Some(vec!["sub1".to_owned()]),
            all: false,
            verbose: false,
            exclude_by_default: false,
        };
        let provider = make_azure_provider();
        let mut builder = GraphBuilder::new();
        projector::build(&mut builder, &provider, &s);

        let s = format!("{}", Dot::with_config(&builder.graph, &[]));
        assert!(s.contains("Azure::Compute::VirtualMachine"));
        assert!(s.contains("Azure::Network::VirtualNetwork"));
        assert!(s.contains("Azure::Network::Subnet"));
        assert!(s.contains("Azure::Network::NetworkSecurityGroup"));
        fs::write("mock_atlas_azure.dot", s).unwrap();
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
