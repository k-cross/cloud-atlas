use crate::Settings;
use crate::api::azure::client::AzureApiClient;
use crate::api::azure::models::*;
use crate::atlas::collection::{CollectionReport, CollectionSource, FailureKind, ProviderScan};
use crate::cloud::definition::{MicrosoftCollection, Provider};
use serde::Deserialize;

const SOURCE: CollectionSource = CollectionSource::Azure;

const MAX_REPORTED_ROWS: usize = 5;

pub async fn build_azure(_verbose: bool, opts: &Settings) -> ProviderScan {
    let mut report = CollectionReport::default();

    let client = match AzureApiClient::new().await {
        Ok(client) => client,
        Err(e) => {
            report.record(SOURCE, FailureKind::Unauthorized, "credentials", e);
            return ProviderScan {
                provider: Provider::Azure(Vec::new()),
                report,
            };
        }
    };

    let subscriptions = opts.azure_subscriptions.as_deref().unwrap_or_default();

    let collections = match client.query_graph(&arg_query(), subscriptions).await {
        Ok(rows) => {
            let (collections, mapping) = map_resources(rows);
            report.merge(mapping);
            collections
        }
        Err(e) => {
            report.record(SOURCE, FailureKind::Unavailable, "resource_graph", e);
            Vec::new()
        }
    };

    ProviderScan {
        provider: Provider::Azure(collections),
        report,
    }
}

macro_rules! azure_types {
    ($($variant:ident => $arg_type:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum AzureType {
            $($variant),+
        }

        impl AzureType {
            const ALL: &'static [AzureType] = &[$(AzureType::$variant),+];

            fn arg_type(self) -> &'static str {
                match self {
                    $(AzureType::$variant => $arg_type),+
                }
            }

            fn parse(raw: &str) -> Option<Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|t| t.arg_type().eq_ignore_ascii_case(raw))
            }
        }
    };
}

azure_types! {
    VirtualMachines => "microsoft.compute/virtualmachines",
    VirtualNetworks => "microsoft.network/virtualnetworks",
    NetworkSecurityGroups => "microsoft.network/networksecuritygroups",
    PublicIpAddresses => "microsoft.network/publicipaddresses",
    StorageAccounts => "microsoft.storage/storageaccounts",
    ManagedClusters => "microsoft.containerservice/managedclusters",
    SqlServers => "microsoft.sql/servers",
    Sites => "microsoft.web/sites",
    ApiManagement => "microsoft.apimanagement/service",
    CosmosDbs => "microsoft.documentdb/databaseaccounts",
    ServiceBuses => "microsoft.servicebus/namespaces",
    EventGridTopics => "microsoft.eventgrid/topics",
    DnsZones => "microsoft.network/dnszones",
    CdnProfiles => "microsoft.cdn/profiles",
}

fn arg_query() -> String {
    let types = AzureType::ALL
        .iter()
        .map(|t| format!("\"{}\"", t.arg_type()))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "Resources
        | where type in~ ({types})
        | project id, name, type, location, kind, properties"
    )
}

