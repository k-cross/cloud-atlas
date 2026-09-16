pub mod collector {
    use aws_sdk_ec2::Client;
    use aws_sdk_ec2::types::NetworkInterface;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<NetworkInterface>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut interfaces = Vec::new();
        let mut next_token = None;
        loop {
            let mut req = client.describe_network_interfaces();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }
            let mut resp = req.send().await?;
            next_token = resp.next_token.take();
            interfaces.extend(resp.network_interfaces.take().unwrap_or_default());
            if next_token.is_none() {
                break;
            }
        }

        Ok(interfaces)
    }
}
