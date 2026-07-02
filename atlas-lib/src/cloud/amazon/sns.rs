pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_sns::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut topics = Vec::new();
        let mut next_token = None;

        loop {
            let mut req = client.list_topics();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }

            let resp = req.send().await?;
            topics.extend(resp.topics().to_vec());

            next_token = resp.next_token().map(|s| s.to_string());
            if next_token.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonSns(topics))
    }
}
