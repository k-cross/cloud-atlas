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
    pub next_page_token: Option<String>,
}

pub async fn list_functions(
    client: &GoogleApiClient,
    project: &str,
) -> Result<Vec<CloudFunction>, Box<dyn std::error::Error>> {
    let mut all_functions = Vec::new();
    let mut page_token = None;

    // Cloud Functions v2 uses locations/- for all locations
    let base_url = format!(
        "https://cloudfunctions.googleapis.com/v2/projects/{}/locations/-/functions",
        project
    );

    loop {
        let mut url = base_url.clone();
        if let Some(token) = &page_token {
            url.push_str(&format!("?pageToken={}", token));
        }

        let req = client.get(&url);
        let res = req.send().await?;

        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("GCP API error (functions): {}", text).into());
        }

        let parsed: FunctionsListResponse = serde_json::from_str(&text)?;

        if let Some(items) = parsed.functions {
            all_functions.extend(items);
        }

        if let Some(token) = parsed.next_page_token {
            page_token = Some(token);
        } else {
            break;
        }
    }

    Ok(all_functions)
}
