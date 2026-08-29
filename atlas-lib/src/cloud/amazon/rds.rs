pub mod collector {
    use aws_sdk_rds::Client;
    use aws_sdk_rds::types::DbInstance;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<DbInstance>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut dbs = Vec::new();
        let mut marker = None;

        loop {
            let mut req = client.describe_db_instances();
            if let Some(m) = &marker {
                req = req.marker(m);
            }

            let resp = req.send().await?;
            dbs.extend(resp.db_instances().to_vec());

            marker = resp.marker().map(|s| s.to_string());
            if marker.is_none() {
                break;
            }
        }

        Ok(dbs)
    }
}
