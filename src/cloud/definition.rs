use aws_sdk_config::types::ResourceIdentifier as AWSResource;
use aws_sdk_ec2::types::Instance as AWSInstance;
use aws_sdk_ecs::types::Cluster as AWSCluster;
use aws_sdk_eventbridge::types::EventBus as AWSEventbridge;
use aws_sdk_iam::types::{
    Group as AWSGroup, Policy as AWSPolicy, Role as AWSRole, User as AWSUser,
};
use aws_sdk_lambda::types::FunctionConfiguration as AWSLambda;
use aws_sdk_networkmanager::types::GlobalNetwork as AWSNetwork;
use std::collections::HashMap;

#[derive(Debug)]
pub enum CloudError {
    AwsEC2Error(aws_sdk_ec2::Error),
    AwsConfigError(aws_sdk_config::Error),
}

#[derive(Debug)]
pub enum Provider {
    AWS(Vec<(&str, AmazonCollection)>),
    GCP(Vec<GoogleCollection>),
    Azure(Vec<MicrosoftCollection>),
}

#[derive(Debug)]
pub enum AmazonCollection {
    AmazonInstances(Vec<AWSInstance>),
    AmazonNetworks(Vec<AWSNetwork>),
    AmazonClusters(Vec<AWSCluster>),
    AmazonLambdas(Vec<AWSLambda>),
    AmazonEventbridge(Vec<AWSEventbridge>),
    AmazonResources(HashMap<String, Vec<AWSResource>>),
    AmazonIAM {
        groups: Vec<AWSGroup>,
        policies: Vec<AWSPolicy>,
        roles: Vec<AWSRole>,
        users: Vec<AWSUser>,
    },
}

#[derive(Debug)]
pub enum GoogleCollection {
    GoogleInstances,
}

#[derive(Debug)]
pub enum MicrosoftCollection {
    MicrosoftInstances,
}
