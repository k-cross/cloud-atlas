use crate::cloud::definition::{
    AWSConnectivity, AWSDirectConnect, AWSDirectConnectGateways, AWSLoadBalancing, AWSNetworking,
    AWSRoute53, AWSTransitGatewayRouting, HostedZoneId, TableName,
};
use aws_credential_types::Credentials;
use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;

async fn replay_config(responses: &[(&'static str, &'static str)]) -> aws_config::SdkConfig {
    replay(responses).await.0
}

async fn replay(
    responses: &[(&'static str, &'static str)],
) -> (aws_config::SdkConfig, StaticReplayClient) {
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
    let http = StaticReplayClient::new(events);
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(Credentials::for_tests())
        .http_client(http.clone())
        .load()
        .await;
    (config, http)
}

const XML: &str = "text/xml";
const JSON: &str = "application/json";
const JSON10: &str = "application/x-amz-json-1.0";
const JSON11: &str = "application/x-amz-json-1.1";

#[tokio::test]
async fn lambda_functions() {
    let body = r#"{"Functions":[{"FunctionName":"fn1","FunctionArn":"arn:aws:lambda:us-east-1:111:function:fn1","Runtime":"python3.12"}]}"#;
    let cfg = replay_config(&[(JSON, body)]).await;
    let fns = super::lambda::collector::runner(&cfg).await.unwrap();
    assert_eq!(fns[0].function_name(), Some("fn1"));
}

#[tokio::test]
async fn lambda_functions_follow_every_page() {
    let first = r#"{"Functions":[{"FunctionName":"fn1"}],"NextMarker":"page-2"}"#;
    let second = r#"{"Functions":[{"FunctionName":"fn2"}]}"#;
    let cfg = replay_config(&[(JSON, first), (JSON, second)]).await;
    let fns = super::lambda::collector::runner(&cfg).await.unwrap();
    let names: Vec<_> = fns.iter().filter_map(|f| f.function_name()).collect();
    assert_eq!(names, vec!["fn1", "fn2"], "the second page is read");
}

#[tokio::test]
async fn eks_clusters() {
    let list = r#"{"clusters":["c1"]}"#;
    let describe = r#"{"cluster":{"name":"c1","arn":"arn:aws:eks:us-east-1:111:cluster/c1","status":"ACTIVE"}}"#;
    let cfg = replay_config(&[(JSON, list), (JSON, describe)]).await;
    let clusters = super::eks::collector::runner(&cfg).await.unwrap();
    assert_eq!(clusters[0].name(), Some("c1"));
}

#[tokio::test]
async fn ecs_clusters() {
    let list = r#"{"clusterArns":["arn:aws:ecs:us-east-1:111:cluster/c1"]}"#;
    let body = r#"{"clusters":[{"clusterArn":"arn:aws:ecs:us-east-1:111:cluster/c1","clusterName":"c1","status":"ACTIVE"}]}"#;
    let cfg = replay_config(&[(JSON11, list), (JSON11, body)]).await;
    let clusters = super::container_service::collector::runner(&cfg)
        .await
        .unwrap();
    assert_eq!(clusters[0].cluster_name(), Some("c1"));
}

#[tokio::test]
async fn ecs_describes_every_listed_cluster_not_just_the_default() {
    let list = r#"{"clusterArns":["arn:aws:ecs:us-east-1:111:cluster/web","arn:aws:ecs:us-east-1:111:cluster/gone"],"nextToken":"page-2"}"#;
    let rest = r#"{"clusterArns":["arn:aws:ecs:us-east-1:111:cluster/batch"]}"#;
    let described = r#"{
      "clusters":[
        {"clusterArn":"arn:aws:ecs:us-east-1:111:cluster/web","clusterName":"web"},
        {"clusterArn":"arn:aws:ecs:us-east-1:111:cluster/batch","clusterName":"batch"}
      ],
      "failures":[{"arn":"arn:aws:ecs:us-east-1:111:cluster/gone","reason":"MISSING"}]
    }"#;
    let cfg = replay_config(&[(JSON11, list), (JSON11, rest), (JSON11, described)]).await;
    let clusters = super::container_service::collector::runner(&cfg)
        .await
        .unwrap();
    let names: Vec<_> = clusters.iter().filter_map(|c| c.cluster_name()).collect();
    assert_eq!(
        names,
        vec!["web", "batch"],
        "a cluster deleted between the list and the describe is simply gone"
    );
}

#[tokio::test]
async fn an_ecs_cluster_that_could_not_be_described_is_an_error_not_an_absence() {
    let list = r#"{"clusterArns":["arn:aws:ecs:us-east-1:111:cluster/web"]}"#;
    let described = r#"{"clusters":[],"failures":[{"arn":"arn:aws:ecs:us-east-1:111:cluster/web","reason":"ACCESS_DENIED"}]}"#;
    let cfg = replay_config(&[(JSON11, list), (JSON11, described)]).await;
    let error = super::container_service::collector::runner(&cfg)
        .await
        .expect_err("a cluster that exists but was not described must not read as absent");
    assert!(error.to_string().contains("ACCESS_DENIED"), "{error}");
}

