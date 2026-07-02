use crate::api::google::client::GoogleApiClient;
use crate::cloud::definition::GoogleCollection;
use tracing::warn;

pub async fn runner(
    project_id: &str,
    client: &GoogleApiClient,
) -> Result<GoogleCollection, Box<dyn std::error::Error>> {
    let mut buckets = Vec::new();

    match client.list_buckets(project_id).await {
        Ok(items) => {
            buckets = items;
        }
        Err(e) => {
            warn!(
                "Failed to fetch GCS buckets for project {}: {}",
                project_id, e
            );
        }
    }

    Ok(GoogleCollection::GoogleStorageBuckets(buckets))
}
