use aws_sdk_config::model::ResourceIdentifier as AWSResource;
use aws_sdk_ec2::model::Instance as AWSInstance;
use std::collections::HashMap;

pub enum CloudError {
    AwsEC2Error(aws_sdk_ec2::Error),
    AwsConfigError(aws_sdk_config::Error),
}

pub enum Provider {
    AWS(Vec<AmazonCollection>),
    GCP(Vec<GoogleCollection>),
    Azure(Vec<MicrosoftCollection>),
}

pub enum AmazonCollection {
    AmazonInstance(Vec<AWSInstance>),
    AmazonResource(HashMap<String, Vec<AWSResource>>),
}

pub enum GoogleCollection {
    GoogleInstance,
}

pub enum MicrosoftCollection {
    MicrosoftInstance,
}
