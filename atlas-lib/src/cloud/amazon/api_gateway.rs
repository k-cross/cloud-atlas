pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_apigateway::Client;
    use aws_sdk_config::config::Region;

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));

        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;

        let client = Client::new(&shared_config);

        let mut apis = Vec::new();
        let mut has_next = true;
        let mut position = None;

        while has_next {
            let mut req = client.get_rest_apis();
            if let Some(pos) = position.clone() {
                req = req.position(pos);
            }

            let resp = req.send().await?;
            apis.extend(resp.items().to_vec());

            position = resp.position().map(|s| s.to_string());
            if position.is_none() {
                has_next = false;
            }
        }

        Ok(AmazonCollection::AmazonApiGateway(apis))
    }
}
