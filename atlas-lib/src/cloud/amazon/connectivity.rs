pub mod collector {
    use crate::cloud::definition::AWSConnectivity;
    use aws_sdk_ec2::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AWSConnectivity, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        Ok(AWSConnectivity {
            vpcs: client
                .describe_vpcs()
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await?,
            peering_connections: client
                .describe_vpc_peering_connections()
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await?,
            transit_gateways: client
                .describe_transit_gateways()
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await?,
            transit_gateway_attachments: client
                .describe_transit_gateway_attachments()
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await?,
            vpn_gateways: client
                .describe_vpn_gateways()
                .send()
                .await?
                .vpn_gateways
                .unwrap_or_default(),
            customer_gateways: client
                .describe_customer_gateways()
                .send()
                .await?
                .customer_gateways
                .unwrap_or_default(),
            vpn_connections: client
                .describe_vpn_connections()
                .send()
                .await?
                .vpn_connections
                .unwrap_or_default(),
        })
    }
}
