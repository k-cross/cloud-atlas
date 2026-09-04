pub mod collector {
    use aws_sdk_apigateway::Client;
    use aws_sdk_apigateway::types::RestApi;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<RestApi>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut apis = Vec::new();
        let mut position = None;

        loop {
            let mut req = client.get_rest_apis();
            if let Some(pos) = &position {
                req = req.position(pos);
            }

            let mut resp = req.send().await?;
            position = resp.position.take();
            apis.extend(resp.items.take().unwrap_or_default());
            if position.is_none() {
                break;
            }
        }

        Ok(apis)
    }
}
