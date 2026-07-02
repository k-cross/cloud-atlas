pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_config::config::Region;
    use aws_sdk_rds::Client;

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));

        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;

        let client = Client::new(&shared_config);

        let mut dbs = Vec::new();
        let mut has_next = true;
        let mut marker = None;

        while has_next {
            let mut req = client.describe_db_instances();
            if let Some(m) = marker.clone() {
                req = req.marker(m);
            }

            let resp = req.send().await?;
            dbs.extend(resp.db_instances().to_vec());

            marker = resp.marker().map(|s| s.to_string());
            if marker.is_none() {
                has_next = false;
            }
        }

        Ok(AmazonCollection::AmazonRds(dbs))
    }
}
