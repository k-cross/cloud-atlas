pub mod collector {
    use crate::cloud::definition::AWSNetworking;
    use aws_sdk_ec2::Client;

    /// Collects the VPC routing/egress plane: route tables (with their routes
    /// and subnet associations), internet gateways, NAT gateways, and Elastic
    /// IPs. Together these let the projector answer "can this subnet actually
    /// reach the internet, and how".
    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AWSNetworking, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut route_tables = Vec::new();
        let mut next_token = None;
        loop {
            let mut req = client.describe_route_tables();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }
            let mut resp = req.send().await?;
            next_token = resp.next_token.take();
            route_tables.extend(resp.route_tables.take().unwrap_or_default());
            if next_token.is_none() {
                break;
            }
        }

        let mut internet_gateways = Vec::new();
        let mut next_token = None;
        loop {
            let mut req = client.describe_internet_gateways();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }
            let mut resp = req.send().await?;
            next_token = resp.next_token.take();
            internet_gateways.extend(resp.internet_gateways.take().unwrap_or_default());
            if next_token.is_none() {
                break;
            }
        }

        let mut nat_gateways = Vec::new();
        let mut next_token = None;
        loop {
            let mut req = client.describe_nat_gateways();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }
            let mut resp = req.send().await?;
            next_token = resp.next_token.take();
            nat_gateways.extend(resp.nat_gateways.take().unwrap_or_default());
            if next_token.is_none() {
                break;
            }
        }

        // DescribeAddresses is not paginated.
        let resp = client.describe_addresses().send().await?;
        let addresses = resp.addresses.unwrap_or_default();

        Ok(AWSNetworking {
            route_tables,
            internet_gateways,
            nat_gateways,
            addresses,
        })
    }
}
