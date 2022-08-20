pub enum Error {
    AwsEC2Error(aws_sdk_ec2::Error),
    AwsConfigError(aws_sdk_config::Error),
}