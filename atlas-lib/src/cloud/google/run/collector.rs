use crate::api::google::client::GoogleApiClient;
use crate::cloud::definition::GoogleCollection;
use tracing::warn;

pub async fn runner(
    project_id: &str,
    client: &GoogleApiClient,
) -> Result<GoogleCollection, Box<dyn std::error::Error>> {
    let mut services = Vec::new();

    match client.list_run_services(project_id).await {
        Ok(items) => {
            services = items;
        }
        Err(e) => {
            warn!(
                "Failed to fetch Cloud Run Services for project {}: {}",
                project_id, e
            );
        }
    }

    Ok(GoogleCollection::GoogleRunServices(services))
}
