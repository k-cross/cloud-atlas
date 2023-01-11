use crate::cloud::definition::{AmazonCollection, GoogleCollection, MicrosoftCollection, Provider};
use crate::Opt;
use petgraph::graphmap::DiGraphMap;

pub fn build<'b>(data: &'b Provider, opts: &'b Opt) -> DiGraphMap<&'b str, u8> {
    match data {
        Provider::AWS(aws_data) => aws_projector(&aws_data, opts),
        Provider::GCP(gcp_data) => gcp_projector(&gcp_data),
        Provider::Azure(azure_data) => azure_projector(&azure_data),
    }
}

fn aws_projector<'a>(
    aws_data: &'a Vec<AmazonCollection>,
    opts: &'a Opt,
) -> DiGraphMap<&'a str, u8> {
    let mut graph = DiGraphMap::new();
    let region = opts.region.as_str();

    // TODO: start building mappings by ARN

    for x in aws_data {
        match x {
            AmazonCollection::AmazonInstances(instance_data) => {
                for inst in instance_data {
                    // add main edges
                    graph.add_edge(
                        inst.image_id.as_ref().unwrap().as_str(),
                        inst.private_ip_address.as_ref().unwrap().as_str(),
                        0,
                    );

                    graph.add_edge(
                        inst.image_id.as_ref().unwrap().as_str(),
                        inst.instance_id.as_ref().unwrap().as_str(),
                        0,
                    );

                    graph.add_edge(
                        inst.image_id.as_ref().unwrap().as_str(),
                        inst.vpc_id.as_ref().unwrap().as_str(),
                        0,
                    );

                    graph.add_edge(
                        inst.image_id.as_ref().unwrap().as_str(),
                        inst.subnet_id.as_ref().unwrap().as_str(),
                        0,
                    );

                    // get AZ info
                    match inst.placement.as_ref() {
                        Some(place) => {
                            graph.add_edge(
                                place.availability_zone.as_ref().unwrap().as_str(),
                                inst.image_id.as_ref().unwrap().as_str(),
                                0,
                            );
                        }
                        None => (),
                    }

                    // add tags if they exist
                    match inst.tags.as_ref() {
                        Some(tags) => {
                            for tag in tags {
                                graph.add_edge(
                                    tag.key.as_ref().unwrap().as_ref(),
                                    tag.value.as_ref().unwrap().as_ref(),
                                    0,
                                );
                                graph.add_edge(
                                    inst.image_id.as_ref().unwrap().as_str(),
                                    tag.key.as_ref().unwrap().as_ref(),
                                    0,
                                );
                            }
                        }
                        None => (),
                    }

                    // add region info
                    if opts.all {
                        graph.add_edge(region, inst.image_id.as_ref().unwrap().as_str(), 0);
                    }
                }
            }
            AmazonCollection::AmazonResources(resource_map) => {
                for (res_name, rs) in resource_map {
                    if use_aws_resource(res_name.as_str()) {
                        for r in rs {
                            // add main edges
                            graph.add_edge(res_name.as_str(), r.resource_id().unwrap(), 0);

                            // add region edges
                            if opts.all {
                                graph.add_edge(region, r.resource_id().unwrap(), 0);
                            }
                        }
                        if opts.all {
                            graph.add_edge(region, res_name.as_str(), 0);
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
    match name {
        "AWS::CodeDeploy::DeploymentConfig" => false,
        _ => true,
    }
}
