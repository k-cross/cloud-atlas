pub mod api_gateway;
pub mod cloudfront;
pub mod container_service;
pub mod dynamodb;
pub mod eks;
pub mod eventbridge;
pub mod instance;
pub mod lambda;
pub mod load_balancer;
pub mod networking;
pub mod provider;
pub mod rds;
pub mod resource;
pub mod route53;
pub mod security_group;
pub mod sns;
pub mod sqs;

#[cfg(test)]
mod collector_tests;

/// Load the shared AWS SDK config for a region once, so every collector in
/// that region reuses it instead of re-resolving the credential chain.
pub async fn load_config(region: &str) -> aws_config::SdkConfig {
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(region.to_owned()))
        .load()
        .await
}
