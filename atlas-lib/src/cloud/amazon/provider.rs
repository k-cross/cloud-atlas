use crate::Settings;
use crate::atlas::collection::{CollectionReport, CollectionSource, FailureKind, ProviderScan};
use crate::cloud::amazon::{
    api_gateway, cloudfront, connectivity, container_service, direct_connect, dynamodb, eks,
    eventbridge, instance, lambda, load_balancer, network_interface, networking, rds, resource,
    route53, security_group, sns, sqs,
};
use crate::cloud::collector::{NamedCollector, run_all};
use crate::cloud::definition::{AmazonCollection, Provider};

const SOURCE: CollectionSource = CollectionSource::Aws;
const GLOBAL: &str = "global";

macro_rules! collectors {
    ($($name:literal => $variant:path, $run:expr),+ $(,)?) => {
        vec![$(($name, Box::pin(async { $run.await.map($variant) }) as _)),+]
    };
}

pub async fn build_aws(verbose: bool, opts: &Settings) -> ProviderScan {
    let regional = opts.regions.iter().map(|r| async move {
        let config = match authorised_config(r, r).await {
            Ok(config) => config,
            Err(report) => return (Vec::new(), report),
        };
        collect_scope(
            r,
            collectors![
                "ecs" => AmazonCollection::AmazonClusters, container_service::collector::runner(&config),
                "eventbridge" => AmazonCollection::AmazonEventbridge, eventbridge::collector::runner(&config),
                "ec2" => AmazonCollection::AmazonInstances, instance::collector::runner(&config),
                "lambda" => AmazonCollection::AmazonLambdas, lambda::collector::runner(&config),
                "elbv2" => AmazonCollection::AmazonLoadBalancers, load_balancer::collector::runner(&config),
                "config" => AmazonCollection::AmazonResources, resource::collector::runner(verbose, opts.exclude_by_default, &config),
                "eks" => AmazonCollection::AmazonEks, eks::collector::runner(&config),
                "apigateway" => AmazonCollection::AmazonApiGateway, api_gateway::collector::runner(&config),
                "rds" => AmazonCollection::AmazonRds, rds::collector::runner(&config),
                "dynamodb" => AmazonCollection::AmazonDynamoDb, dynamodb::collector::runner(&config),
                "sqs" => AmazonCollection::AmazonSqs, sqs::collector::runner(&config),
                "sns" => AmazonCollection::AmazonSns, sns::collector::runner(&config),
                "security_groups" => AmazonCollection::AmazonSecurityGroups, security_group::collector::runner(&config),
                "networking" => AmazonCollection::AmazonNetworking, networking::collector::runner(&config),
                "network_interfaces" => AmazonCollection::AmazonNetworkInterfaces, network_interface::collector::runner(&config),
                "connectivity" => AmazonCollection::AmazonConnectivity, connectivity::collector::runner(&config),
                "transit_gateway_routing" => AmazonCollection::AmazonTransitGatewayRouting, connectivity::collector::transit_gateway_routing(&config),
                "direct_connect" => AmazonCollection::AmazonDirectConnect, direct_connect::collector::runner(&config),
            ],
        )
        .await
    });

    let account_wide = async {
        let Some(r) = opts.regions.first() else {
            return (Vec::new(), CollectionReport::default());
        };
        let config = match authorised_config(r, GLOBAL).await {
            Ok(config) => config,
            Err(report) => return (Vec::new(), report),
        };
        collect_scope(
            GLOBAL,
            collectors![
                "route53" => AmazonCollection::AmazonRoute53, route53::collector::runner(&config),
                "cloudfront" => AmazonCollection::AmazonCloudFront, cloudfront::collector::runner(&config),
                "direct_connect_gateways" => AmazonCollection::AmazonDirectConnectGateways, direct_connect::collector::gateways(&config),
            ],
        )
        .await
    };

    let (regional, account_wide) =
        futures::join!(futures::future::join_all(regional), account_wide);

    let mut services = Vec::new();
    let mut report = CollectionReport::default();
    for (mut scope_services, scope_report) in regional.into_iter().chain([account_wide]) {
        services.append(&mut scope_services);
        report.merge(scope_report);
    }

    ProviderScan {
        provider: Provider::AWS(services),
        report,
    }
}

async fn authorised_config(
    region: &str,
    scope: &str,
) -> Result<aws_config::SdkConfig, CollectionReport> {
    let config = super::load_config(region).await;
    match super::resolve_credentials(&config).await {
        Ok(()) => Ok(config),
        Err(e) => {
            let mut report = CollectionReport::default();
            report.record(
                SOURCE,
                FailureKind::Unauthorized,
                format!("{scope}/credentials"),
                e,
            );
            Err(report)
        }
    }
}

async fn collect_scope(
    scope: &str,
    collectors: Vec<NamedCollector<'_, AmazonCollection>>,
) -> (Vec<(String, AmazonCollection)>, CollectionReport) {
    let (collections, report) = run_all(collectors, SOURCE, scope).await;
    let services = collections
        .into_iter()
        .map(|collection| (scope.to_owned(), collection))
        .collect();
    (services, report)
}
