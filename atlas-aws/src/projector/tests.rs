use super::*;

#[test]
fn every_aws_kind_config_records_has_one_source() {
    use aws_sdk_config::types::ResourceType;

    let config_records_nothing_like = [
        "AwsRegion",
        "AwsTag",
        "AwsEc2AvailabilityZone",
        "AwsRoute53RecordSet",
        "AwsConfigResource",
        "AwsDirectConnectConnection",
        "AwsDirectConnectGateway",
        "AwsDirectConnectVirtualInterface",
        "AwsEc2Route",
        "AwsEc2TransitGatewayRoute",
        "AwsEc2TransitGatewayPeeringAttachment",
    ];
    let typed: Vec<&str> = TYPED_CONFIG_TYPES
        .iter()
        .map(|typed| (typed.node)("probe".into()).kind())
        .collect();
    for kind in Node::ALL_KINDS
        .iter()
        .filter(|kind| kind.starts_with("Aws"))
    {
        assert!(
            typed.contains(kind) != config_records_nothing_like.contains(kind),
            "{kind}: add its Config type to TYPED_CONFIG_TYPES, or list it as a kind Config \
             does not record"
        );
    }
    for typed in TYPED_CONFIG_TYPES {
        assert!(
            ResourceType::values().contains(&typed.resource_type),
            "{} is not a Config resource type",
            typed.resource_type
        );
        assert!(
            !use_aws_resource(typed.resource_type, false),
            "{} has a typed node, so the catch-all must never draw it",
            typed.resource_type
        );
    }
}

#[test]
fn a_config_listing_never_redraws_a_resource_its_own_collector_draws() {
    use aws_sdk_config::types::ResourceIdentifier;
    use std::collections::HashMap;

    let listed = |id: &str| vec![ResourceIdentifier::builder().resource_id(id).build()];
    let mut resources: HashMap<String, Vec<ResourceIdentifier>> = TYPED_CONFIG_TYPES
        .iter()
        .map(|typed| (typed.resource_type.to_owned(), listed("twin")))
        .collect();
    for drawn_as_an_edge in [
        "AWS::EC2::TransitGatewayAttachment",
        "AWS::ElasticLoadBalancingV2::Listener",
    ] {
        resources.insert(drawn_as_an_edge.to_owned(), listed("twin"));
    }
    resources.insert("AWS::S3::Bucket".to_owned(), listed("assets"));

    let mut builder = GraphBuilder::new();
    aws_projector(
        &mut builder,
        &[(
            "us-east-1".to_owned(),
            AmazonCollection::AmazonResources(resources),
        )],
        &Settings {
            regions: vec!["us-east-1".to_owned()],
            ..Default::default()
        },
    );

    let generic: Vec<&Node> = builder
        .graph
        .node_weights()
        .filter(|node| matches!(node, Node::AwsConfigResource { .. }))
        .collect();
    assert_eq!(
        generic,
        vec![&Node::AwsConfigResource {
            resource_type: "AWS::S3::Bucket".into(),
            id: "assets".into(),
        }],
        "Config is the source only for what no collector draws"
    );
}
