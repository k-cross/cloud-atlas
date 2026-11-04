pub mod collector {
    use crate::cloud::definition::{
        AWSConnectivity, AWSTransitGatewayRouteTable, AWSTransitGatewayRouting,
    };
    use aws_sdk_ec2::Client;
    use aws_sdk_ec2::types::{Filter, TransitGatewayRouteTableState};
    use std::error::Error;

    pub async fn runner(config: &aws_config::SdkConfig) -> Result<AWSConnectivity, Box<dyn Error>> {
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

    pub async fn transit_gateway_routing(
        config: &aws_config::SdkConfig,
    ) -> Result<AWSTransitGatewayRouting, Box<dyn Error>> {
        let client = Client::new(config);

        Ok(AWSTransitGatewayRouting {
            route_tables: transit_gateway_route_tables(&client).await?,
            peerings: client
                .describe_transit_gateway_peering_attachments()
                .into_paginator()
                .items()
                .send()
                .try_collect()
                .await?,
        })
    }

    async fn transit_gateway_route_tables(
        client: &Client,
    ) -> Result<Vec<AWSTransitGatewayRouteTable>, Box<dyn Error>> {
        let tables: Vec<_> = client
            .describe_transit_gateway_route_tables()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?;
        let mut routing = Vec::new();
        for table in tables {
            let Some(table_id) = table
                .transit_gateway_route_table_id()
                .filter(|_| table.state() == Some(&TransitGatewayRouteTableState::Available))
                .map(str::to_owned)
            else {
                continue;
            };
            routing.push(AWSTransitGatewayRouteTable {
                associations: client
                    .get_transit_gateway_route_table_associations()
                    .transit_gateway_route_table_id(&table_id)
                    .into_paginator()
                    .items()
                    .send()
                    .try_collect()
                    .await?,
                propagations: client
                    .get_transit_gateway_route_table_propagations()
                    .transit_gateway_route_table_id(&table_id)
                    .into_paginator()
                    .items()
                    .send()
                    .try_collect()
                    .await?,
                static_routes: client
                    .search_transit_gateway_routes()
                    .transit_gateway_route_table_id(&table_id)
                    .filters(Filter::builder().name("type").values("static").build())
                    .filters(
                        Filter::builder()
                            .name("state")
                            .values("active")
                            .values("blackhole")
                            .build(),
                    )
                    .into_paginator()
                    .items()
                    .send()
                    .try_collect()
                    .await?,
                table,
            });
        }
        Ok(routing)
    }
}
