use cloudflare::endpoints::zones::zone::{ListZones, ListZonesParams, Zone};

const ZONES_PER_PAGE: u32 = 50;

pub async fn get_zones(
    client: &cloudflare::framework::client::async_api::Client,
) -> Result<Vec<Zone>, Box<dyn std::error::Error>> {
    let mut zones = Vec::new();
    let mut page = 1;

    loop {
        let request = ListZones {
            params: ListZonesParams {
                name: None,
                status: None,
                page: Some(page),
                per_page: Some(ZONES_PER_PAGE),
                order: None,
                direction: None,
                search_match: None,
            },
        };
        let response = client.request(&request).await?;
        let received = response.result.len() as u32;
        zones.extend(response.result);

        if received < ZONES_PER_PAGE {
            break;
        }
        page += 1;
    }

    Ok(zones)
}
