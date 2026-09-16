use crate::Settings;
use crate::atlas::collection::{CollectionReport, CollectionSource, FailureKind, ProviderScan};
use crate::cloud::amazon::{
    api_gateway, cloudfront, container_service, dynamodb, eks, eventbridge, instance, lambda,
    load_balancer, network_interface, networking, rds, resource, route53, security_group, sns, sqs,
};
use crate::cloud::collector::{NamedCollector, run_all};
use crate::cloud::definition::{AmazonCollection, Provider};

const SOURCE: CollectionSource = CollectionSource::Aws;

macro_rules! collectors {
    ($($name:literal => $variant:path, $run:expr),+ $(,)?) => {
        vec![$(($name, Box::pin(async { $run.await.map($variant) }) as _)),+]
    };
}

pub async fn build_aws(verbose: bool, opts: &Settings) -> ProviderScan {
    let mut services = Vec::new();
    let mut report = CollectionReport::default();

    let mut futures = Vec::new();

    for r in &opts.regions {
        futures.push(async move {
            let config = super::load_config(r).await;

            if let Err(e) = super::resolve_credentials(&config).await {
                let mut report = CollectionReport::default();
                report.record(
                    SOURCE,
                    FailureKind::Unauthorized,
                    format!("{r}/credentials"),
                    e,
                );
                return (Vec::new(), report);
            }

            let collectors: Vec<NamedCollector<'_, AmazonCollection>> = collectors![
                "ecs" => AmazonCollection::AmazonClusters, container_service::collector::runner(&config),
                "eventbridge" => AmazonCollection::AmazonEventbridge, eventbridge::collector::runner(&config),
                "ec2" => AmazonCollection::AmazonInstances, instance::collector::runner(&config),
                "lambda" => AmazonCollection::AmazonLambdas, lambda::collector::runner(&config),
                "elbv2" => AmazonCollection::AmazonLoadBalancers, load_balancer::collector::runner(&config),
                "config" => AmazonCollection::AmazonResources, resource::collector::runner(verbose, &config),
                "route53" => AmazonCollection::AmazonRoute53, route53::collector::runner(&config),
                "eks" => AmazonCollection::AmazonEks, eks::collector::runner(&config),
                "apigateway" => AmazonCollection::AmazonApiGateway, api_gateway::collector::runner(&config),
                "rds" => AmazonCollection::AmazonRds, rds::collector::runner(&config),
                "dynamodb" => AmazonCollection::AmazonDynamoDb, dynamodb::collector::runner(&config),
                "sqs" => AmazonCollection::AmazonSqs, sqs::collector::runner(&config),
                "sns" => AmazonCollection::AmazonSns, sns::collector::runner(&config),
                "cloudfront" => AmazonCollection::AmazonCloudFront, cloudfront::collector::runner(&config),
                "security_groups" => AmazonCollection::AmazonSecurityGroups, security_group::collector::runner(&config),
                "networking" => AmazonCollection::AmazonNetworking, networking::collector::runner(&config),
                "network_interfaces" => AmazonCollection::AmazonNetworkInterfaces, network_interface::collector::runner(&config),
            ];

            let (collections, local_report) = run_all(collectors, SOURCE, r).await;
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
