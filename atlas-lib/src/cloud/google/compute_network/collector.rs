use crate::api::google::client::GoogleApiClient;
use crate::api::google::compute_network;
use crate::cloud::definition::GoogleCollection;
use tracing::warn;

pub async fn runner(
    project_id: &str,
    client: &GoogleApiClient,
) -> Result<(GoogleCollection, GoogleCollection, GoogleCollection), Box<dyn std::error::Error>> {
    let mut networks = Vec::new();
    let mut subnetworks = Vec::new();
    let mut forwarding_rules = Vec::new();

    match compute_network::list_networks(client, project_id).await {
        Ok(items) => {
            networks = items;
        }
        Err(e) => {
            warn!(
                "Failed to fetch GCP Networks for project {}: {}",
                project_id, e
            );
        }
    }

    match compute_network::list_subnetworks(client, project_id).await {
        Ok(items) => {
            subnetworks = items;
        }
        Err(e) => {
            warn!(
                "Failed to fetch GCP Subnetworks for project {}: {}",
                project_id, e
            );
        }
    }

    match compute_network::list_forwarding_rules(client, project_id).await {
        Ok(items) => {
            forwarding_rules = items;
        }
        Err(e) => {
            warn!(
                "Failed to fetch GCP Forwarding Rules for project {}: {}",
                project_id, e
            );
        }
    }

    Ok((
        GoogleCollection::GoogleNetworks(networks),
        GoogleCollection::GoogleSubnetworks(subnetworks),
        GoogleCollection::GoogleForwardingRules(forwarding_rules),
    ))
}
