pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_config::config::Region;
    use aws_sdk_ec2::Client;

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));

        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;

        let client = Client::new(&shared_config);

        let mut groups = Vec::new();
        let mut has_next = true;
        let mut next_token = None;

        while has_next {
            let mut req = client.describe_security_groups();
            if let Some(token) = next_token.clone() {
                req = req.next_token(token);
            }

            let resp = req.send().await?;
            for sg in resp.security_groups() {
                groups.push(sg.clone());
            }

            next_token = resp.next_token().map(|s| s.to_string());
            if next_token.is_none() {
                has_next = false;
            }
        }

        Ok(AmazonCollection::AmazonSecurityGroups(groups))
    }
}
