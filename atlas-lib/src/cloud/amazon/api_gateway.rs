pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_apigateway::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut apis = Vec::new();
        let mut position = None;

        loop {
            let mut req = client.get_rest_apis();
            if let Some(pos) = &position {
                req = req.position(pos);
            }

            let resp = req.send().await?;
            apis.extend(resp.items().to_vec());

            position = resp.position().map(|s| s.to_string());
            if position.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonApiGateway(apis))
    }
}
