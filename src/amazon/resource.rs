pub mod collector {
    // dependencies
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_config::model::{ResourceType, ResourceIdentifier};
    use aws_sdk_config::{Client, Error, Region};

    // system
    use std::collections::HashMap;

    // Lists resources
    async fn scan_resources(verbose: bool, client: &Client) -> Result<HashMap<String, Vec<ResourceIdentifier>>, Error> {
        let mut r_map = HashMap::new();

        for value in ResourceType::values() {
            let parsed = ResourceType::from(*value);

            let resp = client
                .list_discovered_resources()
                .resource_type(parsed)
                .send()
                .await?;

            let resources = resp.resource_identifiers().unwrap_or_default();

            // grab exactly 1 of each type for now to discover more info about its structure
            if !resources.is_empty() {
                r_map.insert(value.to_string(), resources.to_owned());
            }
        }

        Ok(r_map)
    }

    pub async fn runner(verbose: bool, region: &str) -> Result<HashMap<String, Vec<ResourceIdentifier>>, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));

        if verbose {
            println!("Region: {}", region_provider.region().await.unwrap().as_ref());
            println!();
        }

        let shared_config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&shared_config);

        match scan_resources(verbose, &client).await {
            Ok(res) => Ok(res),
            Err(e) => Err(e.into()),
        }
    }
}
