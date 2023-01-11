use crate::cloud::amazon::{instance, resource};
use crate::cloud::definition::Provider;

pub async fn build_aws(verbose: bool, region: String) -> Result<Provider, Box<dyn std::error::Error>> {
    let c = resource::collector::runner(verbose, region.as_str());
    let i = instance::collector::runner(region.as_str());
    let configs = c.await?;
    let (running_insts, _offline_insts) = i.await?;

    Ok(Provider::AWS(vec![running_insts, configs]))
}
