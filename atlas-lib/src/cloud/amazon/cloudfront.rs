pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_cloudfront::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut distributions = Vec::new();
        let mut marker = None;

        loop {
            let mut req = client.list_distributions();
            if let Some(m) = &marker {
                req = req.marker(m);
            }

            let resp = req.send().await?;
            let Some(list) = resp.distribution_list() else {
                break;
            };
            distributions.extend(list.items().to_vec());
            marker = list.next_marker().map(|s| s.to_string());
            if marker.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonCloudFront(distributions))
    }
}
