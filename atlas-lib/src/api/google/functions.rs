use super::client::GoogleApiClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloudFunction {
    pub name: Option<String>,
    pub environment: Option<String>,
    pub build_config: Option<BuildConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BuildConfig {
    pub runtime: Option<String>,
    pub entry_point: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionsListResponse {
    pub functions: Option<Vec<CloudFunction>>,
}

pub async fn list_functions(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<CloudFunction>, Box<dyn std::error::Error>> {
    // Cloud Functions v2 uses locations/- for all locations
    let url = format!(
        "https://cloudfunctions.googleapis.com/v2/projects/{}/locations/-/functions",
        project
    );
    client
        .paginated_list(&url, "functions", |r: FunctionsListResponse| r.functions)
        .await
}
