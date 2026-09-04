pub mod collector {
    use crate::cloud::definition::TableName;
    use aws_sdk_dynamodb::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<TableName>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut tables = Vec::new();
        let mut last_eval = None;

        loop {
            let mut req = client.list_tables();
            if let Some(eval) = &last_eval {
                req = req.exclusive_start_table_name(eval);
            }

            let mut resp = req.send().await?;
            last_eval = resp.last_evaluated_table_name.take();
            tables.extend(
                resp.table_names
                    .take()
                    .unwrap_or_default()
                    .into_iter()
                    .map(TableName),
            );
            if last_eval.is_none() {
                break;
            }
        }

        Ok(tables)
    }
}
