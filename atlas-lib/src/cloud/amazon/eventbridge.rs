pub mod collector {
    use aws_sdk_eventbridge::Client;
    use aws_sdk_eventbridge::types::EventBus;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<EventBus>, Box<dyn std::error::Error>> {
        let client = Client::new(config);
        let resp = client.list_event_buses().send().await?;
        Ok(resp.event_buses.unwrap_or_default())
    }
}
