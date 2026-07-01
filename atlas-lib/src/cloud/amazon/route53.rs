pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_route53::{Client, config::Region};

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-east-1"));
        let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;
        let client = Client::new(&shared_config);

        let mut hosted_zones = Vec::new();
        let mut record_sets = Vec::new();

        let hz_resp = client.list_hosted_zones().send().await?;
        hosted_zones.extend(hz_resp.hosted_zones().to_owned());

        for zone in &hosted_zones {
            let id = zone.id();
            // Route53 zone IDs come with a /hostedzone/ prefix which we can just pass along
            let mut is_truncated = true;
            let mut next_record_name = None;
            let mut next_record_type = None;

            while is_truncated {
                let mut req = client.list_resource_record_sets().hosted_zone_id(id);
                if let Some(n) = next_record_name.clone() {
                    req = req.start_record_name(n);
                }
                if let Some(t) = next_record_type.clone() {
                    req = req.start_record_type(t);
                }

                let rr_resp = req.send().await?;
                record_sets.extend(rr_resp.resource_record_sets().to_owned());

                is_truncated = rr_resp.is_truncated();
                next_record_name = rr_resp.next_record_name().map(|s| s.to_owned());
                if let Some(rt) = rr_resp.next_record_type() {
                    next_record_type = Some(rt.clone());
                } else {
                    next_record_type = None;
                }
            }
        }

        Ok(AmazonCollection::AmazonRoute53 {
            hosted_zones,
            record_sets,
        })
    }
}
