use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketList {
    pub items: Option<Vec<Bucket>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub id: Option<String>,
    pub name: Option<String>,
    pub location: Option<String>,
    pub storage_class: Option<String>,
}

impl super::client::GoogleApiClient {
    pub async fn list_buckets(
        &self,
        project_id: &str,
    ) -> Result<Vec<Bucket>, Box<dyn std::error::Error>> {
        let url = format!(
            "https://storage.googleapis.com/storage/v1/b?project={}",
            project_id
        );
        let resp = self.client.get(&url).send().await?;

        if resp.status().is_success() {
            let list: BucketList = resp.json().await?;
            Ok(list.items.unwrap_or_default())
        } else {
            let error_text = resp.text().await?;
            Err(format!("GCS API Error: {}", error_text).into())
        }
    }
}
