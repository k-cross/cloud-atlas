pub mod collector {
    use aws_sdk_ecs::Client;
    use aws_sdk_ecs::types::Cluster;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<Cluster>, Box<dyn std::error::Error>> {
        let client = Client::new(config);
        let resp = client.describe_clusters().send().await?;
        Ok(resp.clusters.unwrap_or_default())
    }
}
