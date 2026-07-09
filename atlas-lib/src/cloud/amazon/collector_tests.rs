//! Consolidated AWS collector coverage. Each test replays canned responses (in
//! request order) through the *real* SDK via `replay_config`, then runs the
//! collector and asserts the fields the projector reads. AWS types aren't
//! `serde`, so this replay path is the only way to test their deserialization.
//! The heavily-commented reference examples live in `instance.rs` /
//! `security_group.rs` / `sqs.rs`.

use crate::cloud::definition::AmazonCollection;
use aws_credential_types::Credentials;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;

/// Build an `SdkConfig` whose HTTP layer replays `(content_type, body)` pairs in
/// order — one per request the collector makes.
async fn replay_config(responses: &[(&'static str, &'static str)]) -> aws_config::SdkConfig {
    let events = responses
        .iter()
        .map(|(ct, body)| {
            ReplayEvent::new(
                http::Request::builder()
                    .uri("https://example.amazonaws.com/")
                    .body(SdkBody::empty())
                    .unwrap(),
                http::Response::builder()
                    .status(200)
                    .header("content-type", *ct)
                    .body(SdkBody::from(*body))
                    .unwrap(),
            )
        })
        .collect();
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(Credentials::for_tests())
        .http_client(StaticReplayClient::new(events))
        .load()
        .await
}

const XML: &str = "text/xml";
const JSON: &str = "application/json";
const JSON10: &str = "application/x-amz-json-1.0";
const JSON11: &str = "application/x-amz-json-1.1";

// ---- JSON-protocol services -------------------------------------------------

#[tokio::test]
async fn lambda_functions() {
    let body = r#"{"Functions":[{"FunctionName":"fn1","FunctionArn":"arn:aws:lambda:us-east-1:111:function:fn1","Runtime":"python3.12"}]}"#;
    let cfg = replay_config(&[(JSON, body)]).await;
    let AmazonCollection::AmazonLambdas(fns) =
        super::lambda::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonLambdas");
    };
    assert_eq!(fns[0].function_name(), Some("fn1"));
}

#[tokio::test]
async fn eks_clusters() {
    // list_clusters, then describe_cluster per name.
    let list = r#"{"clusters":["c1"]}"#;
    let describe = r#"{"cluster":{"name":"c1","arn":"arn:aws:eks:us-east-1:111:cluster/c1","status":"ACTIVE"}}"#;
    let cfg = replay_config(&[(JSON, list), (JSON, describe)]).await;
    let AmazonCollection::AmazonEks(clusters) = super::eks::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonEks");
    };
    assert_eq!(clusters[0].name(), Some("c1"));
}

#[tokio::test]
async fn ecs_clusters() {
    let body = r#"{"clusters":[{"clusterArn":"arn:aws:ecs:us-east-1:111:cluster/c1","clusterName":"c1","status":"ACTIVE"}]}"#;
    let cfg = replay_config(&[(JSON11, body)]).await;
    let AmazonCollection::AmazonClusters(clusters) =
        super::container_service::collector::runner(&cfg)
            .await
            .unwrap()
    else {
        panic!("expected AmazonClusters");
    };
    assert_eq!(clusters[0].cluster_name(), Some("c1"));
}

#[tokio::test]
async fn dynamodb_tables() {
    let body = r#"{"TableNames":["orders","users"]}"#;
    let cfg = replay_config(&[(JSON10, body)]).await;
    let AmazonCollection::AmazonDynamoDb(tables) =
        super::dynamodb::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonDynamoDb");
    };
    assert_eq!(tables, vec!["orders".to_string(), "users".to_string()]);
}

#[tokio::test]
async fn api_gateway_rest_apis() {
    let body = r#"{"item":[{"id":"api1","name":"my-api"}]}"#;
    let cfg = replay_config(&[(JSON, body)]).await;
    let AmazonCollection::AmazonApiGateway(apis) =
        super::api_gateway::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonApiGateway");
    };
    assert_eq!(apis[0].id(), Some("api1"));
    assert_eq!(apis[0].name(), Some("my-api"));
}

