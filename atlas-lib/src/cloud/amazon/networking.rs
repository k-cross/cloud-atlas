pub mod collector {
    use crate::cloud::definition::AWSNetworking;
    use aws_sdk_ec2::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AWSNetworking, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        Ok(AWSNetworking {
            route_tables: client
                .describe_route_tables()
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await?,
            internet_gateways: client
                .describe_internet_gateways()
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await?,
            nat_gateways: client
                .describe_nat_gateways()
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await?,
            addresses: client
                .describe_addresses()
                .send()
                .await?
                .addresses
                .unwrap_or_default(),
        })
    }
}
