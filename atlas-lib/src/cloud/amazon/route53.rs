pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_route53::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut hosted_zones = Vec::new();
        let mut record_sets = Vec::new();

        let hz_resp = client.list_hosted_zones().send().await?;
        hosted_zones.extend(hz_resp.hosted_zones().to_owned());

        for zone in &hosted_zones {
            // Route53 zone IDs come with a /hostedzone/ prefix which we can just pass along
            let id = zone.id();
            let mut is_truncated = true;
            let mut next_record_name: Option<String> = None;
            let mut next_record_type: Option<aws_sdk_route53::types::RrType> = None;

            while is_truncated {
                let mut req = client.list_resource_record_sets().hosted_zone_id(id);
                if let Some(n) = &next_record_name {
                    req = req.start_record_name(n);
                }
                if let Some(t) = &next_record_type {
                    req = req.start_record_type(t.clone());
                }

                let rr_resp = req.send().await?;
                record_sets.extend(rr_resp.resource_record_sets().to_owned());

                is_truncated = rr_resp.is_truncated();
                next_record_name = rr_resp.next_record_name().map(|s| s.to_owned());
                next_record_type = rr_resp.next_record_type().cloned();
            }
        }

        Ok(AmazonCollection::AmazonRoute53 {
            hosted_zones,
            record_sets,
        })
    }
}
