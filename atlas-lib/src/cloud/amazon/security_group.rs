pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_ec2::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut groups = Vec::new();
        let mut next_token = None;

        loop {
            let mut req = client.describe_security_groups();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }

            let resp = req.send().await?;
            groups.extend(resp.security_groups().to_vec());

            next_token = resp.next_token().map(|s| s.to_string());
            if next_token.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonSecurityGroups(groups))
    }
}

#[cfg(test)]
mod tests {
    use super::collector::runner;
    use crate::cloud::definition::AmazonCollection;
    use aws_credential_types::Credentials;
    use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
    use aws_smithy_types::body::SdkBody;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DescribeSecurityGroupsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <requestId>r</requestId>
  <securityGroupInfo>
    <item>
      <groupId>sg-123</groupId>
      <groupName>web-sg</groupName>
      <vpcId>vpc-aaa</vpcId>
      <ipPermissions/>
      <ipPermissionsEgress/>
    </item>
  </securityGroupInfo>
</DescribeSecurityGroupsResponse>"#;

    #[tokio::test]
    async fn describe_security_groups_maps_the_fields_the_projector_reads() {
        let http = StaticReplayClient::new(vec![ReplayEvent::new(
            http::Request::builder()
                .uri("https://ec2.us-east-1.amazonaws.com/")
                .body(SdkBody::empty())
                .unwrap(),
            http::Response::builder()
                .status(200)
                .header("content-type", "text/xml")
                .body(SdkBody::from(XML))
                .unwrap(),
        )]);
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(Credentials::for_tests())
            .http_client(http)
            .load()
            .await;

        let AmazonCollection::AmazonSecurityGroups(groups) =
            runner(&config).await.expect("runner ok")
        else {
            panic!("expected AmazonSecurityGroups");
        };
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group_id(), Some("sg-123"));
        assert_eq!(groups[0].group_name(), Some("web-sg"));
        assert_eq!(groups[0].vpc_id(), Some("vpc-aaa"));
    }
}
