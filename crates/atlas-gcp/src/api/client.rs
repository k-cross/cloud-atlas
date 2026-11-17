use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use std::error::Error;
use std::fmt;
use tokio::sync::OnceCell;

const RESOURCE_MANAGER: &str = "https://cloudresourcemanager.googleapis.com";

#[derive(Clone)]
pub struct GoogleApiClient {
    pub client: Client,
    pub token: String,
    pub base_url: Option<String>,
}

impl GoogleApiClient {
    pub fn new(token: String) -> Self {
        Self {
            client: Client::new(),
            token,
            base_url: None,
        }
    }

    pub fn with_base_url(token: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            token,
            base_url: Some(base_url),
        }
    }

    pub fn endpoint(&self, default_host: &str, path: &str) -> String {
        let base = self.base_url.as_deref().unwrap_or(default_host);
        format!("{base}{path}")
    }

    pub fn get(&self, url: &str) -> RequestBuilder {
        self.client.get(url).bearer_auth(&self.token)
    }

    async fn read(&self, url: &str, context: &str) -> Result<String, Box<dyn Error>> {
        let res = self.get(url).send().await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            return Err(ApiError {
                context: context.to_owned(),
                body,
            }
            .into());
        }
        Ok(body)
    }

    async fn project_name(&self, project: &str) -> Result<String, Box<dyn Error>> {
        #[derive(Deserialize)]
        struct Project {
            name: Option<String>,
        }
        let url = self.endpoint(RESOURCE_MANAGER, &format!("/v3/projects/{project}"));
        let found: Project = serde_json::from_str(&self.read(&url, "project").await?)?;
        found
            .name
            .ok_or_else(|| format!("Resource Manager named no project number for {project}").into())
    }

    pub async fn paginated_list<T, I, F>(
        &self,
        base_url: &str,
        context: &str,
        extract: F,
    ) -> Result<Vec<I>, Box<dyn Error>>
    where
        T: serde::de::DeserializeOwned,
        F: Fn(T) -> Option<Vec<I>>,
    {
        let mut all_items = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let url = match &page_token {
                Some(token) => {
                    let sep = if base_url.contains('?') { '&' } else { '?' };
                    let encoded: String =
                        url::form_urlencoded::byte_serialize(token.as_bytes()).collect();
                    format!("{}{}pageToken={}", base_url, sep, encoded)
                }
                None => base_url.to_owned(),
            };

            let text = self.read(&url, context).await?;
            let value: serde_json::Value = serde_json::from_str(&text)?;
            page_token = value
                .get("nextPageToken")
                .and_then(|t| t.as_str())
                .map(String::from);

            if let Some(items) = extract(serde_json::from_value(value)?) {
                all_items.extend(items);
            }

            if page_token.is_none() {
                return Ok(all_items);
            }
        }
    }
}

#[derive(Debug)]
pub struct ApiError {
    context: String,
    body: String,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GCP API error ({}): {}", self.context, self.body)
    }
}

impl Error for ApiError {}

impl ApiError {
    fn disabled_consumer(&self) -> Option<String> {
        let body: serde_json::Value = serde_json::from_str(&self.body).ok()?;
        body.get("error")?
            .get("details")?
            .as_array()?
            .iter()
            .find(|detail| {
                detail
                    .get("@type")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind.ends_with("google.rpc.ErrorInfo"))
                    && detail.get("reason").and_then(|reason| reason.as_str())
                        == Some("SERVICE_DISABLED")
            })?
            .get("metadata")?
            .get("consumer")?
            .as_str()
            .map(str::to_owned)
    }
}

pub struct ProjectApis<'a> {
    client: &'a GoogleApiClient,
    project: &'a str,
    name: OnceCell<Result<String, String>>,
}

impl<'a> ProjectApis<'a> {
    pub fn new(client: &'a GoogleApiClient, project: &'a str) -> Self {
        Self {
            client,
            project,
            name: OnceCell::new(),
        }
    }

    pub async fn unless_disabled<T: Default>(
        &self,
        collection: impl Future<Output = Result<T, Box<dyn Error>>>,
    ) -> Result<T, Box<dyn Error>> {
        let error = match collection.await {
            Err(error) => error,
            listed => return listed,
        };
        let Some(consumer) = error
            .downcast_ref::<ApiError>()
            .and_then(ApiError::disabled_consumer)
        else {
            return Err(error);
        };
        let name = self
            .name
            .get_or_init(|| async {
                self.client
                    .project_name(self.project)
                    .await
                    .map_err(|lookup| lookup.to_string())
            })
            .await;
        match name {
            Ok(name) if *name == consumer => Ok(T::default()),
            Ok(_) => Err(error),
            Err(lookup) => Err(format!(
                "{error} (could not tell whether the API is off in {}: {lookup})",
                self.project
            )
            .into()),
        }
    }
}
