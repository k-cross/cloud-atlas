pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_ecs::types::Cluster;
    use aws_sdk_ecs::{Client, Error, config::Region};

    async fn get_clusters(client: &Client) -> Result<Vec<Cluster>, Error> {
        let resp = client.describe_clusters().send().await?;

        let cs = if let Some(clusters) = resp.clusters {
            clusters.to_owned()
        } else {
            Vec::new()
        };

        Ok(cs)
    }

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;
        let client = Client::new(&shared_config);

        match get_clusters(&client).await {
            Ok(res) => Ok(AmazonCollection::AmazonClusters(res)),
            Err(e) => Err(e.into()),
        }
    }
}