#[tokio::test]
async fn eventbridge_buses() {
    let body = r#"{"EventBuses":[{"Name":"default","Arn":"arn:aws:events:us-east-1:111:event-bus/default"}]}"#;
    let cfg = replay_config(&[(JSON11, body)]).await;
    let AmazonCollection::AmazonEventbridge(buses) =
        super::eventbridge::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonEventbridge");
    };
    assert_eq!(buses[0].name(), Some("default"));
}

// ---- XML-protocol services --------------------------------------------------

#[tokio::test]
async fn sns_topics() {
    let body = r#"<ListTopicsResponse xmlns="http://sns.amazonaws.com/doc/2010-03-31/">
  <ListTopicsResult>
    <Topics>
      <member><TopicArn>arn:aws:sns:us-east-1:111:my-topic</TopicArn></member>
    </Topics>
  </ListTopicsResult>
  <ResponseMetadata><RequestId>r</RequestId></ResponseMetadata>
</ListTopicsResponse>"#;
    let cfg = replay_config(&[(XML, body)]).await;
    let AmazonCollection::AmazonSns(topics) = super::sns::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonSns");
    };
    assert_eq!(
        topics[0].topic_arn(),
        Some("arn:aws:sns:us-east-1:111:my-topic")
    );
}

#[tokio::test]
async fn rds_db_instances() {
    let body = r#"<DescribeDBInstancesResponse xmlns="http://rds.amazonaws.com/doc/2014-10-31/">
  <DescribeDBInstancesResult>
    <DBInstances>
      <DBInstance>
        <DBInstanceIdentifier>mydb</DBInstanceIdentifier>
        <Engine>postgres</Engine>
        <Endpoint><Address>mydb.abc.us-east-1.rds.amazonaws.com</Address><Port>5432</Port></Endpoint>
      </DBInstance>
    </DBInstances>
  </DescribeDBInstancesResult>
</DescribeDBInstancesResponse>"#;
    let cfg = replay_config(&[(XML, body)]).await;
    let AmazonCollection::AmazonRds(dbs) = super::rds::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonRds");
    };
    assert_eq!(dbs[0].db_instance_identifier(), Some("mydb"));
    assert_eq!(dbs[0].engine(), Some("postgres"));
    assert_eq!(
        dbs[0].endpoint().and_then(|e| e.address()),
        Some("mydb.abc.us-east-1.rds.amazonaws.com")
    );
}

#[tokio::test]
async fn cloudfront_distributions() {
    // Empty list keeps the (many-required-fields) DistributionSummary out of the
    // fixture; still exercises the payload deserialization + loop termination.
    let body = r#"<DistributionList xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <Marker></Marker>
  <MaxItems>100</MaxItems>
  <IsTruncated>false</IsTruncated>
  <Quantity>0</Quantity>
  <Items/>
</DistributionList>"#;
    let cfg = replay_config(&[(XML, body)]).await;
    let AmazonCollection::AmazonCloudFront(dists) =
        super::cloudfront::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonCloudFront");
    };
    assert!(dists.is_empty());
}

#[tokio::test]
async fn route53_zones_and_records() {
    // list_hosted_zones, then list_resource_record_sets per zone.
    let zones = r#"<ListHostedZonesResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <HostedZones>
    <HostedZone><Id>/hostedzone/Z123</Id><Name>example.com.</Name><CallerReference>ref</CallerReference></HostedZone>
  </HostedZones>
  <IsTruncated>false</IsTruncated>
  <MaxItems>100</MaxItems>
</ListHostedZonesResponse>"#;
    let records = r#"<ListResourceRecordSetsResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ResourceRecordSets>
    <ResourceRecordSet>
      <Name>example.com.</Name>
      <Type>A</Type>
      <TTL>300</TTL>
      <ResourceRecords><ResourceRecord><Value>1.2.3.4</Value></ResourceRecord></ResourceRecords>
    </ResourceRecordSet>
  </ResourceRecordSets>
  <IsTruncated>false</IsTruncated>
  <MaxItems>100</MaxItems>
</ListResourceRecordSetsResponse>"#;
    let cfg = replay_config(&[(XML, zones), (XML, records)]).await;
    let AmazonCollection::AmazonRoute53 {
        hosted_zones,
        record_sets,
    } = super::route53::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonRoute53");
    };
    assert_eq!(hosted_zones[0].name(), "example.com.");
    assert_eq!(record_sets[0].name(), "example.com.");
}

