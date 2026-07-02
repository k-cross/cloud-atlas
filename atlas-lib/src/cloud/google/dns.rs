pub mod collector {
    use crate::api::google::client::GoogleApiClient;
    use crate::api::google::dns;
    use crate::cloud::definition::GoogleCollection;

    pub async fn runner(
        project: &str,
        client: &GoogleApiClient,
    ) -> Result<GoogleCollection, Box<dyn std::error::Error>> {
        let zones = dns::list_managed_zones(client, project).await?;
        Ok(GoogleCollection::GoogleDns(zones))
    }
}
