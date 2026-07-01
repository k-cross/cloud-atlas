use crate::Settings;
use crate::cloud::amazon::{
    container_service, eventbridge, instance, lambda, load_balancer, resource, route53,
};
use crate::cloud::definition::Provider;

pub async fn build_aws(
    verbose: bool,
    opts: &Settings,
) -> Result<Provider, Box<dyn std::error::Error>> {
    let mut services = Vec::new();

    for r in opts.regions.clone() {
        let (ecs, eb, insts, lambdas, load_balancers, ress, route53) = tokio::try_join!(
            container_service::collector::runner(r.as_str()),
            eventbridge::collector::runner(r.as_str()),
            instance::collector::runner(r.as_str()),
            lambda::collector::runner(r.as_str()),
            load_balancer::collector::runner(r.as_str()),
            resource::collector::runner(verbose, r.as_str()),
            route53::collector::runner(r.as_str()),
        )?;

        services.push((r.to_owned(), ecs));
        services.push((r.to_owned(), eb));
        services.push((r.to_owned(), insts));
        services.push((r.to_owned(), lambdas));
        services.push((r.to_owned(), load_balancers));
        services.push((r.to_owned(), ress));
        services.push((r.to_owned(), route53));
    }

    Ok(Provider::AWS(services))
}