const CONFIG_BUCKETS: &str = r#"{"resourceIdentifiers":[{"resourceType":"AWS::S3::Bucket","resourceId":"globex-assets","resourceName":"globex-assets"}]}"#;

#[tokio::test]
async fn config_lists_only_the_drawn_types_it_counted() {
    let counts = r#"{"totalDiscoveredResources":7,"resourceCounts":[
        {"resourceType":"AWS::EC2::VPC","count":2},
        {"resourceType":"AWS::EC2::NetworkAcl","count":4},
        {"resourceType":"AWS::RDS::DBCluster","count":0},
        {"resourceType":"AWS::S3::Bucket","count":1}
    ]}"#;
    let (cfg, http) = replay(&[(JSON11, counts), (JSON11, CONFIG_BUCKETS)]).await;
    let resources = super::resource::collector::runner(false, false, &cfg)
        .await
        .unwrap();

    let ids: Vec<_> = resources["AWS::S3::Bucket"]
        .iter()
        .filter_map(|resource| resource.resource_id())
        .collect();
    assert_eq!(ids, vec!["globex-assets"]);
    assert_eq!(resources.len(), 1);

    let requests = request_bodies(&http);
    assert_eq!(
        requests.len(),
        2,
        "typed, excluded and empty types cost no list call: {requests:?}"
    );
    assert!(requests[1].contains("AWS::S3::Bucket"), "{}", requests[1]);
}

#[tokio::test]
async fn config_counts_follow_every_page() {
    let first =
        r#"{"resourceCounts":[{"resourceType":"AWS::EC2::VPC","count":2}],"nextToken":"page-2"}"#;
    let second = r#"{"resourceCounts":[{"resourceType":"AWS::S3::Bucket","count":1}]}"#;
    let (cfg, http) = replay(&[(JSON11, first), (JSON11, second), (JSON11, CONFIG_BUCKETS)]).await;
    let resources = super::resource::collector::runner(false, false, &cfg)
        .await
        .unwrap();

    assert!(
        resources.contains_key("AWS::S3::Bucket"),
        "a type counted only on the second page is still listed"
    );
    let requests = request_bodies(&http);
    assert!(requests[1].contains("page-2"), "{}", requests[1]);
}

#[tokio::test]
async fn dynamodb_tables() {
    let body = r#"{"TableNames":["orders","users"]}"#;
    let cfg = replay_config(&[(JSON10, body)]).await;
    let tables = super::dynamodb::collector::runner(&cfg).await.unwrap();
    assert_eq!(
        tables,
        vec![
            TableName("orders".to_owned()),
            TableName("users".to_owned())
        ]
    );
}

#[tokio::test]
async fn api_gateway_rest_apis() {
    let body = r#"{"item":[{"id":"api1","name":"my-api"}]}"#;
    let cfg = replay_config(&[(JSON, body)]).await;
    let apis = super::api_gateway::collector::runner(&cfg).await.unwrap();
    assert_eq!(apis[0].id(), Some("api1"));
    assert_eq!(apis[0].name(), Some("my-api"));
}

#[tokio::test]
async fn eventbridge_buses() {
    let body = r#"{"EventBuses":[{"Name":"default","Arn":"arn:aws:events:us-east-1:111:event-bus/default"}]}"#;
    let cfg = replay_config(&[(JSON11, body)]).await;
    let buses = super::eventbridge::collector::runner(&cfg).await.unwrap();
    assert_eq!(buses[0].name(), Some("default"));
}

#[tokio::test]
async fn eventbridge_buses_follow_every_page() {
    let first = r#"{"EventBuses":[{"Name":"default"}],"NextToken":"page-2"}"#;
    let second = r#"{"EventBuses":[{"Name":"orders"}]}"#;
    let cfg = replay_config(&[(JSON11, first), (JSON11, second)]).await;
    let buses = super::eventbridge::collector::runner(&cfg).await.unwrap();
    let names: Vec<_> = buses.iter().filter_map(|b| b.name()).collect();
    assert_eq!(names, vec!["default", "orders"]);
}

#[tokio::test]
async fn eventbridge_refuses_a_page_token_that_repeats() {
    let page = r#"{"EventBuses":[{"Name":"default"}],"NextToken":"stuck"}"#;
    let cfg = replay_config(&[(JSON11, page), (JSON11, page)]).await;
    let error = super::eventbridge::collector::runner(&cfg)
        .await
        .expect_err("a token that never advances must fail, not loop or truncate");
    assert!(error.to_string().contains("repeated"), "{error}");
}

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
    let topics = super::sns::collector::runner(&cfg).await.unwrap();
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
    let dbs = super::rds::collector::runner(&cfg).await.unwrap();
    assert_eq!(dbs[0].db_instance_identifier(), Some("mydb"));
    assert_eq!(dbs[0].engine(), Some("postgres"));
    assert_eq!(
        dbs[0].endpoint().and_then(|e| e.address()),
        Some("mydb.abc.us-east-1.rds.amazonaws.com")
    );
}

