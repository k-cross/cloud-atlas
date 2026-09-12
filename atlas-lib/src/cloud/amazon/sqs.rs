pub mod feed {
    use crate::atlas::collection::{CollectionReport, FailureKind};
    use aws_sdk_sqs::Client;
    use aws_sdk_sqs::types::DeleteMessageBatchRequestEntry;
    use serde::Deserialize;
    use std::borrow::Cow;

    const SOURCE: crate::atlas::collection::CollectionSource =
        crate::atlas::collection::CollectionSource::Aws;

    #[derive(Deserialize)]
    struct SnsEnvelope {
        #[serde(rename = "Type")]
        kind: Option<String>,
        #[serde(rename = "Message")]
        message: Option<String>,
    }

    pub fn unwrap_sns(body: &str) -> Cow<'_, str> {
        match serde_json::from_str::<SnsEnvelope>(body) {
            Ok(SnsEnvelope {
                kind: Some(kind),
                message: Some(message),
            }) if kind == "Notification" => Cow::Owned(message),
            _ => Cow::Borrowed(body),
        }
    }

    pub fn failure_kind(status: Option<u16>) -> FailureKind {
        match status {
            Some(401 | 403) => FailureKind::Unauthorized,
            _ => FailureKind::Unavailable,
        }
    }

    pub async fn delete(
        client: &Client,
        queue_url: &str,
        handles: Vec<String>,
        report: &mut CollectionReport,
        scope: &str,
        noun: &str,
    ) {
        if handles.is_empty() {
            return;
        }

        let entries: Vec<_> = handles
            .into_iter()
            .enumerate()
            .filter_map(|(i, handle)| {
                DeleteMessageBatchRequestEntry::builder()
                    .id(i.to_string())
                    .receipt_handle(handle)
                    .build()
                    .ok()
            })
            .collect();

        let result = client
            .delete_message_batch()
            .queue_url(queue_url)
            .set_entries(Some(entries))
            .send()
            .await;

        match result {
            Ok(response) if !response.failed().is_empty() => report.note(
                SOURCE,
                FailureKind::Malformed,
                scope,
                format!(
                    "{} processed {noun}(s) could not be deleted",
                    response.failed().len()
                ),
            ),
            Ok(_) => {}
            Err(error) => report.note(
                SOURCE,
                FailureKind::Malformed,
                scope,
                format!("could not delete processed {noun}s: {error:?}"),
            ),
        }
    }
}

pub mod collector {
    use crate::cloud::definition::QueueUrl;
    use aws_sdk_sqs::Client;

    pub async fn runner(
        config: &aws_config::SdkConfig,
    ) -> Result<Vec<QueueUrl>, Box<dyn std::error::Error>> {
        let client = Client::new(config);

        let mut queues = Vec::new();
        let mut next_token = None;

        loop {
            let mut req = client.list_queues();
            if let Some(token) = &next_token {
                req = req.next_token(token);
            }

            let mut resp = req.send().await?;
            next_token = resp.next_token.take();
            queues.extend(
                resp.queue_urls
                    .take()
                    .unwrap_or_default()
                    .into_iter()
                    .map(QueueUrl),
            );
            if next_token.is_none() {
                break;
            }
        }

        Ok(queues)
    }
}

#[cfg(test)]
mod tests {
    use super::collector::runner;
    use crate::cloud::definition::QueueUrl;
    use aws_credential_types::Credentials;
    use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
    use aws_smithy_types::body::SdkBody;

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

        let queues = runner(&config).await.expect("runner ok");
        assert_eq!(
            queues,
            vec![QueueUrl(
                "https://sqs.us-east-1.amazonaws.com/111111111111/my-queue".to_owned()
            )]
        );
    }
}
