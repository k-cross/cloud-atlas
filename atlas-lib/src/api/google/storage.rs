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
        let url = self.endpoint(
            "https://storage.googleapis.com",
            &format!("/storage/v1/b?project={}", project_id),
        );
        self.paginated_list(&url, "gcs", |r: BucketList| r.items)
            .await
    }
}
