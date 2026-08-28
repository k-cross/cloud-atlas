use crate::Settings;
use crate::atlas::collection::{CollectionFailure, CollectionSource, ProviderScan};
use crate::cloud::definition::{CloudflareCollection, Provider};
use cloudflare::framework::Environment;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::client::async_api::Client;
use std::env;

pub async fn build_cloudflare(
    verbose: bool,
    _settings: &Settings,
) -> Result<ProviderScan, Box<dyn std::error::Error>> {
    let token = env::var("CLOUDFLARE_API_TOKEN").unwrap_or_default();
    if token.is_empty() {
        return Err("CLOUDFLARE_API_TOKEN is not set".into());
    }

    let credentials = Credentials::UserAuthToken {
        token: token.clone(),
    };
    let client = Client::new(
        credentials,
        cloudflare::framework::client::ClientConfig::default(),
        Environment::Production,
    )?;

    // Client for the raw REST endpoints not covered by the `cloudflare` crate.
    let cf = super::CloudflareApiClient::new(token.clone());

    if verbose {
        println!("Fetching Cloudflare Zones...");
    }
    let zones = super::zone::get_zones(&client).await?;

    let mut all_dns_records = std::collections::HashMap::new();
    let mut all_workers = Vec::new();
    let mut all_kv_namespaces = Vec::new();
    let mut all_r2_buckets = Vec::new();
    let mut all_durable_objects = Vec::new();
    let mut all_d1_databases = Vec::new();
    let mut all_worker_bindings = std::collections::HashMap::new();
    let mut accounts_seen = std::collections::HashSet::new();
    let mut failures: Vec<CollectionFailure> = Vec::new();

    for zone in &zones {
        if verbose {
            println!("Fetching DNS records for zone: {}", zone.name);
        }
        match super::dns::get_dns_records(&client, &zone.id).await {
            Ok(records) => {
                all_dns_records.insert(zone.id.clone(), records);
            }
            Err(e) => failures.push(CollectionFailure {
                source: CollectionSource::Cloudflare,
                scope: format!("zone {}/dns", zone.name),
                message: format!("{e:?}"),
            }),
        }

        // Fetch account-level resources only once per account
        let account_id = &zone.account.id;
        if !accounts_seen.insert(account_id.clone()) {
            continue;
        }
        if verbose {
            println!("Fetching workers for account: {}", account_id);
        }

        let (workers_res, kvs_res, r2s_res, dos_res, d1s_res) = tokio::join!(
            super::worker::get_workers(&cf, account_id),
            super::kv::get_kv_namespaces(&client, account_id),
            super::r2::get_r2_buckets(&client, account_id),
            super::durable_objects::get_do_namespaces(&cf, account_id),
            super::d1::get_d1_databases(&cf, account_id),
        );

        if let Ok(workers) = workers_res {
            let bindings_futures = workers.iter().map(|worker| {
                let cf = &cf;
                let wid = worker.id.clone();
                async move {
                    let res = super::worker::get_worker_bindings(cf, account_id, &wid).await;
                    (wid, res)
                }
            });
            for (wid, res) in futures::future::join_all(bindings_futures).await {
                match res {
                    Ok(bindings) => {
                        all_worker_bindings.insert(wid, bindings);
                    }
                    Err(e) => failures.push(CollectionFailure {
                        source: CollectionSource::Cloudflare,
                        scope: format!("account {account_id}/worker {wid}/bindings"),
                        message: format!("{e:?}"),
                    }),
                }
            }
            all_workers.extend(workers);
        } else if let Err(e) = workers_res {
            failures.push(CollectionFailure {
                source: CollectionSource::Cloudflare,
                scope: format!("account {account_id}/workers"),
                message: format!("{e:?}"),
            });
        }

        match kvs_res {
            Ok(kvs) => all_kv_namespaces.extend(kvs),
            Err(e) => failures.push(CollectionFailure {
                source: CollectionSource::Cloudflare,
                scope: format!("account {account_id}/kv_namespaces"),
                message: format!("{e:?}"),
            }),
        }
        match r2s_res {
            Ok(buckets) => all_r2_buckets.extend(buckets),
            Err(e) => failures.push(CollectionFailure {
                source: CollectionSource::Cloudflare,
                scope: format!("account {account_id}/r2_buckets"),
                message: format!("{e:?}"),
            }),
        }
        match dos_res {
            Ok(dos) => all_durable_objects.extend(dos),
            Err(e) => failures.push(CollectionFailure {
                source: CollectionSource::Cloudflare,
                scope: format!("account {account_id}/durable_objects"),
                message: format!("{e:?}"),
            }),
        }
        match d1s_res {
            Ok(d1s) => all_d1_databases.extend(d1s),
            Err(e) => failures.push(CollectionFailure {
                source: CollectionSource::Cloudflare,
                scope: format!("account {account_id}/d1_databases"),
                message: format!("{e:?}"),
            }),
        }
    }

    let provider = Provider::Cloudflare(Box::new(CloudflareCollection {
        zones,
        dns_records: all_dns_records,
        workers: all_workers,
        kv_namespaces: all_kv_namespaces,
        r2_buckets: all_r2_buckets,
        durable_objects: all_durable_objects,
        d1_databases: all_d1_databases,
        worker_bindings: all_worker_bindings,
    }));

    Ok(ProviderScan { provider, failures })
}