pub fn map_resources(
    raw_resources: Vec<serde_json::Value>,
) -> (Vec<MicrosoftCollection>, CollectionReport) {
    let mut vms = Vec::new();
    let mut vnets = Vec::new();
    let mut subnets = Vec::new();
    let mut nsgs = Vec::new();
    let mut pips = Vec::new();
    let mut storage = Vec::new();
    let mut aks = Vec::new();
    let mut sql = Vec::new();
    let mut apps = Vec::new();
    let mut funcs = Vec::new();
    let mut apims = Vec::new();
    let mut cosmos = Vec::new();
    let mut sbuses = Vec::new();
    let mut egrids = Vec::new();
    let mut dns = Vec::new();
    let mut cdns = Vec::new();

    macro_rules! leaf {
        ($list:ident, $ty:ident, $res:expr) => {{
            let res = $res;
            $list.push($ty {
                id: res.id,
                name: res.name,
                location: res.location,
            })
        }};
    }

    let mut unmapped: Vec<(String, String)> = Vec::new();

    for res_val in raw_resources {
        let res: AzureResource = match AzureResource::deserialize(&res_val) {
            Ok(res) => res,
            Err(e) => {
                let id = res_val
                    .get("id")
                    .and_then(|id| id.as_str())
                    .unwrap_or("<unidentified row>");
                unmapped.push((id.to_owned(), e.to_string()));
                continue;
            }
        };

        let Some(azure_type) = AzureType::parse(res.r#type.as_deref().unwrap_or("")) else {
            continue;
        };

        match azure_type {
            AzureType::VirtualMachines => {
                let mut nic_ids = Vec::new();
                if let Some(props) = &res.properties
                    && let Some(profile) = props.get("networkProfile")
                    && let Some(nics) = profile.get("networkInterfaces").and_then(|n| n.as_array())
                {
                    for nic in nics {
                        if let Some(nic_id) = nic.get("id").and_then(|id| id.as_str()) {
                            nic_ids.push(nic_id.to_string());
                        }
                    }
                }
                vms.push(VirtualMachine {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                    network_interfaces: nic_ids,
                });
            }
            AzureType::VirtualNetworks => {
                let mut subnet_ids = Vec::new();
                if let Some(props) = &res.properties
                    && let Some(subnets_arr) = props.get("subnets").and_then(|s| s.as_array())
                {
                    for sub in subnets_arr {
                        if let Some(sub_id) = sub.get("id").and_then(|id| id.as_str()) {
                            subnet_ids.push(sub_id.to_string());
                        }

                        let sub_nsg = sub
                            .get("properties")
                            .and_then(|p| p.get("networkSecurityGroup"))
                            .and_then(|nsg| nsg.get("id"))
                            .and_then(|id| id.as_str())
                            .map(|s| s.to_string());

                        subnets.push(Subnet {
                            id: sub
                                .get("id")
                                .and_then(|id| id.as_str())
                                .map(|s| s.to_string()),
                            name: sub
                                .get("name")
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string()),
                            vnet_id: res.id.clone(),
                            network_security_group_id: sub_nsg,
                        });
                    }
                }
                vnets.push(VirtualNetwork {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                    subnets: subnet_ids,
                });
            }
            AzureType::NetworkSecurityGroups => {
                let properties = res.properties.as_ref().and_then(properties_of);
                nsgs.push(NetworkSecurityGroup {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                    properties,
                });
            }
            AzureType::PublicIpAddresses => {
                let ip_addr = res
                    .properties
                    .as_ref()
                    .and_then(|p| p.get("ipAddress"))
                    .and_then(|ip| ip.as_str())
                    .map(|s| s.to_string());
                pips.push(PublicIpAddress {
                    id: res.id,
                    name: res.name,
                    ip_address: ip_addr,
                });
            }
            AzureType::StorageAccounts => leaf!(storage, StorageAccount, res),
            AzureType::ManagedClusters => leaf!(aks, ManagedCluster, res),
            AzureType::SqlServers => leaf!(sql, SqlServer, res),
            AzureType::Sites => {
                let kind = res.kind.as_deref().unwrap_or("");
                if kind.contains("functionapp") {
                    leaf!(funcs, FunctionApp, res);
                } else {
                    let properties = res.properties.as_ref().and_then(properties_of);
                    apps.push(AppService {
                        id: res.id,
                        name: res.name,
                        location: res.location,
                        properties,
                    });
                }
            }
            AzureType::ApiManagement => leaf!(apims, ApiManagement, res),
            AzureType::CosmosDbs => leaf!(cosmos, CosmosDb, res),
            AzureType::ServiceBuses => leaf!(sbuses, ServiceBus, res),
            AzureType::EventGridTopics => leaf!(egrids, EventGridTopic, res),
            AzureType::DnsZones => leaf!(dns, DnsZone, res),
            AzureType::CdnProfiles => leaf!(cdns, CdnProfile, res),
        }
    }

    let collections = vec![
        MicrosoftCollection::AzureVirtualMachines(vms),
        MicrosoftCollection::AzureVirtualNetworks(vnets),
        MicrosoftCollection::AzureSubnets(subnets),
        MicrosoftCollection::AzureNetworkSecurityGroups(nsgs),
        MicrosoftCollection::AzurePublicIpAddresses(pips),
        MicrosoftCollection::AzureStorageAccounts(storage),
        MicrosoftCollection::AzureManagedClusters(aks),
        MicrosoftCollection::AzureSqlServers(sql),
        MicrosoftCollection::AzureAppServices(apps),
        MicrosoftCollection::AzureFunctionApps(funcs),
        MicrosoftCollection::AzureApiManagement(apims),
        MicrosoftCollection::AzureCosmosDbs(cosmos),
        MicrosoftCollection::AzureServiceBuses(sbuses),
        MicrosoftCollection::AzureEventGridTopics(egrids),
        MicrosoftCollection::AzureDnsZones(dns),
        MicrosoftCollection::AzureCdnProfiles(cdns),
    ];

    let mut report = CollectionReport::default();
    let remaining = unmapped.len().saturating_sub(MAX_REPORTED_ROWS);
    for (id, error) in unmapped.into_iter().take(MAX_REPORTED_ROWS) {
        report.note(
            SOURCE,
            FailureKind::Malformed,
            format!("resource_graph/row {id}"),
            error,
        );
    }
    if remaining > 0 {
        report.note(
            SOURCE,
            FailureKind::Malformed,
            "resource_graph/rows",
            format!("{remaining} further rows could not be mapped"),
        );
    }

    (collections, report)
}

fn properties_of<T: serde::de::DeserializeOwned>(props: &serde_json::Value) -> Option<T> {
    T::deserialize(props).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::graph_builder::GraphBuilder;
    use crate::atlas::projector::azure::azure_projector;

    #[test]
    fn a_flood_of_bad_rows_is_capped_in_the_report() {
        let malformed: Vec<serde_json::Value> = (0..MAX_REPORTED_ROWS + 2)
            .map(|i| serde_json::json!({ "id": format!("row-{i}"), "name": i }))
            .collect();

        let (_collections, report) = map_resources(malformed);

        assert_eq!(
            report.failures.len(),
            MAX_REPORTED_ROWS + 1,
            "expected {MAX_REPORTED_ROWS} rows plus one summary"
        );
        assert!(
            report.summary().contains("2 further rows"),
            "the tail must be counted: {}",
            report.summary()
        );
    }

    #[test]
    fn every_queried_type_reaches_the_graph() {
        for azure_type in AzureType::ALL {
            let row = serde_json::json!({
                "id": format!("/subscriptions/s/providers/{}/r1", azure_type.arg_type()),
                "name": "r1",
                "type": azure_type.arg_type(),
                "location": "eastus",
                "properties": {}
            });

            let (collections, _) = map_resources(vec![row]);
            let mut builder = GraphBuilder::new();
            azure_projector(&mut builder, &collections);

            assert!(
                builder.graph.node_count() > 0,
                "{} mapped to no graph nodes",
                azure_type.arg_type()
            );
        }
    }
}
