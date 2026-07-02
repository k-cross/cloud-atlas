pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_config::config::Region;
    use aws_sdk_dynamodb::Client;

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));

        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;

        let client = Client::new(&shared_config);

        let mut tables = Vec::new();
        let mut has_next = true;
        let mut last_eval = None;

        while has_next {
            let mut req = client.list_tables();
            if let Some(eval) = last_eval.clone() {
                req = req.exclusive_start_table_name(eval);
            }

            let resp = req.send().await?;
            for t in resp.table_names() {
                tables.push(t.to_string());
            }

            last_eval = resp.last_evaluated_table_name().map(|s| s.to_string());
            if last_eval.is_none() {
                has_next = false;
            }
        }

        Ok(AmazonCollection::AmazonDynamoDb(tables))
    }
}
