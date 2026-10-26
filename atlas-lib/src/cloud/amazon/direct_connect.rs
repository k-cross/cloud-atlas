pub mod collector {
    use crate::cloud::definition::{AWSDirectConnect, AWSDirectConnectGateways};
    use aws_sdk_directconnect::Client;
    use std::collections::HashSet;
    use std::error::Error;
    use std::future::Future;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AWSDirectConnect, Box<dyn Error>> {
        let client = &Client::new(config);

        let connections = every_page("DescribeConnections", |token| async move {
            let page = client
                .describe_connections()
                .set_next_token(token)
                .send()
                .await?;
            Ok((page.connections.unwrap_or_default(), page.next_token))
        })
        .await?;

        let virtual_interfaces = every_page("DescribeVirtualInterfaces", |token| async move {
            let page = client
                .describe_virtual_interfaces()
                .set_next_token(token)
                .send()
                .await?;
            Ok((page.virtual_interfaces.unwrap_or_default(), page.next_token))
        })
        .await?;

        Ok(AWSDirectConnect {
            connections,
            virtual_interfaces,
        })
    }

    pub async fn gateways(
        config: &aws_config::SdkConfig,
    ) -> Result<AWSDirectConnectGateways, Box<dyn Error>> {
        let client = &Client::new(config);

        let gateways = every_page("DescribeDirectConnectGateways", |token| async move {
            let page = client
                .describe_direct_connect_gateways()
                .set_next_token(token)
                .send()
                .await?;
            Ok((
                page.direct_connect_gateways.unwrap_or_default(),
                page.next_token,
            ))
        })
        .await?;

        let mut associations = Vec::new();
        for gateway_id in gateways
            .iter()
            .filter_map(|gateway| gateway.direct_connect_gateway_id())
        {
            associations.extend(
                every_page(
                    "DescribeDirectConnectGatewayAssociations",
                    |token| async move {
                        let page = client
                            .describe_direct_connect_gateway_associations()
                            .direct_connect_gateway_id(gateway_id)
                            .set_next_token(token)
                            .send()
                            .await?;
                        Ok((
                            page.direct_connect_gateway_associations.unwrap_or_default(),
                            page.next_token,
                        ))
                    },
                )
                .await?,
            );
        }

        Ok(AWSDirectConnectGateways {
            gateways,
            associations,
        })
    }

    type Page<T> = Result<(Vec<T>, Option<String>), Box<dyn Error>>;

    async fn every_page<T, Next>(
        operation: &str,
        mut page: impl FnMut(Option<String>) -> Next,
    ) -> Result<Vec<T>, Box<dyn Error>>
    where
        Next: Future<Output = Page<T>>,
    {
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        let mut next_token: Option<String> = None;
        loop {
            let (batch, token) = page(next_token.clone()).await?;
            items.extend(batch);
            match token.filter(|token| !token.is_empty()) {
                None => return Ok(items),
                Some(token) if !seen.insert(token.clone()) => {
                    return Err(format!(
                        "{operation} repeated a page token, so paging would never end"
                    )
                    .into());
                }
                token => next_token = token,
            }
        }
    }
}
