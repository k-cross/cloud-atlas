use crate::atlas::definition::{Edge, Node};
use crate::atlas::projector::GraphBuilder;
use crate::cloud::definition::CloudflareCollection;
use cloudflare::endpoints::dns::dns::DnsContent;

pub fn cloudflare_projector(builder: &mut GraphBuilder, data: &CloudflareCollection) {
    // Project Cloudflare Zones
    for zone in &data.zones {
        let zone_node = builder.get_or_add_node(Node::CloudflareZone(zone.id.as_str().into()));

        // Find DNS records for this zone
        if let Some((_, records)) = data.dns_records.iter().find(|(zid, _)| zid == &zone.id) {
            for record in records {
                let record_node =
                    builder.get_or_add_node(Node::CloudflareDnsRecord(record.id.as_str().into()));

                builder.add_edge(zone_node, record_node, Edge::Contains);

                let hostname_node =
                    builder.get_or_add_node(Node::GenericHostname(record.name.as_str().into()));
                builder.add_edge(
                    record_node,
                    hostname_node,
                    Edge::RoutesTo, // Maps to the generic hostname
                );

                let target = match &record.content {
                    DnsContent::A { content } => {
                        Some(Node::GenericIpAddress(content.to_string().into()))
                    }
                    DnsContent::AAAA { content } => {
                        Some(Node::GenericIpAddress(content.to_string().into()))
                    }
                    DnsContent::CNAME { content } => {
                        Some(Node::GenericHostname(content.as_str().into()))
                    }
                    _ => None,
                };
                if let Some(target) = target {
                    let target_idx = builder.get_or_add_node(target);
                    builder.add_edge(hostname_node, target_idx, Edge::ResolvesTo);
                }
            }
        }
    }

    // Project KV Namespaces
    for kv in &data.kv_namespaces {
        builder.get_or_add_node(Node::CloudflareKvNamespace(kv.id.as_str().into()));
    }

    // Project R2 Buckets
    for r2 in &data.r2_buckets {
        builder.get_or_add_node(Node::CloudflareR2Bucket(r2.name.as_str().into()));
    }

    // Project Durable Object Namespaces
    for dos in &data.durable_objects {
        builder.get_or_add_node(Node::CloudflareDurableObject(dos.id.as_str().into()));
    }

    // Project D1 Databases
    for d1 in &data.d1_databases {
        builder.get_or_add_node(Node::CloudflareD1Database(d1.uuid.as_str().into()));
    }

    // Project Workers and their Bindings
    for worker in &data.workers {
        let worker_node =
            builder.get_or_add_node(Node::CloudflareWorker(worker.id.as_str().into()));

        // Find bindings for this worker
        if let Some((_, bindings)) = data
            .worker_bindings
            .iter()
            .find(|(wid, _)| wid == &worker.id)
        {
            for binding in bindings {
                match binding.binding_type.as_str() {
                    "kv_namespace" => {
                        if let Some(ns_id) = &binding.namespace_id {
                            let kv_node = builder.get_or_add_node(Node::CloudflareKvNamespace(
                                ns_id.as_str().into(),
                            ));
                            builder.add_edge(worker_node, kv_node, Edge::ConnectsTo);
                        }
                    }
                    "r2_bucket" => {
                        if let Some(bucket_name) = &binding.bucket_name {
                            let r2_node = builder.get_or_add_node(Node::CloudflareR2Bucket(
                                bucket_name.as_str().into(),
                            ));
                            builder.add_edge(worker_node, r2_node, Edge::ConnectsTo);
                        }
                    }
                    "durable_object_namespace" => {
                        if let Some(ns_id) = &binding.namespace_id {
                            let do_node = builder.get_or_add_node(Node::CloudflareDurableObject(
                                ns_id.as_str().into(),
                            ));
                            builder.add_edge(worker_node, do_node, Edge::ConnectsTo);
                        }
                    }
                    "d1" => {
                        if let Some(db_id) = &binding.id {
                            let d1_node = builder
                                .get_or_add_node(Node::CloudflareD1Database(db_id.as_str().into()));
                            builder.add_edge(worker_node, d1_node, Edge::ConnectsTo);
                        }
                    }
                    "secret" | "plain_text" => {
                        // Attempt to extract external service connections (e.g. Postgres URIs)
                        if let Some(text) = binding.extra.get("text").and_then(|t| t.as_str())
                            && (text.starts_with("postgres://")
                                || text.starts_with("postgresql://")
                                || text.starts_with("mongodb://")
                                || text.starts_with("mysql://"))
                        {
                            // Extract the host/service part or just use the scheme + host
                            if let Ok(url) = url::Url::parse(text)
                                && let Some(host) = url.host_str()
                            {
                                let external_id = format!("{}://{}", url.scheme(), host);
                                let ext_node = builder
                                    .get_or_add_node(Node::ExternalService(external_id.into()));
                                builder.add_edge(worker_node, ext_node, Edge::ConnectsTo);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
