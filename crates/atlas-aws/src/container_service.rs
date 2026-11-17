pub mod collector {
    use aws_sdk_ecs::Client;
    use aws_sdk_ecs::types::Cluster;

    const DESCRIBE_LIMIT: usize = 100;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<Cluster>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let arns: Vec<String> = client
            .list_clusters()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?;

        let mut clusters = Vec::new();
        for batch in arns.chunks(DESCRIBE_LIMIT) {
            let mut resp = client
                .describe_clusters()
                .set_clusters(Some(batch.to_vec()))
                .send()
                .await?;
            if let Some(failure) = resp
                .failures()
                .iter()
                .find(|failure| failure.reason() != Some("MISSING"))
            {
                return Err(format!(
                    "could not describe ECS cluster {}: {}",
                    failure.arn().unwrap_or("(no arn)"),
                    failure.reason().unwrap_or("no reason given")
                )
                .into());
            }
            clusters.extend(resp.clusters.take().unwrap_or_default());
        }

        Ok(clusters)
    }
}
