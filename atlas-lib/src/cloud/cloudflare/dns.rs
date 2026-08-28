use cloudflare::endpoints::dns::dns::{DnsRecord, ListDnsRecords, ListDnsRecordsParams};

const DNS_RECORDS_PER_PAGE: u32 = 500;

pub async fn get_dns_records(
    client: &cloudflare::framework::client::async_api::Client,
    zone_id: &str,
) -> Result<Vec<DnsRecord>, Box<dyn std::error::Error>> {
    let mut records = Vec::new();
    let mut page = 1;

    loop {
        let request = ListDnsRecords {
            zone_identifier: zone_id,
            params: ListDnsRecordsParams {
                record_type: None,
                name: None,
                page: Some(page),
                per_page: Some(DNS_RECORDS_PER_PAGE),
                order: None,
                direction: None,
                search_match: None,
            },
        };
        let response = client.request(&request).await?;
        let received = response.result.len() as u32;
        records.extend(response.result);

        if received < DNS_RECORDS_PER_PAGE {
            break;
        }
        page += 1;
    }

    Ok(records)
}
