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
                    instances,
                    firewalls,
                    sqls,
                    zones,
                    clusters,
                    funcs,
                    buckets,
                    (topics, subscriptions),
                    runs,
                    (networks, subnets, fwrules),
                ) = tokio::try_join!(
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
                )?;

                let local_services = vec![
                    instances,
                    firewalls,
                    sqls,
                    zones,
                    clusters,
                    funcs,
                    buckets,
                    topics,
                    subscriptions,
                    runs,
                    networks,
                    subnets,
                    fwrules,
                ];

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
