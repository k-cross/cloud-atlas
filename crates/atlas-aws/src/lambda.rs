pub mod collector {
    use aws_sdk_lambda::Client;
    use aws_sdk_lambda::types::FunctionConfiguration;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<FunctionConfiguration>, Box<dyn std::error::Error>> {
        Ok(Client::new(config)
            .list_functions()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?)
    }
}
