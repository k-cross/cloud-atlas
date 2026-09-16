use crate::atlas::collection::CollectionSource;
use crate::atlas::definition::Node;
use crate::atlas::flow::{FlowAction, FlowObservation};

const SOURCE: CollectionSource = CollectionSource::Aws;

const DEFAULT_FIELDS: &[&str] = &[
    "version",
    "account-id",
    "interface-id",
    "srcaddr",
    "dstaddr",
    "srcport",
    "dstport",
    "protocol",
    "packets",
    "bytes",
    "start",
    "end",
    "action",
    "log-status",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    src: Option<usize>,
    dst: Option<usize>,
    src_port: Option<usize>,
    dst_port: Option<usize>,
    packets: Option<usize>,
    bytes: Option<usize>,
    end: Option<usize>,
    action: Option<usize>,
    log_status: Option<usize>,
    instance: Option<usize>,
    interface: Option<usize>,
    width: usize,
}

impl Layout {
    pub fn looks_like_header(line: &str) -> bool {
        let mut tokens = line.split_whitespace().peekable();
        tokens.peek().is_some() && tokens.all(is_field_name)
    }

    pub fn from_header(line: &str) -> Self {
        Self::from_fields(line.split_whitespace())
    }

    pub fn default_v2() -> Self {
        Self::from_fields(DEFAULT_FIELDS.iter().copied())
    }

    fn from_fields<'a>(fields: impl Iterator<Item = &'a str>) -> Self {
        let mut layout = Self {
            src: None,
            dst: None,
            src_port: None,
            dst_port: None,
            packets: None,
            bytes: None,
            end: None,
            action: None,
            log_status: None,
            instance: None,
            interface: None,
            width: 0,
        };
        for (column, field) in fields.enumerate() {
            layout.width = column + 1;
            let slot = match field {
                "srcaddr" => &mut layout.src,
                "dstaddr" => &mut layout.dst,
                "srcport" => &mut layout.src_port,
                "dstport" => &mut layout.dst_port,
                "packets" => &mut layout.packets,
                "bytes" => &mut layout.bytes,
                "end" => &mut layout.end,
                "action" => &mut layout.action,
                "log-status" => &mut layout.log_status,
                "instance-id" => &mut layout.instance,
                "interface-id" => &mut layout.interface,
                _ => continue,
            };
            *slot = Some(column);
        }
        layout
    }

    pub fn is_usable(&self) -> bool {
        self.src.is_some() && self.dst.is_some() && self.end.is_some()
    }
}

fn is_field_name(token: &str) -> bool {
    let mut chars = token.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Skip {
    NoData,
    Unusable,
}

pub struct Parsed {
    pub observations: Vec<FlowObservation>,
    pub unusable: usize,
    pub dropped: usize,
    pub unusable_layout: bool,
}

pub fn parse(text: &str, scope: &str, limit: usize) -> Parsed {
    let mut lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .peekable();

    let layout = match lines.peek() {
        Some(first) if Layout::looks_like_header(first) => {
            let layout = Layout::from_header(first);
            lines.next();
            layout
        }
        _ => Layout::default_v2(),
    };

    if !layout.is_usable() {
        return Parsed {
            observations: Vec::new(),
            unusable: 0,
            dropped: 0,
            unusable_layout: true,
        };
    }

    let mut observations = Vec::new();
    let mut unusable = 0;
    let mut dropped = 0;
    for line in lines {
        if observations.len() >= limit {
            dropped += 1;
            continue;
        }
        match record(&layout, line, scope) {
            Ok(observation) => observations.push(observation),
            Err(Skip::NoData) => {}
            Err(Skip::Unusable) => unusable += 1,
        }
    }

    Parsed {
        observations,
        unusable,
        dropped,
        unusable_layout: false,
    }
}

fn record(layout: &Layout, line: &str, scope: &str) -> Result<FlowObservation, Skip> {
    let columns: Vec<&str> = line.split_whitespace().collect();
    if columns.len() < layout.width {
        return Err(Skip::Unusable);
    }

    if let Some(status) = layout.log_status.and_then(|at| value(&columns, at))
        && status != "OK"
    {
        return Err(Skip::NoData);
    }

    let src = layout
        .src
        .and_then(|at| value(&columns, at))
        .ok_or(Skip::Unusable)?;
    let dst = layout
        .dst
        .and_then(|at| value(&columns, at))
        .ok_or(Skip::Unusable)?;
    let end = layout
        .end
        .and_then(|at| value(&columns, at))
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or(Skip::Unusable)?;

    let number = |slot: Option<usize>| {
        slot.and_then(|at| value(&columns, at))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or_default()
    };

    let port = |slot: Option<usize>| {
        slot.and_then(|at| value(&columns, at))
            .and_then(|s| s.parse::<u16>().ok())
    };

    let action = match layout.action.and_then(|at| value(&columns, at)) {
        Some("ACCEPT") => Some(FlowAction::Accepted),
        Some("REJECT") => Some(FlowAction::Rejected),
        _ => None,
    };

    let mut resources = Vec::new();
    if let Some(id) = layout.instance.and_then(|at| value(&columns, at)) {
        resources.push(Node::AwsEc2Instance(id.into()));
    }
    if let Some(id) = layout.interface.and_then(|at| value(&columns, at)) {
        resources.push(Node::AwsEc2Eni(id.into()));
    }

    Ok(FlowObservation {
        source: SOURCE,
        scope: scope.to_owned(),
        src: Node::ip(src),
        dst: Node::ip(dst),
        src_port: port(layout.src_port),
        dst_port: port(layout.dst_port),
        resources,
        packets: number(layout.packets),
        bytes: number(layout.bytes),
        action,
        observed_at: end.saturating_mul(1_000),
    })
}

fn value<'a>(columns: &[&'a str], at: usize) -> Option<&'a str> {
    match columns.get(at) {
        Some(&"-") | None => None,
        Some(&value) => Some(value),
    }
}

