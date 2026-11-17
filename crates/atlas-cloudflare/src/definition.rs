use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ZoneId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptId {
    pub account: String,
    pub script: String,
}

#[derive(Debug, Default)]
pub struct CloudflareCollection {
    pub zones: Vec<cloudflare::endpoints::zones::zone::Zone>,
    pub dns_records: HashMap<ZoneId, Vec<cloudflare::endpoints::dns::dns::DnsRecord>>,
    pub workers: Vec<ScriptId>,
    pub kv_namespaces: Vec<cloudflare::endpoints::workerskv::WorkersKvNamespace>,
    pub r2_buckets: Vec<cloudflare::endpoints::r2::r2::Bucket>,
    pub durable_objects: Vec<crate::durable_objects::DurableObjectNamespace>,
    pub d1_databases: Vec<crate::d1::D1Database>,
    pub worker_bindings: HashMap<ScriptId, Vec<crate::worker::WorkerBinding>>,
}
