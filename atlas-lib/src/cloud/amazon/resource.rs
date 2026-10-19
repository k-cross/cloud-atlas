pub mod collector {
    use crate::atlas::projector::aws::use_aws_resource;
    use aws_sdk_config::types::{ResourceIdentifier, ResourceType};
    use aws_sdk_config::{Client, Error};
    use std::collections::HashMap;

    async fn scan_resources(
        client: &Client,
        exclude_by_default: bool,
    ) -> Result<HashMap<String, Vec<ResourceIdentifier>>, Box<dyn std::error::Error>> {
        let mut r_map = HashMap::new();

        use futures::stream::{self, StreamExt};

        let drawn = ResourceType::values()
            .iter()
            .filter(|value| use_aws_resource(value, exclude_by_default));
        let requests = stream::iter(drawn).map(|value| {
            let parsed = ResourceType::from(*value);
            async move {
                let resources: Result<Vec<ResourceIdentifier>, _> = client
                    .list_discovered_resources()
                    .resource_type(parsed)
                    .into_paginator()
                    .items()
                    .send()
                    .try_collect()
                    .await;
                (value, resources)
            }
        });

        let mut results = requests.buffer_unordered(10);

        while let Some((value, resources)) = results.next().await {
            let resources = resources.map_err(Error::from)?;

            if !resources.is_empty() {
                r_map.insert(value.to_string(), resources);
            }
        }

        Ok(r_map)
    }

    pub async fn runner(
        verbose: bool,
        exclude_by_default: bool,
        config: &aws_config::SdkConfig,
    ) -> Result<HashMap<String, Vec<ResourceIdentifier>>, Box<dyn std::error::Error>> {
        if verbose && let Some(region) = config.region() {
            println!("Region: {}\n", region);
        }

        let client = Client::new(config);
        scan_resources(&client, exclude_by_default).await
    }
}
