pub mod collector {
    use crate::api::google::client::GoogleApiClient;
    use crate::api::google::compute;
    use crate::cloud::definition::GoogleCollection;

    pub async fn runner(
        project: &str,
        client: &GoogleApiClient,
    ) -> Result<GoogleCollection, Box<dyn std::error::Error>> {
        let firewalls = compute::list_firewalls(client, project).await?;
        Ok(GoogleCollection::GoogleFirewalls(firewalls))
    }
}
