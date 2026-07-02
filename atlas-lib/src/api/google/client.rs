use reqwest::{Client, RequestBuilder};

#[derive(Clone)]
pub struct GoogleApiClient {
    pub client: Client,
    pub token: String,
}

impl GoogleApiClient {
    pub fn new(token: String) -> Self {
        Self {
            client: Client::new(),
            token,
        }
    }

    pub fn get(&self, url: &str) -> RequestBuilder {
        self.client.get(url).bearer_auth(&self.token)
    }
}
