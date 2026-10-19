pub mod collector {
    use aws_sdk_eventbridge::Client;
    use aws_sdk_eventbridge::types::EventBus;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<EventBus>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut buses = Vec::new();
        let mut next_token: Option<String> = None;
        loop {
            let mut resp = client
                .list_event_buses()
                .set_next_token(next_token.clone())
                .send()
                .await?;
            buses.extend(resp.event_buses.take().unwrap_or_default());
            match resp.next_token.take().filter(|token| !token.is_empty()) {
                None => return Ok(buses),
                Some(token) if next_token.as_ref() == Some(&token) => {
                    return Err(
                        "EventBridge repeated a page token, so paging would never end".into(),
                    );
                }
                token => next_token = token,
            }
        }
    }
}
