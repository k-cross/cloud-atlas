pub mod collector {
    use crate::cloud::definition::TableName;
    use aws_sdk_dynamodb::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<TableName>, Box<dyn std::error::Error>> {
        let names: Vec<String> = Client::new(config)
            .list_tables()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?;
        Ok(names.into_iter().map(TableName).collect())
    }
}
