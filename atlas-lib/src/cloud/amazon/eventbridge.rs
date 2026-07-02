pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_eventbridge::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);
        let resp = client.list_event_buses().send().await?;
        Ok(AmazonCollection::AmazonEventbridge(
            resp.event_buses.unwrap_or_default(),
        ))
    }
}
