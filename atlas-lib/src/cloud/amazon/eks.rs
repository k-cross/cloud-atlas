pub mod collector {
    use aws_sdk_eks::Client;
    use aws_sdk_eks::types::Cluster;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<Cluster>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let names: Vec<String> = client
            .list_clusters()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?;

        let mut clusters = Vec::new();
        for name in &names {
            let cluster_resp = client.describe_cluster().name(name).send().await?;
            if let Some(cluster) = cluster_resp.cluster {
                clusters.push(cluster);
            }
        }

        Ok(clusters)
    }
}
