pub mod collector {
    use crate::api::google::client::GoogleApiClient;
    use crate::api::google::functions;
    use crate::cloud::definition::GoogleCollection;

    pub async fn runner(
        project: &str,
        client: &GoogleApiClient,
    ) -> Result<GoogleCollection, Box<dyn std::error::Error>> {
        let functions = functions::list_functions(client, project).await?;
        Ok(GoogleCollection::GoogleFunctions(functions))
    }
}
