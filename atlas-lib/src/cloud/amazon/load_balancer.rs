pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_elasticloadbalancingv2::{Client, config::Region};
    use std::collections::HashMap;

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;
        let client = Client::new(&shared_config);

        // Fetch Load Balancers
        let lbs_resp = client.describe_load_balancers().send().await?;
        let load_balancers = lbs_resp.load_balancers().to_owned();

        // Fetch Listeners for each Load Balancer
        let mut listeners = Vec::new();
        for lb in &load_balancers {
            if let Some(arn) = lb.load_balancer_arn() {
                let listeners_resp = client
                    .describe_listeners()
                    .load_balancer_arn(arn)
                    .send()
                    .await?;
                listeners.extend(listeners_resp.listeners().to_owned());
            }
        }

        // Fetch Target Groups
        let tg_resp = client.describe_target_groups().send().await?;
        let target_groups = tg_resp.target_groups().to_owned();

        // Fetch Target Health for each Target Group
        let mut target_health = HashMap::new();
        for tg in &target_groups {
            if let Some(arn) = tg.target_group_arn() {
                let health_resp = client
                    .describe_target_health()
                    .target_group_arn(arn)
                    .send()
                    .await?;
                target_health.insert(
                    arn.to_owned(),
                    health_resp.target_health_descriptions().to_owned(),
                );
            }
        }

        Ok(AmazonCollection::AmazonLoadBalancers {
            load_balancers,
            target_groups,
            listeners,
            target_health,
        })
    }
}
