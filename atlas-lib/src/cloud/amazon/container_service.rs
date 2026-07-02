pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_ecs::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);
        let resp = client.describe_clusters().send().await?;
        Ok(AmazonCollection::AmazonClusters(
            resp.clusters.unwrap_or_default(),
        ))
    }
}
