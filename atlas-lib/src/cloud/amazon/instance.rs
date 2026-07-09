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

#[cfg(test)]
mod tests {
    use super::collector::runner;
    use crate::cloud::definition::AmazonCollection;
    use aws_credential_types::Credentials;
    use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
    use aws_smithy_types::body::SdkBody;

    // A minimal but well-formed EC2 DescribeInstances response (ec2Query
    // protocol → XML). Because aws-sdk-ec2's own types aren't `serde`, the only
    // way to test the response→struct mapping is through the real SDK — which
    // `StaticReplayClient` lets us do offline. Fields chosen are exactly the
    // ones the AWS projector reads.
    const DESCRIBE_INSTANCES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <requestId>req-1</requestId>
  <reservationSet>
    <item>
      <reservationId>r-1</reservationId>
      <ownerId>111111111111</ownerId>
      <instancesSet>
        <item>
          <instanceId>i-0abc123</instanceId>
          <instanceState><code>16</code><name>running</name></instanceState>
          <privateIpAddress>10.0.1.5</privateIpAddress>
          <vpcId>vpc-aaa</vpcId>
          <subnetId>subnet-bbb</subnetId>
          <placement><availabilityZone>us-east-1a</availabilityZone></placement>
        </item>
      </instancesSet>
    </item>
  </reservationSet>
</DescribeInstancesResponse>"#;

    #[tokio::test]
    async fn describe_instances_maps_the_fields_the_projector_reads() {
        // Replay a canned response for any request the SDK makes (the request
        // in the event is a placeholder; we don't assert on it).
        let http = StaticReplayClient::new(vec![ReplayEvent::new(
            http::Request::builder()
                .uri("https://ec2.us-east-1.amazonaws.com/")
                .body(SdkBody::empty())
                .unwrap(),
            http::Response::builder()
                .status(200)
                .header("content-type", "text/xml;charset=UTF-8")
                .body(SdkBody::from(DESCRIBE_INSTANCES_XML))
                .unwrap(),
        )]);

        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(Credentials::for_tests())
            .http_client(http)
            .load()
            .await;

        let collection = runner(&config).await.expect("runner succeeds");
        let AmazonCollection::AmazonInstances(instances) = collection else {
            panic!("expected AmazonInstances");
        };

        assert_eq!(instances.len(), 1);
        let inst = &instances[0];
        assert_eq!(inst.instance_id(), Some("i-0abc123"));
        assert_eq!(inst.vpc_id(), Some("vpc-aaa"));
        assert_eq!(inst.subnet_id(), Some("subnet-bbb"));
        assert_eq!(inst.private_ip_address(), Some("10.0.1.5"));
        assert_eq!(
            inst.placement().and_then(|p| p.availability_zone()),
            Some("us-east-1a"),
        );
    }
}
