use cloudflare::endpoints::dns::dns::{DnsRecord, ListDnsRecords, ListDnsRecordsParams};

const DNS_RECORDS_PER_PAGE: u32 = 500;

pub async fn get_dns_records(
    client: &cloudflare::framework::client::async_api::Client,
    zone_id: &str,
) -> Result<Vec<DnsRecord>, Box<dyn std::error::Error>> {
    super::paginate(DNS_RECORDS_PER_PAGE, |page| async move {
        client
            .request(&ListDnsRecords {
                zone_identifier: zone_id,
                params: ListDnsRecordsParams {
                    page: Some(page),
                    per_page: Some(DNS_RECORDS_PER_PAGE),
                    ..Default::default()
                },
            })
            .await
    })
    .await
}
