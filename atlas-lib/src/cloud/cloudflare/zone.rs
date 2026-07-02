use cloudflare::endpoints::zones::zone::{ListZones, ListZonesParams, Zone};

pub async fn get_zones(
    client: &cloudflare::framework::client::async_api::Client,
) -> Result<Vec<Zone>, Box<dyn std::error::Error>> {
    let request = ListZones {
        params: ListZonesParams {
            name: None,
            status: None,
            page: Some(1),
            per_page: Some(50),
            order: None,
            direction: None,
            search_match: None,
        },
    };
    let response = client.request(&request).await?;
    Ok(response.result)
}
