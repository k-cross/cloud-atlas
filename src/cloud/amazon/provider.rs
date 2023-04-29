use crate::cloud::amazon::{instance, network, resource};
use crate::cloud::definition::Provider;

pub async fn build_aws(verbose: bool, region: String) -> Result<Provider, Box<dyn std::error::Error>> {
    let c = resource::collector::runner(verbose, region.as_str());
    let i = instance::collector::runner(region.as_str());
    let n = network::collector::runner(region.as_str());

    // await for the collectors to finish
    let nets = n.await?;
    let configs = c.await?;
    let (running_insts, _offline_insts) = i.await?;

    Ok(Provider::AWS(vec![running_insts, configs, nets]))
}
