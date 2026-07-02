use crate::Settings;
use crate::api::azure::client::AzureApiClient;
use crate::api::azure::models::*;
use crate::cloud::definition::{MicrosoftCollection, Provider};

pub async fn build_azure(
    _verbose: bool,
    _opts: &Settings,
) -> Result<Provider, Box<dyn std::error::Error>> {
    let client = AzureApiClient::new().await?;

    let subscriptions = vec![]; // We can fetch subscriptions or the user can provide them.
    // Wait, ARG can query all subscriptions the user has access to if we pass an empty array or omit it.
    // Let's pass an empty array to ARG to query across the entire tenant.

    let query = r#"
        Resources
        | where type in~ (
            "microsoft.compute/virtualmachines",
            "microsoft.network/virtualnetworks",
            "microsoft.network/networksecuritygroups",
            "microsoft.network/publicipaddresses",
            "microsoft.storage/storageaccounts",
            "microsoft.containerservice/managedclusters",
            "microsoft.sql/servers",
            "microsoft.web/sites",
            "microsoft.apimanagement/service",
            "microsoft.documentdb/databaseaccounts",
            "microsoft.servicebus/namespaces",
            "microsoft.eventgrid/topics",
            "microsoft.network/dnszones",
            "microsoft.cdn/profiles"
        )
        | project id, name, type, location, properties
    "#;

    let raw_resources = client.query_graph(query, &subscriptions).await?;

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

    for res_val in raw_resources {
        let res: AzureResource = serde_json::from_value(res_val)?;
        let r_type = res.r#type.as_deref().unwrap_or("").to_lowercase();

        match r_type.as_str() {
            "microsoft.compute/virtualmachines" => {
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
            "microsoft.network/virtualnetworks" => {
                let mut subnet_ids = Vec::new();
                if let Some(props) = &res.properties
                    && let Some(subnets_arr) = props.get("subnets").and_then(|s| s.as_array())
                {
                    for sub in subnets_arr {
                        if let Some(sub_id) = sub.get("id").and_then(|id| id.as_str()) {
                            subnet_ids.push(sub_id.to_string());
                        }

                        // Extract subnet object directly since ARG returns it inline in the VNet properties
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
            "microsoft.network/networksecuritygroups" => {
                let mut properties = None;
                if let Some(props_val) = &res.properties
                    && let Ok(p) = serde_json::from_value(props_val.clone())
                {
                    properties = Some(p);
                }
                nsgs.push(NetworkSecurityGroup {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                    properties,
                });
            }
            "microsoft.network/publicipaddresses" => {
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
            "microsoft.storage/storageaccounts" => {
                storage.push(StorageAccount {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            "microsoft.containerservice/managedclusters" => {
                aks.push(ManagedCluster {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            "microsoft.sql/servers" => {
                sql.push(SqlServer {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            "microsoft.web/sites" => {
                let kind = res
                    .properties
                    .as_ref()
                    .and_then(|p| p.get("kind"))
                    .and_then(|k| k.as_str())
                    .unwrap_or("");
                if kind.contains("functionapp") {
                    funcs.push(FunctionApp {
                        id: res.id,
                        name: res.name,
                        location: res.location,
                    });
                } else {
                    let mut properties = None;
                    if let Some(props_val) = &res.properties
                        && let Ok(p) = serde_json::from_value(props_val.clone())
                    {
                        properties = Some(p);
                    }
                    apps.push(AppService {
                        id: res.id,
                        name: res.name,
                        location: res.location,
                        properties,
                    });
                }
            }
            "microsoft.apimanagement/service" => {
                apims.push(ApiManagement {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            "microsoft.documentdb/databaseaccounts" => {
                cosmos.push(CosmosDb {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            "microsoft.servicebus/namespaces" => {
                sbuses.push(ServiceBus {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            "microsoft.eventgrid/topics" => {
                egrids.push(EventGridTopic {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            "microsoft.network/dnszones" => {
                dns.push(DnsZone {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            "microsoft.cdn/profiles" => {
                cdns.push(CdnProfile {
                    id: res.id,
                    name: res.name,
                    location: res.location,
                });
            }
            _ => {}
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

    Ok(Provider::Azure(collections))
}
