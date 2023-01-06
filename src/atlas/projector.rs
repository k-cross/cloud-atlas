use crate::cloud::definition::{AmazonCollection, GoogleCollection, MicrosoftCollection, Provider};
use petgraph::graphmap::DiGraphMap;

pub fn build<'b>(data: &'b Provider, region: &str) -> DiGraphMap<&'b str, u8> {
    match data {
        Provider::AWS(aws_data) => aws_projector(&aws_data),
        Provider::GCP(gcp_data) => gcp_projector(&gcp_data),
        Provider::Azure(azure_data) => azure_projector(&azure_data),
    }
}

fn aws_projector<'a>(aws_data: &'a Vec<AmazonCollection>) -> DiGraphMap<&'a str, u8> {
    let mut graph = DiGraphMap::new();

    for x in aws_data {
        match x {
            AmazonCollection::AmazonInstances(instance_data) => {
                for inst in instance_data {
                    //dbg!(inst);
                    graph.add_edge(
                        inst.image_id.as_ref().unwrap().as_str(),
                        inst.private_ip_address.as_ref().unwrap().as_str(),
                        0,
                    );
                }
            }
            AmazonCollection::AmazonResources(resource_map) => {
                for (res_name, rs) in resource_map {
                    if use_aws_resource(res_name.as_str()) {
                        for r in rs {
                            graph.add_edge(res_name.as_str(), r.resource_id().unwrap(), 0);
                        }
                    }
                }
            }
        }
    }

    graph
}

fn gcp_projector<'a>(gcp_data: &Vec<GoogleCollection>) -> DiGraphMap<&'a str, u8> {
    todo!()
}

fn azure_projector<'a>(azure_data: &Vec<MicrosoftCollection>) -> DiGraphMap<&'a str, u8> {
    todo!()
}

fn use_aws_resource(name: &str) -> bool {
    // TODO: probably better to invert exceptions instead
    match name {
        "AWS::EC2::Subnet" => true,
        _ => false,
    }
}
