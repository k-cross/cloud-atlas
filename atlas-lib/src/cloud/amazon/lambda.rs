pub mod collector {
    use aws_sdk_lambda::Client;
    use aws_sdk_lambda::types::FunctionConfiguration;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<FunctionConfiguration>, Box<dyn std::error::Error>> {
        let client = Client::new(config);
        let resp = client.list_functions().send().await?;
        Ok(resp.functions.unwrap_or_default())
    }
}
