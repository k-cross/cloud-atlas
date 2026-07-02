use crate::Settings;
use crate::api::google::client::GoogleApiClient;
use crate::cloud::definition::Provider;
use crate::cloud::google::{
    compute_network, dns, firewall, functions, gke, instance, pubsub, run, sql, storage,
};
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
                    r_pubsub,
                    r_runs,
                    r_network,
                ) = tokio::join!(
                    instance::collector::runner(&p, &client_ref),
                    firewall::collector::runner(&p, &client_ref),
                    sql::collector::runner(&p, &client_ref),
                    dns::collector::runner(&p, &client_ref),
                    gke::collector::runner(&p, &client_ref),
                    functions::collector::runner(&p, &client_ref),
                    storage::collector::runner(&p, &client_ref),
                    pubsub::collector::runner(&p, &client_ref),
                    run::collector::runner(&p, &client_ref),
                    compute_network::collector::runner(&p, &client_ref),
                );

                let mut local_services = Vec::new();

                let mut add_if_ok = |res: Result<
                    crate::cloud::definition::GoogleCollection,
                    Box<dyn std::error::Error>,
                >| {
                    if let Ok(collection) = res {
                        local_services.push(collection);
                    } else if let Err(e) = res {
                        eprintln!("Error fetching GCP resource: {:?}", e);
                    }
                };

                add_if_ok(r_instances);
                add_if_ok(r_firewalls);
                add_if_ok(r_sqls);
                add_if_ok(r_zones);
                add_if_ok(r_clusters);
                add_if_ok(r_funcs);
                add_if_ok(r_buckets);
                add_if_ok(r_runs);

                if let Ok((topics, subscriptions)) = r_pubsub {
                    local_services.push(topics);
                    local_services.push(subscriptions);
                } else if let Err(e) = r_pubsub {
                    eprintln!("Error fetching GCP PubSub resources: {:?}", e);
                }

                if let Ok((networks, subnets, fwrules)) = r_network {
                    local_services.push(networks);
                    local_services.push(subnets);
                    local_services.push(fwrules);
                } else if let Err(e) = r_network {
                    eprintln!("Error fetching GCP Network resources: {:?}", e);
                }

                Ok::<Vec<crate::cloud::definition::GoogleCollection>, Box<dyn std::error::Error>>(
                    local_services,
                )
            });
        }
    }

    let results = futures::future::try_join_all(futures).await?;
    for mut res in results {
        services.append(&mut res);
    }

    Ok(Provider::GCP(services))
}