pub mod stream {
    use super::{SOURCE, parse};
    use crate::atlas::collection::{CollectionReport, FailureKind};
    use crate::atlas::flow::FlowObservation;
    use crate::cloud::amazon::sqs::feed::{self, unwrap_sns};
    use aws_sdk_s3::Client as S3Client;
    use aws_sdk_sqs::Client as SqsClient;
    use serde::Deserialize;
    use std::io::Read;

    pub struct FlowBatch {
        pub observations: Vec<FlowObservation>,
        pub report: CollectionReport,
    }

    #[derive(Deserialize)]
    struct S3Notification {
        #[serde(rename = "Records")]
        records: Option<Vec<S3Record>>,
    }

    #[derive(Deserialize)]
    struct S3Record {
        s3: Option<S3Entity>,
    }

    #[derive(Deserialize)]
    struct S3Entity {
        bucket: Option<NamedBucket>,
        object: Option<KeyedObject>,
    }

    #[derive(Deserialize)]
    struct NamedBucket {
        name: Option<String>,
    }

    #[derive(Deserialize)]
    struct KeyedObject {
        key: Option<String>,
    }

    pub struct FlowLogQueue {
        sqs: SqsClient,
        s3: S3Client,
        queue_url: String,
        region: String,
        wait_time_seconds: i32,
    }

    impl FlowLogQueue {
        pub const WAIT_SECONDS: i32 = 20;

        const BATCH: i32 = 10;

        const MAX_RECORDS: usize = 100_000;

        const MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024;

        const MAX_TEXT_BYTES: u64 = 128 * 1024 * 1024;

        pub fn new(
            config: &aws_config::SdkConfig,
            queue_url: impl Into<String>,
            region: impl Into<String>,
        ) -> Self {
            Self {
                sqs: SqsClient::new(config),
                s3: S3Client::new(config),
                queue_url: queue_url.into(),
                region: region.into(),
                wait_time_seconds: Self::WAIT_SECONDS,
            }
        }

        pub fn with_wait_seconds(mut self, seconds: i32) -> Self {
            self.wait_time_seconds = seconds;
            self
        }

        fn scope(&self) -> String {
            format!("{}/flow-logs", self.region)
        }

        pub async fn receive(&self) -> FlowBatch {
            let mut report = CollectionReport::default();

            let response = self
                .sqs
                .receive_message()
                .queue_url(&self.queue_url)
                .max_number_of_messages(Self::BATCH)
                .wait_time_seconds(self.wait_time_seconds)
                .send()
                .await;

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    let kind =
                        feed::failure_kind(error.raw_response().map(|r| r.status().as_u16()));
                    report.record(SOURCE, kind, self.scope(), error);
                    return FlowBatch {
                        observations: Vec::new(),
                        report,
                    };
                }
            };

            let mut observations = Vec::new();
            let mut processed = Vec::new();

            for message in response.messages.unwrap_or_default() {
                let body = message.body.as_deref().unwrap_or_default();
                let mut readable = true;
                for (bucket, key) in objects(body, &mut report, &self.scope()) {
                    match self.read_object(&bucket, &key, &mut report).await {
                        Some(mut fetched) => observations.append(&mut fetched),
                        None => readable = false,
                    }
                }
                if readable && let Some(handle) = message.receipt_handle {
                    processed.push(handle);
                }
            }

            feed::delete(
                &self.sqs,
                &self.queue_url,
                processed,
                &mut report,
                &self.scope(),
                "notification",
            )
            .await;

