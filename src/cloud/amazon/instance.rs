pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_ec2::types::Filter;
    use aws_sdk_ec2::{config::Region, Client, Error};

    async fn match_instances(client: &Client) -> Result<AmazonCollection, Error> {
        // ["running", "pending", "shutting-down", "terminated", "stopped", "stopping"] are all the
        // instance states, only grab active or soon to be active ones.
        let filter = Filter::builder()
            .set_name(Some("instance-state-name".to_owned()))
            .set_values(Some(vec!["running".to_owned(), "pending".to_owned()]))
            .build();
        let resp = client.describe_instances().filters(filter).send().await?;
        let mut running_insts = Vec::new();

        for reservation in resp.reservations().unwrap_or_default() {
            for instance in reservation.instances().unwrap_or_default() {
                running_insts.push(instance.to_owned());
            }
        }

        let r = AmazonCollection::AmazonInstances(running_insts);
        Ok(r)
    }

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        let shared_config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&shared_config);

        match match_instances(&client).await {
            Ok(res) => Ok(res),
            Err(e) => Err(e.into()),
        }
    }
}
