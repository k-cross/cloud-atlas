pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_networkmanager::{config::Region, Client, Error};

    async fn get_networks(client: &Client) -> Result<AmazonCollection, Error> {
        let global_nets = client.describe_global_networks().send().await?;
        let mut g_nets = Vec::new();

        for net in global_nets.global_networks().unwrap_or_default() {
            g_nets.push(net.to_owned());
        }

        Ok(AmazonCollection::AmazonNetworks(g_nets))
    }

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        let shared_config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&shared_config);

        match get_networks(&client).await {
            Ok(res) => Ok(res),
            Err(e) => Err(e.into()),
        }
    }
}