            FlowBatch {
                observations,
                report,
            }
        }

        async fn read_object(
            &self,
            bucket: &str,
            key: &str,
            report: &mut CollectionReport,
        ) -> Option<Vec<FlowObservation>> {
            let scope = self.scope();
            let object = self
                .s3
                .get_object()
                .bucket(bucket)
                .key(key)
                .range(format!("bytes=0-{}", Self::MAX_OBJECT_BYTES))
                .send()
                .await;

            let object = match object {
                Ok(object) => object,
                Err(error) => {
                    let kind = match error.raw_response().map(|r| r.status().as_u16()) {
                        Some(404 | 410) => FailureKind::Malformed,
                        status => feed::failure_kind(status),
                    };
                    report.note(
                        SOURCE,
                        kind,
                        scope,
                        format!("s3://{bucket}/{key}: {error:?}"),
                    );
                    return match kind {
                        FailureKind::Malformed => Some(Vec::new()),
                        _ => None,
                    };
                }
            };

            let body = match object.body.collect().await {
                Ok(body) => body.into_bytes(),
                Err(error) => {
                    report.record(
                        SOURCE,
                        FailureKind::Unavailable,
                        scope,
                        format!("s3://{bucket}/{key}: {error:?}"),
                    );
                    return None;
                }
            };

            if body.len() as u64 > Self::MAX_OBJECT_BYTES {
                report.note(
                    SOURCE,
                    FailureKind::Malformed,
                    scope,
                    format!(
                        "s3://{bucket}/{key} is larger than the {} byte limit; skipped",
                        Self::MAX_OBJECT_BYTES
                    ),
                );
                return Some(Vec::new());
            }

            let text = match decompress(&body, Self::MAX_TEXT_BYTES) {
                Ok(decompressed) => {
                    if decompressed.truncated {
                        report.note(
                            SOURCE,
                            FailureKind::Malformed,
                            &scope,
                            format!(
                                "s3://{bucket}/{key} expanded past the {} byte limit; \
                                 read the first part only",
                                Self::MAX_TEXT_BYTES
                            ),
                        );
                    }
                    decompressed.text
                }
                Err(reason) => {
                    report.note(
                        SOURCE,
                        FailureKind::Malformed,
                        scope,
                        format!("s3://{bucket}/{key}: {reason}"),
                    );
                    return Some(Vec::new());
                }
            };

            let parsed = parse(&text, &self.region, Self::MAX_RECORDS);
            if parsed.unusable_layout {
                report.note(
                    SOURCE,
                    FailureKind::Malformed,
                    &scope,
                    format!(
                        "s3://{bucket}/{key}: no usable field layout (needs both addresses \
                         and an end time); whole object skipped"
                    ),
                );
            }
            if parsed.unusable > 0 {
                report.note(
                    SOURCE,
                    FailureKind::Malformed,
                    &scope,
                    format!(
                        "s3://{bucket}/{key}: dropped {} unreadable record(s)",
                        parsed.unusable
                    ),
                );
            }
            if parsed.dropped > 0 {
                report.note(
                    SOURCE,
                    FailureKind::Malformed,
                    &scope,
                    format!(
                        "s3://{bucket}/{key}: kept {} records and left {} unread",
                        Self::MAX_RECORDS,
                        parsed.dropped
                    ),
                );
            }
            Some(parsed.observations)
        }
    }

    pub fn objects(
        body: &str,
        report: &mut CollectionReport,
        scope: &str,
    ) -> Vec<(String, String)> {
        let body = unwrap_sns(body);
        let notification: S3Notification = match serde_json::from_str(&body) {
            Ok(notification) => notification,
            Err(error) => {
                report.note(
                    SOURCE,
                    FailureKind::Malformed,
                    scope,
                    format!("not an S3 event notification: {error}"),
                );
                return Vec::new();
            }
        };

        notification
            .records
            .unwrap_or_default()
            .into_iter()
            .filter_map(|record| {
                let s3 = record.s3?;
                let bucket = s3.bucket?.name?;
                let key = s3.object?.key?;
                Some((bucket, decode_key(&key)))
            })
            .collect()
    }

    fn decode_key(key: &str) -> String {
        let spaced = key.replace('+', " ");
        percent_encoding::percent_decode_str(&spaced)
            .decode_utf8()
            .map(|decoded| decoded.into_owned())
            .unwrap_or(spaced)
    }

    #[derive(Debug)]
    pub struct Decompressed {
        pub text: String,
        pub truncated: bool,
    }

    pub fn decompress(body: &[u8], limit: u64) -> Result<Decompressed, String> {
        if body.starts_with(b"PAR1") {
            return Err("Parquet-formatted flow logs are not supported; \
                        deliver them as text or gzip"
                .to_owned());
        }

        if body.starts_with(&[0x1f, 0x8b]) {
            let mut bytes = Vec::new();

            let read = flate2::read::GzDecoder::new(body)
                .take(limit.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|e| format!("could not decompress: {e}"))?;
            let truncated = read as u64 > limit;
            if truncated {
                bytes.truncate(floor_char_boundary(&bytes, limit as usize));
            }
            let text = String::from_utf8(bytes).map_err(|e| format!("not text: {e}"))?;
            return Ok(Decompressed { text, truncated });
        }

        String::from_utf8(body.to_vec())
            .map(|text| Decompressed {
                text,
                truncated: false,
            })
            .map_err(|e| format!("not text: {e}"))
    }

    fn floor_char_boundary(bytes: &[u8], index: usize) -> usize {
        let mut cut = index.min(bytes.len());
        while cut > 0 && cut < bytes.len() && bytes[cut] & 0b1100_0000 == 0b1000_0000 {
            cut -= 1;
        }
        cut
    }
}

#[cfg(test)]
mod tests;
