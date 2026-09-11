//! AWS Tier-2 ingestion: VPC Flow Logs → normalized [`FlowObservation`].
//!
//! VPC Flow Logs are ENI-level records of what actually crossed the network.
//! They are the only feed that can say a resource is *doing* something, and
//! they are hopeless at saying what exists: a record carries an IP, an
//! interface id and a byte count, never a resource's type, tags, subnet or
//! security groups. So this adapter feeds the overlay in [`crate::atlas::flow`]
//! and nothing else — the rules it has to respect are documented there.
//!
//! **Transport.** VPC Flow Logs deliver natively to S3, and S3 notifies an SQS
//! queue natively, so the whole path is operator configuration with no code in
//! between: flow logs → S3 bucket → event notification → SQS →
//! [`stream::FlowLogQueue`]. Each message names an object, which this fetches,
//! gunzips and parses. The alternative (CloudWatch Logs → subscription filter →
//! Kinesis) needs a consumer per shard and a Lambda in the middle for no extra
//! signal.
//!
//! **Record layout is read, not assumed.** Flow-log format is chosen field by
//! field per flow log, and S3 plain-text delivery writes the chosen field names
//! as the first line of every object. So the header decides the layout when it
//! is present and the version-2 default order applies when it is not — pinning
//! the default order and hoping is how a custom format silently reads bytes as
//! ports.

use crate::atlas::collection::CollectionSource;
use crate::atlas::definition::Node;
use crate::atlas::flow::{FlowAction, FlowObservation};

const SOURCE: CollectionSource = CollectionSource::Aws;

/// The version-2 default field order, used when an object carries no header.
/// CloudWatch-delivered records never have one, and neither do objects written
/// before a format was chosen explicitly.
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

/// Which column holds each field this adapter reads. Built once per object from
/// its header (or from [`DEFAULT_FIELDS`]), so parsing a record is a handful of
/// indexed lookups rather than a match per column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    src: Option<usize>,
    dst: Option<usize>,
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
    /// A header line looks exactly like a record except that its columns are
    /// field *names*, so the test is that every token *is* one: flow-log field
    /// names are lowercase words with hyphens, and nothing else in a record can
    /// be mistaken for one. Addresses carry dots, `ACCEPT`/`REJECT`/`OK` are
    /// uppercase, the placeholder is a bare `-`, and ids and counts start with
    /// a digit.
    ///
    /// Testing only the first token would misfire: a custom header-less format
    /// beginning with `account-id` puts a twelve-digit number there, which is
    /// not a `version` and not a field name either — the whole object would be
    /// read as a header, match no fields, and be discarded as a single
    /// unreadable record.
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
                "packets" => &mut layout.packets,
                "bytes" => &mut layout.bytes,
                "end" => &mut layout.end,
                "action" => &mut layout.action,
                "log-status" => &mut layout.log_status,
                "instance-id" => &mut layout.instance,
                "interface-id" => &mut layout.interface,
                // Every other field — ports, protocol, TCP flags, the account
                // id — is real and simply not something the graph has a place
                // for.
                _ => continue,
            };
            *slot = Some(column);
        }
        layout
    }

    /// Whether the layout carries enough to place a flow in the graph at all.
    /// Without both addresses there are no endpoints, and without an end time
    /// there is no freshness — which is the entire point of the tier.
    pub fn is_usable(&self) -> bool {
        self.src.is_some() && self.dst.is_some() && self.end.is_some()
    }
}

fn is_field_name(token: &str) -> bool {
    let mut chars = token.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Why a line produced no observation. Kept apart from a parse *failure*: a
/// skipped line is a normal, expected thing for the feed to contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Skip {
    /// `NODATA`/`SKIPDATA` — the interface had no traffic in the window, or AWS
    /// dropped records. Says nothing either way, so it must not be read as
    /// liveness.
    NoData,
    /// A field that should have been there was missing or unparseable.
    Unusable,
}

/// What one object's worth of flow-log text yielded.
///
/// The three failure counts are kept apart because they mean different things
/// to an operator: a scattering of drifted lines, a whole object whose format
/// the graph cannot use, and records deliberately left unread because the
/// object was bigger than the overlay will hold.
pub struct Parsed {
    pub observations: Vec<FlowObservation>,
    /// Individual records that could not be read.
    pub unusable: usize,
    /// Records past `limit`. Counted but never materialized — the whole point
    /// of the cap is not to allocate them.
    pub dropped: usize,
    /// The object's field layout could not place a flow at all, so nothing in
    /// it was read. Distinct from `unusable`: one cause, total loss.
    pub unusable_layout: bool,
}

