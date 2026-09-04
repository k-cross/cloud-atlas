pub mod collector {
    use aws_sdk_config::types::{ResourceIdentifier, ResourceType};
    use aws_sdk_config::{Client, Error};
    use std::collections::HashMap;

    // Lists resources
    async fn scan_resources(
        client: &Client,
    ) -> Result<HashMap<String, Vec<ResourceIdentifier>>, Error> {
        let mut r_map = HashMap::new();

        use futures::stream::{self, StreamExt};

        let requests = stream::iter(ResourceType::values()).map(|value| {
            let parsed = ResourceType::from(*value);
            async move {
                let resp = client
                    .list_discovered_resources()
                    .resource_type(parsed)
                    .send()
                    .await;
                (value, resp)
            }
        });

        let mut results = requests.buffer_unordered(10);

        while let Some((value, resp_result)) = results.next().await {
            let resources = resp_result?.resource_identifiers.unwrap_or_default();

            if !resources.is_empty() {
                r_map.insert(value.to_string(), resources);
            }
        }

        Ok(r_map)
    }

    pub async fn runner(
        verbose: bool,
        config: &aws_config::SdkConfig,
    ) -> Result<HashMap<String, Vec<ResourceIdentifier>>, Box<dyn std::error::Error>> {
        if verbose && let Some(region) = config.region() {
            println!("Region: {}\n", region);
        }

        let client = Client::new(config);
        scan_resources(&client).await.map_err(Into::into)
    }
}
