pub mod collector {
    use crate::cloud::definition::AWSRoute53;
    use aws_sdk_route53::Client;
    use aws_sdk_route53::types::{HostedZone, ResourceRecordSet, RrType};

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AWSRoute53, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let hosted_zones: Vec<HostedZone> = client
            .list_hosted_zones()
            .into_paginator()
            .items()
            .send()
            .try_collect()
            .await?;

        let mut record_sets = Vec::new();
        for zone in &hosted_zones {
            record_sets.extend(records_of(&client, zone.id()).await?);
        }

        Ok(AWSRoute53 {
            hosted_zones,
            record_sets,
        })
    }

    async fn records_of(
        client: &Client,
        zone: &str,
    ) -> Result<Vec<ResourceRecordSet>, Box<dyn std::error::Error>> {
        let mut records = Vec::new();
        let mut start: Option<(String, RrType, Option<String>)> = None;
        loop {
            let mut req = client.list_resource_record_sets().hosted_zone_id(zone);
            if let Some((name, kind, identifier)) = &start {
                req = req
                    .start_record_name(name)
                    .start_record_type(kind.clone())
                    .set_start_record_identifier(identifier.clone());
            }

            let mut resp = req.send().await?;
            records.append(&mut resp.resource_record_sets);
            if !resp.is_truncated() {
                return Ok(records);
            }

            let next = match (resp.next_record_name.take(), resp.next_record_type.take()) {
                (Some(name), Some(kind)) => (name, kind, resp.next_record_identifier.take()),
                _ => {
                    return Err(format!(
                        "Route 53 truncated the records of {zone} without saying where to resume"
                    )
                    .into());
                }
            };
            if start.as_ref() == Some(&next) {
                return Err(format!(
                    "Route 53 repeated the record cursor for {zone}, so paging would never end"
                )
                .into());
            }
            start = Some(next);
        }
    }
}
