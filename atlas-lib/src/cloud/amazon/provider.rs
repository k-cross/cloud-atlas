use crate::Settings;
use crate::atlas::collection::{CollectionFailure, CollectionSource, ProviderScan};
use crate::cloud::amazon::{
    api_gateway, cloudfront, container_service, dynamodb, eks, eventbridge, instance, lambda,
    load_balancer, networking, rds, resource, route53, security_group, sns, sqs,
};
use crate::cloud::definition::{AmazonCollection, Provider};
use std::future::Future;
use std::pin::Pin;

macro_rules! collectors {
    ($($name:literal => $run:expr),+ $(,)?) => {
        vec![$(($name, Box::pin($run) as _)),+]
    };
}

type NamedCollector<'a> = (
    &'static str,
    Pin<Box<dyn Future<Output = Result<AmazonCollection, Box<dyn std::error::Error>>> + 'a>>,
);

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

            let collectors: Vec<NamedCollector<'_>> = collectors![
                "ecs" => container_service::collector::runner(&config),
                "eventbridge" => eventbridge::collector::runner(&config),
                "ec2" => instance::collector::runner(&config),
                "lambda" => lambda::collector::runner(&config),
                "elbv2" => load_balancer::collector::runner(&config),
                "config" => resource::collector::runner(verbose, &config),
                "route53" => route53::collector::runner(&config),
                "eks" => eks::collector::runner(&config),
                "apigateway" => api_gateway::collector::runner(&config),
                "rds" => rds::collector::runner(&config),
                "dynamodb" => dynamodb::collector::runner(&config),
                "sqs" => sqs::collector::runner(&config),
                "sns" => sns::collector::runner(&config),
                "cloudfront" => cloudfront::collector::runner(&config),
                "security_groups" => security_group::collector::runner(&config),
                "networking" => networking::collector::runner(&config),
            ];

            let (names, runs): (Vec<_>, Vec<_>) = collectors.into_iter().unzip();
            let results = futures::future::join_all(runs).await;

            let mut local_services = Vec::new();
            let mut local_failures = Vec::new();

            for (collector, result) in names.into_iter().zip(results) {
                match result {
                    Ok(collection) => local_services.push((r.to_owned(), collection)),
                    Err(e) => local_failures.push(CollectionFailure {
                        source: CollectionSource::Aws,
                        scope: format!("{r}/{collector}"),
                        message: format!("{e:?}"),
                    }),
                }
            }

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
