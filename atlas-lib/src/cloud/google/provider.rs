use crate::Settings;
use crate::api::google::client::{GoogleApiClient, ProjectApis};
use crate::api::google::{
    compute, compute_network, dns, functions, gke, interconnect, network_connectivity, sql, vpn,
};
use crate::atlas::collection::{CollectionReport, CollectionSource, FailureKind, ProviderScan};
use crate::cloud::collector::{NamedCollector, run_all};
use crate::cloud::definition::{GoogleCollection, Provider};
use yup_oauth2::ApplicationSecret;

const SOURCE: CollectionSource = CollectionSource::Gcp;

macro_rules! collectors {
    ($apis:expr; $($name:literal => $variant:path, $run:expr),+ $(,)?) => {
        vec![$(($name, Box::pin(async { $apis.unless_disabled($run).await.map($variant) }) as _)),+]
    };
}

async fn authenticate() -> Result<GoogleApiClient, Box<dyn std::error::Error>> {
    let secret: ApplicationSecret = Default::default();

    let auth = yup_oauth2::InstalledFlowAuthenticator::builder(
        secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .build()
    .await?;

    let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
    let token = auth.token(scopes).await?;

    Ok(GoogleApiClient::new(
        token.token().unwrap_or("").to_string(),
    ))
}

pub async fn build_gcp(_verbose: bool, opts: &Settings) -> ProviderScan {
    let mut services = Vec::new();
    let mut report = CollectionReport::default();

    let client = match authenticate().await {
        Ok(client) => client,
        Err(e) => {
            report.record(SOURCE, FailureKind::Unauthorized, "auth", e);
            return ProviderScan {
                provider: Provider::GCP(services),
                report,
            };
        }
    };

    let mut futures = Vec::new();
    for p in opts.gcp_projects.as_deref().unwrap_or_default() {
        let c = &client;
        futures.push(async move {
            let apis = ProjectApis::new(c, p);
            let collectors: Vec<NamedCollector<'_, GoogleCollection>> = collectors![&apis;
                "compute_instances" => GoogleCollection::GoogleInstances, compute::list_instances(c, p),
                "firewalls" => GoogleCollection::GoogleFirewalls, compute::list_firewalls(c, p),
                "sql" => GoogleCollection::GoogleSql, sql::list_instances(c, p),
                "dns" => GoogleCollection::GoogleDns, dns::list_managed_zones(c, p),
                "gke" => GoogleCollection::GoogleGke, gke::list_clusters(c, p),
                "functions" => GoogleCollection::GoogleFunctions, functions::list_functions(c, p),
                "storage" => GoogleCollection::GoogleStorageBuckets, c.list_buckets(p),
                "pubsub_topics" => GoogleCollection::GooglePubSubTopics, c.list_topics(p),
                "pubsub_subscriptions" => GoogleCollection::GooglePubSubSubscriptions, c.list_subscriptions(p),
                "run" => GoogleCollection::GoogleRunServices, c.list_run_services(p),
                "networks" => GoogleCollection::GoogleNetworks, compute_network::list_networks(c, p),
                "subnetworks" => GoogleCollection::GoogleSubnetworks, compute_network::list_subnetworks(c, p),
                "forwarding_rules" => GoogleCollection::GoogleForwardingRules, compute_network::list_forwarding_rules(c, p),
                "vpn_gateways" => GoogleCollection::GoogleVpnGateways, vpn::list_vpn_gateways(c, p),
                "vpn_tunnels" => GoogleCollection::GoogleVpnTunnels, vpn::list_vpn_tunnels(c, p),
                "external_vpn_gateways" => GoogleCollection::GoogleExternalVpnGateways, vpn::list_external_vpn_gateways(c, p),
                "routers" => GoogleCollection::GoogleRouters, interconnect::list_routers(c, p),
                "interconnects" => GoogleCollection::GoogleInterconnects, interconnect::list_interconnects(c, p),
                "interconnect_attachments" => GoogleCollection::GoogleInterconnectAttachments, interconnect::list_interconnect_attachments(c, p),
                "hubs" => GoogleCollection::GoogleHubs, network_connectivity::list_hubs(c, p),
                "spokes" => GoogleCollection::GoogleSpokes, network_connectivity::list_spokes(c, p),
            ];

            let (collections, local_report) = run_all(collectors, SOURCE, p).await;
            let local_services: Vec<_> = collections
                .into_iter()
                .map(|collection| (p.to_owned(), collection))
                .collect();

            (local_services, local_report)
        });
    }

    for (mut project_services, project_report) in futures::future::join_all(futures).await {
        services.append(&mut project_services);
        report.merge(project_report);
    }

    ProviderScan {
        provider: Provider::GCP(services),
        report,
    }
}
