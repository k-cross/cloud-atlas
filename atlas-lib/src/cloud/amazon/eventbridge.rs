pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_eventbridge::types::EventBus;
    use aws_sdk_eventbridge::{config::Region, Client, Error};

    async fn get_event_info(client: &Client) -> Result<Vec<EventBus>, Error> {
        let resp = client.list_event_buses().send().await?;

        let es = if let Some(buses) = resp.event_buses {
            buses.to_owned()
        } else {
            Vec::new()
        };

        Ok(es)
    }

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        let shared_config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&shared_config);

        match get_event_info(&client).await {
            Ok(res) => Ok(AmazonCollection::AmazonEventbridge(res)),
            Err(e) => Err(e.into()),
        }
    }
}
