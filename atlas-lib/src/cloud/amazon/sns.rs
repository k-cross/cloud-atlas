pub mod collector {
    use aws_sdk_sns::Client;
    use aws_sdk_sns::types::Topic;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<Topic>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut topics = Vec::new();
        let mut next_token = None;

        loop {
            let mut req = client.list_topics();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }

            let mut resp = req.send().await?;
            next_token = resp.next_token.take();
            topics.extend(resp.topics.take().unwrap_or_default());
            if next_token.is_none() {
                break;
            }
        }

        Ok(topics)
    }
}
