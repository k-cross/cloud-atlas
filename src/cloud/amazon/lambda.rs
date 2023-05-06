pub mod collector {
    use aws_config::meta::region::RegionProviderChain;
    use aws_sdk_lambda::types::FunctionConfiguration;
    use aws_sdk_lambda::{config::Region, Client, Error};
    use crate::cloud::definition::AmazonCollection;

    async fn get_lambdas(client: &Client) -> Result<Vec<FunctionConfiguration>, Error> {
        let resp = client
            .list_functions()
            .send()
            .await?;

        let fs = if let Some(funcs) = resp.functions() {
            funcs.to_owned()
        } else { Vec::new() };

        Ok(fs)
    }

    pub async fn runner(region: &str) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let region_provider = RegionProviderChain::first_try(Region::new(region.to_owned()))
            .or_default_provider()
            .or_else(Region::new("us-west-2"));
        let shared_config = aws_config::from_env().region(region_provider).load().await;
        let client = Client::new(&shared_config);

        match get_lambdas(&client).await {
            Ok(res) => Ok(AmazonCollection::AmazonLambdas(res)),
            Err(e) => Err(e.into()),
        }
    }
}
