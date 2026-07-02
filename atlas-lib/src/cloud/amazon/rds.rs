pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_rds::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut dbs = Vec::new();
        let mut marker = None;

        loop {
            let mut req = client.describe_db_instances();
            if let Some(m) = &marker {
                req = req.marker(m);
            }

            let resp = req.send().await?;
            dbs.extend(resp.db_instances().to_vec());

            marker = resp.marker().map(|s| s.to_string());
            if marker.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonRds(dbs))
    }
}
