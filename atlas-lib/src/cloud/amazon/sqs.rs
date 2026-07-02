pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_sqs::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut queues = Vec::new();
        let mut next_token = None;

        loop {
            let mut req = client.list_queues();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }

            let resp = req.send().await?;
            for q in resp.queue_urls() {
                queues.push(q.to_string());
            }

            next_token = resp.next_token().map(|s| s.to_string());
            if next_token.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonSqs(queues))
    }
}
