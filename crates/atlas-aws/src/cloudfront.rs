pub mod collector {
    use aws_sdk_cloudfront::Client;
    use aws_sdk_cloudfront::types::DistributionSummary;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<DistributionSummary>, Box<dyn std::error::Error>> {
        Ok(Client::new(config)
            .list_distributions()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?)
    }
}
