use crate::projector::{self, Provider};
use atlas_graph::derive;
use atlas_graph::graph_builder::GraphBuilder;
use atlas_provider::fixtures::settings;

pub use atlas_aws::fixtures::{
    AWS_NETWORK, GLOBEX_ZONE, WEB_04, burst_flow, flows, observed, web_01_addresses, web_04_flows,
};
pub use atlas_azure::fixtures::{azure_id, azure_network, azure_networking_id};
pub use atlas_gcp::fixtures::gcp_network;
pub use atlas_provider::fixtures::GCP_PROJECT;

pub fn all() -> Vec<Provider> {
    vec![aws(), gcp(), azure(), cloudflare()]
}

pub fn aws() -> Provider {
    Provider::AWS(atlas_aws::fixtures::collections())
}

pub fn gcp() -> Provider {
    Provider::GCP(atlas_gcp::fixtures::collections())
}

pub fn azure() -> Provider {
    Provider::Azure(atlas_azure::fixtures::collections())
}

pub fn cloudflare() -> Provider {
    Provider::Cloudflare(Box::new(atlas_cloudflare::fixtures::collection()))
}

pub fn with_web_04(provider: Provider) -> Provider {
    let Provider::AWS(collections) = provider else {
        return provider;
    };
    Provider::AWS(atlas_aws::fixtures::with_web_04(collections))
}

pub fn build_graph() -> GraphBuilder {
    build_graph_of(all())
}

pub fn build_graph_of(providers: impl IntoIterator<Item = Provider>) -> GraphBuilder {
    let mut builder = topology_of(providers);
    let flows = observed();
    flows.overlay(&mut builder);
    derive::all(&mut builder, &flows);
    builder
}

pub fn topology() -> GraphBuilder {
    topology_of(all())
}

pub fn topology_of(providers: impl IntoIterator<Item = Provider>) -> GraphBuilder {
    let s = settings();
    let mut builder = GraphBuilder::new();
    for provider in providers {
        projector::build(&mut builder, &provider, &s);
    }
    builder
}
