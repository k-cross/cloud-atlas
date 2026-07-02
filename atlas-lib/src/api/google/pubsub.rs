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
        let url = format!(
            "https://pubsub.googleapis.com/v1/projects/{}/topics",
            project_id
        );
        let resp = self.client.get(&url).send().await?;

        if resp.status().is_success() {
            let list: TopicList = resp.json().await?;
            Ok(list.topics.unwrap_or_default())
        } else {
            let error_text = resp.text().await?;
            Err(format!("PubSub Topics API Error: {}", error_text).into())
        }
    }

    pub async fn list_subscriptions(
        &self,
        project_id: &str,
    ) -> Result<Vec<Subscription>, Box<dyn std::error::Error>> {
        let url = format!(
            "https://pubsub.googleapis.com/v1/projects/{}/subscriptions",
            project_id
        );
        let resp = self.client.get(&url).send().await?;

        if resp.status().is_success() {
            let list: SubscriptionList = resp.json().await?;
            Ok(list.subscriptions.unwrap_or_default())
        } else {
            let error_text = resp.text().await?;
            Err(format!("PubSub Subscriptions API Error: {}", error_text).into())
        }
    }
}
