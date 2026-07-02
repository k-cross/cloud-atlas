pub mod collector {
    use crate::api::google::client::GoogleApiClient;
    use crate::api::google::sql;
    use crate::cloud::definition::GoogleCollection;

    pub async fn runner(
        project: &str,
        client: &GoogleApiClient,
    ) -> Result<GoogleCollection, Box<dyn std::error::Error>> {
        let instances = sql::list_instances(client, project).await?;
        Ok(GoogleCollection::GoogleSql(instances))
    }
}
