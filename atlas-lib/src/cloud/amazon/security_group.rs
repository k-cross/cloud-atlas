pub mod collector {
    use aws_sdk_ec2::Client;
    use aws_sdk_ec2::types::SecurityGroup;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<SecurityGroup>, Box<dyn std::error::Error>> {
        Ok(Client::new(config)
            .describe_security_groups()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::collector::runner;
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

        let groups = runner(&config).await.expect("runner ok");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].group_id(), Some("sg-123"));
        assert_eq!(groups[0].group_name(), Some("web-sg"));
        assert_eq!(groups[0].vpc_id(), Some("vpc-aaa"));
    }
}
