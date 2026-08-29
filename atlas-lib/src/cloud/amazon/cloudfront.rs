pub mod collector {
    use aws_sdk_cloudfront::Client;
    use aws_sdk_cloudfront::types::DistributionSummary;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<DistributionSummary>, Box<dyn std::error::Error>> {
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

        Ok(distributions)
    }
}