#[tokio::test]
async fn ec2_networking_plane() {
    // Four EC2 calls in order: route tables, IGWs, NAT GWs, addresses.
    let route_tables = r#"<DescribeRouteTablesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <routeTableSet><item><routeTableId>rtb-1</routeTableId><vpcId>vpc-aaa</vpcId><routeSet/><associationSet/></item></routeTableSet>
</DescribeRouteTablesResponse>"#;
    let igws = r#"<DescribeInternetGatewaysResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <internetGatewaySet><item><internetGatewayId>igw-1</internetGatewayId><attachmentSet/></item></internetGatewaySet>
</DescribeInternetGatewaysResponse>"#;
    let nats = r#"<DescribeNatGatewaysResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <natGatewaySet><item><natGatewayId>nat-1</natGatewayId><vpcId>vpc-aaa</vpcId></item></natGatewaySet>
</DescribeNatGatewaysResponse>"#;
    let addrs = r#"<DescribeAddressesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <addressesSet><item><publicIp>52.1.2.3</publicIp><allocationId>eipalloc-1</allocationId></item></addressesSet>
</DescribeAddressesResponse>"#;
    let cfg = replay_config(&[(XML, route_tables), (XML, igws), (XML, nats), (XML, addrs)]).await;
    let AmazonCollection::AmazonNetworking {
        route_tables,
        internet_gateways,
        nat_gateways,
        addresses,
    } = super::networking::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonNetworking");
    };
    assert_eq!(route_tables[0].route_table_id(), Some("rtb-1"));
    assert_eq!(internet_gateways[0].internet_gateway_id(), Some("igw-1"));
    assert_eq!(nat_gateways[0].nat_gateway_id(), Some("nat-1"));
    assert_eq!(addresses[0].public_ip(), Some("52.1.2.3"));
}

#[tokio::test]
async fn elbv2_load_balancers_and_target_groups() {
    // describe_load_balancers, describe_listeners (per LB), describe_target_groups,
    // describe_target_health (per TG).
    let lbs = r#"<DescribeLoadBalancersResponse xmlns="http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/">
  <DescribeLoadBalancersResult><LoadBalancers><member>
    <LoadBalancerArn>arn:aws:elasticloadbalancing:us-east-1:111:loadbalancer/app/my-lb/abc</LoadBalancerArn>
    <LoadBalancerName>my-lb</LoadBalancerName>
    <DNSName>my-lb-1.us-east-1.elb.amazonaws.com</DNSName>
    <Type>application</Type>
    <VpcId>vpc-aaa</VpcId>
  </member></LoadBalancers></DescribeLoadBalancersResult>
</DescribeLoadBalancersResponse>"#;
    let listeners = r#"<DescribeListenersResponse xmlns="http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/">
  <DescribeListenersResult><Listeners/></DescribeListenersResult>
</DescribeListenersResponse>"#;
    let tgs = r#"<DescribeTargetGroupsResponse xmlns="http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/">
  <DescribeTargetGroupsResult><TargetGroups><member>
    <TargetGroupArn>arn:aws:elasticloadbalancing:us-east-1:111:targetgroup/my-tg/abc</TargetGroupArn>
    <TargetGroupName>my-tg</TargetGroupName>
    <Protocol>HTTP</Protocol>
    <Port>80</Port>
    <VpcId>vpc-aaa</VpcId>
  </member></TargetGroups></DescribeTargetGroupsResult>
</DescribeTargetGroupsResponse>"#;
    let health = r#"<DescribeTargetHealthResponse xmlns="http://elasticloadbalancing.amazonaws.com/doc/2015-12-01/">
  <DescribeTargetHealthResult><TargetHealthDescriptions/></DescribeTargetHealthResult>
</DescribeTargetHealthResponse>"#;
    let cfg = replay_config(&[(XML, lbs), (XML, listeners), (XML, tgs), (XML, health)]).await;
    let AmazonCollection::AmazonLoadBalancers {
        load_balancers,
        target_groups,
        ..
    } = super::load_balancer::collector::runner(&cfg).await.unwrap()
    else {
        panic!("expected AmazonLoadBalancers");
    };
    assert_eq!(load_balancers[0].load_balancer_name(), Some("my-lb"));
    assert_eq!(target_groups[0].target_group_name(), Some("my-tg"));
}
