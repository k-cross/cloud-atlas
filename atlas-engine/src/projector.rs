use crate::Settings;
use atlas_aws::definition::AmazonCollection;
use atlas_aws::projector::aws_projector;
use atlas_azure::definition::MicrosoftCollection;
use atlas_azure::projector::azure_projector;
use atlas_cloudflare::definition::CloudflareCollection;
use atlas_cloudflare::projector::cloudflare_projector;
use atlas_gcp::definition::GoogleCollection;
use atlas_gcp::projector::gcp_projector;
use atlas_graph::graph_builder::GraphBuilder;

#[derive(Debug)]
pub enum Provider {
    AWS(Vec<(String, AmazonCollection)>),
    GCP(Vec<(String, GoogleCollection)>),
    Azure(Vec<MicrosoftCollection>),
    Cloudflare(Box<CloudflareCollection>),
}

pub fn build(builder: &mut GraphBuilder, data: &Provider, opts: &Settings) {
    match data {
        Provider::AWS(aws_data) => aws_projector(builder, aws_data, opts),
        Provider::GCP(gcp_data) => gcp_projector(builder, gcp_data),
        Provider::Azure(azure_data) => azure_projector(builder, azure_data),
        Provider::Cloudflare(cf_data) => cloudflare_projector(builder, cf_data),
    }
}
