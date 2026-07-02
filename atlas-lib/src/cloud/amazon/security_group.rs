pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_ec2::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut groups = Vec::new();
        let mut next_token = None;

        loop {
            let mut req = client.describe_security_groups();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }

            let resp = req.send().await?;
            groups.extend(resp.security_groups().to_vec());

            next_token = resp.next_token().map(|s| s.to_string());
            if next_token.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonSecurityGroups(groups))
    }
}