#[tokio::test]
async fn cloudfront_distributions() {
    let body = r#"<DistributionList xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <Marker></Marker>
  <MaxItems>100</MaxItems>
  <IsTruncated>false</IsTruncated>
  <Quantity>0</Quantity>
  <Items/>
</DistributionList>"#;
    let cfg = replay_config(&[(XML, body)]).await;
    let dists = super::cloudfront::collector::runner(&cfg).await.unwrap();
    assert!(dists.is_empty());
}

#[tokio::test]
async fn route53_zones_and_records() {
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
    let AWSRoute53 {
        hosted_zones,
        record_sets,
    } = super::route53::collector::runner(&cfg).await.unwrap();
    assert_eq!(hosted_zones[0].name(), "example.com.");
    let records = &record_sets[&HostedZoneId("/hostedzone/Z123".into())];
    assert_eq!(records[0].name(), "example.com.");
    assert_eq!(records[0].r#type().as_str(), "A");
}

const ZONE: &str = r#"<ListHostedZonesResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <HostedZones>
    <HostedZone><Id>/hostedzone/Z123</Id><Name>example.com.</Name><CallerReference>ref</CallerReference></HostedZone>
  </HostedZones>
  <IsTruncated>false</IsTruncated>
  <MaxItems>100</MaxItems>
</ListHostedZonesResponse>"#;

const WEIGHTED_FIRST: &str = r#"<ListResourceRecordSetsResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ResourceRecordSets>
    <ResourceRecordSet>
      <Name>api.example.com.</Name><Type>A</Type><SetIdentifier>us</SetIdentifier><Weight>50</Weight><TTL>60</TTL>
      <ResourceRecords><ResourceRecord><Value>192.0.2.1</Value></ResourceRecord></ResourceRecords>
    </ResourceRecordSet>
  </ResourceRecordSets>
  <IsTruncated>true</IsTruncated>
  <NextRecordName>api.example.com.</NextRecordName>
  <NextRecordType>A</NextRecordType>
  <NextRecordIdentifier>eu</NextRecordIdentifier>
  <MaxItems>1</MaxItems>
</ListResourceRecordSetsResponse>"#;

#[tokio::test]
async fn route53_resumes_weighted_records_at_their_set_identifier() {
    let second = r#"<ListResourceRecordSetsResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ResourceRecordSets>
    <ResourceRecordSet>
      <Name>api.example.com.</Name><Type>A</Type><SetIdentifier>eu</SetIdentifier><Weight>50</Weight><TTL>60</TTL>
      <ResourceRecords><ResourceRecord><Value>192.0.2.2</Value></ResourceRecord></ResourceRecords>
    </ResourceRecordSet>
  </ResourceRecordSets>
  <IsTruncated>false</IsTruncated>
  <MaxItems>1</MaxItems>
</ListResourceRecordSetsResponse>"#;
    let (cfg, http) = replay(&[(XML, ZONE), (XML, WEIGHTED_FIRST), (XML, second)]).await;
    let AWSRoute53 { record_sets, .. } = super::route53::collector::runner(&cfg).await.unwrap();

    let identifiers: Vec<_> = record_sets[&HostedZoneId("/hostedzone/Z123".into())]
        .iter()
        .filter_map(|record| record.set_identifier())
        .collect();
    assert_eq!(identifiers, vec!["us", "eu"]);
    let resumed = http.actual_requests().nth(2).expect("a third request");
    assert!(
        resumed.uri().contains("identifier=eu"),
        "without the set identifier the next page restarts at the first `api` A record: {}",
        resumed.uri()
    );
}

#[tokio::test]
async fn route53_refuses_a_record_cursor_that_does_not_advance() {
    let cfg = replay_config(&[(XML, ZONE), (XML, WEIGHTED_FIRST), (XML, WEIGHTED_FIRST)]).await;
    let error = super::route53::collector::runner(&cfg)
        .await
        .expect_err("a cursor that never advances must fail, not loop or truncate");
    assert!(error.to_string().contains("repeated"), "{error}");
}

#[tokio::test]
async fn ec2_networking_plane() {
    let route_tables = r#"<DescribeRouteTablesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <routeTableSet><item><routeTableId>rtb-1</routeTableId><vpcId>vpc-aaa</vpcId>
    <routeSet>
      <item><destinationCidrBlock>10.0.0.0/16</destinationCidrBlock><gatewayId>local</gatewayId><state>active</state></item>
      <item><destinationPrefixListId>pl-0shared</destinationPrefixListId><transitGatewayId>tgw-1</transitGatewayId><state>blackhole</state></item>
    </routeSet>
    <associationSet><item><subnetId>subnet-1</subnetId></item></associationSet>
  </item></routeTableSet>
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
    let AWSNetworking {
        route_tables,
        internet_gateways,
        nat_gateways,
        addresses,
    } = super::networking::collector::runner(&cfg).await.unwrap();
    assert_eq!(route_tables[0].route_table_id(), Some("rtb-1"));
    assert_eq!(route_tables[0].vpc_id(), Some("vpc-aaa"));
    assert_eq!(
        route_tables[0].associations()[0].subnet_id(),
        Some("subnet-1")
    );
    let [local, listed] = route_tables[0].routes() else {
        panic!("two routes, got {:?}", route_tables[0].routes());
    };
    assert_eq!(local.destination_cidr_block(), Some("10.0.0.0/16"));
    assert_eq!(local.gateway_id(), Some("local"));
    assert_eq!(listed.destination_prefix_list_id(), Some("pl-0shared"));
    assert_eq!(listed.transit_gateway_id(), Some("tgw-1"));
    assert_eq!(
        listed.state(),
        Some(&aws_sdk_ec2::types::RouteState::Blackhole)
    );
    assert_eq!(internet_gateways[0].internet_gateway_id(), Some("igw-1"));
    assert_eq!(nat_gateways[0].nat_gateway_id(), Some("nat-1"));
    assert_eq!(addresses[0].public_ip(), Some("52.1.2.3"));
}

#[tokio::test]
async fn ec2_connectivity_follows_every_page() {
    let vpcs_first = r#"<DescribeVpcsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <vpcSet><item><vpcId>vpc-a</vpcId><cidrBlock>10.0.0.0/16</cidrBlock>
    <cidrBlockAssociationSet><item><cidrBlock>10.0.0.0/16</cidrBlock><cidrBlockState><state>associated</state></cidrBlockState></item>
    <item><cidrBlock>10.5.0.0/16</cidrBlock><cidrBlockState><state>associated</state></cidrBlockState></item></cidrBlockAssociationSet>
    <ipv6CidrBlockAssociationSet><item><ipv6CidrBlock>2600:1f18:aaaa::/56</ipv6CidrBlock><ipv6CidrBlockState><state>associated</state></ipv6CidrBlockState></item></ipv6CidrBlockAssociationSet>
  </item></vpcSet>
  <nextToken>page-2</nextToken>
</DescribeVpcsResponse>"#;
    let vpcs_second = r#"<DescribeVpcsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <vpcSet><item><vpcId>vpc-b</vpcId><cidrBlock>10.1.0.0/16</cidrBlock></item></vpcSet>
</DescribeVpcsResponse>"#;
    let peerings = r#"<DescribeVpcPeeringConnectionsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <vpcPeeringConnectionSet><item>
    <vpcPeeringConnectionId>pcx-1</vpcPeeringConnectionId>
    <requesterVpcInfo><vpcId>vpc-a</vpcId></requesterVpcInfo>
    <accepterVpcInfo><vpcId>vpc-b</vpcId></accepterVpcInfo>
    <status><code>active</code><message>Active</message></status>
  </item></vpcPeeringConnectionSet>
</DescribeVpcPeeringConnectionsResponse>"#;
    let gateways = r#"<DescribeTransitGatewaysResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <transitGatewaySet><item><transitGatewayId>tgw-1</transitGatewayId><state>available</state></item></transitGatewaySet>
</DescribeTransitGatewaysResponse>"#;
    let attachments = r#"<DescribeTransitGatewayAttachmentsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <transitGatewayAttachments><item>
    <transitGatewayAttachmentId>tgw-attach-1</transitGatewayAttachmentId>
    <transitGatewayId>tgw-1</transitGatewayId>
    <resourceType>vpc</resourceType>
    <resourceId>vpc-b</resourceId>
    <state>available</state>
  </item></transitGatewayAttachments>
</DescribeTransitGatewayAttachmentsResponse>"#;
    let vpn_gateways = r#"<DescribeVpnGatewaysResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <vpnGatewaySet><item>
    <vpnGatewayId>vgw-1</vpnGatewayId>
    <state>available</state>
    <attachments><item><vpcId>vpc-a</vpcId><state>attached</state></item></attachments>
  </item></vpnGatewaySet>
</DescribeVpnGatewaysResponse>"#;
    let customer_gateways = r#"<DescribeCustomerGatewaysResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <customerGatewaySet><item>
    <customerGatewayId>cgw-1</customerGatewayId>
    <ipAddress>198.51.100.9</ipAddress>
    <state>available</state>
  </item></customerGatewaySet>
</DescribeCustomerGatewaysResponse>"#;
    let vpn_connections = r#"<DescribeVpnConnectionsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <vpnConnectionSet><item>
    <vpnConnectionId>vpn-1</vpnConnectionId>
    <state>available</state>
    <customerGatewayId>cgw-1</customerGatewayId>
    <vpnGatewayId>vgw-1</vpnGatewayId>
    <options><tunnelOptionSet><item><outsideIpAddress>192.0.2.7</outsideIpAddress></item></tunnelOptionSet></options>
    <vgwTelemetry><item><outsideIpAddress>192.0.2.8</outsideIpAddress><status>DOWN</status></item></vgwTelemetry>
  </item><item>
    <vpnConnectionId>vpn-2</vpnConnectionId>
    <state>available</state>
    <customerGatewayId>cgw-1</customerGatewayId>
    <transitGatewayId>tgw-1</transitGatewayId>
  </item></vpnConnectionSet>
</DescribeVpnConnectionsResponse>"#;
    let cfg = replay_config(&[
        (XML, vpcs_first),
        (XML, vpcs_second),
        (XML, peerings),
        (XML, gateways),
        (XML, attachments),
        (XML, vpn_gateways),
        (XML, customer_gateways),
        (XML, vpn_connections),
    ])
    .await;

    let AWSConnectivity {
        vpcs,
        peering_connections,
        transit_gateways,
        transit_gateway_attachments,
        vpn_gateways,
        customer_gateways,
        vpn_connections,
    } = super::connectivity::collector::runner(&cfg).await.unwrap();

    let vpc_ids: Vec<_> = vpcs.iter().filter_map(|vpc| vpc.vpc_id()).collect();
    assert_eq!(vpc_ids, vec!["vpc-a", "vpc-b"], "the second page is read");
    let associated: Vec<_> = vpcs[0]
        .cidr_block_association_set()
        .iter()
        .filter_map(|association| {
            association.cidr_block().zip(
                association
                    .cidr_block_state()
                    .and_then(|state| state.state()),
            )
        })
        .map(|(cidr, _)| cidr)
        .collect();
    assert_eq!(associated, vec!["10.0.0.0/16", "10.5.0.0/16"]);
    let ipv6 = &vpcs[0].ipv6_cidr_block_association_set()[0];
    assert_eq!(ipv6.ipv6_cidr_block(), Some("2600:1f18:aaaa::/56"));
    assert!(
        ipv6.ipv6_cidr_block_state()
            .and_then(|state| state.state())
            .is_some()
    );

    let pcx = &peering_connections[0];
    assert_eq!(pcx.vpc_peering_connection_id(), Some("pcx-1"));
    assert_eq!(
        pcx.status().and_then(|status| status.code()),
        Some(&aws_sdk_ec2::types::VpcPeeringConnectionStateReasonCode::Active)
    );
    assert_eq!(
        pcx.requester_vpc_info().and_then(|info| info.vpc_id()),
        Some("vpc-a")
    );
    assert_eq!(
        pcx.accepter_vpc_info().and_then(|info| info.vpc_id()),
        Some("vpc-b")
    );
    assert_eq!(transit_gateways[0].transit_gateway_id(), Some("tgw-1"));
    assert_eq!(
        transit_gateways[0].state(),
        Some(&aws_sdk_ec2::types::TransitGatewayState::Available)
    );
    let attachment = &transit_gateway_attachments[0];
    assert_eq!(attachment.transit_gateway_id(), Some("tgw-1"));
    assert_eq!(
        attachment.resource_type(),
        Some(&aws_sdk_ec2::types::TransitGatewayAttachmentResourceType::Vpc)
    );
    assert_eq!(attachment.resource_id(), Some("vpc-b"));
    assert_eq!(
        attachment.state(),
        Some(&aws_sdk_ec2::types::TransitGatewayAttachmentState::Available)
    );

    let vgw = &vpn_gateways[0];
    assert_eq!(vgw.vpn_gateway_id(), Some("vgw-1"));
    assert_eq!(vgw.state(), Some(&aws_sdk_ec2::types::VpnState::Available));
    assert_eq!(vgw.vpc_attachments()[0].vpc_id(), Some("vpc-a"));
    assert_eq!(
        vgw.vpc_attachments()[0].state(),
        Some(&aws_sdk_ec2::types::AttachmentStatus::Attached)
    );
    assert_eq!(customer_gateways[0].customer_gateway_id(), Some("cgw-1"));
    assert_eq!(customer_gateways[0].ip_address(), Some("198.51.100.9"));
    assert_eq!(customer_gateways[0].state(), Some("available"));
    let connection = &vpn_connections[0];
    assert_eq!(connection.vpn_connection_id(), Some("vpn-1"));
    assert_eq!(
        connection.state(),
        Some(&aws_sdk_ec2::types::VpnState::Available)
    );
    assert_eq!(connection.customer_gateway_id(), Some("cgw-1"));
    assert_eq!(connection.vpn_gateway_id(), Some("vgw-1"));
    assert_eq!(
        connection.options().unwrap().tunnel_options()[0].outside_ip_address(),
        Some("192.0.2.7")
    );
    assert_eq!(
        connection.vgw_telemetry()[0].outside_ip_address(),
        Some("192.0.2.8")
    );
    assert_eq!(vpn_connections[1].transit_gateway_id(), Some("tgw-1"));
}

#[tokio::test]
async fn ec2_transit_gateway_routing_follows_every_page() {
    let tables = r#"<DescribeTransitGatewayRouteTablesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <transitGatewayRouteTables>
    <item><transitGatewayRouteTableId>tgw-rtb-1</transitGatewayRouteTableId><transitGatewayId>tgw-1</transitGatewayId><state>available</state></item>
    <item><transitGatewayRouteTableId>tgw-rtb-old</transitGatewayRouteTableId><transitGatewayId>tgw-1</transitGatewayId><state>deleting</state></item>
  </transitGatewayRouteTables>
</DescribeTransitGatewayRouteTablesResponse>"#;
    let associations = r#"<GetTransitGatewayRouteTableAssociationsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <associations><item>
    <transitGatewayAttachmentId>tgw-attach-1</transitGatewayAttachmentId>
    <resourceId>vpc-b</resourceId><resourceType>vpc</resourceType><state>associated</state>
  </item></associations>
</GetTransitGatewayRouteTableAssociationsResponse>"#;
    let propagations = r#"<GetTransitGatewayRouteTablePropagationsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <transitGatewayRouteTablePropagations><item>
    <transitGatewayAttachmentId>tgw-attach-vpn</transitGatewayAttachmentId>
    <resourceId>vpn-2</resourceId><resourceType>vpn</resourceType><state>enabled</state>
  </item></transitGatewayRouteTablePropagations>
</GetTransitGatewayRouteTablePropagationsResponse>"#;
    let routes_first = r#"<SearchTransitGatewayRoutesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <routeSet><item>
    <destinationCidrBlock>10.9.0.0/16</destinationCidrBlock>
    <transitGatewayAttachments><item>
      <resourceId>tgw-2</resourceId><transitGatewayAttachmentId>tgw-attach-peer</transitGatewayAttachmentId><resourceType>peering</resourceType>
    </item></transitGatewayAttachments>
    <type>static</type><state>active</state>
  </item></routeSet>
  <nextToken>routes-2</nextToken>
</SearchTransitGatewayRoutesResponse>"#;
    let routes_second = r#"<SearchTransitGatewayRoutesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <routeSet>
    <item><destinationCidrBlock>10.66.0.0/16</destinationCidrBlock><type>static</type><state>blackhole</state></item>
    <item><prefixListId>pl-0shared</prefixListId><type>static</type><state>active</state></item>
  </routeSet>
</SearchTransitGatewayRoutesResponse>"#;
    let peering_attachments = r#"<DescribeTransitGatewayPeeringAttachmentsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <transitGatewayPeeringAttachments><item>
    <transitGatewayAttachmentId>tgw-attach-peer</transitGatewayAttachmentId>
    <requesterTgwInfo><transitGatewayId>tgw-1</transitGatewayId><region>us-east-1</region></requesterTgwInfo>
    <accepterTgwInfo><transitGatewayId>tgw-2</transitGatewayId><region>us-west-2</region></accepterTgwInfo>
    <state>available</state>
  </item></transitGatewayPeeringAttachments>
</DescribeTransitGatewayPeeringAttachmentsResponse>"#;
    let (cfg, http) = replay(&[
        (XML, tables),
        (XML, associations),
        (XML, propagations),
        (XML, routes_first),
        (XML, routes_second),
        (XML, peering_attachments),
    ])
    .await;

    let AWSTransitGatewayRouting {
        route_tables,
        peerings,
    } = super::connectivity::collector::transit_gateway_routing(&cfg)
        .await
        .unwrap();

    assert_eq!(
        route_tables.len(),
        1,
        "a table being deleted is not read further"
    );
    let routing = &route_tables[0];
    assert_eq!(
        routing.table.transit_gateway_route_table_id(),
        Some("tgw-rtb-1")
    );
    assert_eq!(routing.table.transit_gateway_id(), Some("tgw-1"));
    let association = &routing.associations[0];
    assert_eq!(association.resource_id(), Some("vpc-b"));
    assert!(association.resource_type().is_some() && association.state().is_some());
    let propagation = &routing.propagations[0];
    assert_eq!(propagation.resource_id(), Some("vpn-2"));
    assert!(propagation.resource_type().is_some() && propagation.state().is_some());
    let destinations: Vec<_> = routing
        .static_routes
        .iter()
        .filter_map(|route| route.destination_cidr_block())
        .collect();
    assert_eq!(
        destinations,
        vec!["10.9.0.0/16", "10.66.0.0/16"],
        "every page of routes"
    );
    assert_eq!(
        routing.static_routes[2].prefix_list_id(),
        Some("pl-0shared")
    );
    let target = &routing.static_routes[0].transit_gateway_attachments()[0];
    assert_eq!(
        target.transit_gateway_attachment_id(),
        Some("tgw-attach-peer")
    );
    assert!(target.resource_type().is_some() && routing.static_routes[0].state().is_some());
    let peering = &peerings[0];
    assert_eq!(
        peering.transit_gateway_attachment_id(),
        Some("tgw-attach-peer")
    );
    assert_eq!(
        peering
            .accepter_tgw_info()
            .and_then(|side| side.transit_gateway_id()),
        Some("tgw-2")
    );
    assert!(peering.state().is_some());
    let search = request_bodies(&http)
        .into_iter()
        .find(|body| body.contains("SearchTransitGatewayRoutes"))
        .expect("the static routes are searched");
    assert!(
        search.contains("tgw-rtb-1") && search.contains("static") && search.contains("blackhole"),
        "{search}"
    );
}

#[tokio::test]
async fn elbv2_load_balancers_and_target_groups() {
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
    let AWSLoadBalancing {
        load_balancers,
        target_groups,
        ..
    } = super::load_balancer::collector::runner(&cfg).await.unwrap();
    assert_eq!(load_balancers[0].load_balancer_name(), Some("my-lb"));
    assert_eq!(target_groups[0].target_group_name(), Some("my-tg"));
}

#[tokio::test]
async fn network_interfaces() {
    let body = r#"<DescribeNetworkInterfacesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
      <networkInterfaceSet>
        <item>
          <networkInterfaceId>eni-alb-1a</networkInterfaceId>
          <subnetId>subnet-public-1a</subnetId>
          <vpcId>vpc-1</vpcId>
          <description>ELB app/globex/1</description>
          <interfaceType>network_load_balancer</interfaceType>
          <privateIpAddressesSet>
            <item><privateIpAddress>10.10.1.50</privateIpAddress></item>
          </privateIpAddressesSet>
          <groupSet>
            <item><groupId>sg-web</groupId></item>
          </groupSet>
        </item>
      </networkInterfaceSet>
    </DescribeNetworkInterfacesResponse>"#;
    let cfg = replay_config(&[(XML, body)]).await;
    let interfaces = super::network_interface::collector::runner(&cfg)
        .await
        .unwrap();

    let eni = &interfaces[0];
    assert_eq!(eni.network_interface_id(), Some("eni-alb-1a"));
    assert_eq!(eni.subnet_id(), Some("subnet-public-1a"));
    assert_eq!(eni.vpc_id(), Some("vpc-1"));
    assert_eq!(
        eni.description(),
        Some("ELB app/globex/1"),
        "the description is the only thing that names the owning balancer"
    );
    assert_eq!(
        eni.private_ip_addresses()[0].private_ip_address(),
        Some("10.10.1.50"),
        "the address is the whole point; a flow record resolves through it"
    );
    assert_eq!(eni.groups()[0].group_id(), Some("sg-web"));
}

#[tokio::test]
async fn network_interfaces_follow_pagination_past_the_first_page() {
    let first = r#"<DescribeNetworkInterfacesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
      <networkInterfaceSet>
        <item><networkInterfaceId>eni-1</networkInterfaceId></item>
      </networkInterfaceSet>
      <nextToken>page-2</nextToken>
    </DescribeNetworkInterfacesResponse>"#;
    let second = r#"<DescribeNetworkInterfacesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
      <networkInterfaceSet>
        <item><networkInterfaceId>eni-2</networkInterfaceId></item>
      </networkInterfaceSet>
    </DescribeNetworkInterfacesResponse>"#;
    let cfg = replay_config(&[(XML, first), (XML, second)]).await;
    let interfaces = super::network_interface::collector::runner(&cfg)
        .await
        .unwrap();

    let ids: Vec<_> = interfaces
        .iter()
        .filter_map(|e| e.network_interface_id())
        .collect();
    assert_eq!(ids, vec!["eni-1", "eni-2"]);
}

#[tokio::test]
async fn direct_connect_follows_every_page_of_connections_and_interfaces() {
    let connections_first = r#"{"connections":[{"connectionId":"dxcon-1","connectionState":"available","location":"EqDC2"}],"nextToken":"page-2"}"#;
    let connections_second =
        r#"{"connections":[{"connectionId":"dxcon-2","connectionState":"down"}]}"#;
    let interfaces = r#"{"virtualInterfaces":[{
        "virtualInterfaceId":"dxvif-1","virtualInterfaceType":"private","virtualInterfaceState":"available",
        "connectionId":"dxcon-1","virtualGatewayId":"vgw-1","directConnectGatewayId":"dxgw-1",
        "amazonAddress":"169.254.1.1/30","customerAddress":"169.254.1.2/30",
        "bgpPeers":[{"amazonAddress":"169.254.1.1/30","customerAddress":"169.254.1.2/30","bgpPeerState":"available"}]
    }]}"#;
    let (cfg, http) = replay(&[
        (JSON11, connections_first),
        (JSON11, connections_second),
        (JSON11, interfaces),
    ])
    .await;

    let AWSDirectConnect {
        connections,
        virtual_interfaces,
    } = super::direct_connect::collector::runner(&cfg)
        .await
        .unwrap();

    let connection_ids: Vec<_> = connections
        .iter()
        .filter_map(|connection| connection.connection_id())
        .collect();
    assert_eq!(connection_ids, vec!["dxcon-1", "dxcon-2"]);
    assert!(connections[0].connection_state().is_some());

    let interface = &virtual_interfaces[0];
    assert_eq!(interface.virtual_interface_id(), Some("dxvif-1"));
    assert_eq!(interface.virtual_interface_type(), Some("private"));
    assert!(interface.virtual_interface_state().is_some());
    assert_eq!(interface.connection_id(), Some("dxcon-1"));
    assert_eq!(interface.virtual_gateway_id(), Some("vgw-1"));
    assert_eq!(interface.direct_connect_gateway_id(), Some("dxgw-1"));
    assert_eq!(interface.amazon_address(), Some("169.254.1.1/30"));
    assert_eq!(interface.customer_address(), Some("169.254.1.2/30"));
    let peer = &interface.bgp_peers()[0];
    assert_eq!(peer.amazon_address(), Some("169.254.1.1/30"));
    assert_eq!(peer.customer_address(), Some("169.254.1.2/30"));
    assert!(peer.bgp_peer_state().is_some());

    let requests = request_bodies(&http);
    assert_eq!(requests.len(), 3);
    assert!(requests[1].contains("page-2"), "{}", requests[1]);
}

