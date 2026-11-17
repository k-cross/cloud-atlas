use super::*;
use crate::fixtures;
use atlas_graph::testing::{assert_edge, assert_resolves, public};

#[test]
fn cloudflare_projection() {
    let mut builder = GraphBuilder::new();
    cloudflare_projector(&mut builder, &fixtures::collection());

    let zone = Node::CloudflareZone("zone-globex".into());
    let worker = Node::CloudflareWorker("edge-router".into());

    assert_edge(
        &builder,
        &zone,
        &Node::CloudflareDnsRecord("rec-app".into()),
        &Edge::Contains,
    );
    let record = |id: &str| Node::CloudflareDnsRecord(id.into());
    assert_resolves(
        &builder,
        "app.globex.io",
        &record("rec-app"),
        &Node::hostname("app-globex.azurewebsites.net"),
    );
    assert_resolves(
        &builder,
        "data.globex.io",
        &record("rec-data"),
        &Node::hostname("run.globex.app"),
    );
    assert_resolves(
        &builder,
        "db.globex.io",
        &record("rec-db"),
        &public("198.51.100.10"),
    );
    assert_resolves(
        &builder,
        "v6.globex.io",
        &record("rec-v6"),
        &public("2001:db8::10"),
    );

    assert_edge(
        &builder,
        &worker,
        &Node::CloudflareKvNamespace("kv-sessions".into()),
        &Edge::ConnectsTo,
    );
    assert_edge(
        &builder,
        &worker,
        &Node::CloudflareR2Bucket("r2-media".into()),
        &Edge::ConnectsTo,
    );
    assert_edge(
        &builder,
        &worker,
        &Node::CloudflareDurableObject("do-coordinator".into()),
        &Edge::ConnectsTo,
    );
    assert_edge(
        &builder,
        &worker,
        &Node::CloudflareD1Database("d1-edge-cache".into()),
        &Edge::ConnectsTo,
    );
    assert_edge(
        &builder,
        &worker,
        &Node::ExternalService("postgres://db.internal.globex.io".into()),
        &Edge::ConnectsTo,
    );
}
