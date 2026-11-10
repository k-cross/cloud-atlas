pub mod collector {
    use crate::atlas::projector::aws::use_aws_resource;
    use aws_sdk_config::types::{ResourceIdentifier, ResourceType};
    use aws_sdk_config::{Client, Error};
    use std::collections::HashMap;

    async fn counted_types(
        client: &Client,
    ) -> Result<Vec<ResourceType>, Box<dyn std::error::Error>> {
        let pages: Vec<_> = client
            .get_discovered_resource_counts()
            .into_paginator()
            .send()
            .try_collect()
            .await
            .map_err(Error::from)?;

        Ok(pages
            .iter()
            .flat_map(|page| page.resource_counts())
            .filter(|counted| counted.count() > 0)
            .filter_map(|counted| counted.resource_type().cloned())
            .collect())
    }

    async fn scan_resources(
        client: &Client,
        exclude_by_default: bool,
    ) -> Result<HashMap<String, Vec<ResourceIdentifier>>, Box<dyn std::error::Error>> {
        let mut r_map = HashMap::new();

        use futures::stream::{self, StreamExt};

        let drawn = counted_types(client)
            .await?
            .into_iter()
            .filter(|resource_type| use_aws_resource(resource_type.as_str(), exclude_by_default));
        let requests = stream::iter(drawn).map(|resource_type| async move {
            let resources: Result<Vec<ResourceIdentifier>, _> = client
                .list_discovered_resources()
                .resource_type(resource_type.clone())
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await;
            (resource_type, resources)
        });

        let mut results = requests.buffer_unordered(10);

        while let Some((resource_type, resources)) = results.next().await {
            let resources = resources.map_err(Error::from)?;

            if !resources.is_empty() {
                r_map.insert(resource_type.as_str().to_owned(), resources);
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
