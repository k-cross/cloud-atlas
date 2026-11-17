pub mod collector {
    use aws_sdk_sns::Client;
    use aws_sdk_sns::types::Topic;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<Topic>, Box<dyn std::error::Error>> {
        Ok(Client::new(config)
            .list_topics()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?)
    }
}
