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
                r_ecs,
                r_eb,
                r_insts,
                r_lambdas,
                r_load_balancers,
                r_ress,
                r_route53,
                r_eks,
                r_api_gateway,
                r_rds,
                r_dynamodb,
                r_sqs,
                r_sns,
                r_cloudfront,
                r_security_groups,
            ) = tokio::join!(
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
            );

            let mut local_services = Vec::new();

            let mut add_if_ok = |res: Result<AmazonCollection, Box<dyn std::error::Error>>| {
                if let Ok(collection) = res {
                    local_services.push((r.to_owned(), collection));
                } else if let Err(e) = res {
                    eprintln!("Error fetching AWS resource in region {}: {:?}", r, e);
                }
            };

            add_if_ok(r_ecs);
            add_if_ok(r_eb);
            add_if_ok(r_insts);
            add_if_ok(r_lambdas);
            add_if_ok(r_load_balancers);
            add_if_ok(r_ress);
            add_if_ok(r_route53);
            add_if_ok(r_eks);
            add_if_ok(r_api_gateway);
            add_if_ok(r_rds);
            add_if_ok(r_dynamodb);
            add_if_ok(r_sqs);
            add_if_ok(r_sns);
            add_if_ok(r_cloudfront);
            add_if_ok(r_security_groups);

            Ok::<Vec<(String, AmazonCollection)>, Box<dyn std::error::Error>>(local_services)
        });
    }

    let results = futures::future::try_join_all(futures).await?;
    for mut res in results {
        services.append(&mut res);
    }

    Ok(Provider::AWS(services))
}