#[tokio::test]
async fn direct_connect_gateways_ask_each_gateway_for_its_associations() {
    let gateways = r#"{"directConnectGateways":[
        {"directConnectGatewayId":"dxgw-1","directConnectGatewayState":"available"},
        {"directConnectGatewayId":"dxgw-2","directConnectGatewayState":"available"}
    ]}"#;
    let first_associations = r#"{"directConnectGatewayAssociations":[{"directConnectGatewayId":"dxgw-1","associationState":"associated","associatedGateway":{"id":"vgw-1","type":"virtualPrivateGateway"}}],"nextToken":"assoc-2"}"#;
    let more_associations = r#"{"directConnectGatewayAssociations":[{"directConnectGatewayId":"dxgw-1","associationState":"updating","associatedGateway":{"id":"tgw-1","type":"transitGateway"}}]}"#;
    let no_associations = r#"{"directConnectGatewayAssociations":[]}"#;
    let (cfg, http) = replay(&[
        (JSON11, gateways),
        (JSON11, first_associations),
        (JSON11, more_associations),
        (JSON11, no_associations),
    ])
    .await;

    let AWSDirectConnectGateways {
        gateways,
        associations,
    } = super::direct_connect::collector::gateways(&cfg)
        .await
        .unwrap();

    assert_eq!(gateways.len(), 2);
    assert!(gateways[0].direct_connect_gateway_state().is_some());
    let associated: Vec<_> = associations
        .iter()
        .filter_map(|association| association.associated_gateway())
        .filter_map(|gateway| gateway.id().zip(gateway.r#type()))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(associated, vec!["vgw-1", "tgw-1"]);
    assert!(associations[0].association_state().is_some());
    assert_eq!(associations[0].direct_connect_gateway_id(), Some("dxgw-1"));

    let requests = request_bodies(&http);
    assert!(requests[1].contains("dxgw-1"), "{}", requests[1]);
    assert!(
        requests[2].contains("dxgw-1") && requests[2].contains("assoc-2"),
        "{}",
        requests[2]
    );
    assert!(requests[3].contains("dxgw-2"), "{}", requests[3]);
}

fn request_bodies(http: &StaticReplayClient) -> Vec<String> {
    http.actual_requests()
        .map(|request| {
            String::from_utf8_lossy(request.body().bytes().unwrap_or_default()).into_owned()
        })
        .collect()
}

#[tokio::test]
async fn direct_connect_refuses_a_page_token_that_repeats() {
    let page = r#"{"connections":[{"connectionId":"dxcon-1"}],"nextToken":"stuck"}"#;
    let cfg = replay_config(&[(JSON11, page), (JSON11, page)]).await;
    let error = super::direct_connect::collector::runner(&cfg)
        .await
        .expect_err("a token that never advances must fail, not loop or truncate");
    assert!(error.to_string().contains("repeated"), "{error}");
}

#[tokio::test]
async fn direct_connect_refuses_a_page_token_that_comes_back() {
    let first = r#"{"connections":[{"connectionId":"dxcon-1"}],"nextToken":"a"}"#;
    let second = r#"{"connections":[{"connectionId":"dxcon-2"}],"nextToken":"b"}"#;
    let cfg = replay_config(&[(JSON11, first), (JSON11, second), (JSON11, first)]).await;
    let error = super::direct_connect::collector::runner(&cfg)
        .await
        .expect_err("a cursor that cycles must fail, not loop");
    assert!(error.to_string().contains("repeated"), "{error}");
}
