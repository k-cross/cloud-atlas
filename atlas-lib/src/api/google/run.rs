use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceList {
    pub services: Option<Vec<Service>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub name: Option<String>,
    pub uid: Option<String>,
    pub uri: Option<String>,
}

impl super::client::GoogleApiClient {
    pub async fn list_run_services(
        &self,
        project_id: &str,
    ) -> Result<Vec<Service>, Box<dyn std::error::Error>> {
        let url = format!(
            "https://run.googleapis.com/v2/projects/{}/locations/-/services",
            project_id
        );
        let resp = self.client.get(&url).send().await?;

        if resp.status().is_success() {
            let list: ServiceList = resp.json().await?;
            Ok(list.services.unwrap_or_default())
        } else {
            let error_text = resp.text().await?;
            Err(format!("Cloud Run API Error: {}", error_text).into())
        }
    }
}
