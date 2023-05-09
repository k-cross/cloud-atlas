use crate::cloud::definition::Provider;
use crate::cloud::amazon::{
    container_service, 
    eventbridge,
    iam, 
    instance, 
    lambda,
    network,
    resource,
    lambda,
    eventbridge,
};

pub async fn build_aws(verbose: bool, regions: Vec<String>) -> Result<Provider, Box<dyn std::error::Error>> {
    let mut services = Vec::new();

    for r in regions {
      // TODO: add conditionals from options to remove services if needed
      services.push((r.as_str(), resource::collector::runner(verbose, r.as_str())));
      services.push((r.as_str(), instance::collector::runner(r.as_str())));
      services.push((r.as_str(), network::collector::runner(r.as_str())));
      services.push((r.as_str(), container_service::collector::runner(r.as_str())));
      services.push((r.as_str(), lambda::collector::runner(r.as_str())));
      services.push((r.as_str(), eventbridge::collector::runner(r.as_str())));
      services.push((r.as_str(), iam::collector::runner(r.as_str())));
    }

    services = services.into_iter().map(|x, y| (x, y.await?) ).collect();

    Ok(Provider::AWS(services))
}
