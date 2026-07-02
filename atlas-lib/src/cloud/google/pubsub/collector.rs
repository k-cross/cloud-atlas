use crate::api::google::client::GoogleApiClient;
use crate::cloud::definition::GoogleCollection;
use tracing::warn;

pub async fn runner(
    project_id: &str,
    client: &GoogleApiClient,
) -> Result<(GoogleCollection, GoogleCollection), Box<dyn std::error::Error>> {
    let mut topics = Vec::new();
    let mut subscriptions = Vec::new();

    match client.list_topics(project_id).await {
        Ok(items) => {
            topics = items;
        }
        Err(e) => {
            warn!(
                "Failed to fetch PubSub Topics for project {}: {}",
                project_id, e
            );
        }
    }

    match client.list_subscriptions(project_id).await {
        Ok(items) => {
            subscriptions = items;
        }
        Err(e) => {
            warn!(
                "Failed to fetch PubSub Subscriptions for project {}: {}",
                project_id, e
            );
        }
    }

    Ok((
        GoogleCollection::GooglePubSubTopics(topics),
        GoogleCollection::GooglePubSubSubscriptions(subscriptions),
    ))
}
