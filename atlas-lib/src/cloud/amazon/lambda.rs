pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_lambda::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);
        let resp = client.list_functions().send().await?;
        Ok(AmazonCollection::AmazonLambdas(resp.functions().to_owned()))
    }
}
