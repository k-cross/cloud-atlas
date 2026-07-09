pub mod collector {
    use crate::cloud::definition::AmazonCollection;
    use aws_sdk_sqs::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<AmazonCollection, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut queues = Vec::new();
        let mut next_token = None;

        loop {
            let mut req = client.list_queues();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }

            let resp = req.send().await?;
            for q in resp.queue_urls() {
                queues.push(q.to_string());
            }

            next_token = resp.next_token().map(|s| s.to_string());
            if next_token.is_none() {
                break;
            }
        }

        Ok(AmazonCollection::AmazonSqs(queues))
    }
}

#[cfg(test)]
mod tests {
    use super::collector::runner;
    use crate::cloud::definition::AmazonCollection;
    use aws_credential_types::Credentials;
    use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
    use aws_smithy_types::body::SdkBody;

    // SQS uses the awsJson1.0 protocol (JSON, not the EC2 XML) — this proves the
    // replay pattern works for JSON-protocol services too.
    #[tokio::test]
    async fn list_queues_maps_queue_urls() {
        let body = r#"{"QueueUrls":["https://sqs.us-east-1.amazonaws.com/111111111111/my-queue"]}"#;
        let http = StaticReplayClient::new(vec![ReplayEvent::new(
            http::Request::builder()
                .uri("https://sqs.us-east-1.amazonaws.com/")
                .body(SdkBody::empty())
                .unwrap(),
            http::Response::builder()
                .status(200)
                .header("content-type", "application/x-amz-json-1.0")
                .body(SdkBody::from(body))
                .unwrap(),
        )]);
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(Credentials::for_tests())
            .http_client(http)
            .load()
            .await;

        let AmazonCollection::AmazonSqs(queues) = runner(&config).await.expect("runner ok") else {
            panic!("expected AmazonSqs");
        };
        assert_eq!(
            queues,
            vec!["https://sqs.us-east-1.amazonaws.com/111111111111/my-queue".to_string()]
        );
    }
}
