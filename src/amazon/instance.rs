pub mod collector {
    // dependencies
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_ec2::model::{Instance, Tag};
    use aws_sdk_ec2::{Client, Error, Region};

    //project
    use crate::cloud;

    async fn match_instances(client: &Client) -> Result<(Vec<Instance>, Vec<Instance>), Error> {
        let resp = client.describe_instances().send().await?;

        let mut running_insts = Vec::new();
        let mut offline_insts = Vec::new();

        for reservation in resp.reservations().unwrap_or_default() {
            for instance in reservation.instances().unwrap_or_default() {
                match instance.state.clone().unwrap().name {
                    Some(aws_sdk_ec2::model::InstanceStateName::Running) => {
                        running_insts.push(instance.to_owned())
                    }
                    _ => offline_insts.push(instance.to_owned()),
                }
            }
        }

        Ok((running_insts, offline_insts))
    }

    pub async fn runner(region: String) -> Result<(Vec<Instance>, Vec<Instance>), Error> {
        let region_provider = RegionProviderChain::first_try(Region::new(region))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        let shared_config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&shared_config);

        match match_instances(&client).await {
            Ok(res) => Ok(res),
            err => cloud::Error::AwsEC2Error(err),
        }
    }
}
