use crate::Settings;
use crate::cloud::definition::CloudflareCollection;
use cloudflare::framework::Environment;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::client::async_api::Client;
use std::env;

pub async fn build_cloudflare(
    verbose: bool,
    _settings: &Settings,
) -> Result<CloudflareCollection, Box<dyn std::error::Error>> {
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

    if verbose {
        println!("Fetching Cloudflare Zones...");
    }
    let zones = super::zone::get_zones(&client).await?;

    let mut all_dns_records = Vec::new();
    let mut all_workers = Vec::new();
    let mut all_kv_namespaces = Vec::new();
    let mut all_r2_buckets = Vec::new();
    let mut all_durable_objects = Vec::new();
    let mut all_d1_databases = Vec::new();
    let mut all_worker_bindings = Vec::new();
    let mut accounts_seen = std::collections::HashSet::new();

    for zone in &zones {
        if verbose {
            println!("Fetching DNS records for zone: {}", zone.name);
        }
        if let Ok(records) = super::dns::get_dns_records(&client, &zone.id).await {
            all_dns_records.push((zone.id.clone(), records));
        }

        // Fetch workers for the account this zone belongs to (only once per account)
        let account_id = &zone.account.id;
        if !accounts_seen.contains(account_id) {
            accounts_seen.insert(account_id.clone());
            if verbose {
                println!("Fetching workers for account: {}", account_id);
            }

            let workers_fut = super::worker::get_workers(account_id, &token);
            let kvs_fut = super::kv::get_kv_namespaces(&client, account_id);
            let r2s_fut = super::r2::get_r2_buckets(&client, account_id);
            let dos_fut = super::durable_objects::get_do_namespaces(account_id, &token);
            let d1s_fut = super::d1::get_d1_databases(account_id, &token);

            let (workers_res, kvs_res, r2s_res, dos_res, d1s_res) =
                tokio::join!(workers_fut, kvs_fut, r2s_fut, dos_fut, d1s_fut);

            if let Ok(workers) = workers_res {
                let mut bindings_futures = Vec::new();
                for worker in &workers {
                    let t = token.clone();
                    let aid = account_id.clone();
                    let wid = worker.id.clone();
                    bindings_futures.push(async move {
                        let res = super::worker::get_worker_bindings(&aid, &wid, &t).await;
                        (wid, res)
                    });
                }
                let bindings_results = futures::future::join_all(bindings_futures).await;
                for (wid, res) in bindings_results {
                    if let Ok(bindings) = res {
                        all_worker_bindings.push((wid, bindings));
                    }
                }
                all_workers.extend(workers);
            } else if verbose {
                println!(
                    "Warning: Failed to fetch workers for account {}",
                    account_id
                );
            }

            if let Ok(kvs) = kvs_res {
                all_kv_namespaces.extend(kvs);
            }
            if let Ok(buckets) = r2s_res {
                all_r2_buckets.extend(buckets);
            }
            if let Ok(dos) = dos_res {
                all_durable_objects.extend(dos);
            }
            if let Ok(d1s) = d1s_res {
                all_d1_databases.extend(d1s);
            }
        }
    }

    Ok(CloudflareCollection {
        zones,
        dns_records: all_dns_records,
        workers: all_workers,
        kv_namespaces: all_kv_namespaces,
        r2_buckets: all_r2_buckets,
        durable_objects: all_durable_objects,
        d1_databases: all_d1_databases,
        worker_bindings: all_worker_bindings,
    })
}