/// Parse one object's worth of flow-log text, materializing at most `limit`
/// observations.
///
/// The limit is a parameter rather than a constant because it is the caller's
/// memory that is at stake: a busy VPC writes millions of records per window,
/// each of which would otherwise become a `FlowObservation` before anything
/// trimmed the list. The overlay is bounded anyway, so records past the cap
/// would be evicted the moment they landed.
///
/// A drifted line is *not* an error here: flow logs are sampled and lossy by
/// nature, so the honest outcome is to lose that line and say so, exactly as
/// [`FailureKind::Malformed`](crate::atlas::collection::FailureKind::Malformed)
/// means elsewhere.
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
            // Counted, not parsed: knowing how much was left unread is worth a
            // pass over the remaining lines, allocating for them is not.
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

    // A record AWS could not fill in is an absence of evidence, not evidence of
    // silence — treating it as a flow would invent traffic, and counting it as
    // a parse failure would report a problem that is not one.
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

    let action = match layout.action.and_then(|at| value(&columns, at)) {
        Some("ACCEPT") => Some(FlowAction::Accepted),
        Some("REJECT") => Some(FlowAction::Rejected),
        // Either the format left the verdict out, or it carried one we do not
        // recognise. Both mean the same thing: traffic happened and we cannot
        // say whether it got through.
        _ => None,
    };

    // Both identities are read straight off the record, never derived from one
    // another: `interface-id` is in the version-2 default set and is the
    // *subject* of every flow record, while `instance-id` needs a version-3
    // format and is absent for every interface that belongs to a NAT gateway,
    // load balancer, RDS instance or in-VPC Lambda rather than an instance.
    //
    // Neither buys a node. `FlowIndex` admits a typed resource only if a scan
    // already found it, so an interface the graph does not know about is simply
    // not attributed.
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
        src: Node::GenericIpAddress(src.into()),
        dst: Node::GenericIpAddress(dst.into()),
        resources,
        packets: number(layout.packets),
        bytes: number(layout.bytes),
        action,
        observed_at: end.saturating_mul(1_000),
    })
}

/// A column's value, treating flow logs' `-` placeholder as absent.
fn value<'a>(columns: &[&'a str], at: usize) -> Option<&'a str> {
    match columns.get(at) {
        Some(&"-") | None => None,
        Some(&value) => Some(value),
    }
}

/// The S3-plus-SQS consumer that carries flow-log objects off the bucket.
pub mod stream {
    use super::{SOURCE, parse};
    use crate::atlas::collection::{CollectionReport, FailureKind};
    use crate::atlas::flow::FlowObservation;
    use aws_sdk_s3::Client as S3Client;
    use aws_sdk_sqs::Client as SqsClient;
    use aws_sdk_sqs::types::DeleteMessageBatchRequestEntry;
    use serde::Deserialize;
    use std::io::Read;

    /// One drain of the queue. Infallible like every other feed in `cloud/`: a
    /// queue or bucket that could not be read still returns a batch, with the
    /// empty observation list explained by a non-empty report, so nothing can
    /// mistake a broken flow feed for a quiet network.
    pub struct FlowBatch {
        pub observations: Vec<FlowObservation>,
        pub report: CollectionReport,
    }

    /// The S3 event notification shape. `Records` is absent on the test message
    /// S3 posts when a notification is first configured, which is why every
    /// field here is optional.
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

    /// An SNS notification wrapping an S3 event, for the fan-out shape where
    /// the bucket notifies a topic with the queue subscribed behind it.
    #[derive(Deserialize)]
    struct SnsEnvelope {
        #[serde(rename = "Type")]
        kind: Option<String>,
        #[serde(rename = "Message")]
        message: Option<String>,
    }

    /// An SQS queue subscribed to a flow-log bucket's event notifications.
    ///
    /// Objects are fetched and parsed, then the message is deleted — but only
    /// when every object it named was actually *read*. Tier 1 can delete
    /// unconditionally because the only step between receiving and deleting is
    /// a local parse, which cannot fail transiently; Tier 2 puts a fetch from
    /// S3 in that gap, so deleting regardless would turn one 503 into an object
    /// lost for good. A `Malformed` outcome still deletes: the object was read,
    /// and redelivering it will not make it parse. A crash between the two
    /// costs a redelivery, and re-observing a flow is near enough idempotent —
    /// the overlay is keyed by endpoint pair, so only the volume counters
    /// double-count, and they reset when the entry lapses.
    pub struct FlowLogQueue {
        sqs: SqsClient,
        s3: S3Client,
        queue_url: String,
        region: String,
        wait_time_seconds: i32,
    }

    impl FlowLogQueue {
        /// Seconds to hold a receive open. Long-polling keeps latency at "as
        /// soon as the object lands" without spinning.
        pub const WAIT_SECONDS: i32 = 20;

        /// Messages per receive. SQS's maximum.
        const BATCH: i32 = 10;

        /// Records materialized from one object. Flow-log objects are
        /// unbounded — a busy VPC writes millions of records per window — and
        /// the overlay is bounded anyway, so reading a whole one into
        /// observations would spend memory to produce entries that are evicted
        /// the moment they land.
        const MAX_RECORDS: usize = 100_000;

        /// Compressed bytes accepted from one object, enforced by asking S3 for
        /// at most this many.
        const MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024;

        /// Bytes accepted out of the decompressor. The compressed cap bounds
        /// nothing on its own, because the compression ratio belongs to
        /// whoever wrote the object: 64 MB of gzipped flow logs expands past
        /// 600 MB at ordinary ratios and adversarially much further. Set
        /// comfortably above the ~25 MB that [`MAX_RECORDS`](Self::MAX_RECORDS)
        /// lines occupy.
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

