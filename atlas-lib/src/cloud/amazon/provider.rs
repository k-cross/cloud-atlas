use crate::Settings;
use crate::cloud::amazon::{
    api_gateway, cloudfront, container_service, dynamodb, eks, eventbridge, instance, lambda,
    load_balancer, rds, resource, route53, security_group, sns, sqs,
};
use crate::cloud::definition::{AmazonCollection, Provider};

pub async fn build_aws(
    verbose: bool,
    opts: &Settings,
) -> Result<Provider, Box<dyn std::error::Error>> {
    let mut services = Vec::new();

    let mut futures = Vec::new();

    for r in opts.regions.clone() {
        futures.push(async move {
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

            let local_services = vec![
                (r.to_owned(), ecs),
                (r.to_owned(), eb),
                (r.to_owned(), insts),
                (r.to_owned(), lambdas),
                (r.to_owned(), load_balancers),
                (r.to_owned(), ress),
                (r.to_owned(), route53),
                (r.to_owned(), eks),
                (r.to_owned(), api_gateway),
                (r.to_owned(), rds),
                (r.to_owned(), dynamodb),
                (r.to_owned(), sqs),
                (r.to_owned(), sns),
                (r.to_owned(), cloudfront),
                (r.to_owned(), security_groups),
            ];

            Ok::<Vec<(String, AmazonCollection)>, Box<dyn std::error::Error>>(local_services)
        });
    }

    let results = futures::future::try_join_all(futures).await?;
    for mut res in results {
        services.append(&mut res);
    }

    Ok(Provider::AWS(services))
}
