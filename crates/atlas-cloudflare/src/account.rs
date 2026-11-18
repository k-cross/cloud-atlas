use serde::Deserialize;

const ACCOUNTS_PER_PAGE: u32 = 50;

#[derive(Deserialize, Debug, Clone)]
pub struct Account {
    pub id: String,
    pub name: Option<String>,
}

pub async fn get_accounts(
    client: &super::CloudflareApiClient,
) -> Result<Vec<Account>, Box<dyn std::error::Error>> {
    super::paginate_pages(ACCOUNTS_PER_PAGE, |page| async move {
        client
            .get_paged(
                &format!("/client/v4/accounts?page={page}&per_page={ACCOUNTS_PER_PAGE}"),
                "accounts",
            )
            .await
    })
    .await
}
