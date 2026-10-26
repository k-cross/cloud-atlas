use crate::api::google::client::GoogleApiClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Router {
    pub id: Option<String>,
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub network: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Interconnect {
    pub id: Option<String>,
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InterconnectAttachment {
    pub id: Option<String>,
    pub name: Option<String>,
    pub self_link: Option<String>,
    pub state: Option<String>,
    pub router: Option<String>,
    pub interconnect: Option<String>,
    pub cloud_router_ip_address: Option<String>,
    pub customer_router_ip_address: Option<String>,
    pub cloud_router_ipv6_address: Option<String>,
    pub customer_router_ipv6_address: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouterAggregatedList {
    items: Option<HashMap<String, RoutersScopedList>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutersScopedList {
    routers: Option<Vec<Router>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InterconnectList {
    items: Option<Vec<Interconnect>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InterconnectAttachmentAggregatedList {
    items: Option<HashMap<String, InterconnectAttachmentsScopedList>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InterconnectAttachmentsScopedList {
    interconnect_attachments: Option<Vec<InterconnectAttachment>>,
}

pub async fn list_routers(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Router>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!("/compute/v1/projects/{}/aggregated/routers", project),
    );
    client
        .paginated_list(&url, "routers", |r: RouterAggregatedList| {
            r.items.map(|items| {
                items
                    .into_values()
                    .filter_map(|scoped| scoped.routers)
                    .flatten()
                    .collect()
            })
        })
        .await
}

pub async fn list_interconnects(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Interconnect>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!("/compute/v1/projects/{}/global/interconnects", project),
    );
    client
        .paginated_list(&url, "interconnects", |r: InterconnectList| r.items)
        .await
}

pub async fn list_interconnect_attachments(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<InterconnectAttachment>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        "https://compute.googleapis.com",
        &format!(
            "/compute/v1/projects/{}/aggregated/interconnectAttachments",
            project
        ),
    );
    client
        .paginated_list(
            &url,
            "interconnectAttachments",
            |r: InterconnectAttachmentAggregatedList| {
                r.items.map(|items| {
                    items
                        .into_values()
                        .filter_map(|scoped| scoped.interconnect_attachments)
                        .flatten()
                        .collect()
                })
            },
        )
        .await
}
