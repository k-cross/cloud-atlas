pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_eks::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut clusters = Vec::new();
        let mut next_token = None;

        loop {
            let mut req = client.list_clusters();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }

            let resp = req.send().await?;
            for name in resp.clusters() {
                let cluster_resp = client.describe_cluster().name(name).send().await?;
                if let Some(cluster) = cluster_resp.cluster() {
                    clusters.push(cluster.clone());
                }
            }

            next_token = resp.next_token().map(|s| s.to_string());
            if next_token.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonEks(clusters))
    }
}
