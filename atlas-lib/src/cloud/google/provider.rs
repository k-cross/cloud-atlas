use crate::Settings;
use crate::api::google::client::GoogleApiClient;
use crate::api::google::{compute, compute_network, dns, functions, gke, sql};
use crate::cloud::definition::{GoogleCollection, Provider};
use yup_oauth2::ApplicationSecret;

pub async fn build_gcp(
    _verbose: bool,
    opts: &Settings,
) -> Result<Provider, Box<dyn std::error::Error>> {
    let mut services = Vec::new();

    let secret: ApplicationSecret = Default::default();

    let auth = yup_oauth2::InstalledFlowAuthenticator::builder(
        secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .build()
    .await?;

    let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
    let token = auth.token(scopes).await?;
    let token_str = token.token().unwrap_or("").to_string();

    let client = GoogleApiClient::new(token_str);

    let mut futures = Vec::new();

    if let Some(projects) = &opts.gcp_projects {
        for p in projects.clone() {
            let client_ref = client.clone();
            futures.push(async move {
                let (
                    r_instances,
                    r_firewalls,
                    r_sqls,
                    r_zones,
                    r_clusters,
                    r_funcs,
                    r_buckets,
                    r_topics,
                    r_subs,
                    r_runs,
                    r_networks,
                    r_subnets,
                    r_fwrules,
                ) = tokio::join!(
                    compute::list_instances(&client_ref, &p),
                    compute::list_firewalls(&client_ref, &p),
                    sql::list_instances(&client_ref, &p),
                    dns::list_managed_zones(&client_ref, &p),
                    gke::list_clusters(&client_ref, &p),
                    functions::list_functions(&client_ref, &p),
                    client_ref.list_buckets(&p),
                    client_ref.list_topics(&p),
                    client_ref.list_subscriptions(&p),
                    client_ref.list_run_services(&p),
                    compute_network::list_networks(&client_ref, &p),
                    compute_network::list_subnetworks(&client_ref, &p),
                    compute_network::list_forwarding_rules(&client_ref, &p),
                );

                let mut local_services = Vec::new();

                macro_rules! add_if_ok {
                    ($res:expr, $variant:path) => {
                        match $res {
                            Ok(items) => local_services.push($variant(items)),
                            Err(e) => {
                                eprintln!("Error fetching GCP resource in project {}: {:?}", p, e)
                            }
                        }
                    };
                }

                add_if_ok!(r_instances, GoogleCollection::GoogleInstances);
                add_if_ok!(r_firewalls, GoogleCollection::GoogleFirewalls);
                add_if_ok!(r_sqls, GoogleCollection::GoogleSql);
                add_if_ok!(r_zones, GoogleCollection::GoogleDns);
                add_if_ok!(r_clusters, GoogleCollection::GoogleGke);
                add_if_ok!(r_funcs, GoogleCollection::GoogleFunctions);
                add_if_ok!(r_buckets, GoogleCollection::GoogleStorageBuckets);
                add_if_ok!(r_topics, GoogleCollection::GooglePubSubTopics);
                add_if_ok!(r_subs, GoogleCollection::GooglePubSubSubscriptions);
                add_if_ok!(r_runs, GoogleCollection::GoogleRunServices);
                add_if_ok!(r_networks, GoogleCollection::GoogleNetworks);
                add_if_ok!(r_subnets, GoogleCollection::GoogleSubnetworks);
                add_if_ok!(r_fwrules, GoogleCollection::GoogleForwardingRules);

                Ok::<Vec<GoogleCollection>, Box<dyn std::error::Error>>(local_services)
            });
        }
    }

    let results = futures::future::try_join_all(futures).await?;
    for mut res in results {
        services.append(&mut res);
    }

    Ok(Provider::GCP(services))
}
