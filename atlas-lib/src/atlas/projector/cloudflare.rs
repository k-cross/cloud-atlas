use crate::atlas::definition::{Edge, Node};
use crate::atlas::projector::GraphBuilder;
use crate::cloud::definition::{CloudflareCollection, ZoneId};
use cloudflare::endpoints::dns::dns::DnsContent;

pub fn cloudflare_projector(builder: &mut GraphBuilder, data: &CloudflareCollection) {
    for zone in &data.zones {
        let zone_node = builder.get_or_add_node(Node::CloudflareZone(zone.id.as_str().into()));

        if let Some(records) = data.dns_records.get(&ZoneId(zone.id.clone())) {
            for record in records {
                let record_node = builder.link_to(
                    zone_node,
                    Node::CloudflareDnsRecord(record.id.as_str().into()),
                    Edge::Contains,
                );

                let hostname_node =
                    builder.link_to(record_node, Node::hostname(&record.name), Edge::RoutesTo);

                let target = match &record.content {
                    DnsContent::A { content } => Some(Node::ip(&content.to_string())),
                    DnsContent::AAAA { content } => Some(Node::ip(&content.to_string())),
                    DnsContent::CNAME { content } => Some(Node::hostname(content)),
                    _ => None,
                };
                if let Some(target) = target {
                    builder.link_to(hostname_node, target, Edge::ResolvesTo);
                }
            }
        }
    }

    for kv in &data.kv_namespaces {
        builder.get_or_add_node(Node::CloudflareKvNamespace(kv.id.as_str().into()));
    }

    for r2 in &data.r2_buckets {
        builder.get_or_add_node(Node::CloudflareR2Bucket(r2.name.as_str().into()));
    }

    for dos in &data.durable_objects {
        builder.get_or_add_node(Node::CloudflareDurableObject(dos.id.as_str().into()));
    }

    for d1 in &data.d1_databases {
        builder.get_or_add_node(Node::CloudflareD1Database(d1.uuid.as_str().into()));
    }

    for worker in &data.workers {
        let worker_node =
            builder.get_or_add_node(Node::CloudflareWorker(worker.script.as_str().into()));

        if let Some(bindings) = data.worker_bindings.get(worker) {
            for binding in bindings {
                match binding.binding_type.as_str() {
                    "kv_namespace" => {
                        if let Some(ns_id) = &binding.namespace_id {
                            builder.link_to(
                                worker_node,
                                Node::CloudflareKvNamespace(ns_id.as_str().into()),
                                Edge::ConnectsTo,
                            );
                        }
                    }
                    "r2_bucket" => {
                        if let Some(bucket_name) = &binding.bucket_name {
                            builder.link_to(
                                worker_node,
                                Node::CloudflareR2Bucket(bucket_name.as_str().into()),
                                Edge::ConnectsTo,
                            );
                        }
                    }
                    "durable_object_namespace" => {
                        if let Some(ns_id) = &binding.namespace_id {
                            builder.link_to(
                                worker_node,
                                Node::CloudflareDurableObject(ns_id.as_str().into()),
                                Edge::ConnectsTo,
                            );
                        }
                    }
                    "d1" => {
                        if let Some(db_id) = &binding.id {
                            builder.link_to(
                                worker_node,
                                Node::CloudflareD1Database(db_id.as_str().into()),
                                Edge::ConnectsTo,
                            );
                        }
                    }
                    "secret_text" | "plain_text" => {
                        if let Some(text) = binding.extra.get("text").and_then(|t| t.as_str())
                            && (text.starts_with("postgres://")
                                || text.starts_with("postgresql://")
                                || text.starts_with("mongodb://")
                                || text.starts_with("mysql://"))
                        {
                            if let Ok(url) = url::Url::parse(text)
                                && let Some(host) = url.host_str()
                            {
                                let external_id = format!("{}://{}", url.scheme(), host);
                                builder.link_to(
                                    worker_node,
                                    Node::ExternalService(external_id.into()),
                                    Edge::ConnectsTo,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
