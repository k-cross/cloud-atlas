use crate::definition::CloudflareCollection;
use std::collections::HashMap;

fn globex_zone() -> cloudflare::endpoints::zones::zone::Zone {
    use serde_json::json;

    let zone_json = json!({
        "id": "zone-globex",
        "name": "globex.io",
        "account": { "id": "acc-globex", "name": "Globex Org" },
        "activated_on": "2023-01-01T00:00:00Z",
        "created_on": "2023-01-01T00:00:00Z",
        "development_mode": 0,
        "original_name_servers": ["ns1"],
        "original_registrar": "reg",
        "original_dnshost": "host",
        "modified_on": "2023-01-01T00:00:00Z",
        "name_servers": ["ns1"],
        "owner": { "id": "owner1", "type": "user", "email": "admin@globex.io" },
        "permissions": ["#zone:read"],
        "plan": { "id": "plan1", "name": "enterprise", "price": 0, "currency": "USD", "frequency": "monthly", "is_subscribed": true, "can_subscribe": false, "legacy_id": "ent", "legacy_discount": false, "externally_managed": false },
        "plan_pending": { "id": "plan1", "name": "enterprise", "price": 0, "currency": "USD", "frequency": "monthly", "is_subscribed": true, "can_subscribe": false, "legacy_id": "ent", "legacy_discount": false, "externally_managed": false },
        "status": "active",
        "paused": false,
        "type": "full",
        "meta": {
            "custom_certificate_quota": 1,
            "page_rule_quota": 3,
            "phishing_detected": false
        }
    });
    serde_json::from_value(zone_json).expect("fixture zone must deserialize")
}

fn dns_record(
    id: &str,
    name: &str,
    rtype: &str,
    content: &str,
) -> cloudflare::endpoints::dns::dns::DnsRecord {
    use serde_json::json;

    serde_json::from_value(json!({
        "id": id,
        "name": name,
        "type": rtype,
        "content": content,
        "proxied": false,
        "ttl": 300,
        "modified_on": "2023-01-01T00:00:00Z",
        "created_on": "2023-01-01T00:00:00Z",
        "meta": {
            "auto_added": false,
            "managed_by_apps": false,
            "managed_by_argo_tunnel": false,
            "source": "primary"
        },
        "proxiable": true
    }))
    .expect("fixture DNS record must deserialize")
}

pub fn collection() -> CloudflareCollection {
    use crate::d1::D1Database;
    use crate::definition::{ScriptId, ZoneId};
    use crate::durable_objects::DurableObjectNamespace;
    use crate::worker::WorkerBinding;
    use serde_json::json;

    let records = vec![
        dns_record(
            "rec-app",
            "app.globex.io",
            "CNAME",
            "app-globex.azurewebsites.net",
        ),
        dns_record("rec-data", "data.globex.io", "CNAME", "run.globex.app"),
        dns_record("rec-db", "db.globex.io", "A", "198.51.100.10"),
        dns_record("rec-v6", "v6.globex.io", "AAAA", "2001:db8::10"),
    ];

    let worker = ScriptId {
        account: "acc-globex".to_owned(),
        script: "edge-router".to_owned(),
    };

    let kv: cloudflare::endpoints::workerskv::WorkersKvNamespace =
        serde_json::from_value(json!({ "id": "kv-sessions", "title": "sessions" }))
            .expect("fixture KV namespace must deserialize");

    let r2: cloudflare::endpoints::r2::r2::Bucket = serde_json::from_value(
        json!({ "name": "r2-media", "creation_date": "2023-01-01T00:00:00Z" }),
    )
    .expect("fixture R2 bucket must deserialize");

    let durable = DurableObjectNamespace {
        id: "do-coordinator".to_owned(),
        name: "coordinator".to_owned(),
        class: Some("Coordinator".to_owned()),
        script: Some("edge-router".to_owned()),
    };

    let d1 = D1Database {
        uuid: "d1-edge-cache".to_owned(),
        name: "edge-cache".to_owned(),
        version: None,
    };

    let worker_bindings = HashMap::from([(
        worker.clone(),
        vec![
            WorkerBinding {
                name: "SESSIONS".to_owned(),
                binding_type: "kv_namespace".to_owned(),
                namespace_id: Some("kv-sessions".to_owned()),
                bucket_name: None,
                id: None,
                extra: HashMap::new(),
            },
            WorkerBinding {
                name: "MEDIA".to_owned(),
                binding_type: "r2_bucket".to_owned(),
                namespace_id: None,
                bucket_name: Some("r2-media".to_owned()),
                id: None,
                extra: HashMap::new(),
            },
            WorkerBinding {
                name: "COORDINATOR".to_owned(),
                binding_type: "durable_object_namespace".to_owned(),
                namespace_id: Some("do-coordinator".to_owned()),
                bucket_name: None,
                id: None,
                extra: HashMap::new(),
            },
            WorkerBinding {
                name: "EDGE_CACHE".to_owned(),
                binding_type: "d1".to_owned(),
                namespace_id: None,
                bucket_name: None,
                id: Some("d1-edge-cache".to_owned()),
                extra: HashMap::new(),
            },
            WorkerBinding {
                name: "ANALYTICS_DB".to_owned(),
                binding_type: "secret_text".to_owned(),
                namespace_id: None,
                bucket_name: None,
                id: None,
                extra: HashMap::from([(
                    "text".to_owned(),
                    json!("postgres://db.internal.globex.io"),
                )]),
            },
        ],
    )]);

    CloudflareCollection {
        zones: vec![globex_zone()],
        dns_records: HashMap::from([(ZoneId("zone-globex".to_owned()), records)]),
        workers: vec![worker],
        kv_namespaces: vec![kv],
        r2_buckets: vec![r2],
        durable_objects: vec![durable],
        d1_databases: vec![d1],
        worker_bindings,
    }
}
