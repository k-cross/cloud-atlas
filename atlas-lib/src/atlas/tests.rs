mod tests {
    use crate::atlas::projector;
    use crate::cloud::definition::{AmazonCollection, Provider};
    use crate::Settings;
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

        Provider::AWS(vec![(
            "us-east-1".to_owned(),
            AmazonCollection::AmazonInstances(vec![i1, i2]),
        )])
    }

    #[test]
    fn amazon_instance_graph() {
        let s = Settings {
            regions: vec!["us-east-1".to_owned()],
            all: false,
            verbose: false,
            exclude_by_default: false,
        };
        let provider = make_aws_provider();
        dbg!(&provider);
        let g = projector::build(&provider, &s);
        dbg!(g);
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
