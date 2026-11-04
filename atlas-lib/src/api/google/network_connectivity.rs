use crate::api::google::client::GoogleApiClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Hub {
    pub name: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Spoke {
    pub name: Option<String>,
    pub hub: Option<String>,
    pub group: Option<String>,
    pub state: Option<String>,
    pub linked_vpc_network: Option<LinkedVpcNetwork>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LinkedVpcNetwork {
    pub uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HubList {
    hubs: Option<Vec<Hub>>,
}

#[derive(Debug, Deserialize)]
struct SpokeList {
    spokes: Option<Vec<Spoke>>,
}

const HOST: &str = "https://networkconnectivity.googleapis.com";

pub async fn list_hubs(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Hub>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        HOST,
        &format!("/v1/projects/{project}/locations/global/hubs"),
    );
    client
        .paginated_list(&url, "hubs", |list: HubList| list.hubs)
        .await
}

pub async fn list_spokes(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<Spoke>, Box<dyn std::error::Error>> {
    let url = client.endpoint(
        HOST,
        &format!("/v1/projects/{project}/locations/global/spokes"),
    );
    client
        .paginated_list(&url, "spokes", |list: SpokeList| list.spokes)
        .await
}