        /// Shorten the long-poll. For tests, which must not block for twenty
        /// seconds on an empty replay queue.
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
                    // Classified here, where the error is still typed: a 403 on
                    // the queue policy is a problem no amount of polling fixes.
                    let kind = match error.raw_response().map(|r| r.status().as_u16()) {
                        Some(401 | 403) => FailureKind::Unauthorized,
                        _ => FailureKind::Unavailable,
                    };
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
                if readable
                    && let Some(handle) = message.receipt_handle
                {
                    processed.push(handle);
                }
            }

            self.delete(processed, &mut report).await;

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
                // The bound, not a hint: S3 sends at most this many bytes, so
                // an object of any size costs a fixed ceiling of memory. A
                // check after `collect()` is not a guard — the buffering it was
                // meant to prevent has already happened.
                .range(format!("bytes=0-{}", Self::MAX_OBJECT_BYTES))
                .send()
                .await;

            let object = match object {
                Ok(object) => object,
                Err(error) => {
                    // The object exists (S3 said so) and we could not read it,
                    // so this is a read failure of the flow feed — but never of
                    // the *scan*: the caller keeps this report apart, because a
                    // bucket we cannot reach makes liveness stale, not the
                    // topology wrong.
                    let kind = match error.raw_response().map(|r| r.status().as_u16()) {
                        Some(401 | 403) => FailureKind::Unauthorized,
                        _ => FailureKind::Unavailable,
                    };
                    report.record(
                        SOURCE,
                        kind,
                        scope,
                        format!("s3://{bucket}/{key}: {error:?}"),
                    );
                    return None;
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

            // The range asked for one byte past the cap, so this means the
            // object is larger and what we hold is a fragment. A truncated gzip
            // member will not decompress, and truncated text would silently
            // lose every record after the cut, so refuse it and say why.
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

        async fn delete(&self, handles: Vec<String>, report: &mut CollectionReport) {
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

            let result = self
                .sqs
                .delete_message_batch()
                .queue_url(&self.queue_url)
                .set_entries(Some(entries))
                .send()
                .await;

            // A failed delete costs a redelivery, not an observation, and
            // re-observing a flow changes nothing. Worth reporting, never a
            // read failure.
            match result {
                Ok(response) if !response.failed().is_empty() => report.note(
                    SOURCE,
                    FailureKind::Malformed,
                    self.scope(),
                    format!(
                        "{} processed notification(s) could not be deleted",
                        response.failed().len()
                    ),
                ),
                Ok(_) => {}
                Err(error) => report.record(
                    SOURCE,
                    FailureKind::Malformed,
                    self.scope(),
                    format!("could not delete processed notifications: {error:?}"),
                ),
            }
        }
    }

    /// The objects one queue message points at.
    ///
    /// A message with no `Records` is S3's configuration test message, not a
    /// failure — reporting it would put a permanent, meaningless entry in the
    /// feed's health the moment an operator wires the notification up.
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

    fn unwrap_sns(body: &str) -> std::borrow::Cow<'_, str> {
        match serde_json::from_str::<SnsEnvelope>(body) {
            Ok(SnsEnvelope {
                kind: Some(kind),
                message: Some(message),
            }) if kind == "Notification" => std::borrow::Cow::Owned(message),
            _ => std::borrow::Cow::Borrowed(body),
        }
    }

    /// S3 form-encodes object keys in its notifications, so `+` is a space and
    /// everything else is percent-encoded. Fetching the raw key would 404 on
    /// every object whose prefix contains one — and flow-log keys are built
    /// from account, region and timestamp, so `=` shows up routinely.
    fn decode_key(key: &str) -> String {
        let spaced = key.replace('+', " ");
        percent_encoding::percent_decode_str(&spaced)
            .decode_utf8()
            .map(|decoded| decoded.into_owned())
            .unwrap_or(spaced)
    }

    /// Object text, plus whether the size cap cut it short — a partial read
    /// must not pass for a small object.
    #[derive(Debug)]
    pub struct Decompressed {
        pub text: String,
        pub truncated: bool,
    }

    /// Flow logs land gzipped by default and plain when the operator asked for
    /// it, and the magic bytes are the only reliable way to tell — the key
    /// suffix is whatever the delivery was configured to write.
    ///
    /// `limit` bounds the *output*, which is the only place a bound means
    /// anything: how far a member expands is decided by whoever wrote it.
    pub fn decompress(body: &[u8], limit: u64) -> Result<Decompressed, String> {
        // Parquet's magic, the third delivery format. Naming it beats the
        // "invalid utf-8" a raw decode would produce.
        if body.starts_with(b"PAR1") {
            return Err("Parquet-formatted flow logs are not supported; \
                        deliver them as text or gzip"
                .to_owned());
        }

        if body.starts_with(&[0x1f, 0x8b]) {
            let mut bytes = Vec::new();
            // One byte past the cap, so filling it is proof there was more.
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
