use crate::Settings;
use crate::atlas::collection::{CollectionReport, CollectionSource, ProviderScan};
use crate::cloud::amazon::{
    api_gateway, cloudfront, container_service, dynamodb, eks, eventbridge, instance, lambda,
    load_balancer, networking, rds, resource, route53, security_group, sns, sqs,
};
use crate::cloud::collector::{NamedCollector, run_all};
use crate::cloud::definition::{AmazonCollection, Provider};

const SOURCE: CollectionSource = CollectionSource::Aws;

macro_rules! collectors {
    ($($name:literal => $run:expr),+ $(,)?) => {
        vec![$(($name, Box::pin($run) as _)),+]
    };
}

pub async fn build_aws(verbose: bool, opts: &Settings) -> ProviderScan {
    let mut services = Vec::new();
    let mut report = CollectionReport::default();

    let mut futures = Vec::new();

    for r in opts.regions.clone() {
        futures.push(async move {
            let config = super::load_config(&r).await;

            let collectors: Vec<NamedCollector<'_, AmazonCollection>> = collectors![
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

            let (collections, local_report) = run_all(collectors, SOURCE, &r).await;
            let local_services: Vec<_> = collections
                .into_iter()
                .map(|collection| (r.to_owned(), collection))
                .collect();

            (local_services, local_report)
        });
    }

    for (mut region_services, region_report) in futures::future::join_all(futures).await {
        services.append(&mut region_services);
        report.merge(region_report);
    }

    ProviderScan {
        provider: Provider::AWS(services),
        report,
    }
}
