use crate::cloud::{AmazonCollection, GoogleCollection, MicrosoftCollection, Provider};
use petgraph::Graph;

fn main() {}

fn project(data: Provider, region: String) {
    match data {
        Provider::AWS(aws_data) => aws_projector(aws_data),
        Provider::GCP(gcp_data) => gcp_projector(gcp_data),
        Provider::Azure(azure_data) => azure_projector(azure_data),
    }
}

fn aws_projector(aws_data: Vec<AmazonCollection>) {
    let graph = Graph::new();
}

fn gcp_projector(gcp_data: Vec<GoogleCollection>) {
    todo!()
}

fn azure_projector(azure_data: Vec<MicrosoftCollection>) {
    todo!()
}
