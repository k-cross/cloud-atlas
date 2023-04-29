use aws_sdk_config::model::ResourceIdentifier as AWSResource;
use aws_sdk_ec2::model::Instance as AWSInstance;
use aws_sdk_networkmanager::model::GlobalNetwork as AWSNetwork;
use std::collections::HashMap;

#[derive(Debug)]
pub enum CloudError {
    AwsEC2Error(aws_sdk_ec2::Error),
    AwsConfigError(aws_sdk_config::Error),
}

#[derive(Debug)]
pub enum Provider {
    AWS(Vec<AmazonCollection>),
    GCP(Vec<GoogleCollection>),
    Azure(Vec<MicrosoftCollection>),
}

#[derive(Debug)]
pub enum AmazonCollection {
    AmazonInstances(Vec<AWSInstance>),
    AmazonNetworks(Vec<AWSNetwork>),
    AmazonResources(HashMap<String, Vec<AWSResource>>),
}

#[derive(Debug)]
pub enum GoogleCollection {
    GoogleInstances,
}

#[derive(Debug)]
pub enum MicrosoftCollection {
    MicrosoftInstances,
}
