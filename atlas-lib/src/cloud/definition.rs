use aws_sdk_config::types::ResourceIdentifier as AWSResource;
use aws_sdk_ec2::types::Instance as AWSInstance;
use aws_sdk_ecs::types::Cluster as AWSCluster;
use aws_sdk_elasticloadbalancingv2::types::{
    Listener as AWSListener, LoadBalancer as AWSLoadBalancer, TargetGroup as AWSTargetGroup,
    TargetHealthDescription as AWSTargetHealthDescription,
};
use aws_sdk_eventbridge::types::EventBus as AWSEventbridge;
use aws_sdk_lambda::types::FunctionConfiguration as AWSLambda;
use std::collections::HashMap;

#[derive(Debug)]
pub enum CloudError {
    AwsEC2Error(aws_sdk_ec2::Error),
    AwsConfigError(aws_sdk_config::Error),
}

#[derive(Debug)]
pub enum Provider {
    AWS(Vec<(String, AmazonCollection)>),
    GCP(Vec<GoogleCollection>),
    Azure(Vec<MicrosoftCollection>),
}

#[derive(Debug)]
pub enum AmazonCollection {
    AmazonInstances(Vec<AWSInstance>),
    AmazonClusters(Vec<AWSCluster>),
    AmazonLambdas(Vec<AWSLambda>),
    AmazonEventbridge(Vec<AWSEventbridge>),
    AmazonResources(HashMap<String, Vec<AWSResource>>),
    AmazonLoadBalancers {
        load_balancers: Vec<AWSLoadBalancer>,
        target_groups: Vec<AWSTargetGroup>,
        listeners: Vec<AWSListener>,
        target_health: HashMap<String, Vec<AWSTargetHealthDescription>>,
    },
    AmazonRoute53 {
        hosted_zones: Vec<aws_sdk_route53::types::HostedZone>,
        record_sets: Vec<aws_sdk_route53::types::ResourceRecordSet>,
    },
    AmazonEks(Vec<aws_sdk_eks::types::Cluster>),
    AmazonApiGateway(Vec<aws_sdk_apigateway::types::RestApi>),
    AmazonRds(Vec<aws_sdk_rds::types::DbInstance>),
    AmazonDynamoDb(Vec<String>), // Table names
    AmazonSqs(Vec<String>),      // Queue URLs
    AmazonSns(Vec<aws_sdk_sns::types::Topic>),
    AmazonCloudFront(Vec<aws_sdk_cloudfront::types::DistributionSummary>),
    AmazonSecurityGroups(Vec<aws_sdk_ec2::types::SecurityGroup>),
}

#[derive(Debug)]
pub enum GoogleCollection {
    GoogleInstances(Vec<google_compute1::api::Instance>),
}

#[derive(Debug)]
pub enum MicrosoftCollection {
    MicrosoftInstances,
}
