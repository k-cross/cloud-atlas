pub mod aws;
pub mod azure;
pub mod cloudflare;
pub mod gcp;

use crate::Settings;
use crate::atlas::graph_builder::GraphBuilder;
use crate::cloud::definition::Provider as CloudProvider;

pub fn build(builder: &mut GraphBuilder, data: &CloudProvider, opts: &Settings) {
    match data {
        CloudProvider::AWS(aws_data) => aws::aws_projector(builder, aws_data, opts),
        CloudProvider::GCP(gcp_data) => gcp::gcp_projector(builder, gcp_data),
        CloudProvider::Azure(azure_data) => azure::azure_projector(builder, azure_data),
        CloudProvider::Cloudflare(cf_data) => cloudflare::cloudflare_projector(builder, cf_data),
    }
}
