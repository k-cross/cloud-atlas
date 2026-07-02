pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_ec2::types::Filter;
    use aws_sdk_ec2::{Client, Error};

    async fn match_instances(client: &Client) -> Result<AmazonCollection, Error> {
        // ["running", "pending", "shutting-down", "terminated", "stopped", "stopping"] are all the
        // instance states, only grab active or soon to be active ones.
        let filter = Filter::builder()
            .set_name(Some("instance-state-name".to_owned()))
            .set_values(Some(vec!["running".to_owned(), "pending".to_owned()]))
            .build();
        let resp = client.describe_instances().filters(filter).send().await?;
        let mut running_insts = Vec::new();

        for reservation in resp.reservations() {
            for instance in reservation.instances() {
                running_insts.push(instance.to_owned());
            }
        }

        Ok(AmazonCollection::AmazonInstances(running_insts))
    }

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);
        match_instances(&client).await.map_err(Into::into)
    }
}
