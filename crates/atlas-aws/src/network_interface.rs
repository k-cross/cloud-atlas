pub mod collector {
    use aws_sdk_ec2::Client;
    use aws_sdk_ec2::types::NetworkInterface;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<NetworkInterface>, Box<dyn std::error::Error>> {
        Ok(Client::new(config)
            .describe_network_interfaces()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?)
    }
}
