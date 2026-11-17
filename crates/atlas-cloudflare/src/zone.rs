use cloudflare::endpoints::zones::zone::{ListZones, ListZonesParams, Zone};

const ZONES_PER_PAGE: u32 = 50;

pub async fn get_zones(
    client: &cloudflare::framework::client::async_api::Client,
) -> Result<Vec<Zone>, Box<dyn std::error::Error>> {
    super::paginate(ZONES_PER_PAGE, |page| async move {
        client
            .request(&ListZones {
                params: ListZonesParams {
                    page: Some(page),
                    per_page: Some(ZONES_PER_PAGE),
                    ..Default::default()
                },
            })
            .await
    })
    .await
}
