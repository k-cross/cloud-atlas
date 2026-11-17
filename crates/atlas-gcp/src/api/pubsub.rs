use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicList {
    pub topics: Option<Vec<Topic>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub name: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionList {
    pub subscriptions: Option<Vec<Subscription>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub name: Option<String>,
    pub topic: Option<String>,
}

impl super::client::GoogleApiClient {
    pub async fn list_topics(
        &self,
        project_id: &str,
    ) -> Result<Vec<Topic>, Box<dyn std::error::Error>> {
        let url = self.endpoint(
            "https://pubsub.googleapis.com",
            &format!("/v1/projects/{}/topics", project_id),
        );
        self.paginated_list(&url, "pubsub topics", |r: TopicList| r.topics)
            .await
    }

    pub async fn list_subscriptions(
        &self,
        project_id: &str,
    ) -> Result<Vec<Subscription>, Box<dyn std::error::Error>> {
        let url = self.endpoint(
            "https://pubsub.googleapis.com",
            &format!("/v1/projects/{}/subscriptions", project_id),
        );
        self.paginated_list(&url, "pubsub subscriptions", |r: SubscriptionList| {
            r.subscriptions
        })
        .await
    }
}
