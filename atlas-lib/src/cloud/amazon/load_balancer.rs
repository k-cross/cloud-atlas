pub mod collector {
    use crate::cloud::definition::AWSLoadBalancing;
    use aws_sdk_elasticloadbalancingv2::Client;
    use std::collections::HashMap;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AWSLoadBalancing, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        // Fetch Load Balancers
        let lbs_resp = client.describe_load_balancers().send().await?;
        let load_balancers = lbs_resp.load_balancers.unwrap_or_default();

        // Fetch Listeners for each Load Balancer
        let mut listeners = Vec::new();
        for lb in &load_balancers {
            if let Some(arn) = lb.load_balancer_arn() {
                let listeners_resp = client
                    .describe_listeners()
                    .load_balancer_arn(arn)
                    .send()
                    .await?;
                listeners.extend(listeners_resp.listeners.unwrap_or_default());
            }
        }

        // Fetch Target Groups
        let tg_resp = client.describe_target_groups().send().await?;
        let target_groups = tg_resp.target_groups.unwrap_or_default();

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
                    health_resp.target_health_descriptions.unwrap_or_default(),
                );
            }
        }

        Ok(AWSLoadBalancing {
            load_balancers,
            target_groups,
            listeners,
            target_health,
        })
    }
}
