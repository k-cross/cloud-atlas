mod tests {
    use crate::atlas::projector;
    use crate::cloud::{Provider, AmazonCollection};
    use aws_sdk_ec2::model::*;

    fn make_aws_provider() -> Vec<Instance> {
      Provider {
        AWS(vec![AmazonCollection::AmazonInstances(
          vec![
            Instance {
                ami_launch_index: Some(0),
                image_id: Some("ami-09d3b8424b6c5d4aa"),
                instance_id: Some("i-01ee77706a905ce9627"),
                instance_type: Some(T2Micro),
                kernel_id: None,
                key_name: Some("a-key"),
                launch_time: Some(DateTime {
                    seconds: 1666371279,
                    subsecond_nanos: 0,
                }),
                monitoring: Some(Monitoring {
                    state: Some(Disabled),
                }),
                placement: Some(Placement {
                    availability_zone: Some("us-east-1d"),
                    affinity: None,
                    group_name: Some(""),
                    partition_number: None,
                    host_id: None,
                    tenancy: Some(Default),
                    spread_domain: None,
                    host_resource_group_arn: None,
                }),
                platform: None,
                private_dns_name: Some("ip-178-31-61-178.ec2.internal"),
                private_ip_address: Some("178.31.62.185"),
                product_codes: Some([]),
                public_dns_name: Some("ec2-5-95-160-24.compute-1.amazonaws.com"),
                public_ip_address: Some("5.96.160.24"),
                ramdisk_id: None,
                state: Some(InstanceState {
                    code: Some(16),
                    name: Some(Running),
                }),
                state_transition_reason: Some(""),
                subnet_id: Some("subnet-dac7a6f7"),
                vpc_id: Some("vpc-f98d2f9f"),
                architecture: Some(X8664),
                block_device_mappings: Some([InstanceBlockDeviceMapping {
                    device_name: Some("/dev/xvda"),
                    ebs: Some(EbsInstanceBlockDevice {
                        attach_time: Some(DateTime {
                            seconds: 1666371280,
                            subsecond_nanos: 0,
                        }),
                        delete_on_termination: Some(true),
                        status: Some(Attached),
                        volume_id: Some("vol-0d3693f48963f8821"),
                    }),
                }]),
                client_token: Some(""),
                ebs_optimized: Some(false),
                ena_support: Some(true),
                hypervisor: Some(Xen),
                iam_instance_profile: None,
                instance_lifecycle: None,
                elastic_gpu_associations: None,
                elastic_inference_accelerator_associations: None,
                network_interfaces: Some([InstanceNetworkInterface {
                    association: Some(InstanceNetworkInterfaceAssociation {
                        carrier_ip: None,
                        customer_owned_ip: None,
                        ip_owner_id: Some("amazon"),
                        public_dns_name: Some("ec2-5-95-160-24.compute-1.amazonaws.com"),
                        public_ip: Some("5.95.160.24"),
                    }),
                    attachment: Some(InstanceNetworkInterfaceAttachment {
                        attach_time: Some(DateTime {
                            seconds: 1666371279,
                            subsecond_nanos: 0,
                        }),
                        attachment_id: Some("eni-attach-07671ca1b6be32131"),
                        delete_on_termination: Some(true),
                        device_index: Some(0),
                        status: Some(Attached),
                        network_card_index: Some(0),
                    }),
                    description: Some(""),
                    groups: Some([
                        GroupIdentifier {
                            group_name: Some("ec2-rds-5"),
                            group_id: Some("sg-0e7924cea43567852"),
                        },
                        GroupIdentifier {
                            group_name: Some("launch-wizard-6-sec"),
                            group_id: Some("sg-04c42260980ef15e5"),
                        },
                    ]),
                    ipv6_addresses: Some([]),
                    mac_address: Some("12:74:d4:da:ff:b7"),
                    network_interface_id: Some("eni-0b96c4be1eb530476"),
                    owner_id: Some("781865768738"),
                    private_dns_name: Some("ip-178-31-61-178.ec2.internal"),
                    private_ip_address: Some("178.31.62.185"),
                    private_ip_addresses: Some([InstancePrivateIpAddress {
                        association: Some(InstanceNetworkInterfaceAssociation {
                            carrier_ip: None,
                            customer_owned_ip: None,
                            ip_owner_id: Some("amazon"),
                            public_dns_name: Some("ec2-5-95-160-24.compute-1.amazonaws.com"),
                            public_ip: Some("5.95.160.24"),
                        }),
                        primary: Some(true),
                        private_dns_name: Some("ip-178-31-61-178.ec2.internal"),
                        private_ip_address: Some("178.31.62.185"),
                    }]),
                    source_dest_check: Some(true),
                    status: Some(InUse),
                    subnet_id: Some("subnet-dac7a6f7"),
                    vpc_id: Some("vpc-f98d2f9f"),
                    interface_type: Some("interface"),
                    ipv4_prefixes: None,
                    ipv6_prefixes: None,
                }]),
                outpost_arn: None,
                root_device_name: Some("/dev/xvda"),
                root_device_type: Some(Ebs),
                security_groups: Some([
                    GroupIdentifier {
                        group_name: Some("ec2-rds-5"),
                        group_id: Some("sg-0e7924cea43567852"),
                    },
                    GroupIdentifier {
                        group_name: Some("launch-wizard-6-sec2"),
                        group_id: Some("sg-04c42260980ef15e5"),
                    },
                ]),
                source_dest_check: Some(true),
                spot_instance_request_id: None,
                sriov_net_support: None,
                state_reason: None,
                tags: Some([Tag {
                    key: Some("Name"),
                    value: Some("sec2"),
                }]),
                virtualization_type: Some(Hvm),
                cpu_options: Some(CpuOptions {
                    core_count: Some(1),
                    threads_per_core: Some(1),
                }),
                capacity_reservation_id: None,
                capacity_reservation_specification: Some(
                    CapacityReservationSpecificationResponse {
                        capacity_reservation_preference: Some(Open),
                        capacity_reservation_target: None,
                    },
                ),
                hibernation_options: Some(HibernationOptions {
                    configured: Some(false),
                }),
                licenses: None,
                metadata_options: Some(InstanceMetadataOptionsResponse {
                    state: Some(Applied),
                    http_tokens: Some(Optional),
                    http_put_response_hop_limit: Some(1),
                    http_endpoint: Some(Enabled),
                    http_protocol_ipv6: Some(Disabled),
                    instance_metadata_tags: Some(Disabled),
                }),
                enclave_options: Some(EnclaveOptions {
                    enabled: Some(false),
                }),
                boot_mode: None,
                platform_details: Some("Linux/UNIX"),
                usage_operation: Some("RunInstances"),
                usage_operation_update_time: Some(DateTime {
                    seconds: 1666371279,
                    subsecond_nanos: 0,
                }),
                private_dns_name_options: Some(PrivateDnsNameOptionsResponse {
                    hostname_type: Some(IpName),
                    enable_resource_name_dns_a_record: Some(true),
                    enable_resource_name_dns_aaaa_record: Some(false),
                }),
                ipv6_address: None,
                tpm_support: None,
                maintenance_options: Some(InstanceMaintenanceOptions {
                    auto_recovery: Some(Default),
                }),
            },
            Instance {
                ami_launch_index: Some(0),
                image_id: Some("ami-09d3b3274b6c5d4aa"),
                instance_id: Some("i-0385e6e2f59529b68"),
                instance_type: Some(C524xlarge),
                kernel_id: None,
                key_name: Some("drb-rmq-perftest-kp"),
                launch_time: Some(DateTime {
                    seconds: 1666624590,
                    subsecond_nanos: 0,
                }),
                monitoring: Some(Monitoring {
                    state: Some(Disabled),
                }),
                placement: Some(Placement {
                    availability_zone: Some("us-east-1a"),
                    affinity: None,
                    group_name: Some(""),
                    partition_number: None,
                    host_id: None,
                    tenancy: Some(Default),
                    spread_domain: None,
                    host_resource_group_arn: None,
                }),
                platform: None,
                private_dns_name: Some("ip-178-31-61-178.ec2.internal"),
                private_ip_address: Some("178.31.11.76"),
                product_codes: Some([]),
                public_dns_name: Some("ec2-99-163-27-126.compute-1.amazonaws.com"),
                public_ip_address: Some("5.163.27.126"),
                ramdisk_id: None,
                state: Some(InstanceState {
                    code: Some(16),
                    name: Some(Running),
                }),
                state_transition_reason: Some(""),
                subnet_id: Some("subnet-089bca41"),
                vpc_id: Some("vpc-f98d2f9f"),
                architecture: Some(X8664),
                block_device_mappings: Some([InstanceBlockDeviceMapping {
                    device_name: Some("/dev/xvda"),
                    ebs: Some(EbsInstanceBlockDevice {
                        attach_time: Some(DateTime {
                            seconds: 1666624592,
                            subsecond_nanos: 0,
                        }),
                        delete_on_termination: Some(true),
                        status: Some(Attached),
                        volume_id: Some("vol-0c95677277803b019"),
                    }),
                }]),
                client_token: Some(""),
                ebs_optimized: Some(true),
                ena_support: Some(true),
                hypervisor: Some(Xen),
                iam_instance_profile: None,
                instance_lifecycle: None,
                elastic_gpu_associations: None,
                elastic_inference_accelerator_associations: None,
                network_interfaces: Some([InstanceNetworkInterface {
                    association: Some(InstanceNetworkInterfaceAssociation {
                        carrier_ip: None,
                        customer_owned_ip: None,
                        ip_owner_id: Some("amazon"),
                        public_dns_name: Some("ec2-99-163-27-126.compute-1.amazonaws.com"),
                        public_ip: Some("5.163.27.126"),
                    }),
                    attachment: Some(InstanceNetworkInterfaceAttachment {
                        attach_time: Some(DateTime {
                            seconds: 1666624590,
                            subsecond_nanos: 0,
                        }),
                        attachment_id: Some("eni-attach-00e1c7de777819760"),
                        delete_on_termination: Some(true),
                        device_index: Some(0),
                        status: Some(Attached),
                        network_card_index: Some(0),
                    }),
                    description: Some(""),
                    groups: Some([GroupIdentifier {
                        group_name: Some("launch-wizard-15"),
                        group_id: Some("sg-0a1a4d9b4877794d4"),
                    }]),
                    ipv6_addresses: Some([]),
                    mac_address: Some("0a:1e:92:34:e9:93"),
                    network_interface_id: Some("eni-034777fa5aaba2fd3"),
                    owner_id: Some("781865768738"),
                    private_dns_name: Some("ip-178-31-61-178.ec2.internal"),
                    private_ip_address: Some("178.31.11.76"),
                    private_ip_addresses: Some([InstancePrivateIpAddress {
                        association: Some(InstanceNetworkInterfaceAssociation {
                            carrier_ip: None,
                            customer_owned_ip: None,
                            ip_owner_id: Some("amazon"),
                            public_dns_name: Some("ec2-99-163-27-126.compute-1.amazonaws.com"),
                            public_ip: Some("5.163.27.126"),
                        }),
                        primary: Some(true),
                        private_dns_name: Some("ip-178-31-61-178.ec2.internal"),
                        private_ip_address: Some("178.31.11.76"),
                    }]),
                    source_dest_check: Some(true),
                    status: Some(InUse),
                    subnet_id: Some("subnet-087abc41"),
                    vpc_id: Some("vpc-f9777f9f"),
                    interface_type: Some("interface"),
                    ipv4_prefixes: None,
                    ipv6_prefixes: None,
                }]),
                outpost_arn: None,
                root_device_name: Some("/dev/xvda"),
                root_device_type: Some(Ebs),
                security_groups: Some([GroupIdentifier {
                    group_name: Some("launch-wizard-15"),
                    group_id: Some("sg-0a17779b4829694d4"),
                }]),
                source_dest_check: Some(true),
                spot_instance_request_id: None,
                sriov_net_support: None,
                state_reason: None,
                tags: Some([Tag {
                    key: Some("Name"),
                    value: Some("drb-rmq-perftest-source"),
                }]),
                virtualization_type: Some(Hvm),
                cpu_options: Some(CpuOptions {
                    core_count: Some(48),
                    threads_per_core: Some(2),
                }),
                capacity_reservation_id: None,
                capacity_reservation_specification: Some(
                    CapacityReservationSpecificationResponse {
                        capacity_reservation_preference: Some(Open),
                        capacity_reservation_target: None,
                    },
                ),
                hibernation_options: Some(HibernationOptions {
                    configured: Some(false),
                }),
                licenses: None,
                metadata_options: Some(InstanceMetadataOptionsResponse {
                    state: Some(Applied),
                    http_tokens: Some(Optional),
                    http_put_response_hop_limit: Some(1),
                    http_endpoint: Some(Enabled),
                    http_protocol_ipv6: Some(Disabled),
                    instance_metadata_tags: Some(Disabled),
                }),
                enclave_options: Some(EnclaveOptions {
                    enabled: Some(false),
                }),
                boot_mode: None,
                platform_details: Some("Linux/UNIX"),
                usage_operation: Some("RunInstances"),
                usage_operation_update_time: Some(DateTime {
                    seconds: 1666624590,
                    subsecond_nanos: 0,
                }),
                private_dns_name_options: Some(PrivateDnsNameOptionsResponse {
                    hostname_type: Some(IpName),
                    enable_resource_name_dns_a_record: Some(true),
                    enable_resource_name_dns_aaaa_record: Some(false),
                }),
                ipv6_address: None,
                tpm_support: None,
                maintenance_options: Some(InstanceMaintenanceOptions {
                    auto_recovery: Some(Default),
                }),
            },
        ])])
      }
    }

    #[test]
    fn amazon_instance_graph() {
        let provider = make_aws_provider();
        let g = projector::build(provider, "us-east-1".to_owned());
        dbg!(g);
    }
}
