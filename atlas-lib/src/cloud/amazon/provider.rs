use crate::cloud::amazon::{
    container_service, eventbridge, instance, lambda, resource,
};
use crate::cloud::definition::Provider;
use crate::Settings;

pub async fn build_aws(verbose: bool, opts: &Settings) -> Result<Provider, Box<dyn std::error::Error>> {
    let mut services = Vec::new();

    for r in opts.regions.clone() {
        // TODO: add conditionals from options to remove services if needed
        let ecs = container_service::collector::runner(r.as_str()).await?;
        services.push((r.to_owned(), ecs));

        let eb = eventbridge::collector::runner(r.as_str()).await?;
        services.push((r.to_owned(), eb));

        let insts = instance::collector::runner(r.as_str()).await?;
        services.push((r.to_owned(), insts));

        let lambdas = lambda::collector::runner(r.as_str()).await?;
        services.push((r.to_owned(), lambdas));

        let ress = resource::collector::runner(verbose, r.as_str()).await?;
        services.push((r.to_owned(), ress));
    }

    Ok(Provider::AWS(services))
}
