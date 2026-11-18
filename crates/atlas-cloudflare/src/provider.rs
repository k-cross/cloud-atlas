use crate::definition::{CloudflareCollection, ScriptId, ZoneId};
use atlas_graph::collection::{CollectionReport, CollectionSource, FailureKind};
use atlas_provider::ProviderScan;
use atlas_provider::Settings;
use cloudflare::framework::Environment;
use cloudflare::framework::auth::Credentials;
use cloudflare::framework::client::async_api::Client;
use std::env;

const SOURCE: CollectionSource = CollectionSource::Cloudflare;

pub async fn build_cloudflare(
    verbose: bool,
    _settings: &Settings,
) -> ProviderScan<CloudflareCollection> {
    let mut data = CloudflareCollection::default();
    let mut report = CollectionReport::default();

    match clients() {
        Ok((client, cf)) => collect(&client, &cf, verbose, &mut data, &mut report).await,
        Err(e) => report.record(SOURCE, FailureKind::Unauthorized, "credentials", e),
    }

    ProviderScan {
        collections: data,
        report,
    }
}

fn clients() -> Result<(Client, super::CloudflareApiClient), Box<dyn std::error::Error>> {
    let token = env::var("CLOUDFLARE_API_TOKEN").unwrap_or_default();
    if token.is_empty() {
        return Err("CLOUDFLARE_API_TOKEN is not set".into());
    }

    let client = Client::new(
        Credentials::UserAuthToken {
            token: token.clone(),
        },
        cloudflare::framework::client::ClientConfig::default(),
        Environment::Production,
    )?;

    Ok((client, super::CloudflareApiClient::new(token)))
}

pub async fn collect(
    client: &Client,
    cf: &super::CloudflareApiClient,
    verbose: bool,
    data: &mut CloudflareCollection,
    report: &mut CollectionReport,
) {
    if verbose {
        println!("Fetching Cloudflare Zones...");
    }
    match super::zone::get_zones(client).await {
        Ok(zones) => data.zones = zones,
        Err(e) => {
            report.record(SOURCE, FailureKind::Unavailable, "zones", e);
            return;
        }
    }

    if verbose {
        println!("Fetching DNS records for {} zones...", data.zones.len());
    }
    let dns_futures = data
        .zones
        .iter()
        .map(|zone| async move { (zone, super::dns::get_dns_records(client, &zone.id).await) });

    for (zone, result) in futures::future::join_all(dns_futures).await {
        match result {
            Ok(records) => {
                data.dns_records.insert(ZoneId(zone.id.clone()), records);
            }
            Err(e) => report.record(
                SOURCE,
                FailureKind::Unavailable,
                format!("zone {}/dns", zone.name),
                e,
            ),
        }
    }

    if verbose {
        println!("Fetching Cloudflare accounts...");
    }
    let listed = match super::account::get_accounts(cf).await {
        Ok(accounts) => accounts,
        Err(e) => {
            report.record(SOURCE, FailureKind::Unavailable, "accounts", e);
            Vec::new()
        }
    };
    let mut account_ids: Vec<String> = Vec::new();
    let zone_accounts = data.zones.iter().map(|zone| zone.account.id.clone());
    for account_id in listed
        .into_iter()
        .map(|account| account.id)
        .chain(zone_accounts)
    {
        if !account_ids.contains(&account_id) {
            account_ids.push(account_id);
        }
    }

    for account_id in &account_ids {
        if verbose {
            println!("Fetching workers for account: {}", account_id);
        }

        let (workers_res, kvs_res, r2s_res, dos_res, d1s_res) = tokio::join!(
            super::worker::get_workers(cf, account_id),
            super::kv::get_kv_namespaces(client, account_id),
            super::r2::get_r2_buckets(cf, account_id),
            super::durable_objects::get_do_namespaces(cf, account_id),
            super::d1::get_d1_databases(cf, account_id),
        );

        match workers_res {
            Ok(workers) => {
                let bindings_futures = workers.into_iter().map(|worker| {
                    let id = ScriptId {
                        account: account_id.clone(),
                        script: worker.id,
                    };
                    async move {
                        let res =
                            super::worker::get_worker_bindings(cf, account_id, &id.script).await;
                        (id, res)
                    }
                });
                for (id, res) in futures::future::join_all(bindings_futures).await {
                    match res {
                        Ok(bindings) => {
                            data.worker_bindings.insert(id.clone(), bindings);
                        }
                        Err(e) => report.record(
                            SOURCE,
                            FailureKind::Unavailable,
                            format!("account {account_id}/worker {}/bindings", id.script),
                            e,
                        ),
                    }
                    data.workers.push(id);
                }
            }
            Err(e) => report.record(
                SOURCE,
                FailureKind::Unavailable,
                format!("account {account_id}/workers"),
                e,
            ),
        }

        macro_rules! extend_or_record {
            ($field:ident, $result:expr) => {
                match $result {
                    Ok(items) => data.$field.extend(items),
                    Err(e) => {
                        let scope = format!("account {account_id}/{}", stringify!($field));
                        report.record(SOURCE, FailureKind::Unavailable, scope, e)
                    }
                }
            };
        }

        extend_or_record!(kv_namespaces, kvs_res);
        extend_or_record!(r2_buckets, r2s_res);
        extend_or_record!(durable_objects, dos_res);
        extend_or_record!(d1_databases, d1s_res);
    }
}
