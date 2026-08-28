use crate::Settings;
use crate::atlas::collection::{CollectionFailure, CollectionSource, ProviderScan};
use crate::cloud::amazon::{
    api_gateway, cloudfront, container_service, dynamodb, eks, eventbridge, instance, lambda,
    load_balancer, networking, rds, resource, route53, security_group, sns, sqs,
};
use crate::cloud::definition::{AmazonCollection, Provider};

pub async fn build_aws(
    verbose: bool,
    opts: &Settings,
) -> Result<ProviderScan, Box<dyn std::error::Error>> {
    let mut services = Vec::new();
    let mut failures = Vec::new();

    let mut futures = Vec::new();

    for r in opts.regions.clone() {
        futures.push(async move {
            let config = super::load_config(&r).await;

            let results = tokio::join!(
                container_service::collector::runner(&config),
                eventbridge::collector::runner(&config),
                instance::collector::runner(&config),
                lambda::collector::runner(&config),
                load_balancer::collector::runner(&config),
                resource::collector::runner(verbose, &config),
                route53::collector::runner(&config),
                eks::collector::runner(&config),
                api_gateway::collector::runner(&config),
                rds::collector::runner(&config),
                dynamodb::collector::runner(&config),
                sqs::collector::runner(&config),
                sns::collector::runner(&config),
                cloudfront::collector::runner(&config),
                security_group::collector::runner(&config),
                networking::collector::runner(&config),
            );

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
                r_networking,
            ) = results;

            let mut local_services = Vec::new();
            let mut local_failures = Vec::new();

            let mut add_if_ok =
                |collector: &str, res: Result<AmazonCollection, Box<dyn std::error::Error>>| {
                    match res {
                        Ok(collection) => local_services.push((r.to_owned(), collection)),
                        Err(e) => local_failures.push(CollectionFailure {
                            source: CollectionSource::Aws,
                            scope: format!("{r}/{collector}"),
                            message: format!("{e:?}"),
                        }),
                    }
                };

            add_if_ok("ecs", r_ecs);
            add_if_ok("eventbridge", r_eb);
            add_if_ok("ec2", r_insts);
            add_if_ok("lambda", r_lambdas);
            add_if_ok("elbv2", r_load_balancers);
            add_if_ok("config", r_ress);
            add_if_ok("route53", r_route53);
            add_if_ok("eks", r_eks);
            add_if_ok("apigateway", r_api_gateway);
            add_if_ok("rds", r_rds);
            add_if_ok("dynamodb", r_dynamodb);
            add_if_ok("sqs", r_sqs);
            add_if_ok("sns", r_sns);
            add_if_ok("cloudfront", r_cloudfront);
            add_if_ok("security_groups", r_security_groups);
            add_if_ok("networking", r_networking);

            Ok::<_, Box<dyn std::error::Error>>((local_services, local_failures))
        });
    }

    let results = futures::future::try_join_all(futures).await?;
    for (mut region_services, mut region_failures) in results {
        services.append(&mut region_services);
        failures.append(&mut region_failures);
    }

    Ok(ProviderScan {
        provider: Provider::AWS(services),
        failures,
    })
}
