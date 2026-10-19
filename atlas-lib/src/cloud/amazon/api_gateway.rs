pub mod collector {
    use aws_sdk_apigateway::Client;
    use aws_sdk_apigateway::types::RestApi;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<RestApi>, Box<dyn std::error::Error>> {
        Ok(Client::new(config)
            .get_rest_apis()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?)
    }
}
