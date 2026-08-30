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

/// Resolve the credential chain once, before fanning the region's collectors
/// out.
///
/// The SDK resolves credentials lazily at the first request, so a chain that
/// cannot produce any would otherwise surface as sixteen separate collector
/// errors — all boxed, all therefore read as `Unavailable`, all holding the
/// provider for the full retention budget when the real problem needs a human.
/// Probing here turns that into one accurate `Unauthorized` for the region.
///
/// This is the same call the SDK makes internally, so a failure is conclusive.
/// A pass is not a promise: credentials that resolve but are already expired
/// still fail at request time and land as `Unavailable`.
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
