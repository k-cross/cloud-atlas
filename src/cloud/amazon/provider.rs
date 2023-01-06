use crate::cloud::amazon::{instance, resource};
use crate::cloud::definition::Provider;

pub async fn build_aws(verbose: bool, region: String) -> Result<Provider, Box<dyn std::error::Error>> {
    let configs = resource::collector::runner(verbose, region.as_str()).await?;
    let (running_insts, _offline_insts) = instance::collector::runner(region.as_str()).await?;

    // TODO finish up here turn everything into provider
    Ok(Provider::AWS(vec![running_insts, configs]))
}
