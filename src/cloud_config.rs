pub mod collector {
    // dependencies
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_config::model::ResourceType;
    use aws_sdk_config::{Client, Error, Region};

    // system
    use std::collections::HashMap;

    // Lists your resources.
    // snippet-start:[config.rust.list-resources]
    async fn show_resources(verbose: bool, client: &Client) -> Result<(), Error> {
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
            if !resources.is_empty() || verbose {
                println!();
                println!("Resources of type {}:", value);
                r_map.insert(value.to_string(), resources.get(0).unwrap().clone());
            }
            
            //for resource in resources {

            //    println!(
            //        "  Resource ID: {}",
            //        resource.resource_id().as_deref().unwrap_or_default()
            //    );
            //}
        }

        println!("{:#?}", r_map);

        Ok(())
    }

    pub async fn run(verbose: bool, region: String) -> Result<(), Error> {
        let region_provider = RegionProviderChain::first_try(Region::new(region))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        println!();

        if verbose {
            println!(
                "Region:                {}",
                region_provider.region().await.unwrap().as_ref()
            );

            println!();
        }

        let shared_config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&shared_config);

        if !verbose {
            println!("You won't see any output if you don't have any resources defined in the region.");
        }

        show_resources(verbose, &client).await
    }
}