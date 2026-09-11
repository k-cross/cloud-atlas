pub mod api_gateway;
pub mod cloudfront;
pub mod container_service;
pub mod dynamodb;
pub mod eks;
pub mod eventbridge;
pub mod events;
pub mod flow_logs;
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

pub async fn load_config(region: &str) -> aws_config::SdkConfig {
    aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(region.to_owned()))
        .load()
        .await
}

pub async fn resolve_credentials(
    config: &aws_config::SdkConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    use aws_credential_types::provider::ProvideCredentials;

    match config.credentials_provider() {
        Some(provider) => {
            provider.provide_credentials().await?;
            Ok(())
        }
        None => Err("no credentials provider in the chain".into()),
    }
}
