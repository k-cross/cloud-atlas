pub mod collector {
    use aws_sdk_rds::Client;
    use aws_sdk_rds::types::DbInstance;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<DbInstance>, Box<dyn std::error::Error>> {
        Ok(Client::new(config)
            .describe_db_instances()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?)
    }
}
