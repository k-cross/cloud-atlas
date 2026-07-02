use crate::Settings;
use crate::atlas::definition::{Edge, Node};
use crate::atlas::graph_builder::GraphBuilder;
use crate::cloud::definition::{
    AmazonCollection, GoogleCollection, MicrosoftCollection, Provider as CloudProvider,
};

pub fn build(builder: &mut GraphBuilder, data: &CloudProvider, opts: &Settings) {
    match data {
        CloudProvider::AWS(aws_data) => aws_projector(builder, aws_data, opts),
        CloudProvider::GCP(gcp_data) => gcp_projector(builder, gcp_data),
        CloudProvider::Azure(azure_data) => azure_projector(builder, azure_data),
    }
}

pub fn aws_projector(
    builder: &mut GraphBuilder,
    aws_data: &Vec<(String, AmazonCollection)>,
    opts: &Settings,
) {
    for (region, x) in aws_data {
        let region_node = Node::AwsRegion(region.to_string());
        let region_idx = builder.get_or_add_node(region_node);

        match x {
            AmazonCollection::AmazonInstances(instance_data) => {
                for inst in instance_data {
                    let mut vpc_idx = None;
                    if let Some(vpc_id) = inst.vpc_id.as_ref() {
                        let node = Node::AwsEc2Vpc(vpc_id.to_string());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::Contains);
                        vpc_idx = Some(idx);
                    }

                    let mut subnet_idx = None;
                    if let Some(subnet_id) = inst.subnet_id.as_ref() {
                        let node = Node::AwsEc2Subnet(subnet_id.to_string());
                        let idx = builder.get_or_add_node(node);
                        if let Some(v_idx) = vpc_idx {
                            builder.add_edge(v_idx, idx, Edge::Contains);
                        }
                        subnet_idx = Some(idx);
                    }

                    let mut inst_idx = None;
                    if let Some(instance_id) = inst.instance_id.as_ref() {
                        let node = Node::AwsEc2Instance(instance_id.to_string());
                        let idx = builder.get_or_add_node(node);
                        inst_idx = Some(idx);

                        // Pivot: Instance -> HasIp -> ENI -> AttachedTo -> Subnet
                        if let Some(s_idx) = subnet_idx {
                            let eni_node = Node::AwsEc2Eni(format!("{}-eni", instance_id));
                            let eni_idx = builder.get_or_add_node(eni_node);

                            // Instance -> HasIp -> ENI
                            builder.add_edge(idx, eni_idx, Edge::HasIp);
                            // ENI -> AttachedTo -> Subnet
                            builder.add_edge(eni_idx, s_idx, Edge::AttachedTo);
                        }
                    }

                    if let Some(place) = inst.placement.as_ref()
                        && let Some(az_name) = place.availability_zone.as_ref()
                    {
                        let node = Node::AwsEc2AvailabilityZone(az_name.to_string());
                        let az_idx = builder.get_or_add_node(node);

                        if let Some(i_idx) = inst_idx {
                            builder.add_edge(az_idx, i_idx, Edge::Contains);
                        }
                    }

                    if let Some(private_ip) = inst.private_ip_address.as_ref() {
                        let node = Node::GenericIpAddress(private_ip.to_string());
                        let ip_idx = builder.get_or_add_node(node);
                        if let Some(i_idx) = inst_idx {
                            builder.add_edge(i_idx, ip_idx, Edge::ConnectsTo);
                        }
                    }

                    if let Some(tags) = inst.tags.as_ref() {
                        for tag in tags {
                            if let (Some(k), Some(v)) = (tag.key.as_ref(), tag.value.as_ref()) {
                                let node = Node::AwsTag(format!("{}={}", k, v));
                                let tag_idx = builder.get_or_add_node(node);
                                if let Some(i_idx) = inst_idx {
                                    builder.add_edge(i_idx, tag_idx, Edge::DependsOn);
                                }
                            }
                        }
                    }

                    for sg in inst.security_groups() {
                        if let Some(sg_id) = sg.group_id() {
                            let sg_node = Node::AwsEc2SecurityGroup(sg_id.to_string());
                            let sg_idx = builder.get_or_add_node(sg_node);
                            if let Some(i_idx) = inst_idx {
                                builder.add_edge(i_idx, sg_idx, Edge::ConnectsTo);
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonResources(resource_map) => {
                for (res_name, rs) in resource_map {
                    if use_aws_resource(res_name.as_str(), opts.exclude_by_default) {
                        for r in rs {
                            if let Some(id) = r.resource_id() {
                                let node = Node::AwsConfigResource {
                                    resource_type: res_name.to_string(),
                                    id: id.to_string(),
                                };
                                let idx = builder.get_or_add_node(node);

                                if use_global(res_name.as_str()) {
                                    let global_node = Node::AwsRegion("global".to_string());
                                    let g_idx = builder.get_or_add_node(global_node);
                                    builder.add_edge(g_idx, idx, Edge::DependsOn);
                                } else {
                                    builder.add_edge(region_idx, idx, Edge::DependsOn);
                                }
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonClusters(clusters) => {
                for cluster in clusters {
                    if let Some(arn) = cluster.cluster_arn() {
                        let node = Node::AwsEcsCluster(arn.to_string());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::DependsOn);
                    }
                }
            }
            AmazonCollection::AmazonLambdas(lambdas) => {
                for lambda in lambdas {
                    if let Some(name) = lambda.function_name() {
                        let node = Node::AwsLambdaFunction(name.to_string());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::DependsOn);

                        if let Some(role) = lambda.role() {
                            let role_node = Node::AwsIamRole(role.to_string());
                            let r_idx = builder.get_or_add_node(role_node);
                            builder.add_edge(idx, r_idx, Edge::DependsOn);
                        }

                        if let Some(vpc_config) = lambda.vpc_config() {
                            for sg_id in vpc_config.security_group_ids() {
                                let sg_node = Node::AwsEc2SecurityGroup(sg_id.to_string());
                                let sg_idx = builder.get_or_add_node(sg_node);
                                builder.add_edge(idx, sg_idx, Edge::ConnectsTo);
                            }
                        }
                    }
                }

                if opts.verbose {
                    dbg!(&lambdas);
                }
            }
            AmazonCollection::AmazonEventbridge(buses) => {
                if opts.verbose {
                    dbg!(&buses);
                }
            }
            AmazonCollection::AmazonLoadBalancers {
                load_balancers,
                target_groups,
                listeners,
                target_health,
            } => {
                for lb in load_balancers {
                    if let Some(arn) = lb.load_balancer_arn() {
                        let lb_node = Node::AwsElbLoadBalancer(arn.to_string());
                        let lb_idx = builder.get_or_add_node(lb_node);

                        if let Some(vpc_id) = lb.vpc_id() {
                            let vpc_node = Node::AwsEc2Vpc(vpc_id.to_string());
                            let v_idx = builder.get_or_add_node(vpc_node);
                            builder.add_edge(v_idx, lb_idx, Edge::Contains);
                        } else {
                            builder.add_edge(region_idx, lb_idx, Edge::DependsOn);
                        }
                    }
                }

                for tg in target_groups {
                    if let Some(arn) = tg.target_group_arn() {
                        let tg_node = Node::AwsElbTargetGroup(arn.to_string());
                        let tg_idx = builder.get_or_add_node(tg_node);

                        if let Some(vpc_id) = tg.vpc_id() {
                            let vpc_node = Node::AwsEc2Vpc(vpc_id.to_string());
                            let v_idx = builder.get_or_add_node(vpc_node);
                            builder.add_edge(v_idx, tg_idx, Edge::Contains);
                        }

                        if let Some(health_descriptions) = target_health.get(arn) {
                            for target_id in health_descriptions
                                .iter()
                                .filter_map(|h| h.target())
                                .filter_map(|t| t.id())
                            {
                                let inst_node = Node::AwsEc2Instance(target_id.to_string());
                                let i_idx = builder.get_or_add_node(inst_node);
                                builder.add_edge(tg_idx, i_idx, Edge::ConnectsTo);
                            }
                        }
                    }
                }

                for listener in listeners {
                    if let Some(lb_arn) = listener.load_balancer_arn() {
                        for tg_arn in listener
                            .default_actions()
                            .iter()
                            .filter_map(|a| a.target_group_arn())
                        {
                            let lb_node = Node::AwsElbLoadBalancer(lb_arn.to_string());
                            let tg_node = Node::AwsElbTargetGroup(tg_arn.to_string());
                            let lb_idx = builder.get_or_add_node(lb_node);
                            let tg_idx = builder.get_or_add_node(tg_node);
                            builder.add_edge(lb_idx, tg_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            AmazonCollection::AmazonRoute53 {
                hosted_zones,
                record_sets,
            } => {
                let global_node = Node::AwsRegion("global".to_string());
                let g_idx = builder.get_or_add_node(global_node);

                for hz in hosted_zones {
                    let id = hz.id();
                    let hz_node = Node::AwsRoute53HostedZone(id.to_string());
                    let hz_idx = builder.get_or_add_node(hz_node);
                    builder.add_edge(g_idx, hz_idx, Edge::Contains);
                }

                for rs in record_sets {
                    let name = rs.name();
                    let rs_node = Node::AwsRoute53RecordSet(name.to_string());
                    let rs_idx = builder.get_or_add_node(rs_node);

                    builder.add_edge(g_idx, rs_idx, Edge::Contains);

                    let records = rs.resource_records();
                    for r in records {
                        let val = r.value();
                        let ip_node = Node::GenericIpAddress(val.to_string());
                        let ip_idx = builder.get_or_add_node(ip_node);
                        builder.add_edge(rs_idx, ip_idx, Edge::ConnectsTo);
                    }
                }
            }
            AmazonCollection::AmazonEks(clusters) => {
                for cluster in clusters {
                    if let Some(name) = cluster.name() {
                        let node = Node::AwsEksCluster(name.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(vpc_config) = cluster.resources_vpc_config() {
                            if let Some(vpc_id) = vpc_config.vpc_id() {
                                let vpc_node = Node::AwsEc2Vpc(vpc_id.to_string());
                                let vpc_idx = builder.get_or_add_node(vpc_node);
                                builder.add_edge(vpc_idx, idx, Edge::Contains);
                            } else {
                                builder.add_edge(region_idx, idx, Edge::DependsOn);
                            }

                            for sg_id in vpc_config.security_group_ids() {
                                let sg_node = Node::AwsEc2SecurityGroup(sg_id.to_string());
                                let sg_idx = builder.get_or_add_node(sg_node);
                                builder.add_edge(idx, sg_idx, Edge::ConnectsTo);
                            }
                        } else {
                            builder.add_edge(region_idx, idx, Edge::DependsOn);
                        }
                    }
                }
            }
            AmazonCollection::AmazonApiGateway(apis) => {
                for api in apis {
                    if let Some(id) = api.id() {
                        let node = Node::AwsApiGatewayRestApi(id.to_string());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::DependsOn);
                    }
                }
            }
            AmazonCollection::AmazonRds(dbs) => {
                for db in dbs {
                    if let Some(id) = db.db_instance_identifier() {
                        let node = Node::AwsRdsDbInstance(id.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(subnet_group) = db.db_subnet_group() {
                            if let Some(vpc_id) = subnet_group.vpc_id() {
                                let vpc_node = Node::AwsEc2Vpc(vpc_id.to_string());
                                let vpc_idx = builder.get_or_add_node(vpc_node);
                                builder.add_edge(vpc_idx, idx, Edge::Contains);
                            } else {
                                builder.add_edge(region_idx, idx, Edge::DependsOn);
                            }
                        } else {
                            builder.add_edge(region_idx, idx, Edge::DependsOn);
                        }

                        for sg in db.vpc_security_groups() {
                            if let Some(sg_id) = sg.vpc_security_group_id() {
                                let sg_node = Node::AwsEc2SecurityGroup(sg_id.to_string());
                                let sg_idx = builder.get_or_add_node(sg_node);
                                builder.add_edge(idx, sg_idx, Edge::ConnectsTo);
                            }
                        }
                    }
                }
            }
            AmazonCollection::AmazonDynamoDb(tables) => {
                for t in tables {
                    let node = Node::AwsDynamoDbTable(t.to_string());
                    let idx = builder.get_or_add_node(node);
                    builder.add_edge(region_idx, idx, Edge::DependsOn);
                }
            }
            AmazonCollection::AmazonSqs(queues) => {
                for q in queues {
                    let node = Node::AwsSqsQueue(q.to_string());
                    let idx = builder.get_or_add_node(node);
                    builder.add_edge(region_idx, idx, Edge::DependsOn);
                }
            }
            AmazonCollection::AmazonSns(topics) => {
                for t in topics {
                    if let Some(arn) = t.topic_arn() {
                        let node = Node::AwsSnsTopic(arn.to_string());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::DependsOn);
                    }
                }
            }
            AmazonCollection::AmazonCloudFront(dists) => {
                let global_node = Node::AwsRegion("global".to_string());
                let g_idx = builder.get_or_add_node(global_node);

                for d in dists {
                    let id = d.id();
                    let node = Node::AwsCloudFrontDistribution(id.to_string());
                    let idx = builder.get_or_add_node(node);
                    builder.add_edge(g_idx, idx, Edge::Contains);
                }
            }
            AmazonCollection::AmazonSecurityGroups(groups) => {
                for sg in groups {
                    if let Some(id) = sg.group_id() {
                        let node = Node::AwsEc2SecurityGroup(id.to_string());
                        let idx = builder.get_or_add_node(node);
                        builder.add_edge(region_idx, idx, Edge::DependsOn);

                        for perm in sg.ip_permissions() {
                            for pair in perm.user_id_group_pairs() {
                                if let Some(referenced_group_id) = pair.group_id() {
                                    let ref_node =
                                        Node::AwsEc2SecurityGroup(referenced_group_id.to_string());
                                    let ref_idx = builder.get_or_add_node(ref_node);
                                    // The referenced group allows traffic TO this group
                                    builder.add_edge(ref_idx, idx, Edge::ConnectsTo);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn gcp_projector(builder: &mut GraphBuilder, gcp_data: &[GoogleCollection]) {
    for x in gcp_data {
        match x {
            GoogleCollection::GoogleInstances(instances) => {
                for inst in instances {
                    let mut project_idx = None;
                    // Attempt to extract project from the self_link e.g., https://www.googleapis.com/compute/v1/projects/my-project/zones/us-central1-a/instances/my-instance
                    if let Some(self_link) = &inst.self_link
                        && let Some(project_str) = self_link.split("/projects/").nth(1)
                        && let Some(project_id) = project_str.split('/').next()
                    {
                        let project_node = Node::GcpProject(project_id.to_string());
                        project_idx = Some(builder.get_or_add_node(project_node));
                    }

                    if let Some(id) = &inst.id {
                        let node = Node::GcpComputeInstance(id.to_string());
                        let idx = builder.get_or_add_node(node);
                        if let Some(p_idx) = project_idx {
                            builder.add_edge(p_idx, idx, Edge::DependsOn);
                        }

                        if let Some(network_interfaces) = &inst.network_interfaces {
                            for net in network_interfaces {
                                if let Some(network) = &net.network {
                                    let net_node = Node::GcpComputeNetwork(network.to_string());
                                    let n_idx = builder.get_or_add_node(net_node);
                                    builder.add_edge(n_idx, idx, Edge::Contains);
                                }
                            }
                        }
                    }
                }
            }
            GoogleCollection::GoogleFirewalls(firewalls) => {
                for fw in firewalls {
                    if let Some(id) = &fw.id {
                        let node = Node::GcpComputeFirewall(id.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(network) = &fw.network {
                            let net_node = Node::GcpComputeNetwork(network.to_string());
                            let n_idx = builder.get_or_add_node(net_node);
                            builder.add_edge(n_idx, idx, Edge::Contains);
                        }
                    }
                }
            }
            GoogleCollection::GoogleSql(instances) => {
                for sql in instances {
                    if let Some(name) = &sql.name {
                        let node = Node::GcpSqlInstance(name.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(ips) = &sql.ip_addresses {
                            for ip in ips {
                                if let Some(ip_addr) = &ip.ip_address {
                                    let ip_node = Node::GenericIpAddress(ip_addr.to_string());
                                    let ip_idx = builder.get_or_add_node(ip_node);
                                    builder.add_edge(idx, ip_idx, Edge::ConnectsTo);
                                }
                            }
                        }
                    }
                }
            }
            GoogleCollection::GoogleDns(zones) => {
                for zone in zones {
                    if let Some(name) = &zone.name {
                        let node = Node::GcpDnsManagedZone(name.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GoogleGke(clusters) => {
                for cluster in clusters {
                    if let Some(name) = &cluster.name {
                        let node = Node::GcpGkeCluster(name.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(network) = &cluster.network {
                            let net_node = Node::GcpComputeNetwork(network.to_string());
                            let n_idx = builder.get_or_add_node(net_node);
                            builder.add_edge(n_idx, idx, Edge::Contains);
                        }
                    }
                }
            }
            GoogleCollection::GoogleFunctions(functions) => {
                for func in functions {
                    if let Some(name) = &func.name {
                        let node = Node::GcpCloudFunction(name.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GoogleStorageBuckets(buckets) => {
                for bucket in buckets {
                    if let Some(id) = &bucket.id {
                        let node = Node::GcpStorageBucket(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GooglePubSubTopics(topics) => {
                for topic in topics {
                    if let Some(name) = &topic.name {
                        let node = Node::GcpPubSubTopic(name.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GooglePubSubSubscriptions(subscriptions) => {
                for sub in subscriptions {
                    if let Some(name) = &sub.name {
                        let node = Node::GcpPubSubSubscription(name.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(topic) = &sub.topic {
                            let topic_node = Node::GcpPubSubTopic(topic.to_string());
                            let t_idx = builder.get_or_add_node(topic_node);
                            builder.add_edge(idx, t_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            GoogleCollection::GoogleRunServices(services) => {
                for service in services {
                    if let Some(name) = &service.name {
                        let node = Node::GcpCloudRunService(name.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GoogleNetworks(networks) => {
                for network in networks {
                    if let Some(name) = &network.self_link {
                        let node = Node::GcpComputeNetwork(name.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            GoogleCollection::GoogleSubnetworks(subnetworks) => {
                for subnetwork in subnetworks {
                    if let Some(name) = &subnetwork.self_link {
                        let node = Node::GcpComputeSubnetwork(name.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(network) = &subnetwork.network {
                            let net_node = Node::GcpComputeNetwork(network.to_string());
                            let n_idx = builder.get_or_add_node(net_node);
                            builder.add_edge(n_idx, idx, Edge::Contains);
                        }
                    }
                }
            }
            GoogleCollection::GoogleForwardingRules(rules) => {
                for rule in rules {
                    if let Some(id) = &rule.id {
                        let node = Node::GcpComputeForwardingRule(id.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(ip) = &rule.ip_address {
                            let ip_node = Node::GenericIpAddress(ip.to_string());
                            let ip_idx = builder.get_or_add_node(ip_node);
                            builder.add_edge(idx, ip_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
        }
    }
}

pub fn azure_projector(builder: &mut GraphBuilder, azure_data: &[MicrosoftCollection]) {
    for x in azure_data {
        match x {
            MicrosoftCollection::AzureVirtualMachines(vms) => {
                for vm in vms {
                    if let Some(id) = &vm.id {
                        let node = Node::AzureVirtualMachine(id.to_string());
                        let idx = builder.get_or_add_node(node);

                        for nic_id in &vm.network_interfaces {
                            let nic_node = Node::AzureNetworkSecurityGroup(nic_id.to_string());
                            let nic_idx = builder.get_or_add_node(nic_node);
                            builder.add_edge(idx, nic_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureVirtualNetworks(vnets) => {
                for vnet in vnets {
                    if let Some(id) = &vnet.id {
                        let node = Node::AzureVirtualNetwork(id.to_string());
                        let idx = builder.get_or_add_node(node);

                        for subnet_id in &vnet.subnets {
                            let subnet_node = Node::AzureSubnet(subnet_id.to_string());
                            let subnet_idx = builder.get_or_add_node(subnet_node);
                            builder.add_edge(idx, subnet_idx, Edge::Contains);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureSubnets(subnets) => {
                for subnet in subnets {
                    if let Some(id) = &subnet.id {
                        let node = Node::AzureSubnet(id.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(vnet_id) = &subnet.vnet_id {
                            let vnet_node = Node::AzureVirtualNetwork(vnet_id.to_string());
                            let v_idx = builder.get_or_add_node(vnet_node);
                            builder.add_edge(v_idx, idx, Edge::Contains);
                        }

                        if let Some(nsg_id) = &subnet.network_security_group_id {
                            let nsg_node = Node::AzureNetworkSecurityGroup(nsg_id.to_string());
                            let nsg_idx = builder.get_or_add_node(nsg_node);
                            builder.add_edge(idx, nsg_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureNetworkSecurityGroups(nsgs) => {
                for nsg in nsgs {
                    if let Some(id) = &nsg.id {
                        let node = Node::AzureNetworkSecurityGroup(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzurePublicIpAddresses(pips) => {
                for pip in pips {
                    if let Some(id) = &pip.id {
                        let node = Node::AzurePublicIpAddress(id.to_string());
                        let idx = builder.get_or_add_node(node);

                        if let Some(ip) = &pip.ip_address {
                            let ip_node = Node::GenericIpAddress(ip.to_string());
                            let ip_idx = builder.get_or_add_node(ip_node);
                            builder.add_edge(idx, ip_idx, Edge::ConnectsTo);
                        }
                    }
                }
            }
            MicrosoftCollection::AzureStorageAccounts(accounts) => {
                for account in accounts {
                    if let Some(id) = &account.id {
                        let node = Node::AzureStorageAccount(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureManagedClusters(clusters) => {
                for cluster in clusters {
                    if let Some(id) = &cluster.id {
                        let node = Node::AzureManagedCluster(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureSqlServers(servers) => {
                for server in servers {
                    if let Some(id) = &server.id {
                        let node = Node::AzureSqlServer(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureAppServices(apps) => {
                for app in apps {
                    if let Some(id) = &app.id {
                        let node = Node::AzureAppService(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureFunctionApps(funcs) => {
                for func in funcs {
                    if let Some(id) = &func.id {
                        let node = Node::AzureFunctionApp(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureApiManagement(apims) => {
                for apim in apims {
                    if let Some(id) = &apim.id {
                        let node = Node::AzureApiManagement(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureCosmosDbs(cosmos) => {
                for db in cosmos {
                    if let Some(id) = &db.id {
                        let node = Node::AzureCosmosDb(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureServiceBuses(sbuses) => {
                for bus in sbuses {
                    if let Some(id) = &bus.id {
                        let node = Node::AzureServiceBus(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureEventGridTopics(egrids) => {
                for topic in egrids {
                    if let Some(id) = &topic.id {
                        let node = Node::AzureEventGridTopic(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureDnsZones(dns) => {
                for zone in dns {
                    if let Some(id) = &zone.id {
                        let node = Node::AzureDnsZone(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
            MicrosoftCollection::AzureCdnProfiles(cdns) => {
                for cdn in cdns {
                    if let Some(id) = &cdn.id {
                        let node = Node::AzureCdnProfile(id.to_string());
                        builder.get_or_add_node(node);
                    }
                }
            }
        }
    }
}

fn use_aws_resource(name: &str, exclude_by_default: bool) -> bool {
    match name {
        // false assoc. unclear if needed
        "AWS::RDS::DBClusterSnapshot" => false,
        "AWS::StepFunctions::StateMachine" => false,
        "AWS::ApiGateway::Stage" => false,
        "AWS::ApiGatewayV2::Api" => false,
        "AWS::EC2::NetworkAcl" => false,
        "AWS::EC2::EIP" => false,
        "AWS::EC2::NetworkInterface" => false,
        "AWS::SNS::Topic" => false,
        // true assoc.
        "AWS::RDS::DBCluster" => true,
        "AWS::S3::Bucket" => true,
        "AWS::SQS::Queue" => true,
        "AWS::EC2::RouteTable" => true,
        "AWS::EC2::VPC" => true,
        "AWS::EC2::Instance" => true,
        "AWS::ElasticLoadBalancing::LoadBalancer" => true,
        "AWS::ElasticLoadBalancingV2::LoadBalancer" => true,
        "AWS::Redshift::ClusterSubnetGroup" => true,
        "AWS::RDS::DBSubnetGroup" => true,
        "AWS::EC2::Subnet" => true,
        "AWS::EC2::InternetGateway" => true,
        "AWS::ECS::Cluster" => true,
        "AWS::Lambda::Function" => true,
        "AWS::RDS::DBInstance" => true,
        "AWS::EKS::Cluster" => true,
        // listeners are probably too granular
        "AWS::ElasticLoadBalancingV2::Listener" => true,
        // TODO: below are unclear if actually wanted/needed
        "AWS::Route53Resolver::ResolverRuleAssociation" => true,
        "AWS::EC2::VPCEndpoint" => true,
        "AWS::Route53Resolver::ResolverRule" => true,
        "AWS::DynamoDB::Table" => true,
        // exclude by default
        _ => !exclude_by_default.to_owned(),
    }
}

fn use_global(name: &str) -> bool {
    matches!(name, "AWS::S3::Bucket")
}

fn get_category(name: &str) -> String {
    let parts: Vec<&str> = name.split("::").collect();
    if parts.len() >= 2 {
        parts[..parts.len() - 1].join("::")
    } else {
        "Generic".to_string()
    }
}
