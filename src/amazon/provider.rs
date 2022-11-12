use crate::amazon::{instance, resource};

pub async fn build_provider(config, region) {
    let configs = resource::collector::runner(verbose, region.as_str()).await?;
    let (running_insts, _offline_insts) = instance::collector::runner(region.as_str()).await?;

    // TODO finish up here turn everything into provider
}
