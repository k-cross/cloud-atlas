use crate::Settings;
use crate::cloud::amazon::{
    api_gateway, cloudfront, container_service, dynamodb, eks, eventbridge, instance, lambda,
    load_balancer, rds, resource, route53, security_group, sns, sqs,
};
use crate::cloud::definition::Provider;

pub async fn build_aws(
    verbose: bool,
    opts: &Settings,
) -> Result<Provider, Box<dyn std::error::Error>> {
    let mut services = Vec::new();

    for r in opts.regions.clone() {
        let (
            ecs,
            eb,
            insts,
            lambdas,
            load_balancers,
            ress,
            route53,
            eks,
            api_gateway,
            rds,
            dynamodb,
            sqs,
            sns,
            cloudfront,
            security_groups,
        ) = tokio::try_join!(
            container_service::collector::runner(r.as_str()),
            eventbridge::collector::runner(r.as_str()),
            instance::collector::runner(r.as_str()),
            lambda::collector::runner(r.as_str()),
            load_balancer::collector::runner(r.as_str()),
            resource::collector::runner(verbose, r.as_str()),
            route53::collector::runner(r.as_str()),
            eks::collector::runner(r.as_str()),
            api_gateway::collector::runner(r.as_str()),
            rds::collector::runner(r.as_str()),
            dynamodb::collector::runner(r.as_str()),
            sqs::collector::runner(r.as_str()),
            sns::collector::runner(r.as_str()),
            cloudfront::collector::runner(r.as_str()),
            security_group::collector::runner(r.as_str()),
        )?;

        services.push((r.to_owned(), ecs));
        services.push((r.to_owned(), eb));
        services.push((r.to_owned(), insts));
        services.push((r.to_owned(), lambdas));
        services.push((r.to_owned(), load_balancers));
        services.push((r.to_owned(), ress));
        services.push((r.to_owned(), route53));
        services.push((r.to_owned(), eks));
        services.push((r.to_owned(), api_gateway));
        services.push((r.to_owned(), rds));
        services.push((r.to_owned(), dynamodb));
        services.push((r.to_owned(), sqs));
        services.push((r.to_owned(), sns));
        services.push((r.to_owned(), cloudfront));
        services.push((r.to_owned(), security_groups));
    }

    Ok(Provider::AWS(services))
}
