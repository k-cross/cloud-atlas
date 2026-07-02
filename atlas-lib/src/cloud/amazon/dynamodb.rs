pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_dynamodb::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut tables = Vec::new();
        let mut last_eval = None;

        loop {
            let mut req = client.list_tables();
            if let Some(eval) = &last_eval {
                req = req.exclusive_start_table_name(eval);
            }

            let resp = req.send().await?;
            for t in resp.table_names() {
                tables.push(t.to_string());
            }

            last_eval = resp.last_evaluated_table_name().map(|s| s.to_string());
            if last_eval.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonDynamoDb(tables))
    }
}
