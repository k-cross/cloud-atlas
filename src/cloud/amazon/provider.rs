use crate::cloud::amazon::{
    container_service, 
    instance, 
    network,
    resource,
    iam,
    lambda,
    eventbridge,
};
use crate::cloud::definition::Provider;

pub async fn build_aws(verbose: bool, region: String) -> Result<Provider, Box<dyn std::error::Error>> {
    let resource_col  = resource::collector::runner(verbose, region.as_str());
    let instance_col  = instance::collector::runner(region.as_str());
    let network_col   = network::collector::runner(region.as_str());
    let container_col = container_service::collector::runner(region.as_str());
    let lambda_col    = lambda::collector::runner(region.as_str());
    let evts_col      = eventbridge::collector::runner(region.as_str());
    let iam_col       = iam::collector::runner(region.as_str());

    // await for the collectors to finish
    let  nets           =  network_col.await?;
    let  resources      =  resource_col.await?;
    let  running_insts  =  instance_col.await?;
    let  conts          =  container_col.await?;
    let  funcs          =  lambda_col.await?;
    let  evts           =  evts_col.await?;
    let  iam            =  iam_col.await?;

    Ok(Provider::AWS(vec![running_insts, resources, nets, conts, funcs, evts, iam]))
}
