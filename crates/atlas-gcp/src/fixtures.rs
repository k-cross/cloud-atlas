use crate::definition::GoogleCollection;
use atlas_provider::fixtures::{
    AWS_VPN_ADDRESSES, DX_AMAZON_ADDRESS, DX_GOOGLE_ADDRESS, ER_GOOGLE_ADDRESS,
    ER_MICROSOFT_ADDRESS, GCP_PROJECT, GCP_VPN_ADDRESS,
};

pub fn gcp_network() -> String {
    format!("projects/{}/global/networks/vpc-globex", GCP_PROJECT)
}

pub fn collections() -> Vec<(String, GoogleCollection)> {
    use crate::api::compute::{AccessConfig, Firewall, Instance, NetworkInterface};
    use crate::api::compute_network::{ForwardingRule, Network, NetworkPeering, Subnetwork};
    use crate::api::dns::ManagedZone;
    use crate::api::functions::CloudFunction;
    use crate::api::gke::{Cluster, NetworkConfig};
    use crate::api::interconnect::{Interconnect, InterconnectAttachment, Router};
    use crate::api::network_connectivity::{Hub, LinkedVpcNetwork, Spoke};
    use crate::api::pubsub::{Subscription, Topic};
    use crate::api::run::Service;
    use crate::api::sql::{SqlInstance, SqlIpAddress, SqlIpConfiguration, SqlSettings};
    use crate::api::storage::Bucket;
    use crate::api::vpn::{ExternalVpnGateway, GatewayInterface, VpnGateway, VpnTunnel};

    let network_link = format!(
        "https://www.googleapis.com/compute/v1/projects/{}/global/networks/vpc-globex",
        GCP_PROJECT
    );
    let subnet_link = format!(
        "https://www.googleapis.com/compute/v1/projects/{}/regions/us-central1/subnetworks/subnet-data",
        GCP_PROJECT
    );

    let instance = Instance {
        id: Some("7000000000000000001".to_owned()),
        name: Some("gce-worker-01".to_owned()),
        self_link: Some(format!(
            "https://www.googleapis.com/compute/v1/projects/{}/zones/us-central1-a/instances/gce-worker-01",
            GCP_PROJECT
        )),
        network_interfaces: Some(vec![NetworkInterface {
            network: Some(network_link.clone()),
            subnetwork: Some(subnet_link.clone()),
            network_i_p: Some("10.30.0.11".to_owned()),
            ipv6_address: None,
            access_configs: Some(vec![AccessConfig {
                nat_i_p: Some("34.120.0.21".to_owned()),
                external_ipv6: None,
            }]),
            ipv6_access_configs: Some(vec![AccessConfig {
                nat_i_p: None,
                external_ipv6: Some("2600:1900:4000:1::15".to_owned()),
            }]),
        }]),
    };

    let firewall = Firewall {
        id: Some("fw-egress-globex".to_owned()),
        name: Some("allow-egress-partner".to_owned()),
        network: Some(network_link.clone()),
        direction: Some("EGRESS".to_owned()),
        destination_ranges: Some(vec!["192.0.2.55".to_owned()]),
        ..Default::default()
    };

    let sql = SqlInstance {
        name: Some("globex-analytics-db".to_owned()),
        ip_addresses: Some(vec![SqlIpAddress {
            ip_type: Some("PRIVATE".to_owned()),
            ip_address: Some("10.20.0.5".to_owned()),
        }]),
        settings: Some(SqlSettings {
            ip_configuration: Some(SqlIpConfiguration {
                private_network: Some(format!(
                    "projects/{}/global/networks/vpc-globex",
                    GCP_PROJECT
                )),
            }),
        }),
        ..Default::default()
    };

    let dns = ManagedZone {
        name: Some("globex-internal".to_owned()),
        dns_name: Some("internal.globex.io.".to_owned()),
        ..Default::default()
    };

    let gke = Cluster {
        name: Some("gke-globex".to_owned()),
        network: Some("vpc-globex".to_owned()),
        network_config: Some(NetworkConfig {
            network: Some(format!(
                "projects/{}/global/networks/vpc-globex",
                GCP_PROJECT
            )),
        }),
        ..Default::default()
    };

    let func = CloudFunction {
        name: Some(format!(
            "projects/{}/locations/us-central1/functions/resize-images",
            GCP_PROJECT
        )),
        ..Default::default()
    };

    let bucket = Bucket {
        id: Some("globex-datalake".to_owned()),
        name: Some("globex-datalake".to_owned()),
        ..Default::default()
    };

    let topic = Topic {
        name: Some(format!("projects/{}/topics/globex-events", GCP_PROJECT)),
    };
    let sub = Subscription {
        name: Some(format!(
            "projects/{}/subscriptions/globex-sink",
            GCP_PROJECT
        )),
        topic: Some(format!("projects/{}/topics/globex-events", GCP_PROJECT)),
    };

    let run_svc = Service {
        name: Some(format!(
            "projects/{}/locations/us-central1/services/globex-api",
            GCP_PROJECT
        )),
        uri: Some("https://run.globex.app".to_owned()),
        ..Default::default()
    };

    let peering = |name: &str, project: &str, network: &str, state: &str| NetworkPeering {
        name: Some(name.to_owned()),
        network: Some(format!(
            "https://www.googleapis.com/compute/v1/projects/{project}/global/networks/{network}"
        )),
        state: Some(state.to_owned()),
    };
    let net = Network {
        self_link: Some(network_link.clone()),
        peerings: Some(vec![
            peering(
                "servicenetworking-googleapis-com",
                "globex-sql-tenant",
                "servicenetworking",
                "ACTIVE",
            ),
            peering("peer-legacy", "globex-legacy", "legacy", "INACTIVE"),
        ]),
        ..Default::default()
    };
    let hub_name = format!("projects/{GCP_PROJECT}/locations/global/hubs/globex-hub");
    let hub = Hub {
        name: Some(hub_name.clone()),
        state: Some("ACTIVE".to_owned()),
    };
    let spoke = |name: &str, network: &str| Spoke {
        name: Some(format!(
            "projects/{GCP_PROJECT}/locations/global/spokes/{name}"
        )),
        hub: Some(hub_name.clone()),
        group: Some(format!("{hub_name}/groups/default")),
        state: Some("ACTIVE".to_owned()),
        linked_vpc_network: Some(LinkedVpcNetwork {
            uri: Some(format!(
                "https://www.googleapis.com/compute/v1/projects/{GCP_PROJECT}/global/networks/{network}"
            )),
        }),
    };
    let spokes = vec![
        spoke("spoke-globex", "vpc-globex"),
        spoke("spoke-analytics", "vpc-globex-analytics"),
    ];

    let subnet = Subnetwork {
        self_link: Some(subnet_link),
        network: Some(network_link),
        ip_cidr_range: Some("10.30.0.0/24".to_owned()),
        ..Default::default()
    };

    let fw_rule = ForwardingRule {
        id: Some("fr-globex-lb".to_owned()),
        ip_address: Some("34.120.0.9".to_owned()),
        ..Default::default()
    };

    let vpn_gateway_link = format!(
        "https://www.googleapis.com/compute/v1/projects/{GCP_PROJECT}/regions/us-central1/vpnGateways/ha-gw-globex"
    );
    let external_gateway_link = format!(
        "https://www.googleapis.com/compute/v1/projects/{GCP_PROJECT}/global/externalVpnGateways/aws-peer"
    );
    let vpn_gateway = VpnGateway {
        name: Some("ha-gw-globex".to_owned()),
        network: Some(net.self_link.clone().unwrap_or_default()),
        self_link: Some(vpn_gateway_link.clone()),
        vpn_interfaces: Some(vec![GatewayInterface {
            id: Some(0),
            ip_address: Some(GCP_VPN_ADDRESS.to_owned()),
        }]),
        ..Default::default()
    };
    let external_gateway = ExternalVpnGateway {
        name: Some("aws-peer".to_owned()),
        self_link: Some(external_gateway_link.clone()),
        interfaces: Some(vec![GatewayInterface {
            id: Some(0),
            ip_address: Some(AWS_VPN_ADDRESSES[2].to_owned()),
        }]),
        ..Default::default()
    };
    let vpn_tunnel = VpnTunnel {
        name: Some("tun-aws-0".to_owned()),
        self_link: Some(format!(
            "https://www.googleapis.com/compute/v1/projects/{GCP_PROJECT}/regions/us-central1/vpnTunnels/tun-aws-0"
        )),
        status: Some("ESTABLISHED".to_owned()),
        vpn_gateway: Some(vpn_gateway_link),
        peer_external_gateway: Some(external_gateway_link),
        ..Default::default()
    };

    let compute = format!("https://www.googleapis.com/compute/v1/projects/{GCP_PROJECT}");
    let router_link = format!("{compute}/regions/us-east4/routers/rtr-globex-interconnect");
    let router = Router {
        name: Some("rtr-globex-interconnect".to_owned()),
        self_link: Some(router_link.clone()),
        network: net.self_link.clone(),
        ..Default::default()
    };
    let interconnect_link = |name: &str| format!("{compute}/global/interconnects/{name}");
    let interconnect = |name: &str| Interconnect {
        name: Some(name.to_owned()),
        self_link: Some(interconnect_link(name)),
        state: Some("ACTIVE".to_owned()),
        ..Default::default()
    };
    let attachment = |name: &str, interconnect: &str, cloud_router: &str, customer_router: &str| {
        InterconnectAttachment {
            name: Some(name.to_owned()),
            self_link: Some(format!(
                "{compute}/regions/us-east4/interconnectAttachments/{name}"
            )),
            state: Some("ACTIVE".to_owned()),
            router: Some(router_link.clone()),
            interconnect: Some(interconnect_link(interconnect)),
            cloud_router_ip_address: Some(cloud_router.to_owned()),
            customer_router_ip_address: Some(customer_router.to_owned()),
            ..Default::default()
        }
    };
    let interconnects = vec![
        interconnect("cci-globex-aws"),
        interconnect("cci-globex-azure"),
    ];
    let attachments = vec![
        attachment(
            "att-globex-aws",
            "cci-globex-aws",
            DX_GOOGLE_ADDRESS,
            DX_AMAZON_ADDRESS,
        ),
        attachment(
            "att-globex-azure",
            "cci-globex-azure",
            ER_GOOGLE_ADDRESS,
            ER_MICROSOFT_ADDRESS,
        ),
    ];

    let p = GCP_PROJECT.to_owned();
    vec![
        (p.clone(), GoogleCollection::GoogleInstances(vec![instance])),
        (p.clone(), GoogleCollection::GoogleFirewalls(vec![firewall])),
        (p.clone(), GoogleCollection::GoogleSql(vec![sql])),
        (p.clone(), GoogleCollection::GoogleDns(vec![dns])),
        (p.clone(), GoogleCollection::GoogleGke(vec![gke])),
        (p.clone(), GoogleCollection::GoogleFunctions(vec![func])),
        (
            p.clone(),
            GoogleCollection::GoogleStorageBuckets(vec![bucket]),
        ),
        (p.clone(), GoogleCollection::GooglePubSubTopics(vec![topic])),
        (
            p.clone(),
            GoogleCollection::GooglePubSubSubscriptions(vec![sub]),
        ),
        (
            p.clone(),
            GoogleCollection::GoogleRunServices(vec![run_svc]),
        ),
        (p.clone(), GoogleCollection::GoogleNetworks(vec![net])),
        (p.clone(), GoogleCollection::GoogleSubnetworks(vec![subnet])),
        (
            p.clone(),
            GoogleCollection::GoogleForwardingRules(vec![fw_rule]),
        ),
        (
            p.clone(),
            GoogleCollection::GoogleVpnGateways(vec![vpn_gateway]),
        ),
        (
            p.clone(),
            GoogleCollection::GoogleVpnTunnels(vec![vpn_tunnel]),
        ),
        (
            p.clone(),
            GoogleCollection::GoogleExternalVpnGateways(vec![external_gateway]),
        ),
        (p.clone(), GoogleCollection::GoogleRouters(vec![router])),
        (
            p.clone(),
            GoogleCollection::GoogleInterconnects(interconnects),
        ),
        (
            p.clone(),
            GoogleCollection::GoogleInterconnectAttachments(attachments),
        ),
        (p.clone(), GoogleCollection::GoogleHubs(vec![hub])),
        (p, GoogleCollection::GoogleSpokes(spokes)),
    ]
}
