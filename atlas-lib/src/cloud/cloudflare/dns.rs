use cloudflare::endpoints::dns::dns::{DnsRecord, ListDnsRecords, ListDnsRecordsParams};

pub async fn get_dns_records(
    client: &cloudflare::framework::client::async_api::Client,
    zone_id: &str,
) -> Result<Vec<DnsRecord>, Box<dyn std::error::Error>> {
    let request = ListDnsRecords {
        zone_identifier: zone_id,
        params: ListDnsRecordsParams {
            record_type: None,
            name: None,
            page: Some(1),
            per_page: Some(500),
            order: None,
            direction: None,
            search_match: None,
        },
    };
    let response = client.request(&request).await?;
    Ok(response.result)
}
