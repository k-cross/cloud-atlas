use crate::cloud::{AmazonCollection, GoogleCollection, MicrosoftCollection, Provider};
use petgraph::Graph;

pub fn build(data: Provider, region: String) {
    match data {
        Provider::AWS(aws_data) => aws_projector(aws_data),
        Provider::GCP(gcp_data) => gcp_projector(gcp_data),
        Provider::Azure(azure_data) => azure_projector(azure_data),
    }
}

fn aws_projector(aws_data: Vec<AmazonCollection>) -> Graph {
    let graph = Graph::new();

    for x in aws_data {
        match x {
            AmazonCollection::AmazonInstances(instance_data) => {
                for inst in instance_data {
                    let img = graph.add_node(inst.image_id);
                    let pip = graph.add_node(inst.private_ip_address.unwrap());
                    graph.add_edge(img, pip);
                }
            }
            AmazonCollection::AmazonResources(instance_data) => todo!(),
        }
    }

    graph
}

fn gcp_projector(gcp_data: Vec<GoogleCollection>) {
    todo!()
}

fn azure_projector(azure_data: Vec<MicrosoftCollection>) {
    todo!()
}
